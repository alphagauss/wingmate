//! 管理已安装的业务运行时；操作锁与发行引导共用，Node 持有业务运行锁。
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
    process::Child,
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};

const PROTOCOL: u32 = 2;
const PRODUCT: &str = "wingmate";

#[derive(Deserialize)]
struct Endpoint {
    product: String,
    protocol: u32,
    instance: String,
    runtime: String,
    pid: u32,
    port: u16,
    token: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub product: String,
    pub protocol: u32,
    pub instance: String,
    pub runtime: String,
    pub runtime_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dsh_home: Option<std::path::PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents_home: Option<std::path::PathBuf>,
    pub pid: u32,
    pub engine_pid: Option<u32>,
    pub phase: String,
    pub url: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum StartupStage {
    Checking,
    Connecting,
    Extracting,
    Starting,
    Stopping,
}

/// Hold while performing an operation that must not race with backend replacement.
pub fn operation_lock(home: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.join("operation.lock"))?;
    FileExt::lock_exclusive(&file)?;
    Ok(file)
}

fn endpoint(home: &Path) -> Result<Endpoint> {
    let endpoint: Endpoint = serde_json::from_slice(
        &fs::read(home.join("endpoint.json")).context("后台尚未发布发现信息")?,
    )?;
    anyhow::ensure!(
        endpoint.product == PRODUCT
            && endpoint.protocol == PROTOCOL
            && endpoint.port != 0
            && !endpoint.token.is_empty()
            && !endpoint.instance.is_empty(),
        "后台发现文件无效或协议不匹配"
    );
    Ok(endpoint)
}

fn call(endpoint: &Endpoint, action: &str) -> Result<Snapshot> {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    let client = match CLIENT.get() {
        Some(client) => client,
        None => {
            let client = reqwest::blocking::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(20))
                .build()?;
            CLIENT.get_or_init(|| client)
        }
    };
    let response = client
        .post(format!("http://127.0.0.1:{}/rpc", endpoint.port))
        .bearer_auth(&endpoint.token)
        .header("X-Wingmate-Instance", &endpoint.instance)
        .body(action.to_owned())
        .send()?
        .error_for_status()?;
    let snapshot: Snapshot = response.json()?;
    validate_identity(endpoint, &snapshot)?;
    Ok(snapshot)
}

fn validate_identity(endpoint: &Endpoint, snapshot: &Snapshot) -> Result<()> {
    anyhow::ensure!(
        snapshot.product == PRODUCT
            && snapshot.protocol == PROTOCOL
            && snapshot.instance == endpoint.instance
            && snapshot.runtime == endpoint.runtime
            && snapshot.pid == endpoint.pid
            && snapshot.engine_pid == Some(endpoint.pid),
        "后台身份检查失败，拒绝操作此端点"
    );
    if let Some(url) = &snapshot.url {
        anyhow::ensure!(crate::process::ready_url(url), "后台返回了无效的 DSH 地址");
    }
    Ok(())
}

pub fn status(home: &Path) -> Result<Option<Snapshot>> {
    if !crate::process::occupied(home)? {
        return Ok(None);
    }
    let result = endpoint(home).and_then(|current| call(&current, "status"));
    if result.is_err() && !crate::process::occupied(home)? {
        return Ok(None);
    }
    result.map(Some)
}

