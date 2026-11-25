use crate::storage::paths::{hash_id, under};
use anyhow::{Context, Result, ensure};
use ring::digest::{self, SHA1_FOR_LEGACY_USE_ONLY, SHA256};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Blob {
    pub sha256: String,
    pub size: u64,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn id(bytes: &[u8]) -> String {
    hex(digest::digest(&SHA256, bytes).as_ref())
}

pub(crate) fn path(root: &Path, blob: &Blob) -> Result<PathBuf> {
    hash_id(&blob.sha256, 64)?;
    under(
        root,
        &format!("store/blobs/sha256/{}/{}", &blob.sha256[..2], blob.sha256),
    )
}

pub(crate) fn stream(source: &Path, mut output: impl Write, sha1: Option<&str>) -> Result<Blob> {
    let mut input = File::open(source).with_context(|| format!("open {}", source.display()))?;
    let mut internal = digest::Context::new(&SHA256);
    let mut upstream = sha1.map(|_| digest::Context::new(&SHA1_FOR_LEGACY_USE_ONLY));
    let mut size = 0;
    let mut buf = [0; 64 * 1024];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let bytes = &buf[..n];
        internal.update(bytes);
        if let Some(hash) = &mut upstream {
            hash.update(bytes);
        }
        output.write_all(bytes)?;
        size += n as u64;
    }
    if let Some(hash) = upstream {
        ensure!(
            Some(hex(hash.finish().as_ref()).as_str()) == sha1,
            "source SHA-1 mismatch: {}",
            source.display()
        );
    }
    Ok(Blob {
        sha256: hex(internal.finish().as_ref()),
        size,
    })
}

pub(crate) fn copy_checked(root: &Path, blob: &Blob, output: impl Write) -> Result<()> {
    let source = path(root, blob)?;
    ensure!(
        stream(&source, output, None)? == *blob,
        "damaged blob {}",
        blob.sha256
    );
    Ok(())
}

pub(crate) fn fingerprint(path: &Path) -> Result<Blob> {
    stream(path, io::sink(), None)
}

pub(crate) fn ingest(
    root: &Path,
    source: &Path,
    sha1: Option<&str>,
    size: Option<u64>,
) -> Result<Blob> {
    let mut tmp = NamedTempFile::new_in(under(root, "work")?)?;
    let blob = stream(source, &mut tmp, sha1)?;
    ensure!(
        size.is_none_or(|n| n == blob.size),
        "source size mismatch: {}",
        source.display()
    );
    let dest = path(root, &blob)?;
    fs::create_dir_all(dest.parent().unwrap())?;
    if dest.try_exists()? {
        copy_checked(root, &blob, io::sink())?;
    } else {
        tmp.as_file().sync_all()?;
        tmp.persist_noclobber(dest)?;
    }
    Ok(blob)
}

pub(crate) fn publish_json(root: &Path, folder: &str, record: &impl Serialize) -> Result<String> {
    let bytes = serde_json::to_vec(record)?;
    let key = id(&bytes);
    let path = under(root, &format!("{folder}/{key}.json"))?;
    fs::create_dir_all(path.parent().unwrap())?;
    if path.try_exists()? {
        ensure!(fs::read(path)? == bytes, "damaged record {key}");
    } else {
        let mut file = NamedTempFile::new_in(under(root, "work")?)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist_noclobber(path)?;
    }
    Ok(key)
}

pub(crate) fn record<T: serde::de::DeserializeOwned>(
    root: &Path,
    folder: &str,
    key: &str,
) -> Result<T> {
    hash_id(key, 64)?;
    let bytes = fs::read(under(root, &format!("{folder}/{key}.json"))?)?;
    ensure!(id(&bytes) == key, "damaged record {key}");
    Ok(serde_json::from_slice(&bytes)?)
}
