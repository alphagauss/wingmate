//! 管理客户端及发行引导；业务进程只由独立服务操作。
use crate::components;
use anyhow::{Context, Result};
pub use engine::{operation_lock, Snapshot, StartupStage};
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub use wingmate_service::host::{
    ShareCommand, ShareStatus, Status as HostStatus, TunnelMode, DEV_WEB_PORTS, WEB_PORTS,
};
use wingmate_service::{engine, host, runtime::Runtime};

#[derive(serde::Serialize, serde::Deserialize)]
pub enum BootstrapEvent {
    Stage(StartupStage),
    Ready(Box<Snapshot>),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RestartTarget {
    pub service: String,
    pub business: Option<String>,
}

impl RestartTarget {
    pub fn from_status(status: &HostStatus) -> Self {
        Self {
            service: status.instance.clone(),
            business: status.business.as_ref().map(|state| state.instance.clone()),
        }
    }

    pub fn matches(&self, status: &HostStatus) -> bool {
        self.service == status.instance
            && self.business.as_deref()
                == status
                    .business
                    .as_ref()
                    .map(|state| state.instance.as_str())
    }
}

pub(crate) fn bootstrap_lock(home: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join("bootstrap.lock"))?;
    fs2::FileExt::lock_exclusive(&file)?;
    Ok(file)
}

pub fn status(home: &Path) -> Result<Option<Snapshot>> {
    Ok(connection_status(home)?.and_then(|host| host.business))
}

pub fn share(home: &Path, command: &ShareCommand) -> Result<ShareStatus> {
    let endpoint = host::endpoint(home)?.context("请先启动管理服务")?;
    host::share(&endpoint, command)
}

pub fn desktop_session(home: &Path, service: &str, instance: &str) -> Result<String> {
    let endpoint = host::endpoint(home)?.context("管理服务已退出")?;
    anyhow::ensure!(
        endpoint.instance == service,
        "管理服务实例已更换，请重新连接"
    );
    host::share(
        &endpoint,
        &ShareCommand::DesktopSession {
            instance: instance.into(),
        },
    )?
    .pairing_url
    .context("管理服务未提供桌面连接凭据")
}

pub fn dsh_entry(home: &Path, state: &Snapshot) -> Result<std::path::PathBuf> {
    crate::runtime::installed_dsh_entry(home, state.runtime_revision.as_deref())
}

fn ensure_host(home: &Path) -> Result<host::Endpoint> {
    if let Some(endpoint) = host::endpoint(home)? {
        host::call(&endpoint, "status", None)?;
        return Ok(endpoint);
    }
    start_prepared_host(home, components::prepare(home, false)?)
}

