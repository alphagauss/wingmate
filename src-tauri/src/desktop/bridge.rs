use super::Desktop;
use std::sync::Arc;
use tauri::{ipc::CapabilityBuilder, AppHandle, Manager, Url, WebviewWindow};
use wingmate_launcher::integration::{dsh_command as cli, login_startup as startup};
use wingmate_launcher::service::Snapshot;

#[tauri::command]
pub(super) fn open_dsh_home(
    window: WebviewWindow,
    desktop: tauri::State<'_, Arc<Desktop>>,
    instance: String,
) -> Result<(), String> {
    let path = desktop
        .connection
        .lock()
        .unwrap()
        .connected_workspace(&window, &instance)?
        .dsh_home
        .clone()
        .filter(|path| path.is_absolute() && path.is_dir())
        .ok_or("DSH 数据目录不存在或不可用")?;
    open::that(path).map_err(|error| format!("无法打开 DSH 数据目录：{error}"))
}

#[tauri::command]
pub(super) fn login_startup(
    window: WebviewWindow,
    desktop: tauri::State<'_, Arc<Desktop>>,
    instance: String,
    enabled: Option<bool>,
) -> Result<startup::Status, String> {
    desktop
        .connection
        .lock()
        .unwrap()
        .connected_workspace(&window, &instance)?;
    startup::configure(enabled).map_err(|error| format!("无法访问登录启动项：{error:#}"))
}

#[tauri::command]
pub(super) async fn dsh_command(
    window: WebviewWindow,
    desktop: tauri::State<'_, Arc<Desktop>>,
    instance: String,
    enabled: Option<bool>,
) -> Result<cli::Status, String> {
    let state = desktop
        .connection
        .lock()
        .unwrap()
        .connected_workspace(&window, &instance)?
        .clone();
    let home = desktop.home.clone();
    tauri::async_runtime::spawn_blocking(move || cli::configure(&home, Some(state), enabled))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| format!("无法配置终端命令：{error:#}"))
}

pub(super) fn origin(state: &Snapshot) -> Option<String> {
    let url = Url::parse(state.url.as_deref()?).ok()?;
    Some(url.origin().ascii_serialization())
}

pub(super) fn authorize(app: &AppHandle, origin: &str) -> tauri::Result<()> {
    app.add_capability(
        CapabilityBuilder::new("wingmate-workspace")
            .webview(&app.state::<Arc<Desktop>>().window_label)
            .local(false)
            .remote(format!("{origin}/*"))
            .permission("allow-manage-backend")
            .permission("allow-backend-settings")
            .permission("allow-window-chrome")
            .permission("allow-open-connections"),
    )
}

pub(super) fn expose(window: &WebviewWindow, url: &Url, state: &Snapshot) {
    let Some(origin) = origin(state) else { return };
    if url.origin().ascii_serialization() != origin {
        return;
    }
    let version = state.runtime.strip_prefix("dsh-").unwrap_or(&state.runtime);
    let data = serde_json::json!([
        origin,
        state.instance,
        version,
        state.url,
        state.dsh_home,
        state.runtime_revision.as_deref() != super::RUNTIME_REVISION,
        startup::SUPPORTED,
        cli::SUPPORTED
    ]);
    // Old settings pages only know the version field.
    // Recheck in the document: navigation can change between PageLoad and eval.
    let _ = window.eval(&format!(
        r#"(([origin, instance, dshVersion, webUrl, dshHome, runtimeUpdate, canRegisterStartup, canRegisterDsh]) => {{
            if (window !== window.top || location.origin !== origin || window.__WINGMATE_BACKEND__) return;
            const invoke = window.__TAURI_INTERNALS__.invoke;
            Object.defineProperty(window, '__WINGMATE_BACKEND__', {{ value: Object.freeze({{
                version: runtimeUpdate ? `${{dshVersion}} · 新运行包将在重启后台后启用` : dshVersion,
                dshVersion,
                runtimeUpdate,
                dshHome,
                openDshHome: dshHome ? () => invoke('open_dsh_home', {{ instance }}) : undefined,
                loginStartup: canRegisterStartup ? enabled => invoke('login_startup', {{ instance, enabled }}) : undefined,
                dshCommand: canRegisterDsh ? enabled => invoke('dsh_command', {{ instance, enabled }}) : undefined,
                copyWebLink: () => navigator.clipboard.writeText(webUrl),
                manage: action => invoke('manage_backend', {{ action, instance }}),
            }}) }});
            window.dispatchEvent(new Event('wingmate:backend-bridge'));
        }})({data})"#
    ));
}
