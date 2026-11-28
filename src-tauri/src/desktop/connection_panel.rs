use super::{titlebar::Theme, Desktop};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use tauri::{
    webview::WebviewBuilder, AppHandle, Manager, PhysicalPosition, State, Webview, WebviewUrl,
};

const PREFIX: &str = "wingmate-connections-";

#[derive(Clone, Copy, serde::Deserialize, serde::Serialize)]
pub(super) struct Anchor {
    pub right: f64,
    pub top: f64,
}

#[cfg(target_os = "linux")]
pub(super) fn install_overlay(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    use gtk::prelude::*;

    let native = window.gtk_window()?;
    let content = window.default_vbox()?;
    let overlay = gtk::Overlay::new();
    native.remove(&content);
    overlay.add(&content);
    native.add(&overlay);
    overlay.show();
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn place_in_overlay(view: &Webview, top: i32) -> tauri::Result<()> {
    use gtk::prelude::*;

    view.with_webview(move |native| {
        let widget = native.inner();
        if let Some(content) = widget.parent().and_downcast::<gtk::Box>() {
            if let Some(overlay) = content.parent().and_downcast::<gtk::Overlay>() {
                // Tauri adds Linux child webviews to the vertical box by default.
                content.remove(&widget);
                widget.set_margin_top(top);
                overlay.add_overlay(&widget);
                if top > 0 {
                    // Leave the workbench header exposed and keep the panel above the workbench.
                    overlay.reorder_overlay(&widget, 0);
                } else {
                    widget.grab_focus();
                }
            }
        }
    })
}

pub(super) fn local_entry(webview: &Webview, desktop: &Desktop) -> Result<(), String> {
    let url = webview.url().map_err(|error| error.to_string())?;
    let current = desktop.connection.lock().unwrap();
    let workspace = current
        .workspace()
        .map(|workspace| workspace.origin.as_str());
    if webview.label() != desktop.window_label
        || (!desktop.page.is_home(&url)
            && workspace != Some(url.origin().ascii_serialization().as_str()))
    {
        return Err("此页面不能操作 Wingmate 连接".into());
    }
    Ok(())
}

pub(super) fn local_panel(webview: &Webview, desktop: &Desktop) -> Result<Webview, String> {
    let app = webview.app_handle();
    if webview.label().starts_with(PREFIX)
        && webview.window().label() == desktop.window_label
        && webview
            .url()
            .is_ok_and(|url| url == desktop.page.connections())
    {
        if let Some(panel) = app.get_webview(webview.label()) {
            return Ok(panel);
        }
    }
    Err("Wingmate 连接浮层已关闭".into())
}

pub(super) fn workspace_source(
    webview: &Webview,
    desktop: &Desktop,
    instance: &str,
) -> Result<(), String> {
    let url = webview.url().map_err(|error| error.to_string())?;
    let current = desktop.connection.lock().unwrap();
    let workspace = current.workspace().ok_or("本机后台尚未连接")?;
    if webview.label() != desktop.window_label
        || url.path() == "/_wingmate/desktop"
        || workspace.state.instance != instance
        || workspace.origin != url.origin().ascii_serialization()
    {
        return Err("此页面不能操作本机 Wingmate 连接".into());
    }
    Ok(())
}

fn panel(app: &AppHandle) -> Option<Webview> {
    app.webviews()
        .into_values()
        .find(|view| view.label().starts_with(PREFIX))
}

fn close_panel(panel: &Webview, desktop: &Desktop) -> tauri::Result<()> {
    panel.close()?;
    super::remote::cancel_scan(desktop, panel.label(), None);
    Ok(())
}

pub(super) fn dismiss(app: &AppHandle) {
    if let Some(panel) = panel(app) {
        let _ = close_panel(&panel, &app.state::<Arc<Desktop>>());
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

pub(super) fn open(
    webview: &Webview,
    desktop: &Desktop,
    anchor: Option<Anchor>,
) -> Result<(), String> {
    open_panel(webview, desktop, anchor, None)
}

fn open_panel(
    webview: &Webview,
    desktop: &Desktop,
    anchor: Option<Anchor>,
    password_instance: Option<&str>,
) -> Result<(), String> {
    // Callers run off the UI thread so WebView2 can finish creating the child.
    static OPEN: Mutex<()> = Mutex::new(());
    let _open = OPEN.lock().unwrap();
    let authorize = || match password_instance {
        Some(instance) => workspace_source(webview, desktop, instance),
        None => local_entry(webview, desktop),
    };
    authorize()?;
    let app = webview.app_handle();
    if let Some(panel) = panel(app) {
        if password_instance.is_none() {
            return panel.set_focus().map_err(|error| error.to_string());
        }
        close_panel(&panel, desktop).map_err(|error| error.to_string())?;
    }

    let (theme, address) = {
        let current = desktop.connection.lock().unwrap();
        (
            current.titlebar_theme,
            current
                .remote
                .as_ref()
                .map(|remote| remote.display_address().to_owned()),
        )
    };
    let owner = webview.window();
    let size = owner.inner_size().map_err(|error| error.to_string())?;
    let anchor = anchor.unwrap_or(Anchor {
        right: 12.,
        top: 40.,
    });
    let args = serde_json::json!({
        "theme": theme, "address": address, "anchor": anchor,
        "passwordOnly": password_instance.is_some(),
    });
    let url = desktop.page.connections();
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let label = format!("{PREFIX}{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let builder = WebviewBuilder::new(label, WebviewUrl::App("connections.html".into()))
        .transparent(true)
        .on_navigation(move |target| target == &url)
        .initialization_script(format!("window.wingmateConnections = {args};"));
    #[cfg(target_os = "linux")]
    let builder = builder.focused(false);
    #[cfg(not(target_os = "linux"))]
    let builder = builder.auto_resize();
    let builder = if wingmate_launcher::IS_DEV {
        builder.data_directory(desktop.home.join("webview"))
    } else {
        builder
    };
    let panel = owner
        .add_child(builder, PhysicalPosition::new(0, 0), size)
        .map_err(|error| error.to_string())?;
    if let Err(error) = authorize() {
        let _ = close_panel(&panel, desktop);
        return Err(error);
    }
    #[cfg(target_os = "linux")]
    if let Err(error) = place_in_overlay(&panel, 0) {
        let _ = close_panel(&panel, desktop);
        return Err(error.to_string());
    }
    Ok(())
}

#[tauri::command]
pub(super) async fn open_connections(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    anchor: Option<Anchor>,
) -> Result<(), String> {
    open(&webview, &desktop, anchor)
}

#[tauri::command]
pub(super) async fn open_lan_password(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    instance: String,
) -> Result<(), String> {
    open_panel(&webview, &desktop, None, Some(&instance))
}

#[tauri::command]
pub(super) fn close_connections(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    restore_focus: bool,
) -> Result<(), String> {
    let panel = local_panel(&webview, &desktop)?;
    close_panel(&panel, &desktop).map_err(|error| error.to_string())?;
    if !restore_focus {
        return Ok(());
    }
    let workbench = desktop
        .connection
        .lock()
        .unwrap()
        .remote
        .as_ref()
        .map(|remote| remote.view.clone());
    if let Some(view) =
        workbench.or_else(|| webview.app_handle().get_webview(&desktop.window_label))
    {
        let _ = view.set_focus();
    }
    Ok(())
}

#[tauri::command]
pub(super) fn open_tunnel_settings(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
) -> Result<(), String> {
    let panel = local_panel(&webview, &desktop)?;
    let main = webview
        .app_handle()
        .get_webview(&desktop.window_label)
        .ok_or("主窗口已关闭")?;
    let origin = desktop
        .connection
        .lock()
        .unwrap()
        .workspace()
        .map(|workspace| workspace.origin.clone())
        .ok_or("请先启动本机 DSH，再配置具名隧道")?;
    if main
        .url()
        .map_err(|error| error.to_string())?
        .origin()
        .ascii_serialization()
        != origin
    {
        return Err("请先打开本机工作台，再配置具名隧道".into());
    }
    super::remote::return_local(main.clone(), desktop.clone())?;
    main.eval(format!(
        "if (location.origin === {} && window.__WINGMATE_BACKEND__) window.dispatchEvent(new Event('wingmate:tunnel-settings'));",
        serde_json::json!(origin)
    )).map_err(|error| error.to_string())?;
    let _ = close_panel(&panel, &desktop);
    Ok(())
}
