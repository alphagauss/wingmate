//! 桌面窗口、首页与连接观察；服务生命周期由独立 wingmate-service 库负责。
mod bridge;
mod popup;
mod startup_page;
mod titlebar;

use anyhow::Result;
use startup_page::{CardStatus, StartupPage};
use std::{
    cell::Cell,
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{webview::PageLoadEvent, Manager, WebviewUrl, WebviewWindowBuilder};
use wingmate_service::{
    service::{self, Snapshot, StartupStage},
    RUNTIME_REVISION,
};

#[derive(Default)]
struct ConnectionState {
    busy: bool,
    exit_pending: bool,
    revision: u64,
    last: Option<String>,
    workspace: Option<Snapshot>,
    pending_update: Option<String>,
    authorized_origins: HashSet<String>,
    titlebar_theme: titlebar::Theme,
    titlebar: titlebar::Frame,
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

    fn defer_exit(&mut self) -> bool {
        self.exit_pending = self.busy;
        self.exit_pending
    }

    fn open_workspace(
        &mut self,
        app: &tauri::AppHandle,
        page: &StartupPage,
        state: Snapshot,
    ) -> Result<()> {
        let origin =
            bridge::origin(&state).ok_or_else(|| anyhow::anyhow!("后台未提供可用的工作台地址"))?;
        if !self.authorized_origins.contains(&origin) {
            bridge::authorize(app, &origin)?;
            self.authorized_origins.insert(origin);
        }
        self.last = Some(state.instance.clone());
        self.pending_update = None;
        self.workspace = Some(state);
        page.open_workspace(app, self.workspace.as_ref().unwrap());
        Ok(())
    }

    fn connect(
        &mut self,
        app: &tauri::AppHandle,
        page: &StartupPage,
        state: Snapshot,
        continue_instance: Option<&str>,
    ) {
        self.last = Some(state.instance.clone());
        if state.runtime_revision.as_deref() != RUNTIME_REVISION
            && continue_instance != Some(&state.instance)
        {
            self.workspace = None;
            self.pending_update = Some(state.instance);
            page.show(app, CardStatus::update_available());
        } else if let Err(error) = self.open_workspace(app, page, state) {
            self.workspace = None;
            self.pending_update = None;
            page.show(
                app,
                CardStatus::failure(StartupStage::Connecting, format!("{error:#}")),
            );
        }
    }

    fn finish_operation(
        &mut self,
        app: &tauri::AppHandle,
        page: &StartupPage,
        title: &str,
        continue_instance: Option<&str>,
        outcome: Result<Option<Snapshot>, CardStatus>,
    ) {
        match outcome {
            Ok(Some(state)) => self.connect(app, page, state, continue_instance),
            Ok(None) => {
                page.show(app, CardStatus::stopped());
                self.last = None;
                self.workspace = None;
                self.pending_update = None;
            }
            Err(status) => {
                page.show(app, status);
                self.workspace = None;
                self.pending_update = None;
            }
        }
        if let Some(window) = app.get_webview_window("main") {
            titlebar::connection_title(&window, title, false);
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
                        self.connect(app, page, state, None);
                    }
                } else if self.workspace.is_some() || self.pending_update.is_some() {
                    page.show(
                        app,
                        CardStatus::failure(
                            StartupStage::Connecting,
                            state.error.unwrap_or_else(|| "后台未运行".into()),
                        ),
                    );
                    self.workspace = None;
                    self.pending_update = None;
                }
                titlebar::connection_title(window, title, false);
            }
            Ok(None) => {
                self.last = None;
                if self.workspace.take().is_some() || self.pending_update.is_some() {
                    page.show(app, CardStatus::stopped());
                    titlebar::connection_title(window, title, false);
                }
                self.pending_update = None;
            }
            Err(_) if self.workspace.is_some() => {
                // 短暂断连不销毁页面和输入草稿。
                titlebar::connection_title(window, title, true);
            }
            _ => {}
        }
    }
}

