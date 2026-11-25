pub(crate) mod install;
pub(crate) mod model;

use crate::{
    Loader,
    storage::{
        lock,
        paths::{self, component, under},
        store,
    },
    workspace,
};
use anyhow::{Context, Result, ensure};
use model::{Generation, Profile};
use serde::Serialize;
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

pub(crate) fn instance(root: &Path, id: &str) -> Result<PathBuf> {
    component(id)?;
    ensure!(
        id.len() <= 64
            && id
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_')),
        "instance id must contain 1–64 lowercase letters, digits, '-' or '_'"
    );
    under(root, &format!("instances/{id}"))
}

pub(crate) fn generation(root: &Path, instance: &Path) -> Result<Generation> {
    let profile: Profile = serde_json::from_reader(File::open(under(instance, "profile.json")?)?)?;
    store::record(root, "generations", &profile.generation)
        .with_context(|| format!("read generations/{}.json", profile.generation))
}

pub fn list(root: &Path) -> Result<Vec<String>> {
    let ids = instance_ids(root)?;
    for id in &ids {
        generation(root, &instance(root, id)?).with_context(|| format!("invalid instance {id}"))?;
    }
    Ok(ids)
}

pub(crate) fn instance_ids(root: &Path) -> Result<Vec<String>> {
    let dir = under(root, "instances")?;
    if !dir.try_exists()? {
        return Ok(vec![]);
    }
    let mut ids = vec![];
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("invalid instance name"))?;
        let path = instance(root, &name)?;
        if path.is_dir() {
            ids.push(name);
        }
    }
    ids.sort();
    Ok(ids)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceInfo {
    pub id: String,
    pub minecraft: String,
    pub loader: Loader,
    pub loader_version: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fabric: String,
    pub java_major: u32,
    pub platform: String,
    pub game: PathBuf,
    pub worlds: Vec<String>,
}

pub fn list_info(root: &Path) -> Result<Vec<InstanceInfo>> {
    instance_ids(root)?
        .iter()
        .map(|id| info(root, id))
        .collect()
}

pub fn info(root: &Path, id: &str) -> Result<InstanceInfo> {
    let dir = instance(root, id)?;
    let current = generation(root, &dir).with_context(|| format!("read instance {id}"))?;
    let game = under(&dir, "game")?;
    let saves = under(&game, "saves")?;
    let mut worlds = vec![];
    if saves.try_exists()? {
        for entry in fs::read_dir(saves)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() && !entry.file_type()?.is_symlink() {
                paths::no_link(&entry.path())?;
                if under(&entry.path(), "level.dat")?.is_file() {
                    worlds.push(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
    }
    worlds.sort();
    Ok(InstanceInfo {
        id: id.into(),
        minecraft: current.minecraft,
        loader: current.loader,
        fabric: if current.loader == Loader::Fabric {
            current.loader_version.clone()
        } else {
            String::new()
        },
        loader_version: current.loader_version,
        java_major: current.java_major,
        platform: current.platform,
        game,
        worlds,
    })
}

/// Detach atomically before optionally purging the private data.
pub fn uninstall(root: &Path, id: &str, delete_data: bool) -> Result<Option<PathBuf>> {
    let dir = instance(root, id)?;
    let _global = lock(&under(root, "store.lock")?, true)?;
    let _instances = lock(&under(root, "instances.lock")?, true)?;
    generation(root, &dir)?;
    drop(lock(&under(&dir, "run.lock")?, false)?);
    let selected = under(root, "selected.json")?;
    let clear = selected.try_exists()? && workspace::selected(root)? == id;
    let removed = under(root, "removed")?;
    fs::create_dir_all(&removed)?;
    let backup = tempfile::Builder::new()
        .prefix(&format!("{id}-"))
        .tempdir_in(removed)?;
    let dest = backup.path().join(id);
    fs::rename(&dir, &dest).context("detach instance")?;
    let backup = backup.keep();
    if clear {
        fs::remove_file(selected)?;
    }
    if delete_data {
        fs::remove_dir_all(&backup).with_context(|| {
            format!(
                "instance detached; cleanup incomplete in {}",
                backup.display()
            )
        })?;
        Ok(None)
    } else {
        Ok(Some(dest))
    }
}
