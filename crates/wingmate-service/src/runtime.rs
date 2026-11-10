//! 按组件清单安装开发或内嵌运行包。
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
type Archive = &'static [u8];
#[cfg(not(feature = "bundled-runtime"))]
type Archive = &'static str;
include!(concat!(env!("OUT_DIR"), "/components.rs"));

const BASE_MARKER: &str = ".wingmate-base";
const INSTALLED: &str = "installed.json";
const OBSOLETE: &str = ".wingmate-obsolete";
const SHORT_HASH: usize = 6;

#[derive(serde::Deserialize)]
struct Manifest {
    format: u32,
    components: std::collections::BTreeMap<String, Component>,
}

#[derive(serde::Deserialize, PartialEq, Eq)]
struct Component {
    sha256: String,
    paths: Vec<String>,
}

impl Manifest {
    fn parse(source: &str) -> Result<Self> {
        let manifest: Self = serde_json::from_str(source)?;
        anyhow::ensure!(
            manifest.format == 1
                && manifest.components.contains_key("base")
                && manifest
                    .components
                    .values()
                    .all(|part| is_fingerprint(&part.sha256)),
            "运行组件清单无效"
        );
        Ok(manifest)
    }

    fn base(&self) -> &str {
        &self.components["base"].sha256
    }

    fn directory(&self) -> String {
        format!("{ID}-{}", &self.base()[..SHORT_HASH])
    }
}

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

