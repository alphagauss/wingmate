use crate::{components, service::bootstrap_lock};
#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::Result;
#[cfg(not(target_os = "linux"))]
use std::process::Stdio;
use std::{
    io::{self, Write},
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

pub fn open_desktop(home: &Path) -> Result<()> {
    let _bootstrap = bootstrap_lock(home)?;
    if components::desktop_reopen_requested(home)? {
        return Ok(());
    }
    let prepared = components::prepare(home, true)?;
    if !components::desktop_pids(home)?.is_empty() && prepared.replaces_file() {
        return spawn_desktop(home, &prepared.binary, Some("update"));
    }
    spawn_desktop(home, &prepared.publish()?, None)
}

pub fn reopen_desktop(home: &Path) -> Result<()> {
    let bootstrap = bootstrap_lock(home)?;
    anyhow::ensure!(!components::desktop_reopen_requested(home)?, "桌面正在重开");
    let prepared = components::prepare(home, true)?;
    #[cfg(windows)]
    let processes = components::desktop_pids(home)?
        .into_iter()
        .map(crate::process_wait::open)
        .collect::<Result<Vec<_>>>()?;
    let reopen = components::desktop_reopen_lock(home)?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "ready")?;
    stdout.flush()?;
    drop(stdout);
    // 已在收尾的窗口仍可调用发行入口，避免持锁等待它们退出。
    drop(bootstrap);

    // 后台 RPC 最长可等待 180 秒，桌面会先等已有操作收尾。
    let deadline = Instant::now() + Duration::from_secs(210);
    #[cfg(windows)]
    for process in processes.iter().flatten() {
        crate::process_wait::wait(process, deadline, "等待桌面窗口退出超时")?;
    }
    #[cfg(not(windows))]
    while !components::desktop_pids(home)?.is_empty() {
        anyhow::ensure!(Instant::now() < deadline, "等待桌面窗口退出超时");
        thread::sleep(Duration::from_millis(50));
    }

    let _bootstrap = bootstrap_lock(home)?;
    let binary = prepared.publish()?;
    drop(reopen);
    spawn_desktop(home, &binary, Some("reopen"))
}

fn spawn_desktop(home: &Path, binary: &Path, action: Option<&str>) -> Result<()> {
    let mut command = Command::new(binary);
    command
        .env("WINGMATE_LAUNCHER", std::env::current_exe()?)
        .env("WINGMATE_HOME", home)
        .env("WINGMATE_SERVICE_REVISION", components::service_revision()?)
        .env(
            "WINGMATE_TARGET_RUNTIME_REVISION",
            crate::RUNTIME_REVISION.unwrap_or_default(),
        )
        .env_remove("WINGMATE_DESKTOP_ACTION");
    if let Some(action) = action {
        command.env("WINGMATE_DESKTOP_ACTION", action);
    }
    #[cfg(target_os = "linux")]
    let log_path = {
        use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};

        let path = home.join("desktop.log");
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&path)
            .with_context(|| format!("无法打开桌面日志 {}", path.display()))?;
        command.stdout(log.try_clone()?).stderr(log);
        path
    };
    #[cfg(not(target_os = "linux"))]
    if action == Some("reopen") {
        command.stdout(Stdio::null());
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let result = (|| -> Result<()> {
        while !components::desktop_ready(binary, child.id())? {
            anyhow::ensure!(child.try_wait()?.is_none(), "桌面进程启动失败");
            anyhow::ensure!(Instant::now() < deadline, "桌面组件登记超时");
            thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    #[cfg(target_os = "linux")]
    let result = result.with_context(|| format!("请检查桌面日志 {}", log_path.display()));
    result
}
