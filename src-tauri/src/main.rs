//! 桌面客户端；运行内容准备和管理操作经 Wingmate 入口完成。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod desktop;
fn main() {
    if wingmate_launcher::IS_DEV && std::env::var_os("WINGMATE_LAUNCHER").is_none() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        std::env::set_var(
            "WINGMATE_LAUNCHER",
            root.join("target/debug")
                .join(format!("wingmate{}", std::env::consts::EXE_SUFFIX)),
        );
    }
    if let Err(error) = wingmate_launcher::directory().and_then(|home| {
        let _lease = wingmate_launcher::components::desktop_lease(&home)?;
        desktop::run(home)
    }) {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
