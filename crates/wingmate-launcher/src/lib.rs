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
pub fn desktop_component(home: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join("bootstrap.lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    components::install(home, true)
}
