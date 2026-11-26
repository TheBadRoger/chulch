pub mod maintenance;
pub mod resources;

use crate::{
    instances::generation, instances::instance, instances::model::Version, storage::lock,
    storage::paths::under,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    schema_version: Version,
    workspace: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Selection {
    schema_version: Version,
    instance: String,
}

pub(crate) fn config_dir() -> Result<PathBuf> {
    let base =
        PathBuf::from(env::var_os("APPDATA").context("APPDATA is unavailable; pass --workspace")?);
    preferred_dir(base.join("chulch"), base.join("mcdocker"))
}

fn preferred_dir(current: PathBuf, legacy: PathBuf) -> Result<PathBuf> {
    if !current.try_exists()? && legacy.is_dir() {
        return Ok(legacy);
    }
    Ok(current)
}

pub(crate) fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("missing parent directory")?;
    fs::create_dir_all(parent)?;
    let file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(file.as_file(), value)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

pub fn resolve(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit.or_else(|| {
        env::var_os("CHULCH_WORKSPACE")
            .or_else(|| env::var_os("MCDOCKER_WORKSPACE"))
            .map(PathBuf::from)
    }) {
        return Ok(std::path::absolute(path)?);
    }
    let config = config_dir()?.join("settings.json");
    if config.try_exists()? {
        let settings: Settings = serde_json::from_reader(fs::File::open(config)?)?;
        return Ok(settings.workspace);
    }
    let local = preferred_dir(".chulch".into(), ".mcdocker".into())?;
    if local.is_dir() {
        return Ok(std::path::absolute(local)?);
    }
    let base = PathBuf::from(
        env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is unavailable; pass --workspace")?,
    );
    preferred_dir(
        base.join("chulch/workspace"),
        base.join("mcdocker/workspace"),
    )
}

pub fn set_default(path: &Path) -> Result<PathBuf> {
    let workspace = std::path::absolute(path)?;
    crate::storage::paths::no_link(&workspace)?;
    fs::create_dir_all(&workspace)?;
    let config = config_dir()?.join("settings.json");
    if config.try_exists()? {
        let _: Settings = serde_json::from_reader(fs::File::open(&config)?)?;
    }
    write_json(
        &config,
        &Settings {
            schema_version: Version::V1,
            workspace: workspace.clone(),
        },
    )?;
    Ok(workspace)
}

pub fn select(root: &Path, id: &str) -> Result<()> {
    let _global = lock(&under(root, "instances.lock")?, true)?;
    generation(root, &instance(root, id)?)?;
    write_json(
        &under(root, "selected.json")?,
        &Selection {
            schema_version: Version::V1,
            instance: id.into(),
        },
    )
}

pub fn selected(root: &Path) -> Result<String> {
    let file = fs::File::open(under(root, "selected.json")?)
        .context("no current instance; specify an id or run chulch use <id>")?;
    let selection: Selection = serde_json::from_reader(file)?;
    Ok(selection.instance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_installs_use_chulch_and_upgrades_find_the_existing_directory() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let current = temp.path().join("chulch");
        let legacy = temp.path().join("mcdocker");
        let choose = || preferred_dir(current.clone(), legacy.clone());
        assert_eq!(choose()?, current);
        fs::create_dir(&legacy)?;
        assert_eq!(choose()?, legacy);
        fs::create_dir(&current)?;
        assert_eq!(choose()?, current);
        Ok(())
    }
}
