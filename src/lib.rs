mod archive;
mod distribution;
mod loader;
pub mod maintenance;
mod metadata;
pub use loader::Loader;
mod network;
pub use distribution::install_pack;
pub mod java;
mod paths;
pub mod resources;
mod runtime;
mod store;
pub mod workspace;

use anyhow::{Context, Result, ensure};
use metadata::{Layer, Recipe};
use paths::{component, under};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

pub use runtime::{Player, launch, launch_with_java};

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
enum Version {
    V1,
}

impl TryFrom<u32> for Version {
    type Error = String;
    fn try_from(n: u32) -> std::result::Result<Self, String> {
        match n {
            1 => Ok(Self::V1),
            _ => Err(format!("unsupported schemaVersion {n}")),
        }
    }
}
impl From<Version> for u32 {
    fn from(_: Version) -> Self {
        1
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Generation {
    schema_version: Version,
    minecraft: String,
    #[serde(default)]
    loader: Loader,
    #[serde(alias = "fabric")]
    loader_version: String,
    platform: String,
    java_major: u32,
    main_class: String,
    argument_profile: String,
    #[serde(default)]
    arguments: loader::Arguments,
    #[serde(default)]
    quick_play: Option<bool>,
    asset_index: String,
    layers: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    schema_version: Version,
    generation: String,
}

fn instance(root: &Path, id: &str) -> Result<PathBuf> {
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

fn lock(path: &Path, wait: bool) -> Result<File> {
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    if wait {
        file.lock()?;
    } else {
        file.try_lock().context("instance is busy")?;
    }
    Ok(file)
}

fn generation(root: &Path, instance: &Path) -> Result<Generation> {
    let profile: Profile = serde_json::from_reader(File::open(under(instance, "profile.json")?)?)?;
    store::record(root, "generations", &profile.generation)
        .with_context(|| format!("read generations/{}.json", profile.generation))
}

/// Install into an unpublished directory. Only a complete directory becomes visible to list/launch.
pub fn import(root: &Path, id: &str, manifest: &Path, source: &Path) -> Result<()> {
    let recipe: Recipe =
        serde_json::from_reader(File::open(manifest).context("open local recipe")?)?;
    separate_source(root, source)?;
    install(root, id, |_| resolve(root, source, source, &recipe))
}

fn separate_source(root: &Path, source: &Path) -> Result<()> {
    let source = source.canonicalize().context("open source installation")?;
    ensure!(source.is_dir(), "source must be a directory");
    // Resolve even a not-yet-created data root before writing anything into the source.
    let absolute = std::path::absolute(root)?;
    let existing = absolute
        .ancestors()
        .find(|p| p.exists())
        .context("invalid data root")?;
    let resolved = existing
        .canonicalize()?
        .join(absolute.strip_prefix(existing)?);
    ensure!(
        !resolved.starts_with(&source) && !source.starts_with(&resolved),
        "source and data root must be separate"
    );
    Ok(())
}

fn resolve(root: &Path, source: &Path, pack_source: &Path, recipe: &Recipe) -> Result<Generation> {
    metadata::resolve(root, source, recipe, pack_source)
}

fn install(root: &Path, id: &str, prepare: impl FnOnce(&Path) -> Result<Generation>) -> Result<()> {
    let dest = instance(root, id)?;
    fs::create_dir_all(root)?;
    let _store_lock = lock(&under(root, "store.lock")?, true)?;
    ensure!(!dest.try_exists()?, "instance already exists: {id}");
    let work = under(root, "work")?;
    fs::create_dir_all(&work)?;
    fs::create_dir_all(under(root, "instances")?)?;
    let stage = tempfile::Builder::new()
        .prefix("install-")
        .tempdir_in(&work)?;
    let scratch = tempfile::Builder::new()
        .prefix("input-")
        .tempdir_in(&work)?;
    let current = prepare(scratch.path())?;
    fs::create_dir_all(stage.path().join("game/mods"))?;
    let pack: Layer = store::record(root, "layers", &current.layers[2])?;
    for entry in pack.entries {
        materialize(root, stage.path(), &entry)?;
    }
    let generation = store::publish_json(root, "generations", &current)?;
    let profile = File::create_new(stage.path().join("profile.json"))?;
    serde_json::to_writer(
        &profile,
        &Profile {
            schema_version: Version::V1,
            generation,
        },
    )?;
    profile.sync_all()?;
    drop(profile);
    fs::rename(stage.path(), dest).context("publish instance")?;
    Ok(())
}

fn materialize(root: &Path, view: &Path, entry: &metadata::Entry) -> Result<PathBuf> {
    let dest = under(view, &entry.target)?;
    fs::create_dir_all(dest.parent().unwrap())?;
    store::copy_checked(root, &entry.blob, File::create_new(&dest)?)?;
    Ok(dest)
}

pub fn list(root: &Path) -> Result<Vec<String>> {
    let ids = instance_ids(root)?;
    for id in &ids {
        generation(root, &instance(root, id)?).with_context(|| format!("invalid instance {id}"))?;
    }
    Ok(ids)
}

fn instance_ids(root: &Path) -> Result<Vec<String>> {
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
