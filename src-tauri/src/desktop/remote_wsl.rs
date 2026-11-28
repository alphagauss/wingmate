//! 枚举 WSL 发行版并在所选实例中接入 Wingmate 的回环网关。
use super::{connection_panel, remote, remote_install, remote_relay::BoxStream, Desktop};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddrV4,
    pin::Pin,
    process::Stdio,
    sync::Arc,
    task::{Context as TaskContext, Poll},
    time::Duration,
};
use tauri::{State, Url, Webview};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, Join, ReadBuf},
    process::{Child, ChildStdin, ChildStdout, Command},
};

fn command() -> Command {
    let mut command = Command::new("wsl.exe");
    command.kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    command
}

fn distro_command(distro: &str, script: String) -> Command {
    let mut command = command();
    command.args(["--distribution", distro, "--exec", "sh", "-c", &script]);
    command
}

fn decode(output: &[u8]) -> String {
    let utf16 = output.starts_with(&[0xff, 0xfe])
        || (output.len() >= 2
            && output.chunks_exact(2).filter(|pair| pair[1] == 0).count() > output.len() / 4);
    if utf16 {
        let bytes = output.strip_prefix(&[0xff, 0xfe]).unwrap_or(output);
        return String::from_utf16_lossy(
            &bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>(),
        );
    }
    String::from_utf8_lossy(output).into_owned()
}

#[tauri::command]
pub(super) async fn list_wsl(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
) -> Result<Vec<String>, String> {
    connection_panel::local_panel(&webview, &desktop)?;
    if !cfg!(windows) {
        return Ok(Vec::new());
    }
    let output = tokio::time::timeout(
        Duration::from_secs(8),
        command().args(["--list", "--quiet"]).output(),
    )
    .await
    .map_err(|_| "列出 WSL 发行版超时".to_owned())?
    .map_err(|error| format!("无法运行 WSL：{error}"))?;
    if !output.status.success() {
        let details = decode(&output.stderr);
        return Err(format!("无法读取 WSL 发行版。{}", details.trim()));
    }
    Ok(decode(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect())
}

struct Stream {
    pipe: Join<ChildStdout, ChildStdin>,
    _process: Child,
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.pipe).poll_read(context, buffer)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.pipe).poll_write(context, buffer)
    }
    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.pipe).poll_flush(context)
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.pipe).poll_shutdown(context)
    }
}

struct GatewayProcess {
    node: String,
    pid: String,
    started: String,
    port: String,
}

