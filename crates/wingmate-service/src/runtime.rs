//! 已安装运行内容的描述；不依赖开发目录或构建期资源。
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const DSH_ENTRY: &str = "node_modules/@deepseek-ai/dsh/lib/bin.js";
pub const WEB_PATCH: &str = "plugins/cordis.patch.yml";

pub fn toolchain_node(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("node.exe")
    } else {
        root.join("bin/node")
    }
}

pub fn toolchain_fallback(root: &Path) -> PathBuf {
    root.join("fallback")
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Runtime {
    pub id: String,
    pub revision: String,
    pub directory: PathBuf,
    pub node: PathBuf,
    #[cfg(target_os = "macos")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_path: Option<std::ffi::OsString>,
    pub dsh_home: Option<PathBuf>,
    pub agents_home: Option<PathBuf>,
}

impl Runtime {
    pub fn toolchain_environment(&self, home: &Path, command: &mut Command) -> Result<()> {
        let tools = home.join("toolchains");
        if self.node == toolchain_node(&tools) {
            let inherited = std::env::var_os("PATH").unwrap_or_default();
            let managed = self.node.parent().context("Node 路径没有父目录")?;
            let fallback = toolchain_fallback(&tools);
            let mut entries = vec![managed.to_path_buf()];
            entries.extend(std::env::split_paths(&inherited).filter(|path| {
                !path.as_os_str().is_empty() && path != managed && path != &fallback
            }));
            if fallback.is_dir() {
                entries.push(fallback);
            }
            command.env("PATH", std::env::join_paths(entries)?);
        } else {
            #[cfg(target_os = "macos")]
            if let Some(path) = &self.search_path {
                command.env("PATH", path);
            }
        }
        Ok(())
    }

    pub fn load(home: &Path) -> Result<Self> {
        serde_json::from_slice(
            &fs::read(home.join("runtime.json"))
                .context("运行内容尚未准备，请使用 wingmate start 安装或修复")?,
        )
        .map_err(Into::into)
    }

    pub fn validate(&self, home: &Path) -> Result<()> {
        let parent = home.join("runtimes").canonicalize()?;
        let directory = self.directory.canonicalize()?;
        anyhow::ensure!(
            directory.parent() == Some(parent.as_path()),
            "运行目录不属于当前管理目录"
        );
        let manifest = fs::read(directory.join("installed.json"))?;
        anyhow::ensure!(
            format!("{:x}", Sha256::digest(&manifest)) == self.revision,
            "已安装运行内容与配置不一致，请使用 wingmate 修复"
        );
        let value: serde_json::Value = serde_json::from_slice(&manifest)?;
        let base = value["components"]["base"]["sha256"]
            .as_str()
            .context("缺少基础包指纹")?;
        anyhow::ensure!(
            base.len() == 64
                && base.bytes().all(|b| b.is_ascii_hexdigit())
                && fs::read_to_string(directory.join(".wingmate-base"))? == base
                && directory.file_name().and_then(|n| n.to_str())
                    == Some(&format!("{}-{}", self.id, &base[..6])),
            "无法确认运行目录归属"
        );
        anyhow::ensure!(
            self.node.is_absolute()
                && self.node.is_file()
                && directory.join(DSH_ENTRY).is_file()
                && directory.join(WEB_PATCH).is_file(),
            "运行内容缺失，请使用 wingmate start 修复"
        );
        if let Some(expected) = value["components"]["toolchains"]["sha256"].as_str() {
            let tools = home.join("toolchains");
            anyhow::ensure!(
                tools.symlink_metadata()?.is_dir()
                    && self.node == toolchain_node(&tools)
                    && fs::read_to_string(tools.join(".wingmate-toolchains"))? == expected,
                "随包工具与运行清单不一致，请使用 wingmate start 修复"
            );
        }
        Ok(())
    }
}

pub fn quiet(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}
#[cfg(windows)]
pub fn isolate_standard_pipes() -> Result<()> {
    use windows_sys::Win32::{
        Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT},
        Storage::FileSystem::{GetFileType, FILE_TYPE_PIPE},
        System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
    };

    // Windows 创建进程时也会继承调用方已有的可继承管道句柄，导致 CLI 输出一直无法结束。
    for stream in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        unsafe {
            let handle = GetStdHandle(stream);
            if GetFileType(handle) == FILE_TYPE_PIPE
                && SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) == 0
            {
                return Err(std::io::Error::last_os_error()).context("无法隔离启动器的标准管道");
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn isolate_standard_pipes() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_runtime(node: PathBuf) -> Runtime {
        Runtime {
            id: "dsh-test".into(),
            revision: "test".into(),
            directory: PathBuf::from("runtime"),
            node,
            #[cfg(target_os = "macos")]
            search_path: None,
            dsh_home: None,
            agents_home: None,
        }
    }

    fn command_path(command: &Command) -> Option<Vec<PathBuf>> {
        command
            .get_envs()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            .and_then(|(_, value)| value)
            .map(|value| std::env::split_paths(value).collect())
    }

    #[test]
    fn toolchain_environment_prepends_js_tools_and_appends_fallback() -> Result<()> {
        let home = tempfile::tempdir()?;
        let tools = home.path().join("toolchains");
        let node = toolchain_node(&tools);
        let managed = node.parent().unwrap().to_path_buf();
        let fallback = toolchain_fallback(&tools);
        fs::create_dir_all(&managed)?;
        let runtime = test_runtime(node);

        let mut without_fallback = Command::new("cmd");
        runtime.toolchain_environment(home.path(), &mut without_fallback)?;
        let entries = command_path(&without_fallback).context("缺少 PATH")?;
        assert_eq!(entries.first(), Some(&managed));
        assert!(!entries.contains(&fallback));

        fs::create_dir_all(&fallback)?;
        let mut with_fallback = Command::new("cmd");
        runtime.toolchain_environment(home.path(), &mut with_fallback)?;
        let entries = command_path(&with_fallback).context("缺少 PATH")?;
        assert_eq!(entries.first(), Some(&managed));
        assert_eq!(entries.last(), Some(&fallback));
        assert_eq!(entries.iter().filter(|entry| *entry == &managed).count(), 1);
        assert_eq!(
            entries.iter().filter(|entry| *entry == &fallback).count(),
            1
        );
        Ok(())
    }

    #[test]
    fn system_node_does_not_inject_managed_toolchain_paths() -> Result<()> {
        let home = tempfile::tempdir()?;
        let tools = home.path().join("toolchains");
        fs::create_dir_all(toolchain_fallback(&tools))?;
        let runtime = test_runtime(PathBuf::from("node"));
        let mut command = Command::new("cmd");
        runtime.toolchain_environment(home.path(), &mut command)?;
        assert_eq!(command_path(&command), None);
        Ok(())
    }
}
