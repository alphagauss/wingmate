//! 运行中的窗口只登记左上角坐标，存活由进程持有的文件锁决定。
use anyhow::Context;
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

pub(super) type Position = [i32; 2];

fn open_lock(path: &Path, create: bool) -> io::Result<File> {
    OpenOptions::new()
        .create(create)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
}

pub(super) fn try_lock(path: &Path, create: bool) -> io::Result<Option<File>> {
    let file = open_lock(path, create)?;
    match FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(Some(file)),
        Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(super) struct Layout {
    directory: PathBuf,
    // 选位和初始登记串行执行，已有窗口仍可更新各自的坐标。
    _lock: File,
}

impl Layout {
    pub(super) fn begin(home: &Path) -> io::Result<Self> {
        let directory = home.join("windows");
        fs::create_dir_all(&directory)?;
        let lock = open_lock(&directory.join("placement.lock"), true)?;
        FileExt::lock_exclusive(&lock)?;
        Ok(Self {
            directory,
            _lock: lock,
        })
    }

    pub(super) fn occupied(&self) -> io::Result<Vec<Position>> {
        let mut positions = Vec::new();
        for entry in fs::read_dir(&self.directory)?.flatten() {
            let lock_path = entry.path();
            if lock_path.extension().and_then(|ext| ext.to_str()) != Some("lock")
                || !lock_path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| stem.parse::<u32>().is_ok())
            {
                continue;
            }
            let path = lock_path.with_extension("json");
            match try_lock(&lock_path, false) {
                Ok(None) => {
                    if let Some(position) = fs::read(path)
                        .ok()
                        .and_then(|data| serde_json::from_slice::<Option<Position>>(&data).ok())
                        .flatten()
                    {
                        positions.push(position);
                    }
                }
                Ok(Some(lock)) => {
                    remove_position(&path);
                    drop(lock);
                    let _ = fs::remove_file(lock_path);
                }
                Err(_) => continue,
            }
        }
        Ok(positions)
    }

    pub(super) fn register(
        self,
        pid: u32,
        position: Option<Position>,
    ) -> anyhow::Result<LiveWindow> {
        let lock_path = self.directory.join(format!("{pid}.lock"));
        let lock = try_lock(&lock_path, true)?.context("窗口登记已被占用")?;
        let mut window = LiveWindow {
            path: lock_path.with_extension("json"),
            lock: Some(lock),
            position: None,
        };
        window.update(position)?;
        Ok(window)
    }
}

pub(super) struct LiveWindow {
    path: PathBuf,
    lock: Option<File>,
    position: Option<Position>,
}

impl LiveWindow {
    pub(super) fn update(&mut self, position: Option<Position>) -> anyhow::Result<()> {
        if self.position != position {
            // 临时文件跟随 PID 命名，写入中断后也能随失效登记一起清理。
            let pending = self.path.with_extension("tmp");
            fs::write(&pending, serde_json::to_vec(&position)?)?;
            fs::rename(pending, &self.path)?;
            self.position = position;
        }
        Ok(())
    }
}

impl Drop for LiveWindow {
    fn drop(&mut self) {
        remove_position(&self.path);
        self.lock.take();
        let _ = fs::remove_file(self.path.with_extension("lock"));
    }
}

fn remove_position(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("tmp"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_positions_update_and_disappear_on_close() -> anyhow::Result<()> {
        let home = tempfile::tempdir()?;
        let mut window = Layout::begin(home.path())?.register(1, Some([10, 20]))?;
        assert_eq!(Layout::begin(home.path())?.occupied()?, vec![[10, 20]]);
        window.update(Some([30, 40]))?;
        assert_eq!(Layout::begin(home.path())?.occupied()?, vec![[30, 40]]);
        window.update(None)?;
        assert!(Layout::begin(home.path())?.occupied()?.is_empty());
        drop(window);
        assert!(!home.path().join("windows/1.lock").exists());
        assert!(!home.path().join("windows/1.json").exists());
        Ok(())
    }

    #[test]
    fn stale_records_are_removed_before_a_pid_is_reused() -> anyhow::Result<()> {
        let home = tempfile::tempdir()?;
        let layout = Layout::begin(home.path())?;
        let stale_lock = layout.directory.join("1.lock");
        let stale_position = layout.directory.join("1.json");
        let stale_pending = layout.directory.join("1.tmp");
        File::create(&stale_lock)?;
        fs::write(&stale_position, b"[10,20]")?;
        fs::write(&stale_pending, b"[30,")?;
        assert!(layout.occupied()?.is_empty());
        assert!(!stale_lock.exists());
        assert!(!stale_position.exists());
        assert!(!stale_pending.exists());
        let _reused = layout.register(1, Some([30, 40]))?;
        assert_eq!(Layout::begin(home.path())?.occupied()?, vec![[30, 40]]);
        Ok(())
    }
}
