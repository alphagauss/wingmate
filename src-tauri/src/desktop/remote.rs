use super::{connection_panel, Desktop};
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

#[path = "remote_lan.rs"]
mod lan;

pub(super) struct LanScan {
    panel: String,
    id: u32,
    cancel: tokio::sync::oneshot::Sender<()>,
}

#[tauri::command]
pub(super) async fn scan_lan(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    saved: Vec<String>,
    scan: u32,
) -> Result<bool, String> {
    connection_panel::local_panel(&webview, &desktop)?;
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    *desktop.lan_scan.lock().unwrap() = Some(LanScan {
        panel: webview.label().into(),
        id: scan,
        cancel,
    });
    let result = tokio::select! {
        _ = cancelled => Ok(false),
        result = lan::scan(&webview, &desktop, saved, scan) => result.map_err(|error| error.to_string()),
    };
    cancel_scan(&desktop, webview.label(), Some(scan));
    result
}

pub(super) fn cancel_scan(desktop: &Desktop, panel: &str, id: Option<u32>) {
    let mut active = desktop.lan_scan.lock().unwrap();
    if active
        .as_ref()
        .is_some_and(|scan| scan.panel == panel && id.is_none_or(|id| scan.id == id))
    {
        if let Some(scan) = active.take() {
            let _ = scan.cancel.send(());
        }
    }
}

#[tauri::command]
pub(super) fn cancel_lan_scan(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    scan: u32,
) -> Result<(), String> {
    connection_panel::local_panel(&webview, &desktop)?;
    cancel_scan(&desktop, webview.label(), Some(scan));
    Ok(())
}

const HEADER_HEIGHT: f64 = 40.;

fn target(link: &str) -> Result<Url> {
    let link = link.trim();
    let url = Url::parse(link).context("请输入 Wingmate 分享链接")?;
    anyhow::ensure!(
        ((url.scheme() == "http"
            && url
                .host_str()
                .is_some_and(|host| host.parse::<Ipv4Addr>().is_ok()))
            || (url.scheme() == "https" && url.domain().is_some()))
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.fragment().is_none()
            && url.query().is_none_or(|query| {
                query
                    .strip_prefix("auth=")
                    .is_some_and(|code| !code.is_empty() && !code.contains('&'))
            }),
        "请输入 Wingmate 的连接地址或配对链接"
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
    transport: String,
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
            remote.transport
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
    name: Option<String>,
    #[serde(default)]
    password: bool,
}

fn client(lan: bool) -> Result<reqwest::Client> {
    let builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(4));
    Ok(if lan { builder.no_proxy() } else { builder }.build()?)
}

async fn probe(client: &reqwest::Client, url: &Url, timeout: Duration) -> Option<Identity> {
    let mut response = client
        .get(url.join("/_wingmate").ok()?)
        .timeout(timeout)
        .send()
        .await
        .ok()?;
    if response.status() != reqwest::StatusCode::OK {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len() + chunk.len() > 2048 {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    let identity: Identity = serde_json::from_slice(&body).ok()?;
    let allowed = match (url.scheme(), identity.transport.as_str()) {
        ("http", "local") => url.host_str() == Some("127.0.0.1"),
        ("http", "lan") | ("https", "public") => true,
        _ => false,
    };
    (identity.product == "wingmate" && allowed).then_some(identity)
}

#[derive(serde::Serialize)]
pub(super) struct ConnectionReply {
    origin: String,
    needs_password: bool,
    needs_pairing: bool,
}

async fn with_cookie_store<T: Send + 'static>(
    panel: &Webview,
    desktop: &Desktop,
    use_store: impl FnOnce(Webview) -> tauri::Result<T> + Send + 'static,
) -> Result<T> {
    let app = panel.app_handle().clone();
    let handle = app.clone();
    let panel_label = panel.label().to_owned();
    let main_label = desktop.window_label.clone();
    let (reply, result) = tokio::sync::oneshot::channel();
    // Resolve the windows on the UI thread so the cookie getter cannot race a destroyed panel.
    app.run_on_main_thread(move || {
        let value = (|| {
            handle
                .get_webview_window(&panel_label)
                .context("Wingmate 连接浮层已关闭")?;
            let main = handle.get_webview(&main_label).context("主窗口已关闭")?;
            Ok::<_, anyhow::Error>(use_store(main)?)
        })();
        let _ = reply.send(value);
    })?;
    result.await?
}

async fn lan_session(
    webview: &Webview,
    desktop: &Desktop,
    client: &reqwest::Client,
    url: &Url,
    password: Option<String>,
) -> Result<bool> {
    use reqwest::{header, StatusCode};
    use tauri::webview::Cookie;

    let host = url.host_str().context("缺少局域网主机地址")?;
    let cookie_name = format!(
        "wingmate_share_{}",
        url.port_or_known_default().unwrap_or(80)
    );
    if let Some(password) = password {
        let reply = client
            .post(url.join("/_wingmate/login")?)
            .header(header::ORIGIN, url.origin().ascii_serialization())
            .body(password)
            .send()
            .await?;
        match reply.status() {
            StatusCode::NO_CONTENT => {}
            StatusCode::UNAUTHORIZED => anyhow::bail!("密码错误，请重试"),
            StatusCode::TOO_MANY_REQUESTS => anyhow::bail!("尝试次数过多，请稍后重试"),
            _ => anyhow::bail!("无法登录主机，请确认已启用局域网密码"),
        }
        let value = reply
            .headers()
            .get(header::SET_COOKIE)
            .context("主机未返回会话")?
            .to_str()?;
        let cookie = Cookie::parse(value)?;
        anyhow::ensure!(
            cookie.name() == cookie_name
                && cookie.value().len() == 36
                && cookie
                    .value()
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() || byte == b'-'),
            "主机返回的会话无效"
        );
        let cookie = Cookie::build((cookie_name, cookie.value().to_owned()))
            .domain(host.to_owned())
            .path("/")
            .http_only(true)
            .same_site(cookie::SameSite::Strict)
            .max_age(cookie::time::Duration::days(30))
            .build();
        with_cookie_store(webview, desktop, move |main| main.set_cookie(cookie)).await?;
        return Ok(true);
    }
    let cookie_url = url.clone();
    let cookie = with_cookie_store(webview, desktop, move |main| {
        Ok(main
            .cookies_for_url(cookie_url)?
            .into_iter()
            .find(|cookie| cookie.name() == cookie_name))
    })
    .await?;
    let Some(cookie) = cookie else {
        return Ok(false);
    };
    Ok(client
        .get(url.join("/_wingmate/session")?)
        .header(
            header::COOKIE,
            format!("{}={}", cookie.name(), cookie.value()),
        )
        .send()
        .await?
        .status()
        == StatusCode::NO_CONTENT)
}

