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
pub const WEB_PATCH: &str = "plugins/cordis.patch.yml";
#[cfg(feature = "bundled-runtime")]
// 桌面和 CLI 的进度回调类型不同，解压代码的各个实例共用同一份归档。
static ARCHIVE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/runtime.tar.zst"));
#[cfg(feature = "bundled-runtime")]
const RELEASE_SUFFIX_LEN: usize = 6;
#[cfg(feature = "bundled-runtime")]
const RELEASE_REVISION_FILE: &str = ".wingmate-revision";

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
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join("package.json"))?)?;
    anyhow::ensure!(
        manifest["dependencies"]["@deepseek-ai/dsh"]
            .as_str()
            .map(|v| format!("dsh-{v}"))
            == Some(ID.into()),
        "开发依赖版本已变化，请重新运行 pnpm dev"
    );
    Ok(path)
}

// Caller holds operation.lock, also held by the development preparation helper.
pub(crate) fn cleanup_after_ready(home: &Path, state: &crate::service::Snapshot) {
    if state.phase != "running"
        || state.runtime != ID
        || state.runtime_revision.as_deref() != REVISION
        || REVISION.is_none()
        || state.url.is_none()
    {
        return;
    }
    if let Err(error) = cleanup(home) {
        use std::io::Write;
        let message = format!("旧运行目录清理未完成，下次启动重试: {error:#}");
        eprintln!("{message}");
        if let Ok(mut log) = fs::OpenOptions::new()
            .append(true)
            .open(home.join("service.log"))
        {
            let _ = writeln!(log, "{message}");
        }
    }
}

fn is_fingerprint(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(feature = "bundled-runtime")]
fn is_release_directory(parent: &Path, name: &str) -> bool {
    name.strip_prefix("dsh-")
        .and_then(|name| name.rsplit_once('-'))
        .is_some_and(|(version, hash)| {
            !version.is_empty()
                && version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
                && hash.len() == RELEASE_SUFFIX_LEN
                // A legacy version-only directory can also end in a hex string.
                && fs::read_to_string(parent.join(name).join(RELEASE_REVISION_FILE))
                    .is_ok_and(|revision| is_fingerprint(&revision) && revision.starts_with(hash))
        })
}

fn prune(parent: &Path, keep: &[String], managed: impl Fn(&str) -> bool) -> Result<()> {
    if !parent.exists() {
        return Ok(());
    }
    anyhow::ensure!(
        !fs::symlink_metadata(parent)?.file_type().is_symlink(),
        "拒绝清理链接目录 {}",
        parent.display()
    );
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() && managed(&name) && !keep.contains(&name) {
            // Only immediate, recognized directories are eligible; never follow directory links.
            fs::remove_dir_all(entry.path())
                .with_context(|| format!("删除 {}", entry.path().display()))?;
        }
    }
    Ok(())
}

#[cfg(feature = "bundled-runtime")]
fn cleanup(home: &Path) -> Result<()> {
    let parent = home.join("runtimes");
    prune(&parent, &[release_directory()], |name| {
        is_release_directory(&parent, name)
    })
}

#[cfg(not(feature = "bundled-runtime"))]
fn cleanup(_: &Path) -> Result<()> {
    let stages = dev_root().join("runtimes");
    let mut keep = vec![REVISION.context("缺少开发运行指纹")?.to_owned()];
    // A build can have prepared the next version before this older executable starts.
    let pending = fs::read_to_string(stages.join("current"))?;
    anyhow::ensure!(is_fingerprint(pending.trim()), "开发运行目录指针无效");
    keep.push(pending.trim().to_owned());
    prune(&stages, &keep, is_fingerprint)?;
    let mut dependencies = Vec::new();
    for entry in fs::read_dir(&stages)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && is_fingerprint(&entry.file_name().to_string_lossy()) {
            let key = fs::read_to_string(entry.path().join(".wingmate-dependency"))?;
            anyhow::ensure!(is_fingerprint(key.trim()), "开发依赖指纹无效");
            dependencies.push(key.trim().to_owned());
        }
    }
    prune(
        &dev_root().join("dependencies"),
        &dependencies,
        is_fingerprint,
    )?;
    // Compatibility with the previous layout, within this checkout and same dev service lock.
    let legacy = dev_root()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("runtime/stage");
    prune(&legacy, &[], is_fingerprint)?;
    if legacy.is_dir() {
        for entry in fs::read_dir(&legacy)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && entry.file_name().to_string_lossy().starts_with("current-")
            {
                let key = fs::read_to_string(entry.path())?;
                if is_fingerprint(key.trim()) && !legacy.join(key.trim()).exists() {
                    fs::remove_file(entry.path())?;
                }
            }
        }
        if fs::read_dir(&legacy)?.next().is_none() {
            fs::remove_dir(&legacy)?;
        }
    }
    Ok(())
}