fn wait_ready(home: &Path, mut child: Option<&mut Child>) -> Result<Snapshot> {
    let deadline = Instant::now() + Duration::from_secs(150);
    loop {
        if let Some(child) = child.as_mut() {
            if let Some(exit) = child.try_wait()? {
                bail!("DSH 启动退出（{exit}），请检查 service.log");
            }
        }
        anyhow::ensure!(
            crate::process::occupied(home)?,
            "后台启动失败，请检查 service.log"
        );
        if home.join("endpoint.json").is_file() {
            let state = call(&endpoint(home)?, "status")?;
            match state.phase.as_str() {
                "running" => return Ok(state),
                "starting" => {}
                _ => bail!(
                    "{}",
                    state.error.as_deref().unwrap_or("DSH 未运行，请重启后台")
                ),
            }
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "等待后台启动超时，请检查 service.log"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

fn start_or_connect_locked(home: &Path, report: impl Fn(StartupStage)) -> Result<Snapshot> {
    let Some(lock) = crate::process::try_acquire(home)? else {
        report(StartupStage::Connecting);
        return wait_ready(home, None);
    };
    match fs::remove_file(home.join("endpoint.json")) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let runtime = crate::runtime::Runtime::load(home)?;
    runtime.validate(home)?;
    report(StartupStage::Starting);
    let (mut child, _control) = crate::process::start(home, &runtime, lock)?;
    let result = wait_ready(home, Some(&mut child));
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    } else {
        // 常驻宿主必须回收已退出的子进程，不能沿用短命启动器的隐式回收。
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    result
}

fn start_locked(
    home: &Path,
    report: impl Fn(StartupStage),
    ready: impl FnOnce(&Snapshot),
) -> Result<Snapshot> {
    let state = start_or_connect_locked(home, report)?;
    ready(&state);
    Ok(state)
}

pub fn start(home: &Path) -> Result<Snapshot> {
    start_with_progress(home, |_| {}, |_| {})
}

pub fn start_with_progress(
    home: &Path,
    report: impl Fn(StartupStage),
    ready: impl FnOnce(&Snapshot),
) -> Result<Snapshot> {
    report(StartupStage::Checking);
    let _operation = operation_lock(home)?;
    start_locked(home, report, ready)
}

fn stop_locked(home: &Path, expected_instance: Option<&str>) -> Result<()> {
    if !crate::process::occupied(home)? {
        anyhow::ensure!(expected_instance.is_none(), "后台已停止，请重新连接");
        return Ok(());
    }
    // 先验证活跃端点的身份，未知服务不会收到停止指令。
    let current = endpoint(home)?;
    anyhow::ensure!(
        expected_instance.is_none_or(|instance| instance == current.instance),
        "后台实例已更换，请重新连接"
    );
    call(&current, "status")?;
    call(&current, "stop")?;
    let deadline = Instant::now() + Duration::from_secs(20);
    while crate::process::occupied(home)? {
        anyhow::ensure!(
            Instant::now() < deadline,
            "后台尚未释放运行锁，停止结果未确认"
        );
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

pub fn stop(home: &Path) -> Result<()> {
    let _operation = operation_lock(home)?;
    stop_locked(home, None)
}

pub fn stop_instance(home: &Path, expected_instance: &str) -> Result<()> {
    let _operation = operation_lock(home)?;
    stop_locked(home, Some(expected_instance))
}

pub fn restart(home: &Path) -> Result<Snapshot> {
    restart_with_progress(home, None, |_| {}, |_| {})
}

pub fn restart_instance_with_progress(
    home: &Path,
    expected_instance: &str,
    report: impl Fn(StartupStage),
    ready: impl FnOnce(&Snapshot),
) -> Result<Snapshot> {
    restart_with_progress(home, Some(expected_instance), report, ready)
}

fn restart_with_progress(
    home: &Path,
    expected_instance: Option<&str>,
    report: impl Fn(StartupStage),
    ready: impl FnOnce(&Snapshot),
) -> Result<Snapshot> {
    report(StartupStage::Stopping);
    let _operation = operation_lock(home)?;
    stop_locked(home, expected_instance)?;
    report(StartupStage::Checking);
    start_locked(home, report, ready)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_not_stale_file_decides_ownership() {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join("endpoint.json"), "stale").unwrap();
        assert!(!crate::process::occupied(home.path()).unwrap());
        let owner = crate::process::try_acquire(home.path()).unwrap().unwrap();
        assert!(crate::process::occupied(home.path()).unwrap());
        drop(owner);
        assert!(!crate::process::occupied(home.path()).unwrap());
    }

    #[test]
    fn instance_bound_restart_rejects_a_replaced_backend() -> Result<()> {
        let home = tempfile::tempdir()?;
        let _owner = crate::process::try_acquire(home.path())?.unwrap();
        let current = serde_json::json!({
            "product": PRODUCT, "protocol": PROTOCOL, "runtime": "dsh-test",
            "instance": "new", "pid": 1, "port": 1, "token": "test",
        });
        fs::write(home.path().join("endpoint.json"), current.to_string())?;

        let result = restart_instance_with_progress(home.path(), "old", |_| {}, |_| {});
        assert_eq!(
            result.err().unwrap().to_string(),
            "后台实例已更换，请重新连接"
        );
        assert!(crate::process::occupied(home.path())?);
        Ok(())
    }

    #[test]
    fn rejects_identity_mismatch_even_with_valid_json() {
        let endpoint = Endpoint {
            product: PRODUCT.into(),
            protocol: PROTOCOL,
            instance: "a".into(),
            runtime: "r".into(),
            pid: 1,
            port: 80,
            token: "secret".into(),
        };
        let mut state = Snapshot {
            product: PRODUCT.into(),
            protocol: PROTOCOL,
            instance: "b".into(),
            runtime: "r".into(),
            pid: 1,
            engine_pid: Some(1),
            runtime_revision: None,
            dsh_home: None,
            agents_home: None,
            phase: "running".into(),
            url: None,
            error: None,
        };
        assert!(validate_identity(&endpoint, &state).is_err());
        state.instance = "a".into();
        assert!(validate_identity(&endpoint, &state).is_ok());
        state.url = Some("http://example.com/?token=x".into());
        assert!(validate_identity(&endpoint, &state).is_err());
    }
}