enum Operation {
    Start,
    Continue(String),
    Stop(String),
    Restart(String),
}

fn run_operation(
    app: &tauri::AppHandle,
    page: &StartupPage,
    home: &Path,
    action: &Operation,
) -> Result<Option<Snapshot>, CardStatus> {
    let stage = Cell::new(StartupStage::Checking);
    let report = |next| {
        stage.set(next);
        page.show_progress(app, next);
    };
    let result = match action {
        Operation::Stop(instance) => {
            report(StartupStage::Stopping);
            service::stop_instance(home, instance).map(|()| None)
        }
        Operation::Restart(instance) => {
            service::restart_instance_with_progress(home, instance, report).map(Some)
        }
        Operation::Start => service::start_with_progress(home, report).map(Some),
        Operation::Continue(_) => {
            report(StartupStage::Connecting);
            service::status(home).and_then(|state| {
                anyhow::ensure!(
                    state.as_ref().is_none_or(|state| state.phase == "running"),
                    "后台正在切换，请稍后重试"
                );
                Ok(state)
            })
        }
    };
    result.map_err(|error| CardStatus::failure(stage.get(), format!("{error:#}")))
}

struct Desktop {
    home: PathBuf,
    title: &'static str,
    page: StartupPage,
    connection: Mutex<ConnectionState>,
}

impl Desktop {
    fn run_operation(&self, app: &tauri::AppHandle, action: Operation) {
        let outcome = run_operation(app, &self.page, &self.home, &action);
        let continue_instance = match &action {
            Operation::Continue(instance) => Some(instance.as_str()),
            _ => None,
        };
        let mut current = self.connection.lock().unwrap();
        if current.exit_pending {
            drop(current);
            app.exit(0);
        } else {
            current.finish_operation(app, &self.page, self.title, continue_instance, outcome);
        }
    }

    fn submit(self: &Arc<Self>, app: tauri::AppHandle, action: Operation) {
        let desktop = self.clone();
        std::thread::spawn(move || desktop.run_operation(&app, action));
    }
}

#[tauri::command]
fn home_backend(
    window: tauri::WebviewWindow,
    desktop: tauri::State<'_, Arc<Desktop>>,
    action: String,
) -> Result<(), String> {
    if !window.url().is_ok_and(|url| desktop.page.is_home(&url)) {
        return Err("此操作只能在 Wingmate 首页执行".into());
    }
    let mut current = desktop.connection.lock().unwrap();
    if current.busy {
        return Err("后台操作正在进行".into());
    }
    let app = window.app_handle();
    let operation = match action.as_str() {
        "start" => Operation::Start,
        "continue" | "restart" => {
            let instance = current
                .pending_update
                .clone()
                .ok_or("没有待切换的后台，请重新连接")?;
            if action == "continue" {
                Operation::Continue(instance)
            } else {
                Operation::Restart(instance)
            }
        }
        _ => return Err("未知的后台操作".into()),
    };
    current.begin_operation();
    drop(current);
    desktop.inner().submit(app.clone(), operation);
    Ok(())
}

#[tauri::command]
fn manage_backend(
    window: tauri::WebviewWindow,
    desktop: tauri::State<'_, Arc<Desktop>>,
    action: String,
    instance: String,
) -> Result<(), String> {
    let origin = window
        .url()
        .map_err(|error| error.to_string())?
        .origin()
        .ascii_serialization();
    let mut current = desktop.connection.lock().unwrap();
    let state = current.workspace.as_ref().ok_or("后台尚未连接")?;
    if state.instance != instance || bridge::origin(state).as_deref() != Some(&origin) {
        return Err("后台实例已更换，请重新连接".into());
    }
    let operation = match action.as_str() {
        "stop" => Operation::Stop(instance),
        "restart" => Operation::Restart(instance),
        _ => return Err("未知的后台操作".into()),
    };
    if !current.begin_operation() {
        return Err("后台操作正在进行".into());
    }
    drop(current);
    desktop
        .inner()
        .submit(window.app_handle().clone(), operation);
    Ok(())
}

