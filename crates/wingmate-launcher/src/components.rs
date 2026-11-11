//! 发行组件的释放；常驻进程一律从管理目录的副本运行。
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
include!(concat!(env!("OUT_DIR"), "/binaries.rs"));

const SHORT_HASH: usize = 6;

pub fn service_revision() -> Result<String> {
    if SERVICE_BYTES.is_empty() {
        wingmate_service::host::revision(
            &Path::new(BINARY_DIRECTORY)
                .join(format!("wingmate-service{}", std::env::consts::EXE_SUFFIX)),
        )
        .context("管理服务尚未构建，请重新准备 Wingmate")
    } else {
        Ok(format!("{:x}", Sha256::digest(SERVICE_BYTES)))
    }
}

pub fn install(home: &Path, desktop: bool) -> Result<PathBuf> {
    let (name, embedded, parent) = if desktop {
        ("wingmate-desktop", DESKTOP_BYTES, "desktops")
    } else {
        ("wingmate-service", SERVICE_BYTES, "services")
    };
    let filename = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let bytes = if embedded.is_empty() {
        fs::read(Path::new(BINARY_DIRECTORY).join(&filename))
            .context("组件尚未构建，请重新准备 Wingmate")?
    } else {
        embedded.to_vec()
    };
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let parent = home.join(parent);
    fs::create_dir_all(&parent)?;
    anyhow::ensure!(
        parent.symlink_metadata()?.is_dir(),
        "组件根目录不是普通目录"
    );
    let directory = parent.join(&hash[..SHORT_HASH]);
    let binary = directory.join(&filename);
    if directory.exists() {
        anyhow::ensure!(
            directory.symlink_metadata()?.is_dir()
                && fs::read_to_string(directory.join(".wingmate-component"))? == name,
            "无法确认组件目录归属"
        );
        anyhow::ensure!(
            binary.is_file() && wingmate_service::host::revision(&binary)? == hash,
            "组件指纹不匹配，请停止对应进程并移走 {} 后重试",
            directory.display()
        );
        return Ok(binary);
    }
    let stage = tempfile::tempdir_in(&parent)?;
    let mut file = fs::File::create(stage.path().join(&filename))?;
    file.write_all(&bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    drop(file);
    fs::write(stage.path().join(".wingmate-component"), name)?;
    fs::rename(stage.path(), directory)?;
    Ok(binary)
}

pub(crate) fn cleanup_services(home: &Path, revision: &str) -> Result<()> {
    cleanup(
        home,
        &home.join("services").join(&revision[..SHORT_HASH]),
        "services",
        "wingmate-service",
        |_| Ok(false),
    )
}

#[cfg(feature = "desktop")]
pub(crate) fn cleanup_desktops(home: &Path, active: &Path) -> Result<()> {
    let keep = active.parent().context("桌面组件目录无效")?;
    cleanup(home, keep, "desktops", "wingmate-desktop", |directory| {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && entry.path().extension().and_then(|value| value.to_str()) == Some("lock")
                && locked(&entry.path())?
            {
                return Ok(true);
            }
        }
        Ok(false)
    })
}

fn cleanup(
    home: &Path,
    keep: &Path,
    parent: &str,
    component: &str,
    in_use: impl Fn(&Path) -> Result<bool>,
) -> Result<()> {
    let parent = home.join(parent);
    if !parent.exists() {
        return Ok(());
    }
    anyhow::ensure!(
        parent.symlink_metadata()?.is_dir(),
        "组件根目录不是普通目录"
    );
    for entry in fs::read_dir(&parent)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if entry.path() != keep
            && entry.file_type()?.is_dir()
            && name.len() == SHORT_HASH
            && name.bytes().all(|b| b.is_ascii_hexdigit())
            && fs::read_to_string(entry.path().join(".wingmate-component"))
                .is_ok_and(|value| value == component)
            && !in_use(&entry.path())?
        {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

fn lease_path(binary: &Path, pid: u32) -> Result<PathBuf> {
    Ok(binary
        .parent()
        .context("桌面组件目录无效")?
        .join(format!("{pid}.lock")))
}

pub fn desktop_lease(home: &Path) -> Result<Option<File>> {
    let binary = std::env::current_exe()?;
    let Some(directory) = binary.parent() else {
        return Ok(None);
    };
    let Ok(root) = home.join("desktops").canonicalize() else {
        return Ok(None);
    };
    if directory.canonicalize()?.parent() != Some(&root) {
        return Ok(None);
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lease_path(&binary, std::process::id())?)?;
    fs2::FileExt::lock_exclusive(&lock)?;
    anyhow::ensure!(
        directory.join(".wingmate-component").is_file(),
        "桌面组件已被回收，请重新打开 Wingmate"
    );
    Ok(Some(lock))
}

#[cfg(feature = "desktop")]
pub(crate) fn desktop_ready(binary: &Path, pid: u32) -> Result<bool> {
    locked(&lease_path(binary, pid)?)
}

#[cfg(feature = "desktop")]
fn locked(path: &Path) -> Result<bool> {
    let file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    match fs2::FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(false),
        Err(error)
            if error.kind() == std::io::ErrorKind::WouldBlock
                || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
        {
            Ok(true)
        }
        Err(error) => Err(error.into()),
    }
}
