use super::{registration_lock, CommandFile, Status, OWNER, WINGMATE_OWNER};
use anyhow::{Context, Result};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

const SHEBANG: &str = "#!/usr/bin/env node\n";
// Keep the old blocks so existing registrations can be updated or removed.
const SH_PATH: [&str; 2] = [
    r#"
# >>> Wingmate dsh >>>
if [ -x "$HOME/.wingmate/bin/dsh" ]; then
  case "${PATH-}:" in
    "$HOME/.wingmate/bin:"*) ;;
    *) export PATH="$HOME/.wingmate/bin${PATH:+:$PATH}" ;;
  esac
fi
# <<< Wingmate dsh <<<
"#,
    r#"
# >>> Wingmate dsh >>>
if [ -x "$HOME/.wingmate/bin/dsh" ]; then
  case ":${PATH-}:" in
    *":$HOME/.wingmate/bin:"*) ;;
    *) export PATH="${PATH:+$PATH:}$HOME/.wingmate/bin" ;;
  esac
fi
# <<< Wingmate dsh <<<
"#,
];
const FISH_PATH: [&str; 2] = [
    r#"
# >>> Wingmate dsh >>>
if test -x "$HOME/.wingmate/bin/dsh"; and test "$PATH[1]" != "$HOME/.wingmate/bin"
  set -gx PATH "$HOME/.wingmate/bin" $PATH
end
# <<< Wingmate dsh <<<
"#,
    r#"
# >>> Wingmate dsh >>>
if test -x "$HOME/.wingmate/bin/dsh"; and not contains -- "$HOME/.wingmate/bin" $PATH
  set -gx PATH $PATH "$HOME/.wingmate/bin"
end
# <<< Wingmate dsh <<<
"#,
];

pub(super) fn configure(
    enabled: Option<bool>,
    launcher: impl FnOnce() -> Result<String>,
    wingmate: &str,
) -> Result<Status> {
    let home = dirs::home_dir().context("无法确定用户主目录")?;
    let config_dir = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let zsh = config_dir("ZDOTDIR").unwrap_or_else(|| home.clone());
    let fish = config_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
    configure_in(&home, &zsh, &fish, enabled, launcher, wingmate)
}

fn configure_in(
    home: &Path,
    zsh: &Path,
    fish: &Path,
    enabled: Option<bool>,
    launcher: impl FnOnce() -> Result<String>,
    wingmate: &str,
) -> Result<Status> {
    let bin = home.join(".wingmate/bin");
    let _lock = registration_lock(&bin, enabled)?;
    let owned = |owner| {
        move |text: &str| {
            text.strip_prefix(SHEBANG)
                .is_some_and(|text| text.starts_with(owner))
        }
    };
    let dsh = CommandFile::open(&bin, "dsh", owned(OWNER))?;
    let wingmate_file = CommandFile::open(&bin, "wingmate", owned(WINGMATE_OWNER))?;
    let wingmate = format!("{SHEBANG}{wingmate}");
    let login_profiles = [".bash_profile", ".bash_login", ".profile"].map(|name| home.join(name));
    let bash_login = login_profiles
        .iter()
        .find(|path| path.is_file())
        .unwrap_or(&login_profiles[2]);
    let bash_rc = home.join(".bashrc");
    let zsh_rc = zsh.join(".zshrc");
    let fish_rc = fish.join("fish/config.fish");
    let profiles = [
        (&bash_rc, SH_PATH),
        (bash_login, SH_PATH),
        (&zsh_rc, SH_PATH),
        (&fish_rc, FISH_PATH),
    ];
    match enabled {
        Some(true) => {
            dsh.ensure_available()?;
            wingmate_file.ensure_available()?;
            let script = format!("{SHEBANG}{}", launcher()?);
            for (path, block) in profiles {
                update_profile(path, block, true)
                    .with_context(|| format!("更新 {}", path.display()))?;
            }
            dsh.install(&script)?;
            wingmate_file.install(&wingmate)?;
            Ok(Status {
                enabled: true,
                needs_update: false,
            })
        }
        Some(false) => {
            for (path, block) in [
                (&bash_rc, SH_PATH),
                (&zsh_rc, SH_PATH),
                (&fish_rc, FISH_PATH),
            ]
            .into_iter()
            .chain(login_profiles.iter().map(|path| (path, SH_PATH)))
            {
                update_profile(path, block, false)
                    .with_context(|| format!("更新 {}", path.display()))?;
            }
            wingmate_file.remove()?;
            dsh.remove()?;
            Ok(Status {
                enabled: false,
                needs_update: false,
            })
        }
        None => {
            let enabled = dsh.current.is_some() || wingmate_file.current.is_some();
            let needs_update = enabled
                && (dsh.current.is_none()
                    || launcher().ok().as_deref()
                        != dsh
                            .current
                            .as_deref()
                            .and_then(|text| text.strip_prefix(SHEBANG))
                    || wingmate_file.current.as_deref() != Some(wingmate.as_str())
                    || [&dsh, &wingmate_file].iter().any(|file| {
                        !fs::metadata(&file.path)
                            .is_ok_and(|metadata| metadata.permissions().mode() & 0o100 != 0)
                    })
                    || profiles.iter().any(|(path, [block, _])| {
                        !fs::read_to_string(path).is_ok_and(|text| text.contains(block))
                    }));
            Ok(Status {
                enabled,
                needs_update,
            })
        }
    }
}

