use super::{registration_lock, CommandFile, Status, OWNER, WINGMATE_OWNER};
use anyhow::{Context, Result};
use std::{io, path::Path};
use winreg::{
    enums::{HKEY_CURRENT_USER, REG_EXPAND_SZ, REG_SZ},
    types::{FromRegValue, ToRegValue},
    RegKey,
};

const CMD: &str = "@setlocal DisableDelayedExpansion\r\n@node \"%~dp0dsh.cjs\" %*\r\n";
const WINGMATE_CMD: &str =
    "@setlocal DisableDelayedExpansion\r\n@node \"%~dp0wingmate.cjs\" %*\r\n";

pub(super) fn configure(
    enabled: Option<bool>,
    launcher: impl FnOnce() -> Result<String>,
    wingmate: &str,
) -> Result<Status> {
    let bin = dirs::data_local_dir()
        .context("无法确定本机应用数据目录")?
        .join("Wingmate")
        .join("bin");
    let user = RegKey::predef(HKEY_CURRENT_USER);
    let (status, path_changed) = configure_in(&user, &bin, enabled, launcher, wingmate)?;
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
    wingmate: &str,
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
    let cmd = CommandFile::open(bin, "dsh.cmd", |text| text == CMD)?;
    let script = CommandFile::open(bin, "dsh.cjs", |text| text.starts_with(OWNER))?;
    let wingmate_cmd = CommandFile::open(bin, "wingmate.cmd", |text| text == WINGMATE_CMD)?;
    let wingmate_script =
        CommandFile::open(bin, "wingmate.cjs", |text| text.starts_with(WINGMATE_OWNER))?;

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
        for file in [&cmd, &script, &wingmate_cmd, &wingmate_script] {
            file.ensure_available()?;
        }
        let content = launcher()?;
        for (file, content) in [
            (&script, content.as_str()),
            (&cmd, CMD),
            (&wingmate_script, wingmate),
            (&wingmate_cmd, WINGMATE_CMD),
        ] {
            if file.current.as_deref() != Some(content) {
                file.install(content)?;
            }
        }
    } else if enabled == Some(false) {
        for file in [wingmate_cmd, wingmate_script, cmd, script] {
            file.remove()?;
        }
    } else {
        let needs_update = registered
            && (!first
                || cmd.current.is_none()
                || script.current.is_none()
                || launcher().ok() != script.current
                || wingmate_cmd.current.is_none()
                || wingmate_script.current.as_deref() != Some(wingmate));
        return Ok((
            Status {
                enabled: registered,
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
    use crate::integration::dsh_command::{launcher, wingmate_launcher};
    use std::fs;

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
            let fixture = std::env::current_exe()?;
            let wingmate = wingmate_launcher(&fixture, directory.path())?;
            let configure = |enabled| configure_in(&test, &bin, enabled, script, &wingmate);
            let path = r"%USERPROFILE%\bin;C:\Tools";
            let (environment, _) = test.create_subkey("Environment")?;
            let mut original = path.to_reg_value();
            original.vtype = REG_EXPAND_SZ;
            environment.set_raw_value("Path", &original)?;

            assert!(!configure(None)?.0.enabled);
            let (status, changed) = configure(Some(true))?;
            assert!(status.enabled && changed);
            let (status, changed) = configure(Some(true))?;
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
            assert!(configure(None)?.0.needs_update);
            assert!(configure(Some(true))?.1);
            assert_eq!(environment.get_raw_value("Path")?, registered);
            fs::remove_file(bin.join("wingmate.cmd"))?;
            fs::remove_file(bin.join("wingmate.cjs"))?;
            assert!(configure(None)?.0.needs_update);
            assert!(!configure(Some(true))?.1);
            assert!(!configure(None)?.0.needs_update);
            assert!(
                configure_in(
                    &test,
                    &bin,
                    None,
                    || { launcher(&entry, &dsh_home, &directory.path().join("other-agents")) },
                    &wingmate
                )?
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
            let output = std::process::Command::new(bin.join("wingmate.cmd"))
                .arg("--list")
                .output()?;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout)
                .contains("integration::dsh_command::windows::tests::registration_round_trip"));

            let (status, changed) = configure(Some(false))?;
            assert!(!status.enabled && changed);
            assert!(!bin.join("dsh.cmd").exists() && !bin.join("dsh.cjs").exists());
            assert!(!bin.join("wingmate.cmd").exists() && !bin.join("wingmate.cjs").exists());
            assert_eq!(environment.get_raw_value("Path")?, original);

            let script_path = bin.join("dsh.cjs");
            assert!(configure_in(
                &test,
                &bin,
                Some(true),
                || {
                    fs::write(&script_path, "user's script")?;
                    script()
                },
                &wingmate
            )
            .is_err());
            assert_eq!(fs::read_to_string(&script_path)?, "user's script");
            assert!(!bin.join("dsh.cmd").exists());
            assert_eq!(environment.get_raw_value("Path")?, original);
            fs::remove_file(script_path)?;

            configure(Some(true))?;
            fs::remove_file(bin.join("dsh.cjs"))?;
            let wingmate_cmd = bin.join("wingmate.cmd");
            fs::write(&wingmate_cmd, [0xff])?;
            let (status, _) = configure(None)?;
            assert!(status.enabled && status.needs_update);
            assert!(configure(Some(true)).is_err());
            assert!(!bin.join("dsh.cjs").exists());
            assert!(!configure(Some(false))?.0.enabled);
            assert_eq!(environment.get_raw_value("Path")?, original);
            assert_eq!(fs::read(&wingmate_cmd)?, [0xff]);
            assert!(!bin.join("wingmate.cjs").exists());
            fs::remove_file(wingmate_cmd)?;

            fs::write(bin.join("dsh.cmd"), "user's command")?;
            assert!(configure(Some(true)).is_err());
            assert!(!configure(Some(false))?.0.enabled);
            assert_eq!(fs::read_to_string(bin.join("dsh.cmd"))?, "user's command");
            assert_eq!(environment.get_raw_value("Path")?, original);
            Ok(())
        })();
        drop(test);
        user.delete_subkey_all(key)?;
        result
    }
}
