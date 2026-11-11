use super::{bridge, connection_panel, Desktop};
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
            && url.path() == "/"
            && url.fragment().is_none()
            && url.query().is_none_or(|query| {
                query
                    .strip_prefix("auth=")
                    .is_some_and(|code| !code.is_empty() && !code.contains('&'))
            }),
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

pub(super) struct Workbench {
    pub view: Webview,
    pub origin: String,
    local: bool,
}

pub(super) fn render_header(app: &tauri::AppHandle) {
    let desktop = app.state::<Arc<Desktop>>();
    let args = {
        let current = desktop.connection.lock().unwrap();
        let Some(remote) = current.remote.as_ref() else {
            return;
        };
        serde_json::json!([
            remote.origin,
            current.titlebar_theme,
            cfg!(windows),
            remote.local
        ])
    };
    let Some(main) = app.get_webview(&desktop.window_label) else {
        return;
    };
    if connection_panel::local_entry(&main, &desktop).is_err() {
        return;
    }
    let script = include_str!("remote_header.js");
    let _ = main.eval(format!("({script})({args})"));
}

#[derive(serde::Deserialize)]
struct Identity {
    product: String,
    transport: String,
}

#[tauri::command]
pub(super) async fn connect_remote(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    link: String,
) -> Result<String, String> {
    connection_panel::local_panel(&webview, &desktop)?;
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
    let local = url.host_str() == Some("127.0.0.1") && identity.transport == "local";
    if identity.product != "wingmate" || !(identity.transport == "lan" || local) {
        return Err("此链接未提供 Wingmate 连接入口".into());
    }
    connection_panel::local_panel(&webview, &desktop)?;
    let origin_guard = url.origin();
    let app = webview.app_handle().clone();
    let main = app
        .get_webview(&desktop.window_label)
        .ok_or("主窗口已关闭")?;
    let parent = main.window();
    let revision = {
        let current = desktop.connection.lock().unwrap();
        if current.busy || current.exit_pending {
            return Err("后台操作正在进行，请稍后重试".into());
        }
        current.revision
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let label = format!("remote-workbench-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let popup_app = app.clone();
    let builder = WebviewBuilder::new(label, WebviewUrl::External(url))
        .focused(false)
        .on_navigation(move |target| target.origin() == origin_guard)
        .on_new_window(move |url, features| super::popup::open(&popup_app, url, features));
    let builder = if wingmate_launcher::IS_DEV {
        builder.data_directory(desktop.home.join("webview"))
    } else {
        builder
    };
    let rect = bounds(&parent).map_err(|error| error.to_string())?;
    let view = parent
        .add_child(builder, rect.position, rect.size)
        .map_err(|error| error.to_string())?;
    let panel_open = connection_panel::local_panel(&webview, &desktop).is_ok();
    let mut current = desktop.connection.lock().unwrap();
    if !panel_open || current.revision != revision {
        drop(current);
        let _ = view.close();
        return Err("连接已取消，请重试".into());
    }
    current.revision += 1;
    let previous = current.remote.replace(Workbench {
        view,
        origin: origin.clone(),
        local,
    });
    drop(current);
    if let Some(previous) = previous {
        let _ = previous.view.close();
    }
    render_header(&app);
    Ok(origin)
}

#[tauri::command]
pub(super) fn return_local(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
) -> Result<(), String> {
    connection_panel::local_entry(&webview, &desktop)?;
    let remote = {
        let mut current = desktop.connection.lock().unwrap();
        let Some(remote) = current.remote.take() else {
            return Ok(());
        };
        current.revision += 1;
        remote
    };
    let _ = remote.view.close();
    let _ = webview.eval("document.querySelector('[data-wingmate-remote-header]')?.remove()");
    let _ = webview.set_focus();
    Ok(())
}

#[derive(serde::Serialize)]
pub(super) struct ShareReply {
    #[serde(flatten)]
    status: ShareStatus,
    pairings: Vec<SharePairing>,
}

#[derive(serde::Serialize)]
struct SharePairing {
    url: String,
    qr: String,
}

fn lan_pairings(status: &ShareStatus) -> Result<Vec<SharePairing>> {
    use qrcode::render::svg;

    let urls = if status.pairing_urls.is_empty() {
        status.pairing_url.as_slice()
    } else {
        &status.pairing_urls
    };
    urls.iter()
        .map(|url| {
            let qr = qrcode::QrCode::new(url.as_bytes())?
                .render::<svg::Color>()
                .min_dimensions(192, 192)
                .build();
            Ok(SharePairing {
                url: url.clone(),
                qr,
            })
        })
        .collect()
}

#[tauri::command]
pub(super) async fn local_share(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    command: ShareCommand,
    instance: Option<String>,
) -> Result<ShareReply, String> {
    if connection_panel::local_panel(&webview, &desktop).is_err() {
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
        let pairings = if matches!(command, ShareCommand::Pair) {
            lan_pairings(&status)?
        } else {
            Vec::new()
        };
        Ok(ShareReply { status, pairings })
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
        assert!(target("http://127.0.0.1:17321/?auth=code").is_ok());
        for invalid in [
            "javascript:alert(1)",
            "https://example.com/",
            "http://user:pass@192.168.1.5/",
            "http://192.168.1.5/?auth=",
            "http://192.168.1.5/?auth=code&auth=other",
            "http://192.168.1.5/?token=secret",
            "http://192.168.1.5/_wingmate/pair#code",
        ] {
            assert!(target(invalid).is_err(), "{invalid}");
        }
    }
}
