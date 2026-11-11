//! Build tooling uses the same operation lock as dev startup and runtime collection.
use anyhow::{ensure, Result};
use fs2::FileExt;
use std::{
    fs::{self, OpenOptions},
    path::Path,
    process::{Command, Stdio},
};

pub fn prepare(dev: bool) -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    fs::create_dir_all(root.join("work/build"))?;
    // Both modes build the same plugin outputs; serialize them as well as archive writes.
    let build_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("work/build/prepare.lock"))?;
    build_lock.lock_exclusive()?;
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
