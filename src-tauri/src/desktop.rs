//! 桌面窗口、菜单与连接观察；服务生命周期由独立 wingmate-service 库负责。
mod startup_page;

use anyhow::Result;
use startup_page::{CardStatus, StartupPage};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem, Submenu},
    webview::PageLoadEvent,
    Manager, WebviewUrl, WebviewWindowBuilder,
};
use wingmate_service::service::{self, Snapshot, StartupStage};

#[derive(Default)]
struct ConnectionState {
    busy: bool,
    revision: u64,
    last: Option<String>,
    workspace: bool,
}

impl ConnectionState {
    fn begin_operation(&mut self) -> bool {
        if self.busy {
            return false;
        }
        self.busy = true;
        self.revision += 1;
        true
    }

    fn observation_revision(&self) -> Option<u64> {
        (!self.busy).then_some(self.revision)
    }

    fn open_workspace(&mut self, app: &tauri::AppHandle, page: &StartupPage, state: Snapshot) {
        page.open_workspace(app, &state);
        self.last = Some(state.instance);
        self.workspace = true;
    }

    fn finish_operation(
        &mut self,
        app: &tauri::AppHandle,
        page: &StartupPage,
        title: &str,
        outcome: Result<Option<Snapshot>, CardStatus>,
    ) {
        match outcome {
            Ok(Some(state)) => self.open_workspace(app, page, state),
            Ok(None) => {
                page.show(app, CardStatus::stopped());
                self.last = None;
                self.workspace = false;
            }
            Err(status) => {
                page.show(app, status);
                self.workspace = false;
            }
        }
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_title(title);
        }
        self.busy = false;
    }

    fn observe(
        &mut self,
        app: &tauri::AppHandle,
        page: &StartupPage,
        window: &tauri::WebviewWindow,
        title: &str,
        status: Result<Option<Snapshot>>,
    ) {
        match status {
            Ok(Some(state)) if matches!(state.phase.as_str(), "starting" | "stopping") => {}
            Ok(Some(state)) => {
                if state.phase == "running" {
                    if self.last.as_deref() != Some(&state.instance) {
                        self.open_workspace(app, page, state);
                    }
                } else if self.workspace {
                    page.show(
                        app,
                        CardStatus::failure(
                            StartupStage::Connecting,
                            state.error.unwrap_or_else(|| "后台未运行".into()),
                        ),
                    );
                    self.workspace = false;
                }
                let _ = window.set_title(title);
            }
            Ok(None) => {
                self.last = None;
                if self.workspace {
                    page.show(app, CardStatus::stopped());
                    let _ = window.set_title(title);
                    self.workspace = false;
                }
            }
            Err(_) if self.workspace => {
                // 短暂断连不销毁页面和输入草稿。
                let _ = window.set_title(&format!("{title} · 后台连接暂时中断"));
            }
            _ => {}
        }
    }
}

fn run_operation(
    app: &tauri::AppHandle,
    page: &StartupPage,
    home: &Path,
    action: &str,
) -> Result<Option<Snapshot>, CardStatus> {
    let stage = Cell::new(StartupStage::Checking);
    let report = |next| {
        stage.set(next);
        page.show_progress(app, next);
    };
    let result = match action {
        "stop" => {
            report(StartupStage::Stopping);
            service::stop(home).map(|()| None)
        }
        "restart" => service::restart_with_progress(home, report).map(Some),
        _ => service::start_with_progress(home, report).map(Some),
    };
    result.map_err(|error| CardStatus::failure(stage.get(), format!("{error:#}")))
}

pub fn run(home: PathBuf) -> Result<()> {
    let title = if wingmate_service::IS_DEV {
        "Wingmate · 开发版"
    } else {
        "Wingmate"
    };
    let connection = Arc::new(Mutex::new(ConnectionState {
        busy: true,
        ..ConnectionState::default()
    }));
    let menu_connection = connection.clone();
    let state_home = home.clone();
    let page = StartupPage::new();
    let menu_page = page.clone();
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
            let page_load = page.clone();
            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .on_page_load(move |window, event| {
                        if event.event() == PageLoadEvent::Finished {
                            page_load.loaded(&window, event.url());
                        }
                    })
                    .title(title)
                    .inner_size(1200.0, 820.0)
                    .min_inner_size(720.0, 480.0);
            let window = if wingmate_service::IS_DEV {
                window.data_directory(state_home.join("webview"))
            } else {
                window
            };
            window.build()?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let outcome = run_operation(&handle, &page, &state_home, "start");
                connection
                    .lock()
                    .unwrap()
                    .finish_operation(&handle, &page, title, outcome);
                loop {
                    std::thread::sleep(Duration::from_secs(2));
                    let Some(window) = handle.get_webview_window("main") else {
                        break;
                    };
                    let Some(revision) = connection.lock().unwrap().observation_revision() else {
                        continue;
                    };
                    let status = service::status(&state_home);
                    let mut current = connection.lock().unwrap();
                    // 即使菜单操作已完成，操作开始前发出的轮询也不能更新界面。
                    if current.observation_revision() == Some(revision) {
                        current.observe(&handle, &page, &window, title, status);
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
            if !menu_connection.lock().unwrap().begin_operation() {
                return;
            }
            let action = action.to_owned();
            let home = home.clone();
            let page = menu_page.clone();
            let connection = menu_connection.clone();
            let app = app.clone();
            std::thread::spawn(move || {
                let outcome = run_operation(&app, &page, &home, &action);
                connection
                    .lock()
                    .unwrap()
                    .finish_operation(&app, &page, title, outcome);
            });
        })
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ConnectionState;

    #[test]
    fn menu_operation_invalidates_an_in_flight_observation() {
        let mut connection = ConnectionState::default();
        let revision = connection.observation_revision().unwrap();

        assert!(connection.begin_operation());
        assert!(!connection.begin_operation());
        assert_eq!(connection.observation_revision(), None);

        connection.busy = false;
        assert_ne!(connection.observation_revision().unwrap(), revision);
    }
}
