//! SSH 登录、主机指纹确认和到目标 Wingmate 网关的本地转发。
use super::{remote, remote_relay::BoxStream, Desktop};
use anyhow::{bail, ensure, Context, Result};
use russh::{
    client,
    keys::{self, HashAlg, PrivateKeyWithHashAlg, PublicKey},
    ChannelMsg,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{State, Webview};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Target {
    host: String,
    port: u16,
    username: String,
    executable: Option<String>,
    identity_file: Option<String>,
}

impl Target {
    fn validate(&mut self) -> Result<()> {
        self.host = self
            .host
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase();
        self.username = self.username.trim().to_owned();
        ensure!(
            self.host.parse::<std::net::IpAddr>().is_ok()
                || (!self.host.is_empty()
                    && self.host.len() <= 253
                    && self
                        .host
                        .bytes()
                        .all(|ch| ch.is_ascii_alphanumeric() || b".-_".contains(&ch))),
            "SSH 主机地址无效"
        );
        ensure!(
            self.port != 0
                && !self.username.is_empty()
                && self.username.len() <= 256
                && !self.username.chars().any(char::is_control),
            "请填写有效的 SSH 端口和用户"
        );
        remote::validate_executable(self.executable.as_deref())
    }

    fn authority(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum Reply {
    ConfirmHost { fingerprint: String },
    Connected,
}

struct VerifyHost {
    path: PathBuf,
    endpoint: String,
    approved: Option<String>,
    unknown: Arc<Mutex<Option<String>>>,
}

impl VerifyHost {
    fn check(&self, fingerprint: &str) -> Result<bool> {
        let parent = self.path.parent().context("缺少 SSH 主机记录目录")?;
        fs::create_dir_all(parent)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.path.with_extension("lock"))?;
        fs2::FileExt::lock_exclusive(&lock)?;
        let mut hosts: BTreeMap<String, String> = match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error.into()),
        };
        if let Some(recorded) = hosts.get(&self.endpoint) {
            ensure!(
                recorded == fingerprint,
                "SSH 主机指纹已变更，原指纹为 {recorded}，当前为 {fingerprint}"
            );
            return Ok(true);
        }
        if self.approved.as_deref() != Some(fingerprint) {
            *self.unknown.lock().unwrap() = Some(fingerprint.to_owned());
            return Ok(false);
        }
        hosts.insert(self.endpoint.clone(), fingerprint.to_owned());
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&serde_json::to_vec(&hosts)?)?;
        file.persist(&self.path)?;
        Ok(true)
    }
}

impl client::Handler for VerifyHost {
    type Error = anyhow::Error;

    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool> {
        self.check(&key.fingerprint(HashAlg::Sha256).to_string())
    }
}

type Session = client::Handle<VerifyHost>;

