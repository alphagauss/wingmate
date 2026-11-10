// 将准备好的 DSH 运行包内嵌到服务库，最终由唯一桌面可执行文件携带。
use sha2::{Digest, Sha256};
use std::{env, fmt::Write, fs, io::BufReader, path::PathBuf};

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
    let bundled = env::var_os("CARGO_FEATURE_BUNDLED_RUNTIME").is_some();
    let directory = if bundled {
        checkout.join("work/build/runtime")
    } else {
        let dev = checkout.join("work/dev");
        println!("cargo:rustc-env=WINGMATE_DEV_ROOT={}", dev.display());
        dev.join("components")
    };
    let manifest = directory.join("manifest.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let generated = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("components.rs");
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
            if bundled {
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
