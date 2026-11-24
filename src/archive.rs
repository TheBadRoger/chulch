use crate::{network::MAX_FILE, paths::component};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
};
use zip::ZipArchive;

pub(crate) const MAX_TOTAL: u64 = 4 * 1024 * 1024 * 1024;
pub(crate) const MAX_ENTRIES: usize = 65_536;

pub(crate) fn validate(path: &str) -> Result<()> {
    for part in path.split('/') {
        component(part)?;
    }
    Ok(())
}

// Validate all names, types and declared sizes before materializing any entry.
pub(crate) fn index(zip: &mut ZipArchive<File>) -> Result<(BTreeMap<String, usize>, u64)> {
    ensure!(zip.len() <= MAX_ENTRIES, "too many ZIP entries");
    let mut entries = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let path = entry.name().strip_suffix('/').unwrap_or(entry.name());
        validate(path)?;
        ensure!(
            entry
                .unix_mode()
                .is_none_or(|m| matches!(m & 0o170000, 0 | 0o040000 | 0o100000)),
            "unsupported ZIP entry type"
        );
        ensure!(
            seen.insert(path.to_uppercase()),
            "duplicate ZIP path: {path}"
        );
        ensure!(entry.size() <= MAX_FILE, "oversized ZIP entry: {path}");
        total = total
            .checked_add(entry.size())
            .context("ZIP size overflow")?;
        ensure!(
            total <= MAX_TOTAL,
            "ZIP expands beyond the installation limit"
        );
        if !entry.is_dir() {
            entries.insert(path.into(), i);
        }
    }
    Ok((entries, total))
}