fn update_profile(path: &Path, [block, legacy]: [&str; 2], enabled: bool) -> Result<()> {
    // Dotfiles can be symlinks, so preserve the link and edit its target.
    let (path, permissions, original) = match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {
            let target = path.canonicalize()?;
            let text = fs::read_to_string(&target)?;
            (target, Some(metadata.permissions()), text)
        }
        Ok(_) if !enabled => return Ok(()),
        Ok(_) => anyhow::bail!("shell 配置不是普通文件"),
        Err(error) if error.kind() == io::ErrorKind::NotFound && !enabled => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            (path.to_path_buf(), None, String::new())
        }
        Err(error) => return Err(error.into()),
    };
    let mut content = original.replace(block, "").replace(legacy, "");
    if enabled {
        content.push_str(block);
    }
    if content == original {
        return Ok(());
    }
    let parent = path.parent().context("无法确定 shell 配置目录")?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(content.as_bytes())?;
    if let Some(permissions) = permissions {
        file.as_file().set_permissions(permissions)?;
        anyhow::ensure!(
            fs::read_to_string(&path)? == original,
            "shell 配置已变化，请重试"
        );
        file.persist(path)?;
    } else {
        file.persist_noclobber(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn unix_registration_preserves_profiles_and_adds_executable() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let home = directory.path();
        let zsh = home.join("zsh");
        let fish = home.join("fish-config");
        let bash_target = home.join("bash-config");
        let original = "export CUSTOM=keep";
        fs::write(&bash_target, original)?;
        symlink(&bash_target, home.join(".bashrc"))?;
        symlink(home.join("missing"), home.join(".bash_profile"))?;
        fs::write(home.join(".bash_login"), original)?;
        let launcher =
            || super::super::launcher(&home.join("entry.js"), home, &home.join("agents"));
        let wingmate = super::super::wingmate_launcher(&home.join("desktop"), home)?;
        let configure = |enabled| configure_in(home, &zsh, &fish, enabled, launcher, &wingmate);
        assert!(!configure(None)?.enabled);
        assert!(configure(Some(true))?.enabled);
        assert!(!configure(None)?.needs_update);
        let wingmate_path = home.join(".wingmate/bin/wingmate");
        fs::remove_file(&wingmate_path)?;
        assert!(configure(None)?.needs_update);
        fs::write(&bash_target, format!("{original}{}", SH_PATH[1]))?;
        configure(Some(true))?;
        assert!(!configure(None)?.needs_update);
        assert_eq!(
            fs::read_to_string(&bash_target)?,
            format!("{original}{}", SH_PATH[0])
        );
        assert!(home.join(".bashrc").symlink_metadata()?.is_symlink());
        assert!(!home.join(".profile").exists());
        assert!(
            home.join(".wingmate/bin/dsh")
                .metadata()?
                .permissions()
                .mode()
                & 0o100
                != 0
        );
        assert!(wingmate_path.metadata()?.permissions().mode() & 0o100 != 0);
        assert!(!configure(Some(false))?.enabled);
        assert_eq!(fs::read_to_string(bash_target)?, original);
        assert_eq!(fs::read_to_string(home.join(".bash_login"))?, original);
        assert!(home.join(".bash_profile").symlink_metadata()?.is_symlink());
        assert_eq!(fs::read_to_string(zsh.join(".zshrc"))?, "");
        assert_eq!(fs::read_to_string(fish.join("fish/config.fish"))?, "");
        assert!(!home.join(".wingmate/bin/dsh").exists());
        assert!(!wingmate_path.exists());

        configure(Some(true))?;
        let dsh_path = home.join(".wingmate/bin/dsh");
        fs::write(&dsh_path, "user's command")?;
        let status = configure(None)?;
        assert!(status.enabled && status.needs_update);
        assert!(configure(Some(true)).is_err());
        assert!(!configure(Some(false))?.enabled);
        assert_eq!(fs::read_to_string(dsh_path)?, "user's command");
        assert!(!wingmate_path.exists());
        Ok(())
    }
}
