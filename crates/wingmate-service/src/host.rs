//! 同一用户的本机管理端点；运行资源由发行入口准备。
use crate::engine::{self, Snapshot};
use crate::gateway;
pub use crate::gateway::{Command as ShareCommand, ShareStatus};
pub use crate::tunnel::Mode as TunnelMode;
use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    net::SocketAddrV4,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

const PRODUCT: &str = "wingmate-service";
const PROTOCOL: u32 = 1;
pub const WEB_PORTS: &[u16] = &[8746, 8747, 8748];
pub const DEV_WEB_PORTS: &[u16] = &[8749];

#[derive(Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub product: String,
    pub protocol: u32,
    pub instance: String,
    pub pid: u32,
    pub revision: String,
    pub port: u16,
    pub token: String,
}

#[derive(Serialize, Deserialize)]
pub struct Status {
    pub product: String,
    pub protocol: u32,
    pub instance: String,
    pub pid: u32,
    pub revision: String,
    pub business: Option<Snapshot>,
    pub web_address: Option<SocketAddrV4>,
}

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub action: String,
    pub expected_instance: Option<String>,
}

pub fn try_lock(home: &Path) -> Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.join("host.lock"))?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(file)),
        Err(error)
            if error.kind() == std::io::ErrorKind::WouldBlock
                || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
        {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

pub fn endpoint(home: &Path) -> Result<Option<Endpoint>> {
    if try_lock(home)?.is_some() {
        return Ok(None);
    }
    let endpoint: Endpoint =
        serde_json::from_slice(&fs::read(home.join("host.json")).context("管理服务尚未就绪")?)?;
    anyhow::ensure!(
        endpoint.product == PRODUCT
            && endpoint.protocol == PROTOCOL
            && endpoint.port != 0
            && !endpoint.token.is_empty()
            && !endpoint.instance.is_empty(),
        "管理服务身份或协议不匹配"
    );
    Ok(Some(endpoint))
}

pub fn call(endpoint: &Endpoint, action: &str, expected_instance: Option<&str>) -> Result<Status> {
    let state: Status = call_local(
        endpoint,
        "/rpc",
        &Request {
            action: action.into(),
            expected_instance: expected_instance.map(str::to_owned),
        },
    )?;
    anyhow::ensure!(
        state.product == endpoint.product
            && state.protocol == endpoint.protocol
            && state.instance == endpoint.instance
            && state.pid == endpoint.pid
            && state.revision == endpoint.revision,
        "管理服务身份检查失败"
    );
    Ok(state)
}

pub fn share(endpoint: &Endpoint, command: &ShareCommand) -> Result<ShareStatus> {
    call_local(endpoint, "/share", command)
}

fn call_local<T: serde::de::DeserializeOwned>(
    endpoint: &Endpoint,
    path: &str,
    body: &impl Serialize,
) -> Result<T> {
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(180))
        .build()?;
    let response = client
        .post(format!("http://127.0.0.1:{}{path}", endpoint.port))
        .bearer_auth(&endpoint.token)
        .header("X-Wingmate-Instance", &endpoint.instance)
        .json(body)
        .send()?;
    let status = response.status();
    if !status.is_success() {
        if path == "/share" && status == reqwest::StatusCode::NOT_FOUND {
            anyhow::bail!(
                "当前管理服务版本不支持局域网分享，请重启后台以更新管理服务（wingmate restart）"
            );
        }
        let message = response.text()?;
        anyhow::ensure!(
            !message.trim().is_empty(),
            "管理服务请求失败（HTTP {status}）"
        );
        anyhow::bail!("{message}");
    }
    Ok(response.json()?)
}

pub fn revision(executable: &Path) -> Result<String> {
    let mut digest = Sha256::new();
    std::io::copy(&mut File::open(executable)?, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()))
}

pub fn publish(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.persist(path)?;
    Ok(())
}

pub(super) struct Manager {
    pub(super) home: PathBuf,
    endpoint: Endpoint,
    stopping: AtomicBool,
    mutations: Mutex<()>,
}

