use crate::{host, runtime};
use anyhow::{ensure, Context, Result};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    io::{BufRead, BufReader, Write},
    net::SocketAddrV4,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub(crate) const NAMED_PORT: u16 = 3089;
const BINARY: &str = if cfg!(windows) {
    "cloudflared.exe"
} else {
    "cloudflared"
};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Quick,
    Named,
}

#[derive(Serialize, Deserialize)]
struct Named {
    hostname: String,
    token: String,
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Config {
    pub(crate) enabled: Option<Mode>,
    named: Option<Named>,
}

fn hostname(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 253
        || !value.contains('.')
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        || value.split('.').any(|part| {
            part.is_empty() || part.len() > 63 || part.starts_with('-') || part.ends_with('-')
        })
    {
        return None;
    }
    let url = Url::parse(&format!("https://{value}")).ok()?;
    url.domain().map(str::to_owned)
}

impl Config {
    fn path(home: &Path) -> PathBuf {
        home.join("tunnel/config.json")
    }

    pub(crate) fn load(home: &Path) -> Result<Self> {
        match fs::read(Self::path(home)) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("公网隧道配置无法读取"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).context("公网隧道配置无法读取"),
        }
    }

    pub(crate) fn save(&self, home: &Path) -> Result<()> {
        fs::create_dir_all(home.join("tunnel"))?;
        host::publish(&Self::path(home), self)
    }

    pub(crate) fn configure(&mut self, host: &str, token: Option<String>) -> Result<()> {
        let hostname = hostname(host).context("请输入公开域名，不含协议、端口或路径")?;
        let token = token
            .map(|token| token.trim().to_owned())
            .or_else(|| self.named.as_ref().map(|named| named.token.clone()))
            .filter(|token| !token.is_empty() && !token.chars().any(char::is_whitespace))
            .context("请输入 Cloudflare Tunnel Token")?;
        self.named = Some(Named { hostname, token });
        Ok(())
    }

