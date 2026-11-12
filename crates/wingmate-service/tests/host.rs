//! 用独立宿主进程和无模型的 Node 夹具检查生命周期与身份边界。
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use wingmate_service::{
    engine, host,
    runtime::{Runtime, DSH_ENTRY, WEB_PATCH},
};

struct Host {
    child: Child,
    home: tempfile::TempDir,
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = engine::stop(self.home.path());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn spawn(home: &Path) -> Result<Child> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wingmate-service"));
    wingmate_service::runtime::quiet(&mut command);
    Ok(command
        .arg("--home")
        .arg(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?)
}

fn tunnel_host(home: &Path, binary: &Path, origin: &Path) -> Result<Host> {
    let directory = tempfile::tempdir_in(home)?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_wingmate-service"));
    wingmate_service::runtime::quiet(&mut command);
    let child = command
        .arg("--home")
        .arg(directory.path())
        .env("CLOUDFLARED_PATH", binary)
        .env("TEST_TUNNEL_ORIGIN", origin)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(Host {
        child,
        home: directory,
    })
}

fn tunnel_ready(endpoint: &host::Endpoint) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let status = host::share(endpoint, &host::ShareCommand::Status)?
            .tunnel
            .unwrap();
        if let Some(url) = status.url {
            return Ok(url);
        }
        anyhow::ensure!(status.error.is_none(), "{:?}", status.error);
        anyhow::ensure!(Instant::now() < deadline, "隧道未就绪");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn tunnel_modes_use_the_paired_public_gateway() -> Result<()> {
    use host::{ShareCommand as Share, TunnelMode};

    let temp = tempfile::tempdir()?;
    let binary = temp.path().join(if cfg!(windows) {
        "cloudflared.exe"
    } else {
        "cloudflared"
    });
    let compiled = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cloudflared.rs"))
        .arg("-o")
        .arg(&binary)
        .output()?;
    anyhow::ensure!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let origin_path = temp.path().join("origin");
    let mut service = tunnel_host(temp.path(), &binary, &origin_path)?;
    let endpoint = wait(service.home.path())?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()?;
    host::share(
        &endpoint,
        &Share::TunnelStart {
            mode: TunnelMode::Quick,
        },
    )?;
    let public = tunnel_ready(&endpoint)?;
    let loopback = fs::read_to_string(&origin_path)?;
    let authority = public.trim_start_matches("https://");
    assert_eq!(client.get(&loopback).send()?.status(), 421);
    assert_eq!(
        client
            .get(format!("{loopback}/_wingmate/status"))
            .header("Host", authority)
            .send()?
            .status(),
        401
    );
    let pairing = host::share(&endpoint, &Share::TunnelPair)?
        .pairing_url
        .unwrap();
    let code = pairing.strip_prefix(&format!("{public}/?auth=")).unwrap();
    let exchange = |origin: &str| {
        client
            .post(format!("{loopback}/?auth"))
            .header("Host", authority)
            .header("Origin", origin)
            .body(code.to_owned())
            .send()
    };
    assert_eq!(exchange("https://wrong.example")?.status(), 403);
    let paired = exchange(&public)?;
    assert_eq!(paired.status(), 204);
    let cookie = paired.headers()["set-cookie"].to_str()?;
    assert!(cookie.ends_with("; Secure"));
    let cookie = cookie.split(';').next().unwrap();
    assert_eq!(exchange(&public)?.status(), 401);
    let recovery = client
        .get(&loopback)
        .header("Host", authority)
        .header("Cookie", cookie)
        .header("Accept", "text/html")
        .send()?;
    assert_eq!(recovery.status(), 200);
    assert!(recovery.text()?.contains("/_wingmate/home.js"));

    let named = "wingmate.example.test";
    let configured = host::share(
        &endpoint,
        &Share::TunnelConfigure {
            hostname: named.into(),
            token: Some("test-tunnel-token".into()),
        },
    )?;
    assert!(!serde_json::to_string(&configured)?.contains("test-tunnel-token"));
    assert_eq!(
        configured.tunnel.unwrap().url.as_deref(),
        Some(public.as_str())
    );
    host::share(
        &endpoint,
        &Share::TunnelStart {
            mode: TunnelMode::Named,
        },
    )?;
    assert_eq!(tunnel_ready(&endpoint)?, format!("https://{named}"));
    assert!(client
        .get(&loopback)
        .header("Host", authority)
        .send()
        .is_err());
    host::call(&endpoint, "shutdown", None)?;
    assert!(service.child.wait()?.success());
    assert!(std::net::TcpListener::bind("127.0.0.1:3089").is_ok());
    Ok(())
}
fn wait(home: &Path) -> Result<host::Endpoint> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(Some(endpoint)) = host::endpoint(home) {
            if host::call(&endpoint, "status", None).is_ok() {
                return Ok(endpoint);
            }
        }
        anyhow::ensure!(Instant::now() < deadline, "宿主未就绪");
        std::thread::sleep(Duration::from_millis(30));
    }
}
fn runtime(home: &Path) -> Result<()> {
    let directory = home.join("runtimes/dsh-test-aaaaaa");
    let entry = directory.join(DSH_ENTRY);
    fs::create_dir_all(entry.parent().unwrap())?;
    fs::write(
        entry,
        r#"
const { createServer } = require('node:http');
const { createHash } = require('node:crypto');
const server = createServer(async (request, response) => {
  if (request.url === '/?token=test') {
    response.writeHead(303, { 'set-cookie': 'dsh_test=owner; Path=/', location: '/' });
    response.end();
    return;
  }
  if (request.headers.cookie !== 'dsh_test=owner') {
    response.writeHead(401).end();
    return;
  }
  if (request.url === '/file?download') {
    response.writeHead(302, { location: '?version=1' }).end();
    return;
  }
  response.setHeader('set-cookie', 'dsh_test=hidden; Path=/');
  response.setHeader('content-type', 'application/octet-stream');
  for await (const chunk of request) response.write(chunk);
  response.end('DSH');
});
server.on('upgrade', (request, socket) => {
  if (request.headers.cookie !== 'dsh_test=owner') return socket.destroy();
  const accept = createHash('sha1').update(request.headers['sec-websocket-key'] +
    '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
  socket.write('HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n' +
    'Sec-WebSocket-Accept: ' + accept + '\r\n\r\n');
});
server.listen(0, '127.0.0.1', () => console.log('dsh web: http://127.0.0.1:' +
  server.address().port + '/?token=test'));
process.on('SIGTERM', () => process.exit(0));
"#,
    )?;
    let paths = directory.join("node_modules/@deepseek-ai/dsh-home-paths");
    fs::create_dir_all(&paths)?;
    fs::write(
        paths.join("index.js"),
        "exports.resolveDshHome = exports.resolveAgentsHome = require('node:os').homedir;",
    )?;
    let patch = directory.join(WEB_PATCH);
    fs::create_dir_all(patch.parent().unwrap())?;
    fs::write(patch, "")?;
    let manifest =
        serde_json::json!({"format":1,"components":{"base":{"sha256":"a".repeat(64),"paths":[]}}})
            .to_string();
    fs::write(directory.join("installed.json"), &manifest)?;
    fs::write(directory.join(".wingmate-base"), "a".repeat(64))?;
    let output = Command::new("node")
        .args(["-p", "process.execPath"])
        .output()?;
    let runtime = Runtime {
        id: "dsh-test".into(),
        revision: format!("{:x}", Sha256::digest(manifest.as_bytes())),
        directory,
        node: String::from_utf8(output.stdout)?.trim().into(),
        dsh_home: None,
        agents_home: None,
    };
    runtime.validate(home)?;
    host::publish(&home.join("runtime.json"), &runtime)
}

#[test]
fn gateway_pairs_proxies_and_revokes_without_exposing_upstream_credentials() -> Result<()> {
    use host::ShareCommand::{Disable, Enable, Pair, Revoke, Status};
    let home = tempfile::tempdir()?;
    runtime(home.path())?;
    let child = spawn(home.path())?;
    let guard = Host { child, home };
    let endpoint = wait(guard.home.path())?;
    let share = |command| host::share(&endpoint, &command);
    assert!(share(Status)?.address.is_none());
    let enabled = share(Enable {
        address: Some(Ipv4Addr::LOCALHOST),
    })?;
    let address = enabled.address.unwrap();
    assert_eq!(enabled.lan_addresses, vec![address]);
    let origin = format!("http://{address}");
    let url = |path: &str| format!("{origin}{path}");
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()?;
    assert_eq!(client.get(url("/")).send()?.status(), 401);
    assert_eq!(
        client
            .get(url("/"))
            .header("Host", "unrelated.invalid")
            .send()?
            .status(),
        421
    );
    let pairing = share(Pair)?;
    let link = pairing.pairing_url.unwrap();
    assert_eq!(pairing.pairing_urls, vec![link.clone()]);
    let prefix = url("/?auth=");
    let code = link.strip_prefix(&prefix).context("配对链接格式错误")?;
    let landing = client.get(&link).send()?;
    assert_eq!(landing.status(), 200);
    assert_eq!(landing.headers()["cache-control"], "no-store");
    assert_eq!(landing.headers()["referrer-policy"], "no-referrer");
    assert!(!landing.headers().contains_key("set-cookie"));
    assert!(landing.text()?.contains("连接 Wingmate"));
    assert!(share(Status)?.devices.is_empty());
    let exchange = |origin: &str| {
        client
            .post(url("/?auth"))
            .header("Origin", origin)
            .body(code.to_owned())
            .send()
    };
    assert_eq!(exchange("http://unrelated.invalid")?.status(), 403);
    let paired = exchange(&origin)?;
    assert_eq!(paired.status(), 204);
    let cookie = paired.headers()["set-cookie"]
        .to_str()?
        .split(';')
        .next()
        .unwrap();
    assert_eq!(exchange(&origin)?.status(), 401);
    assert_eq!(
        client
            .get(url("/"))
            .header("Cookie", cookie)
            .send()?
            .status(),
        503
    );
    let manage = |action: &str, instance: Option<&str>| {
        client
            .post(url("/_wingmate/manage"))
            .header("Cookie", cookie)
            .header("Origin", &origin)
            .json(&serde_json::json!({ "action": action, "expected_instance": instance }))
            .send()
    };
    let recovery = client
        .get(url("/"))
        .header("Cookie", cookie)
        .header("Accept", "text/html")
        .send()?;
    assert_eq!(recovery.status(), 200);
    assert!(recovery.text()?.contains("/_wingmate/home.js"));
    assert_eq!(manage("shutdown", None)?.status(), 400);
    let started = manage("start", None)?;
    assert_eq!(started.status(), 200);
    let started: serde_json::Value = started.json()?;
    assert_eq!(started["phase"], "running");
    assert_eq!(manage("stop", None)?.status(), 400);
    assert_eq!(manage("stop", Some("stale"))?.status(), 409);
    let stopped: serde_json::Value = manage("stop", started["instance"].as_str())?.json()?;
    assert_eq!(stopped["phase"], "stopped");
    let recovered: serde_json::Value = manage("start", None)?.json()?;
    assert_eq!(recovered["phase"], "running");
    assert_ne!(recovered["instance"], started["instance"]);
    let restarted: serde_json::Value = manage("restart", recovered["instance"].as_str())?.json()?;
    assert_eq!(restarted["phase"], "running");
    assert_ne!(restarted["instance"], recovered["instance"]);
    assert!(restarted["dsh_origin"].is_null());
    let root = client
        .post(url("/"))
        .header("Cookie", cookie)
        .header("Origin", &origin)
        .send()?;
    assert_eq!(root.text()?, "DSH");
    let upload = client
        .post(url("/file"))
        .header("Cookie", cookie)
        .header("Origin", &origin)
        .body(vec![0, 1, 255])
        .send()?;
    assert_eq!(upload.status(), 200);
    assert!(!upload.headers().contains_key("set-cookie"));
    assert_eq!(upload.bytes()?.as_ref(), &[0, 1, 255, b'D', b'S', b'H']);
    let redirect = client
        .get(url("/file?download"))
        .header("Cookie", cookie)
        .send()?;
    assert_eq!(redirect.status(), 302);
    assert_eq!(redirect.headers()["location"], "/file?version=1");
    assert_eq!(
        client
            .get(url("/"))
            .header("Cookie", cookie)
            .header("Origin", "http://unrelated.invalid")
            .send()?
            .status(),
        403
    );
    let mut socket = TcpStream::connect(address)?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(socket, "GET /api/remote.mux HTTP/1.1\r\nHost: {address}\r\nOrigin: {origin}\r\nCookie: {cookie}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n")?;
    let mut socket = BufReader::new(socket);
    let mut line = String::new();
    socket.read_line(&mut line)?;
    assert!(line.starts_with("HTTP/1.1 101 "), "{line}");
    while line != "\r\n" {
        line.clear();
        assert_ne!(socket.read_line(&mut line)?, 0);
    }
    let id = share(Status)?.devices[0].id;
    share(Revoke { id })?;
    assert_eq!(socket.read_line(&mut line)?, 0);
    assert_eq!(
        client
            .get(url("/"))
            .header("Cookie", cookie)
            .send()?
            .status(),
        401
    );
    assert_eq!(
        manage("stop", restarted["instance"].as_str())?.status(),
        401
    );
    host::call(&endpoint, "stop", restarted["instance"].as_str())?;
    share(Disable)?;
    assert!(share(Status)?.address.is_none());
    Ok(())
}

#[test]
fn loopback_web_keeps_its_own_pairing_when_lan_is_disabled() -> Result<()> {
    use host::ShareCommand::{Disable, Enable, Pair, PairLocal, Status};
    let home = tempfile::tempdir()?;
    runtime(home.path())?;
    let child = spawn(home.path())?;
    let mut guard = Host { child, home };
    let endpoint = wait(guard.home.path())?;
    let share = |command| host::share(&endpoint, &command);
    let initial = share(Status)?;
    assert!(initial.address.is_none());
    let address = initial.local_address.context("缺少本机 Web 入口")?;
    assert_eq!(*address.ip(), Ipv4Addr::LOCALHOST);
    let local = format!("http://{address}");
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    let status = |cookie: &str| {
        client
            .get(format!("{local}/_wingmate/status"))
            .header("Cookie", cookie)
            .send()
    };
    assert_eq!(status("")?.status(), 401);
    let link = share(PairLocal)?.pairing_url.context("缺少本机配对链接")?;
    let prefix = format!("{local}/?auth=");
    let code = link.strip_prefix(&prefix).context("本机配对链接格式错误")?;
    let lan_address = share(Enable {
        address: Some(Ipv4Addr::LOCALHOST),
    })?
    .address
    .context("缺少局域网入口")?;
    assert_ne!(address, lan_address);
    let lan = format!("http://{lan_address}");
    let lan_link = share(Pair)?.pairing_url.context("缺少局域网配对链接")?;
    let (_, lan_code) = lan_link
        .split_once("?auth=")
        .context("局域网配对链接格式错误")?;
    let exchange = |origin: &str, code: &str| {
        client
            .post(format!("{origin}/?auth"))
            .header("Origin", origin)
            .body(code.to_owned())
            .send()
    };
    assert_eq!(exchange(&lan, code)?.status(), 401);
    assert_eq!(exchange(&local, lan_code)?.status(), 401);
    let paired = exchange(&local, code)?;
    assert_eq!(paired.status(), 204);
    assert_eq!(exchange(&local, code)?.status(), 401);
    let cookie = paired.headers()["set-cookie"]
        .to_str()?
        .split(';')
        .next()
        .unwrap();
    let token = cookie.split_once('=').unwrap().1;
    assert_eq!(
        client
            .get(format!("{lan}/_wingmate/status"))
            .header(
                "Cookie",
                format!("wingmate_share_{}={token}", lan_address.port())
            )
            .send()?
            .status(),
        401
    );
    let disabled = share(Disable)?;
    assert!(disabled.address.is_none());
    assert_eq!(disabled.local_address, Some(address));
    host::call(&endpoint, "start", None)?;
    let started: serde_json::Value = status(cookie)?.error_for_status()?.json()?;
    assert_eq!(started["phase"], "running");
    let dsh = started["dsh_origin"]
        .as_str()
        .context("缺少 DSH 本机地址")?;
    let url = reqwest::Url::parse(dsh)?;
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    assert_ne!(url.port(), Some(address.port()));
    assert!(url.query().is_none());
    host::call(&endpoint, "stop", started["instance"].as_str())?;
    host::call(&endpoint, "shutdown", None)?;
    assert!(guard.child.wait()?.success());
    guard.child = spawn(guard.home.path())?;
    let restored = wait(guard.home.path())?;
    assert_eq!(
        host::share(&restored, &Status)?.local_address,
        Some(address)
    );
    assert_eq!(status(cookie)?.status(), 200);
    Ok(())
}

#[test]
fn host_owns_management_independently_of_business_and_clients() -> Result<()> {
    let home = tempfile::tempdir()?;
    runtime(home.path())?;
    let child = spawn(home.path())?;
    let mut guard = Host { child, home };
    let endpoint = wait(guard.home.path())?;
    let mut wrong = endpoint.clone();
    wrong.token = "invalid".into();
    assert!(host::call(&wrong, "start", None).is_err());
    wrong = endpoint.clone();
    wrong.instance = "wrong".into();
    assert!(host::call(&wrong, "start", None).is_err());
    assert!(host::call(&endpoint, "status", None)?.business.is_none());
    let a = host::call(&endpoint, "start", None)?
        .business
        .context("业务未启动")?;
    assert_eq!(
        host::call(&endpoint, "start", None)?
            .business
            .unwrap()
            .instance,
        a.instance
    );
    assert!(host::call(&endpoint, "stop", Some("replaced")).is_err());
    assert!(host::call(&endpoint, "shutdown", None).is_err());
    let duplicate = spawn(guard.home.path())?.wait()?;
    assert!(!duplicate.success());
    let b = host::call(&endpoint, "restart", Some(&a.instance))?
        .business
        .unwrap();
    assert_ne!(a.instance, b.instance);
    host::call(&endpoint, "stop", Some(&b.instance))?;
    let state = host::call(&endpoint, "status", None)?;
    assert_eq!(state.instance, endpoint.instance);
    assert!(state.business.is_none());
    // 服务只读取已安装的运行内容，无需发行入口再次释放。
    assert!(host::call(&endpoint, "start", None)?.business.is_some());
    host::call(&endpoint, "stop", None)?;
    fs::write(guard.home.path().join("runtime.json"), "invalid")?;
    assert!(host::call(&endpoint, "start", None).is_err());
    assert!(host::call(&endpoint, "status", None)?.business.is_none());
    host::call(&endpoint, "shutdown", None)?;
    assert!(guard.child.wait()?.success());
    assert!(host::endpoint(guard.home.path())?.is_none());
    Ok(())
}