#[tauri::command]
pub(super) async fn connect_remote(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    link: String,
    password: Option<String>,
) -> Result<ConnectionReply, String> {
    connection_panel::local_panel(&webview, &desktop)?;
    let mut url = target(&link).map_err(|error| error.to_string())?;
    let client = client(url.scheme() == "http").map_err(|_| "无法初始化远程连接")?;
    let mut identity = probe(&client, &url, Duration::from_secs(4)).await;
    if identity.is_none() && password.is_none() && url.query().is_none() && url.scheme() == "http" {
        for candidate in lan::alternatives(&url.clone()) {
            let found = probe(&client, &candidate, Duration::from_millis(650)).await;
            if found
                .as_ref()
                .is_some_and(|identity| identity.transport == "lan")
            {
                url = candidate;
                identity = found;
                break;
            }
        }
    }
    let identity = identity.ok_or("无法连接 Wingmate 主机，请检查地址及网络")?;
    let origin = url.origin().ascii_serialization();
    if password.is_some()
        && (identity.transport != "lan" || !identity.password || url.query().is_some())
    {
        return Err("此主机未启用局域网密码登录".into());
    }
    if identity.transport == "lan"
        && url.query().is_none()
        && !lan_session(&webview, &desktop, &client, &url, password)
            .await
            .map_err(|error| error.to_string())?
    {
        return Ok(ConnectionReply {
            origin,
            needs_password: identity.password,
            needs_pairing: !identity.password,
        });
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
        transport: identity.transport,
    });
    drop(current);
    if let Some(previous) = previous {
        let _ = previous.view.close();
    }
    render_header(&app);
    Ok(ConnectionReply {
        origin,
        needs_password: false,
        needs_pairing: false,
    })
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

fn pairings(status: &ShareStatus) -> Result<Vec<SharePairing>> {
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
        if matches!(
            command,
            ShareCommand::LanPasswordSet { .. }
                | ShareCommand::LanPasswordReveal
                | ShareCommand::LanPasswordDisable
        ) {
            return Err("请在本机连接浮窗的「局域网」管理密码".into());
        }
        let url = webview.url().map_err(|error| error.to_string())?;
        let current = desktop.connection.lock().unwrap();
        let state = current.workspace().ok_or("本机后台尚未连接")?;
        if webview.label() != desktop.window_label
            || instance.as_deref() != Some(state.state.instance.as_str())
            || state.origin != url.origin().ascii_serialization()
        {
            return Err("此页面不能管理本机分享".into());
        }
    }
    let home = desktop.home.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<ShareReply> {
        let status = service::share(&home, &command)?;
        let pairings = if matches!(
            command,
            ShareCommand::Pair | ShareCommand::TunnelPair { .. }
        ) {
            pairings(&status)?
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
    fn accepts_wingmate_http_ipv4_and_https_domain_links() {
        assert!(target("http://192.168.1.5:17321/").is_ok());
        assert!(target("http://127.0.0.1:17321/?auth=code").is_ok());
        assert!(target("https://example.com/?auth=code").is_ok());
        for invalid in [
            "javascript:alert(1)",
            "http://example.com/",
            "https://127.0.0.1/",
            "https://example.com/anything",
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