async fn agent_auth(
    session: &mut Session,
    username: &str,
    mut agent: keys::agent::client::AgentClient<
        impl keys::agent::client::AgentStream + Send + Unpin,
    >,
) -> Result<bool> {
    let hash = session.best_supported_rsa_hash().await?.flatten();
    for identity in agent.request_identities().await? {
        if session
            .authenticate_publickey_with(
                username,
                identity.public_key().into_owned(),
                hash,
                &mut agent,
            )
            .await?
            .success()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn authenticate(session: &mut Session, target: &Target, secret: Option<&str>) -> Result<()> {
    let authenticated = if let Some(path) = &target.identity_file {
        let key = keys::load_secret_key(path, secret).context("无法读取 SSH 私钥")?;
        let hash = session.best_supported_rsa_hash().await?.flatten();
        session
            .authenticate_publickey(
                &target.username,
                PrivateKeyWithHashAlg::new(Arc::new(key), hash),
            )
            .await?
            .success()
    } else if let Some(secret) = secret {
        session
            .authenticate_password(&target.username, secret)
            .await?
            .success()
    } else {
        #[cfg(unix)]
        let agent = keys::agent::client::AgentClient::connect_env().await;
        #[cfg(windows)]
        let agent =
            keys::agent::client::AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent")
                .await;
        #[cfg(any(unix, windows))]
        {
            agent_auth(
                session,
                &target.username,
                agent.context("无法连接 SSH agent，可指定私钥或输入密码")?,
            )
            .await?
        }
        #[cfg(not(any(unix, windows)))]
        bail!("请指定 SSH 私钥或输入密码");
    };
    ensure!(authenticated, "SSH 身份验证失败，请检查用户和凭据");
    Ok(())
}

async fn exec(
    session: &Session,
    command: String,
    timeout: Duration,
) -> Result<(u32, Vec<u8>, Vec<u8>)> {
    tokio::time::timeout(timeout, async {
        let mut channel = session.channel_open_session().await?;
        channel.exec(true, command).await?;
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut status = None;
        while let Some(message) = channel.wait().await {
            let (target, data) = match message {
                ChannelMsg::Data { data } => (&mut stdout, data),
                ChannelMsg::ExtendedData { data, .. } => (&mut stderr, data),
                ChannelMsg::ExitStatus { exit_status } => {
                    status = Some(exit_status);
                    continue;
                }
                ChannelMsg::Failure => bail!("SSH 主机拒绝执行命令"),
                _ => continue,
            };
            ensure!(target.len() + data.len() <= 64 * 1024, "SSH 命令输出过长");
            target.extend_from_slice(&data);
        }
        Ok((status.context("SSH 命令未返回退出状态")?, stdout, stderr))
    })
    .await
    .context("SSH 命令执行超时")?
}

#[tauri::command]
pub(super) async fn connect_ssh(
    webview: Webview,
    desktop: State<'_, Arc<Desktop>>,
    mut target: Target,
    secret: Option<String>,
    fingerprint: Option<String>,
) -> Result<Reply, String> {
    let attempt = remote::Attempt::start(&webview, &desktop, None)?;
    target.validate().map_err(|error| error.to_string())?;
    let endpoint = target.authority();
    let unknown = Arc::new(Mutex::new(None));
    let config = client::Config {
        keepalive_interval: Some(Duration::from_secs(15)),
        keepalive_max: 3,
        ..Default::default()
    };
    let session = tokio::time::timeout(
        Duration::from_secs(15),
        client::connect(
            Arc::new(config),
            (target.host.as_str(), target.port),
            VerifyHost {
                path: desktop.home.join("ssh-hosts.json"),
                endpoint: endpoint.clone(),
                approved: fingerprint,
                unknown: unknown.clone(),
            },
        ),
    )
    .await
    .map_err(|_| "SSH 连接超时".to_owned())?;
    attempt.verify()?;
    let mut session = match session {
        Ok(session) => session,
        Err(error) => {
            return match unknown.lock().unwrap().take() {
                Some(fingerprint) => Ok(Reply::ConfirmHost { fingerprint }),
                None => Err(format!("无法连接 SSH 主机：{error}")),
            }
        }
    };
    tokio::time::timeout(
        Duration::from_secs(20),
        authenticate(&mut session, &target, secret.as_deref()),
    )
    .await
    .map_err(|_| "SSH 登录超时".to_owned())?
    .map_err(|error| format!("{error:#}"))?;
    drop(secret);
    attempt.verify()?;
    let pairing = remote::bootstrap(target.executable.as_deref(), |command, timeout| {
        exec(&session, command, timeout)
    })
    .await
    .map_err(|error| format!("{error:#}"))?;
    let port = pairing.port().expect("validated gateway port");
    let session = Arc::new(session);
    let dial = move || {
        let session = session.clone();
        async move {
            let channel = session
                .channel_open_direct_tcpip("127.0.0.1", u32::from(port), "127.0.0.1", 0)
                .await
                .context("SSH 主机拒绝转发到 Wingmate 网关")?;
            Ok(Box::new(channel.into_stream()) as BoxStream)
        }
    };
    remote::open_tunnel(
        attempt,
        pairing,
        format!("{}@{endpoint}", target.username),
        "ssh",
        dial,
    )
    .await?;
    Ok(Reply::Connected)
}
