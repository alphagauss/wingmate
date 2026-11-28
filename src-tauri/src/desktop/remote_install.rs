//! 官方 Linux 发行包的选择、校验和本机缓存。
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, SeekFrom},
    path::Path,
    time::{Duration, Instant},
};
use tauri::{Url, Webview};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt},
};

const REPOSITORY: &str = "alphagauss/wingmate";
pub(super) const NAME: &str = "wingmate-linux-x64";
pub(super) const DESTINATION: &str = "~/.wingmate/downloads/wingmate-linux-x64";

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    Download,
    Transfer,
    Verify,
    Connect,
}

pub(super) struct Progress<'a> {
    webview: &'a Webview,
    page: Url,
    id: Option<u32>,
}

impl<'a> Progress<'a> {
    pub(super) fn new(webview: &'a Webview, page: Url, id: Option<u32>) -> Self {
        Self { webview, page, id }
    }

    pub(super) fn stage(&self, stage: Stage) {
        self.publish(stage, 0, None);
    }

    fn publish(&self, stage: Stage, completed: u64, total: Option<u64>) {
        if let Some(id) = self.id {
            let page = serde_json::json!(self.page.as_str());
            let args = serde_json::json!([id, stage, completed, total]);
            let _ = self.webview.eval(format!(
                "if (location.href === {page}) window.wingmateInstallProgress?.(...{args})"
            ));
        }
    }

    fn meter(&self, stage: Stage, total: u64) -> impl FnMut(u64) + '_ {
        let mut last = Instant::now();
        self.publish(stage, 0, Some(total));
        move |completed| {
            let now = Instant::now();
            if completed == total || now.duration_since(last) >= Duration::from_millis(200) {
                self.publish(stage, completed, Some(total));
                last = now;
            }
        }
    }
}

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

async fn cached(path: &Path, offer: &Offer) -> Result<Option<File>> {
    let mut file = match File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if file.metadata().await?.len() != offer.size {
        return Ok(None);
    }
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 128 * 1024];
    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            if format!("{:x}", hash.finalize()) != offer.sha256 {
                return Ok(None);
            }
            file.seek(SeekFrom::Start(0)).await?;
            return Ok(Some(file));
        }
        hash.update(&buffer[..count]);
    }
}

fn clean_old_downloads(cache: &Path) {
    let previous = cache.join("remote");
    if !previous
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir())
    {
        return;
    }
    if let Ok(entries) = previous.read_dir() {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_str().is_some_and(|name| {
                name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit())
            }) && entry.file_type().is_ok_and(|kind| kind.is_file())
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let _ = fs::remove_dir(previous);
}

pub(super) struct Download {
    file: File,
    // Windows 不能原子替换仍打开的发行文件，传输结束后再释放缓存锁。
    _lock: fs::File,
}

pub(super) async fn download(
    home: &Path,
    platform: &str,
    approved: &Offer,
    progress: &Progress<'_>,
) -> Result<Download> {
    let cache = home.join("downloads");
    tokio::fs::create_dir_all(&cache).await?;
    let lock = cache.join(".wingmate-download.lock");
    let lock = tokio::task::spawn_blocking(move || -> Result<_> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock)?;
        fs2::FileExt::lock_exclusive(&file)?;
        Ok(file)
    })
    .await??;
    // 复核桌面呈现给用户的版本，禁止从前端输入下载地址或更换确认过的资产。
    let offer = latest(platform).await?;
    ensure!(&offer == approved, "官方发行已更新，请重新检查版本后确认");
    let path = cache.join(NAME);
    if let Some(file) = cached(&path, &offer).await? {
        clean_old_downloads(&cache);
        return Ok(Download { file, _lock: lock });
    }
    let temp = tempfile::NamedTempFile::new_in(&cache)?;
    let mut file = File::from_std(temp.reopen()?);
    let mut report = progress.meter(Stage::Download, offer.size);
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
        report(size);
    }
    ensure!(
        size == offer.size && format!("{:x}", hash.finalize()) == offer.sha256,
        "发行包的大小或 SHA-256 校验失败"
    );
    file.sync_all().await?;
    drop(file);
    let mut file = File::from_std(temp.persist(&path)?);
    file.seek(SeekFrom::Start(0)).await?;
    clean_old_downloads(&cache);
    Ok(Download { file, _lock: lock })
}

pub(super) async fn transfer(
    mut download: Download,
    output: &mut (impl AsyncWrite + Unpin),
    total: u64,
    progress: &Progress<'_>,
) -> Result<()> {
    let mut buffer = vec![0; 128 * 1024];
    let mut sent = 0;
    let mut report = progress.meter(Stage::Transfer, total);
    loop {
        let count = download.file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        let next = sent + count as u64;
        ensure!(next <= total, "本机发行包大小已变化");
        output.write_all(&buffer[..count]).await?;
        sent = next;
        report(sent);
    }
    ensure!(sent == total, "本机发行包大小已变化");
    output.flush().await?;
    Ok(())
}

// 临时文件与正式文件在同一个目录，校验通过后原子替换。
pub(super) fn publish_script(hash: &str) -> String {
    let hash = super::remote::shell_quote(hash);
    format!(
        r#"
        expected={hash}
        actual=$(sha256sum < "$staged") || exit
        case "$actual" in
            "$expected "*) ;;
            *) echo '发行包 SHA-256 校验失败' >&2; exit 45 ;;
        esac
        chmod 700 "$staged" || exit
        (
            flock -x 9 || exit
            launcher="$base/{NAME}"
            mv -fT -- "$staged" "$launcher" || exit
            "$launcher" install >/dev/null || exit
            legacy="$HOME/.wingmate/remote-install"
            if [ -d "$legacy" ] && [ ! -L "$legacy" ] &&
                [ -f "$legacy/wingmate" ] && [ -x "$legacy/wingmate" ] &&
                [ ! -L "$legacy/wingmate" ]; then
                rm -f -- "$legacy/wingmate" && rmdir -- "$legacy" 2>/dev/null || :
            fi
        ) 9>"$base/.wingmate-install.lock"
    "#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn verified_cache_is_rewound_and_revalidates_replaced_content() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join(NAME);
        let contents = b"previous verified package";
        fs::write(&path, contents)?;
        let offer = Offer {
            tag: "v0.4.2".into(),
            size: contents.len() as u64,
            sha256: format!("{:x}", Sha256::digest(contents)),
        };
        let mut previous = cached(&path, &offer).await?.unwrap();
        let mut read = Vec::new();
        previous.read_to_end(&mut read).await?;
        assert_eq!(read, contents);
        drop(previous);
        let replacement = tempfile::NamedTempFile::new_in(directory.path())?;
        fs::write(replacement.path(), b"new package")?;
        drop(replacement.persist(&path)?);
        assert!(cached(&path, &offer).await?.is_none());
        Ok(())
    }

    #[test]
    fn old_cache_cleanup_only_removes_completed_downloads() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let legacy = directory.path().join("remote");
        fs::create_dir(&legacy)?;
        let completed = legacy.join("a".repeat(64));
        let unrelated = legacy.join("notes");
        fs::write(&completed, [])?;
        fs::write(&unrelated, [])?;
        clean_old_downloads(directory.path());
        assert!(!completed.exists());
        assert!(unrelated.is_file());
        Ok(())
    }

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
