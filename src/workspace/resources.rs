use crate::{
    instances::model::Version,
    storage::paths::{no_link, under},
    storage::store,
    workspace::{config_dir, write_json},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::ErrorKind,
    path::{Path, PathBuf},
};

const KINDS: [&str; 2] = ["resourcepacks", "shaderpacks"];

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    schema_version: Version,
    directory: PathBuf,
}

pub fn current() -> Result<Option<PathBuf>> {
    if std::env::var_os("APPDATA").is_none() {
        return Ok(None);
    }
    let path = config_dir()?.join("resources.json");
    match File::open(&path) {
        Ok(file) => Ok(Some(
            serde_json::from_reader::<_, Settings>(file)?.directory,
        )),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| path.display().to_string()),
    }
}

pub fn clear() -> Result<()> {
    match fs::remove_file(config_dir()?.join("resources.json")) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        result => Ok(result?),
    }
}

pub fn set(workspace: &Path, directory: &Path) -> Result<PathBuf> {
    let directory = std::path::absolute(directory)?;
    no_link(&directory)?;
    fs::create_dir_all(&directory)?;
    let real = directory.canonicalize()?;
    if workspace.try_exists()? {
        let root = workspace.canonicalize()?;
        for area in ["instances", "removed", "work", "store", "runtimes"] {
            ensure!(
                !real.starts_with(root.join(area)),
                "choose a shared directory outside the workspace's {area} directory"
            );
        }
        for kind in KINDS {
            ensure!(
                !root.starts_with(real.join(kind)),
                "the workspace is inside the shared {kind} directory"
            );
        }
    }
    for kind in KINDS {
        fs::create_dir_all(under(&directory, kind)?)?;
    }
    write_json(
        &config_dir()?.join("resources.json"),
        &Settings {
            schema_version: Version::V1,
            directory: directory.clone(),
        },
    )?;
    Ok(directory)
}

#[derive(Serialize, Deserialize)]
struct Pack {
    kind: String,
    name: String,
    digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    schema_version: Version,
    packs: Vec<Pack>,
}

// Include entry names and empty directories in the digest so a changed pack is kept.
fn pack_digest(path: &Path, copy_to: Option<&Path>) -> Result<String> {
    no_link(path)?;
    let meta = fs::symlink_metadata(path)?;
    if meta.is_file() {
        let blob = match copy_to {
            Some(dest) => store::stream(path, File::create_new(dest)?, None)?,
            None => store::fingerprint(path)?,
        };
        return Ok(format!("file:{}", blob.sha256));
    }
    ensure!(meta.is_dir(), "unsupported pack entry {}", path.display());
    if let Some(dest) = copy_to {
        fs::create_dir(dest)?;
    }
    let mut contents = BTreeMap::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("invalid pack filename in {}", path.display()))?;
        let dest = copy_to.map(|dir| dir.join(&name));
        contents.insert(name, pack_digest(&entry.path(), dest.as_deref())?);
    }
    Ok(format!(
        "dir:{}",
        store::id(&serde_json::to_vec(&contents)?)
    ))
}

fn receipt_path(instance: &Path) -> Result<PathBuf> {
    under(instance, "shared-packs.json")
}

fn cleanup(instance: &Path) -> Result<()> {
    let path = receipt_path(instance)?;
    if !path.try_exists()? {
        return Ok(());
    }
    let receipt: Receipt = serde_json::from_reader(File::open(&path)?)?;
    for pack in receipt.packs {
        ensure!(
            KINDS.contains(&pack.kind.as_str()),
            "invalid shared pack category"
        );
        let parent = under(instance, &format!("game/{}", pack.kind))?;
        crate::storage::paths::component(&pack.name)?;
        let target = parent.join(pack.name);
        if !target.try_exists()? {
            continue;
        }
        match pack_digest(&target, None) {
            Ok(hash) if hash == pack.digest => {
                if target.is_dir() {
                    fs::remove_dir_all(target)?;
                } else {
                    fs::remove_file(target)?;
                }
            }
            _ => eprintln!(
                "Keeping changed or unreadable instance pack: {}",
                target.display()
            ),
        }
    }
    fs::remove_file(path)?;
    Ok(())
}

fn pack_name(
    name: &str,
    is_file: bool,
    occupied: &mut BTreeSet<String>,
    originals: &BTreeSet<String>,
) -> String {
    if occupied.insert(name.to_lowercase()) {
        return name.to_owned();
    }
    let (stem, ext) = if is_file {
        Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map_or((name, ""), |e| name.split_at(name.len() - e.len() - 1))
    } else {
        (name, "")
    };
    for n in 1.. {
        let suffix = if n == 1 {
            "shared".into()
        } else {
            format!("shared {n}")
        };
        let alias = format!("{stem} ({suffix}){ext}");
        let key = alias.to_lowercase();
        if !originals.contains(&key) && occupied.insert(key) {
            return alias;
        }
    }
    unreachable!()
}

