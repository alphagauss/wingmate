//! 枚举 WSL 发行版并在所选实例中接入 Wingmate 的回环网关。
use super::{connection_panel, remote, remote_install, remote_relay::BoxStream, Desktop};
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use std::{
    pin::Pin,
    process::Stdio,
    sync::Arc,
    task::{Context as TaskContext, Poll},
    time::Duration,
};
use tauri::{State, Webview};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt, Join, ReadBuf},
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

fn open_pipe(distro: &str, script: String) -> Result<BoxStream> {
    let mut child = distro_command(distro, script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("无法在所选 WSL 发行版运行 Wingmate")?;
    let pipe = tokio::io::join(child.stdout.take().unwrap(), child.stdin.take().unwrap());
    Ok(Box::new(Stream {
        pipe,
        _process: child,
    }))
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

async fn upload(distro: &str, path: &std::path::Path, offer: &remote_install::Offer) -> Result<()> {
    let script = format!(
        r#"
        umask 077
        base="$HOME/.wingmate/remote-install"
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
    let mut file = tokio::fs::File::open(path).await?;
    let (write, output) = tokio::time::timeout(Duration::from_secs(900), async move {
        tokio::join!(
            async move {
                tokio::io::copy(&mut file, &mut input).await?;
                input.shutdown().await
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
            let (platform, _) = platform(&distro).await?;
            let path = remote_install::download(&desktop.home, &platform, offer).await?;
            attempt.verify().map_err(anyhow::Error::msg)?;
            upload(&distro, &path, offer).await?;
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
        Ok(gateway) if gateway.instance.is_some() => gateway,
        Ok(_) => return Err("WSL 中的 Wingmate 版本过旧，请在此发行版更新后重试".into()),
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
    let script = format!(
        "{}; exec \"$wm\" gateway-stdio",
        remote::executable_script(executable.as_deref())
    );
    let address = distro.clone();
    remote::open_tunnel(attempt, gateway, address, "wsl", move || {
        std::future::ready(open_pipe(&distro, script.clone()))
    })
    .await?;
    Ok(Reply::Connected)
}