impl Manager {
    pub(super) fn execute(&self, command: Request) -> Result<Status> {
        let home = &self.home;
        if command.action != "status" {
            let _mutation = self
                .mutations
                .try_lock()
                .map_err(|_| anyhow::anyhow!("后台操作正在进行，请稍后重试"))?;
            anyhow::ensure!(!self.stopping.load(Ordering::Acquire), "管理服务正在退出");
            match command.action.as_str() {
                "start" => {
                    engine::start(home)?;
                }
                "stop" => match command.expected_instance.as_deref() {
                    Some(instance) => engine::stop_instance(home, instance)?,
                    None => engine::stop(home)?,
                },
                "restart" => match command.expected_instance.as_deref() {
                    Some(instance) => {
                        engine::restart_instance_with_progress(home, instance, |_| {}, |_| {})?;
                    }
                    None => {
                        engine::restart(home)?;
                    }
                },
                "shutdown" => {
                    let _operation = engine::operation_lock(home)?;
                    anyhow::ensure!(
                        engine::status(home)?.is_none(),
                        "请先停止业务后台，再退出管理服务"
                    );
                    self.stopping.store(true, Ordering::Release);
                }
                _ => anyhow::bail!("未知管理操作"),
            }
        }
        Ok(Status {
            product: self.endpoint.product.clone(),
            protocol: PROTOCOL,
            instance: self.endpoint.instance.clone(),
            pid: self.endpoint.pid,
            revision: self.endpoint.revision.clone(),
            business: engine::status(home)?,
            web_address: None,
        })
    }
}

pub fn run(home: &Path, ports: &'static [u16]) -> Result<()> {
    fs::create_dir_all(home)?;
    let _owner = try_lock(home)?.context("管理服务已经运行")?;
    let server =
        Arc::new(tiny_http::Server::http("127.0.0.1:0").map_err(|e| anyhow::anyhow!("{e}"))?);
    let endpoint = Endpoint {
        product: PRODUCT.into(),
        protocol: PROTOCOL,
        instance: uuid::Uuid::new_v4().to_string(),
        pid: std::process::id(),
        revision: revision(&std::env::current_exe()?)?,
        port: server.server_addr().to_ip().unwrap().port(),
        token: uuid::Uuid::new_v4().to_string(),
    };
    let manager = Arc::new(Manager {
        home: home.to_owned(),
        endpoint,
        stopping: AtomicBool::new(false),
        mutations: Mutex::new(()),
    });
    let gateway = Arc::new(gateway::Control::new(manager.clone(), ports)?);
    let mut workers = Vec::new();
    for _ in 0..4 {
        let (server, manager, gateway) = (server.clone(), manager.clone(), gateway.clone());
        workers.push(std::thread::spawn(move || -> Result<()> {
            let endpoint = &manager.endpoint;
            while !manager.stopping.load(Ordering::Acquire) {
                let Some(mut request) = server.recv_timeout(Duration::from_millis(200))? else {
                    continue;
                };
                let authorized = |name: &'static str, value: &str| {
                    request
                        .headers()
                        .iter()
                        .any(|header| header.field.equiv(name) && header.value.as_str() == value)
                };
                let share = request.url() == "/share";
                let code = if request.method() != &tiny_http::Method::Post
                    || (!share && request.url() != "/rpc")
                {
                    404
                } else if !authorized("Authorization", &format!("Bearer {}", endpoint.token))
                    || !authorized("X-Wingmate-Instance", &endpoint.instance)
                {
                    401
                } else if !request
                    .body_length()
                    .is_some_and(|length| length > 0 && length <= 4096)
                {
                    400
                } else {
                    200
                };
                if code != 200 {
                    let _ = request.respond(tiny_http::Response::empty(code));
                    continue;
                }
                let result = (|| -> Result<String> {
                    let mut body = String::new();
                    request.as_reader().take(4097).read_to_string(&mut body)?;
                    if share {
                        let state = gateway.call(serde_json::from_str(&body)?)?;
                        return Ok(serde_json::to_string(&state)?);
                    }
                    let mut state = manager.execute(serde_json::from_str(&body)?)?;
                    state.web_address = gateway.local_address();
                    Ok(serde_json::to_string(&state)?)
                })();
                let (code, body) = match result {
                    Ok(body) => (200, body),
                    Err(error) => (409, format!("{error:#}")),
                };
                let _ = request.respond(
                    tiny_http::Response::from_string(body)
                        .with_status_code(code)
                        .with_header(
                            tiny_http::Header::from_bytes("Content-Type", "application/json")
                                .unwrap(),
                        ),
                );
            }
            Ok(())
        }));
    }
    publish(&home.join("host.json"), &manager.endpoint)?;
    for worker in workers {
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("管理服务工作线程退出"))??;
    }
    fs::remove_file(home.join("host.json"))?;
    Ok(())
}
