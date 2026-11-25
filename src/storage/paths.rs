use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub(crate) fn component(name: &str) -> Result<()> {
    let base = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_uppercase();
    let device = matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        base.strip_prefix(prefix).is_some_and(|n| {
            matches!(
                n,
                "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    });
    ensure!(
        !name.is_empty()
            && !name.ends_with(['.', ' '])
            && !device
            && !name
                .chars()
                .any(|c| c.is_control() || r#"\/:"<>|?*~"#.contains(c)),
        "unsafe Windows path component {name:?}"
    );
    Ok(())
}

pub(crate) fn linked(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        std::os::windows::fs::MetadataExt::file_attributes(meta) & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.is_symlink()
    }
}

pub(crate) fn no_link(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            ensure!(
                !linked(&meta),
                "reparse point or symlink at {}",
                path.display()
            );
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| path.display().to_string()),
    }
    Ok(())
}

/// The caller chooses the root. All paths below that boundary are untrusted.
pub(crate) fn under(root: &Path, relative: &str) -> Result<PathBuf> {
    no_link(root)?;
    let mut path = root.to_owned();
    for name in relative.split('/') {
        component(name)?;
        path.push(name);
        no_link(&path)?;
    }
    Ok(path)
}

pub(crate) fn hash_id(value: &str, len: usize) -> Result<()> {
    ensure!(
        value.len() == len
            && value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "invalid digest {value:?}"
    );
    Ok(())
}
