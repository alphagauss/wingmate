//! Serve the managed DSH Web through local and LAN listeners.
mod proxy;

use crate::{engine, host};
use anyhow::{Context, Result};
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full, Limited};
use hyper::{
    body::{Bytes, Incoming},
    header,
    server::conn::http1,
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    convert::Infallible,
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddrV4, UdpSocket},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type Body = UnsyncBoxBody<Bytes, hyper::Error>;
const SESSION_AGE: u64 = 30 * 24 * 60 * 60;
const PAIR_AGE: Duration = Duration::from_secs(300);

#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Command {
    Status,
    Enable { address: Option<Ipv4Addr> },
    Disable,
    Pair,
    PairLocal,
    Revoke { id: Uuid },
}

#[derive(Serialize, Deserialize)]
pub struct ShareStatus {
    pub address: Option<SocketAddrV4>,
    pub local_address: Option<SocketAddrV4>,
    pub suggested_address: Option<Ipv4Addr>,
    #[serde(default)]
    pub lan_addresses: Vec<SocketAddrV4>,
    pub pairing_url: Option<String>,
    #[serde(default)]
    pub pairing_urls: Vec<String>,
    pub devices: Vec<Device>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Local,
    #[default]
    Lan,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: Uuid,
    pub paired_at: u64,
    pub expires_at: u64,
    #[serde(default)]
    pub access: Access,
}

#[derive(Clone)]
struct Session {
    device: Device,
    revoked: CancellationToken,
}

impl Session {
    fn active(&self) -> bool {
        self.device.expires_at > now() && !self.revoked.is_cancelled()
    }

    async fn ended(&self) {
        if !self.active() {
            return;
        }
        tokio::select! {
            _ = self.revoked.cancelled() => {},
            _ = tokio::time::sleep(Duration::from_secs(self.device.expires_at.saturating_sub(now()))) => {},
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[derive(Default)]
struct Auth {
    pairing: HashMap<Access, (Uuid, Instant)>,
    sessions: HashMap<String, Session>,
}

impl Auth {
    fn load(home: &Path) -> Result<Self> {
        let devices: HashMap<String, Device> =
            read(&home.join("share-devices.json"))?.unwrap_or_default();
        Ok(Self {
            sessions: devices
                .into_iter()
                .filter(|(_, device)| device.expires_at > now())
                .map(|(key, device)| {
                    (
                        key,
                        Session {
                            device,
                            revoked: CancellationToken::new(),
                        },
                    )
                })
                .collect(),
            ..Self::default()
        })
    }

    fn save(
        &self,
        home: &Path,
        revoked: Option<Uuid>,
        added: Option<(&str, &Device)>,
    ) -> Result<()> {
        let mut devices: HashMap<_, _> = self
            .sessions
            .iter()
            .filter(|(_, session)| {
                Some(session.device.id) != revoked && session.device.expires_at > now()
            })
            .map(|(key, session)| (key.as_str(), &session.device))
            .collect();
        devices.extend(added);
        host::publish(&home.join("share-devices.json"), &devices)
    }

    fn exchange(&mut self, home: &Path, access: Access, provided: Uuid) -> Result<Option<Uuid>> {
        if !self
            .pairing
            .get(&access)
            .is_some_and(|&(code, deadline)| code == provided && Instant::now() < deadline)
        {
            return Ok(None);
        }
        let token = Uuid::new_v4();
        let key = digest(&token.to_string());
        let paired_at = now();
        let device = Device {
            id: Uuid::new_v4(),
            paired_at,
            expires_at: paired_at + SESSION_AGE,
            access,
        };
        self.save(home, None, Some((&key, &device)))?;
        self.sessions
            .retain(|_, session| session.device.expires_at > paired_at);
        self.sessions.insert(
            key,
            Session {
                device,
                revoked: CancellationToken::new(),
            },
        );
        self.pairing.remove(&access);
        Ok(Some(token))
    }
}

struct Gateway {
    home: PathBuf,
    endpoint: Arc<host::Endpoint>,
    address: SocketAddrV4,
    access: Access,
    origin: String,
    cookie: String,
    auth: Arc<Mutex<Auth>>,
    closed: CancellationToken,
    upstream: proxy::Credentials,
}

struct LanShare {
    bind: SocketAddrV4,
    gateways: Vec<Arc<Gateway>>,
}

impl LanShare {
    fn close(self) {
        for gateway in self.gateways {
            gateway.closed.cancel();
        }
    }
}

pub(super) struct Control {
    home: PathBuf,
    endpoint: Arc<host::Endpoint>,
    lan: Mutex<Option<LanShare>>,
    local: Option<Arc<Gateway>>,
    auth: Arc<Mutex<Auth>>,
    runtime: tokio::runtime::Runtime,
}

fn suggested_address() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // Ask the OS which interface it would use. UDP connect sends no packets.
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(address) if address.is_private() => Some(address),
        _ => None,
    }
}

fn lan_addresses() -> Vec<Ipv4Addr> {
    let preferred = suggested_address();
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return preferred.into_iter().collect();
    };
    let mut addresses: Vec<_> = interfaces
        .into_iter()
        .filter(|interface| interface.is_oper_up())
        .filter_map(|interface| match interface.ip() {
            IpAddr::V4(address) if address.is_private() => Some(address),
            _ => None,
        })
        .collect();
    addresses.sort_unstable_by_key(|&address| (Some(address) != preferred, address));
    addresses.dedup();
    addresses
}

