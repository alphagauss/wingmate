//! Serve the managed DSH Web through local, LAN and public listeners.
mod proxy;

use crate::{engine, host, tunnel};
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
    fs, io,
    net::{IpAddr, Ipv4Addr, SocketAddrV4, UdpSocket},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type Body = UnsyncBoxBody<Bytes, anyhow::Error>;
const SESSION_AGE: u64 = 30 * 24 * 60 * 60;
const PAIR_AGE: Duration = Duration::from_secs(300);
const PAIR_PAGE: &str = include_str!("gateway/pair.html");

#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Command {
    Status,
    Enable {
        address: Option<Ipv4Addr>,
    },
    Disable,
    Pair,
    PairLocal,
    DesktopSession {
        instance: String,
    },
    Revoke {
        id: Uuid,
    },
    TunnelConfigure {
        hostname: String,
        token: Option<String>,
    },
    TunnelStart {
        mode: tunnel::Mode,
    },
    TunnelStop,
    TunnelPair,
}

#[derive(Serialize, Deserialize)]
pub struct ShareStatus {
    pub address: Option<SocketAddrV4>,
    pub local_address: Option<SocketAddrV4>,
    pub web_error: Option<String>,
    pub suggested_address: Option<Ipv4Addr>,
    #[serde(default)]
    pub lan_addresses: Vec<SocketAddrV4>,
    pub pairing_url: Option<String>,
    #[serde(default)]
    pub pairing_urls: Vec<String>,
    pub devices: Vec<Device>,
    #[serde(default)]
    pub tunnel: Option<tunnel::Status>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Local,
    #[default]
    Lan,
    Public,
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

fn session_key(token: Uuid, public_origin: Option<&str>) -> String {
    match public_origin {
        Some(origin) => digest(&format!("{origin}\n{token}")),
        None => digest(&token.to_string()),
    }
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
    desktop_tickets: HashMap<Uuid, DesktopTicket>,
    sessions: HashMap<String, Session>,
}

struct DesktopTicket {
    instance: String,
    deadline: Instant,
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

    fn exchange(
        &mut self,
        home: &Path,
        access: Access,
        provided: Uuid,
        public_origin: Option<&str>,
    ) -> Result<Option<Uuid>> {
        if !self
            .pairing
            .get(&access)
            .is_some_and(|&(code, deadline)| code == provided && Instant::now() < deadline)
        {
            return Ok(None);
        }
        let token = self.create_session(home, access, public_origin)?;
        self.pairing.remove(&access);
        Ok(Some(token))
    }

    fn create_session(
        &mut self,
        home: &Path,
        access: Access,
        public_origin: Option<&str>,
    ) -> Result<Uuid> {
        let token = Uuid::new_v4();
        let key = session_key(token, public_origin);
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
        Ok(token)
    }
}

struct Gateway {
    manager: Arc<host::Manager>,
    address: SocketAddrV4,
    access: Access,
    origin: String,
    public: Option<tunnel::State>,
    cookie: String,
    auth: Arc<Mutex<Auth>>,
    closed: CancellationToken,
    upstream: proxy::Credentials,
}

struct LanShare {
    gateways: Vec<Arc<Gateway>>,
}

impl LanShare {
    fn close(self) {
        for gateway in self.gateways {
            gateway.closed.cancel();
        }
    }
}

type PublicGateway = Arc<Mutex<Option<Arc<Gateway>>>>;

#[derive(Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct WebConfig {
    port: Option<u16>,
    lan: Option<Ipv4Addr>,
}

#[derive(Default)]
struct Web {
    config: WebConfig,
    local: Option<Arc<Gateway>>,
    lan: Option<LanShare>,
    tunnel: tunnel::Tunnel,
    error: Option<String>,
}

impl Web {
    fn local_address(&self) -> Option<SocketAddrV4> {
        self.local.as_ref().map(|gateway| gateway.address)
    }
}

struct Bindings {
    local: Arc<Gateway>,
    lan: Option<LanShare>,
    listeners: Vec<(Arc<Gateway>, tokio::net::TcpListener)>,
}

