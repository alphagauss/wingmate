// 将准备好的 DSH 运行包内嵌到服务库，最终由唯一桌面可执行文件携带。
use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let archive = root.join("../../runtime/out/runtime.tar.zst");
    let manifest = root.join("../../runtime/package.json");
    println!("cargo:rerun-if-changed={}", archive.display());
    println!("cargo:rerun-if-changed={}", manifest.display());
    let package: serde_json::Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    let version = package["dependencies"]["@deepseek-ai/dsh"]
        .as_str()
        .expect("runtime/package.json 必须声明固定 DSH 版本");
    println!("cargo:rustc-env=WINGMATE_RUNTIME_ID=dsh-{version}");
    fs::copy(
        archive,
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("runtime.tar.zst"),
    )
    .expect("请先在仓库根目录运行 pnpm runtime:prepare");
}
