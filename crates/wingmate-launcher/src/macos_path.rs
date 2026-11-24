use std::{
    env,
    ffi::OsString,
    fs,
    os::unix::{ffi::OsStringExt, process::CommandExt},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) fn from_login_shell() -> Option<OsString> {
    let shell = env::var_os("SHELL")
        .filter(|shell| Path::new(shell).is_absolute())
        .unwrap_or_else(|| "/bin/zsh".into());
    read_path(&mut Command::new(shell)).ok()
}

fn read_path(shell: &mut Command) -> anyhow::Result<OsString> {
    let directory = tempfile::tempdir()?;
    let output = directory.path().join("path");
    // An interactive login shell reads both login and interactive configuration.
    // Capture only the exported PATH, regardless of what those files print.
    let mut child = shell
        .args([
            "-ilc",
            r#"exec /usr/bin/printenv PATH > "$WINGMATE_SHELL_PATH_FILE""#,
        ])
        .env("WINGMATE_SHELL_PATH_FILE", &output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait()? {
            anyhow::ensure!(status.success(), "登录 Shell 未返回 PATH");
            let mut path = fs::read(&output)?;
            if path.last() == Some(&b'\n') {
                path.pop();
            }
            anyhow::ensure!(!path.is_empty(), "登录 Shell 的 PATH 为空");
            return Ok(OsString::from_vec(path));
        }
        thread::sleep(Duration::from_millis(25));
    }
    // Terminate any foreground command that the startup files were waiting for.
    unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
    let _ = child.wait();
    anyhow::bail!("读取登录 Shell 的 PATH 超时");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_path_with_noisy_startup_and_noclobber_enabled() -> anyhow::Result<()> {
        let home = tempfile::tempdir()?;
        fs::write(
            home.path().join(".zshrc"),
            "setopt noclobber\nprintf 'shell startup output\\n'\nexport PATH='/example/Node Tools:/usr/bin:/bin'\n",
        )?;
        let path = read_path(Command::new("/bin/zsh").env("ZDOTDIR", home.path()))?;
        assert_eq!(path, "/example/Node Tools:/usr/bin:/bin");
        Ok(())
    }
}
