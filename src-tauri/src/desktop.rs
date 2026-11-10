//! 桌面窗口、首页与连接观察；服务生命周期由独立 wingmate-service 库负责。
mod bridge;
mod popup;
mod startup_page;
mod titlebar;
mod window_state;

use anyhow::Result;
use startup_page::{CardStatus, StartupPage};
use std::{
    cell::Cell,
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{
    webview::PageLoadEvent, AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use wingmate_service::{
    service::{self, Snapshot, StartupStage},
    RUNTIME_REVISION,
};

#[derive(Default)]
enum View {
    #[default]
    Home,
    Workspace(Snapshot),
    UpdateAvailable(String),
}

enum Observation {
    Ignore,
    KeepPage { disconnected: bool },
    Connect(Snapshot),
    ShowHome(CardStatus),
}

#[derive(Default)]
struct ConnectionState {
    busy: bool,
    exit_pending: bool,
    revision: u64,
    last: Option<String>,
    view: View,
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

    fn workspace(&self) -> Option<&Snapshot> {
        match &self.view {
            View::Workspace(state) => Some(state),
            _ => None,
        }
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
        self.view = View::Workspace(state);
        page.open_workspace(app, self.workspace().unwrap());
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
            self.view = View::UpdateAvailable(state.instance);
            page.show(app, CardStatus::update_available());
        } else if let Err(error) = self.open_workspace(app, page, state) {
            self.view = View::Home;
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
                self.view = View::Home;
            }
            Err(status) => {
                page.show(app, status);
                self.view = View::Home;
            }
        }
        if let Some(window) = desktop_window(app) {
            titlebar::connection_title(&window, title, false);
        }
        self.busy = false;
    }

    fn observe(&mut self, revision: u64, status: Result<Option<Snapshot>>) -> Observation {
        // 操作开始前发出的轮询不能覆盖操作结果。
        if self.observation_revision() != Some(revision) {
            return Observation::Ignore;
        }
        match status {
            Ok(Some(state)) => match state.phase.as_str() {
                "starting" | "stopping" => Observation::Ignore,
                "running" if self.last.as_deref() != Some(&state.instance) => {
                    Observation::Connect(state)
                }
                "running" => Observation::KeepPage {
                    disconnected: false,
                },
                _ if !matches!(self.view, View::Home) => {
                    self.view = View::Home;
                    Observation::ShowHome(CardStatus::failure(
                        StartupStage::Connecting,
                        state.error.unwrap_or_else(|| "后台未运行".into()),
                    ))
                }
                _ => Observation::KeepPage {
                    disconnected: false,
                },
            },
            Ok(None) => {
                self.last = None;
                match std::mem::take(&mut self.view) {
                    View::Home => Observation::Ignore,
                    _ => Observation::ShowHome(CardStatus::stopped()),
                }
            }
            Err(_) if self.workspace().is_some() => {
                // 短暂断连不销毁页面和输入草稿。
                Observation::KeepPage { disconnected: true }
            }
            Err(_) => Observation::Ignore,
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
    window_label: String,
    title: &'static str,
    page: StartupPage,
    connection: Mutex<ConnectionState>,
}

fn desktop_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(&app.state::<Arc<Desktop>>().window_label)
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
            let View::UpdateAvailable(instance) = &current.view else {
                return Err("没有待切换的后台，请重新连接".into());
            };
            if action == "continue" {
                Operation::Continue(instance.clone())
            } else {
                Operation::Restart(instance.clone())
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
    let state = current.workspace().ok_or("后台尚未连接")?;
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
    let primary = window_state::PrimaryWindow::claim(&home)?;
    let window_label = if primary.is_some() {
        "main".into()
    } else {
        format!("temporary-{}", std::process::id())
    };
    let title = if wingmate_service::IS_DEV {
        "Wingmate · 开发版"
    } else {
        "Wingmate"
    };
    let desktop = Arc::new(Desktop {
        home,
        window_label,
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
            let label = desktop.window_label.as_str();
            let size = window_state::DEFAULT_SIZE;
            let window =
                WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
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
                        let workspace = current.workspace().cloned();
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
                    .visible(false)
                    .inner_size(size.width, size.height);
            let window = if wingmate_service::IS_DEV {
                window.data_directory(desktop.home.join("webview"))
            } else {
                window
            };
            let window = window.build()?;
            window_state::initialize(&window, &desktop.home, primary);
            if !cfg!(windows) {
                window.show()?;
            }
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
                    let Some(window) = desktop_window(&handle) else {
                        break;
                    };
                    let Some(revision) = desktop.connection.lock().unwrap().observation_revision()
                    else {
                        continue;
                    };
                    let status = service::status(&desktop.home);
                    let mut current = desktop.connection.lock().unwrap();
                    let disconnected = match current.observe(revision, status) {
                        Observation::Ignore => continue,
                        Observation::KeepPage { disconnected } => disconnected,
                        Observation::Connect(state) => {
                            current.connect(&handle, &desktop.page, state, None);
                            false
                        }
                        Observation::ShowHome(status) => {
                            desktop.page.show(&handle, status);
                            false
                        }
                    };
                    titlebar::connection_title(&window, title, disconnected);
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
    use super::{CardStatus, ConnectionState, Observation, Snapshot, StartupStage, View};

    fn snapshot(instance: &str, phase: &str) -> Snapshot {
        Snapshot {
            product: "wingmate".into(),
            protocol: 2,
            instance: instance.into(),
            runtime: "dsh-test".into(),
            runtime_revision: None,
            pid: 1,
            engine_pid: Some(1),
            phase: phase.into(),
            url: None,
            error: None,
        }
    }

    #[test]
    fn transient_disconnect_and_recovery_keep_the_current_workspace() {
        let mut connection = ConnectionState {
            last: Some("current".into()),
            view: View::Workspace(snapshot("current", "running")),
            ..ConnectionState::default()
        };
        let revision = connection.observation_revision().unwrap();

        assert!(matches!(
            connection.observe(revision, Err(anyhow::anyhow!("connection lost"))),
            Observation::KeepPage { disconnected: true }
        ));
        for phase in ["starting", "stopping"] {
            assert!(matches!(
                connection.observe(revision, Ok(Some(snapshot("current", phase)))),
                Observation::Ignore
            ));
        }
        assert!(matches!(
            connection.observe(revision, Ok(Some(snapshot("current", "running")))),
            Observation::KeepPage {
                disconnected: false
            }
        ));
        assert_eq!(connection.workspace().unwrap().instance, "current");
        assert!(matches!(
            connection.observe(revision, Ok(Some(snapshot("replacement", "running")))),
            Observation::Connect(state) if state.instance == "replacement"
        ));
    }

    #[test]
    fn only_confirmed_stop_forgets_the_last_instance() {
        let mut connection = ConnectionState {
            last: Some("current".into()),
            view: View::UpdateAvailable("current".into()),
            ..ConnectionState::default()
        };
        let revision = connection.observation_revision().unwrap();

        assert!(matches!(
            connection.observe(revision, Err(anyhow::anyhow!("connection lost"))),
            Observation::Ignore
        ));
        assert!(
            matches!(connection.view, View::UpdateAvailable(ref instance) if instance == "current")
        );
        assert!(matches!(
            connection.observe(revision, Ok(Some(snapshot("current", "failed")))),
            Observation::ShowHome(status) if status == CardStatus::failure(StartupStage::Connecting, "后台未运行".into())
        ));
        assert!(matches!(connection.view, View::Home));
        assert_eq!(connection.last.as_deref(), Some("current"));

        connection.view = View::UpdateAvailable("current".into());
        assert!(matches!(
            connection.observe(revision, Ok(None)),
            Observation::ShowHome(status) if status == CardStatus::stopped()
        ));
        assert!(matches!(connection.view, View::Home));
        assert_eq!(connection.last, None);
        assert!(matches!(
            connection.observe(revision, Ok(None)),
            Observation::Ignore
        ));
    }

    #[test]
    fn operation_invalidates_an_in_flight_observation() {
        let mut connection = ConnectionState::default();
        let revision = connection.observation_revision().unwrap();

        assert!(connection.begin_operation());
        assert!(!connection.begin_operation());
        assert_eq!(connection.observation_revision(), None);
        assert!(matches!(
            connection.observe(revision, Ok(None)),
            Observation::Ignore
        ));

        connection.busy = false;
        connection.last = Some("replacement".into());
        connection.view = View::Workspace(snapshot("replacement", "running"));
        assert!(matches!(
            connection.observe(revision, Ok(None)),
            Observation::Ignore
        ));
        assert_eq!(connection.workspace().unwrap().instance, "replacement");
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
