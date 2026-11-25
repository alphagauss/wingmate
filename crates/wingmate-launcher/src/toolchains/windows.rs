use super::{ToolchainBusy, ToolchainProcess};
use crate::service::ToolchainApproval;
use anyhow::{Context, Result};
use std::{
    ffi::OsString,
    io,
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStringExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{ERROR_NO_MORE_FILES, FILETIME, HANDLE, INVALID_HANDLE_VALUE},
    Security::{EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        Threading::{
            GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
            QueryFullProcessImageNameW, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        },
    },
};

fn token_user(process: HANDLE) -> Result<Vec<usize>> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut bytes = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut bytes,
        )
    };
    anyhow::ensure!(
        bytes as usize >= size_of::<TOKEN_USER>(),
        "无法读取进程用户身份"
    );
    let mut buffer = vec![0_usize; (bytes as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(buffer)
}

fn identity(
    process: &OwnedHandle,
    pid: u32,
    expected: &Path,
    current_user: &[usize],
) -> Result<Option<ToolchainProcess>> {
    let handle = process.as_raw_handle();
    let mut image = vec![0_u16; 32768];
    let mut count = image.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle, 0, image.as_mut_ptr(), &mut count) } == 0 {
        return Ok(None);
    }
    let image = PathBuf::from(OsString::from_wide(&image[..count as usize]));
    let Ok(image) = image.canonicalize() else {
        return Ok(None);
    };
    if !image.as_os_str().eq_ignore_ascii_case(expected.as_os_str()) {
        return Ok(None);
    }
    let owner = token_user(handle)?;
    let user = |token: &[usize]| unsafe { (*token.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    if unsafe { EqualSid(user(current_user), user(&owner)) } == 0 {
        return Ok(None);
    }
    let mut created: FILETIME = unsafe { zeroed() };
    let mut exit: FILETIME = unsafe { zeroed() };
    let mut kernel: FILETIME = unsafe { zeroed() };
    let mut user_time: FILETIME = unsafe { zeroed() };
    if unsafe { GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user_time) } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(Some(ToolchainProcess {
        pid,
        // FILETIME exceeds the integer precision of JavaScript in the confirmation UI.
        started: format!(
            "{:08x}{:08x}",
            created.dwHighDateTime, created.dwLowDateTime
        ),
    }))
}

pub(super) fn release(node: &Path, approval: ToolchainApproval<'_>) -> Result<()> {
    if !node.is_file() {
        return Ok(());
    }
    let expected = node.canonicalize()?;
    let current_user = token_user(unsafe { GetCurrentProcess() })?;
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error()).context("无法枚举进程");
    }
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let mut entry: PROCESSENTRY32W = unsafe { zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut status = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) };
    let mut blockers = Vec::new();
    while status != 0 {
        let pid = entry.th32ProcessID;
        let name = String::from_utf16_lossy(
            &entry.szExeFile[..entry
                .szExeFile
                .iter()
                .position(|&value| value == 0)
                .unwrap_or(entry.szExeFile.len())],
        );
        if name.eq_ignore_ascii_case("node.exe") {
            let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if !handle.is_null() {
                let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
                if let Some(process) = identity(&handle, pid, &expected, &current_user)? {
                    blockers.push(process);
                }
            }
        }
        status = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) };
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err(error).context("无法完成进程枚举");
    }
    if blockers.iter().any(|process| !approval.allows(process)) {
        return Err(ToolchainBusy(blockers).into());
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    for expected_process in blockers {
        let pid = expected_process.pid;
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error()
                == Some(windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER as i32)
            {
                continue;
            }
            return Err(error).with_context(|| format!("无法打开占用工具目录的 Node 进程 {pid}"));
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        if identity(&handle, pid, &expected, &current_user)?.as_ref() != Some(&expected_process) {
            continue;
        }
        if unsafe { TerminateProcess(handle.as_raw_handle(), 1) } == 0 {
            crate::process_wait::wait(&handle, Instant::now(), "Node 进程仍在运行")
                .with_context(|| format!("无法结束 PID {pid}"))?;
        }
        crate::process_wait::wait(&handle, deadline, "随包 Node 尚未退出")?;
    }
    Ok(())
}
