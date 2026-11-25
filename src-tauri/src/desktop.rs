//! 桌面窗口、首页与连接观察；经发行入口连接独立管理服务。
mod bridge;
mod connection_panel;
mod popup;
mod remote;
mod startup_page;
mod titlebar;
mod window_state;

use anyhow::{Context, Result};
use startup_page::{CardStatus, StartupPage};
use std::{
    cell::Cell,
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{
    webview::PageLoadEvent, AppHandle, Manager, Url, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use wingmate_launcher::{
    service::{self, HostStatus, RestartTarget, Snapshot, StartupStage},
    RUNTIME_REVISION,
};

#[cfg(all(target_os = "macos", feature = "custom-protocol"))]
fn set_dock_icon() -> Result<()> {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let main_thread =
        MainThreadMarker::new().ok_or_else(|| anyhow::anyhow!("Dock 图标必须在主线程设置"))?;
    let data = NSData::with_bytes(include_bytes!("../icons/icon.png"));
    let icon = NSImage::initWithData(NSImage::alloc(), &data)
        .ok_or_else(|| anyhow::anyhow!("无法加载 Wingmate Dock 图标"))?;
    unsafe { NSApplication::sharedApplication(main_thread).setApplicationIconImage(Some(&icon)) };
    Ok(())
}

#[derive(Default)]
enum View {
    #[default]
    Home,
    Workspace(Workspace),
    UpdateAvailable(Option<RestartTarget>),
    ReopenFailed,
}

#[derive(Clone)]
struct Workspace {
    state: Snapshot,
    origin: String,
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
    desktop_update: bool,
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

    fn workspace(&self) -> Option<&Workspace> {
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
        if state.state.instance != instance || state.origin != origin {
            return Err("后台实例已更换，请重新连接".into());
        }
        Ok(&state.state)
    }

    fn open_workspace(
        &mut self,
        app: &tauri::AppHandle,
        desktop: &Desktop,
        service_instance: &str,
        state: Snapshot,
    ) -> Result<()> {
        let link = service::desktop_session(&desktop.home, service_instance, &state.instance)?;
        let url = Url::parse(&link).context("后台未提供可用的工作台地址")?;
        anyhow::ensure!(
            url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.port().is_some(),
            "本机工作台地址无效"
        );
        let origin = url.origin().ascii_serialization();
        if !self.authorized_origins.contains(&origin) {
            bridge::authorize(app, &origin)?;
            self.authorized_origins.insert(origin.clone());
        }
        self.view = View::Workspace(Workspace { state, origin });
        desktop.page.open_workspace(app, url);
        Ok(())
    }

    fn connect(
        &mut self,
        app: &tauri::AppHandle,
        desktop: &Desktop,
        host: HostStatus,
        continued: bool,
    ) {
        let page = &desktop.page;
        self.last = Some(RestartTarget::from_status(&host));
        self.service_update = host.revision != desktop.service_revision;
        let runtime_update = host
            .business
            .as_ref()
            .is_some_and(|state| state.runtime_revision != desktop.runtime_revision);
        let backend_update = self.service_update || runtime_update;
        if !continued && (backend_update || self.desktop_update) {
            let target = self.last.clone().filter(|_| backend_update);
            self.view = View::UpdateAvailable(target.clone());
            page.show(
                app,
                CardStatus::update_available(
                    self.service_update,
                    runtime_update,
                    self.desktop_update,
                    target,
                ),
            );
        } else if let Some(state) = host.business {
            if self.workspace().is_some_and(|workspace| {
                workspace.state.instance == state.instance
                    && Some(&workspace.origin) == web_origin(&host.web_address).as_ref()
            }) {
                return;
            }
            if let Err(error) = self.open_workspace(app, desktop, &host.instance, state) {
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
        if continued && outcome.is_ok() {
            self.desktop_update = false;
        }
        match outcome {
            Ok(Some(state)) => self.connect(app, desktop, state, continued),
            Ok(None) => {
                self.last = None;
                if self.desktop_update {
                    self.view = View::UpdateAvailable(None);
                    page.show(app, CardStatus::update_available(false, false, true, None));
                } else {
                    self.view = View::Home;
                    page.show(app, CardStatus::stopped());
                }
            }
            Err(status) => {
                self.view = if status.can_retry_reopen() {
                    View::ReopenFailed
                } else {
                    View::Home
                };
                page.show(app, status);
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
        if self.observation_revision() != Some(revision) || matches!(self.view, View::ReopenFailed)
        {
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
                        && self.workspace().is_none_or(|workspace| {
                            Some(&workspace.origin) == web_origin(&host.web_address).as_ref()
                        }) {
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
        if matches!(self.view, View::UpdateAvailable(None)) {
            return Observation::Ignore;
        }
        if self.desktop_update {
            self.view = View::UpdateAvailable(None);
            return Observation::ShowHome(CardStatus::update_available(false, false, true, None));
        }
        match std::mem::take(&mut self.view) {
            View::Home => Observation::Ignore,
            _ => Observation::ShowHome(CardStatus::stopped()),
        }
    }
}

fn web_origin(address: &Option<std::net::SocketAddrV4>) -> Option<String> {
    address.map(|address| format!("http://{address}"))
}

enum Operation {
    Start,
    Inspect,
    Continue(Option<RestartTarget>),
    Stop(String),
    RestartDsh(String),
    Update(Option<RestartTarget>, Option<service::ToolchainBusy>),
}

fn run_operation(
    app: &tauri::AppHandle,
    desktop: &Desktop,
    action: &Operation,
    ready: impl FnOnce(Result<Option<HostStatus>, CardStatus>),
) -> Result<(), CardStatus> {
    let home = &desktop.home;
    let stage = Cell::new(StartupStage::Checking);
    let reopening = Cell::new(false);
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
            Operation::RestartDsh(instance) => {
                report(StartupStage::Stopping);
                service::restart_instance(home, instance).map(|host| ready(Ok(Some(host))))
            }
            Operation::Update(target, blocked) => {
                let approval = blocked
                    .as_ref()
                    .map_or(service::ToolchainApproval::None, |busy| {
                        service::ToolchainApproval::Listed(&busy.0)
                    });
                if target.is_some() || wingmate_launcher::IS_DEV {
                    service::restart_with_toolchain_approval(
                        home,
                        target.as_ref(),
                        approval,
                        report,
                        |state| {
                            if wingmate_launcher::IS_DEV {
                                ready(connected(state));
                            }
                        },
                    )?;
                }
                if wingmate_launcher::IS_DEV {
                    return Ok(());
                }
                reopening.set(true);
                desktop
                    .page
                    .show(app, CardStatus::reopening(target.is_some()));
                service::request_desktop_reopen(home)
            }
            Operation::Inspect | Operation::Continue(None) => {
                ready(Ok(service::connection_status(home)?));
                Ok(())
            }
            Operation::Start => {
                let host = service::host_status(home)?;
                if host.as_ref().is_some_and(|host| {
                    host.business.is_none() && host.revision != desktop.service_revision
                }) {
                    ready(Ok(host));
                    Ok(())
                } else {
                    service::start_with_progress(home, report, |state| ready(connected(state)))
                        .map(drop)
                }
            }
            Operation::Continue(Some(target)) => {
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
    result.map_err(|error| {
        let detail = format!("{error:#}");
        if reopening.get() {
            CardStatus::reopen_failure(detail)
        } else if let Some(blocked) = error.downcast_ref::<service::ToolchainBusy>() {
            match service::host_status(home) {
                Ok(Some(host)) if host.business.is_none() => {
                    CardStatus::toolchain_busy(RestartTarget::from_status(&host), blocked.clone())
                }
                _ => CardStatus::failure(stage.get(), detail),
            }
        } else {
            CardStatus::failure(stage.get(), detail)
        }
    })
}

struct Desktop {
    home: PathBuf,
    service_revision: String,
    runtime_revision: Option<String>,
    window_label: String,
    title: &'static str,
    page: StartupPage,
    connection: Mutex<ConnectionState>,
    lan_scan: Mutex<Option<remote::LanScan>>,
}

fn desktop_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(&app.state::<Arc<Desktop>>().window_label)
}

impl Desktop {
    fn run_operation(&self, app: &tauri::AppHandle, action: Operation) {
        let continued = matches!(&action, Operation::Continue(_) | Operation::RestartDsh(_));
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
    expected: Option<RestartTarget>,
    blocked: Option<service::ToolchainBusy>,
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
        "force-toolchain" => {
            let target = expected.ok_or("缺少待更新后台的身份")?;
            let blocked = blocked
                .filter(|blocked| desktop.page.confirms_toolchain(&target, blocked))
                .ok_or("占用进程已变化，请重新确认")?;
            Operation::Update(Some(target), Some(blocked))
        }
        "retry-reopen" if matches!(current.view, View::ReopenFailed) => {
            Operation::Update(None, None)
        }
        "continue" | "restart" | "update-desktop" => {
            let View::UpdateAvailable(target) = &current.view else {
                return Err("没有待切换的后台，请重新连接".into());
            };
            match action.as_str() {
                "continue" => Operation::Continue(target.clone()),
                "restart" if target.is_some() && *target == expected => {
                    Operation::Update(expected, None)
                }
                "update-desktop" if target.is_none() => Operation::Update(None, None),
                _ => return Err("待更新组件已变化，请重新确认".into()),
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
    let state = current.connected_workspace(&window, &instance)?;
    let operation = match action.as_str() {
        "stop" => Operation::Stop(instance),
        "restart_dsh" => Operation::RestartDsh(instance),
        // Installed older plugins use "restart" for the launcher operation.
        "restart" | "update" => {
            if action == "update"
                && !current.service_update
                && state.runtime_revision == desktop.runtime_revision
            {
                return Err("没有待切换的后台组件".into());
            }
            Operation::Update(
                Some(current.last.clone().ok_or("后台实例不可用，请重新连接")?),
                None,
            )
        }
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
    let runtime_revision = std::env::var("WINGMATE_TARGET_RUNTIME_REVISION")
        .ok()
        .filter(|revision| !revision.is_empty())
        .or_else(|| RUNTIME_REVISION.map(str::to_owned));
    let desktop_action = std::env::var("WINGMATE_DESKTOP_ACTION").unwrap_or_default();
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
    let mut connection = ConnectionState {
        desktop_update: desktop_action == "update",
        ..ConnectionState::default()
    };
    connection.begin_operation();
    let context = tauri::generate_context!();
    let dev_url = context
        .config()
        .build
        .dev_url
        .as_ref()
        .filter(|_| tauri::is_dev());
    let desktop = Arc::new(Desktop {
        home,
        service_revision,
        runtime_revision,
        window_label,
        title,
        page: StartupPage::new(dev_url),
        connection: Mutex::new(connection),
        lan_scan: Mutex::default(),
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
            connection_panel::open_lan_password,
            connection_panel::close_connections,
            remote::connect_remote,
            remote::scan_lan,
            remote::cancel_lan_scan,
            remote::connection_history,
            connection_panel::open_tunnel_settings,
            remote::return_local,
            remote::local_share
        ])
        .setup(move |app| {
            #[cfg(all(target_os = "macos", feature = "custom-protocol"))]
            set_dock_icon()?;
            let page_load = desktop.clone();
            let popup_app = app.handle().clone();
            let label = desktop.window_label.as_str();
            let size = window_state::DEFAULT_SIZE;
            let home_url = serde_json::to_string(desktop.page.url().as_str())?;
            let window =
                WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
                    .initialization_script(format!(
                        "if (window === window.top && location.href === {home_url}) window.__WINGMATE_LOCAL_HOME__ = true;"
                    ))
                    .on_new_window(move |url, features| popup::open(&popup_app, url, features))
                    .on_page_load(move |window, event| {
                        match event.event() {
                            PageLoadEvent::Started => {
                                popup::close_all(window.app_handle());
                                connection_panel::dismiss(window.app_handle());
                                remote::cancel_scan(&page_load, window.label(), None);
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
                        // WebView2 can evaluate on the outgoing document when navigation starts.
                        let source = if event.event() == PageLoadEvent::Finished
                            && event.url().path() != "/_wingmate/desktop"
                        {
                            current
                                .titlebar
                                .bind(event.url(), &page_load.page, workspace.as_ref())
                                .cloned()
                        } else {
                            None
                        };
                        drop(current);
                        if let (PageLoadEvent::Finished, Some(state)) = (event.event(), &workspace)
                        {
                            let runtime_update =
                                state.state.runtime_revision != page_load.runtime_revision;
                            bridge::expose(&window, event.url(), state, service_update, runtime_update);
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
            #[cfg(target_os = "macos")]
            let window = window
                .title_bar_style(tauri::TitleBarStyle::Transparent)
                .hidden_title(true);
            let window = if wingmate_launcher::IS_DEV {
                window.data_directory(desktop.home.join("webview"))
            } else {
                window
            };
            let window = window.build()?;
            #[cfg(target_os = "linux")]
            {
                connection_panel::install_overlay(&window)?;
                titlebar::install_native(&window)?;
            }
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
                #[cfg(not(target_os = "linux"))]
                tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } => {
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
            let startup = match desktop_action.as_str() {
                "update" | "reopen" => Operation::Inspect,
                _ => Operation::Start,
            };
            desktop.submit(handle.clone(), startup);
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(2));
                let Some(window) = desktop_window(&handle) else {
                    break;
                };
                if !wingmate_launcher::IS_DEV
                    && wingmate_launcher::components::desktop_reopen_requested(&desktop.home)
                        .unwrap_or(false)
                {
                    let _ = window.close();
                    break;
                }
                let Some(revision) = desktop.connection.lock().unwrap().observation_revision()
                else {
                    continue;
                };
                let status = service::connection_status(&desktop.home);
                let mut current = desktop.connection.lock().unwrap();
                let disconnected =
                    match current.observe(revision, status, &desktop.service_revision) {
                        Observation::Ignore => continue,
                        Observation::KeepPage { disconnected } => disconnected,
                        Observation::Connect(state) => {
                            current.connect(&handle, &desktop, state, false);
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
        .build(context)?
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
        StartupStage, View, Workspace,
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
            web_address: Some("127.0.0.1:8749".parse().unwrap()),
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

    fn workspace(instance: &str) -> View {
        View::Workspace(Workspace {
            state: snapshot(instance, "running"),
            origin: "http://127.0.0.1:8749".into(),
        })
    }

    #[test]
    fn transient_disconnect_and_recovery_keep_the_current_workspace() {
        let mut connection = ConnectionState {
            last: Some(target("current")),
            view: workspace("current"),
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
        assert_eq!(connection.workspace().unwrap().state.instance, "current");
        let mut fallback = host(Some(snapshot("current", "running")));
        fallback.web_address = Some("127.0.0.1:8747".parse().unwrap());
        assert!(matches!(
            connection.observe(revision, Ok(Some(fallback)), SERVICE),
            Observation::Connect(_)
        ));
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
            view: View::UpdateAvailable(Some(target("current"))),
            ..ConnectionState::default()
        };
        let revision = connection.observation_revision().unwrap();

        assert!(matches!(
            connection.observe(revision, Err(anyhow::anyhow!("connection lost")), SERVICE),
            Observation::Ignore
        ));
        assert!(
            matches!(connection.view, View::UpdateAvailable(Some(ref update)) if update == &target("current"))
        );
        assert!(matches!(
            connection.observe(revision, Ok(Some(host(Some(snapshot("current", "failed"))))), SERVICE),
            Observation::ShowHome(status) if status == CardStatus::failure(StartupStage::Connecting, "后台未运行".into())
        ));
        assert!(matches!(connection.view, View::Home));
        assert_eq!(connection.last, Some(target("current")));

        connection.view = View::UpdateAvailable(Some(target("current")));
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
        connection.view = workspace("replacement");
        assert!(matches!(
            connection.observe(revision, Ok(None), SERVICE),
            Observation::Ignore
        ));
        assert_eq!(
            connection.workspace().unwrap().state.instance,
            "replacement"
        );
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
