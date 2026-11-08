// 将准备好的 DSH 运行包内嵌到服务库，最终由唯一桌面可执行文件携带。
use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let checkout = root.parent().unwrap().parent().unwrap();
    let runtime = checkout.join("runtime");
    let manifest = runtime.join("package.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let package: serde_json::Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    let version = package["dependencies"]["@deepseek-ai/dsh"]
        .as_str()
        .expect("runtime/package.json 必须声明固定 DSH 版本");
    println!("cargo:rustc-env=WINGMATE_RUNTIME_ID=dsh-{version}");
    if env::var_os("CARGO_FEATURE_BUNDLED_RUNTIME").is_none() {
        let stage = runtime.join("stage");
        let current = stage.join(format!("current-{version}"));
        println!("cargo:rerun-if-changed={}", current.display());
        println!(
            "cargo:rustc-env=WINGMATE_DEV_ROOT={}",
            checkout.join("work/dev").display()
        );
        if let Ok(key) = fs::read_to_string(current) {
            let key = key.trim();
            assert!(
                key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
                "开发安装树标识无效，请运行 pnpm runtime:dev"
            );
            println!(
                "cargo:rustc-env=WINGMATE_DEV_STAGE={}",
                stage.join(key).display()
            );
        }
        return;
    }

    let archive = runtime.join("out/runtime.tar.zst");
    println!("cargo:rerun-if-changed={}", archive.display());
    fs::copy(
        archive,
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("runtime.tar.zst"),
    )
    .expect("请先在仓库根目录运行 pnpm runtime:prepare");
}