#[cfg(feature = "bundled-runtime")]
pub fn prepare(home: &Path, on_extract: impl FnOnce()) -> Result<PathBuf> {
    let parent = home.join("runtimes");
    let destination = parent.join(release_directory());
    let revision = REVISION.unwrap();
    match destination.symlink_metadata() {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir()
                    && fs::read_to_string(destination.join(RELEASE_REVISION_FILE))
                        .is_ok_and(|stored| stored == revision),
                "运行目录 {} 已被占用且无法确认指纹一致，请检查后手动移走该目录再启动",
                destination.display()
            );
            return Ok(destination);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("检查 {}", destination.display())),
    }
    on_extract();
    fs::create_dir_all(&parent)?;
    // 解压完成后才发布版本目录，避免中途退出留下被误用的半成品目录。
    let staging = tempfile::tempdir_in(&parent)?;
    tar::Archive::new(zstd::stream::read::Decoder::new(ARCHIVE)?).unpack(staging.path())?;
    fs::write(staging.path().join(RELEASE_REVISION_FILE), revision)?;
    fs::rename(staging.path(), &destination).context("无法提交运行包")?;
    Ok(destination)
}

#[cfg(feature = "bundled-runtime")]
fn release_directory() -> String {
    format!("{ID}-{}", &REVISION.unwrap()[..RELEASE_SUFFIX_LEN])
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;

    #[test]
    fn collection_preserves_active_pending_and_unmanaged_directories() -> Result<()> {
        let root = tempfile::tempdir()?;
        let active = "a".repeat(64);
        let pending = "b".repeat(64);
        let old = "c".repeat(64);
        for name in [&active, &pending, &old, ".install-in-progress", "user-data"] {
            fs::create_dir(root.path().join(name))?;
            fs::write(root.path().join(name).join("file"), "keep")?;
        }
        prune(
            root.path(),
            &[active.clone(), pending.clone()],
            is_fingerprint,
        )?;
        assert!(!root.path().join(old).exists());
        for name in [&active, &pending, ".install-in-progress", "user-data"] {
            assert!(root.path().join(name).join("file").exists());
        }
        Ok(())
    }

    #[test]
    fn failed_or_different_backend_cannot_trigger_collection() -> Result<()> {
        let home = tempfile::tempdir()?;
        let old = home
            .path()
            .join("runtimes")
            .join(format!("dsh-0.1.0-{}", "a".repeat(64)));
        fs::create_dir_all(&old)?;
        let mut state = crate::service::Snapshot {
            product: "wingmate".into(),
            protocol: 2,
            instance: "test".into(),
            runtime: ID.into(),
            runtime_revision: REVISION.map(str::to_owned),
            dsh_home: None,
            pid: 1,
            engine_pid: Some(1),
            phase: "failed".into(),
            url: Some("http://127.0.0.1:1234".into()),
            error: None,
        };
        cleanup_after_ready(home.path(), &state);
        assert!(old.exists());
        state.phase = "running".into();
        state.runtime_revision = Some("different".into());
        cleanup_after_ready(home.path(), &state);
        assert!(old.exists());
        Ok(())
    }
}

#[cfg(all(test, feature = "bundled-runtime"))]
mod tests {
    use super::*;

    #[test]
    fn short_directory_is_reused_when_full_fingerprint_matches() -> Result<()> {
        let home = tempfile::tempdir()?;
        let parent = home.path().join("runtimes");
        let revision = REVISION.unwrap();
        let destination = parent.join(format!("{ID}-{}", &revision[..6]));
        fs::create_dir_all(&destination)?;
        fs::write(destination.join(RELEASE_REVISION_FILE), revision)?;

        assert_eq!(
            prepare(home.path(), || panic!("复用目录时不应报告解压"))?,
            destination
        );
        assert!(!destination.join("node_modules").exists());
        Ok(())
    }

    #[test]
    fn short_name_collision_rejects_startup_and_preserves_directory() -> Result<()> {
        let home = tempfile::tempdir()?;
        let destination = home.path().join("runtimes").join(release_directory());
        fs::create_dir_all(&destination)?;
        let mut collision = REVISION.unwrap().to_owned();
        let last = if collision.ends_with('0') { "1" } else { "0" };
        collision.replace_range(63.., last);
        let marker = destination.join(RELEASE_REVISION_FILE);
        fs::write(&marker, &collision)?;

        let error = prepare(home.path(), || panic!("撞号时不应解压")).unwrap_err();
        assert!(error.to_string().contains("无法确认指纹一致"));
        assert_eq!(fs::read_to_string(marker)?, collision);
        Ok(())
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
