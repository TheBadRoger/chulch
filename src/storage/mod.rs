pub(crate) mod archive;
pub(crate) mod paths;
pub(crate) mod store;

use anyhow::{Context, Result};
use std::{fs::File, path::Path};

pub(crate) fn lock(path: &Path, wait: bool) -> Result<File> {
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