impl Control {
    pub(super) fn new(home: PathBuf, endpoint: Arc<host::Endpoint>) -> Result<Self> {
        let auth = Arc::new(Mutex::new(Auth::load(&home).unwrap_or_else(|error| {
            eprintln!("无法读取分享设备：{error:#}");
            Auth::default()
        })));
        let mut control = Self {
            home,
            endpoint,
            lan: Mutex::new(None),
            local: None,
            auth,
            runtime: tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?,
        };
        let restore = || -> Result<()> {
            if let Some(address) =
                read::<Option<SocketAddrV4>>(&control.home.join("share-listener.json"))?.flatten()
            {
                control.enable(&mut control.lan.lock().unwrap(), address, &lan_addresses())?;
            }
            Ok(())
        };
        if let Err(error) = restore() {
            eprintln!("无法恢复分享入口：{error:#}");
        }
        match control.start_local() {
            Ok(gateway) => control.local = Some(gateway),
            Err(error) => eprintln!("无法启动本机 Web 入口：{error:#}"),
        }
        Ok(control)
    }

    fn start_local(&self) -> Result<Arc<Gateway>> {
        let path = self.home.join("local-web-port.json");
        let port = read::<u16>(&path).unwrap_or_else(|error| {
            eprintln!("无法读取本机 Web 端口：{error:#}");
            None
        });
        let bind = |port| self.bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port), Access::Local);
        let (gateway, listener) = port
            .and_then(|port| bind(port).ok())
            .map_or_else(|| bind(0), Ok)?;
        if port != Some(gateway.address.port()) {
            host::publish(&path, &gateway.address.port())?;
        }
        self.runtime.spawn(gateway.clone().serve(listener));
        Ok(gateway)
    }

    fn enable(
        &self,
        active: &mut Option<LanShare>,
        bind: SocketAddrV4,
        local_addresses: &[Ipv4Addr],
    ) -> Result<()> {
        anyhow::ensure!(
            !bind.ip().is_multicast() && !bind.ip().is_broadcast(),
            "请选择一个本机 IPv4 地址"
        );
        // The saved unspecified address records automatic mode. Each listener binds a concrete IP.
        let addresses = if bind.ip().is_unspecified() {
            local_addresses
        } else {
            std::slice::from_ref(bind.ip())
        };
        anyhow::ensure!(!addresses.is_empty(), "未找到可用的局域网 IPv4 地址");
        if active.as_ref().is_some_and(|share| {
            bind.ip() == share.bind.ip()
                && (bind.port() == 0 || bind.port() == share.bind.port())
                && share
                    .gateways
                    .iter()
                    .map(|gateway| gateway.address.ip())
                    .eq(addresses)
        }) {
            return Ok(());
        }
        // Reserve the same port on every selected interface before replacing an active share.
        let mut port = bind.port();
        let mut listeners = Vec::new();
        for &ip in addresses {
            let (gateway, listener) = self
                .bind(SocketAddrV4::new(ip, port), Access::Lan)
                .with_context(|| format!("无法在 {ip} 开启局域网分享"))?;
            port = gateway.address.port();
            listeners.push((gateway, listener));
        }
        let bind = SocketAddrV4::new(*bind.ip(), port);
        host::publish(&self.home.join("share-listener.json"), &bind)?;
        let mut auth = self.auth.lock().unwrap();
        auth.pairing.remove(&Access::Lan);
        let share = LanShare {
            bind,
            gateways: listeners
                .iter()
                .map(|(gateway, _)| gateway.clone())
                .collect(),
        };
        if let Some(old) = active.replace(share) {
            old.close();
        }
        for (gateway, listener) in listeners {
            self.runtime.spawn(gateway.serve(listener));
        }
        Ok(())
    }

    fn bind(
        &self,
        bind: SocketAddrV4,
        access: Access,
    ) -> Result<(Arc<Gateway>, tokio::net::TcpListener)> {
        let listener = std::net::TcpListener::bind(bind).context("无法监听 Web 入口")?;
        listener.set_nonblocking(true)?;
        let address = SocketAddrV4::new(*bind.ip(), listener.local_addr()?.port());
        let _enter = self.runtime.enter();
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let gateway = Arc::new(Gateway {
            home: self.home.clone(),
            endpoint: self.endpoint.clone(),
            address,
            access,
            origin: format!("http://{address}"),
            cookie: format!("wingmate_share_{}", address.port()),
            auth: self.auth.clone(),
            closed: CancellationToken::new(),
            upstream: proxy::Credentials::new()?,
        });
        Ok((gateway, listener))
    }

    pub(super) fn call(&self, command: Command) -> Result<ShareStatus> {
        let local_addresses = lan_addresses();
        let mut active = self.lan.lock().unwrap();
        let mut pairing_urls = Vec::new();
        match command {
            Command::Status => {}
            Command::Enable { address } => {
                anyhow::ensure!(
                    address != Some(Ipv4Addr::UNSPECIFIED),
                    "请选择一个本机 IPv4 地址"
                );
                self.enable(
                    &mut active,
                    SocketAddrV4::new(address.unwrap_or(Ipv4Addr::UNSPECIFIED), 0),
                    &local_addresses,
                )?;
            }
            Command::Disable => {
                host::publish(
                    &self.home.join("share-listener.json"),
                    &Option::<SocketAddrV4>::None,
                )?;
                let mut auth = self.auth.lock().unwrap();
                if let Some(share) = active.take() {
                    share.close();
                }
                auth.pairing.remove(&Access::Lan);
            }
            Command::Pair | Command::PairLocal => {
                let gateways = match command {
                    Command::PairLocal => {
                        std::slice::from_ref(self.local.as_ref().context("本机 Web 入口不可用")?)
                    }
                    _ => active.as_ref().context("请先开启分享")?.gateways.as_slice(),
                };
                let code = Uuid::new_v4();
                self.auth
                    .lock()
                    .unwrap()
                    .pairing
                    .insert(gateways[0].access, (code, Instant::now() + PAIR_AGE));
                pairing_urls = gateways
                    .iter()
                    .map(|gateway| format!("{}/?auth={code}", gateway.origin))
                    .collect();
            }
            Command::Revoke { id } => {
                let mut auth = self.auth.lock().unwrap();
                auth.save(&self.home, Some(id), None)?;
                auth.sessions.retain(|_, session| {
                    if session.device.id != id {
                        return true;
                    }
                    session.revoked.cancel();
                    false
                });
            }
        }
        let mut devices: Vec<_> = self
            .auth
            .lock()
            .unwrap()
            .sessions
            .values()
            .filter(|session| session.device.expires_at > now())
            .map(|session| session.device.clone())
            .collect();
        devices.sort_by_key(|device| (device.paired_at, device.id));
        let lan_addresses: Vec<_> = active
            .as_ref()
            .into_iter()
            .flat_map(|share| share.gateways.iter().map(|gateway| gateway.address))
            .collect();
        Ok(ShareStatus {
            address: lan_addresses.first().copied(),
            local_address: self.local.as_ref().map(|gateway| gateway.address),
            suggested_address: local_addresses.first().copied(),
            lan_addresses,
            pairing_url: pairing_urls.first().cloned(),
            pairing_urls,
            devices,
        })
    }
}

