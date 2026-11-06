//! 桌面和 CLI 共用一个二进制；只有桌面入口初始化 Tauri。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod desktop;
use wingmate_service::service;

use anyhow::{bail, Result};

fn main() {
    let command = std::env::args().nth(1);
    #[cfg(windows)]
    if command.is_some() {
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(u32::MAX);
        }
    }
    if let Err(error) = entry(command.as_deref()) {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn entry(command: Option<&str>) -> Result<()> {
    let home = wingmate_service::directory()?;
    match command {
        None => desktop::run(home),
        Some("start") => {
            print_state(Some(service::start(&home)?));
            Ok(())
        }
        Some("status") => {
            print_state(service::status(&home)?);
            Ok(())
        }
        Some("stop") => {
            service::stop(&home)?;
            print_state(None);
            Ok(())
        }
        Some("restart") => {
            print_state(Some(service::restart(&home)?));
            Ok(())
        }
        Some("--help") => {
            println!("wingmate [start|status|stop|restart]\n无参数打开桌面；关闭窗口保留后台。需要系统 Node.js >=24.9.0。");
            Ok(())
        }
        Some(_) => bail!("未知命令，请使用 wingmate --help"),
    }
}

fn print_state(state: Option<service::Snapshot>) {
    let value = match state {
        None => serde_json::json!({"phase":"stopped"}),
        Some(mut state) => {
            // 管理凭据和 Web 启动 token 不进入终端输出。
            state.url = state
                .url
                .and_then(|s| tauri::Url::parse(&s).ok())
                .map(|url| url.origin().ascii_serialization());
            serde_json::to_value(state).unwrap()
        }
    };
    println!("{}", serde_json::to_string_pretty(&value).unwrap());
}
