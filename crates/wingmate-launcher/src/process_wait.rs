use anyhow::{Context, Result};
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_PARAMETER, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
};

pub(crate) fn open(pid: u32) -> Result<Option<OwnedHandle>> {
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            return Ok(None);
        }
        return Err(error).with_context(|| format!("无法打开进程 {pid}"));
    }
    Ok(Some(unsafe { OwnedHandle::from_raw_handle(handle) }))
}

pub(crate) fn wait(process: &OwnedHandle, deadline: Instant, timeout: &str) -> Result<()> {
    let millis = deadline
        .saturating_duration_since(Instant::now())
        .as_millis() as u32;
    match unsafe { WaitForSingleObject(process.as_raw_handle(), millis) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => anyhow::bail!("{timeout}"),
        _ => Err(io::Error::last_os_error()).context("等待进程退出失败"),
    }
}
