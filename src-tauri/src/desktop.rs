//! 桌面窗口、菜单与连接观察；服务生命周期由独立 wingmate-service 库负责。
use anyhow::Result;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem, Submenu},
    Manager, WebviewUrl, WebviewWindowBuilder,
};
use wingmate_service::service;

fn show_message(app: &tauri::AppHandle, message: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let message = serde_json::to_string(message).unwrap();
        let _ = window.eval(&format!("(()=>{{document.body.innerHTML='';const h=document.createElement('h1');h.textContent='Wingmate';const p=document.createElement('p');p.textContent={message};document.body.append(h,p);document.body.style.cssText='font:16px system-ui;padding:48px;line-height:1.8';}})()"));
    }
}

fn navigate(app: &tauri::AppHandle, state: &service::Snapshot) -> Result<()> {
    if let Some(url) = &state.url {
        if let Some(window) = app.get_webview_window("main") {
            window.navigate(url.parse()?)?;
        }
    } else {
        show_message(app, state.error.as_deref().unwrap_or("后台未运行"));
    }
    Ok(())
}

pub fn run(home: PathBuf) -> Result<()> {
    let title = if wingmate_service::IS_DEV {
        "Wingmate · 开发版"
    } else {
        "Wingmate"
    };
    let busy = Arc::new(AtomicBool::new(true));
    let menu_busy = busy.clone();
    let menu_home = home.clone();
    let reconnect = Arc::new(AtomicBool::new(false));
    let menu_reconnect = reconnect.clone();
    tauri::Builder::default()
        .setup(move |app| {
            let start = MenuItem::with_id(app, "start", "启动 / 连接后台", true, None::<&str>)?;
            let restart = MenuItem::with_id(
                app,
                "restart",
                "重启后台（中断运行任务）",
                true,
                None::<&str>,
            )?;
            let stop =
                MenuItem::with_id(app, "stop", "停止后台（中断运行任务）", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "关闭窗口，保留后台", true, None::<&str>)?;
            let backend =
                Submenu::with_items(app, "后台", true, &[&start, &restart, &stop, &quit])?;
            app.set_menu(Menu::with_items(app, &[&backend])?)?;
            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title(title)
                    .inner_size(1200.0, 820.0)
                    .min_inner_size(720.0, 480.0);
            let window = if wingmate_service::IS_DEV {
                window.data_directory(home.join("webview"))
            } else {
                window
            };
            window.build()?;
            let handle = app.handle().clone();
            let state_home = home.clone();
            std::thread::spawn(move || {
                let mut last = None;
                match service::start(&state_home) {
                    Ok(state) => {
                        let _ = navigate(&handle, &state);
                        last = Some(state.instance);
                    }
                    Err(error) => show_message(&handle, &format!("启动失败：{error:#}")),
                }
                busy.store(false, Ordering::SeqCst);
                loop {
                    std::thread::sleep(Duration::from_secs(2));
                    if handle.get_webview_window("main").is_none() {
                        break;
                    }
                    if busy.load(Ordering::SeqCst) {
                        continue;
                    }
                    match service::status(&state_home) {
                        Ok(Some(state))
                            if matches!(state.phase.as_str(), "starting" | "stopping") => {}
                        Ok(Some(state))
                            if reconnect.swap(false, Ordering::SeqCst)
                                || last.as_deref() != Some(&state.instance) =>
                        {
                            let _ = navigate(&handle, &state);
                            last = Some(state.instance);
                        }
                        Ok(Some(state)) if state.phase != "running" => {
                            show_message(&handle, state.error.as_deref().unwrap_or("后台未运行"));
                        }
                        Ok(None) if last.is_some() => {
                            show_message(&handle, "后台已停止。可通过“后台”菜单重新启动。");
                            last = None;
                        }
                        Err(_) if last.is_some() => {
                            // 短暂断连不销毁页面和输入草稿。
                            if let Some(window) = handle.get_webview_window("main") {
                                let _ = window.set_title(&format!("{title} · 后台连接暂时中断"));
                            }
                        }
                        Ok(Some(_)) => {
                            if let Some(window) = handle.get_webview_window("main") {
                                let _ = window.set_title(title);
                            }
                        }
                        _ => {}
                    }
                }
            });
            Ok(())
        })
        .on_menu_event(move |app, event| {
            let action = event.id().as_ref();
            if action == "quit" {
                app.exit(0);
                return;
            }
            if !matches!(action, "start" | "stop" | "restart") {
                return;
            }
            if menu_busy.swap(true, Ordering::SeqCst) {
                return;
            }
            let action = action.to_owned();
            let home = menu_home.clone();
            let busy = menu_busy.clone();
            let reconnect = menu_reconnect.clone();
            let app = app.clone();
            std::thread::spawn(move || {
                let result = match action.as_str() {
                    "stop" => service::stop(&home).map(|()| None),
                    "restart" => service::restart(&home).map(Some),
                    _ => service::start(&home).map(Some),
                };
                match result {
                    Ok(Some(_)) => {
                        // 统一由连接观察者导航，避免菜单与轮询连续刷新同一页面。
                        reconnect.store(true, Ordering::SeqCst);
                    }
                    Ok(None) => show_message(&app, "后台已停止。可通过“后台”菜单重新启动。"),
                    Err(error) => show_message(&app, &format!("操作失败：{error:#}")),
                }
                busy.store(false, Ordering::SeqCst);
            });
        })
        .run(tauri::generate_context!())?;
    Ok(())
}
