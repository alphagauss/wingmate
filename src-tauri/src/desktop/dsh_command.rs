#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(windows)]
mod windows;

use anyhow::{Context, Result};
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::Path,
};
use wingmate_service::service::{self, Snapshot};

pub(super) const SUPPORTED: bool =
    cfg!(any(windows, target_os = "linux", target_os = "macos")) && !wingmate_service::IS_DEV;
const OWNER: &str = "// Wingmate dsh command\n";
const LAUNCHER: &str = include_str!("dsh_command/launch.cjs");

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Status {
    enabled: bool,
    needs_update: bool,
}

pub(super) fn configure(home: &Path, state: Snapshot, enabled: Option<bool>) -> Result<Status> {
    anyhow::ensure!(SUPPORTED, "dsh 命令仅适用于支持此功能的发行版");
    let _operation = (enabled == Some(true))
        .then(|| service::operation_lock(home))
        .transpose()?;
    let launcher = || {
        let state = if enabled == Some(true) {
            service::status(home)?
                .filter(|current| current.instance == state.instance && current.phase == "running")
                .context("后台实例已更换，请重新连接")?
        } else {
            state
        };
        let entry = service::dsh_entry(home, &state)?;
        let dsh_home = state.dsh_home.context("后台未提供 DSH 数据目录")?;
        let agents_home = state
            .agents_home
            .context("后台未提供 Agent 数据目录，请用当前版本重启后台后再注册")?;
        launcher(&entry, &dsh_home, &agents_home)
    };
    #[cfg(windows)]
    return windows::configure(enabled, launcher);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return unix::configure(enabled, launcher);
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = launcher;
        anyhow::bail!("此平台不支持注册 dsh 命令")
    }
}

fn launcher(entry: &Path, dsh_home: &Path, agents_home: &Path) -> Result<String> {
    let config = serde_json::to_string(&(entry, dsh_home, agents_home))?;
    Ok(format!(
        "{OWNER}const [entry, dshHome, agentsHome] = {config};\n{LAUNCHER}"
    ))
}

fn registration_lock(bin: &Path, enabled: Option<bool>) -> Result<Option<File>> {
    if enabled.is_none() {
        return Ok(None);
    }
    fs::create_dir_all(bin)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(bin.join("registration.lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    Ok(Some(lock))
}

fn read(path: &Path) -> Result<Option<String>> {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.is_file() => Ok(Some(fs::read_to_string(path)?)),
        Ok(_) => anyhow::bail!("命令文件已被占用：{}", path.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