fn is_runtime_directory(parent: &Path, name: &str) -> bool {
    name.strip_prefix("dsh-")
        .and_then(|name| name.rsplit_once('-'))
        .is_some_and(|(version, hash)| {
            !version.is_empty()
                && version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
                && hash.len() == SHORT_HASH
                && fs::read_to_string(parent.join(name).join(BASE_MARKER))
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

fn cleanup(home: &Path) -> Result<()> {
    let parent = home.join("runtimes");
    let manifest = Manifest::parse(MANIFEST)?;
    let active = manifest.directory();
    let directory = parent.join(&active);
    anyhow::ensure!(
        parent.symlink_metadata()?.is_dir()
            && directory.symlink_metadata()?.is_dir()
            && fs::read_to_string(directory.join(BASE_MARKER))? == manifest.base(),
        "无法确认当前运行目录归属，跳过清理"
    );
    prune(&parent, std::slice::from_ref(&active), |name| {
        is_runtime_directory(&parent, name)
    })?;
    remove_directory(&directory.join(OBSOLETE))?;
    Ok(())
}

fn remove_directory(path: &Path) -> Result<()> {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => bail!("拒绝删除非普通目录 {}", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub fn prepare(home: &Path, on_extract: impl FnOnce()) -> Result<PathBuf> {
    let target = Manifest::parse(MANIFEST).context("运行组件尚未准备，请检查构建产物")?;
    install_components(
        &home.join("runtimes"),
        MANIFEST,
        &target,
        on_extract,
        |name, stage| {
            let archive = ARCHIVES
                .iter()
                .find(|(key, _)| *key == name)
                .with_context(|| format!("缺少运行组件 {name}"))?
                .1;
            #[cfg(not(feature = "bundled-runtime"))]
            let archive =
                fs::File::open(archive).context("缺少开发运行组件，请运行 pnpm runtime:dev")?;
            tar::Archive::new(zstd::stream::read::Decoder::new(archive)?).unpack(stage)?;
            #[cfg(not(feature = "bundled-runtime"))]
            if name == "base" {
                let package: serde_json::Value =
                    serde_json::from_slice(&fs::read(stage.join("package.json"))?)?;
                anyhow::ensure!(
                    package["dependencies"]["@deepseek-ai/dsh"]
                        .as_str()
                        .is_some_and(|version| ID.strip_prefix("dsh-") == Some(version)),
                    "开发依赖版本已变化，请重新运行 pnpm dev"
                );
            }
            Ok(())
        },
    )
}

fn install_components(
    parent: &Path,
    source: &str,
    target: &Manifest,
    on_extract: impl FnOnce(),
    unpack: impl Fn(&str, &Path) -> Result<()>,
) -> Result<PathBuf> {
    fs::create_dir_all(parent)?;
    anyhow::ensure!(
        parent.symlink_metadata()?.is_dir(),
        "运行根目录不是普通目录"
    );
    let destination = parent.join(target.directory());
    let installed_file = destination.join(INSTALLED);
    let installed = match destination.symlink_metadata() {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir()
                    && fs::read_to_string(destination.join(BASE_MARKER))
                        .is_ok_and(|hash| hash == target.base()),
                "运行目录 {} 已被占用且无法确认基础包指纹，请检查后手动移走",
                destination.display()
            );
            match fs::read_to_string(&installed_file) {
                Ok(current) if current == source => {
                    ensure_entry(&destination, &destination, &[])?;
                    return Ok(destination);
                }
                Ok(current) => {
                    let current = Manifest::parse(&current).with_context(|| {
                        format!("安装清单无效，请移走 {} 后重试", destination.display())
                    })?;
                    anyhow::ensure!(current.base() == target.base(), "安装清单与基础包不匹配");
                    Some(current)
                }
                // The manifest is removed before any in-place changes. Rebuild interrupted installs.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    remove_directory(&destination)?;
                    None
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let changed: Vec<_> = target
        .components
        .iter()
        .filter(|(name, part)| {
            installed
                .as_ref()
                .and_then(|current| current.components.get(*name))
                != Some(*part)
        })
        .collect();
    if !changed.is_empty() {
        on_extract();
    }
    let stage = tempfile::tempdir_in(parent)?;
    for (name, _) in &changed {
        unpack(name, stage.path()).with_context(|| format!("释放运行组件 {name}"))?;
    }
    ensure_entry(stage.path(), &destination, &changed)?;
    fs::write(stage.path().join(INSTALLED), source)?;
    if let Some(current) = installed {
        let mut paths: Vec<_> = changed
            .iter()
            .flat_map(|(_, part)| part.paths.iter().cloned())
            .collect();
        // Only removed, managed plugin roots are eligible. Patch file ownership is tied to the base.
        for (name, part) in &current.components {
            let plugin = format!("plugins/{name}");
            if !target.components.contains_key(name)
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && part.paths == [plugin.as_str()]
            {
                paths.push(plugin);
            }
        }
        if paths.is_empty() {
            fs::rename(stage.path().join(INSTALLED), installed_file)?;
            return Ok(destination);
        }
        let obsolete = destination.join(OBSOLETE);
        fs::create_dir_all(&obsolete)?;
        anyhow::ensure!(
            obsolete.symlink_metadata()?.is_dir(),
            "待清理路径不是普通目录"
        );
        let retired = tempfile::tempdir_in(obsolete)?.keep();
        fs::remove_file(&installed_file)?;
        for (index, path) in paths.iter().enumerate() {
            let live = destination.join(path);
            match live.symlink_metadata() {
                Ok(_) => fs::rename(&live, retired.join(index.to_string()))?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            let replacement = stage.path().join(path);
            if replacement.exists() {
                fs::create_dir_all(live.parent().unwrap())?;
                fs::rename(replacement, live)?;
            }
        }
        fs::rename(stage.path().join(INSTALLED), installed_file)?;
    } else {
        fs::write(stage.path().join(BASE_MARKER), target.base())?;
        fs::rename(stage.path(), &destination).context("无法提交运行目录")?;
    }
    Ok(destination)
}

fn ensure_entry(stage: &Path, destination: &Path, changed: &[(&String, &Component)]) -> Result<()> {
    for (_, part) in changed {
        for path in &part.paths {
            anyhow::ensure!(stage.join(path).exists(), "运行组件缺少 {path}");
        }
    }
    for path in [DSH_ENTRY, WEB_PATCH] {
        anyhow::ensure!(
            stage.join(path).is_file() || destination.join(path).is_file(),
            "运行目录缺少 {path}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;

    #[test]
    fn plugin_update_reuses_base_and_replaces_whole_plugin_directory() -> Result<()> {
        let root = tempfile::tempdir()?;
        let components = |plugin_hash: char| {
            serde_json::json!({
                "format": 1,
                "components": {
                    "base": { "sha256": "a".repeat(64), "paths": ["node_modules"] },
                    "config": { "sha256": "b".repeat(64), "paths": [WEB_PATCH] },
                    "wingmate-web": {
                        "sha256": plugin_hash.to_string().repeat(64),
                        "paths": ["plugins/wingmate-web"]
                    }
                }
            })
            .to_string()
        };
        let first = components('c');
        let second = components('d');
        let unpack = |name: &str, stage: &Path| {
            let path = match name {
                "base" => DSH_ENTRY,
                "config" => WEB_PATCH,
                "wingmate-web" => "plugins/wingmate-web/host.mjs",
                _ => unreachable!(),
            };
            let path = stage.join(path);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, name)?;
            Ok(())
        };
        let path = install_components(
            root.path(),
            &first,
            &Manifest::parse(&first)?,
            || {},
            unpack,
        )?;
        fs::write(path.join("plugins/wingmate-web/old.js"), "obsolete")?;
        fs::write(path.join(DSH_ENTRY), "base probe")?;

        let target = Manifest::parse(&second)?;
        let updated = install_components(
            root.path(),
            &second,
            &target,
            || {},
            |name, stage| {
                assert_eq!(name, "wingmate-web");
                unpack(name, stage)
            },
        )?;
        assert_eq!(updated, path);
        assert_eq!(fs::read_to_string(path.join(DSH_ENTRY))?, "base probe");
        assert!(!path.join("plugins/wingmate-web/old.js").exists());
        assert_eq!(fs::read_to_string(path.join(INSTALLED))?, second);

        let reformatted = format!("{second}\n");
        install_components(
            root.path(),
            &reformatted,
            &target,
            || panic!("清单格式变化不应解包"),
            |_, _| panic!("清单格式变化不应解包"),
        )?;
        assert_eq!(fs::read_to_string(path.join(INSTALLED))?, reformatted);

        fs::remove_file(path.join(INSTALLED))?;
        install_components(root.path(), &second, &target, || {}, unpack)?;
        assert_eq!(fs::read_to_string(path.join(DSH_ENTRY))?, "base");
        Ok(())
    }

    #[test]
    fn collection_preserves_active_and_unmanaged_directories() -> Result<()> {
        let root = tempfile::tempdir()?;
        let active = "dsh-0.1.0-aaaaaa";
        let old = "dsh-0.1.0-bbbbbb";
        let unmanaged = "dsh-0.1.0-cccccc";
        let mismatched = "dsh-0.1.0-dddddd";
        for name in [active, old, unmanaged, mismatched] {
            fs::create_dir(root.path().join(name))?;
        }
        for (name, hash) in [(active, 'a'), (old, 'b'), (mismatched, 'e')] {
            fs::write(
                root.path().join(name).join(BASE_MARKER),
                hash.to_string().repeat(64),
            )?;
        }
        prune(root.path(), &[active.to_owned()], |name| {
            is_runtime_directory(root.path(), name)
        })?;
        assert!(!root.path().join(old).exists());
        for name in [active, unmanaged, mismatched] {
            assert!(root.path().join(name).is_dir());
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
