//! 官方 Linux 发行包的选择、校验和本机缓存。
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const REPOSITORY: &str = "alphagauss/wingmate";
const NAME: &str = "wingmate-linux-x64";
pub(super) const DESTINATION: &str = "~/.wingmate/remote-install/wingmate";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Offer {
    pub(super) tag: String,
    pub(super) size: u64,
    pub(super) sha256: String,
}

impl Offer {
    fn from_release(release: Release) -> Result<Self> {
        let tag = release.tag_name;
        ensure!(
            tag.starts_with('v')
                && tag.len() <= 64
                && tag
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
            "官方发行版本号无效"
        );
        let asset = release
            .assets
            .into_iter()
            .find(|asset| asset.name == NAME)
            .context("最新的官方发行尚未提供 Linux x64 安装包")?;
        let sha256 = asset
            .digest
            .as_deref()
            .and_then(|value| value.strip_prefix("sha256:"))
            .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .context("官方资产没有有效的 SHA-256")?
            .to_ascii_lowercase();
        ensure!(
            (1..=512 * 1024 * 1024).contains(&asset.size),
            "发行包大小超出支持范围"
        );
        Ok(Self {
            tag,
            size: asset.size,
            sha256,
        })
    }

    fn download_url(&self) -> String {
        format!(
            "https://github.com/{REPOSITORY}/releases/download/{}/{NAME}",
            self.tag
        )
    }
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent("wingmate-desktop")
        .connect_timeout(Duration::from_secs(12))
        .read_timeout(Duration::from_secs(45))
        .build()?)
}

pub(super) async fn latest(platform: &str) -> Result<Offer> {
    ensure!(
        platform.trim() == "Linux x86_64",
        "当前自动安装支持 Linux x64"
    );
    let release = client()?
        .get(format!(
            "https://api.github.com/repos/{REPOSITORY}/releases/latest"
        ))
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(20))
        .send()
        .await?
        .error_for_status()?
        .json::<Release>()
        .await?;
    Offer::from_release(release)
}

async fn matches(path: &Path, offer: &Offer) -> Result<bool> {
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if file.metadata().await?.len() != offer.size {
        return Ok(false);
    }
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 128 * 1024];
    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            return Ok(format!("{:x}", hash.finalize()) == offer.sha256);
        }
        hash.update(&buffer[..count]);
    }
}

pub(super) async fn download(home: &Path, platform: &str, approved: &Offer) -> Result<PathBuf> {
    // 复核桌面呈现给用户的版本，禁止从前端输入下载地址或更换确认过的资产。
    let offer = latest(platform).await?;
    ensure!(&offer == approved, "官方发行已更新，请重新检查版本后确认");
    let cache = home.join("downloads").join("remote");
    tokio::fs::create_dir_all(&cache).await?;
    let path = cache.join(&offer.sha256);
    if matches(&path, &offer).await? {
        return Ok(path);
    }
    let temp = tempfile::NamedTempFile::new_in(&cache)?;
    let mut file = tokio::fs::File::from_std(temp.reopen()?);
    let mut response = client()?
        .get(offer.download_url())
        .timeout(Duration::from_secs(900))
        .send()
        .await?
        .error_for_status()?;
    let mut size = 0u64;
    let mut hash = Sha256::new();
    while let Some(bytes) = response.chunk().await? {
        size += bytes.len() as u64;
        ensure!(size <= offer.size, "收到的发行包超过预期大小");
        hash.update(&bytes);
        file.write_all(&bytes).await?;
    }
    ensure!(
        size == offer.size && format!("{:x}", hash.finalize()) == offer.sha256,
        "发行包的大小或 SHA-256 校验失败"
    );
    file.sync_all().await?;
    drop(file);
    temp.persist(&path)?;
    Ok(path)
}

// 先校验临时文件，再用原子硬链接发布。并发安装已发布相同文件时允许复用。
pub(super) fn publish_script(hash: &str) -> String {
    let hash = super::remote::shell_quote(hash);
    format!(
        r#"
        expected={hash}
        check() {{
            actual=$(sha256sum -- "$1") || return 1
            case "$actual" in "$expected "*) return 0 ;; esac
            return 1
        }}
        check "$staged" || {{ echo '发行包 SHA-256 校验失败' >&2; exit 45; }}
        chmod 700 "$staged" || exit
        if ! ln -T -- "$staged" "$base/wingmate" 2>/dev/null; then
            [ -f "$base/wingmate" ] && [ -x "$base/wingmate" ] && check "$base/wingmate" ||
                {{ echo '安装位置已存在不同的文件，请指定可执行文件' >&2; exit 46; }}
        fi
    "#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_existing_official_linux_asset() {
        let release = |tag: &str, digest: Option<String>| Release {
            tag_name: tag.into(),
            assets: vec![Asset {
                name: NAME.into(),
                size: 100,
                digest,
            }],
        };
        let digest = || Some(format!("sha256:{}", "B".repeat(64)));
        let offer = Offer::from_release(release("v0.4.2", digest())).unwrap();
        assert_eq!(offer.sha256, "b".repeat(64));
        assert!(offer.download_url().ends_with("/v0.4.2/wingmate-linux-x64"));
        assert!(Offer::from_release(release("v0.4.2", None)).is_err());
    }
}