fn start_prepared_host(
    home: &Path,
    prepared: components::PreparedBinary,
) -> Result<host::Endpoint> {
    let revision = prepared.revision.clone();
    let binary = prepared.publish()?;
    let log = File::create(home.join("host.log"))?;
    let mut command = Command::new(&binary);
    crate::runtime::quiet(&mut command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    wingmate_service::runtime::isolate_standard_pipes()?;
    command.arg("--home").arg(home);
    if crate::IS_DEV {
        command.arg("--dev");
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        anyhow::ensure!(
            child.try_wait()?.is_none(),
            "管理服务启动失败，请检查 host.log"
        );
        if let Ok(Some(endpoint)) = host::endpoint(home) {
            if host::call(&endpoint, "status", None).is_ok() {
                anyhow::ensure!(
                    endpoint.pid == child.id() && endpoint.revision == revision,
                    "启动期间管理服务实例已更换"
                );
                return Ok(endpoint);
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("管理服务启动超时，请检查 host.log");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn prepare(home: &Path, report: impl Fn(StartupStage)) -> Result<()> {
    let _operation = operation_lock(home)?;
    anyhow::ensure!(
        engine::status(home)?.is_none(),
        "业务后台已启动，请重新连接后再切换运行内容"
    );
    let directory = crate::runtime::prepare(home, || report(StartupStage::Extracting))?;
    let node = crate::runtime::node_info()?;
    let runtime = Runtime {
        id: crate::runtime::ID.into(),
        revision: crate::runtime::REVISION.context("缺少运行组件清单")?.into(),
        directory,
        node: node.path,
        #[cfg(target_os = "macos")]
        search_path: node.search_path,
        dsh_home: if crate::IS_DEV {
            Some(home.parent().unwrap().join("dsh"))
        } else {
            std::env::var_os("DSH_HOME").map(Into::into)
        },
        agents_home: if crate::IS_DEV {
            Some(home.parent().unwrap().join("agents"))
        } else {
            std::env::var_os("DSH_AGENTS_HOME").map(Into::into)
        },
    };
    runtime.validate(home)?;
    host::publish(&home.join("runtime.json"), &runtime)
}

fn start_local(
    home: &Path,
    expected: Option<&RestartTarget>,
    restart: bool,
    report: impl Fn(StartupStage),
    ready: impl FnOnce(&Snapshot),
) -> Result<Snapshot> {
    let _bootstrap = bootstrap_lock(home)?;
    report(StartupStage::Checking);
    let desired = components::service_revision()?;
    let mut endpoint = if let Some(target) = expected {
        let current = host::endpoint(home)?.context("原管理服务已退出，请重新检查")?;
        anyhow::ensure!(
            current.instance == target.service,
            "管理服务实例已更换，请重新检查"
        );
        current
    } else {
        ensure_host(home)?
    };
    let mut prepared = false;
    if restart {
        let current = host::call(&endpoint, "status", None)?;
        anyhow::ensure!(
            expected.is_none_or(|target| target.matches(&current)),
            "后台实例已更换，请重新检查"
        );
        let replacement = if endpoint.revision != desired {
            let binary = components::prepare(home, false)?;
            anyhow::ensure!(binary.revision == desired, "服务构建产物已变化，请重试");
            Some(binary)
        } else {
            None
        };
        if let Some(state) = current.business {
            report(StartupStage::Stopping);
            host::call(&endpoint, "stop", Some(&state.instance))?;
        }
        if let Some(binary) = replacement {
            report(StartupStage::Stopping);
            shutdown_endpoint(home, &endpoint).context("退出旧管理服务失败")?;
            prepare(home, &report)?;
            prepared = true;
            report(StartupStage::Starting);
            endpoint = start_prepared_host(home, binary).context("启动新版管理服务失败")?;
        }
    }
    let state = if let Some(state) = host::call(&endpoint, "status", None)?.business {
        report(StartupStage::Connecting);
        let deadline = Instant::now() + Duration::from_secs(150);
        let mut state = state;
        while state.phase == "starting" {
            anyhow::ensure!(
                Instant::now() < deadline,
                "等待业务后台就绪超时，请检查 service.log"
            );
            std::thread::sleep(Duration::from_millis(100));
            state = host::call(&endpoint, "status", None)?
                .business
                .context("业务后台已退出")?;
        }
        anyhow::ensure!(state.phase == "running", "业务后台正在停止，请稍后重试");
        state
    } else {
        if !prepared {
            prepare(home, &report)?;
        }
        report(StartupStage::Starting);
        host::call(&endpoint, "start", None)?
            .business
            .context("业务后台未就绪")?
    };
    anyhow::ensure!(
        !restart
            || (endpoint.revision == desired
                && state.runtime_revision.as_deref() == crate::RUNTIME_REVISION),
        "启动的后台与目标组件不一致，请重新连接"
    );
    ready(&state);
    if endpoint.revision == desired {
        let cleanup = (|| -> Result<()> {
            let _operation = operation_lock(home)?;
            if let Some(current) = engine::status(home)?.filter(|current| {
                current.instance == state.instance
                    && current.phase == "running"
                    && current.runtime_revision.as_deref() == crate::RUNTIME_REVISION
            }) {
                crate::runtime::cleanup_after_ready(home, &current);
            }
            Ok(())
        })();
        if let Err(error) = cleanup {
            log_cleanup_error(home, error);
        }
    }
    Ok(state)
}

fn start_client(
    home: &Path,
    expected: Option<&RestartTarget>,
    restart: bool,
    report: impl Fn(StartupStage),
    ready: impl FnOnce(&Snapshot),
) -> Result<Snapshot> {
    if let Some(executable) = std::env::var_os("WINGMATE_LAUNCHER") {
        report(if restart {
            StartupStage::Stopping
        } else {
            StartupStage::Checking
        });
        let mut command = Command::new(executable);
        crate::runtime::quiet(&mut command);
        command.arg(if restart {
            "internal-restart"
        } else {
            "internal-start"
        });
        if let Some(target) = expected {
            command.arg(serde_json::to_string(target)?);
        }
        let mut child = command
            .env("WINGMATE_HOME", home)
            .env_remove("WINGMATE_LAUNCHER")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut ready = Some(ready);
        let mut state = None;
        for line in BufReader::new(child.stdout.take().unwrap()).lines() {
            match serde_json::from_str::<BootstrapEvent>(&line?)? {
                BootstrapEvent::Stage(stage) => report(stage),
                BootstrapEvent::Ready(value) => {
                    if let Some(ready) = ready.take() {
                        ready(&value);
                    }
                    state = Some(*value);
                }
            }
        }
        let output = child.wait_with_output()?;
        anyhow::ensure!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        state.context("发行入口未返回业务状态")
    } else {
        start_local(home, expected, restart, report, ready)
    }
}

pub fn start(home: &Path) -> Result<Snapshot> {
    start_with_progress(home, |_| {}, |_| {})
}
pub fn start_with_progress(
    home: &Path,
    report: impl Fn(StartupStage),
    ready: impl FnOnce(&Snapshot),
) -> Result<Snapshot> {
    start_client(home, None, false, report, ready)
}
pub fn restart(home: &Path) -> Result<Snapshot> {
    restart_with_progress(home, None, |_| {}, |_| {})
}
pub fn restart_with_progress(
    home: &Path,
    target: Option<&RestartTarget>,
    report: impl Fn(StartupStage),
    ready: impl FnOnce(&Snapshot),
) -> Result<Snapshot> {
    start_client(home, target, true, report, ready)
}

pub fn request_desktop_reopen(home: &Path) -> Result<()> {
    let executable = std::env::var_os("WINGMATE_LAUNCHER").context("缺少 Wingmate 发行入口")?;
    let log = home.join("desktop-update.log");
    let mut command = Command::new(executable);
    crate::runtime::quiet(&mut command);
    let mut child = command
        .arg("internal-reopen-desktop")
        .env("WINGMATE_HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(File::create(&log)?))
        .spawn()?;
    let mut reply = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut reply)?;
    if reply.trim() != "ready" {
        child.wait()?;
        let detail = std::fs::read_to_string(&log).unwrap_or_default();
        anyhow::bail!("无法重开窗口。{}", detail.trim());
    }
    Ok(())
}

pub fn stop(home: &Path) -> Result<()> {
    stop_request(home, None)
}
pub fn stop_instance(home: &Path, instance: &str) -> Result<()> {
    stop_request(home, Some(instance))
}
pub fn restart_instance(home: &Path, instance: &str) -> Result<HostStatus> {
    let endpoint = host::endpoint(home)?.context("管理服务未运行，请先恢复连接")?;
    host::call(&endpoint, "restart", Some(instance))
}
fn stop_request(home: &Path, instance: Option<&str>) -> Result<()> {
    match host::endpoint(home)? {
        Some(endpoint) => {
            host::call(&endpoint, "stop", instance)?;
        }
        None => {
            anyhow::ensure!(
                instance.is_none() && engine::status(home)?.is_none(),
                "管理服务未运行，请先使用 wingmate start 恢复连接"
            );
        }
    }
    Ok(())
}

pub fn host_status(home: &Path) -> Result<Option<HostStatus>> {
    host::endpoint(home)?
        .map(|endpoint| host::call(&endpoint, "status", None))
        .transpose()
}

pub fn connection_status(home: &Path) -> Result<Option<HostStatus>> {
    let host = host_status(home)?;
    anyhow::ensure!(
        host.is_some() || engine::status(home)?.is_none(),
        "管理服务已退出，请使用 wingmate start 恢复连接"
    );
    Ok(host)
}
fn shutdown_endpoint(_home: &Path, endpoint: &host::Endpoint) -> Result<()> {
    #[cfg(windows)]
    let process = crate::process_wait::open(endpoint.pid)?.context("管理服务已退出")?;
    host::call(endpoint, "shutdown", None)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    #[cfg(windows)]
    {
        crate::process_wait::wait(&process, deadline, "管理服务尚未退出")
    }
    #[cfg(not(windows))]
    {
        while host::try_lock(_home)?.is_none() {
            anyhow::ensure!(Instant::now() < deadline, "管理服务尚未退出");
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }
}
pub fn shutdown(home: &Path) -> Result<()> {
    let _bootstrap = bootstrap_lock(home)?;
    if let Some(endpoint) = host::endpoint(home)? {
        shutdown_endpoint(home, &endpoint)?;
    }
    Ok(())
}
pub fn restart_host(home: &Path) -> Result<()> {
    let _bootstrap = bootstrap_lock(home)?;
    let binary = components::prepare(home, false)?;
    if let Some(endpoint) = host::endpoint(home)? {
        shutdown_endpoint(home, &endpoint)?;
    }
    start_prepared_host(home, binary)?;
    Ok(())
}

fn log_cleanup_error(home: &Path, error: anyhow::Error) {
    let message = format!("组件清理失败，下次启动重试: {error:#}");
    eprintln!("{message}");
    if let Ok(mut log) = OpenOptions::new()
        .append(true)
        .open(home.join("service.log"))
    {
        let _ = writeln!(log, "{message}");
    }
}

pub fn start_host(home: &Path) -> Result<()> {
    let _bootstrap = bootstrap_lock(home)?;
    let endpoint = ensure_host(home)?;
    if host::call(&endpoint, "status", None)?.business.is_none() {
        prepare(home, |_| {})?;
    }
    Ok(())
}