fn prepare(instance: &Path, stage: &Path, pool: &Path) -> Result<()> {
    let game = under(instance, "game")?;
    let real_game = game.canonicalize()?;
    let real_stage = stage.canonicalize()?;
    let mut receipt = Receipt {
        schema_version: Version::V1,
        packs: vec![],
    };
    for kind in KINDS {
        let source = under(pool, kind)?;
        let real = source.canonicalize().with_context(|| {
            format!(
                "open shared {kind} at {}; reconnect the directory or run mcdocker resources clear",
                source.display()
            )
        })?;
        ensure!(
            ![&real_game, &real_stage]
                .iter()
                .any(|p| p.starts_with(&real) || real.starts_with(p)),
            "shared {kind} overlaps instance or launch data"
        );
        let packs = fs::read_dir(&source)?.collect::<std::io::Result<Vec<_>>>()?;
        if packs.is_empty() {
            continue;
        }
        let target = under(&game, kind)?;
        fs::create_dir_all(&target)?;
        let mut occupied = fs::read_dir(&target)?
            .map(|e| Ok(e?.file_name().to_string_lossy().to_lowercase()))
            .collect::<std::io::Result<BTreeSet<_>>>()?;
        let originals = packs
            .iter()
            .map(|e| e.file_name().to_string_lossy().to_lowercase())
            .collect();
        for entry in packs {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("invalid shared pack filename"))?;
            crate::storage::paths::component(&name)?;
            let name = pack_name(
                &name,
                entry.file_type()?.is_file(),
                &mut occupied,
                &originals,
            );
            let staged = stage.join(receipt.packs.len().to_string());
            let digest = pack_digest(&entry.path(), Some(&staged))
                .with_context(|| format!("prepare shared pack {}", entry.path().display()))?;
            receipt.packs.push(Pack {
                kind: kind.into(),
                name,
                digest,
            });
        }
    }
    if receipt.packs.is_empty() {
        return Ok(());
    }
    let journal = receipt_path(instance)?;
    write_json(&journal, &receipt)?;
    for (i, pack) in receipt.packs.iter().enumerate() {
        let target = game.join(&pack.kind).join(&pack.name);
        let staged = tempfile::TempPath::try_from_path(stage.join(i.to_string()))?;
        if let Err(error) = staged.persist_noclobber(&target) {
            // A file may have appeared in the game folder since we took its listing.
            // Only entries already published belong to this run.
            receipt.packs.truncate(i);
            write_json(&journal, &receipt)?;
            cleanup(instance)?;
            return Err(error.error)
                .with_context(|| format!("publish shared pack {}", target.display()));
        }
    }
    Ok(())
}

pub(crate) fn with_packs<T>(
    instance: &Path,
    view: &Path,
    launch: impl FnOnce() -> Result<T>,
) -> Result<T> {
    cleanup(instance)?;
    let Some(pool) = current()? else {
        return launch();
    };
    let stage = tempfile::Builder::new().prefix("packs-").tempdir_in(view)?;
    prepare(instance, stage.path(), &pool)?;
    let result = launch();
    cleanup(instance)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_instances_share_packs_and_keep_private_changes() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let pool = tmp.path().join("公共包");
        let source = pool.join("resourcepacks/材质 A.zip");
        fs::create_dir_all(source.parent().unwrap())?;
        fs::write(&source, b"shared")?;
        fs::create_dir_all(pool.join("shaderpacks/Sky/shaders"))?;
        fs::write(pool.join("shaderpacks/Sky/shaders/test.fsh"), b"shader")?;
        let a = tmp.path().join("work-a/instances/first");
        let b = tmp.path().join("work-b/instances/second");
        let local = a.join("game/resourcepacks");
        fs::create_dir_all(&local)?;
        fs::create_dir_all(b.join("game"))?;
        fs::write(local.join("材质 A.zip"), b"local")?;
        fs::write(local.join("材质 A (shared).zip"), b"another local")?;

        for instance in [&a, &b] {
            let stage = tempfile::tempdir_in(tmp.path())?;
            prepare(instance, stage.path(), &pool)?;
            assert_eq!(
                fs::read(instance.join("game/shaderpacks/Sky/shaders/test.fsh"))?,
                b"shader"
            );
        }
        let alias = local.join("材质 A (shared 2).zip");
        assert_eq!(fs::read(&alias)?, b"shared");
        assert_eq!(
            fs::read(b.join("game/resourcepacks/材质 A.zip"))?,
            b"shared"
        );
        fs::write(b.join("game/shaderpacks/Sky/new.txt"), b"instance edit")?;
        cleanup(&a)?;
        cleanup(&b)?;
        assert!(!alias.exists());
        assert_eq!(fs::read(local.join("材质 A.zip"))?, b"local");
        assert_eq!(
            fs::read(local.join("材质 A (shared).zip"))?,
            b"another local"
        );
        assert!(!a.join("game/shaderpacks/Sky").exists());
        assert!(b.join("game/shaderpacks/Sky/new.txt").is_file());
        assert!(!pool.join("shaderpacks/Sky/new.txt").exists());

        fs::write(source, b"updated")?;
        let stage = tempfile::tempdir_in(tmp.path())?;
        prepare(&b, stage.path(), &pool)?;
        assert_eq!(
            fs::read(b.join("game/resourcepacks/材质 A.zip"))?,
            b"updated"
        );
        assert!(
            b.join("game/shaderpacks/Sky (shared)/shaders/test.fsh")
                .is_file()
        );
        cleanup(&b)?;
        Ok(())
    }
}
