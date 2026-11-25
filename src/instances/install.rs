use super::{
    instance,
    model::{Generation, Profile, Version},
};
use crate::{
    packs::metadata::{self, Layer, Recipe},
    storage::{lock, paths::under, store},
};
use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

/// Install into an unpublished directory. Only a complete directory becomes visible to list/launch.
pub fn import(root: &Path, id: &str, manifest: &Path, source: &Path) -> Result<()> {
    let recipe: Recipe =
        serde_json::from_reader(File::open(manifest).context("open local recipe")?)?;
    separate_source(root, source)?;
    install(root, id, |_| {
        metadata::resolve(root, source, &recipe, source)
    })
}

pub(crate) fn separate_source(root: &Path, source: &Path) -> Result<()> {
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

pub(crate) fn install(
    root: &Path,
    id: &str,
    prepare: impl FnOnce(&Path) -> Result<Generation>,
) -> Result<()> {
    install_with_private(root, id, prepare, |_| Ok(()))
}

pub(crate) fn install_with_private(
    root: &Path,
    id: &str,
    prepare: impl FnOnce(&Path) -> Result<Generation>,
    private: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
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
    private(&stage.path().join("game"))?;
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

pub(crate) fn materialize(root: &Path, view: &Path, entry: &metadata::Entry) -> Result<PathBuf> {
    let dest = under(view, &entry.target)?;
    fs::create_dir_all(dest.parent().unwrap())?;
    store::copy_checked(root, &entry.blob, File::create_new(&dest)?)?;
    Ok(dest)
}
