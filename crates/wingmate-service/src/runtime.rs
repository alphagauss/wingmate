//! 开发版直接使用安装树，发行版按 DSH 版本释放内嵌运行包。
use anyhow::{bail, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const ID: &str = env!("WINGMATE_RUNTIME_ID");
pub const DSH_ENTRY: &str = "node_modules/@deepseek-ai/dsh/lib/bin.js";
#[cfg(feature = "bundled-runtime")]
const ARCHIVE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/runtime.tar.zst"));

#[cfg(not(feature = "bundled-runtime"))]
pub fn dev_root() -> &'static Path {
    Path::new(env!("WINGMATE_DEV_ROOT"))
}

pub fn directory() -> Result<PathBuf> {
    #[cfg(not(feature = "bundled-runtime"))]
    let path = dev_root().join("wingmate");
    #[cfg(feature = "bundled-runtime")]
    let path = match std::env::var_os("WINGMATE_HOME") {
        Some(path) => PathBuf::from(path),
        None => dirs::home_dir()
            .context("无法确定用户目录")?
            .join(".wingmate"),
    };
    if !path.is_absolute() {
        bail!("WINGMATE_HOME 必须是绝对路径");
    }
    fs::create_dir_all(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(path)
}

#[cfg(not(feature = "bundled-runtime"))]
pub fn prepare(_: &Path) -> Result<PathBuf> {
    let path = option_env!("WINGMATE_DEV_STAGE")
        .map(PathBuf::from)
        .context("开发运行时尚未准备，请运行 pnpm dev")?;
    anyhow::ensure!(
        path.join(DSH_ENTRY).is_file(),
        "开发运行时缺少 DSH 入口，请运行 pnpm dev"
    );
    Ok(path)
}

#[cfg(feature = "bundled-runtime")]
pub fn prepare(home: &Path) -> Result<PathBuf> {
    let parent = home.join("runtimes");
    fs::create_dir_all(&parent)?;
    let destination = parent.join(ID);
    if destination.is_dir() {
        return Ok(destination);
    }
    // 解压完成后才发布版本目录，避免中途退出留下被误用的半成品目录。
    let staging = tempfile::tempdir_in(&parent)?;
    tar::Archive::new(zstd::stream::read::Decoder::new(ARCHIVE)?).unpack(staging.path())?;
    fs::rename(staging.path(), &destination).context("无法提交运行包")?;
    Ok(destination)
}

#[cfg(all(test, feature = "bundled-runtime"))]
mod tests {
    use super::*;

    #[test]
    fn existing_version_directory_is_reused_without_validation() {
        let home = tempfile::tempdir().unwrap();
        let destination = home.path().join("runtimes").join(ID);
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("local-note"), "keep").unwrap();

        assert_eq!(prepare(home.path()).unwrap(), destination);
        assert_eq!(
            fs::read_to_string(destination.join("local-note")).unwrap(),
            "keep"
        );
        assert!(!destination.join("node_modules").exists());
        assert!(!destination.join(".complete").exists());
    }
}

pub fn quiet(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command
}

pub fn node() -> Result<PathBuf> {
    let mut command = Command::new("node");
    quiet(&mut command);
    let output = command
        .args([
            "-p",
            "JSON.stringify({version:process.versions.node,path:process.execPath})",
        ])
        .env_remove("NODE_OPTIONS")
        .output()
        .context("找不到系统 Node.js，请安装 Node.js 24.9 或更新版本")?;
    anyhow::ensure!(output.status.success(), "系统 Node.js 检查失败");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let version = value["version"].as_str().context("Node 未返回版本")?;
    let parts: Vec<u32> = version
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    anyhow::ensure!(
        parts.len() == 3 && (parts[0] > 24 || parts[0] == 24 && parts[1] >= 9),
        "需要 Node.js >=24.9.0，当前 {version}"
    );
    Ok(PathBuf::from(
        value["path"].as_str().context("Node 未返回路径")?,
    ))
}