impl GatewayProcess {
    async fn dial(&self, distro: &str) -> Result<BoxStream> {
        let mut child = command()
            .args([
                "--distribution",
                distro,
                "--exec",
                "env",
                "-u",
                "NODE_OPTIONS",
                "-u",
                "NODE_PATH",
                &self.node,
                "--input-type=commonjs",
                "--eval",
                include_str!("remote_wsl_relay.cjs"),
                "--",
                &self.port,
                &self.pid,
                &self.started,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("无法在所选 WSL 发行版运行转发程序")?;
        let mut stdout = child.stdout.take().unwrap();
        let state = stdout.read_u8().await.context("WSL 转发未返回就绪状态")?;
        ensure!(state == 0, "无法确认实际网关属于所选 WSL 发行版的 Wingmate");
        let pipe = tokio::io::join(stdout, child.stdin.take().unwrap());
        Ok(Box::new(Stream {
            pipe,
            _process: child,
        }))
    }
}

async fn exec(distro: &str, script: String, timeout: Duration) -> Result<(u32, Vec<u8>, Vec<u8>)> {
    let output = tokio::time::timeout(timeout, distro_command(distro, script).output())
        .await
        .context("WSL 命令执行超时")??;
    Ok((
        output
            .status
            .code()
            .and_then(|code| u32::try_from(code).ok())
            .unwrap_or(1),
        output.stdout,
        decode(&output.stderr).into_bytes(),
    ))
}

async fn platform(distro: &str) -> Result<(String, String)> {
    let (code, stdout, _) =
        exec(distro, "uname -sm && id -un".into(), Duration::from_secs(8)).await?;
    ensure!(code == 0, "无法读取 WSL 发行版信息");
    let output = decode(&stdout);
    let (platform, user) = output.trim().split_once('\n').context("未获得 WSL 用户")?;
    Ok((platform.trim().into(), user.trim().into()))
}

async fn gateway_process(
    distro: &str,
    executable: Option<&str>,
    pairing: &Url,
) -> Result<GatewayProcess> {
    const STATUS: &str = "__wingmate_service_status__";
    const DOCTOR: &str = "__wingmate_doctor__";
    let (code, stdout, _) = exec(distro, format!(
        "{}; printf '\\n{STATUS}\\n'; \"$wm\" service-status || exit; printf '\\n{DOCTOR}\\n'; exec \"$wm\" doctor",
        remote::executable_script(executable)
    ), Duration::from_secs(12)).await?;
    ensure!(code == 0, "无法查询 WSL 中的 Wingmate 管理服务和 Node.js");
    let text = std::str::from_utf8(&stdout)?;
    let (before_doctor, doctor) = text
        .rsplit_once(DOCTOR)
        .context("Wingmate 未返回 Node.js 信息")?;
    let (_, status) = before_doctor
        .rsplit_once(STATUS)
        .context("Wingmate 未返回管理服务状态")?;
    #[derive(Deserialize)]
    struct Status {
        pid: u32,
        web_address: SocketAddrV4,
    }
    let status: Status = serde_json::from_str(status).context("无法识别 Wingmate 管理服务状态")?;
    ensure!(
        status.pid != 0
            && status.web_address.ip().is_loopback()
            && Some(status.web_address.port()) == pairing.port(),
        "WSL 中的 Wingmate 网关已变化，请重新连接"
    );
    let doctor: serde_json::Value =
        serde_json::from_str(doctor).context("无法识别 Wingmate Node.js 诊断")?;
    let node = doctor
        .pointer("/node/info/path")
        .and_then(serde_json::Value::as_str)
        .context("Wingmate 未找到可用于 WSL 转发的 Node.js")?;
    remote::validate_executable(Some(node))?;
    let pid = status.pid.to_string();
    let (code, stat, _) = exec(
        distro,
        format!("exec cat /proc/{pid}/stat"),
        Duration::from_secs(5),
    )
    .await?;
    ensure!(code == 0, "WSL 中的 Wingmate 管理服务已退出");
    let started = std::str::from_utf8(&stat)?
        .rsplit_once(") ")
        .and_then(|(_, fields)| fields.split_whitespace().nth(19))
        .filter(|value| value.parse::<u64>().is_ok())
        .context("无法确认 WSL 中 Wingmate 管理服务的启动时间")?;
    Ok(GatewayProcess {
        node: node.into(),
        pid,
        started: started.into(),
        port: status.web_address.port().to_string(),
    })
}

async fn upload(
    distro: &str,
    download: remote_install::Download,
    offer: &remote_install::Offer,
    progress: &remote_install::Progress<'_>,
) -> Result<()> {
    let script = format!(
        r#"
        umask 077
        base="$HOME/.wingmate/downloads"
        mkdir -p -- "$base" || exit
        staged=$(mktemp "$base/.part.XXXXXXXXXX") || exit
        trap 'rm -f -- "$staged"' EXIT
        trap 'exit 1' HUP INT TERM
        cat > "$staged" || exit
        {}
    "#,
        remote_install::publish_script(&offer.sha256)
    );
    let mut process = distro_command(distro, script)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("无法向 WSL 传输 Wingmate")?;
    let mut input = process.stdin.take().context("WSL 未提供写入管道")?;
    let (write, output) = tokio::time::timeout(Duration::from_secs(900), async move {
        tokio::join!(
            async move {
                remote_install::transfer(download, &mut input, offer.size, progress).await?;
                input.shutdown().await?;
                drop(input);
                progress.stage(remote_install::Stage::Verify);
                Ok::<(), anyhow::Error>(())
            },
            process.wait_with_output()
        )
    })
    .await
    .context("WSL 安装超时")?;
    let output = output?;
    ensure!(
        output.status.success(),
        "安装 Wingmate 失败。{}",
        decode(&output.stderr).trim()
    );
    write?;
    Ok(())
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum Reply {
    Install {
        offer: remote_install::Offer,
        user: String,
        path: &'static str,
    },
    Connected,
}

#[tauri::command]
pub(super) async fn connect_wsl(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    distro: String,
    mut executable: Option<String>,
    approved: Option<remote_install::Offer>,
    progress_id: Option<u32>,
) -> Result<Reply, String> {
    let attempt = remote::Attempt::start(&webview, &desktop, None)?;
    async {
        ensure!(cfg!(windows), "WSL 连接需要 Windows");
        ensure!(
            !distro.is_empty()
                && distro.len() <= 256
                && !distro.starts_with('-')
                && !distro.chars().any(char::is_control),
            "请选择有效的 WSL 发行版"
        );
        remote::validate_executable(executable.as_deref())?;
        if let Some(offer) = &approved {
            let progress =
                remote_install::Progress::new(&webview, desktop.page.connections(), progress_id);
            let (platform, _) = platform(&distro).await?;
            let download =
                remote_install::download(&desktop.home, &platform, offer, &progress).await?;
            attempt.verify().map_err(anyhow::Error::msg)?;
            upload(&distro, download, offer, &progress).await?;
            progress.stage(remote_install::Stage::Connect);
            executable = None;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await
    .map_err(|error| format!("{error:#}"))?;
    let gateway = match remote::bootstrap(executable.as_deref(), |script, timeout| {
        exec(&distro, script, timeout)
    })
    .await
    {
        Ok(gateway) => gateway,
        Err(error) if approved.is_none() && error.is::<remote::MissingExecutable>() => {
            let (platform, user) = platform(&distro)
                .await
                .map_err(|error| format!("{error:#}"))?;
            let offer = remote_install::latest(&platform)
                .await
                .map_err(|error| format!("找不到 Wingmate。{error:#}"))?;
            attempt.verify()?;
            return Ok(Reply::Install {
                offer,
                user,
                path: remote_install::DESTINATION,
            });
        }
        Err(error) => return Err(format!("{error:#}")),
    };
    let process = Arc::new(
        gateway_process(&distro, executable.as_deref(), &gateway)
            .await
            .map_err(|error| format!("{error:#}"))?,
    );
    let address = distro.clone();
    remote::open_tunnel(attempt, gateway, address, "wsl", move || {
        let distro = distro.clone();
        let process = process.clone();
        async move { process.dial(&distro).await }
    })
    .await?;
    Ok(Reply::Connected)
}
