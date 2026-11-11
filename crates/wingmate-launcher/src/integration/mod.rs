//! 本机发行入口的命令与登录启动集成，与 GUI 无关。
pub mod dsh_command;
pub mod login_startup;
pub fn executable() -> anyhow::Result<std::path::PathBuf> {
    Ok(match std::env::var_os("WINGMATE_LAUNCHER") {
        Some(path) => path.into(),
        None => std::env::current_exe()?,
    })
}
