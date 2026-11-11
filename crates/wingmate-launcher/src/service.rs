//! 管理客户端及发行引导；业务进程只由独立服务操作。
use anyhow::{Context, Result};
pub use engine::{operation_lock, Snapshot, StartupStage};
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub use wingmate_service::host::{ShareCommand, ShareStatus};
use wingmate_service::{engine, host, runtime::Runtime};

#[derive(serde::Serialize, serde::Deserialize)]
pub enum BootstrapEvent {
    Stage(StartupStage),
    Ready(Box<Snapshot>),
}

fn bootstrap_lock(home: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join("bootstrap.lock"))?;
    fs2::FileExt::lock_exclusive(&file)?;
    Ok(file)
}

pub fn status(home: &Path) -> Result<Option<Snapshot>> {
    match host::endpoint(home)? {
        Some(endpoint) => Ok(host::call(&endpoint, "status", None)?.business),
        None => {
            anyhow::ensure!(
                engine::status(home)?.is_none(),
                "管理服务已退出，请使用 wingmate start 恢复连接"
            );
            Ok(None)
        }
    }
}

pub fn share(home: &Path, command: &ShareCommand) -> Result<ShareStatus> {
    let endpoint = host::endpoint(home)?.context("请先启动管理服务")?;
    host::share(&endpoint, command)
}

pub fn dsh_entry(home: &Path, state: &Snapshot) -> Result<std::path::PathBuf> {
    crate::runtime::installed_dsh_entry(home, state.runtime_revision.as_deref())
}

fn ensure_host(home: &Path) -> Result<host::Endpoint> {
    if let Some(endpoint) = host::endpoint(home)? {
        host::call(&endpoint, "status", None)?;
        return Ok(endpoint);
    }
    let binary = crate::components::install(home, false)?;
    let log = File::create(home.join("host.log"))?;
    let mut command = Command::new(&binary);
    crate::runtime::quiet(&mut command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    wingmate_service::runtime::isolate_standard_pipes()?;
    let mut child = command
        .arg("--home")
        .arg(home)
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
                    endpoint.pid == child.id() && endpoint.revision == host::revision(&binary)?,
                    "启动期间管理服务实例已更换"
                );
                if let Err(error) = crate::components::cleanup_services(home, &binary) {
                    eprintln!("旧服务组件清理失败: {error:#}");
                }
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
    let runtime = Runtime {
        id: crate::runtime::ID.into(),
        revision: crate::runtime::REVISION.context("缺少运行组件清单")?.into(),
        directory,
        node: crate::runtime::node()?,
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
    expected: Option<&str>,
    restart: bool,
    report: impl Fn(StartupStage),
    ready: impl FnOnce(&Snapshot),
) -> Result<Snapshot> {
    let _bootstrap = bootstrap_lock(home)?;
    report(StartupStage::Checking);
    let endpoint = ensure_host(home)?;
    if restart {
        report(StartupStage::Stopping);
        host::call(&endpoint, "stop", expected)?;
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
        prepare(home, &report)?;
        report(StartupStage::Starting);
        host::call(&endpoint, "start", None)?
            .business
            .context("业务后台未就绪")?
    };
    ready(&state);
    let _operation = operation_lock(home)?;
    if let Some(current) = engine::status(home)? {
        if current.instance == state.instance {
            crate::runtime::cleanup_after_ready(home, &current);
        }
    }
    Ok(state)
}

fn start_client(
    home: &Path,
    expected: Option<&str>,
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
        if let Some(instance) = expected {
            command.arg(instance);
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
    start_client(home, None, true, |_| {}, |_| {})
}
pub fn restart_with_progress(
    home: &Path,
    report: impl Fn(StartupStage),
    ready: impl FnOnce(&Snapshot),
) -> Result<Snapshot> {
    start_client(home, None, true, report, ready)
}
pub fn restart_instance_with_progress(
    home: &Path,
    instance: &str,
    report: impl Fn(StartupStage),
    ready: impl FnOnce(&Snapshot),
) -> Result<Snapshot> {
    start_client(home, Some(instance), true, report, ready)
}
pub fn stop(home: &Path) -> Result<()> {
    stop_request(home, None)
}
pub fn stop_instance(home: &Path, instance: &str) -> Result<()> {
    stop_request(home, Some(instance))
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

pub fn host_status(home: &Path) -> Result<Option<host::Status>> {
    host::endpoint(home)?
        .map(|endpoint| host::call(&endpoint, "status", None))
        .transpose()
}
fn shutdown_locked(home: &Path) -> Result<()> {
    if let Some(endpoint) = host::endpoint(home)? {
        host::call(&endpoint, "shutdown", None)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while host::try_lock(home)?.is_none() {
            anyhow::ensure!(Instant::now() < deadline, "管理服务尚未退出");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    Ok(())
}
pub fn shutdown(home: &Path) -> Result<()> {
    let _bootstrap = bootstrap_lock(home)?;
    shutdown_locked(home)
}
pub fn restart_host(home: &Path) -> Result<()> {
    let _bootstrap = bootstrap_lock(home)?;
    shutdown_locked(home)?;
    ensure_host(home)?;
    Ok(())
}

pub fn start_host(home: &Path) -> Result<()> {
    let _bootstrap = bootstrap_lock(home)?;
    let endpoint = ensure_host(home)?;
    if host::call(&endpoint, "status", None)?.business.is_none() {
        prepare(home, |_| {})?;
    }
    Ok(())
}
