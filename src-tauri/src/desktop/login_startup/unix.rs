use super::{optional, Status};
use anyhow::{Context, Result};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const ID: &str = "dev.wingmate.desktop";

pub(super) fn configure(executable: &Path, enabled: Option<bool>) -> Result<Status> {
    configure_in(&entry_path()?, executable, enabled)
}

fn configure_in(path: &Path, executable: &Path, enabled: Option<bool>) -> Result<Status> {
    match enabled {
        Some(true) => {
            let content = encode_entry(executable)?;
            let parent = path.parent().expect("login entry has a parent directory");
            fs::create_dir_all(parent)?;
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            file.write_all(&content)?;
            file.persist(path)?;
        }
        Some(false) => {
            optional(fs::remove_file(path))?;
        }
        None => {}
    }
    let entry = optional(fs::read(path))?;
    let (enabled, matches) = match &entry {
        Some(content) => entry_status(content, executable),
        None => (false, false),
    };
    Ok(Status {
        registered: entry.is_some(),
        enabled,
        needs_update: entry.is_some() && !matches,
    })
}

#[cfg(target_os = "linux")]
fn entry_path() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("无法确定用户配置目录")?
        .join("autostart")
        .join(format!("{ID}.desktop")))
}

#[cfg(target_os = "linux")]
fn exec_command(executable: &Path) -> Result<String> {
    let path = executable
        .to_str()
        .context("可执行文件路径不是有效的 UTF-8")?;
    anyhow::ensure!(
        !path.contains('=') && !path.chars().any(char::is_control),
        "可执行文件路径包含登录启动项不支持的字符"
    );
    // XDG applies desktop-entry string escaping before parsing the quoted Exec argument.
    let mut command = String::new();
    for ch in path.chars() {
        match ch {
            '\\' => command.push_str(r"\\\"),
            '"' | '$' | '`' => command.push_str(r"\\"),
            '%' => command.push('%'),
            _ => {}
        }
        command.push(ch);
    }
    Ok(format!(r#""{command}" start"#))
}

#[cfg(target_os = "linux")]
fn encode_entry(executable: &Path) -> Result<Vec<u8>> {
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=Wingmate\nTerminal=false\nExec={}\n",
        exec_command(executable)?
    )
    .into_bytes())
}

#[cfg(target_os = "linux")]
fn entry_status(content: &[u8], executable: &Path) -> (bool, bool) {
    let Ok(content) = std::str::from_utf8(content) else {
        return (false, false);
    };
    let fields = || {
        content
            .lines()
            .map(str::trim)
            .skip_while(|line| *line != "[Desktop Entry]")
            .skip(1)
            .take_while(|line| !line.starts_with('['))
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim(), value.trim()))
    };
    let hidden = fields().any(|field| {
        matches!(
            field,
            ("Hidden", "true") | ("X-GNOME-Autostart-enabled", "false")
        )
    });
    let command = fields().find_map(|(key, value)| (key == "Exec").then_some(value));
    let matches = exec_command(executable).is_ok_and(|expected| command == Some(expected.as_str()));
    (command.is_some() && !hidden, matches)
}

#[cfg(target_os = "macos")]
fn entry_path() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("无法确定用户主目录")?
        .join("Library/LaunchAgents")
        .join(format!("{ID}.plist")))
}

#[cfg(target_os = "macos")]
fn encode_entry(executable: &Path) -> Result<Vec<u8>> {
    let program = executable
        .to_str()
        .context("可执行文件路径不是有效的 UTF-8")?;
    let mut agent = plist::Dictionary::new();
    agent.insert("Label".into(), ID.into());
    agent.insert(
        "ProgramArguments".into(),
        plist::Value::Array(vec![program.into(), "start".into()]),
    );
    agent.insert("RunAtLoad".into(), true.into());
    // Wingmate's launcher exits after the backend takes ownership.
    agent.insert("AbandonProcessGroup".into(), true.into());
    if let Ok(path) = std::env::var("PATH") {
        // launchd's default PATH can omit the Node installation used by Wingmate.
        let mut environment = plist::Dictionary::new();
        environment.insert("PATH".into(), path.into());
        agent.insert(
            "EnvironmentVariables".into(),
            plist::Value::Dictionary(environment),
        );
    }
    let mut content = Vec::new();
    plist::Value::Dictionary(agent).to_writer_xml(&mut content)?;
    Ok(content)
}

#[cfg(target_os = "macos")]
fn entry_status(content: &[u8], executable: &Path) -> (bool, bool) {
    let Ok(plist::Value::Dictionary(agent)) = plist::Value::from_reader_xml(content) else {
        return (false, false);
    };
    let enabled = agent.get("RunAtLoad").and_then(plist::Value::as_boolean) == Some(true)
        && agent.get("Disabled").and_then(plist::Value::as_boolean) != Some(true);
    let matches = agent.get("ProgramArguments").and_then(plist::Value::as_array).is_some_and(|args| {
        matches!(args.as_slice(), [program, command] if program.as_string() == executable.to_str()
            && command.as_string() == Some("start"))
    });
    (enabled, matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn startup_status_only_reads_the_desktop_entry_group() -> Result<()> {
        let executable = Path::new("/opt/Wingmate");
        let mut entry = encode_entry(executable)?;
        entry.extend_from_slice(b"[Desktop Action Other]\nHidden=true\nExec=other\n");
        assert_eq!(entry_status(&entry, executable), (true, true));
        assert_eq!(
            entry_status(b"[Desktop Action Other]\nExec=other\n", executable),
            (false, false)
        );
        let mut hidden = encode_entry(executable)?;
        hidden.extend_from_slice(b"Hidden=true\n");
        assert_eq!(entry_status(&hidden, executable), (false, true));
        Ok(())
    }

    #[test]
    fn startup_file_round_trip_with_a_moved_executable() -> Result<()> {
        let root = tempfile::tempdir()?;
        let entry = root.path().join("autostart").join("wingmate");
        let original = Path::new("/opt/Tools & Apps/100%/Wingmate");
        let moved = Path::new("/opt/Wingmate");
        assert!(!configure_in(&entry, original, None)?.registered);
        assert!(configure_in(&entry, original, Some(true))?.enabled);
        assert!(configure_in(&entry, moved, None)?.needs_update);
        let updated = configure_in(&entry, moved, Some(true))?;
        assert!(updated.enabled && !updated.needs_update);
        assert!(!configure_in(&entry, moved, Some(false))?.registered);
        assert!(!configure_in(&entry, moved, Some(false))?.registered);
        Ok(())
    }
}
