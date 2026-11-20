//! 用独立宿主进程和无模型的 Node 夹具检查生命周期与身份边界。
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

static TEST_HOSTS: Mutex<()> = Mutex::new(());
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
    let child = spawn_tunnel(directory.path(), binary, origin)?;
    Ok(Host {
        child,
        home: directory,
    })
}

fn spawn_tunnel(home: &Path, binary: &Path, origin: &Path) -> Result<Child> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wingmate-service"));
    wingmate_service::runtime::quiet(&mut command);
    Ok(command
        .arg("--home")
        .arg(home)
        .env("CLOUDFLARED_PATH", binary)
        .env("TEST_TUNNEL_ORIGIN", origin)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?)
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
    use serde_json::{json, Value};
    let _serial = TEST_HOSTS.lock().unwrap_or_else(|error| error.into_inner());

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
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build()?;
    let initial = host::share(&endpoint, &Share::Status)?;
    let enabled = host::share(
        &endpoint,
        &Share::Enable {
            address: initial.suggested_address,
        },
    )?;
    let local = enabled.local_address.context("缺少本机入口")?;
    let loopback = format!("http://{local}");
    let lan = format!("http://{}", enabled.address.context("缺少局域网入口")?);
    let pair = |command, origin: &str| -> Result<String> {
        let link = host::share(&endpoint, &command)?
            .pairing_url
            .context("缺少配对链接")?;
        let code = link.split_once("/?auth=").unwrap().1;
        let reply = client
            .post(format!("{origin}/?auth"))
            .header("Origin", origin)
            .body(code.to_owned())
            .send()?
            .error_for_status()?;
        Ok(reply.headers()["set-cookie"]
            .to_str()?
            .split(';')
            .next()
            .unwrap()
            .into())
    };
    let local_cookie = pair(Share::PairLocal, &loopback)?;
    let lan_cookie = pair(Share::Pair, &lan)?;
    let share_web = |origin: &str, cookie: &str, command: Value| {
        client
            .post(format!("{origin}/_wingmate/share"))
            .header("Origin", origin)
            .header("Cookie", cookie)
            .json(&command)
            .send()
    };
    let start = json!({"action": "quick_tunnel", "operation": "start"});
    assert_eq!(share_web(&loopback, "", start.clone())?.status(), 401);
    let started: Value = share_web(&loopback, &local_cookie, start.clone())?
        .error_for_status()?
        .json()?;
    assert_eq!(started["quick_tunnel_control"], true);
    assert_eq!(started["tunnel"]["mode"], "quick");
    let public = tunnel_ready(&endpoint)?;
    let authority = public.trim_start_matches("https://");
    assert_eq!(fs::read_to_string(&origin_path)?, loopback);
    let repeated: Value = share_web(&lan, &lan_cookie, start.clone())?
        .error_for_status()?
        .json()?;
    assert_eq!(repeated["quick_tunnel_control"], true);
    assert_eq!(repeated["tunnel"]["url"], public);
    assert_eq!(client.get(&loopback).send()?.status(), 401);
    let identity = |host: Option<&str>| -> Result<serde_json::Value> {
        let mut request = client.get(format!("{loopback}/_wingmate"));
        if let Some(host) = host {
            request = request.header("Host", host);
        }
        Ok(request.send()?.error_for_status()?.json()?)
    };
    assert_eq!(identity(None)?["transport"], "local");
    assert_eq!(identity(Some(authority))?["transport"], "public");
    let manifest = client
        .get(format!("{loopback}/manifest.webmanifest"))
        .header("Host", authority)
        .header("Accept", "application/manifest+json")
        .send()?;
    assert_eq!(manifest.status(), 200);
    assert_eq!(manifest.headers()["cache-control"], "no-store");
    assert_eq!(manifest.json::<serde_json::Value>()?["short_name"], "DSH");
    let phone_pair = client
        .get(format!("{loopback}/?pair=1"))
        .header("Host", authority)
        .send()?;
    assert_eq!(phone_pair.status(), 401);
    assert!(phone_pair.text()?.contains("id=\"loginForm\""));
    assert_eq!(
        client
            .get(format!("{loopback}/_wingmate/status"))
            .header("Host", authority)
            .send()?
            .status(),
        401
    );
    let pairing = host::share(&endpoint, &Share::TunnelPair { long_lived: false })?
        .pairing_url
        .unwrap();
    assert!(host::share(&endpoint, &Share::TunnelPair { long_lived: true }).is_err());
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
    let public_share = |command: Value| {
        client
            .post(format!("{loopback}/_wingmate/share"))
            .header("Host", authority)
            .header("Origin", &public)
            .header("Cookie", cookie)
            .json(&command)
            .send()
    };
    let status: Value = public_share(json!({"action": "status"}))?
        .error_for_status()?
        .json()?;
    assert_eq!(status["quick_tunnel_control"], false);
    assert_eq!(status["tunnel"]["url"], public);
    for operation in ["start", "replace_address", "stop"] {
        assert_eq!(
            public_share(
                json!({"action": "quick_tunnel", "operation": operation, "expected_url": public})
            )?
            .status(),
            403
        );
    }
    assert_eq!(exchange(&public)?.status(), 401);
    let reopen = client
        .get(format!("{loopback}/?auth={code}"))
        .header("Host", authority)
        .header("Cookie", cookie)
        .send()?;
    assert_eq!(reopen.status(), 303);
    assert_eq!(reopen.headers()["location"], "/");
    assert!(!reopen.headers().contains_key("set-cookie"));
    let recovery = client
        .get(&loopback)
        .header("Host", authority)
        .header("Cookie", cookie)
        .header("Accept", "text/html")
        .send()?;
    assert_eq!(recovery.status(), 200);
    assert!(recovery.text()?.contains("/_wingmate/home.js"));

    runtime(service.home.path())?;
    let started = host::call(&endpoint, "start", None)?;
    let events = |encoding: &str| {
        client
            .get(format!("{loopback}/plugins/events"))
            .header("Host", authority)
            .header("Cookie", cookie)
            .header("Accept-Encoding", encoding)
            .send()
    };
    let plain = events("gzip;q=0, *;q=1")?;
    assert_eq!(plain.status(), 200);
    assert!(!plain.headers().contains_key("content-encoding"));
    drop(plain);
    let compressed = events("br, gzip")?;
    assert_eq!(compressed.status(), 200);
    assert_eq!(compressed.headers()["content-type"], "text/event-stream");
    assert_eq!(compressed.headers()["content-encoding"], "gzip");
    assert_eq!(compressed.headers()["cache-control"], "no-store");
    assert_eq!(compressed.headers()["vary"], "accept-encoding");
    let mut stream = BufReader::new(flate2::read::GzDecoder::new(compressed));
    for expected in ["data: graph\n", "\n", "data: rebuilt\n", "\n"] {
        let mut line = String::new();
        stream.read_line(&mut line)?;
        assert_eq!(line, expected);
    }
    drop(stream);
    let business = started.business.context("缺少已启动的业务实例")?;
    host::call(&endpoint, "stop", Some(&business.instance))?;

    let pending: Value = public_share(json!({"action": "tunnel_pair"}))?
        .error_for_status()?
        .json()?;
    let old_invite = pending["pairings"][0]["url"]
        .as_str()
        .unwrap()
        .split_once("/?auth=")
        .unwrap()
        .1;
    let replace =
        json!({"action": "quick_tunnel", "operation": "replace_address", "expected_url": public});
    assert_eq!(share_web(&lan, &lan_cookie, replace.clone())?.status(), 200);
    let replacement = tunnel_ready(&endpoint)?;
    assert_ne!(replacement, public);
    let current: Value = share_web(&lan, &lan_cookie, json!({"action": "status"}))?
        .error_for_status()?
        .json()?;
    assert_eq!(current["tunnel"]["url"], replacement);
    assert!(current["tunnel_pairing_id"].is_null());
    assert_eq!(share_web(&loopback, &local_cookie, replace)?.status(), 409);
    let stale_stop = json!({"action": "quick_tunnel", "operation": "stop", "expected_url": public});
    assert_eq!(share_web(&lan, &lan_cookie, stale_stop)?.status(), 409);
    let replacement_authority = replacement.strip_prefix("https://").unwrap();
    assert_eq!(
        client
            .get(format!("{loopback}/_wingmate/session"))
            .header("Host", replacement_authority)
            .header("Cookie", cookie)
            .send()?
            .status(),
        401
    );
    assert_eq!(
        client
            .post(format!("{loopback}/?auth"))
            .header("Host", replacement_authority)
            .header("Origin", &replacement)
            .body(old_invite.to_owned())
            .send()?
            .status(),
        401
    );
    let fresh: Value = share_web(&lan, &lan_cookie, json!({"action": "tunnel_pair"}))?
        .error_for_status()?
        .json()?;
    assert_eq!(
        fresh["pairings"][0]["url"]
            .as_str()
            .unwrap()
            .split_once("/?auth=")
            .unwrap()
            .0,
        replacement
    );

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
        Some(replacement.as_str())
    );
    let stop = json!({"action": "quick_tunnel", "operation": "stop", "expected_url": replacement});
    let stopped: Value = share_web(&lan, &lan_cookie, stop.clone())?
        .error_for_status()?
        .json()?;
    assert!(stopped["tunnel"]["url"].is_null());
    assert!(stopped["tunnel"]["mode"].is_null());
    host::share(
        &endpoint,
        &Share::TunnelStart {
            mode: TunnelMode::Named,
        },
    )?;
    assert_eq!(tunnel_ready(&endpoint)?, format!("https://{named}"));
    assert_eq!(share_web(&lan, &lan_cookie, start)?.status(), 409);
    assert_eq!(share_web(&loopback, &local_cookie, stop)?.status(), 409);
    assert_eq!(
        client
            .get(&loopback)
            .header("Host", authority)
            .send()?
            .status(),
        421
    );
    assert_eq!(identity(Some(named))?["transport"], "public");
    let named_origin = format!("https://{named}");
    let create_invite =
        |endpoint: &host::Endpoint| host::share(endpoint, &Share::TunnelPair { long_lived: true });
    let invite = create_invite(&endpoint)?;
    let expiry = invite.pairing_expires_at.unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    assert!((86_395..=86_400).contains(&expiry.saturating_sub(now)));
    assert_eq!(invite.tunnel_invite_expires_at, Some(expiry));
    let invite_id = invite.tunnel_pairing_id.context("缺少具名邀请标识")?;
    let link = invite.pairing_url.unwrap();
    let code = link.split_once("?auth=").unwrap().1;
    assert!(!fs::read_to_string(service.home.path().join("share-devices.json"))?.contains(code));
    host::call(&endpoint, "shutdown", None)?;
    assert!(service.child.wait()?.success());
    service.child = spawn_tunnel(service.home.path(), &binary, &origin_path)?;
    let endpoint = wait(service.home.path())?;
    assert_eq!(tunnel_ready(&endpoint)?, named_origin);
    let status = host::share(&endpoint, &Share::Status)?;
    assert_eq!(status.tunnel_invite_expires_at, Some(expiry));
    assert_eq!(
        status.tunnel_pairing_id.as_deref(),
        Some(invite_id.as_str())
    );
    let redeem = |code: &str| {
        client
            .post(format!("{loopback}/?auth"))
            .header("Host", named)
            .header("Origin", &named_origin)
            .body(code.to_owned())
            .send()
    };
    let named_session = redeem(code)?;
    assert_eq!(named_session.status(), 204);
    let named_cookie = named_session.headers()["set-cookie"]
        .to_str()?
        .split(';')
        .next()
        .unwrap();
    assert_eq!(redeem(code)?.status(), 401);
    assert!(host::share(&endpoint, &Share::Status)?
        .tunnel_invite_expires_at
        .is_none());
    let web_request = |path: &str| {
        client
            .post(format!("{loopback}/_wingmate/{path}"))
            .header("Host", named)
            .header("Origin", &named_origin)
            .header("Cookie", named_cookie)
    };
    let web_share = |command: serde_json::Value| web_request("share").json(&command).send();
    let second: serde_json::Value = web_share(serde_json::json!({
        "action": "tunnel_pair", "long_lived": false,
    }))?
    .error_for_status()?
    .json()?;
    assert_ne!(
        second["tunnel_pairing_id"].as_str(),
        Some(invite_id.as_str())
    );
    assert_eq!(second["tunnel"]["url"], named_origin);
    let second = second["pairings"][0]["url"].as_str().unwrap();
    let (invite_origin, second_code) = second.split_once("/?auth=").unwrap();
    assert_eq!(invite_origin, named_origin);
    assert_eq!(
        web_share(serde_json::json!({ "action": "tunnel_revoke" }))?.status(),
        200
    );
    assert!(host::share(&endpoint, &Share::Status)?
        .tunnel_pairing_id
        .is_none());
    assert_eq!(redeem(second_code)?.status(), 401);
    let logout = web_request("logout").send()?;
    assert_eq!(logout.status(), 204);
    let expired_cookie = logout.headers()["set-cookie"].to_str()?;
    assert!(expired_cookie.contains("Max-Age=0"));
    assert!(expired_cookie.ends_with("; Secure"));
    assert_eq!(
        web_share(serde_json::json!({ "action": "status" }))?.status(),
        401
    );
    assert_eq!(
        client
            .post(format!("{loopback}/_wingmate/login"))
            .header("Host", named)
            .header("Origin", &named_origin)
            .body("unused-password")
            .send()?
            .status(),
        404
    );
    host::share(&endpoint, &Share::TunnelStop)?;
    assert_eq!(identity(None)?["transport"], "local");
    assert_eq!(
        client.get(&loopback).header("Host", named).send()?.status(),
        421
    );
    host::call(&endpoint, "shutdown", None)?;
    assert!(service.child.wait()?.success());
    assert!(TcpListener::bind(local).is_ok());
    Ok(())
}

