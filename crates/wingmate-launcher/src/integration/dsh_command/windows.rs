use super::{registration_lock, CommandFile, Status, OLD_DSH_OWNER, OLD_WINGMATE_OWNER};
use anyhow::{Context, Result};
use std::{io, path::Path};
use winreg::{
    enums::{HKEY_CURRENT_USER, REG_EXPAND_SZ, REG_SZ},
    types::{FromRegValue, ToRegValue},
    RegKey,
};

const OLD_DSH_CMD: &str = "@setlocal DisableDelayedExpansion\r\n@node \"%~dp0dsh.cjs\" %*\r\n";
const OLD_WINGMATE_CMD: &str =
    "@setlocal DisableDelayedExpansion\r\n@node \"%~dp0wingmate.cjs\" %*\r\n";
const OLD_WINGMATE_DISPATCH: &str =
    "@setlocal DisableDelayedExpansion\r\n@node \"%~dp0..\\wingmate.cjs\" %*\r\n";
const DSH_CMD: &str = concat!(
    "@setlocal DisableDelayedExpansion\r\n",
    "@\"%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe\" ",
    "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"%~dp0internal\\launch.ps1\" dsh %*\r\n"
);
const WINGMATE_CMD: &str =
    "@setlocal DisableDelayedExpansion\r\n@\"%~dp0internal\\wingmate.cmd\" %*\r\n";
// Chain without CALL so cmd.exe can finish even when uninstall removes the public entry point.
const WINGMATE_DISPATCH: &str = concat!(
    "@setlocal DisableDelayedExpansion\r\n",
    "@\"%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe\" ",
    "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"%~dp0launch.ps1\" wingmate %*\r\n"
);
const LAUNCH: &str = include_str!("windows_launch.ps1");
pub(super) const POWERSHELL_PATH: &str = concat!(
    r"& { $bin = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Wingmate\bin'; ",
    r"$rest = @(); if ($env:Path) { $rest = @($env:Path -split ';' | Where-Object { $_.TrimEnd([char[]]'\/') -ne $bin }) }; ",
    r"$env:Path = (@($bin) + $rest) -join ';' }",
);

