// 将并列的服务、桌面和运行组件内嵌到无 GUI 的发行入口。
use sha2::{Digest, Sha256};
use std::{env, fmt::Write, fs, io::BufReader, path::PathBuf};

const RELEASE_COMPRESSION_LEVEL: i32 = 9;

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let checkout = root.parent().unwrap().parent().unwrap();
    let runtime = checkout.join("runtime");
    let manifest = runtime.join("package.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let package: serde_json::Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    let version = package["dependencies"]["@deepseek-ai/dsh"]
        .as_str()
        .expect("runtime/package.json 必须声明固定 DSH 版本");
    println!("cargo:rustc-env=WINGMATE_RUNTIME_ID=dsh-{version}");
    let bundled = env::var_os("CARGO_FEATURE_BUNDLED_RUNTIME").is_some();
    let release = bundled || env::var_os("CARGO_FEATURE_RELEASE_CLIENT").is_some();
    let directory = if release {
        checkout.join("work/build/runtime")
    } else {
        let dev = checkout.join("work/dev");
        println!("cargo:rustc-env=WINGMATE_DEV_ROOT={}", dev.display());
        dev.join("components")
    };
    let bin_directory = checkout.join(if release {
        "target/release"
    } else {
        "target/debug"
    });
    let mut binaries = format!(
        "const BINARY_DIRECTORY: &str = {:?};\n",
        bin_directory.to_str().unwrap()
    );
    for (name, constant) in [
        ("wingmate-service", "SERVICE_ARCHIVE"),
        ("wingmate-desktop", "DESKTOP_ARCHIVE"),
    ] {
        let binary = bin_directory.join(format!(
            "{name}{}",
            if env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
                ".exe"
            } else {
                ""
            }
        ));
        println!("cargo:rerun-if-changed={}", binary.display());
        let enabled = name == "wingmate-service" || env::var_os("CARGO_FEATURE_DESKTOP").is_some();
        if bundled && enabled {
            let bytes = fs::read(&binary).unwrap_or_else(|error| {
                panic!("无法读取 {}，请先准备发行组件: {error}", binary.display())
            });
            let hash = format!("{:x}", Sha256::digest(&bytes));
            let compressed = zstd::bulk::compress(&bytes, RELEASE_COMPRESSION_LEVEL).unwrap();
            let archive = out.join(format!("{name}.zst"));
            fs::write(&archive, compressed).unwrap();
            writeln!(
                binaries,
                "const {constant}: Option<(&[u8], &str)> = Some((include_bytes!({archive:?}), {hash:?}));"
            )
            .unwrap();
        } else {
            writeln!(binaries, "const {constant}: Option<(&[u8], &str)> = None;").unwrap();
        }
    }
    fs::write(out.join("binaries.rs"), binaries).unwrap();
    let manifest = directory.join("manifest.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let generated = out.join("components.rs");
    let mut source = String::from("static ARCHIVES: &[(&str, Archive)] = &[\n");
    match fs::read(&manifest) {
        Ok(bytes) => {
            let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(data["format"], 1);
            let target = env::var("TARGET").unwrap();
            if data["target"].as_str() != Some(target.as_str()) {
                assert!(
                    !bundled,
                    "运行包目标与 {target} 不匹配，请运行 pnpm runtime:prepare"
                );
                // The preparation tool itself must build before it can replace a stale dev manifest.
                fs::write(
                    generated,
                    "static ARCHIVES: &[(&str, Archive)] = &[];\nstatic MANIFEST: &str = \"\";\n",
                )
                .unwrap();
                return;
            }
            if release {
                let node_arch = data["node_arch"].as_str().expect("运行包未声明 Node 架构");
                println!("cargo:rustc-env=WINGMATE_NODE_ARCH={node_arch}");
            }
            for (name, part) in data["components"].as_object().unwrap() {
                let hash = part["sha256"].as_str().unwrap();
                assert!(hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()));
                assert!(name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'));
                let archive = directory.join(format!("{name}-{hash}.tar.zst"));
                if bundled {
                    println!("cargo:rerun-if-changed={}", archive.display());
                    let mut reader = BufReader::new(fs::File::open(&archive).unwrap());
                    let mut digest = Sha256::new();
                    std::io::copy(&mut reader, &mut digest).unwrap();
                    assert_eq!(
                        format!("{:x}", digest.finalize()),
                        hash,
                        "{}",
                        archive.display()
                    );
                    writeln!(
                        source,
                        "({name:?}, include_bytes!({:?})),",
                        archive.to_str().unwrap()
                    )
                    .unwrap();
                } else {
                    writeln!(source, "({name:?}, {:?}),", archive.to_str().unwrap()).unwrap();
                }
            }
            println!(
                "cargo:rustc-env=WINGMATE_RUNTIME_REVISION={:x}",
                Sha256::digest(&bytes)
            );
            source.push_str("];\n");
            writeln!(
                source,
                "static MANIFEST: &str = {:?};",
                String::from_utf8(bytes).unwrap()
            )
            .unwrap();
        }
        Err(error) if !bundled && error.kind() == std::io::ErrorKind::NotFound => {
            source.push_str("];\nstatic MANIFEST: &str = \"\";\n");
        }
        Err(error) => panic!(
            "无法读取 {}，请运行 pnpm runtime:prepare: {error}",
            manifest.display()
        ),
    }
    fs::write(generated, source).unwrap();
}
