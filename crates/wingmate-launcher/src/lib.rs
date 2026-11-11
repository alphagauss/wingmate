//! 不依赖 GUI 的发行引导、资源准备及管理客户端。
pub mod components;
#[cfg(not(any(feature = "bundled-runtime", feature = "release-client")))]
pub mod development;
pub mod integration;
mod runtime;
pub mod service;
pub use runtime::{directory, REVISION as RUNTIME_REVISION};
pub const IS_DEV: bool = !cfg!(any(feature = "bundled-runtime", feature = "release-client"));

#[cfg(feature = "desktop")]
pub fn open_desktop(home: &std::path::Path) -> anyhow::Result<()> {
    use std::time::{Duration, Instant};
    let _bootstrap = service::bootstrap_lock(home)?;
    let binary = components::install(home, true)?;
    let mut child = std::process::Command::new(&binary)
        .env("WINGMATE_LAUNCHER", std::env::current_exe()?)
        .env("WINGMATE_HOME", home)
        .env("WINGMATE_SERVICE_REVISION", components::service_revision()?)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let result = (|| -> anyhow::Result<()> {
        while !components::desktop_ready(&binary, child.id())? {
            anyhow::ensure!(child.try_wait()?.is_none(), "桌面进程启动失败");
            anyhow::ensure!(Instant::now() < deadline, "桌面组件登记超时");
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return result;
    }
    if let Err(error) = components::cleanup_desktops(home, &binary) {
        eprintln!("旧桌面组件清理失败: {error:#}");
    }
    Ok(())
}
