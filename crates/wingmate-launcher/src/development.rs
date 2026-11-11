//! Build tooling uses the same operation lock as dev startup and runtime collection.
use anyhow::{ensure, Result};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    path::{Component, Path},
    process::{Command, Stdio},
};

fn project_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

fn build_lock(root: &Path) -> Result<File> {
    fs::create_dir_all(root.join("work/build"))?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("work/build/prepare.lock"))?;
    lock.lock_exclusive()?;
    Ok(lock)
}

pub fn prepare(dev: bool) -> Result<()> {
    let root = project_root();
    // Archive preparation and development client builds share the plugin outputs.
    let _build_lock = build_lock(root)?;
    let _operation = if dev {
        Some(crate::service::operation_lock(
            &crate::runtime::directory()?
        )?)
    } else {
        None
    };
    let mut command = Command::new(crate::runtime::node()?);
    crate::runtime::quiet(&mut command);
    command
        .arg(root.join("scripts/prepare-runtime.mjs"))
        .current_dir(root)
        .env("WINGMATE_PREPARE_LOCKED", "1")
        .env_remove("NODE_OPTIONS")
        .stdin(Stdio::piped());
    if dev {
        command.arg("--dev");
    }
    let mut child = command.spawn()?;
    // Keep a lifeline open: an interrupted lock owner must not leave the Node preparer running.
    // Child::wait would close stdin itself unless we take ownership here.
    let _lifeline = child.stdin.take();
    ensure!(child.wait()?.success(), "运行包准备失败");
    let mut command = Command::new("cargo");
    command.args(["build", "-p", "wingmate-service"]);
    if !dev {
        command.arg("--release");
    }
    let status = command.current_dir(root).status()?;
    ensure!(status.success(), "管理服务构建失败");
    if dev {
        let status = Command::new("cargo")
            .args([
                "build",
                "-p",
                "wingmate-launcher",
                "--no-default-features",
                "--bin",
                "wingmate",
            ])
            .current_dir(root)
            .status()?;
        ensure!(status.success(), "开发引导入口构建失败");
    }
    Ok(())
}

pub fn rebuild_client(plugin: &str) -> Result<()> {
    let mut components = Path::new(plugin).components();
    ensure!(
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none(),
        "插件名称必须是单层目录"
    );
    let root = project_root();
    let source = root.join("plugins").join(plugin);
    if !source.join("package.json").is_file() {
        return Ok(());
    }
    let _build_lock = build_lock(root)?;
    let node = crate::runtime::node()?;
    let script = root.join("scripts/dev-plugin-clients.mjs");
    let status = Command::new(&node)
        .arg(&script)
        .arg("build")
        .arg(&source)
        .status()?;
    ensure!(status.success(), "插件客户端构建失败");

    let home = crate::directory()?;
    let _operation = crate::service::operation_lock(&home)?;
    if !home.join("runtime.json").is_file() {
        return Ok(());
    }
    let runtime = wingmate_service::runtime::Runtime::load(&home)?;
    runtime.validate(&home)?;
    let status = Command::new(node)
        .arg(script)
        .arg("publish")
        .arg(source)
        .arg(runtime.directory.join("plugins").join(plugin))
        .status()?;
    ensure!(status.success(), "插件客户端发布失败");
    Ok(())
}