impl Gateway {
    async fn serve(self: Arc<Self>, listener: tokio::net::TcpListener) {
        loop {
            let socket = tokio::select! {
                _ = self.closed.cancelled() => return,
                incoming = listener.accept() => match incoming {
                    Ok((socket, _)) => socket,
                    Err(error) => {
                        eprintln!("分享入口无法接受连接：{error}");
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        continue;
                    }
                },
            };
            let gateway = self.clone();
            tokio::spawn(async move {
                let service = service_fn(|request| async {
                    Ok::<_, Infallible>(gateway.handle(request).await)
                });
                let connection = http1::Builder::new()
                    .timer(TokioTimer::new())
                    .header_read_timeout(Duration::from_secs(10))
                    .serve_connection(TokioIo::new(socket), service)
                    .with_upgrades();
                tokio::select! {
                    _ = gateway.closed.cancelled() => {},
                    _ = connection => {},
                }
            });
        }
    }

    async fn handle(&self, request: Request<Incoming>) -> Response<Body> {
        let headers = request.headers();
        if headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            != Some(&self.address.to_string())
        {
            return response(StatusCode::MISDIRECTED_REQUEST, "分享地址不匹配");
        }
        if headers
            .get(header::ORIGIN)
            .is_some_and(|value| value != self.origin.as_str())
            || (headers
                .get("sec-fetch-site")
                .is_some_and(|value| value == "cross-site")
                && headers
                    .get("sec-fetch-mode")
                    .is_none_or(|value| value != "navigate"))
            || request.uri().authority().is_some()
            || matches!(*request.method(), Method::CONNECT | Method::TRACE)
        {
            return response(StatusCode::FORBIDDEN, "不接受此来源的请求");
        }
        let path = request.uri().path();
        if path == "/" {
            match (request.method(), request.uri().query()) {
                (&Method::GET, Some(query)) if query.starts_with("auth=") => {
                    return html(include_str!("gateway/pair.html"));
                }
                (&Method::POST, Some("auth")) => return self.pair(request).await,
                _ => {}
            }
        }
        if request.method() == Method::GET && path == "/_wingmate/identity" {
            return json(serde_json::json!({"product": "wingmate", "transport": self.access}));
        }
        let session = headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(';'))
            .filter_map(|part| part.trim().split_once('='))
            .find(|(name, _)| *name == self.cookie)
            .and_then(|(_, value)| Uuid::parse_str(value).ok())
            .and_then(|token| {
                self.auth
                    .lock()
                    .unwrap()
                    .sessions
                    .get(&digest(&token.to_string()))
                    .filter(|session| session.device.access == self.access)
                    .cloned()
            })
            .filter(Session::active);
        let Some(session) = session else {
            return response(StatusCode::UNAUTHORIZED, "请从主机获取新的配对链接");
        };
        if path.starts_with("/_wingmate/") && path != "/_wingmate/status" {
            return match (request.method(), path) {
                (&Method::GET, "/_wingmate/recovery") => recovery(),
                (&Method::GET, "/_wingmate/home.js") => content(
                    StatusCode::OK,
                    "text/javascript; charset=utf-8",
                    Bytes::from_static(include_bytes!("../../../apps/launcher/home.js")),
                ),
                (&Method::GET, "/_wingmate/logo.svg") => content(
                    StatusCode::OK,
                    "image/svg+xml",
                    Bytes::from_static(include_bytes!("../../../src-tauri/icons/logo.svg")),
                ),
                (&Method::POST, "/_wingmate/manage") => self.manage(request, session).await,
                _ => response(StatusCode::NOT_FOUND, ""),
            };
        }
        let home = self.home.clone();
        let snapshot = tokio::task::spawn_blocking(move || engine::status(&home)).await;
        let Ok(Ok(snapshot)) = snapshot else {
            return response(StatusCode::BAD_GATEWAY, "无法读取业务后台状态");
        };
        if path == "/_wingmate/status" {
            return if request.method() == Method::GET {
                self.business_status(snapshot.as_ref())
            } else {
                response(StatusCode::METHOD_NOT_ALLOWED, "")
            };
        }
        if let Some(state) = snapshot.filter(|state| state.url.is_some()) {
            return self
                .upstream
                .forward(request, state, session, self.closed.clone())
                .await;
        }
        if request.method() == Method::GET
            && headers.get(header::ACCEPT).is_some_and(|value| {
                value
                    .to_str()
                    .is_ok_and(|value| value.contains("text/html"))
            })
        {
            return recovery();
        }
        response(StatusCode::SERVICE_UNAVAILABLE, "业务后台未就绪")
    }

    async fn manage(&self, request: Request<Incoming>, session: Session) -> Response<Body> {
        if request.headers().get(header::ORIGIN).is_none() {
            return response(StatusCode::FORBIDDEN, "");
        }
        let Some(body) = bounded(request.into_body(), 4096).await else {
            return response(StatusCode::BAD_REQUEST, "管理请求无效");
        };
        let Ok(command) = serde_json::from_slice::<host::Request>(&body) else {
            return response(StatusCode::BAD_REQUEST, "管理请求无效");
        };
        match (
            command.action.as_str(),
            command.expected_instance.as_deref(),
        ) {
            ("start", None) => {}
            ("stop" | "restart", Some(instance)) if !instance.is_empty() => {}
            _ => return response(StatusCode::BAD_REQUEST, "管理操作或实例无效"),
        }
        let endpoint = self.endpoint.clone();
        let closed = self.closed.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<host::Status> {
            anyhow::ensure!(
                !closed.is_cancelled() && session.active(),
                "连接已撤销或过期"
            );
            host::call(
                &endpoint,
                &command.action,
                command.expected_instance.as_deref(),
            )
        })
        .await;
        match result {
            Ok(Ok(status)) => self.business_status(status.business.as_ref()),
            _ => response(
                StatusCode::CONFLICT,
                "后台操作未完成，请确认主机当前状态后重试",
            ),
        }
    }

    async fn pair(&self, request: Request<Incoming>) -> Response<Body> {
        if request.headers().get(header::ORIGIN).is_none() {
            return response(StatusCode::FORBIDDEN, "");
        }
        let Some(bytes) = bounded(request.into_body(), 36).await else {
            return response(StatusCode::BAD_REQUEST, "配对请求无效");
        };
        let Some(code) = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| Uuid::parse_str(text).ok())
        else {
            return response(StatusCode::BAD_REQUEST, "配对请求无效");
        };
        let token = {
            let mut auth = self.auth.lock().unwrap();
            if self.closed.is_cancelled() {
                return response(StatusCode::SERVICE_UNAVAILABLE, "分享入口已关闭");
            }
            auth.exchange(&self.home, self.access, code)
        };
        match token {
            Ok(Some(token)) => {
                let mut reply = response(StatusCode::NO_CONTENT, "");
                reply.headers_mut().insert(
                    header::SET_COOKIE,
                    format!(
                        "{}={token}; Path=/; Max-Age={SESSION_AGE}; HttpOnly; SameSite=Strict",
                        self.cookie
                    )
                    .parse()
                    .unwrap(),
                );
                reply
            }
            Ok(None) => response(StatusCode::UNAUTHORIZED, "配对链接已失效，请从主机重新获取"),
            Err(error) => {
                eprintln!("无法保存配对设备：{error:#}");
                response(StatusCode::INTERNAL_SERVER_ERROR, "无法保存配对设备")
            }
        }
    }

    fn business_status(&self, snapshot: Option<&engine::Snapshot>) -> Response<Body> {
        let dsh_origin = snapshot
            .filter(|_| self.access == Access::Local)
            .and_then(|state| state.url.as_deref())
            .and_then(|url| reqwest::Url::parse(url).ok())
            .map(|url| url.origin().ascii_serialization());
        json(serde_json::json!({
            "product": "wingmate",
            "phase": snapshot.map_or("stopped", |state| state.phase.as_str()),
            "instance": snapshot.map(|state| &state.instance),
            "dsh_origin": dsh_origin,
        }))
    }
}