pub fn run(home: PathBuf) -> Result<()> {
    let title = if wingmate_service::IS_DEV {
        "Wingmate · 开发版"
    } else {
        "Wingmate"
    };
    let desktop = Arc::new(Desktop {
        home,
        title,
        page: StartupPage::new(),
        connection: Mutex::new(ConnectionState {
            busy: true,
            ..ConnectionState::default()
        }),
    });
    let exit_state = desktop.clone();
    tauri::Builder::default()
        .manage(desktop.clone())
        .invoke_handler(tauri::generate_handler![
            home_backend,
            manage_backend,
            titlebar::window_chrome
        ])
        .setup(move |app| {
            let page_load = desktop.clone();
            let popup_app = app.handle().clone();
            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .on_new_window(move |url, features| popup::open(&popup_app, url, features))
                    .on_page_load(move |window, event| {
                        match event.event() {
                            PageLoadEvent::Started => {
                                popup::close_all(window.app_handle());
                            }
                            PageLoadEvent::Finished => {
                                page_load.page.loaded(&window, event.url());
                            }
                        }
                        let mut current = page_load.connection.lock().unwrap();
                        if event.event() == PageLoadEvent::Started {
                            current.titlebar.start(&window);
                        }
                        let workspace = current.workspace.clone();
                        let theme = current.titlebar_theme;
                        let navigation = current.titlebar.navigation();
                        let source = current
                            .titlebar
                            .bind(event.url(), &page_load.page, workspace.as_ref())
                            .cloned();
                        drop(current);
                        if let (PageLoadEvent::Finished, Some(state)) = (event.event(), &workspace)
                        {
                            bridge::expose(&window, event.url(), state);
                        }
                        if let Some(source) = source {
                            titlebar::expose(&window, &source, theme, navigation);
                        }
                    })
                    .title(title)
                    .decorations(!cfg!(windows))
                    .visible(!cfg!(windows))
                    .inner_size(1200.0, 820.0)
                    .min_inner_size(720.0, 480.0);
            let window = if wingmate_service::IS_DEV {
                window.data_directory(desktop.home.join("webview"))
            } else {
                window
            };
            let window = window.build()?;
            titlebar::watch(&window, 0);
            titlebar::observe(&window);
            let handle = app.handle().clone();
            let popup_app = handle.clone();
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    popup::close_all(&popup_app);
                }
            });
            std::thread::spawn(move || {
                desktop.run_operation(&handle, Operation::Start);
                loop {
                    std::thread::sleep(Duration::from_secs(2));
                    let Some(window) = handle.get_webview_window("main") else {
                        break;
                    };
                    let Some(revision) = desktop.connection.lock().unwrap().observation_revision()
                    else {
                        continue;
                    };
                    let status = service::status(&desktop.home);
                    let mut current = desktop.connection.lock().unwrap();
                    // 操作开始前发出的轮询不能覆盖操作结果。
                    if current.observation_revision() == Some(revision) {
                        current.observe(&handle, &desktop.page, &window, title, status);
                    }
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())?
        .run(move |_, event| {
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                // 允许窗口关闭，等已开始的后台操作完成后再退出桌面进程。
                if exit_state.connection.lock().unwrap().defer_exit() {
                    api.prevent_exit();
                }
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ConnectionState;

    #[test]
    fn operation_invalidates_an_in_flight_observation() {
        let mut connection = ConnectionState::default();
        let revision = connection.observation_revision().unwrap();

        assert!(connection.begin_operation());
        assert!(!connection.begin_operation());
        assert_eq!(connection.observation_revision(), None);

        connection.busy = false;
        assert_ne!(connection.observation_revision().unwrap(), revision);
    }

    #[test]
    fn closing_window_defers_exit_during_backend_operation() {
        let mut connection = ConnectionState::default();
        assert!(!connection.defer_exit());
        assert!(connection.begin_operation());
        assert!(connection.defer_exit());
        assert!(connection.exit_pending);
    }
}
