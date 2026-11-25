//! 不依赖 GUI 的发行引导、资源准备及管理客户端。
pub mod components;
#[cfg(feature = "desktop")]
mod desktop;
#[cfg(not(any(feature = "bundled-runtime", feature = "release-client")))]
pub mod development;
pub mod integration;
#[cfg(target_os = "macos")]
mod macos_path;
pub mod management;
#[cfg(windows)]
mod process_wait;
mod runtime;
pub mod service;
mod toolchains;
pub use runtime::{directory, REVISION as RUNTIME_REVISION};
pub const IS_DEV: bool = !cfg!(any(feature = "bundled-runtime", feature = "release-client"));

#[cfg(feature = "desktop")]
pub use desktop::{open_desktop, reopen_desktop};
