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
const OLD_DSH_OWNER: &str = "// Wingmate dsh command\n";
const OLD_WINGMATE_OWNER: &str = "// Wingmate desktop command\n";
#[cfg(not(windows))]
const UNIX_DSH_HEADER: &str = "#!/bin/sh\n# Wingmate dsh command\n";
#[cfg(not(windows))]
const UNIX_WINGMATE_HEADER: &str = "#!/bin/sh\n# Wingmate desktop command\n";

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    enabled: bool,
    needs_update: bool,
}

pub fn configure(home: &Path, state: Option<Snapshot>, enabled: Option<bool>) -> Result<Status> {
    anyhow::ensure!(SUPPORTED, "终端命令注册仅适用于支持此功能的发行版");
    let _operation = (enabled == Some(true))
        .then(|| service::operation_lock(home))
        .transpose()?;
    let launcher = || {
        let state = if enabled == Some(true) {
            service::status(home)?
                .filter(|current| {
                    state
                        .as_ref()
                        .is_none_or(|state| current.instance == state.instance)
                        && current.phase == "running"
                })
                .context("后台实例已更换，请重新连接")?
        } else {
            state.context("业务后台尚未连接")?
        };
        let entry = service::dsh_entry(home, &state)?;
        let dsh_home = state.dsh_home.context("后台未提供 DSH 数据目录")?;
        let agents_home = state
            .agents_home
            .context("后台未提供 Agent 数据目录，请用当前版本重启后台后再注册")?;
        dsh_launcher(
            &crate::toolchains::node(home)?,
            &entry,
            &dsh_home,
            &agents_home,
            &home.join("toolchains"),
        )
    };
    configure_commands(Some(home), enabled, Some(launcher))
}

pub fn install(home: &Path) -> Result<Status> {
    anyhow::ensure!(SUPPORTED, "终端命令注册仅适用于支持此功能的发行版");
    configure_commands(Some(home), Some(true), None::<fn() -> Result<String>>)
}

pub fn uninstall() -> Result<Status> {
    anyhow::ensure!(SUPPORTED, "终端命令注册仅适用于支持此功能的发行版");
    configure_commands(None, Some(false), None::<fn() -> Result<String>>)
}

pub fn shell_path_update(shell: &str) -> Result<&'static str> {
    match shell {
        #[cfg(windows)]
        "powershell" => Ok(windows::POWERSHELL_PATH),
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        "sh" => Ok(unix::SH_PATH[0].trim()),
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        "fish" => Ok(unix::FISH_PATH[0].trim()),
        _ => anyhow::bail!("当前平台不支持此 shell 的 PATH 更新：{shell}"),
    }
}

fn configure_commands(
    home: Option<&Path>,
    enabled: Option<bool>,
    dsh_launcher: Option<impl FnOnce() -> Result<String>>,
) -> Result<Status> {
    let wingmate = home
        .map(|home| wingmate_launcher(&super::executable()?, home))
        .transpose()?
        .unwrap_or_default();
    #[cfg(windows)]
    return windows::configure(enabled, dsh_launcher, &wingmate);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return unix::configure(enabled, dsh_launcher, &wingmate);
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (enabled, dsh_launcher, wingmate);
        anyhow::bail!("此平台不支持注册终端命令")
    }
}

fn dsh_launcher(
    node: &Path,
    entry: &Path,
    dsh_home: &Path,
    agents_home: &Path,
    toolchain: &Path,
) -> Result<String> {
    render_launcher(
        "dsh",
        node,
        &[entry],
        &[("DSH_HOME", dsh_home), ("DSH_AGENTS_HOME", agents_home)],
        Some([
            node.parent().context("Node 路径没有父目录")?,
            &wingmate_service::runtime::toolchain_fallback(toolchain),
        ]),
    )
}

fn wingmate_launcher(executable: &Path, home: &Path) -> Result<String> {
    render_launcher(
        "wingmate",
        executable,
        &[],
        &[("WINGMATE_HOME", home)],
        None,
    )
}

#[cfg(windows)]
fn render_launcher(
    kind: &str,
    executable: &Path,
    arguments: &[&Path],
    environment: &[(&str, &Path)],
    tools: Option<[&Path; 2]>,
) -> Result<String> {
    #[derive(serde::Serialize)]
    struct Launch<'a> {
        wingmate_command: &'a str,
        executable: &'a Path,
        arguments: &'a [&'a Path],
        environment: std::collections::BTreeMap<&'a str, &'a Path>,
        tools: Option<[&'a Path; 2]>,
    }
    let environment = environment
        .iter()
        .copied()
        .collect::<std::collections::BTreeMap<_, _>>();
    Ok(serde_json::to_string(&Launch {
        wingmate_command: kind,
        executable,
        arguments,
        environment,
        tools,
    })?)
}

#[cfg(not(windows))]
fn render_launcher(
    kind: &str,
    executable: &Path,
    arguments: &[&Path],
    environment: &[(&str, &Path)],
    tools: Option<[&Path; 2]>,
) -> Result<String> {
    use std::fmt::Write as _;

    let quote = |path: &Path| -> Result<String> {
        let path = path.to_str().context("Wingmate 路径不是 UTF-8")?;
        Ok(format!("'{}'", path.replace('\'', "'\\''")))
    };
    let header = if kind == "dsh" {
        UNIX_DSH_HEADER
    } else {
        UNIX_WINGMATE_HEADER
    };
    let mut script = header.to_owned();
    if let Some([primary, fallback]) = tools {
        let primary = quote(primary)?;
        let fallback = quote(fallback)?;
        let entry = quote(arguments[0])?;
        writeln!(
            script,
            "[ -f {entry} ] || {{ echo '运行内容已切换，请重新注册 dsh' >&2; exit 1; }}"
        )?;
        writeln!(script, "PATH={primary}${{PATH:+:$PATH}}")?;
        writeln!(script, "[ ! -d {fallback} ] || PATH=\"$PATH\":{fallback}")?;
        writeln!(script, "export PATH")?;
    }
    for (name, value) in environment {
        write!(script, "{name}={} ", quote(value)?)?;
    }
    write!(script, "exec {}", quote(executable)?)?;
    for argument in arguments {
        write!(script, " {}", quote(argument)?)?;
    }
    script.push_str(" \"$@\"\n");
    Ok(script)
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
