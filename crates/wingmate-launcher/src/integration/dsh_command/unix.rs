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
pub(super) const SH_PATH: [&str; 3] = [
    r#"
# >>> Wingmate dsh >>>
if [ -x "$HOME/.wingmate/bin/wingmate" ] || [ -x "$HOME/.wingmate/bin/dsh" ]; then
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
pub(super) const FISH_PATH: [&str; 3] = [
    r#"
# >>> Wingmate dsh >>>
if test -x "$HOME/.wingmate/bin/wingmate"; or test -x "$HOME/.wingmate/bin/dsh"
  if test "$PATH[1]" != "$HOME/.wingmate/bin"
    set -gx PATH "$HOME/.wingmate/bin" $PATH
  end
end
# <<< Wingmate dsh <<<
"#,
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
    dsh_launcher: Option<impl FnOnce() -> Result<String>>,
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
    configure_in(&home, &zsh, &fish, enabled, dsh_launcher, wingmate)
}

fn configure_in(
    home: &Path,
    zsh: &Path,
    fish: &Path,
    enabled: Option<bool>,
    dsh_launcher: Option<impl FnOnce() -> Result<String>>,
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
    let open_dsh = || CommandFile::open(&bin, "dsh", owned(OWNER));
    let dsh = dsh_launcher
        .map(|launcher| -> Result<_> { Ok((open_dsh()?, launcher)) })
        .transpose()?;
    let wingmate_file = CommandFile::open(&bin, "wingmate", owned(WINGMATE_OWNER))?;
    let wingmate = format!("{SHEBANG}{wingmate}");
    let login_profiles = [".bash_profile", ".bash_login", ".profile"].map(|name| home.join(name));
    let bash_login = login_profiles
        .iter()
        .find(|path| path.is_file())
        .unwrap_or(&login_profiles[2]);
    let bash_rc = home.join(".bashrc");
    let zsh_env = zsh.join(".zshenv");
    let zsh_rc = zsh.join(".zshrc");
    let fish_rc = fish.join("fish/config.fish");
    let profiles = [
        (bash_login, SH_PATH, false),
        (&zsh_env, SH_PATH, false),
        (&zsh_rc, SH_PATH, false),
        (&fish_rc, FISH_PATH, false),
        (&bash_rc, SH_PATH, true),
    ];
    match enabled {
        Some(true) => {
            wingmate_file.ensure_available()?;
            let dsh = dsh
                .map(|(file, launcher)| -> Result<_> {
                    file.ensure_available()?;
                    Ok((file, format!("{SHEBANG}{}", launcher()?)))
                })
                .transpose()?;
            for (path, blocks, prepend) in profiles {
                update_profile(path, blocks, true, prepend)
                    .with_context(|| format!("更新 {}", path.display()))?;
            }
            if let Some((file, script)) = dsh {
                file.install(&script)?;
            }
            wingmate_file.install(&wingmate)?;
            Ok(Status {
                enabled: true,
                needs_update: false,
            })
        }
        Some(false) => {
            if dsh.is_some() || open_dsh()?.current.is_none() {
                for (path, blocks, _) in profiles.into_iter().chain(
                    login_profiles
                        .iter()
                        .filter(|path| *path != bash_login)
                        .map(|path| (path, SH_PATH, false)),
                ) {
                    update_profile(path, blocks, false, false)
                        .with_context(|| format!("更新 {}", path.display()))?;
                }
            }
            wingmate_file.remove()?;
            if let Some((file, _)) = dsh {
                file.remove()?;
            }
            Ok(Status {
                enabled: false,
                needs_update: false,
            })
        }
        None => {
            let enabled = wingmate_file.current.is_some()
                || dsh.as_ref().is_some_and(|(file, _)| file.current.is_some());
            let needs_update = enabled
                && (dsh.is_some_and(|(file, launcher)| {
                    file.current.is_none()
                        || launcher().ok().as_deref()
                            != file
                                .current
                                .as_deref()
                                .and_then(|text| text.strip_prefix(SHEBANG))
                        || !is_executable(&file.path)
                }) || wingmate_file.current.as_deref() != Some(wingmate.as_str())
                    || !is_executable(&wingmate_file.path)
                    || profiles.iter().any(|(path, [block, ..], prepend)| {
                        !fs::read_to_string(path).is_ok_and(|text| {
                            if *prepend {
                                text.starts_with(*block)
                            } else {
                                text.contains(*block)
                            }
                        })
                    }));
            Ok(Status {
                enabled,
                needs_update,
            })
        }
    }
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o100 != 0)
}