pub(super) fn configure(
    enabled: Option<bool>,
    dsh_launcher: Option<impl FnOnce() -> Result<String>>,
    wingmate: &str,
) -> Result<Status> {
    let bin = dirs::data_local_dir()
        .context("无法确定本机应用数据目录")?
        .join("Wingmate")
        .join("bin");
    let user = RegKey::predef(HKEY_CURRENT_USER);
    let (status, path_changed) = configure_in(&user, &bin, enabled, dsh_launcher, wingmate)?;
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
    dsh_launcher: Option<impl FnOnce() -> Result<String>>,
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
    let open_config = |name| {
        CommandFile::open(bin, &format!("internal/{name}.json"), |text| {
            serde_json::from_str::<serde_json::Value>(text)
                .is_ok_and(|value| value["wingmate_command"] == name)
        })
    };
    let open_dsh = || -> Result<_> {
        Ok((
            CommandFile::open(bin, "dsh.cmd", |text| {
                text == DSH_CMD || text == OLD_DSH_CMD
            })?,
            open_config("dsh")?,
            CommandFile::open(bin, "dsh.cjs", |text| text.starts_with(OLD_DSH_OWNER))?,
        ))
    };
    let dsh = dsh_launcher
        .map(|launcher| -> Result<_> {
            let (cmd, config, old) = open_dsh()?;
            Ok((cmd, config, old, launcher))
        })
        .transpose()?;
    let wingmate_cmd = CommandFile::open(bin, "wingmate.cmd", |text| {
        text == WINGMATE_CMD || text == OLD_WINGMATE_CMD
    })?;
    let wingmate_config = open_config("wingmate")?;
    let old_wingmate = CommandFile::open(bin, "wingmate.cjs", |text| {
        text.starts_with(OLD_WINGMATE_OWNER)
    })?;
    let open_dispatch = || {
        CommandFile::open(bin, "internal/wingmate.cmd", |text| {
            text == WINGMATE_DISPATCH || text == OLD_WINGMATE_DISPATCH
        })
    };
    let open_launch = || {
        CommandFile::open(bin, "internal/launch.ps1", |text| {
            text.starts_with("# Wingmate command dispatcher")
        })
    };

    let without_launcher = || {
        path.split(';')
            .filter(|part| !is_ours(part))
            .collect::<Vec<_>>()
            .join(";")
    };
    let keep_path = if enabled == Some(false) && dsh.is_none() {
        let (cmd, config, old) = open_dsh()?;
        cmd.current.is_some() || config.current.is_some() || old.current.is_some()
    } else {
        false
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
        Some(false) if registered && !keep_path => Some(without_launcher()),
        _ => None,
    };
    if enabled == Some(true) {
        let dispatch = open_dispatch()?;
        let launch = open_launch()?;
        for file in [&wingmate_cmd, &wingmate_config, &dispatch, &launch] {
            file.ensure_available()?;
        }
        std::fs::create_dir_all(bin.join("internal"))?;
        let dsh = if let Some((cmd, config, old, launcher)) = dsh {
            cmd.ensure_available()?;
            config.ensure_available()?;
            Some((cmd, config, old, launcher()?))
        } else {
            None
        };
        if launch.current.as_deref() != Some(LAUNCH) {
            launch.install(LAUNCH)?;
        }
        if let Some((cmd, config, old, content)) = dsh {
            for (file, content) in [(&config, content.as_str()), (&cmd, DSH_CMD)] {
                if file.current.as_deref() != Some(content) {
                    file.install(content)?;
                }
            }
            old.remove()?;
        }
        for (file, content) in [
            (&dispatch, WINGMATE_DISPATCH),
            (&wingmate_config, wingmate),
            (&wingmate_cmd, WINGMATE_CMD),
        ] {
            if file.current.as_deref() != Some(content) {
                file.install(content)?;
            }
        }
        old_wingmate.remove()?;
    } else if enabled == Some(false) {
        for file in [wingmate_cmd, wingmate_config, old_wingmate] {
            file.remove()?;
        }
        if let Some((cmd, config, old, _)) = dsh {
            cmd.remove()?;
            config.remove()?;
            old.remove()?;
        }
    } else {
        let needs_update = registered
            && (!first
                || dsh.is_some_and(|(cmd, config, _, launcher)| {
                    cmd.current.as_deref() != Some(DSH_CMD)
                        || config.current.is_none()
                        || launcher().ok() != config.current
                })
                || wingmate_cmd.current.as_deref() != Some(WINGMATE_CMD)
                || open_dispatch()?.current.as_deref() != Some(WINGMATE_DISPATCH)
                || open_launch()?.current.as_deref() != Some(LAUNCH)
                || wingmate_config.current.as_deref() != Some(wingmate));
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
    use crate::integration::dsh_command::{dsh_launcher, wingmate_launcher};
    use std::fs;

    #[test]
    fn powershell_refresh_preserves_session_path_and_is_idempotent() -> Result<()> {
        let update = super::super::shell_path_update("powershell")?.replace('\'', "''");
        let script = format!(
            r"
$bin = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Wingmate\bin'
$env:Path = 'C:\Session Tools;;' + $bin.ToLowerInvariant() + '\;C:\Other'
'{update}' | Invoke-Expression
'{update}' | Invoke-Expression
if ($env:Path -cne ($bin + ';C:\Session Tools;;C:\Other')) {{ throw 'Unexpected PATH after refresh' }}
Remove-Item Env:Path
'{update}' | Invoke-Expression
if ($env:Path -cne $bin) {{ throw 'Unexpected PATH from an empty session' }}
"
        );
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

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
                r#"console.log(JSON.stringify([process.argv.slice(2), process.env.DSH_HOME, process.env.DSH_AGENTS_HOME, process.cwd()])); process.exitCode = 7;"#,
            )?;
            let node = crate::runtime::node()?;
            let tools = directory.path().join("toolchains");
            let script = || dsh_launcher(&node, &entry, &dsh_home, &agents_home, &tools);
            let fixture = std::env::current_exe()?;
            let wingmate = wingmate_launcher(&fixture, directory.path())?;
            let configure = |enabled| configure_in(&test, &bin, enabled, Some(script), &wingmate);
            let install = || {
                configure_in(
                    &test,
                    &bin,
                    Some(true),
                    None::<fn() -> Result<String>>,
                    &wingmate,
                )
            };
            let uninstall =
                || configure_in(&test, &bin, Some(false), None::<fn() -> Result<String>>, "");
            let path = r"%USERPROFILE%\bin;C:\Tools";
            let (environment, _) = test.create_subkey("Environment")?;
            let mut original = path.to_reg_value();
            original.vtype = REG_EXPAND_SZ;
            environment.set_raw_value("Path", &original)?;

            assert!(!configure(None)?.0.enabled);
            let (status, changed) = install()?;
            assert!(status.enabled && changed);
            let wingmate_config = bin.join("internal/wingmate.json");
            let dsh_config = bin.join("internal/dsh.json");
            assert!(!bin.join("dsh.cmd").exists() && !dsh_config.exists());
            assert!(bin.join("wingmate.cmd").exists() && wingmate_config.exists());
            assert!(!install()?.1);
            assert!(uninstall()?.1);
            assert!(!uninstall()?.1);
            assert!(!bin.join("wingmate.cmd").exists() && !wingmate_config.exists());
            assert_eq!(environment.get_raw_value("Path")?, original);
            install()?;
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
            fs::remove_file(&wingmate_config)?;
            assert!(configure(None)?.0.needs_update);
            assert!(!configure(Some(true))?.1);
            assert!(!configure(None)?.0.needs_update);
            fs::write(bin.join("wingmate.cmd"), OLD_WINGMATE_CMD)?;
            fs::write(bin.join("wingmate.cjs"), OLD_WINGMATE_OWNER)?;
            fs::write(bin.join("internal/wingmate.cmd"), OLD_WINGMATE_DISPATCH)?;
            assert!(configure(None)?.0.needs_update);
            assert!(!install()?.1);
            assert_eq!(fs::read_to_string(bin.join("wingmate.cmd"))?, WINGMATE_CMD);
            assert!(!bin.join("wingmate.cjs").exists());
            assert!(
                configure_in(
                    &test,
                    &bin,
                    None,
                    Some(|| dsh_launcher(
                        &node,
                        &entry,
                        &dsh_home,
                        &directory.path().join("other-agents"),
                        &tools
                    )),
                    &wingmate
                )?
                .0
                .needs_update
            );

            let output = std::process::Command::new(bin.join("dsh.cmd"))
                .current_dir(directory.path())
                .env_remove("PATH")
                .args(["an argument & value", "-label:value", "--flag=value", ""])
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
                    ["an argument & value", "-label:value", "--flag=value", ""],
                    dsh_home,
                    agents_home,
                    directory.path()
                ])
            );
            let output = std::process::Command::new(bin.join("wingmate.cmd"))
                .env_remove("PATH")
                .arg("--list")
                .output()?;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout)
                .contains("integration::dsh_command::windows::tests::registration_round_trip"));

            fs::write(
                bin.join("internal/launch.ps1"),
                "# Wingmate command dispatcher\nif ($args[2] -ne 'argument & ! value') { exit 7 }\nRemove-Item -LiteralPath (Join-Path (Split-Path $PSScriptRoot) 'wingmate.cmd')\n",
            )?;
            let output = std::process::Command::new(bin.join("wingmate.cmd"))
                .arg("uninstall")
                .arg("argument & ! value")
                .output()?;
            assert!(
                output.status.success() && output.stderr.is_empty(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!bin.join("wingmate.cmd").exists());

            let (status, changed) = uninstall()?;
            assert!(!status.enabled && !changed);
            assert!(bin.join("dsh.cmd").exists() && dsh_config.exists());
            assert!(!bin.join("wingmate.cmd").exists() && !wingmate_config.exists());
            assert_eq!(environment.get_raw_value("Path")?, registered);
            assert!(configure(None)?.0.needs_update);

            let (status, changed) = configure(Some(false))?;
            assert!(!status.enabled && changed);
            assert!(!bin.join("dsh.cmd").exists() && !dsh_config.exists());
            assert!(!bin.join("wingmate.cmd").exists() && !wingmate_config.exists());
            assert_eq!(environment.get_raw_value("Path")?, original);

            let script_path = &dsh_config;
            assert!(configure_in(
                &test,
                &bin,
                Some(true),
                Some(|| {
                    fs::write(script_path, "user's script")?;
                    script()
                }),
                &wingmate
            )
            .is_err());
            assert_eq!(fs::read_to_string(script_path)?, "user's script");
            assert!(!bin.join("dsh.cmd").exists());
            assert_eq!(environment.get_raw_value("Path")?, original);
            fs::remove_file(script_path)?;

            configure(Some(true))?;
            fs::remove_file(&dsh_config)?;
            let wingmate_cmd = bin.join("wingmate.cmd");
            fs::write(&wingmate_cmd, [0xff])?;
            let (status, _) = configure(None)?;
            assert!(status.enabled && status.needs_update);
            assert!(configure(Some(true)).is_err());
            assert!(!dsh_config.exists());
            assert!(!configure(Some(false))?.0.enabled);
            assert_eq!(environment.get_raw_value("Path")?, original);
            assert_eq!(fs::read(&wingmate_cmd)?, [0xff]);
            assert!(!wingmate_config.exists());
            fs::remove_file(wingmate_cmd)?;

            fs::write(bin.join("dsh.cmd"), "user's command")?;
            assert!(install()?.0.enabled);
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
