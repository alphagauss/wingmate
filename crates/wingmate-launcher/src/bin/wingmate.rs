//! 不链接 GUI 的发行、引导和命令入口。
#![cfg_attr(windows, windows_subsystem = "windows")]
use anyhow::{bail, Result};
use wingmate_launcher::{integration, service};
fn main() {
    #[cfg(windows)]
    if std::env::args_os().len() > 1 {
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(u32::MAX);
        }
    }
    // A launcher may itself have been invoked by a desktop client.
    std::env::remove_var("WINGMATE_LAUNCHER");
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let remaining: Vec<_> = args.collect();
    let max = if matches!(
        command.as_deref(),
        Some("commands" | "login-startup" | "internal-restart")
    ) {
        1
    } else {
        0
    };
    anyhow::ensure!(remaining.len() <= max, "命令参数过多");
    let mut args = remaining.into_iter();
    if matches!(command.as_deref(), Some("--help" | "-h")) {
        println!("用法: wingmate [命令]\n\n无参数         打开桌面\nstart          准备运行内容并启动业务后台\nstatus         查看业务后台状态\nstop           停止业务后台，保留管理服务\nrestart        重启业务后台并切换运行内容\nservice-status 查看管理服务状态\nservice-start  启动管理服务\nservice-stop   业务停止后退出管理服务\nservice-restart 业务停止后切换管理服务版本\ncommands [on|off] 注册或撤销终端命令\nlogin-startup [on|off] 设置登录启动\n-h, --help     显示帮助");
        return Ok(());
    }
    let home = wingmate_launcher::directory()?;
    if matches!(
        command.as_deref(),
        Some("internal-start" | "internal-restart")
    ) {
        use service::BootstrapEvent;
        let emit = |event| println!("{}", serde_json::to_string(&event).unwrap());
        let report = |stage| emit(BootstrapEvent::Stage(stage));
        let ready =
            |state: &service::Snapshot| emit(BootstrapEvent::Ready(Box::new(state.clone())));
        if command.as_deref() == Some("internal-start") {
            service::start_with_progress(&home, report, ready)?;
        } else if let Some(instance) = args.next() {
            service::restart_instance_with_progress(&home, &instance, report, ready)?;
        } else {
            service::restart_with_progress(&home, report, ready)?;
        }
        return Ok(());
    }
    let value = match command.as_deref() {
        Some("start") => public_state(Some(service::start(&home)?)),
        Some("status") => public_state(service::status(&home)?),
        Some("stop") => {
            service::stop(&home)?;
            public_state(None)
        }
        Some("restart") => public_state(Some(service::restart(&home)?)),
        Some("service-status") => match service::host_status(&home)? {
            Some(mut state) => {
                if let Some(business) = &mut state.business {
                    sanitize(business);
                }
                serde_json::to_value(state)?
            }
            None => serde_json::json!({"phase":"stopped"}),
        },
        Some("service-start") => {
            service::start_host(&home)?;
            serde_json::json!({"phase":"running"})
        }
        Some("service-stop") => {
            service::shutdown(&home)?;
            serde_json::json!({"phase":"stopped"})
        }
        Some("service-restart") => {
            service::restart_host(&home)?;
            serde_json::json!({"phase":"running"})
        }
        Some("commands") => {
            let enabled = setting(args.next())?;
            let state = if enabled == Some(false) {
                None
            } else {
                service::status(&home)?
            };
            serde_json::to_value(integration::dsh_command::configure(&home, state, enabled)?)?
        }
        Some("login-startup") => serde_json::to_value(integration::login_startup::configure(
            setting(args.next())?,
        )?)?,
        None => {
            #[cfg(feature = "desktop")]
            {
                let desktop = wingmate_launcher::desktop_component(&home)?;
                let mut command = std::process::Command::new(desktop);
                command
                    .env("WINGMATE_LAUNCHER", std::env::current_exe()?)
                    .env("WINGMATE_HOME", &home);
                command.spawn()?;
                return Ok(());
            }
            #[cfg(not(feature = "desktop"))]
            bail!("此构建不包含桌面，请使用 wingmate --help 查看命令");
        }
        Some(_) => bail!("未知命令，请使用 wingmate --help"),
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
fn setting(value: Option<String>) -> Result<Option<bool>> {
    match value.as_deref() {
        None => Ok(None),
        Some("on") => Ok(Some(true)),
        Some("off") => Ok(Some(false)),
        _ => bail!("参数应为 on 或 off"),
    }
}
fn sanitize(state: &mut service::Snapshot) {
    state.url = state
        .url
        .as_ref()
        .and_then(|url| reqwest::Url::parse(url).ok())
        .map(|url| url.origin().ascii_serialization());
}
fn public_state(state: Option<service::Snapshot>) -> serde_json::Value {
    match state {
        None => serde_json::json!({"phase":"stopped"}),
        Some(mut state) => {
            sanitize(&mut state);
            serde_json::to_value(state).unwrap()
        }
    }
}