async fn bounded(body: Incoming, limit: usize) -> Option<Bytes> {
    tokio::time::timeout(Duration::from_secs(5), Limited::new(body, limit).collect())
        .await
        .ok()?
        .ok()
        .map(|body| body.to_bytes())
}

fn json(value: serde_json::Value) -> Response<Body> {
    content(
        StatusCode::OK,
        "application/json",
        Bytes::from(value.to_string()),
    )
}

fn response(status: StatusCode, text: &'static str) -> Response<Body> {
    content(
        status,
        "text/plain; charset=utf-8",
        Bytes::from_static(text.as_bytes()),
    )
}

fn content(status: StatusCode, media: &'static str, body: Bytes) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, media)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::REFERRER_POLICY, "no-referrer")
        .body(
            Full::new(body)
                .map_err(|never| match never {})
                .boxed_unsync(),
        )
        .unwrap()
}

fn recovery() -> Response<Body> {
    html(include_str!("../../../apps/launcher/index.html"))
}

fn html(text: &'static str) -> Response<Body> {
    let mut reply = content(
        StatusCode::OK,
        "text/html; charset=utf-8",
        Bytes::from_static(text.as_bytes()),
    );
    reply.headers_mut().insert(header::CONTENT_SECURITY_POLICY, header::HeaderValue::from_static(
        "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'"));
    reply
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_pairing_does_not_create_a_device() -> Result<()> {
        let code = Uuid::new_v4();
        let mut auth = Auth {
            pairing: HashMap::from([(Access::Lan, (code, Instant::now()))]),
            ..Auth::default()
        };
        let home = tempfile::tempdir()?;
        assert!(auth.exchange(home.path(), Access::Lan, code)?.is_none());
        assert!(auth.sessions.is_empty());
        Ok(())
    }
}