pub(super) struct Control {
    manager: Arc<host::Manager>,
    ports: &'static [u16],
    web: Mutex<Web>,
    public: PublicGateway,
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
    pub(super) fn local_address(&self) -> Option<SocketAddrV4> {
        self.web.lock().unwrap().local_address()
    }

    pub(super) fn new(manager: Arc<host::Manager>, ports: &'static [u16]) -> Result<Self> {
        let auth = Auth::load(&manager.home).unwrap_or_else(|error| {
            eprintln!("无法读取分享设备：{error:#}");
            Auth::default()
        });
        let control = Self {
            manager,
            ports,
            web: Mutex::default(),
            public: Arc::default(),
            auth: Arc::new(Mutex::new(auth)),
            runtime: tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?,
        };
        {
            let mut web = control.web.lock().unwrap();
            match read::<WebConfig>(&control.config_path()) {
                Ok(Some(config)) => web.config = config,
                Ok(None) => {}
                Err(error) => web.error = Some(format!("无法读取 Web 配置：{error:#}")),
            }
            let lan = web.config.lan;
            let restored = (|| -> Result<Option<Bindings>> {
                let Some(bind) = lan else {
                    return Ok(None);
                };
                let interfaces = lan_addresses();
                control
                    .select(&web, Self::lan_interfaces(&bind, &interfaces)?)
                    .map(Some)
            })();
            let bindings = match restored {
                Ok(Some(bindings)) => Ok(bindings),
                Ok(None) => control.select(&web, &[]),
                Err(error) => {
                    web.error = Some(format!("无法恢复局域网分享：{error:#}"));
                    control.select(&web, &[])
                }
            };
            match bindings.and_then(|bindings| {
                control.save_config(
                    &mut web,
                    WebConfig {
                        port: Some(bindings.local.address.port()),
                        lan,
                    },
                )?;
                Ok(bindings)
            }) {
                Ok(bindings) => control.activate(&mut web, bindings),
                Err(error) => web.error = Some(format!("{error:#}")),
            }
            if let Some(error) = &web.error {
                eprintln!("{error}");
            }
            control.restore_tunnel(&mut web);
        }
        Ok(control)
    }

    fn config_path(&self) -> PathBuf {
        self.manager.home.join("web.json")
    }

    fn save_config(&self, web: &mut Web, config: WebConfig) -> Result<()> {
        if config != web.config {
            host::publish(&self.config_path(), &config).context("无法保存 Web 配置")?;
            web.config = config;
        }
        Ok(())
    }

