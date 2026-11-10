#[cfg(any(target_os = "macos", target_os = "linux"))]
mod unix;
#[cfg(windows)]
mod windows;

pub(super) const SUPPORTED: bool =
    cfg!(any(windows, target_os = "macos", target_os = "linux")) && !wingmate_service::IS_DEV;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Status {
    registered: bool,
    enabled: bool,
    needs_update: bool,
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn optional<T>(result: std::io::Result<T>) -> std::io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn configure(enabled: Option<bool>) -> anyhow::Result<Status> {
    anyhow::ensure!(SUPPORTED, "登录启动仅适用于支持此功能的发行版");
    #[cfg(windows)]
    return windows::configure(&std::env::current_exe()?, enabled);
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    return unix::configure(&std::env::current_exe()?, enabled);
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = enabled;
        anyhow::bail!("此平台不支持登录启动")
    }
}
