use super::{host, read};
use anyhow::{ensure, Context, Result};
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};
use uuid::Uuid;

pub(super) const MAX_BYTES: usize = 256;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Password {
    pub(super) hash: String,
    credential: Option<Uuid>,
}

const FILE: &str = "lan-password.json";

impl Password {
    pub(super) fn load(home: &Path) -> Result<Option<Self>> {
        read(&home.join(FILE))
    }

    pub(super) fn install(home: &Path, value: &str, recoverable: bool) -> Result<Self> {
        ensure!(
            value.chars().count() >= 8
                && value.len() <= MAX_BYTES
                && !value.chars().any(char::is_control),
            "局域网密码至少 8 个字符、最多 {MAX_BYTES} 字节，不能包含控制字符"
        );
        let random = Uuid::new_v4();
        let salt = SaltString::encode_b64(random.as_bytes())?;
        let hash = Argon2::default()
            .hash_password(value.as_bytes(), &salt)?
            .to_string();
        let credential = if recoverable {
            entry(home, random)?
                .set_password(value)
                .context("无法保存到系统凭据库")?;
            Some(random)
        } else {
            None
        };
        let password = Self { hash, credential };
        if let Err(error) = host::publish(&home.join(FILE), &password) {
            password.delete_credential(home);
            return Err(error);
        }
        Ok(password)
    }

    pub(super) fn disable(home: &Path) -> Result<()> {
        match fs::remove_file(home.join(FILE)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }

    pub(super) fn reveal(&self, home: &Path) -> Result<String> {
        entry(
            home,
            self.credential
                .context("此密码由命令行设置，仅保存校验值。可在桌面重新设置密码。")?,
        )?
        .get_password()
        .context("无法从系统凭据库读取密码")
    }

    pub(super) fn delete_credential(&self, home: &Path) {
        if let Some(id) = self.credential {
            let result = entry(home, id).and_then(|entry| match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(error.into()),
            });
            if let Err(error) = result {
                eprintln!("无法清理旧的局域网密码凭据：{error:#}");
            }
        }
    }
}

fn entry(home: &Path, id: Uuid) -> Result<keyring::Entry> {
    let account = super::digest(&format!("{}\n{id}", home.display()));
    Ok(keyring::Entry::new("Wingmate LAN password", &account)?)
}

pub(super) fn matches(hash: &str, password: &[u8]) -> bool {
    PasswordHash::new(hash)
        .is_ok_and(|hash| Argon2::default().verify_password(password, &hash).is_ok())
}
