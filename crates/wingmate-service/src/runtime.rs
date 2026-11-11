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

#[derive(Clone, Serialize, Deserialize)]
pub struct Runtime {
    pub id: String,
    pub revision: String,
    pub directory: PathBuf,
    pub node: PathBuf,
    pub dsh_home: Option<PathBuf>,
    pub agents_home: Option<PathBuf>,
}

impl Runtime {
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