fn update_profile(
    path: &Path,
    [block, previous, legacy]: [&str; 3],
    enabled: bool,
    prepend: bool,
) -> Result<()> {
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
    let mut content = original
        .replace(block, "")
        .replace(previous, "")
        .replace(legacy, "");
    if enabled {
        if prepend {
            // Non-interactive SSH shells may return early from .bashrc.
            content.insert_str(0, block);
        } else {
            content.push_str(block);
        }
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
        let configure =
            |enabled| configure_in(home, &zsh, &fish, enabled, Some(launcher), &wingmate);
        assert!(!configure(None)?.enabled);
        assert!(configure(Some(true))?.enabled);
        assert!(!configure(None)?.needs_update);
        let wingmate_path = home.join(".wingmate/bin/wingmate");
        fs::remove_file(&wingmate_path)?;
        assert!(configure(None)?.needs_update);
        fs::write(&bash_target, format!("{original}{}", SH_PATH[2]))?;
        configure(Some(true))?;
        assert!(!configure(None)?.needs_update);
        assert_eq!(
            fs::read_to_string(&bash_target)?,
            format!("{}{original}", SH_PATH[0])
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
        let removed = configure_in(
            home,
            &zsh,
            &fish,
            Some(false),
            None::<fn() -> Result<String>>,
            "",
        )?;
        assert!(!removed.enabled && !wingmate_path.exists());
        assert!(home.join(".wingmate/bin/dsh").exists());
        assert!(fs::read_to_string(&bash_target)?.starts_with(SH_PATH[0]));
        assert!(configure(None)?.needs_update);
        assert!(!configure(Some(false))?.enabled);
        assert_eq!(fs::read_to_string(bash_target)?, original);
        assert_eq!(fs::read_to_string(home.join(".bash_login"))?, original);
        assert!(home.join(".bash_profile").symlink_metadata()?.is_symlink());
        assert_eq!(fs::read_to_string(zsh.join(".zshrc"))?, "");
        assert_eq!(fs::read_to_string(zsh.join(".zshenv"))?, "");
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

    #[test]
    fn install_without_dsh_sets_path_before_bashrc_returns() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let home = directory.path();
        let original = "[ \"${-#*i}\" = \"$-\" ] && return\n";
        fs::write(home.join(".bashrc"), original)?;
        symlink(home.join(".bashrc"), home.join(".bash_profile"))?;
        let wingmate = super::super::wingmate_launcher(&home.join("release"), home)?;
        let configure_wingmate = |enabled| {
            configure_in(
                home,
                home,
                home,
                Some(enabled),
                None::<fn() -> Result<String>>,
                &wingmate,
            )
        };
        assert!(configure_wingmate(true)?.enabled);
        assert!(configure_wingmate(true)?.enabled);
        let bin = home.join(".wingmate/bin");
        assert!(!bin.join("dsh").exists());
        let output = std::process::Command::new("bash")
            .env("HOME", home)
            .env_remove("BASH_ENV")
            .args(["-c", ". \"$HOME/.bashrc\"; command -v wingmate"])
            .output()?;
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(
            String::from_utf8(output.stdout)?.trim_end(),
            bin.join("wingmate").to_str().unwrap()
        );
        assert_eq!(
            fs::read_to_string(home.join(".bashrc"))?,
            format!("{}{original}", SH_PATH[0])
        );
        assert_eq!(fs::read_to_string(home.join(".zshenv"))?, SH_PATH[0]);
        assert!(!configure_wingmate(false)?.enabled);
        assert!(!configure_wingmate(false)?.enabled);
        assert!(!bin.join("wingmate").exists());
        assert_eq!(fs::read_to_string(home.join(".bashrc"))?, original);
        Ok(())
    }
}
