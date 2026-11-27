#[cfg(windows)]
mod windows;

use crate::runtime::{self, TOOLCHAINS};
use anyhow::{Context, Result};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use wingmate_service::runtime::toolchain_node;

const REVISION: &str = ".wingmate-toolchains";
const PREVIOUS: &str = ".toolchains-previous";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolchainProcess {
    pub pid: u32,
    pub started: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolchainBusy(pub Vec<ToolchainProcess>);

impl std::fmt::Display for ToolchainBusy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids = self
            .0
            .iter()
            .map(|process| process.pid.to_string())
            .collect::<Vec<_>>()
            .join("、");
        write!(formatter, "Wingmate 随包 Node 仍在运行，进程 PID：{ids}。关闭这些进程后重试，或执行 wingmate restart --force-toolchain。")
    }
}

impl std::error::Error for ToolchainBusy {}

pub(crate) struct Prepared {
    stage: tempfile::TempDir,
}

pub(crate) struct Switch {
    live: PathBuf,
    previous: PathBuf,
    committed: bool,
}

fn installed(directory: &Path) -> Result<Option<String>> {
    let fingerprint = match directory.symlink_metadata() {
        Ok(metadata) if metadata.is_dir() => fs::read_to_string(directory.join(REVISION))
            .with_context(|| format!("无法确认目录归属：{}", directory.display()))?,
        Ok(_) => anyhow::bail!("工具目录不是普通目录：{}", directory.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        fingerprint.len() == 64 && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "工具目录清单无效：{}",
        directory.display()
    );
    Ok(Some(fingerprint))
}

fn check_entries(root: &Path) -> Result<()> {
    let modules = root.join(if cfg!(windows) {
        "node_modules"
    } else {
        "lib/node_modules"
    });
    let node = toolchain_node(root);
    let bin = node.parent().unwrap();
    for entry in [
        node.clone(),
        bin.join(if cfg!(windows) { "npm.cmd" } else { "npm" }),
        bin.join(if cfg!(windows) { "npx.cmd" } else { "npx" }),
        bin.join(if cfg!(windows) { "pnpm.cmd" } else { "pnpm" }),
        bin.join(if cfg!(windows) { "pnpx.cmd" } else { "pnpx" }),
        modules.join("npm/bin/npm-cli.js"),
        modules.join("npm/bin/npx-cli.js"),
        modules.join("pnpm/bin/pnpm.mjs"),
        modules.join("pnpm/bin/pnpx.mjs"),
    ] {
        anyhow::ensure!(entry.is_file(), "随包工具缺少入口：{}", entry.display());
    }
    Ok(())
}

pub(crate) fn prepare(home: &Path, on_extract: impl FnOnce()) -> Result<Option<Prepared>> {
    let Some(desired) = runtime::toolchain_revision()? else {
        return Ok(None);
    };
    let live = home.join(TOOLCHAINS);
    let previous = home.join(PREVIOUS);
    if installed(&live)?.is_none() && installed(&previous)?.is_some() {
        fs::rename(&previous, &live).context("恢复上次未完成的工具更新")?;
    }
    if installed(&live)?.as_deref() == Some(desired.as_str()) && check_entries(&live).is_ok() {
        return Ok(None);
    }
    on_extract();
    let stage = tempfile::Builder::new()
        .prefix(".toolchains-stage-")
        .tempdir_in(home)?;
    runtime::unpack(TOOLCHAINS, stage.path())?;
    let tools = stage.path().join(TOOLCHAINS);
    check_entries(&tools)?;
    runtime::check_node(&toolchain_node(&tools)).context("验证随包 Node")?;
    fs::write(tools.join(REVISION), desired)?;
    Ok(Some(Prepared { stage }))
}

impl Prepared {
    pub(crate) fn publish(
        self,
        home: &Path,
        approval: super::service::ToolchainApproval<'_>,
    ) -> Result<Switch> {
        let live = home.join(TOOLCHAINS);
        let previous = home.join(PREVIOUS);
        let has_live = installed(&live)?.is_some();
        if has_live {
            #[cfg(windows)]
            windows::release(&toolchain_node(&live), approval)?;
            #[cfg(not(windows))]
            let _ = approval;
            if installed(&previous)?.is_some() {
                fs::remove_dir_all(&previous).context("清理上一次已替换的工具目录")?;
            }
            fs::rename(&live, &previous).context("移开已安装的随包工具")?;
        }
        if let Err(error) = fs::rename(self.stage.path().join(TOOLCHAINS), &live) {
            if has_live {
                fs::rename(&previous, &live)
                    .context("恢复随包工具目录失败，旧工具仍保存在 .toolchains-previous")?;
            }
            return Err(error).context("替换随包工具失败");
        }
        Ok(Switch {
            live,
            previous,
            committed: false,
        })
    }
}

impl Switch {
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for Switch {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let restore = (|| -> io::Result<()> {
            if self.previous.is_dir() {
                let rejected = tempfile::Builder::new()
                    .prefix(".toolchains-rejected-")
                    .tempdir_in(self.live.parent().unwrap())?;
                let staged = rejected.path().join(TOOLCHAINS);
                fs::rename(&self.live, &staged)?;
                if let Err(error) = fs::rename(&self.previous, &self.live) {
                    fs::rename(&staged, &self.live)?;
                    return Err(error);
                }
            }
            Ok(())
        })();
        if let Err(error) = restore {
            eprintln!(
                "恢复随包工具目录失败：{error}。旧工具目录：{}",
                self.previous.display()
            );
        }
    }
}

pub(crate) fn cleanup(home: &Path) -> Result<()> {
    let previous = home.join(PREVIOUS);
    if installed(&previous)?.is_some() {
        fs::remove_dir_all(previous)?;
    }
    Ok(())
}

pub(crate) fn node(home: &Path) -> Result<PathBuf> {
    let tools = home.join(TOOLCHAINS);
    check_entries(&tools)?;
    Ok(toolchain_node(&tools))
}

pub(crate) fn diagnostics(home: &Path) -> Result<serde_json::Value> {
    let node = if crate::IS_DEV {
        runtime::node_info()?
    } else {
        runtime::check_node(&node(home)?)?
    };
    Ok(
        serde_json::json!({"source": if crate::IS_DEV {"system"} else {"bundled"}, "info": node, "ok": true}),
    )
}

#[cfg(test)]
#[test]
fn restores_fixed_toolchain_directory_if_publication_is_abandoned() -> Result<()> {
    let root = tempfile::tempdir()?;
    let home = root.path();
    let live = home.join(TOOLCHAINS);
    fs::create_dir(&live)?;
    let old = "a".repeat(64);
    let new = "b".repeat(64);
    fs::write(live.join(REVISION), &old)?;
    let stage = tempfile::tempdir_in(home)?;
    let candidate = stage.path().join(TOOLCHAINS);
    fs::create_dir(&candidate)?;
    fs::write(candidate.join(REVISION), &new)?;
    let switched = Prepared { stage }.publish(home, crate::service::ToolchainApproval::None)?;
    assert_eq!(installed(&live)?, Some(new));
    assert_eq!(installed(&home.join(PREVIOUS))?, Some(old.clone()));
    drop(switched);
    assert_eq!(installed(&live)?, Some(old));
    assert_eq!(installed(&home.join(PREVIOUS))?, None);
    Ok(())
}
