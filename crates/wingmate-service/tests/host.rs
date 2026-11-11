//! 用独立宿主进程和无模型的 Node 夹具检查生命周期与身份边界。
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
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
console.log('dsh web: http://127.0.0.1:12345/?token=test');
process.on('SIGTERM', () => process.exit(0));
setInterval(() => {}, 1000);
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
