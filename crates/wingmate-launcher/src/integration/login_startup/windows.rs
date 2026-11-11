use super::{optional, Status};
use std::{ffi::OsString, io, os::windows::ffi::OsStrExt, path::Path};
use winreg::{
    enums::{HKEY_CURRENT_USER, KEY_SET_VALUE, REG_BINARY},
    types::FromRegValue,
    RegKey,
};

const NAME: &str = "Wingmate";
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVAL: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

fn read<T: FromRegValue>(user: &RegKey, path: &str) -> io::Result<Option<T>> {
    optional(user.open_subkey(path).and_then(|key| key.get_value(NAME)))
}

fn remove(user: &RegKey, path: &str) -> io::Result<()> {
    optional(
        user.open_subkey_with_flags(path, KEY_SET_VALUE)
            .and_then(|key| key.delete_value(NAME)),
    )?;
    Ok(())
}

pub(super) fn configure(executable: &Path, enabled: Option<bool>) -> anyhow::Result<Status> {
    configure_in(&RegKey::predef(HKEY_CURRENT_USER), executable, enabled)
}

fn configure_in(user: &RegKey, executable: &Path, enabled: Option<bool>) -> anyhow::Result<Status> {
    let mut command = OsString::from("\"");
    command.push(executable);
    command.push("\" start");
    match enabled {
        Some(true) => {
            anyhow::ensure!(
                command.encode_wide().count() <= 260,
                "Wingmate 的路径太长，无法注册登录启动"
            );
            let (key, _) = user.create_subkey(RUN)?;
            key.set_value(NAME, &command)?;
            // A new explicit opt-in also clears a prior disable from Windows Startup Apps.
            remove(user, APPROVAL)?;
        }
        Some(false) => {
            remove(user, RUN)?;
            remove(user, APPROVAL)?;
        }
        None => {}
    }
    let registered: Option<OsString> = read(user, RUN)?;
    let approved = optional(
        user.open_subkey(APPROVAL)
            .and_then(|key| key.get_raw_value(NAME)),
    )?;
    // StartupApproved stores the state first; the trailing timestamp is not the state.
    let disabled = approved.is_some_and(|value| {
        value.vtype == REG_BINARY && matches!(value.bytes.first(), Some(3 | 7))
    });
    Ok(Status {
        registered: registered.is_some(),
        enabled: registered.is_some() && !disabled,
        needs_update: registered.is_some_and(|registered| registered != command),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::ensure;
    use winreg::RegValue;

    #[test]
    fn login_startup_round_trip_in_an_isolated_registry_key() -> anyhow::Result<()> {
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let path = format!(r"Software\Wingmate.Test.{}", std::process::id());
        let (root, _) = user.create_subkey(&path)?;
        let result = (|| {
            let original = Path::new(r"C:\Some Folder\Wingmate.exe");
            let moved = Path::new(r"D:\Another Folder\Wingmate.exe");
            ensure!(!configure_in(&root, original, None)?.registered);
            ensure!(configure_in(&root, original, Some(true))?.enabled);
            let command: Option<String> = read(&root, RUN)?;
            ensure!(command.as_deref() == Some(r#""C:\Some Folder\Wingmate.exe" start"#));
            ensure!(configure_in(&root, moved, None)?.needs_update);
            let (approval, _) = root.create_subkey(APPROVAL)?;
            approval.set_raw_value(
                NAME,
                &RegValue {
                    bytes: vec![3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0].into(),
                    vtype: REG_BINARY,
                },
            )?;
            ensure!(!configure_in(&root, moved, None)?.enabled);
            let repaired = configure_in(&root, moved, Some(true))?;
            ensure!(repaired.enabled && !repaired.needs_update);
            ensure!(!configure_in(&root, moved, Some(false))?.registered);
            ensure!(!configure_in(&root, moved, Some(false))?.registered);
            Ok(())
        })();
        drop(root);
        user.delete_subkey_all(path)?;
        result
    }
}
