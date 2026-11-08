//! 开发版直接使用安装树，发行版按内嵌运行包指纹释放。
use anyhow::{bail, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const ID: &str = env!("WINGMATE_RUNTIME_ID");
pub const REVISION: Option<&str> = option_env!("WINGMATE_RUNTIME_REVISION");
pub const DSH_ENTRY: &str = "node_modules/@deepseek-ai/dsh/lib/bin.js";
pub const WEB_PATCH: &str = "plugins/wingmate-web/cordis.patch.yml";
#[cfg(feature = "bundled-runtime")]
// 桌面和 CLI 的进度回调类型不同，解压代码的各个实例共用同一份归档。
static ARCHIVE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/runtime.tar.zst"));

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
pub fn prepare(_: &Path, _: impl FnOnce()) -> Result<PathBuf> {
    let path = option_env!("WINGMATE_DEV_STAGE")
        .map(PathBuf::from)
        .context("开发运行时尚未准备，请运行 pnpm dev")?;
    anyhow::ensure!(
        path.join(DSH_ENTRY).is_file() && path.join(WEB_PATCH).is_file(),
        "开发运行时缺少 DSH 或后台服务插件，请运行 pnpm dev"
    );
    Ok(path)
}

#[cfg(feature = "bundled-runtime")]
pub fn prepare(home: &Path, on_extract: impl FnOnce()) -> Result<PathBuf> {
    let parent = home.join("runtimes");
    let destination = parent.join(release_directory());
    if destination.is_dir() {
        return Ok(destination);
    }
    on_extract();
    fs::create_dir_all(&parent)?;
    // 解压完成后才发布版本目录，避免中途退出留下被误用的半成品目录。
    let staging = tempfile::tempdir_in(&parent)?;
    tar::Archive::new(zstd::stream::read::Decoder::new(ARCHIVE)?).unpack(staging.path())?;
    fs::rename(staging.path(), &destination).context("无法提交运行包")?;
    Ok(destination)
}

#[cfg(feature = "bundled-runtime")]
fn release_directory() -> String {
    format!("{ID}-{}", REVISION.unwrap())
}

#[cfg(all(test, feature = "bundled-runtime"))]
mod tests {
    use super::*;

    #[test]
    fn existing_fingerprint_directory_is_reused_without_validation() {
        let home = tempfile::tempdir().unwrap();
        let destination = home.path().join("runtimes").join(release_directory());
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("local-note"), "keep").unwrap();

        assert_eq!(
            prepare(home.path(), || panic!("复用目录时不应报告解压")).unwrap(),
            destination
        );
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
