//! 发行组件的释放；常驻进程一律从管理目录的副本运行。
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
include!(concat!(env!("OUT_DIR"), "/binaries.rs"));

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
    let directory = parent.join(&hash);
    let binary = directory.join(&filename);
    if directory.exists() {
        anyhow::ensure!(
            directory.symlink_metadata()?.is_dir()
                && fs::read_to_string(directory.join(".wingmate-component"))? == name,
            "无法确认组件目录归属"
        );
        anyhow::ensure!(
            binary.is_file() && wingmate_service::host::revision(&binary)? == hash,
            "组件已损坏，请停止对应进程并移走 {} 后重试",
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

pub fn cleanup_services(home: &Path, active: &Path) -> Result<()> {
    let parent = home.join("services");
    if !parent.exists() {
        return Ok(());
    }
    anyhow::ensure!(
        parent.symlink_metadata()?.is_dir(),
        "服务根目录不是普通目录"
    );
    let keep = active.parent().context("服务目录无效")?;
    for entry in fs::read_dir(&parent)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if entry.path() != keep
            && entry.file_type()?.is_dir()
            && name.len() == 64
            && name.bytes().all(|b| b.is_ascii_hexdigit())
            && fs::read_to_string(entry.path().join(".wingmate-component"))
                .is_ok_and(|value| value == "wingmate-service")
        {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}
