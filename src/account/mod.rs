mod service;

use crate::{Player, storage::paths::under, workspace::config_dir};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};

/// A Minecraft session issued after Microsoft sign-in and Java entitlement verification.
pub struct Session {
    pub(crate) player: Player,
    pub(crate) access: String,
    pub(crate) client: String,
    pub(crate) xuid: String,
}

impl Session {
    // Only the canonical game argument receives the bearer token. Pack templates and
    // custom JVM options never expand to credentials.
    pub(crate) fn set_game_identity(&self, args: &mut Vec<String>) {
        let fields = [
            ("--username", self.player.name.as_str()),
            ("--uuid", &self.player.uuid),
            ("--accessToken", &self.access),
            ("--userType", "msa"),
            ("--clientId", &self.client),
            ("--xuid", &self.xuid),
        ];
        let mut original = std::mem::take(args).into_iter().peekable();
        for (key, value) in fields {
            if !value.is_empty() {
                args.extend([key.to_owned(), value.to_owned()]);
            }
        }
        while let Some(arg) = original.next() {
            if arg == "--" {
                args.push(arg);
                args.extend(original);
                break;
            }
            let key = arg.split('=').next().unwrap_or(&arg);
            if fields.iter().any(|(k, _)| {
                key.trim_start_matches('-') == k.trim_start_matches('-') && key.starts_with('-')
            }) {
                if key == arg {
                    original.next_if(|s| !s.starts_with('-'));
                }
            } else {
                args.push(arg);
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    client: String,
    refresh: String,
    name: String,
    uuid: String,
}

struct Vault {
    path: PathBuf,
    _lock: fs::File,
}

impl Vault {
    fn open() -> Result<Self> {
        let dir = config_dir()?;
        fs::create_dir_all(&dir)?;
        Ok(Self {
            path: under(&dir, "microsoft-account.bin")?,
            _lock: crate::storage::lock(&under(&dir, "microsoft-account.lock")?, true)?,
        })
    }

    fn load(&self) -> Result<Option<Saved>> {
        if !self.path.try_exists()? {
            return Ok(None);
        }
        let bytes = protect(&fs::read(&self.path)?, false)?;
        Ok(Some(
            serde_json::from_slice(&bytes).context("Invalid saved account. Sign in again.")?,
        ))
    }

    fn save(&self, account: &Saved) -> Result<()> {
        let bytes = protect(&serde_json::to_vec(account)?, true)?;
        let mut file = tempfile::NamedTempFile::new_in(
            self.path.parent().context("Missing account directory")?,
        )?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(&self.path)?;
        Ok(())
    }
}

#[cfg(windows)]
fn protect(data: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    use windows_dpapi::{Scope, decrypt_data, encrypt_data};
    if encrypt {
        encrypt_data(data, Scope::User, None)
    } else {
        decrypt_data(data, Scope::User, None)
    }
    .context("Windows could not access the saved account for this user")
}

#[cfg(not(windows))]
fn protect(_: &[u8], _: bool) -> Result<Vec<u8>> {
    anyhow::bail!("Microsoft account storage requires Windows")
}

pub fn login(client: &str) -> Result<String> {
    ensure!(
        crate::launch::valid_uuid(client),
        "Supply the application (client) ID as a hyphenated GUID"
    );
    let service = service::Service::new();
    let tokens = service.login(client)?;
    let refresh = tokens
        .refresh_token
        .context("Microsoft did not grant offline_access. Sign in again.")?;
    let session = service.minecraft(client, &tokens.access_token)?;
    Vault::open()?.save(&Saved {
        client: client.into(),
        refresh,
        name: session.player.name.clone(),
        uuid: session.player.uuid,
    })?;
    Ok(session.player.name)
}

pub fn status() -> Result<Option<(String, String)>> {
    Ok(Vault::open()?.load()?.map(|s| (s.name, s.uuid)))
}

pub fn logout() -> Result<()> {
    let vault = Vault::open()?;
    if vault.path.try_exists()? {
        fs::remove_file(&vault.path)?;
    }
    Ok(())
}

pub fn session() -> Result<Session> {
    let vault = Vault::open()?;
    let mut account = vault
        .load()?
        .context("No Microsoft account saved. Run chulch account login first.")?;
    let service = service::Service::new();
    let tokens = service.refresh(&account.client, &account.refresh)?;
    if let Some(refresh) = tokens.refresh_token {
        account.refresh = refresh;
        vault.save(&account)?;
    }
    let session = service.minecraft(&account.client, &tokens.access_token)?;
    ensure!(
        session.player.uuid == account.uuid,
        "Minecraft profile changed. Sign in again."
    );
    if session.player.name != account.name {
        account.name.clone_from(&session.player.name);
        vault.save(&account)?;
    }
    Ok(session)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn vault_round_trip_preserves_identity_and_protects_refresh_token() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let vault = Vault {
            path: dir.path().join("account.bin"),
            _lock: crate::storage::lock(&dir.path().join("account.lock"), true)?,
        };
        let saved = Saved {
            client: "fixture".into(),
            refresh: "fake-refresh-for-test".into(),
            name: "Test".into(),
            uuid: "fixture-id".into(),
        };
        vault.save(&saved)?;
        let blob = fs::read(&vault.path)?;
        assert!(
            !blob
                .windows(saved.refresh.len())
                .any(|s| s == saved.refresh.as_bytes())
        );
        assert_eq!(vault.load()?.unwrap().refresh, saved.refresh);
        Ok(())
    }
}
