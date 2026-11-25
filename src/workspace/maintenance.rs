use crate::{
    instances::generation, instances::instance, instances::instance_ids, packs::metadata::Layer,
    storage::paths, storage::store,
};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, ErrorKind},
    path::Path,
};

struct Object {
    blob: store::Blob,
    target: String,
    instances: BTreeSet<String>,
}

#[derive(Serialize)]
pub struct Issue {
    pub file: String,
    pub instances: Vec<String>,
    pub reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verification {
    pub instances: usize,
    pub checked_objects: usize,
    pub issues: Vec<Issue>,
}

// Install and uninstall take this lock exclusively. An empty workspace has no lock yet.
fn read_lock(root: &Path) -> Result<Option<File>> {
    match File::open(paths::under(root, "store.lock")?) {
        Ok(file) => {
            file.lock_shared()?;
            Ok(Some(file))
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[derive(Default)]
struct Inventory {
    objects: BTreeMap<String, Object>,
    issues: Vec<Issue>,
}

impl Inventory {
    fn load(root: &Path, ids: &[String]) -> Result<Self> {
        let mut index = Self::default();
        for id in ids {
            let dir = instance(root, id)?;
            let mut pending = match generation(root, &dir) {
                Ok(generation) => generation.layers,
                Err(e) => {
                    index.issue(id, format!("instances/{id}/profile.json"), e);
                    continue;
                }
            };
            let mut seen = BTreeSet::new();
            while let Some(key) = pending.pop() {
                if !seen.insert(key.clone()) {
                    continue;
                }
                let layer: Layer = match store::record(root, "layers", &key) {
                    Ok(layer) => layer,
                    Err(e) => {
                        index.issue(id, format!("layers/{key}.json"), e);
                        continue;
                    }
                };
                pending.extend(layer.dependencies);
                for entry in layer.entries {
                    let size = entry.blob.size;
                    let object = index
                        .objects
                        .entry(entry.blob.sha256.clone())
                        .or_insert_with(|| Object {
                            blob: entry.blob,
                            target: entry.target,
                            instances: BTreeSet::new(),
                        });
                    object.instances.insert(id.clone());
                    if size != object.blob.size {
                        index.issues.push(Issue {
                            file: format!("layers/{key}.json"),
                            instances: vec![id.clone()],
                            reason: format!("conflicting sizes for {}", object.blob.sha256),
                        });
                    }
                }
            }
        }
        Ok(index)
    }

    fn issue(&mut self, id: &str, file: String, error: anyhow::Error) {
        self.issues.push(Issue {
            file,
            instances: vec![id.into()],
            reason: format!("{error:#}"),
        });
    }
}

pub fn verify(root: &Path) -> Result<Verification> {
    let _lock = read_lock(root)?;
    let ids = instance_ids(root)?;
    let Inventory {
        objects,
        mut issues,
    } = Inventory::load(root, &ids)?;
    let checked_objects = objects.len();
    for (hash, object) in objects {
        if let Err(error) = store::copy_checked(root, &object.blob, io::sink()) {
            issues.push(Issue {
                file: format!("{} ({hash})", object.target),
                instances: object.instances.into_iter().collect(),
                reason: format!("{error:#}"),
            });
        }
    }
    Ok(Verification {
        instances: ids.len(),
        checked_objects,
        issues,
    })
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub store_bytes: u64,
    pub referenced_bytes: u64,
    pub saved_bytes: u64,
    pub private_bytes_by_instance: BTreeMap<String, u64>,
    pub runtime_copy_bytes: u64,
}

// Private directories may contain junctions and files that disappear while the game runs.
fn tree_bytes(path: &Path) -> Result<u64> {
    let mut pending = vec![path.to_owned()];
    let mut bytes = 0;
    while let Some(path) = pending.pop() {
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| path.display().to_string()),
        };
        if paths::linked(&meta) {
            continue;
        }
        if meta.is_file() {
            bytes += meta.len();
        } else if meta.is_dir() {
            let entries = match fs::read_dir(&path) {
                Ok(entries) => entries,
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                Err(e) => return Err(e).with_context(|| path.display().to_string()),
            };
            for entry in entries {
                pending.push(entry?.path());
            }
        }
    }
    Ok(bytes)
}

pub fn usage(root: &Path) -> Result<Usage> {
    let _lock = read_lock(root)?;
    let ids = instance_ids(root)?;
    let index = Inventory::load(root, &ids)?;
    if let Some(issue) = index.issues.first() {
        bail!(
            "cannot determine usage for {}: {}: {} (run verify for details)",
            issue.instances.join(", "),
            issue.file,
            issue.reason
        );
    }
    let mut usage = Usage {
        store_bytes: tree_bytes(&paths::under(root, "store/blobs/sha256")?)?,
        ..Usage::default()
    };
    for object in index.objects.values() {
        usage.referenced_bytes = usage
            .referenced_bytes
            .checked_add(object.blob.size)
            .context("referenced size overflow")?;
        let copies = object.instances.len() as u64 - 1;
        let saved = object
            .blob
            .size
            .checked_mul(copies)
            .context("shared size overflow")?;
        usage.saved_bytes = usage
            .saved_bytes
            .checked_add(saved)
            .context("shared size overflow")?;
    }
    for id in ids {
        let bytes = tree_bytes(&paths::under(&instance(root, &id)?, "game")?)?;
        usage.private_bytes_by_instance.insert(id, bytes);
    }
    let work = paths::under(root, "work")?;
    if work.try_exists()? {
        for entry in fs::read_dir(work)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with("run-") {
                usage.runtime_copy_bytes += tree_bytes(&entry.path())?;
            }
        }
    }
    Ok(usage)
}