#[test]
fn lan_password_issues_a_revocable_session_and_is_limited_to_lan() -> Result<()> {
    use host::ShareCommand as Share;
    let _serial = TEST_HOSTS.lock().unwrap_or_else(|error| error.into_inner());
    let home = tempfile::tempdir()?;
    let child = spawn(home.path())?;
    let mut guard = Host { child, home };
    let endpoint = wait(guard.home.path())?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    let password = "sample-strong-password";
    let configured = host::share(
        &endpoint,
        &Share::LanPasswordSet {
            password: password.into(),
            recoverable: false,
        },
    )?;
    assert!(configured.lan_password_enabled);
    assert!(configured.lan_password.is_none());
    assert!(!fs::read_to_string(guard.home.path().join("lan-password.json"))?.contains(password));
    assert!(host::share(&endpoint, &Share::LanPasswordReveal).is_err());
    let local = format!("http://{}", configured.local_address.unwrap());
    let address = configured
        .suggested_address
        .context("缺少测试用局域网地址")?;
    let lan = format!(
        "http://{}",
        host::share(
            &endpoint,
            &Share::Enable {
                address: Some(address)
            }
        )?
        .address
        .unwrap()
    );
    let login = |target: &str, origin: &str, password: &str| {
        client
            .post(format!("{target}/_wingmate/login"))
            .header("Origin", origin)
            .body(password.to_owned())
            .send()
    };
    assert_eq!(login(&local, &local, password)?.status(), 404);
    assert_eq!(login(&lan, &local, password)?.status(), 403);
    assert_eq!(login(&lan, &lan, "wrong-password")?.status(), 401);
    let reply = login(&lan, &lan, password)?;
    assert_eq!(reply.status(), 204);
    let cookie = reply.headers()["set-cookie"]
        .to_str()?
        .split(';')
        .next()
        .unwrap();
    let session = || {
        client
            .get(format!("{lan}/_wingmate/session"))
            .header("Cookie", cookie)
            .send()
    };
    assert_eq!(session()?.status(), 204);
    host::call(&endpoint, "shutdown", None)?;
    assert!(guard.child.wait()?.success());
    guard.child = spawn(guard.home.path())?;
    let endpoint = wait(guard.home.path())?;
    let status = host::share(&endpoint, &Share::Status)?;
    assert!(status.lan_password_enabled);
    assert_eq!(session()?.status(), 204);
    host::share(
        &endpoint,
        &Share::Revoke {
            id: status.devices[0].id,
        },
    )?;
    assert_eq!(session()?.status(), 401);
    for _ in 0..5 {
        assert_eq!(login(&lan, &lan, "wrong-password")?.status(), 401);
    }
    assert_eq!(login(&lan, &lan, password)?.status(), 429);
    let replacement = "replacement-password";
    host::share(
        &endpoint,
        &Share::LanPasswordSet {
            password: replacement.into(),
            recoverable: false,
        },
    )?;
    assert_eq!(login(&lan, &lan, password)?.status(), 401);
    assert_eq!(login(&lan, &lan, replacement)?.status(), 204);
    host::share(&endpoint, &Share::LanPasswordDisable)?;
    assert_eq!(login(&lan, &lan, replacement)?.status(), 404);
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
  if (request.url === '/plugins/events') {
    response.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
    response.write('data: graph\n\n');
    const timer = setTimeout(() => response.write('data: rebuilt\n\n'), 100);
    response.on('close', () => clearTimeout(timer));
    return;
  }
  if (request.url.startsWith('/plugins/') || request.url === '/api/private?rev=test') {
    response.writeHead(200, { 'cache-control': 'public, max-age=31536000, immutable' }).end('bundle');
    return;
  }
  if (request.url === '/assets/main.js') {
    const unchanged = request.headers['if-none-match'] === '"main"';
    response.writeHead(unchanged ? 304 : 200, { 'cache-control': 'no-cache', etag: '"main"' });
    response.end(unchanged ? undefined : 'static');
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
    use host::ShareCommand::{Disable, Enable, Pair, PairLocal, Status};
    use serde_json::{json, Value};
    let _serial = TEST_HOSTS.lock().unwrap_or_else(|error| error.into_inner());
    let home = tempfile::tempdir()?;
    runtime(home.path())?;
    let child = spawn(home.path())?;
    let mut guard = Host { child, home };
    let endpoint = wait(guard.home.path())?;
    let share = |command| host::share(&endpoint, &command);
    let initial = share(Status)?;
    assert!(initial.address.is_none());
    let ip = initial
        .suggested_address
        .context("缺少测试用内网 IPv4 地址")?;
    let primary = initial.local_address.context("缺少本机 Web 入口")?;
    let occupied = TcpListener::bind((ip, primary.port()))?;
    let enabled = share(Enable { address: Some(ip) })?;
    let address = enabled.address.unwrap();
    assert!(host::WEB_PORTS.contains(&address.port()));
    assert_ne!(address.port(), primary.port());
    assert_eq!(enabled.local_address.unwrap().port(), address.port());
    let named_service = format!("http://127.0.0.1:{}", address.port());
    assert_eq!(
        enabled.tunnel.unwrap().named_service.as_deref(),
        Some(named_service.as_str())
    );
    assert_eq!(enabled.lan_addresses, vec![address]);
    let origin = format!("http://{address}");
    let url = |path: &str| format!("{origin}{path}");
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()?;
    let identity: serde_json::Value = client
        .get(url("/_wingmate"))
        .send()?
        .error_for_status()?
        .json()?;
    assert_eq!(identity["transport"], "lan");
    assert_eq!(client.get(url("/")).send()?.status(), 401);
    assert_eq!(
        client
            .get(url("/"))
            .header("Host", "unrelated.invalid")
            .send()?
            .status(),
        421
    );
    let previous = share(Pair)?.lan_pairing_id.context("缺少局域网配对标识")?;
    let pairing = share(Pair)?;
    assert_ne!(pairing.lan_pairing_id.as_deref(), Some(previous.as_str()));
    let link = pairing.pairing_url.unwrap();
    assert_eq!(
        share(Enable { address: Some(ip) })?.lan_pairing_id,
        pairing.lan_pairing_id
    );
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
    assert!(share(Status)?.lan_pairing_id.is_none());
    let cookie = paired.headers()["set-cookie"]
        .to_str()?
        .split(';')
        .next()
        .unwrap();
    assert_eq!(exchange(&origin)?.status(), 401);
    let web_share = |auth: Option<&str>, source: Option<&str>, command: Value| {
        let mut request = client.post(url("/_wingmate/share")).json(&command);
        if let Some(auth) = auth {
            request = request.header("Cookie", auth);
        }
        if let Some(source) = source {
            request = request.header("Origin", source);
        }
        request.send()
    };
    let share_from_lan = |command| web_share(Some(cookie), Some(&origin), command);
    assert_eq!(
        web_share(None, Some(&origin), json!({"action": "status"}))?.status(),
        401
    );
    assert_eq!(
        web_share(Some(cookie), None, json!({"action": "pair"}))?.status(),
        403
    );
    assert_eq!(
        web_share(
            Some(cookie),
            Some("http://unrelated.invalid"),
            json!({"action": "pair"})
        )?
        .status(),
        403
    );
    for action in [
        "enable",
        "disable",
        "pair_local",
        "lan_password_reveal",
        "tunnel_start",
        "tunnel_stop",
        "tunnel_configure",
    ] {
        assert_eq!(share_from_lan(json!({"action": action}))?.status(), 400);
    }
    let info: Value = client
        .get(url("/_wingmate/status"))
        .header("Cookie", cookie)
        .send()?
        .json()?;
    assert_eq!(info["transport"], "lan");
    assert_eq!(info["share_management"], true);
    let local = format!("http://{}", enabled.local_address.unwrap());
    let local_link = share(PairLocal)?.pairing_url.unwrap();
    let local_code = local_link.split_once("?auth=").unwrap().1;
    assert_eq!(
        client
            .post(format!("{local}/?auth"))
            .header("Origin", &local)
            .body(local_code.to_owned())
            .send()?
            .status(),
        204
    );
    let status: Value = share_from_lan(json!({"action": "status"}))?
        .error_for_status()?
        .json()?;
    let current = &status["current_device"];
    let current_id = uuid::Uuid::parse_str(current.as_str().unwrap())?;
    let local_device = share(Status)?
        .devices
        .into_iter()
        .find(|device| device.id != current_id)
        .unwrap();
    assert_eq!(status["devices"].as_array().unwrap().len(), 1);
    assert_eq!(*current, status["devices"][0]["id"]);
    assert!(status.get("local_address").is_none());
    assert!(status["tunnel"].get("named_service").is_none());
    assert_eq!(
        share_from_lan(json!({"action": "revoke", "id": current}))?.status(),
        400
    );
    assert_eq!(
        share_from_lan(json!({"action": "revoke", "id": local_device.id}))?.status(),
        409
    );

    let invitation: Value = share_from_lan(json!({"action": "pair"}))?
        .error_for_status()?
        .json()?;
    assert!(invitation["pairings"][0]["qr"]
        .as_str()
        .unwrap()
        .contains("<svg"));
    let invited_url = invitation["pairings"][0]["url"].as_str().unwrap();
    let invited_code = invited_url.strip_prefix(&prefix).unwrap();
    let invited = client
        .post(url("/?auth"))
        .header("Origin", &origin)
        .body(invited_code.to_owned())
        .send()?;
    assert_eq!(invited.status(), 204);
    let invited_cookie = invited.headers()["set-cookie"]
        .to_str()?
        .split(';')
        .next()
        .unwrap();
    let status: Value = share_from_lan(json!({"action": "status"}))?
        .error_for_status()?
        .json()?;
    let devices = status["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 2);
    let invited_id = &devices
        .iter()
        .find(|device| &device["id"] != current)
        .unwrap()["id"];
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
    let bundle = url("/plugins/??test/client.js&rev=one");
    let versioned = client.get(&bundle).header("Cookie", cookie).send()?;
    assert_eq!(versioned.status(), 200);
    assert_eq!(
        versioned.headers()["cache-control"],
        "private, max-age=31536000, immutable"
    );
    let anonymous = client.get(&bundle).send()?;
    assert_eq!(anonymous.status(), 401);
    assert_eq!(anonymous.headers()["cache-control"], "no-store");
    for path in ["/plugins/test/client.js", "/api/private?rev=test"] {
        let reply = client.get(url(path)).header("Cookie", cookie).send()?;
        assert_eq!(reply.headers()["cache-control"], "no-store");
    }
    let changed = client
        .get(url("/assets/main.js"))
        .header("Cookie", cookie)
        .send()?;
    assert_eq!(changed.headers()["cache-control"], "private, no-cache");
    let etag = changed.headers()["etag"].clone();
    let unchanged = client
        .get(url("/assets/main.js"))
        .header("Cookie", cookie)
        .header("If-None-Match", etag)
        .send()?;
    assert_eq!(unchanged.status(), 304);
    assert_eq!(unchanged.headers()["cache-control"], "private, no-cache");
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
    write!(socket, "GET /api/remote.mux HTTP/1.1\r\nHost: {address}\r\nOrigin: {origin}\r\nCookie: {invited_cookie}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n")?;
    let mut socket = BufReader::new(socket);
    let mut line = String::new();
    socket.read_line(&mut line)?;
    assert!(line.starts_with("HTTP/1.1 101 "), "{line}");
    while line != "\r\n" {
        line.clear();
        assert_ne!(socket.read_line(&mut line)?, 0);
    }
    assert_eq!(
        share_from_lan(json!({"action": "revoke", "id": invited_id}))?.status(),
        200
    );
    assert_eq!(socket.read_line(&mut line)?, 0);
    let session_status = |cookie: &str| {
        client
            .get(url("/_wingmate/session"))
            .header("Cookie", cookie)
            .send()
    };
    assert_eq!(session_status(invited_cookie)?.status(), 401);
    assert_eq!(session_status(cookie)?.status(), 204);
    let logout = |source: &str| {
        client
            .post(url("/_wingmate/logout"))
            .header("Origin", source)
            .header("Cookie", cookie)
            .send()
    };
    assert_eq!(logout("http://unrelated.invalid")?.status(), 403);
    let logged_out = logout(&origin)?;
    assert_eq!(logged_out.status(), 204);
    assert!(logged_out.headers()["set-cookie"]
        .to_str()?
        .contains("Max-Age=0"));
    assert_eq!(session_status(cookie)?.status(), 401);
    assert_eq!(
        host::call(&endpoint, "status", None)?
            .business
            .unwrap()
            .phase,
        "running"
    );
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
    host::call(&endpoint, "shutdown", None)?;
    assert!(guard.child.wait()?.success());
    drop(occupied);
    guard.child = spawn(guard.home.path())?;
    let restored = wait(guard.home.path())?;
    assert_eq!(
        host::call(&restored, "status", None)?
            .web_address
            .unwrap()
            .port(),
        address.port()
    );
    Ok(())
}

#[test]
fn loopback_web_keeps_its_own_pairing_when_lan_is_disabled() -> Result<()> {
    use host::ShareCommand::{DesktopSession, Disable, Enable, Pair, PairLocal, Status};
    let _serial = TEST_HOSTS.lock().unwrap_or_else(|error| error.into_inner());
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
        .redirect(reqwest::redirect::Policy::none())
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
        address: Some(
            initial
                .suggested_address
                .context("缺少测试用内网 IPv4 地址")?,
        ),
    })?
    .address
    .context("缺少局域网入口")?;
    assert_ne!(address, lan_address);
    assert_eq!(address.port(), lan_address.port());
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
    assert!(share(DesktopSession {
        instance: "stale".into()
    })
    .is_err());
    let instance = started["instance"].as_str().unwrap();
    let ticket = || -> Result<String> {
        share(DesktopSession {
            instance: instance.into(),
        })?
        .pairing_url
        .context("缺少桌面连接凭据")
    };
    let desktop = ticket()?;
    let redeemed = client.get(&desktop).send()?;
    assert_eq!(redeemed.status(), 200);
    assert_eq!(redeemed.headers()["referrer-policy"], "no-referrer");
    let desktop_cookie_header = redeemed.headers()["set-cookie"].to_str()?;
    assert!(desktop_cookie_header.contains("SameSite=Strict"));
    let desktop_cookie = desktop_cookie_header.split(';').next().unwrap();
    assert_eq!(status(desktop_cookie)?.status(), 200);
    assert_eq!(client.get(&desktop).send()?.status(), 401);
    let reused = client
        .get(ticket()?)
        .header("Cookie", desktop_cookie)
        .send()?;
    assert_eq!(reused.status(), 200);
    assert!(!reused.headers().contains_key("set-cookie"));
    assert!(redeemed.text()?.contains("location.replace('/')"));
    assert_eq!(share(Status)?.devices.len(), 2);
    let stale = ticket()?;
    host::call(&endpoint, "stop", Some(instance))?;
    assert_eq!(client.get(&stale).send()?.status(), 409);
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
    let _serial = TEST_HOSTS.lock().unwrap_or_else(|error| error.into_inner());
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
