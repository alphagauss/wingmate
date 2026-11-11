use super::{bridge, titlebar::Theme, Desktop};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, State, Webview, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

const PREFIX: &str = "wingmate-connections-";
const WIDTH: f64 = 356.;
const TOP: f64 = 44.;
const MIN_HEIGHT: u32 = 120;
const MAX_HEIGHT: u32 = 560;

pub(super) fn local_entry(webview: &Webview, desktop: &Desktop) -> Result<(), String> {
    let url = webview.url().map_err(|error| error.to_string())?;
    let current = desktop.connection.lock().unwrap();
    let workspace = current.workspace().and_then(bridge::origin);
    if webview.label() != desktop.window_label
        || (!desktop.page.is_home(&url)
            && workspace.as_deref() != Some(&url.origin().ascii_serialization()))
    {
        return Err("此页面不能操作 Wingmate 连接".into());
    }
    Ok(())
}

pub(super) fn local_panel(webview: &Webview, desktop: &Desktop) -> Result<WebviewWindow, String> {
    let app = webview.app_handle();
    if webview.label().starts_with(PREFIX)
        && webview
            .url()
            .is_ok_and(|url| url == desktop.page.connections())
    {
        if let Some(panel) = app.get_webview_window(webview.label()) {
            return Ok(panel);
        }
    }
    Err("Wingmate 连接浮层已关闭".into())
}

fn panel(app: &AppHandle) -> Option<WebviewWindow> {
    app.webview_windows()
        .into_values()
        .find(|window| window.label().starts_with(PREFIX))
}

pub(super) fn dismiss(app: &AppHandle) {
    if let Some(panel) = panel(app) {
        let _ = panel.destroy();
    }
}

pub(super) fn sync_theme(app: &AppHandle, theme: Theme) {
    if let Some(panel) = panel(app) {
        let theme = serde_json::json!(theme);
        let _ = panel.eval(format!(
            "if (window.wingmateConnections) {{ window.wingmateConnections.theme = {theme}; document.documentElement.dataset.theme = {theme}; }}"
        ));
    }
}

fn open(webview: &Webview, desktop: &Desktop) -> Result<(), String> {
    local_entry(webview, desktop)?;
    let app = webview.app_handle();
    if let Some(panel) = panel(app) {
        return panel.set_focus().map_err(|error| error.to_string());
    }

    let (theme, origin) = {
        let current = desktop.connection.lock().unwrap();
        (
            current.titlebar_theme,
            current.remote.as_ref().map(|remote| remote.origin.clone()),
        )
    };
    let owner = webview.window();
    let scale = owner.scale_factor().map_err(|error| error.to_string())?;
    let size = owner.inner_size().map_err(|error| error.to_string())?;
    let position = owner.inner_position().map_err(|error| error.to_string())?;
    let right = if cfg!(windows) { 144. } else { 12. };
    let left = (size.width as f64 / scale - WIDTH - right).max(8.);
    let max_height =
        (size.height as f64 / scale - TOP - 8.).clamp(f64::from(MIN_HEIGHT), f64::from(MAX_HEIGHT));
    let args = serde_json::json!({"theme": theme, "origin": origin, "maxHeight": max_height});
    let url = desktop.page.connections();
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let label = format!("{PREFIX}{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let builder =
        WebviewWindowBuilder::new(app, &label, WebviewUrl::App("connections.html".into()))
            .title("远程连接")
            .decorations(false)
            .resizable(false)
            .skip_taskbar(true)
            .visible(false)
            .inner_size(WIDTH, 180.)
            .on_navigation(move |target| target == &url)
            .initialization_script(format!("window.wingmateConnections = {args};"));
    #[cfg(windows)]
    let builder = builder.owner_raw(owner.hwnd().map_err(|error| error.to_string())?);
    #[cfg(target_os = "macos")]
    let builder = builder.parent_raw(owner.ns_window().map_err(|error| error.to_string())?);
    #[cfg(target_os = "linux")]
    let builder =
        builder.transient_for_raw(&owner.gtk_window().map_err(|error| error.to_string())?);
    let builder = if wingmate_launcher::IS_DEV {
        builder.data_directory(desktop.home.join("webview"))
    } else {
        builder
    };
    let panel = builder.build().map_err(|error| error.to_string())?;
    let floating = panel.clone();
    panel.on_window_event(move |event| {
        if matches!(event, WindowEvent::Focused(false)) {
            let _ = floating.destroy();
        }
    });
    if let Err(error) = panel.set_position(PhysicalPosition::new(
        position.x + (left * scale).round() as i32,
        position.y + (TOP * scale).round() as i32,
    )) {
        let _ = panel.destroy();
        return Err(error.to_string());
    }
    Ok(())
}

#[tauri::command]
pub(super) async fn open_connections(webview: Webview) -> Result<(), String> {
    let (reply, result) = tokio::sync::oneshot::channel();
    let caller = webview.clone();
    webview
        .run_on_main_thread(move || {
            let desktop = caller.app_handle().state::<Arc<Desktop>>();
            let _ = reply.send(open(&caller, &desktop));
        })
        .map_err(|error| error.to_string())?;
    result.await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub(super) fn resize_connections(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    height: u32,
) -> Result<(), String> {
    let panel = local_panel(&webview, &desktop)?;
    panel
        .set_size(LogicalSize::new(
            WIDTH,
            f64::from(height.clamp(MIN_HEIGHT, MAX_HEIGHT)),
        ))
        .map_err(|error| error.to_string())?;
    if !panel.is_visible().map_err(|error| error.to_string())? {
        panel.show().map_err(|error| error.to_string())?;
        panel.set_focus().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub(super) fn close_connections(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
) -> Result<(), String> {
    local_panel(&webview, &desktop)?
        .destroy()
        .map_err(|error| error.to_string())
}
