//! 在启动器退出前把运行锁和 Windows 进程树的所有权交给 DSH。
use anyhow::{Context, Result};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    process::{Child, Command, Stdio},
};

pub fn ready_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url
            .query_pairs()
            .any(|(k, v)| k == "token" && !v.is_empty())
}

pub fn try_acquire(home: &Path) -> Result<Option<File>> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;

        match options.share_mode(0).open(home.join("service.lock")) {
            Ok(file) => Ok(Some(file)),
            Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    #[cfg(unix)]
    {
        let file = options.open(home.join("service.lock"))?;
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => Ok(Some(file)),
            Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }
}

pub fn occupied(home: &Path) -> Result<bool> {
    Ok(try_acquire(home)?.is_none())
}

pub fn start(home: &Path, runtime: &Path, lock: File) -> Result<(Child, tempfile::NamedTempFile)> {
    // 控制入口随 EXE 更新，已经释放的同版本 DSH 目录可以继续复用。
    let mut control = tempfile::Builder::new().suffix(".mjs").tempfile_in(home)?;
    control.write_all(include_bytes!("../../../runtime/control.mjs"))?;
    let control_url = reqwest::Url::from_file_path(control.path())
        .map_err(|_| anyhow::anyhow!("无法转换控制模块路径"))?;
    let log = File::create(home.join("service.log"))?;
    let mut command = Command::new(crate::runtime::node()?);
    crate::runtime::quiet(&mut command);
    #[cfg(unix)]
    {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        let fd = lock.as_raw_fd();
        // flock 随打开的文件描述保留。仅在 Node 子进程中清除 close-on-exec。
        unsafe {
            command.process_group(0).pre_exec(move || {
                if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    command
        .arg("--import")
        .arg(control_url.as_str())
        .arg(runtime.join(crate::runtime::DSH_ENTRY))
        .args(["web", "--host", "127.0.0.1", "--port", "0", "--no-open"])
        .env("WINGMATE_HOME", home)
        .env("WINGMATE_RUNTIME_ID", crate::runtime::ID)
        .env_remove("NODE_OPTIONS")
        .stdin(Stdio::piped())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    #[cfg(not(feature = "bundled-runtime"))]
    command
        .env("DSH_HOME", crate::runtime::dev_root().join("dsh"))
        .env("DSH_AGENTS_HOME", crate::runtime::dev_root().join("agents"));
    #[cfg(windows)]
    prevent_standard_pipe_inheritance()?;
    let mut child = command.spawn().context("无法启动 DSH")?;
    let handoff = (|| -> Result<()> {
        #[cfg(windows)]
        give_ownership(&child, &lock)?;
        // Node 在收到完整交接指令前不会启动 DSH。启动器提前退出时，管道 EOF 使 Node 退出。
        child.stdin.take().unwrap().write_all(b"start\n")?;
        Ok(())
    })();
    if let Err(error) = handoff {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    drop(lock);
    Ok((child, control))
}

#[cfg(windows)]
fn prevent_standard_pipe_inheritance() -> Result<()> {
    use windows_sys::Win32::{
        Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT},
        Storage::FileSystem::{GetFileType, FILE_TYPE_PIPE},
        System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
    };

    // Windows 创建进程时也会继承调用方已有的可继承管道句柄，导致 CLI 输出一直无法结束。
    for stream in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        unsafe {
            let handle = GetStdHandle(stream);
            if GetFileType(handle) == FILE_TYPE_PIPE
                && SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) == 0
            {
                return Err(std::io::Error::last_os_error()).context("无法隔离启动器的标准管道");
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn give_ownership(child: &Child, lock: &File) -> Result<()> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS},
        System::{JobObjects::*, Threading::GetCurrentProcess},
    };

    unsafe {
        let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if handle.is_null() {
            return Err(std::io::Error::last_os_error()).context("创建进程 Job 失败");
        }
        let job = OwnedHandle::from_raw_handle(handle);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            std::mem::size_of_val(&limits) as u32,
        ) == 0
        {
            return Err(std::io::Error::last_os_error()).context("设置进程 Job 失败");
        }
        if AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) == 0 {
            return Err(std::io::Error::last_os_error()).context("绑定进程 Job 失败");
        }
        // 复制到 Node 的句柄不可继承，Node 退出时系统会释放运行锁并回收 Job 内的进程。
        for handle in [lock.as_raw_handle(), job.as_raw_handle()] {
            let mut remote = std::ptr::null_mut();
            if DuplicateHandle(
                GetCurrentProcess(),
                handle,
                child.as_raw_handle(),
                &mut remote,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            ) == 0
            {
                return Err(std::io::Error::last_os_error()).context("交接后台进程所有权失败");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_accepts_owned_loopback_readiness() {
        assert!(ready_url("http://127.0.0.1:1234/?token=abc"));
        for url in [
            "http://example.com:1234/?token=abc",
            "http://127.0.0.1:1234/",
            "http://user@127.0.0.1:1234/?token=abc",
            "other: http://127.0.0.1:1234/?token=abc",
        ] {
            assert!(!ready_url(url));
        }
    }

    #[cfg(windows)]
    #[test]
    fn node_exit_releases_ownership_and_kills_its_child() -> Result<()> {
        use std::{
            fs,
            os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
            thread,
            time::{Duration, Instant},
        };
        use windows_sys::Win32::{
            Foundation::WAIT_OBJECT_0,
            System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
        };

        let home = tempfile::tempdir()?;
        let bin = home.path().join("node_modules/@deepseek-ai/dsh/lib");
        fs::create_dir_all(&bin)?;
        fs::write(
            bin.join("bin.js"),
            r#"
const { spawn } = require('node:child_process');
const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
const marker = require('node:path').join(process.env.WINGMATE_HOME, 'child.pid');
const fs = require('node:fs');
fs.writeFileSync(marker + '.tmp', String(child.pid));
fs.renameSync(marker + '.tmp', marker);
"#,
        )?;
        let lock = try_acquire(home.path())?.context("无法取得测试运行锁")?;
        let (mut node, _control) = start(home.path(), home.path(), lock)?;
        let result = (|| -> Result<()> {
            let marker = home.path().join("child.pid");
            let deadline = Instant::now() + Duration::from_secs(10);
            while !marker.is_file() {
                anyhow::ensure!(
                    node.try_wait()?.is_none() && Instant::now() < deadline,
                    "测试子进程未启动"
                );
                thread::sleep(Duration::from_millis(50));
            }
            let pid = fs::read_to_string(marker)?.parse()?;
            let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
            anyhow::ensure!(!handle.is_null(), "无法打开测试子进程");
            let child = unsafe { OwnedHandle::from_raw_handle(handle) };
            assert!(occupied(home.path())?);
            node.kill()?;
            node.wait()?;
            assert_eq!(
                unsafe { WaitForSingleObject(child.as_raw_handle(), 5000) },
                WAIT_OBJECT_0
            );
            assert!(!occupied(home.path())?);
            Ok(())
        })();
        if result.is_err() {
            let _ = node.kill();
            let _ = node.wait();
        }
        result
    }
}
