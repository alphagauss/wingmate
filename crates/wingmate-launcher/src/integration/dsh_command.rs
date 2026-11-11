#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(windows)]
mod windows;

use crate::service::{self, Snapshot};
use anyhow::{Context, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const SUPPORTED: bool =
    cfg!(any(windows, target_os = "linux", target_os = "macos")) && !crate::IS_DEV;
const OWNER: &str = "// Wingmate dsh command\n";
const LAUNCHER: &str = include_str!("dsh_command/launch.cjs");
const WINGMATE_OWNER: &str = "// Wingmate desktop command\n";

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    enabled: bool,
    needs_update: bool,
}

pub fn configure(home: &Path, state: Snapshot, enabled: Option<bool>) -> Result<Status> {
    anyhow::ensure!(SUPPORTED, "终端命令注册仅适用于支持此功能的发行版");
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
    let wingmate = wingmate_launcher(&super::executable()?, home)?;
    #[cfg(windows)]
    return windows::configure(enabled, launcher, &wingmate);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return unix::configure(enabled, launcher, &wingmate);
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (launcher, wingmate);
        anyhow::bail!("此平台不支持注册终端命令")
    }
}

fn launcher(entry: &Path, dsh_home: &Path, agents_home: &Path) -> Result<String> {
    let config = serde_json::to_string(&(entry, dsh_home, agents_home))?;
    Ok(format!(
        "{OWNER}const [entry, dshHome, agentsHome] = {config};\n{LAUNCHER}"
    ))
}

fn wingmate_launcher(executable: &Path, home: &Path) -> Result<String> {
    let config = serde_json::to_string(&(executable, home))?;
    Ok(format!(
        "{WINGMATE_OWNER}const [executable, home] = {config};\n{}",
        include_str!("dsh_command/wingmate.cjs")
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

struct CommandFile {
    path: PathBuf,
    occupied: bool,
    current: Option<String>,
}

impl CommandFile {
    fn open(bin: &Path, name: &str, owned: impl FnOnce(&str) -> bool) -> Result<Self> {
        let path = bin.join(name);
        let (exists, current) = match path.symlink_metadata() {
            Ok(metadata) if metadata.is_file() => match fs::read_to_string(&path) {
                Ok(text) => (true, Some(text).filter(|text| owned(text))),
                Err(error) if error.kind() == io::ErrorKind::InvalidData => (true, None),
                Err(error) if error.kind() == io::ErrorKind::NotFound => (false, None),
                Err(error) => return Err(error.into()),
            },
            Ok(_) => (true, None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (false, None),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            occupied: exists && current.is_none(),
            current,
        })
    }

    fn ensure_available(&self) -> Result<()> {
        anyhow::ensure!(
            !self.occupied,
            "命令目录已有同名文件：{}",
            self.path.display()
        );
        Ok(())
    }

    fn install(&self, content: &str) -> Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(self.path.parent().unwrap())?;
        file.write_all(content.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.as_file()
                .set_permissions(fs::Permissions::from_mode(0o755))?;
        }
        if self.current.is_some() {
            file.persist(&self.path)?;
        } else {
            file.persist_noclobber(&self.path)?;
        }
        Ok(())
    }

    fn remove(&self) -> Result<()> {
        if self.current.is_some() {
            fs::remove_file(&self.path)?;
        }
        Ok(())
    }
}
