//! Share the managed DSH Web through a listener owned by the management service.
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
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
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
    Revoke { id: Uuid },
}

#[derive(Serialize, Deserialize)]
pub struct ShareStatus {
    pub address: Option<SocketAddrV4>,
    pub suggested_address: Option<Ipv4Addr>,
    pub pairing_url: Option<String>,
    pub devices: Vec<Device>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: Uuid,
    pub paired_at: u64,
    pub expires_at: u64,
}

#[derive(Clone)]
struct Session {
    device: Device,
    revoked: CancellationToken,
}

impl Session {
    async fn ended(&self) {
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
    pairing: Option<(Uuid, Instant)>,
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

    fn exchange(&mut self, home: &Path, provided: Uuid) -> Result<Option<Uuid>> {
        if !self
            .pairing
            .is_some_and(|(code, deadline)| code == provided && Instant::now() < deadline)
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
        self.pairing = None;
        Ok(Some(token))
    }
}

struct Gateway {
    home: PathBuf,
    endpoint: Arc<host::Endpoint>,
    address: SocketAddrV4,
    origin: String,
    cookie: String,
    auth: Arc<Mutex<Auth>>,
    closed: CancellationToken,
    upstream: proxy::Credentials,
}

pub(super) struct Control {
    home: PathBuf,
    endpoint: Arc<host::Endpoint>,
    active: Mutex<Option<Arc<Gateway>>>,
    auth: Arc<Mutex<Auth>>,
    runtime: tokio::runtime::Runtime,
}

fn suggested_address() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // Ask the OS which interface it would use. UDP connect sends no packets.
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(address) if !address.is_loopback() => Some(address),
        _ => None,
    }
}

impl Control {
    pub(super) fn new(home: PathBuf, endpoint: Arc<host::Endpoint>) -> Result<Self> {
        let auth = Arc::new(Mutex::new(Auth::load(&home).unwrap_or_else(|error| {
            eprintln!("无法读取分享设备：{error:#}");
            Auth::default()
        })));
        let control = Self {
            home,
            endpoint,
            active: Mutex::new(None),
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
                control.enable(&mut control.active.lock().unwrap(), address)?;
            }
            Ok(())
        };
        if let Err(error) = restore() {
            eprintln!("无法恢复分享入口：{error:#}");
        }
        Ok(control)
    }

    fn enable(&self, active: &mut Option<Arc<Gateway>>, bind: SocketAddrV4) -> Result<()> {
        anyhow::ensure!(
            !bind.ip().is_unspecified() && !bind.ip().is_multicast() && !bind.ip().is_broadcast(),
            "请选择一个本机 IPv4 地址"
        );
        if active.as_ref().is_some_and(|gateway| {
            bind.ip() == gateway.address.ip()
                && (bind.port() == 0 || bind.port() == gateway.address.port())
        }) {
            return Ok(());
        }
        let listener = std::net::TcpListener::bind(bind).context("无法监听分享地址")?;
        listener.set_nonblocking(true)?;
        let address = SocketAddrV4::new(*bind.ip(), listener.local_addr()?.port());
        let _enter = self.runtime.enter();
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let gateway = Arc::new(Gateway {
            home: self.home.clone(),
            endpoint: self.endpoint.clone(),
            address,
            origin: format!("http://{address}"),
            cookie: format!("wingmate_share_{}", address.port()),
            auth: self.auth.clone(),
            closed: CancellationToken::new(),
            upstream: proxy::Credentials::new()?,
        });
        host::publish(&self.home.join("share-listener.json"), &address)?;
        let mut auth = self.auth.lock().unwrap();
        auth.pairing = None;
        if let Some(old) = active.replace(gateway.clone()) {
            old.closed.cancel();
        }
        self.runtime.spawn(gateway.serve(listener));
        Ok(())
    }

    pub(super) fn call(&self, command: Command) -> Result<ShareStatus> {
        let mut active = self.active.lock().unwrap();
        let mut pairing_url = None;
        match command {
            Command::Status => {}
            Command::Enable { address } => {
                let address = address
                    .or_else(suggested_address)
                    .context("未找到局域网地址，请指定本机 IPv4 地址")?;
                self.enable(&mut active, SocketAddrV4::new(address, 0))?;
            }
            Command::Disable => {
                host::publish(
                    &self.home.join("share-listener.json"),
                    &Option::<SocketAddrV4>::None,
                )?;
                let mut auth = self.auth.lock().unwrap();
                if let Some(gateway) = active.take() {
                    gateway.closed.cancel();
                }
                auth.pairing = None;
            }
            Command::Pair => {
                let gateway = active.as_ref().context("请先开启分享")?;
                let code = Uuid::new_v4();
                self.auth.lock().unwrap().pairing = Some((code, Instant::now() + PAIR_AGE));
                pairing_url = Some(format!("{}/_wingmate/pair#{code}", gateway.origin));
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
        Ok(ShareStatus {
            address: active.as_ref().map(|gateway| gateway.address),
            suggested_address: suggested_address(),
            pairing_url,
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
        if request.uri().path() == "/_wingmate/pair" {
            return self.pair(request).await;
        }
        if request.method() == Method::GET && request.uri().path() == "/_wingmate/identity" {
            return json(serde_json::json!({"product": "wingmate", "transport": "lan"}));
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
                    .cloned()
            })
            .filter(|session| session.device.expires_at > now() && !session.revoked.is_cancelled());
        let Some(session) = session else {
            return response(StatusCode::UNAUTHORIZED, "请从主机获取新的配对链接");
        };
        let path = request.uri().path();
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
                business_status(snapshot.as_ref())
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
                !closed.is_cancelled()
                    && !session.revoked.is_cancelled()
                    && session.device.expires_at > now(),
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
            Ok(Ok(status)) => business_status(status.business.as_ref()),
            _ => response(
                StatusCode::CONFLICT,
                "后台操作未完成，请确认主机当前状态后重试",
            ),
        }
    }

    async fn pair(&self, request: Request<Incoming>) -> Response<Body> {
        if request.method() == Method::GET {
            return html(include_str!("gateway/pair.html"));
        }
        if request.method() != Method::POST || request.headers().get(header::ORIGIN).is_none() {
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
            auth.exchange(&self.home, code)
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
}

async fn bounded(body: Incoming, limit: usize) -> Option<Bytes> {
    tokio::time::timeout(Duration::from_secs(5), Limited::new(body, limit).collect())
        .await
        .ok()?
        .ok()
        .map(|body| body.to_bytes())
}

fn business_status(snapshot: Option<&engine::Snapshot>) -> Response<Body> {
    json(serde_json::json!({
        "product": "wingmate",
        "phase": snapshot.map_or("stopped", |state| state.phase.as_str()),
        "instance": snapshot.map(|state| &state.instance),
    }))
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
            pairing: Some((code, Instant::now())),
            ..Auth::default()
        };
        let home = tempfile::tempdir()?;
        assert!(auth.exchange(home.path(), code)?.is_none());
        assert!(auth.sessions.is_empty());
        Ok(())
    }
}
