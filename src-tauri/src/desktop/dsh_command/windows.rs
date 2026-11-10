use super::{read, registration_lock, Status, OWNER};
use anyhow::{Context, Result};
use std::{
    fs,
    io::{self, Write},
    path::Path,
};
use winreg::{
    enums::{HKEY_CURRENT_USER, REG_EXPAND_SZ, REG_SZ},
    types::{FromRegValue, ToRegValue},
    RegKey,
};

const CMD: &str = "@setlocal DisableDelayedExpansion\r\n@node \"%~dp0dsh.cjs\" %*\r\n";
pub(super) fn configure(
    enabled: Option<bool>,
    launcher: impl FnOnce() -> Result<String>,
) -> Result<Status> {
    let bin = dirs::data_local_dir()
        .context("无法确定本机应用数据目录")?
        .join("Wingmate")
        .join("bin");
    let user = RegKey::predef(HKEY_CURRENT_USER);
    let (status, path_changed) = configure_in(&user, &bin, enabled, launcher)?;
    if path_changed {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
        };
        let environment: Vec<u16> = "Environment\0".encode_utf16().collect();
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                environment.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                100,
                std::ptr::null_mut(),
            );
        }
    }
    Ok(status)
}

fn configure_in(
    user: &RegKey,
    bin: &Path,
    enabled: Option<bool>,
    launcher: impl FnOnce() -> Result<String>,
) -> Result<(Status, bool)> {
    let bin_text = bin.to_str().context("命令目录路径无法表示")?;
    anyhow::ensure!(!bin_text.contains([';', '%']), "命令目录不能写入 PATH");
    let _lock = registration_lock(bin, enabled)?;

    let environment = user.open_subkey("Environment");
    let (path, kind) = match environment.and_then(|key| key.get_raw_value("Path")) {
        Ok(value) => (String::from_reg_value(&value)?, value.vtype),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (String::new(), REG_EXPAND_SZ),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        matches!(kind, REG_SZ | REG_EXPAND_SZ),
        "用户 PATH 类型不受支持"
    );
    let is_ours = |part: &str| {
        part.trim_end_matches(['\\', '/'])
            .eq_ignore_ascii_case(bin_text)
    };
    let registered = path.split(';').any(is_ours);
    let first = path.split(';').next().is_some_and(is_ours);
    let cmd = bin.join("dsh.cmd");
    let script = bin.join("dsh.cjs");
    let cmd_content = read(&cmd)?;
    let script_content = read(&script)?;
    anyhow::ensure!(
        cmd_content.as_deref().is_none_or(|text| text == CMD)
            && script_content
                .as_deref()
                .is_none_or(|text| text.starts_with(OWNER)),
        "命令目录已有同名文件：{bin_text}"
    );

    let without_launcher = || {
        path.split(';')
            .filter(|part| !is_ours(part))
            .collect::<Vec<_>>()
            .join(";")
    };
    let next_path = match enabled {
        Some(true) if !first => {
            let rest = without_launcher();
            Some(if rest.is_empty() {
                bin_text.to_owned()
            } else {
                format!("{bin_text};{rest}")
            })
        }
        Some(false) if registered => Some(without_launcher()),
        _ => None,
    };
    if enabled == Some(true) {
        let content = launcher()?;
        let mut temporary = tempfile::NamedTempFile::new_in(bin)?;
        temporary.write_all(content.as_bytes())?;
        if script_content.is_some() {
            temporary.persist(&script)?;
        } else {
            temporary.persist_noclobber(&script)?;
        }
        if cmd_content.is_none() {
            let mut temporary = tempfile::NamedTempFile::new_in(bin)?;
            temporary.write_all(CMD.as_bytes())?;
            temporary.persist_noclobber(&cmd)?;
        }
    } else if enabled == Some(false) {
        if cmd_content.is_some() {
            fs::remove_file(&cmd)?;
        }
        if script_content.is_some() {
            fs::remove_file(&script)?;
        }
    } else {
        let enabled = registered && cmd_content.is_some() && script_content.is_some();
        let needs_update = enabled && (!first || launcher().ok() != script_content);
        return Ok((
            Status {
                enabled,
                needs_update,
            },
            false,
        ));
    }
    if let Some(path) = &next_path {
        let (key, _) = user.create_subkey("Environment")?;
        let mut value = path.to_reg_value();
        value.vtype = kind;
        key.set_raw_value("Path", &value)?;
    }
    Ok((
        Status {
            enabled: enabled == Some(true),
            needs_update: false,
        },
        next_path.is_some(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::dsh_command::launcher;

    #[test]
    fn registration_round_trip_preserves_user_path_and_forwards_arguments() -> Result<()> {
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let key = format!(r"Software\Wingmate.CliTest.{}", std::process::id());
        let (test, _) = user.create_subkey(&key)?;
        let result = (|| -> Result<()> {
            let directory = tempfile::tempdir()?;
            let bin = directory.path().join("测试命令 ! 文件夹");
            let entry = directory.path().join("dsh entry.cjs");
            let dsh_home = directory.path().join("DSH 配置");
            let agents_home = directory.path().join("Agent 配置");
            fs::write(
                &entry,
                r#"console.log(JSON.stringify([process.argv[2], process.env.DSH_HOME, process.env.DSH_AGENTS_HOME, process.cwd()])); process.exitCode = 7;"#,
            )?;
            let script = || launcher(&entry, &dsh_home, &agents_home);
            let path = r"%USERPROFILE%\bin;C:\Tools";
            let (environment, _) = test.create_subkey("Environment")?;
            let mut original = path.to_reg_value();
            original.vtype = REG_EXPAND_SZ;
            environment.set_raw_value("Path", &original)?;

            assert!(!configure_in(&test, &bin, None, script)?.0.enabled);
            let (status, changed) = configure_in(&test, &bin, Some(true), script)?;
            assert!(status.enabled && changed);
            let (status, changed) = configure_in(&test, &bin, Some(true), script)?;
            assert!(status.enabled && !changed);
            let registered = environment.get_raw_value("Path")?;
            assert_eq!(registered.vtype, REG_EXPAND_SZ);
            assert_eq!(
                String::from_reg_value(&registered)?,
                format!("{};{path}", bin.display())
            );
            let old_path = format!("{path};{}", bin.display());
            let mut appended = old_path.to_reg_value();
            appended.vtype = REG_EXPAND_SZ;
            environment.set_raw_value("Path", &appended)?;
            assert!(configure_in(&test, &bin, None, script)?.0.needs_update);
            assert!(configure_in(&test, &bin, Some(true), script)?.1);
            assert_eq!(environment.get_raw_value("Path")?, registered);
            assert!(
                configure_in(&test, &bin, None, || {
                    launcher(&entry, &dsh_home, &directory.path().join("other-agents"))
                })?
                .0
                .needs_update
            );

            let output = std::process::Command::new(bin.join("dsh.cmd"))
                .current_dir(directory.path())
                .arg("an argument & value")
                .output()?;
            assert_eq!(
                output.status.code(),
                Some(7),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&output.stdout)?,
                serde_json::json!([
                    "an argument & value",
                    dsh_home,
                    agents_home,
                    directory.path()
                ])
            );

            let (status, changed) = configure_in(&test, &bin, Some(false), script)?;
            assert!(!status.enabled && changed);
            assert!(!bin.join("dsh.cmd").exists() && !bin.join("dsh.cjs").exists());
            assert_eq!(environment.get_raw_value("Path")?, original);

            let script_path = bin.join("dsh.cjs");
            assert!(configure_in(&test, &bin, Some(true), || {
                fs::write(&script_path, "user's script")?;
                script()
            })
            .is_err());
            assert_eq!(fs::read_to_string(&script_path)?, "user's script");
            assert!(!bin.join("dsh.cmd").exists());
            assert_eq!(environment.get_raw_value("Path")?, original);
            fs::remove_file(script_path)?;

            fs::write(bin.join("dsh.cmd"), "user's command")?;
            assert!(configure_in(&test, &bin, Some(true), script).is_err());
            assert!(configure_in(&test, &bin, Some(false), script).is_err());
            assert_eq!(fs::read_to_string(bin.join("dsh.cmd"))?, "user's command");
            assert_eq!(environment.get_raw_value("Path")?, original);
            Ok(())
        })();
        drop(test);
        user.delete_subkey_all(key)?;
        result
    }
}