    pub(crate) fn check(&self, mode: Mode) -> Result<()> {
        if mode == Mode::Named {
            let named = self.named.as_ref().context("请先配置具名隧道")?;
            ensure!(
                hostname(&named.hostname).as_deref() == Some(named.hostname.as_str()),
                "公开域名无效"
            );
            ensure!(!named.token.is_empty(), "请先配置 Tunnel Token");
        }
        Ok(())
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Status {
    pub mode: Option<Mode>,
    pub hostname: Option<String>,
    pub url: Option<String>,
    pub error: Option<String>,
    pub progress: Option<Progress>,
    pub named_service: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Progress {
    stage: String,
    received: Option<u64>,
    total: Option<u64>,
}

impl Progress {
    fn stage(stage: &str) -> Self {
        Self {
            stage: stage.into(),
            received: None,
            total: None,
        }
    }
}

pub(crate) type State = Arc<Mutex<Status>>;

pub(crate) struct Listener {
    pub(crate) address: SocketAddrV4,
    pub(crate) closed: CancellationToken,
    pub(crate) task: tokio::task::JoinHandle<()>,
}

#[derive(Default)]
pub(crate) struct Tunnel {
    active: Option<Active>,
    error: Option<String>,
}

struct Active {
    state: State,
    closed: CancellationToken,
    worker: Option<JoinHandle<()>>,
}

impl Drop for Active {
    fn drop(&mut self) {
        self.closed.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Tunnel {
    pub(crate) fn status(&self, home: &Path) -> Status {
        let mut status = self
            .active
            .as_ref()
            .map(|run| run.state.lock().unwrap().clone())
            .unwrap_or_default();
        status.error = status.error.or_else(|| self.error.clone());
        status.named_service = Some(format!("http://127.0.0.1:{NAMED_PORT}"));
        match Config::load(home) {
            Ok(config) => {
                status.mode = config.enabled;
                status.hostname = config.named.map(|named| named.hostname);
            }
            Err(_) => status.error = Some("公网隧道配置无法读取".into()),
        }
        status
    }

    pub(crate) fn stop(&mut self) {
        self.active = None;
        self.error = None;
    }

    pub(crate) fn failed(&mut self, error: anyhow::Error) {
        self.error = Some(format!("{error:#}"));
    }

    pub(crate) fn start(
        &mut self,
        home: PathBuf,
        config: Config,
        mode: Mode,
        state: State,
        listener: Listener,
    ) {
        let worker_state = state.clone();
        let closed = listener.closed.clone();
        let runtime = tokio::runtime::Handle::current();
        let worker = thread::spawn(move || {
            let result = run(
                &home,
                config,
                mode,
                &worker_state,
                &listener.closed,
                listener.address,
            );
            listener.closed.cancel();
            {
                let mut status = worker_state.lock().unwrap();
                status.url = None;
                status.progress = None;
                status.error = result.err().map(|error| format!("{error:#}"));
            }
            let _ = runtime.block_on(listener.task);
        });
        self.active = Some(Active {
            state,
            closed,
            worker: Some(worker),
        });
    }
}

struct Process {
    child: Child,
    lines: mpsc::Receiver<String>,
    #[cfg(windows)]
    _job: Option<std::os::windows::io::OwnedHandle>,
}

impl Process {
    fn spawn(command: &mut Command) -> Result<Self> {
        runtime::quiet(command);
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            let parent = std::process::id();
            unsafe {
                command.pre_exec(move || {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::getppid() as u32 != parent {
                        libc::_exit(1);
                    }
                    Ok(())
                });
            }
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        #[cfg(windows)]
        let job = match crate::process::job_for_child(&child) {
            Ok(job) => Some(job),
            Err(_) if child.try_wait()?.is_some() => None,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let (send, lines) = mpsc::channel();
        read_lines(child.stdout.take().unwrap(), send.clone());
        read_lines(child.stderr.take().unwrap(), send);
        Ok(Self {
            child,
            lines,
            #[cfg(windows)]
            _job: job,
        })
    }

    fn monitor(
        &mut self,
        closed: &CancellationToken,
        mut on_line: impl FnMut(&str),
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<ExitStatus> {
        loop {
            ensure!(!closed.is_cancelled(), "公网隧道已停止");
            check()?;
            match self.lines.recv_timeout(Duration::from_millis(100)) {
                Ok(line) => on_line(&line),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    thread::sleep(Duration::from_millis(100))
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Some(status) = self.child.try_wait()? {
                // All output from the exited child precedes EOF.
                while let Ok(line) = self.lines.recv_timeout(Duration::from_millis(100)) {
                    check()?;
                    ensure!(!closed.is_cancelled(), "公网隧道已停止");
                    on_line(&line);
                }
                return Ok(status);
            }
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_lines(pipe: impl std::io::Read + Send + 'static, send: mpsc::Sender<String>) {
    thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            if send.send(line).is_err() {
                break;
            }
        }
    });
}

fn available(binary: &std::ffi::OsStr, closed: &CancellationToken) -> bool {
    let mut version = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    Process::spawn(Command::new(binary).arg("--version").stdin(Stdio::null()))
        .and_then(|mut process| {
            process.monitor(
                closed,
                |line| version |= line.starts_with("cloudflared version "),
                || {
                    ensure!(Instant::now() < deadline, "cloudflared 版本检查超时");
                    Ok(())
                },
            )
        })
        .is_ok_and(|status| status.success() && version)
}

fn resolve(home: &Path, state: &State, closed: &CancellationToken) -> Result<OsString> {
    if let Some(path) = std::env::var_os("CLOUDFLARED_PATH").filter(|path| !path.is_empty()) {
        ensure!(
            available(&path, closed),
            "CLOUDFLARED_PATH 指向的程序不可用"
        );
        return Ok(path);
    }
    let binary = OsString::from(BINARY);
    if available(&binary, closed) {
        return Ok(binary);
    }
    let directory = home.join("tunnel/bin");
    let installed = directory.join(BINARY);
    if installed.is_file() && available(installed.as_os_str(), closed) {
        return Ok(installed.into_os_string());
    }
    ensure!(!closed.is_cancelled(), "公网隧道已停止");
    let node = runtime::Runtime::load(home)
        .ok()
        .map(|runtime| runtime.node)
        .filter(|node| node.is_absolute() && node.is_file())
        .unwrap_or_else(|| "node".into());
    fs::create_dir_all(&directory)?;
    fs::write(
        directory.join("LICENSE.desktop"),
        include_bytes!("tunnel/LICENSE.desktop"),
    )?;
    let staging = tempfile::tempdir_in(&directory)?;
    let mut download = Process::spawn(
        Command::new(node)
            .args(["--input-type=module", "-"])
            .arg(staging.path())
            .env_remove("NODE_OPTIONS")
            .env("NODE_USE_ENV_PROXY", "1")
            .stdin(Stdio::piped()),
    )
    .context("无法启动 Node 下载 cloudflared；也可安装 cloudflared 或设置 CLOUDFLARED_PATH")?;
    download
        .child
        .stdin
        .take()
        .unwrap()
        .write_all(include_bytes!("tunnel/download.mjs"))?;
    let mut error = None;
    let result = download.monitor(
        closed,
        |line| {
            if let Ok(progress) = serde_json::from_str::<Progress>(line) {
                if progress.stage != "installed" {
                    state.lock().unwrap().progress = Some(progress);
                }
            } else {
                error = Some(line.chars().take(1500).collect::<String>());
            }
        },
        || Ok(()),
    )?;
    ensure!(
        result.success() && staging.path().join(BINARY).is_file(),
        "{}",
        error.as_deref().unwrap_or("cloudflared 下载失败")
    );
    fs::rename(staging.path().join(BINARY), &installed)?;
    Ok(installed.into_os_string())
}

fn quick_url(line: &str) -> Option<String> {
    line.split_whitespace()
        .filter_map(|part| Url::parse(part).ok())
        .find_map(|url| {
            (url.scheme() == "https"
                && url.domain().is_some_and(|domain| {
                    domain
                        .strip_suffix(".trycloudflare.com")
                        .is_some_and(|name| !name.is_empty() && !name.contains('.'))
                })
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none())
            .then(|| url.origin().ascii_serialization())
        })
}

fn run(
    home: &Path,
    config: Config,
    mode: Mode,
    state: &State,
    closed: &CancellationToken,
    address: SocketAddrV4,
) -> Result<()> {
    let binary = resolve(home, state, closed)?;
    ensure!(!closed.is_cancelled(), "公网隧道已停止");
    state.lock().unwrap().progress = Some(Progress::stage("connect"));
    let mut command = Command::new(binary);
    command
        .args(["tunnel", "--no-autoupdate"])
        .stdin(Stdio::null())
        .env_remove("TUNNEL_TOKEN")
        .env_remove("TUNNEL_TOKEN_FILE");
    let mut url = match mode {
        Mode::Quick => {
            command.args(["--url", &format!("http://{address}")]);
            None
        }
        Mode::Named => {
            let named = config.named.context("请先配置具名隧道")?;
            command.arg("run").env("TUNNEL_TOKEN", named.token);
            Some(format!("https://{}", named.hostname))
        }
    };
    let mut registered = false;
    let mut published = false;
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut process = Process::spawn(&mut command).context("无法启动 cloudflared")?;
    let result = process.monitor(
        closed,
        |line| {
            if published {
                return;
            }
            if mode == Mode::Quick && url.is_none() {
                url = quick_url(line);
            }
            registered |= line.contains("Registered tunnel connection");
            if registered {
                if let Some(url) = url.take() {
                    let mut status = state.lock().unwrap();
                    status.url = Some(url);
                    status.progress = None;
                    published = true;
                }
            }
        },
        || {
            ensure!(
                Instant::now() < deadline || state.lock().unwrap().url.is_some(),
                "cloudflared 未能连接 Cloudflare，请检查网络和隧道配置"
            );
            Ok(())
        },
    )?;
    anyhow::bail!("cloudflared 已退出（{result}），请检查网络和隧道配置")
}

pub(crate) fn starting() -> State {
    Arc::new(Mutex::new(Status {
        progress: Some(Progress::stage("check")),
        ..Status::default()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_accepts_expected_tunnel_addresses() {
        assert_eq!(
            hostname(" public.Example.org "),
            Some("public.example.org".into())
        );
        assert!(hostname("https://public.example.org").is_none());
        assert!(hostname("127.0.0.1").is_none());
        assert!(hostname("public.example.org/path").is_none());
        assert!(hostname("public..example.org").is_none());
        assert_eq!(
            quick_url("| https://abc.trycloudflare.com |"),
            Some("https://abc.trycloudflare.com".into())
        );
        assert!(quick_url("https://abc.trycloudflare.com.invalid").is_none());
        assert!(quick_url("https://abc.trycloudflare.com/?secret=1").is_none());
    }
}
