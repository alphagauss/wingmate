use super::{bridge, Desktop, Operation};
use anyhow::{Context, Result};
use std::{
    net::Ipv4Addr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{
    Manager, PhysicalPosition, PhysicalSize, Rect, State, Url, Webview, WebviewBuilder, WebviewUrl,
    Window,
};
use wingmate_launcher::service::{self, ShareCommand, ShareStatus};

const HEADER_HEIGHT: f64 = 40.;

fn target(link: &str) -> Result<Url> {
    let link = link.trim();
    let url = Url::parse(link).context("请输入 Wingmate 分享链接")?;
    anyhow::ensure!(
        url.scheme() == "http"
            && url
                .host_str()
                .is_some_and(|host| host.parse::<Ipv4Addr>().is_ok())
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && matches!(
                (url.path(), url.fragment()),
                ("/", None) | ("/_wingmate/pair", Some(_))
            ),
        "请输入 Wingmate 的局域网地址或配对链接"
    );
    Ok(url)
}

fn bounds(window: &Window) -> tauri::Result<Rect> {
    let size = window.inner_size()?;
    let top = (HEADER_HEIGHT * window.scale_factor()?).round() as u32;
    Ok(Rect {
        position: PhysicalPosition::new(0, top as i32).into(),
        size: PhysicalSize::new(size.width, size.height.saturating_sub(top)).into(),
    })
}

pub(super) fn fit(window: &Window, view: &Webview) -> tauri::Result<()> {
    view.set_bounds(bounds(window)?)
}

fn local_panel(webview: &Webview, desktop: &Desktop) -> Result<(), String> {
    if webview.label() != desktop.window_label
        || !webview
            .url()
            .is_ok_and(|url| url == desktop.page.connections())
    {
        return Err("此操作只能在 Wingmate 连接面板执行".into());
    }
    Ok(())
}

#[tauri::command]
pub(super) fn open_connections(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
) -> Result<(), String> {
    let url = webview.url().map_err(|error| error.to_string())?;
    {
        let mut current = desktop.connection.lock().unwrap();
        let verified_workspace = current.workspace().and_then(bridge::origin).as_deref()
            == Some(&url.origin().ascii_serialization());
        if webview.label() != desktop.window_label
            || (!desktop.page.is_home(&url) && !verified_workspace)
        {
            return Err("此页面不能打开 Wingmate 连接面板".into());
        }
        if current.busy || current.exit_pending {
            return Err("后台操作正在进行，请稍后重试".into());
        }
        current.connections_open = true;
        current.revision += 1;
    }
    if let Err(error) = webview.navigate(desktop.page.connections()) {
        desktop.connection.lock().unwrap().connections_open = false;
        return Err(error.to_string());
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct Identity {
    product: String,
    transport: String,
}

#[derive(serde::Serialize)]
pub(super) struct Remote {
    origin: String,
    transport: String,
}

#[tauri::command]
pub(super) async fn connect_remote(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    link: String,
) -> Result<Remote, String> {
    local_panel(&webview, &desktop)?;
    let revision = desktop.connection.lock().unwrap().revision;
    let url = target(&link).map_err(|error| error.to_string())?;
    let origin = url.origin().ascii_serialization();
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(4))
        .build()
        .map_err(|_| "无法初始化远程连接")?;
    let response = client
        .get(format!("{origin}/_wingmate/identity"))
        .send()
        .await
        .map_err(|_| "无法连接远程主机")?;
    let identity: Identity = response
        .error_for_status()
        .map_err(|_| "无法识别 Wingmate 主机")?
        .json()
        .await
        .map_err(|_| "无法识别 Wingmate 主机")?;
    if identity.product != "wingmate" || identity.transport != "lan" {
        return Err("此链接未提供 Wingmate 局域网入口".into());
    }
    local_panel(&webview, &desktop)?;
    let parent = webview.window();
    let origin_guard = url.origin();
    let app = webview.app_handle().clone();
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let label = format!("remote-workbench-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let builder = WebviewBuilder::new(label, WebviewUrl::External(url))
        .on_navigation(move |target| target.origin() == origin_guard)
        .on_new_window(move |url, features| super::popup::open(&app, url, features));
    let builder = if wingmate_launcher::IS_DEV {
        builder.data_directory(desktop.home.join("webview"))
    } else {
        builder
    };
    let rect = bounds(&parent).map_err(|error| error.to_string())?;
    let view = parent
        .add_child(builder, rect.position, rect.size)
        .map_err(|error| error.to_string())?;
    let mut current = desktop.connection.lock().unwrap();
    if !current.connections_open || current.revision != revision {
        drop(current);
        let _ = view.close();
        return Err("连接面板已关闭".into());
    }
    let previous = current.remote_view.replace(view);
    drop(current);
    if let Some(previous) = previous {
        let _ = previous.close();
    }
    Ok(Remote {
        origin,
        transport: identity.transport,
    })
}

#[tauri::command]
pub(super) fn show_remote(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    visible: bool,
) -> Result<(), String> {
    local_panel(&webview, &desktop)?;
    let view = desktop
        .connection
        .lock()
        .unwrap()
        .remote_view
        .clone()
        .ok_or("尚未连接远程主机")?;
    let result = if visible { view.show() } else { view.hide() };
    result.map_err(|error| error.to_string())
}

#[tauri::command]
pub(super) fn return_local(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
) -> Result<(), String> {
    local_panel(&webview, &desktop)?;
    let (view, operation) = {
        let mut current = desktop.connection.lock().unwrap();
        if !current.begin_operation() {
            return Err("后台操作正在进行，请稍后重试".into());
        }
        current.connections_open = false;
        let instance = current
            .workspace()
            .map(|state| state.instance.clone())
            .unwrap_or_default();
        (current.remote_view.take(), Operation::Continue(instance))
    };
    if let Some(view) = view {
        let _ = view.close();
    }
    desktop
        .inner()
        .submit(webview.app_handle().clone(), operation);
    Ok(())
}

#[derive(serde::Serialize)]
pub(super) struct ShareReply {
    #[serde(flatten)]
    status: ShareStatus,
    qr: Option<String>,
}

#[tauri::command]
pub(super) async fn local_share(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    command: ShareCommand,
    instance: Option<String>,
) -> Result<ShareReply, String> {
    if local_panel(&webview, &desktop).is_err() {
        let url = webview.url().map_err(|error| error.to_string())?;
        let current = desktop.connection.lock().unwrap();
        let state = current.workspace().ok_or("本机后台尚未连接")?;
        if webview.label() != desktop.window_label
            || instance.as_deref() != Some(state.instance.as_str())
            || bridge::origin(state).as_deref() != Some(&url.origin().ascii_serialization())
        {
            return Err("此页面不能管理本机分享".into());
        }
    }
    let home = desktop.home.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<ShareReply> {
        let status = service::share(&home, &command)?;
        let qr = status
            .pairing_url
            .as_ref()
            .map(|url| -> Result<String> {
                use qrcode::render::svg;
                Ok(qrcode::QrCode::new(url.as_bytes())?
                    .render::<svg::Color>()
                    .min_dimensions(192, 192)
                    .build())
            })
            .transpose()?;
        Ok(ShareReply { status, qr })
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| format!("{error:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_wingmate_lan_links() {
        assert!(target("http://192.168.1.5:17321/").is_ok());
        assert!(target("http://127.0.0.1:17321/_wingmate/pair#code").is_ok());
        for invalid in [
            "javascript:alert(1)",
            "https://example.com/",
            "http://user:pass@192.168.1.5/",
            "http://192.168.1.5/?token=secret",
            "http://192.168.1.5/_wingmate/pair",
        ] {
            assert!(target(invalid).is_err(), "{invalid}");
        }
    }
}
