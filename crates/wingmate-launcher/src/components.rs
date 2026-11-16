//! 发行可执行文件的校验和发布。调用方持有 bootstrap.lock。
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use wingmate_service::host;
include!(concat!(env!("OUT_DIR"), "/binaries.rs"));

pub fn service_revision() -> Result<String> {
    if let Some((_, hash)) = SERVICE_ARCHIVE {
        Ok(hash.to_owned())
    } else {
        host::revision(
            &Path::new(BINARY_DIRECTORY)
                .join(format!("wingmate-service{}", std::env::consts::EXE_SUFFIX)),
        )
        .context("管理服务尚未构建，请重新准备 Wingmate")
    }
}

pub(crate) struct PreparedBinary {
    pub revision: String,
    pub binary: PathBuf,
    replacement: Option<NamedTempFile>,
}

impl PreparedBinary {
    #[cfg(feature = "desktop")]
    pub fn replaces_file(&self) -> bool {
        self.replacement.is_some()
    }

    pub fn publish(self) -> Result<PathBuf> {
        if let Some(file) = self.replacement {
            file.persist(&self.binary)
                .map_err(|error| error.error)
                .with_context(|| format!("无法替换 {}", self.binary.display()))?;
        }
        Ok(self.binary)
    }
}

pub(crate) fn prepare(home: &Path, desktop: bool) -> Result<PreparedBinary> {
    let (name, embedded, parent) = if desktop {
        ("wingmate-desktop", DESKTOP_ARCHIVE, "desktops")
    } else {
        ("wingmate-service", SERVICE_ARCHIVE, "services")
    };
    let filename = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let local_bytes;
    let (bytes, revision) = if let Some((compressed, hash)) = embedded {
        (compressed, hash.to_owned())
    } else {
        local_bytes = fs::read(Path::new(BINARY_DIRECTORY).join(&filename))
            .context("组件尚未构建，请重新准备 Wingmate")?;
        let hash = format!("{:x}", Sha256::digest(&local_bytes));
        (local_bytes.as_slice(), hash)
    };
    let parent = home.join(parent);
    fs::create_dir_all(&parent)?;
    anyhow::ensure!(
        parent.symlink_metadata()?.is_dir(),
        "组件根目录不是普通目录"
    );
    let binary = parent.join(filename);
    let mut prepared = PreparedBinary {
        binary,
        revision,
        replacement: None,
    };
    match prepared.binary.symlink_metadata() {
        Ok(metadata) => {
            anyhow::ensure!(metadata.is_file(), "组件路径不是普通文件");
            if host::revision(&prepared.binary)? == prepared.revision {
                return Ok(prepared);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let mut stage = NamedTempFile::new_in(&parent)?;
    if embedded.is_some() {
        zstd::stream::copy_decode(bytes, stage.as_file_mut())
            .with_context(|| format!("解压 {name} 失败"))?;
    } else {
        stage.write_all(bytes)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        stage
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    anyhow::ensure!(
        host::revision(stage.path())? == prepared.revision,
        "待安装的 {name} 指纹不匹配"
    );
    prepared.replacement = Some(stage);
    Ok(prepared)
}

fn lease_path(directory: &Path, pid: u32) -> PathBuf {
    directory.join(format!("{pid}.lock"))
}

pub fn desktop_lease(home: &Path) -> Result<Option<File>> {
    let binary = std::env::current_exe()?;
    let Some(directory) = binary.parent() else {
        return Ok(None);
    };
    let Ok(root) = home.join("desktops").canonicalize() else {
        return Ok(None);
    };
    if directory.canonicalize()? != root {
        return Ok(None);
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lease_path(directory, std::process::id()))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    Ok(Some(lock))
}

#[cfg(feature = "desktop")]
pub(crate) fn desktop_pids(home: &Path) -> Result<Vec<u32>> {
    let mut pids = Vec::new();
    for entry in fs::read_dir(home.join("desktops"))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|value| value != "lock") || !entry.file_type()?.is_file() {
            continue;
        }
        if let Some(pid) = path
            .file_stem()
            .and_then(|value| value.to_str())
            .and_then(|value| value.parse::<u32>().ok())
        {
            if locked(&path)? {
                pids.push(pid);
            } else {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(pids)
}

pub fn desktop_reopen_requested(home: &Path) -> Result<bool> {
    locked(&home.join("desktops/reopen.lock"))
}

#[cfg(feature = "desktop")]
pub(crate) fn desktop_reopen_lock(home: &Path) -> Result<File> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.join("desktops/reopen.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("桌面正在重开")?;
    Ok(lock)
}

#[cfg(feature = "desktop")]
pub(crate) fn desktop_ready(binary: &Path, pid: u32) -> Result<bool> {
    let directory = binary.parent().context("桌面组件目录无效")?;
    locked(&lease_path(directory, pid))
}

fn locked(path: &Path) -> Result<bool> {
    let file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    match fs2::FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(false),
        Err(error)
            if error.kind() == io::ErrorKind::WouldBlock
                || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
        {
            Ok(true)
        }
        Err(error) => Err(error.into()),
    }
}
