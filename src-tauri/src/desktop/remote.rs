use super::{
    connection_panel,
    remote_relay::{BoxStream, Relay},
    Desktop,
};
use anyhow::{Context, Result};
use std::{
    fs,
    future::Future,
    io::Write,
    net::Ipv4Addr,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{
    ipc::CapabilityBuilder, Manager, PhysicalPosition, PhysicalSize, Rect, State, Url, Webview,
    WebviewBuilder, WebviewUrl, Window,
};
use wingmate_launcher::service::{self, ShareCommand, ShareStatus};

#[path = "remote_lan.rs"]
mod lan;

pub(super) struct LanScan {
    panel: String,
    id: u32,
    cancel: tokio::sync::oneshot::Sender<()>,
}

fn authorized(webview: &Webview, desktop: &Desktop, instance: Option<&str>) -> Result<(), String> {
    match instance {
        Some(instance) => connection_panel::workspace_source(webview, desktop, instance),
        None => connection_panel::local_panel(webview, desktop).map(drop),
    }
}

fn read_history(path: &Path) -> Result<Vec<String>> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

fn change_history(home: &Path, update: impl FnOnce(&mut Vec<String>)) -> Result<Vec<String>> {
    fs::create_dir_all(home)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.join("recent-hosts.lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    let path = home.join("recent-hosts.json");
    let mut history = read_history(&path)?;
    update(&mut history);
    history.truncate(20);
    let mut file = tempfile::NamedTempFile::new_in(home)?;
    file.write_all(&serde_json::to_vec(&history)?)?;
    file.persist(path)?;
    Ok(history)
}

#[tauri::command]
pub(super) async fn connection_history(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    instance: Option<String>,
    legacy: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    authorized(&webview, &desktop, instance.as_deref())?;
    let home = desktop.home.clone();
    let legacy = legacy.filter(|items| instance.is_none() && !items.is_empty());
    tauri::async_runtime::spawn_blocking(move || match legacy {
        Some(legacy) => change_history(&home, |history| {
            for origin in legacy
                .iter()
                .take(20)
                .filter_map(|item| target(item).ok())
                .filter(|url| url.query().is_none())
                .map(|url| url.origin().ascii_serialization())
            {
                if !history.contains(&origin) {
                    history.push(origin);
                }
            }
        }),
        None => read_history(&home.join("recent-hosts.json")),
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| format!("无法读取最近连接：{error:#}"))
}

#[tauri::command]
pub(super) async fn scan_lan(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    scan: u32,
    instance: Option<String>,
) -> Result<Option<bool>, String> {
    authorized(&webview, &desktop, instance.as_deref())?;
    let saved = read_history(&desktop.home.join("recent-hosts.json")).unwrap_or_default();
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    *desktop.lan_scan.lock().unwrap() = Some(LanScan {
        panel: webview.label().into(),
        id: scan,
        cancel,
    });
    let result = tokio::select! {
        _ = cancelled => Ok(None),
        result = lan::scan(&webview, &desktop, saved, scan, instance.as_deref()) => result.map(Some).map_err(|error| error.to_string()),
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
    instance: Option<String>,
) -> Result<(), String> {
    authorized(&webview, &desktop, instance.as_deref())?;
    cancel_scan(&desktop, webview.label(), Some(scan));
    Ok(())
}

const HEADER_HEIGHT: i32 = 40;

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
    let top = (f64::from(HEADER_HEIGHT) * window.scale_factor()?).round() as u32;
    Ok(Rect {
        position: PhysicalPosition::new(0, top as i32).into(),
        size: PhysicalSize::new(size.width, size.height.saturating_sub(top)).into(),
    })
}

#[cfg(not(target_os = "linux"))]
pub(super) fn fit(window: &Window, view: &Webview) -> tauri::Result<()> {
    view.set_bounds(bounds(window)?)
}

pub(super) struct Workbench {
    pub view: Webview,
    pub origin: String,
    transport: String,
    address: String,
    _relay: Option<Relay>,
}

impl Workbench {
    pub(super) fn display_address(&self) -> &str {
        if self._relay.is_none() {
            return self
                .address
                .split_once("://")
                .map_or(&self.address, |(_, host)| host);
        }
        &self.address
    }
}

pub(super) fn render_header(app: &tauri::AppHandle) {
    let desktop = app.state::<Arc<Desktop>>();
    let args = {
        let current = desktop.connection.lock().unwrap();
        let Some(remote) = current.remote.as_ref() else {
            return;
        };
        serde_json::json!([
            remote.display_address(),
            current.titlebar_theme,
            std::env::consts::OS,
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
    let _ = main.eval(format!(
        "{{ const renderHeader = {script}\nrenderHeader({args}); }}"
    ));
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

pub(super) struct Attempt<'a> {
    webview: &'a Webview,
    desktop: &'a Desktop,
    instance: Option<&'a str>,
    revision: (u64, u32),
}

impl<'a> Attempt<'a> {
    pub(super) fn start(
        webview: &'a Webview,
        desktop: &'a Desktop,
        instance: Option<&'a str>,
    ) -> Result<Self, String> {
        authorized(webview, desktop, instance)?;
        let mut current = desktop.connection.lock().unwrap();
        if current.busy || current.exit_pending {
            return Err("后台操作正在进行，请稍后重试".into());
        }
        current.revision += 1;
        Ok(Self {
            webview,
            desktop,
            instance,
            revision: (current.revision, current.titlebar.navigation()),
        })
    }

    pub(super) fn verify(&self) -> Result<(), String> {
        authorized(self.webview, self.desktop, self.instance)?;
        let current = self.desktop.connection.lock().unwrap();
        if (current.revision, current.titlebar.navigation()) != self.revision {
            return Err("连接已取消，请重试".into());
        }
        Ok(())
    }
}

pub(super) fn validate_executable(executable: Option<&str>) -> Result<()> {
    if let Some(path) = executable {
        anyhow::ensure!(
            path.starts_with('/') && path.len() <= 4096 && !path.chars().any(char::is_control),
            "请填写 Wingmate 在目标系统上的绝对路径"
        );
    }
    Ok(())
}

pub(super) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn executable_script(executable: Option<&str>) -> String {
    if let Some(path) = executable {
        format!("wm={}; [ -x \"$wm\" ] || exit 44", shell_quote(path))
    } else {
        "if [ -x \"$HOME/.wingmate/remote-install/wingmate\" ]; then wm=\"$HOME/.wingmate/remote-install/wingmate\"; elif [ -x \"$HOME/.wingmate/bin/wingmate\" ]; then wm=\"$HOME/.wingmate/bin/wingmate\"; else wm=$(command -v wingmate) || exit 44; fi".to_owned()
    }
}

#[derive(Debug)]
pub(super) struct MissingExecutable;

impl std::fmt::Display for MissingExecutable {
    fn fmt(&self, writer: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writer.write_str("找不到 Wingmate，请安装或指定可执行文件")
    }
}

impl std::error::Error for MissingExecutable {}

pub(super) async fn bootstrap<F, S>(executable: Option<&str>, exec: F) -> Result<Url>
where
    F: Fn(String, Duration) -> S,
    S: Future<Output = Result<(u32, Vec<u8>, Vec<u8>)>>,
{
    const OUTPUT: &str = "__wingmate_connect_info__";
    let init = executable_script(executable);
    let (code, stdout, stderr) = exec(format!(
        "{init}; if ! \"$wm\" connect-info >/dev/null 2>&1; then \"$wm\" service-start >/dev/null || exit; fi; printf '\\n{OUTPUT}\\n'; exec \"$wm\" connect-info --pair"
    ), Duration::from_secs(150)).await?;
    if code == 44 {
        return Err(MissingExecutable.into());
    }
    anyhow::ensure!(
        code == 0,
        "无法准备 Wingmate 网关（退出码 {code}）{}",
        {
            let details = String::from_utf8_lossy(&stderr);
            if details.trim().is_empty() {
                String::new()
            } else {
                format!("：{}", details.trim().chars().take(500).collect::<String>())
            }
        }
    );
    #[derive(serde::Deserialize)]
    struct Pairing {
        schema_version: u32,
        host: std::net::Ipv4Addr,
        port: u16,
        pairing_url: String,
    }
    let (_, json) = std::str::from_utf8(&stdout)
        .context("网关连接信息编码无效")?
        .rsplit_once(OUTPUT)
        .context("Wingmate 网关未返回连接信息")?;
    let info: Pairing = serde_json::from_str(json).context("无法识别 Wingmate 网关连接信息")?;
    let pairing = target(&info.pairing_url)?;
    anyhow::ensure!(
        info.schema_version == 1
            && info.host == Ipv4Addr::LOCALHOST
            && info.port != 0
            && pairing.scheme() == "http"
            && pairing.host_str() == Some("127.0.0.1")
            && pairing.port() == Some(info.port)
            && pairing.query().is_some(),
        "Wingmate 返回了无效的回环配对地址"
    );
    Ok(pairing)
}

pub(super) async fn open_tunnel<F, S>(
    attempt: Attempt<'_>,
    pairing: Url,
    address: String,
    transport: &str,
    dial: F,
) -> Result<(), String>
where
    F: Fn() -> S + Send + Sync + 'static,
    S: Future<Output = Result<BoxStream>> + Send,
{
    attempt.verify()?;
    let stream = tokio::time::timeout(Duration::from_secs(8), dial())
        .await
        .map_err(|_| "连接 Wingmate 网关超时".to_owned())?
        .map_err(|error| format!("{error:#}"))?;
    drop(stream);
    let (relay, url) = Relay::listen(pairing, dial)
        .await
        .map_err(|error| format!("{error:#}"))?;
    let client = client(true).map_err(|_| "无法初始化远程连接")?;
    if probe(&client, &url, Duration::from_secs(10))
        .await
        .is_none()
    {
        return Err("转发已建立，Wingmate 网关未响应".into());
    }
    open_workbench(attempt, url, address, transport.to_owned(), Some(relay))
}

async fn with_cookie_store<T: Send + 'static>(
    source: &Webview,
    desktop: &Desktop,
    use_store: impl FnOnce(Webview) -> tauri::Result<T> + Send + 'static,
) -> Result<T> {
    let app = source.app_handle().clone();
    let handle = app.clone();
    let source_label = source.label().to_owned();
    let main_label = desktop.window_label.clone();
    let (reply, result) = tokio::sync::oneshot::channel();
    // Resolve the webviews on the UI thread so cookie access cannot race a destroyed source.
    app.run_on_main_thread(move || {
        let value = (|| {
            handle
                .get_webview(&source_label)
                .context("Wingmate 连接页面已关闭")?;
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
    requested: Option<String>,
    instance: Option<String>,
) -> Result<ConnectionReply, String> {
    let attempt = Attempt::start(&webview, &desktop, instance.as_deref())?;
    let mut url = target(&link).map_err(|error| error.to_string())?;
    let requested_origin = requested
        .and_then(|link| target(&link).ok())
        .unwrap_or_else(|| url.clone())
        .origin()
        .ascii_serialization();
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
    attempt.verify()?;
    let identity = identity.ok_or("无法连接 Wingmate 主机，请检查地址及网络")?;
    let origin = url.origin().ascii_serialization();
    if password.is_some()
        && (identity.transport != "lan" || !identity.password || url.query().is_some())
    {
        return Err("此主机未启用局域网密码登录".into());
    }
    if identity.transport == "lan" && url.query().is_none() {
        let logged_in = lan_session(&webview, &desktop, &client, &url, password)
            .await
            .map_err(|error| error.to_string())?;
        attempt.verify()?;
        if !logged_in {
            return Ok(ConnectionReply {
                origin,
                needs_password: identity.password,
                needs_pairing: !identity.password,
            });
        }
    }
    open_workbench(attempt, url, origin.clone(), identity.transport, None)?;
    let home = desktop.home.clone();
    let connected = origin.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        change_history(&home, |history| {
            history.retain(|item| item != &connected && item != &requested_origin);
            history.insert(0, connected);
        })
    })
    .await;
    Ok(ConnectionReply {
        origin,
        needs_password: false,
        needs_pairing: false,
    })
}

fn open_workbench(
    attempt: Attempt<'_>,
    url: Url,
    address: String,
    transport: String,
    relay: Option<Relay>,
) -> Result<(), String> {
    attempt.verify()?;
    let origin_guard = url.origin();
    let origin = origin_guard.ascii_serialization();
    let desktop = attempt.desktop;
    let app = attempt.webview.app_handle().clone();
    let main = app
        .get_webview(&desktop.window_label)
        .ok_or("主窗口已关闭")?;
    let parent = main.window();
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let label = format!("remote-workbench-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    app.add_capability(
        CapabilityBuilder::new(&label)
            .webview(&label)
            .local(false)
            .remote(format!("{origin}/*"))
            .permission("allow-return-local"),
    )
    .map_err(|error| error.to_string())?;
    let expected_origin = serde_json::json!(origin);
    let popup_app = app.clone();
    let builder = WebviewBuilder::new(label, WebviewUrl::External(url))
        .focused(false)
        .incognito(relay.is_some())
        .initialization_script(format!(
            r#"if (window === window.top && location.origin === {expected_origin}) {{
                Object.defineProperty(window, '__WINGMATE_RETURN_LOCAL__', {{
                    value: () => window.__TAURI_INTERNALS__.invoke('return_local')
                }});
            }}"#,
        ))
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
    #[cfg(target_os = "linux")]
    if let Err(error) = connection_panel::place_in_overlay(&view, HEADER_HEIGHT) {
        let _ = view.close();
        return Err(error.to_string());
    }
    let source_valid = authorized(attempt.webview, desktop, attempt.instance).is_ok();
    let previous = {
        let mut current = desktop.connection.lock().unwrap();
        if !source_valid || (current.revision, current.titlebar.navigation()) != attempt.revision {
            drop(current);
            let _ = view.close();
            return Err("连接已取消，请重试".into());
        }
        current.revision += 1;
        current.remote.replace(Workbench {
            view: view.clone(),
            origin,
            transport,
            address,
            _relay: relay,
        })
    };
    if let Some(previous) = previous {
        let _ = previous.view.close();
    }
    render_header(&app);
    if attempt.instance.is_some() {
        let _ = view.set_focus();
    }
    Ok(())
}

#[tauri::command]
pub(super) fn return_local(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
) -> Result<(), String> {
    let source = webview.url().map_err(|error| error.to_string())?;
    let local = connection_panel::local_entry(&webview, &desktop).is_ok();
    let main = webview
        .app_handle()
        .get_webview(&desktop.window_label)
        .ok_or("主窗口已关闭")?;
    let remote = {
        let mut current = desktop.connection.lock().unwrap();
        if !local
            && !current.remote.as_ref().is_some_and(|remote| {
                remote.view.label() == webview.label()
                    && remote.origin == source.origin().ascii_serialization()
            })
        {
            return Err("此页面不能退出远程工作台".into());
        }
        let Some(remote) = current.remote.take() else {
            return Ok(());
        };
        current.revision += 1;
        remote
    };
    let _ = remote.view.close();
    let _ = main.eval("document.querySelector('[data-wingmate-remote-header]')?.remove()");
    let _ = main.set_focus();
    Ok(())
}

#[tauri::command]
pub(super) async fn local_share(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    command: ShareCommand,
    instance: Option<String>,
) -> Result<ShareStatus, String> {
    if connection_panel::local_panel(&webview, &desktop).is_err() {
        if matches!(
            command,
            ShareCommand::LanPasswordSet { .. }
                | ShareCommand::LanPasswordReveal
                | ShareCommand::LanPasswordDisable
        ) {
            return Err("请在本机连接浮窗的「局域网」管理密码".into());
        }
        connection_panel::workspace_source(
            &webview,
            &desktop,
            instance.as_deref().ok_or("此页面不能管理本机分享")?,
        )?;
    }
    let home = desktop.home.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<ShareStatus> {
        let mut status = service::share(&home, &command)?;
        // Support continuing with an older service until the user restarts it.
        if matches!(
            command,
            ShareCommand::Pair | ShareCommand::TunnelPair { .. }
        ) {
            status.fill_pairing_qr()?;
        }
        Ok(status)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| format!("{error:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn accepts_pretty_printed_cli_output_after_login_banner() {
        let gateway = bootstrap(None, |_, _| async {
            Ok((
                0,
                br#"login banner
__wingmate_connect_info__
{
  "schema_version": 1,
  "host": "127.0.0.1",
  "port": 8746,
  "pairing_url": "http://127.0.0.1:8746/?auth=one-use"
}
"#
                .to_vec(),
                Vec::new(),
            ))
        })
        .await
        .unwrap();
        assert_eq!(gateway.port(), Some(8746));
    }

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
