//! 桌面窗口、首页与连接观察；经发行入口连接独立管理服务。
mod bridge;
mod connection_panel;
mod popup;
mod remote;
mod startup_page;
mod titlebar;
mod window_state;

use anyhow::Result;
use startup_page::{CardStatus, StartupPage};
use std::{
    cell::Cell,
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{
    webview::PageLoadEvent, AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use wingmate_launcher::{
    service::{self, HostStatus, RestartTarget, Snapshot, StartupStage},
    RUNTIME_REVISION,
};

#[derive(Default)]
enum View {
    #[default]
    Home,
    Workspace(Snapshot),
    UpdateAvailable(RestartTarget),
}

enum Observation {
    Ignore,
    KeepPage { disconnected: bool },
    Connect(HostStatus),
    ShowHome(CardStatus),
}

#[derive(Default)]
struct ConnectionState {
    busy: bool,
    active_operations: usize,
    exit_pending: bool,
    revision: u64,
    last: Option<RestartTarget>,
    service_update: bool,
    view: View,
    authorized_origins: HashSet<String>,
    titlebar_theme: titlebar::Theme,
    titlebar: titlebar::Frame,
    remote: Option<remote::Workbench>,
}

impl ConnectionState {
    fn begin_operation(&mut self) -> bool {
        if self.busy || self.exit_pending {
            return false;
        }
        self.busy = true;
        self.active_operations += 1;
        self.revision += 1;
        true
    }

    fn observation_revision(&self) -> Option<u64> {
        (!self.busy && self.remote.is_none()).then_some(self.revision)
    }

    fn defer_exit(&mut self) -> bool {
        self.exit_pending = self.active_operations > 0;
        self.exit_pending
    }

    fn operation_finished(&mut self) -> bool {
        self.active_operations -= 1;
        self.exit_pending && self.active_operations == 0
    }

    fn workspace(&self) -> Option<&Snapshot> {
        match &self.view {
            View::Workspace(state) => Some(state),
            _ => None,
        }
    }

    fn connected_workspace(
        &self,
        window: &WebviewWindow,
        instance: &str,
    ) -> Result<&Snapshot, String> {
        let origin = window
            .url()
            .map_err(|error| error.to_string())?
            .origin()
            .ascii_serialization();
        let state = self.workspace().ok_or("后台尚未连接")?;
        if state.instance != instance || bridge::origin(state).as_deref() != Some(&origin) {
            return Err("后台实例已更换，请重新连接".into());
        }
        Ok(state)
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
        host: HostStatus,
        desired_service: &str,
        continued: bool,
    ) {
        let target = RestartTarget::from_status(&host);
        self.last = Some(target.clone());
        self.service_update = host.revision != desired_service;
        let runtime_update = host
            .business
            .as_ref()
            .is_some_and(|state| state.runtime_revision.as_deref() != RUNTIME_REVISION);
        if !continued && (self.service_update || runtime_update) {
            self.view = View::UpdateAvailable(target);
            page.show(
                app,
                CardStatus::update_available(
                    self.service_update,
                    runtime_update,
                    host.business.is_some(),
                ),
            );
        } else if let Some(state) = host.business {
            if self
                .workspace()
                .is_some_and(|workspace| workspace.instance == state.instance)
            {
                return;
            }
            if let Err(error) = self.open_workspace(app, page, state) {
                self.view = View::Home;
                page.show(
                    app,
                    CardStatus::failure(StartupStage::Connecting, format!("{error:#}")),
                );
            }
        } else {
            self.view = View::Home;
            page.show(app, CardStatus::stopped());
        }
    }

    fn finish_operation(
        &mut self,
        app: &tauri::AppHandle,
        desktop: &Desktop,
        continued: bool,
        outcome: Result<Option<HostStatus>, CardStatus>,
    ) {
        let page = &desktop.page;
        match outcome {
            Ok(Some(state)) => self.connect(app, page, state, &desktop.service_revision, continued),
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
            titlebar::connection_title(&window, desktop.title, false);
        }
        self.busy = false;
    }

    fn observe(
        &mut self,
        revision: u64,
        status: Result<Option<HostStatus>>,
        desired_service: &str,
    ) -> Observation {
        // 操作开始前发出的轮询不能覆盖操作结果。
        if self.observation_revision() != Some(revision) {
            return Observation::Ignore;
        }
        match status {
            Ok(Some(host)) => {
                let phase = host.business.as_ref().map(|state| state.phase.as_str());
                if matches!(phase, Some("starting" | "stopping")) {
                    return Observation::Ignore;
                }
                if phase == Some("running") || (phase.is_none() && host.revision != desired_service)
                {
                    return if self
                        .last
                        .as_ref()
                        .is_some_and(|target| target.matches(&host))
                    {
                        Observation::KeepPage {
                            disconnected: false,
                        }
                    } else {
                        Observation::Connect(host)
                    };
                }
                if let Some(state) = host.business {
                    if matches!(self.view, View::Home) {
                        return Observation::Ignore;
                    }
                    self.view = View::Home;
                    return Observation::ShowHome(CardStatus::failure(
                        StartupStage::Connecting,
                        state.error.unwrap_or_else(|| "后台未运行".into()),
                    ));
                }
                self.observe_stopped()
            }
            Ok(None) => self.observe_stopped(),
            Err(_) if self.workspace().is_some() => {
                // 短暂断连不销毁页面和输入草稿。
                Observation::KeepPage { disconnected: true }
            }
            Err(_) => Observation::Ignore,
        }
    }

    fn observe_stopped(&mut self) -> Observation {
        self.last = None;
        match std::mem::take(&mut self.view) {
            View::Home => Observation::Ignore,
            _ => Observation::ShowHome(CardStatus::stopped()),
        }
    }
}

enum Operation {
    Start,
    Continue(RestartTarget),
    Stop(String),
    Restart(RestartTarget),
}

fn run_operation(
    app: &tauri::AppHandle,
    desktop: &Desktop,
    action: &Operation,
    ready: impl FnOnce(Result<Option<HostStatus>, CardStatus>),
) -> Result<(), CardStatus> {
    let home = &desktop.home;
    let stage = Cell::new(StartupStage::Checking);
    let report = |next| {
        stage.set(next);
        desktop.page.show_progress(app, next);
    };
    let connected = |state: &Snapshot| {
        (|| -> Result<Option<HostStatus>> {
            let host =
                service::host_status(home)?.ok_or_else(|| anyhow::anyhow!("管理服务已退出"))?;
            anyhow::ensure!(
                host.business.as_ref().is_some_and(
                    |current| current.instance == state.instance && current.phase == "running"
                ),
                "业务后台实例已更换，请重新检查"
            );
            Ok(Some(host))
        })()
        .map_err(|error| CardStatus::failure(StartupStage::Connecting, format!("{error:#}")))
    };
    let result = (|| -> Result<()> {
        match action {
            Operation::Stop(instance) => {
                report(StartupStage::Stopping);
                service::stop_instance(home, instance).map(|()| ready(Ok(None)))
            }
            Operation::Restart(target) => {
                service::restart_with_progress(home, Some(target), report, |state| {
                    ready(connected(state))
                })
                .map(drop)
            }
            Operation::Start => {
                if let Some(host) = service::host_status(home)?.filter(|host| {
                    host.business.is_none() && host.revision != desktop.service_revision
                }) {
                    ready(Ok(Some(host)));
                    Ok(())
                } else {
                    service::start_with_progress(home, report, |state| ready(connected(state)))
                        .map(drop)
                }
            }
            Operation::Continue(target) => {
                report(StartupStage::Connecting);
                let host = service::host_status(home)?
                    .ok_or_else(|| anyhow::anyhow!("管理服务已退出，请重新检查"))?;
                anyhow::ensure!(target.matches(&host), "后台实例已更换，请重新检查");
                if let Some(state) = &host.business {
                    anyhow::ensure!(state.phase == "running", "后台正在切换，请稍后重试");
                    ready(Ok(Some(host)));
                    Ok(())
                } else {
                    service::start_with_progress(home, report, |state| ready(connected(state)))
                        .map(drop)
                }
            }
        }
    })();
    result.map_err(|error| CardStatus::failure(stage.get(), format!("{error:#}")))
}

struct Desktop {
    home: PathBuf,
    service_revision: String,
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
        let continued = matches!(&action, Operation::Continue(_));
        let publish = |outcome| {
            let mut current = self.connection.lock().unwrap();
            if !current.exit_pending {
                current.finish_operation(app, self, continued, outcome);
            }
        };
        if let Err(status) = run_operation(app, self, &action, publish) {
            publish(Err(status));
        }
        if self.connection.lock().unwrap().operation_finished() {
            app.exit(0);
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
    if current.busy || current.exit_pending {
        return Err("后台操作正在进行".into());
    }
    let app = window.app_handle();
    let operation = match action.as_str() {
        "start" => Operation::Start,
        "continue" | "restart" => {
            let View::UpdateAvailable(target) = &current.view else {
                return Err("没有待切换的后台，请重新连接".into());
            };
            if action == "continue" {
                Operation::Continue(target.clone())
            } else {
                Operation::Restart(target.clone())
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
    let mut current = desktop.connection.lock().unwrap();
    current.connected_workspace(&window, &instance)?;
    let operation = match action.as_str() {
        "stop" => Operation::Stop(instance),
        "restart" => Operation::Restart(current.last.clone().ok_or("后台实例不可用，请重新连接")?),
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
    let service_revision = match std::env::var("WINGMATE_SERVICE_REVISION") {
        Ok(revision) => revision,
        Err(std::env::VarError::NotPresent) => wingmate_launcher::components::service_revision()?,
        Err(error) => return Err(error.into()),
    };
    let primary = window_state::PrimaryWindow::claim(&home)?;
    let window_label = if primary.is_some() {
        "main".into()
    } else {
        format!("temporary-{}", std::process::id())
    };
    let title = if wingmate_launcher::IS_DEV {
        "Wingmate · 开发版"
    } else {
        "Wingmate"
    };
    let mut connection = ConnectionState::default();
    connection.begin_operation();
    let desktop = Arc::new(Desktop {
        home,
        service_revision,
        window_label,
        title,
        page: StartupPage::new(),
        connection: Mutex::new(connection),
    });
    let exit_state = desktop.clone();
    tauri::Builder::default()
        .manage(desktop.clone())
        .invoke_handler(tauri::generate_handler![
            home_backend,
            manage_backend,
            bridge::open_dsh_home,
            bridge::login_startup,
            bridge::dsh_command,
            titlebar::window_chrome,
            connection_panel::open_connections,
            connection_panel::resize_connections,
            connection_panel::close_connections,
            remote::connect_remote,
            remote::return_local,
            remote::local_share
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
                        let service_update = current.service_update;
                        let theme = current.titlebar_theme;
                        let navigation = current.titlebar.navigation();
                        let source = current
                            .titlebar
                            .bind(event.url(), &page_load.page, workspace.as_ref())
                            .cloned();
                        drop(current);
                        if let (PageLoadEvent::Finished, Some(state)) = (event.event(), &workspace)
                        {
                            bridge::expose(&window, event.url(), state, service_update);
                        }
                        if let Some(source) = source {
                            titlebar::expose(&window, &source, theme, navigation);
                        }
                        if event.event() == PageLoadEvent::Finished {
                            remote::render_header(window.app_handle());
                        }
                    })
                    .title(title)
                    .decorations(!cfg!(windows))
                    .visible(false)
                    .inner_size(size.width, size.height);
            let window = if wingmate_launcher::IS_DEV {
                window.data_directory(desktop.home.join("webview"))
            } else {
                window
            };
            let window = window.build()?;
            window_state::initialize(&window, &desktop.home, primary);
            if !cfg!(windows) {
                window.show()?;
            }
            titlebar::watch(window.app_handle(), 0);
            titlebar::observe(&window);
            let handle = app.handle().clone();
            let popup_app = handle.clone();
            window.on_window_event(move |event| match event {
                tauri::WindowEvent::Destroyed => {
                    popup::close_all(&popup_app);
                    connection_panel::dismiss(&popup_app);
                }
                tauri::WindowEvent::Moved(_) => connection_panel::dismiss(&popup_app),
                tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } => {
                    connection_panel::dismiss(&popup_app);
                    let desktop = popup_app.state::<Arc<Desktop>>();
                    let view = desktop
                        .connection
                        .lock()
                        .unwrap()
                        .remote
                        .as_ref()
                        .map(|remote| remote.view.clone());
                    if let Some(view) = view {
                        let _ = remote::fit(&view.window(), &view);
                    }
                }
                _ => {}
            });
            desktop.submit(handle.clone(), Operation::Start);
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(2));
                if handle.get_webview(&desktop.window_label).is_none() {
                    break;
                }
                let Some(revision) = desktop.connection.lock().unwrap().observation_revision()
                else {
                    continue;
                };
                let Some(window) = desktop_window(&handle) else {
                    continue;
                };
                let status = service::connection_status(&desktop.home);
                let mut current = desktop.connection.lock().unwrap();
                let disconnected =
                    match current.observe(revision, status, &desktop.service_revision) {
                        Observation::Ignore => continue,
                        Observation::KeepPage { disconnected } => disconnected,
                        Observation::Connect(state) => {
                            current.connect(
                                &handle,
                                &desktop.page,
                                state,
                                &desktop.service_revision,
                                false,
                            );
                            false
                        }
                        Observation::ShowHome(status) => {
                            desktop.page.show(&handle, status);
                            false
                        }
                    };
                titlebar::connection_title(&window, title, disconnected);
            });
            Ok(())
        })
        .build(tauri::generate_context!())?
        .run(move |_, event| {
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                // 允许窗口关闭，等启停和清理完成后再退出桌面进程。
                if exit_state.connection.lock().unwrap().defer_exit() {
                    api.prevent_exit();
                }
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CardStatus, ConnectionState, HostStatus, Observation, RestartTarget, Snapshot,
        StartupStage, View,
    };

    const SERVICE: &str = "current-service-revision";

    fn host(business: Option<Snapshot>) -> HostStatus {
        HostStatus {
            product: "wingmate-service".into(),
            protocol: 1,
            instance: "service".into(),
            pid: 1,
            revision: SERVICE.into(),
            business,
        }
    }

    fn target(instance: &str) -> RestartTarget {
        RestartTarget::from_status(&host(Some(snapshot(instance, "running"))))
    }

    fn snapshot(instance: &str, phase: &str) -> Snapshot {
        Snapshot {
            product: "wingmate".into(),
            protocol: 2,
            instance: instance.into(),
            runtime: "dsh-test".into(),
            runtime_revision: None,
            dsh_home: None,
            agents_home: None,
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
            last: Some(target("current")),
            view: View::Workspace(snapshot("current", "running")),
            ..ConnectionState::default()
        };
        let revision = connection.observation_revision().unwrap();

        assert!(matches!(
            connection.observe(revision, Err(anyhow::anyhow!("connection lost")), SERVICE),
            Observation::KeepPage { disconnected: true }
        ));
        for phase in ["starting", "stopping"] {
            assert!(matches!(
                connection.observe(
                    revision,
                    Ok(Some(host(Some(snapshot("current", phase))))),
                    SERVICE
                ),
                Observation::Ignore
            ));
        }
        assert!(matches!(
            connection.observe(
                revision,
                Ok(Some(host(Some(snapshot("current", "running"))))),
                SERVICE
            ),
            Observation::KeepPage {
                disconnected: false
            }
        ));
        assert_eq!(connection.workspace().unwrap().instance, "current");
        assert!(matches!(
            connection.observe(revision, Ok(Some(host(Some(snapshot("replacement", "running"))))), SERVICE),
            Observation::Connect(state) if state.business.as_ref().is_some_and(|business| business.instance == "replacement")
        ));
    }

    #[test]
    fn an_idle_old_service_is_detected_and_a_replaced_service_requires_a_new_decision() {
        let mut connection = ConnectionState::default();
        let revision = connection.observation_revision().unwrap();
        let mut service = host(None);
        service.revision = "old-revision".into();
        connection.last = Some(RestartTarget::from_status(&service));
        service.instance = "replacement-service".into();
        assert!(matches!(
            connection.observe(revision, Ok(Some(service)), SERVICE),
            Observation::Connect(host) if host.instance == "replacement-service" && host.business.is_none()
        ));
    }

    #[test]
    fn only_confirmed_stop_forgets_the_last_instance() {
        let mut connection = ConnectionState {
            last: Some(target("current")),
            view: View::UpdateAvailable(target("current")),
            ..ConnectionState::default()
        };
        let revision = connection.observation_revision().unwrap();

        assert!(matches!(
            connection.observe(revision, Err(anyhow::anyhow!("connection lost")), SERVICE),
            Observation::Ignore
        ));
        assert!(
            matches!(connection.view, View::UpdateAvailable(ref update) if update == &target("current"))
        );
        assert!(matches!(
            connection.observe(revision, Ok(Some(host(Some(snapshot("current", "failed"))))), SERVICE),
            Observation::ShowHome(status) if status == CardStatus::failure(StartupStage::Connecting, "后台未运行".into())
        ));
        assert!(matches!(connection.view, View::Home));
        assert_eq!(connection.last, Some(target("current")));

        connection.view = View::UpdateAvailable(target("current"));
        assert!(matches!(
            connection.observe(revision, Ok(None), SERVICE),
            Observation::ShowHome(status) if status == CardStatus::stopped()
        ));
        assert!(matches!(connection.view, View::Home));
        assert_eq!(connection.last, None);
        assert!(matches!(
            connection.observe(revision, Ok(None), SERVICE),
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
            connection.observe(revision, Ok(None), SERVICE),
            Observation::Ignore
        ));

        connection.busy = false;
        connection.last = Some(target("replacement"));
        connection.view = View::Workspace(snapshot("replacement", "running"));
        assert!(matches!(
            connection.observe(revision, Ok(None), SERVICE),
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
        assert!(connection.operation_finished());
    }

    #[test]
    fn closing_window_waits_for_cleanup_after_workspace_is_available() {
        let mut connection = ConnectionState::default();
        assert!(connection.begin_operation());
        connection.busy = false;
        assert!(connection.observation_revision().is_some());
        assert!(connection.defer_exit());
        assert!(!connection.begin_operation());
        assert!(connection.operation_finished());
    }
}