    fn lan_interfaces<'a>(
        bind: &'a Ipv4Addr,
        interfaces: &'a [Ipv4Addr],
    ) -> Result<&'a [Ipv4Addr]> {
        anyhow::ensure!(
            !bind.is_loopback() && !bind.is_multicast() && !bind.is_broadcast(),
            "请选择一个内网 IPv4 地址"
        );
        let addresses = if bind.is_unspecified() {
            interfaces
        } else {
            std::slice::from_ref(bind)
        };
        anyhow::ensure!(!addresses.is_empty(), "未找到可用的局域网 IPv4 地址");
        Ok(addresses)
    }

    fn select(&self, web: &Web, addresses: &[Ipv4Addr]) -> Result<Bindings> {
        let current = web
            .local_address()
            .map(|address| address.port())
            .or(web.config.port.filter(|port| self.ports.contains(port)));
        let ports = current.into_iter().chain(
            self.ports
                .iter()
                .copied()
                .filter(|port| Some(*port) != current),
        );
        let mut conflicts = Vec::new();
        for port in ports {
            match self.prepare(web, port, addresses) {
                Ok(bindings) => return Ok(bindings),
                Err(error)
                    if error.downcast_ref::<io::Error>().is_some_and(|error| {
                        matches!(
                            error.kind(),
                            io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
                        )
                    }) =>
                {
                    conflicts.push(format!("{error:#}"))
                }
                Err(error) => return Err(error),
            }
        }
        anyhow::bail!("没有可用的 Web 端口（{}）", conflicts.join("；"))
    }

    fn prepare(&self, web: &Web, port: u16, addresses: &[Ipv4Addr]) -> Result<Bindings> {
        let mut listeners = Vec::new();
        let mut bind = |ip, access, existing: Option<&Arc<Gateway>>| -> Result<_> {
            let address = SocketAddrV4::new(ip, port);
            if let Some(gateway) = existing.filter(|gateway| gateway.address == address) {
                return Ok(gateway.clone());
            }
            let socket = std::net::TcpListener::bind(address)
                .with_context(|| format!("无法监听 {address}"))?;
            socket.set_nonblocking(true)?;
            let _enter = self.runtime.enter();
            let socket = tokio::net::TcpListener::from_std(socket)?;
            let gateway = self.gateway(address, access, None)?;
            listeners.push((gateway.clone(), socket));
            Ok(gateway)
        };
        let local = bind(Ipv4Addr::LOCALHOST, Access::Local, web.local.as_ref())?;
        let mut gateways = Vec::new();
        for &ip in addresses {
            let existing = web.lan.as_ref().and_then(|share| {
                share
                    .gateways
                    .iter()
                    .find(|gateway| *gateway.address.ip() == ip)
            });
            gateways.push(bind(ip, Access::Lan, existing)?);
        }
        let lan = (!gateways.is_empty()).then_some(LanShare { gateways });
        Ok(Bindings {
            local,
            lan,
            listeners,
        })
    }

    fn activate(&self, web: &mut Web, bindings: Bindings) {
        let switched = web
            .local_address()
            .is_some_and(|address| address != bindings.local.address);
        if switched {
            self.stop_tunnel(web);
            self.auth.lock().unwrap().desktop_tickets.clear();
        }
        if let Some(old) = web.local.replace(bindings.local) {
            if switched {
                old.closed.cancel();
            }
        }
        if let Some(old) = std::mem::replace(&mut web.lan, bindings.lan) {
            for gateway in old.gateways {
                let reused = web.lan.as_ref().is_some_and(|share| {
                    share
                        .gateways
                        .iter()
                        .any(|next| Arc::ptr_eq(next, &gateway))
                });
                if !reused {
                    gateway.closed.cancel();
                }
            }
        }
        for (gateway, listener) in bindings.listeners {
            let public = (gateway.access == Access::Local).then(|| self.public.clone());
            self.runtime.spawn(gateway.serve(listener, public));
        }
        if switched {
            self.auth.lock().unwrap().pairing.remove(&Access::Local);
            self.restore_tunnel(web);
        }
    }

    fn gateway(
        &self,
        address: SocketAddrV4,
        access: Access,
        public: Option<tunnel::State>,
    ) -> Result<Arc<Gateway>> {
        Ok(Arc::new(Gateway {
            manager: self.manager.clone(),
            address,
            access,
            origin: format!("http://{address}"),
            cookie: if access == Access::Public {
                "wingmate_public".into()
            } else {
                format!("wingmate_share_{}", address.port())
            },
            public,
            auth: self.auth.clone(),
            closed: CancellationToken::new(),
            upstream: proxy::Credentials::new(access)?,
        }))
    }

    fn restore_tunnel(&self, web: &mut Web) {
        let result = (|| -> Result<()> {
            let config = tunnel::Config::load(&self.manager.home)?;
            if let Some(mode) = config.enabled {
                config.check(mode)?;
                self.start_tunnel(web, config, mode)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("无法恢复公网隧道：{error:#}");
            web.tunnel.failed(error);
        }
    }

    fn stop_tunnel(&self, web: &mut Web) {
        self.public.lock().unwrap().take();
        web.tunnel.stop();
        self.auth.lock().unwrap().pairing.remove(&Access::Public);
    }

    fn start_tunnel(
        &self,
        web: &mut Web,
        config: tunnel::Config,
        mode: tunnel::Mode,
    ) -> Result<()> {
        self.stop_tunnel(web);
        let address = web
            .local_address()
            .context("本机 Web 入口不可用，无法启动公网隧道")?;
        let state = tunnel::starting();
        let gateway = self.gateway(address, Access::Public, Some(state.clone()))?;
        web.tunnel.start(
            self.manager.home.clone(),
            config,
            mode,
            state,
            address,
            gateway.closed.clone(),
        );
        *self.public.lock().unwrap() = Some(gateway);
        Ok(())
    }

    pub(super) fn call(&self, command: Command) -> Result<ShareStatus> {
        let local_addresses = lan_addresses();
        let home = &self.manager.home;
        let mut web = self.web.lock().unwrap();
        let mut pairing_urls = Vec::new();
        match command {
            Command::Status => {}
            Command::Enable { address } => {
                anyhow::ensure!(
                    address != Some(Ipv4Addr::UNSPECIFIED),
                    "请选择一个本机 IPv4 地址"
                );
                let bind = address.unwrap_or(Ipv4Addr::UNSPECIFIED);
                let interfaces = Self::lan_interfaces(&bind, &local_addresses)?;
                let bindings = self.select(&web, interfaces)?;
                self.save_config(
                    &mut web,
                    WebConfig {
                        port: Some(bindings.local.address.port()),
                        lan: Some(bind),
                    },
                )?;
                let unchanged = web.lan.as_ref().is_some_and(|share| {
                    share
                        .gateways
                        .iter()
                        .map(|gateway| gateway.address)
                        .eq(bindings
                            .lan
                            .iter()
                            .flat_map(|share| &share.gateways)
                            .map(|gateway| gateway.address))
                });
                if !unchanged {
                    self.auth.lock().unwrap().pairing.remove(&Access::Lan);
                }
                web.error = None;
                let local_was_down = web.local.is_none();
                self.activate(&mut web, bindings);
                if local_was_down {
                    self.restore_tunnel(&mut web);
                }
            }
            Command::Disable => {
                let config = WebConfig {
                    lan: None,
                    ..web.config
                };
                self.save_config(&mut web, config)?;
                let mut auth = self.auth.lock().unwrap();
                if let Some(share) = web.lan.take() {
                    share.close();
                }
                if web.local.is_some() {
                    web.error = None;
                }
                auth.pairing.remove(&Access::Lan);
            }
            Command::Pair | Command::PairLocal => {
                let gateways = match command {
                    Command::PairLocal => {
                        std::slice::from_ref(web.local.as_ref().context("本机 Web 入口不可用")?)
                    }
                    _ => web
                        .lan
                        .as_ref()
                        .context("请先开启分享")?
                        .gateways
                        .as_slice(),
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
            Command::DesktopSession { instance } => {
                let local = web.local.as_ref().with_context(|| {
                    web.error
                        .clone()
                        .unwrap_or_else(|| "本机 Web 入口不可用".into())
                })?;
                anyhow::ensure!(
                    engine::status(home)?.is_some_and(|state| {
                        state.phase == "running" && state.instance == instance
                    }),
                    "业务后台实例已更换，请重新连接"
                );
                let ticket = Uuid::new_v4();
                let mut auth = self.auth.lock().unwrap();
                let now = Instant::now();
                auth.desktop_tickets
                    .retain(|_, ticket| ticket.deadline > now);
                auth.desktop_tickets.insert(
                    ticket,
                    DesktopTicket {
                        instance,
                        deadline: now + Duration::from_secs(30),
                    },
                );
                pairing_urls.push(format!(
                    "{}/_wingmate/desktop?ticket={ticket}",
                    local.origin
                ));
            }
            Command::Revoke { id } => {
                let mut auth = self.auth.lock().unwrap();
                auth.save(home, Some(id), None)?;
                auth.sessions.retain(|_, session| {
                    if session.device.id != id {
                        return true;
                    }
                    session.revoked.cancel();
                    false
                });
            }
            Command::TunnelConfigure { hostname, token } => {
                let mut config = tunnel::Config::load(home)?;
                config.configure(&hostname, token)?;
                config.save(home)?;
            }
            Command::TunnelStart { mode } => {
                let mut config = tunnel::Config::load(home)?;
                config.check(mode)?;
                config.enabled = Some(mode);
                config.save(home)?;
                if let Err(error) = self.start_tunnel(&mut web, config, mode) {
                    web.tunnel.failed(error);
                }
            }
            Command::TunnelStop => {
                let mut config = tunnel::Config::load(home)?;
                config.enabled = None;
                config.save(home)?;
                self.stop_tunnel(&mut web);
            }
            Command::TunnelPair => {
                let url = web
                    .tunnel
                    .status(home, web.local_address())
                    .url
                    .context("请先开启公网隧道")?;
                let code = Uuid::new_v4();
                self.auth
                    .lock()
                    .unwrap()
                    .pairing
                    .insert(Access::Public, (code, Instant::now() + PAIR_AGE));
                pairing_urls.push(format!("{url}/?auth={code}"));
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
        let lan_addresses: Vec<_> = web
            .lan
            .as_ref()
            .into_iter()
            .flat_map(|share| share.gateways.iter().map(|gateway| gateway.address))
            .collect();
        Ok(ShareStatus {
            address: lan_addresses.first().copied(),
            local_address: web.local_address(),
            web_error: web.error.clone(),
            suggested_address: local_addresses.first().copied(),
            lan_addresses,
            pairing_url: pairing_urls.first().cloned(),
            pairing_urls,
            devices,
            tunnel: Some(web.tunnel.status(home, web.local_address())),
        })
    }
}

impl Gateway {
    async fn serve(
        self: Arc<Self>,
        listener: tokio::net::TcpListener,
        public: Option<PublicGateway>,
    ) {
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
            let public = public.clone();
            tokio::spawn(async move {
                let service = service_fn(|request: Request<Incoming>| {
                    let target = public
                        .as_ref()
                        .and_then(|public| {
                            let host = request.headers().get(header::HOST)?.to_str().ok()?;
                            if gateway.origin.strip_prefix("http://") == Some(host) {
                                return None;
                            }
                            public.lock().unwrap().clone()
                        })
                        .unwrap_or_else(|| gateway.clone());
                    async move { Ok::<_, Infallible>(target.handle(request).await) }
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
        if self.closed.is_cancelled() {
            return response(StatusCode::SERVICE_UNAVAILABLE, "分享入口已关闭");
        }
        let headers = request.headers();
        let public = self
            .public
            .as_ref()
            .and_then(|state| state.lock().unwrap().url.clone());
        let (origin, authority) = match public.as_deref() {
            Some(origin) => (origin, origin.strip_prefix("https://").unwrap()),
            None if self.access == Access::Public => {
                return response(StatusCode::SERVICE_UNAVAILABLE, "公网隧道尚未就绪");
            }
            None => (
                self.origin.as_str(),
                self.origin.strip_prefix("http://").unwrap(),
            ),
        };
        if headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            != Some(authority)
        {
            return response(StatusCode::MISDIRECTED_REQUEST, "分享地址不匹配");
        }
        if headers
            .get(header::ORIGIN)
            .is_some_and(|value| value != origin)
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
        let query = request.uri().query();
        let root_navigation = request.method() == Method::GET && path == "/";
        let pairing_link = root_navigation && query.is_some_and(|query| query.starts_with("auth="));
        let wants_html = headers.get(header::ACCEPT).is_some_and(|value| {
            value
                .to_str()
                .is_ok_and(|value| value.contains("text/html"))
        });
        if request.method() == Method::POST && path == "/" && query == Some("auth") {
            return self.pair(request, public.as_deref()).await;
        }
        // The DSH mobile app probes for this public manifest before opening a saved host.
        if request.method() == Method::GET
            && path == "/manifest.webmanifest"
            && headers
                .get(header::ACCEPT)
                .is_some_and(|value| value == "application/manifest+json")
        {
            return content(
                StatusCode::OK,
                "application/manifest+json",
                Bytes::from_static(
                    br#"{"name":"DeepSeek Harness","short_name":"DSH","start_url":"/"}"#,
                ),
            );
        }
        if request.method() == Method::GET && path == "/_wingmate" {
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
                    .get(&session_key(token, public.as_deref()))
                    .filter(|session| session.device.access == self.access)
                    .cloned()
            })
            .filter(Session::active);
        if request.method() == Method::GET
            && path == "/_wingmate/desktop"
            && self.access == Access::Local
        {
            return self.desktop_session(query, session).await;
        }
        let Some(session) = session else {
            if pairing_link {
                return html(StatusCode::OK, PAIR_PAGE);
            }
            if root_navigation && (wants_html || query == Some("pair=1")) {
                return html(StatusCode::UNAUTHORIZED, PAIR_PAGE);
            }
            return response(StatusCode::UNAUTHORIZED, "请从主机获取新的配对链接");
        };
        if pairing_link {
            // A WebView may reopen the original one-time link while its cookie is still valid.
            let mut reply = response(StatusCode::SEE_OTHER, "");
            reply
                .headers_mut()
                .insert(header::LOCATION, header::HeaderValue::from_static("/"));
            return reply;
        }
        if (path == "/_wingmate" || path.starts_with("/_wingmate/")) && path != "/_wingmate/status"
        {
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
        let manager = self.manager.clone();
        let snapshot = tokio::task::spawn_blocking(move || engine::status(&manager.home)).await;
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
        if request.method() == Method::GET && wants_html {
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
        let manager = self.manager.clone();
        let closed = self.closed.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<host::Status> {
            anyhow::ensure!(
                !closed.is_cancelled() && session.active(),
                "连接已撤销或过期"
            );
            manager.execute(command)
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

    async fn pair(
        &self,
        request: Request<Incoming>,
        public_origin: Option<&str>,
    ) -> Response<Body> {
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
            auth.exchange(&self.manager.home, self.access, code, public_origin)
        };
        match token {
            Ok(Some(token)) => {
                let mut reply = response(StatusCode::NO_CONTENT, "");
                self.set_cookie(&mut reply, token);
                reply
            }
            Ok(None) => response(StatusCode::UNAUTHORIZED, "配对链接已失效，请从主机重新获取"),
            Err(error) => {
                eprintln!("无法保存配对设备：{error:#}");
                response(StatusCode::INTERNAL_SERVER_ERROR, "无法保存配对设备")
            }
        }
    }

    async fn desktop_session(
        &self,
        query: Option<&str>,
        session: Option<Session>,
    ) -> Response<Body> {
        let Some(ticket) = query
            .and_then(|query| query.strip_prefix("ticket="))
            .and_then(|value| Uuid::parse_str(value).ok())
        else {
            return response(StatusCode::BAD_REQUEST, "桌面连接凭据无效");
        };
        let pending = self.auth.lock().unwrap().desktop_tickets.remove(&ticket);
        let Some(pending) = pending.filter(|pending| pending.deadline > Instant::now()) else {
            return response(StatusCode::UNAUTHORIZED, "桌面连接凭据已失效");
        };
        let manager = self.manager.clone();
        match tokio::task::spawn_blocking(move || engine::status(&manager.home)).await {
            Ok(Ok(Some(state)))
                if state.phase == "running" && state.instance == pending.instance => {}
            Ok(Ok(_)) => return response(StatusCode::CONFLICT, "业务后台实例已更换，请重新连接"),
            _ => return response(StatusCode::BAD_GATEWAY, "无法读取业务后台状态"),
        }
        let mut auth = self.auth.lock().unwrap();
        if self.closed.is_cancelled() {
            return response(StatusCode::SERVICE_UNAVAILABLE, "本机 Web 入口已关闭");
        }
        let mut reply = response(StatusCode::SEE_OTHER, "");
        reply
            .headers_mut()
            .insert(header::LOCATION, header::HeaderValue::from_static("/"));
        if session.is_some_and(|session| session.active()) {
            return reply;
        }
        match auth.create_session(&self.manager.home, Access::Local, None) {
            Ok(token) => {
                self.set_cookie(&mut reply, token);
                reply
            }
            Err(error) => {
                eprintln!("无法建立桌面会话：{error:#}");
                response(StatusCode::INTERNAL_SERVER_ERROR, "无法建立桌面会话")
            }
        }
    }

    fn set_cookie(&self, reply: &mut Response<Body>, token: Uuid) {
        let secure = if self.access == Access::Public {
            "; Secure"
        } else {
            ""
        };
        reply.headers_mut().insert(
            header::SET_COOKIE,
            format!(
                "{}={token}; Path=/; Max-Age={SESSION_AGE}; HttpOnly; SameSite=Strict{secure}",
                self.cookie
            )
            .parse()
            .unwrap(),
        );
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
    html(
        StatusCode::OK,
        include_str!("../../../apps/launcher/index.html"),
    )
}

fn html(status: StatusCode, text: &'static str) -> Response<Body> {
    let mut reply = content(
        status,
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
        assert!(auth
            .exchange(home.path(), Access::Lan, code, None)?
            .is_none());
        assert!(auth.sessions.is_empty());
        Ok(())
    }
}
