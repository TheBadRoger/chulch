use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug)]
pub struct Java {
    pub executable: PathBuf,
    pub version: String,
}

fn probe(executable: PathBuf, major: u32) -> Result<Java> {
    let check = Command::new(&executable)
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .with_context(|| format!("probe {}", executable.display()))?;
    ensure!(check.status.success(), "Java version probe failed");
    let props = String::from_utf8_lossy(&check.stderr);
    let value = |key: &str| {
        props
            .lines()
            .filter_map(|line| line.trim().split_once(" = "))
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
    };
    let spec = value("java.specification.version").unwrap_or("");
    ensure!(
        spec.strip_prefix("1.").unwrap_or(spec).parse::<u32>() == Ok(major)
            && matches!(value("os.arch"), Some("amd64" | "x86_64"))
            && value("os.name").is_some_and(|v| v.starts_with("Windows")),
        "{} does not provide Windows x64 Java {major}",
        executable.display()
    );
    Ok(Java {
        executable,
        version: value("java.runtime.version")
            .or_else(|| value("java.version"))
            .context("missing JVM version")?
            .to_owned(),
    })
}

fn installations(dir: &Path, candidates: &mut Vec<PathBuf>) {
    if let Ok(children) = fs::read_dir(dir) {
        let mut paths: Vec<_> = children
            .flatten()
            .map(|entry| entry.path().join("bin/java.exe"))
            .collect();
        paths.sort();
        candidates.extend(paths);
    }
}

fn candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut homes = Vec::new();
    if let Some(home) = env::var_os("JAVA_HOME") {
        homes.push(PathBuf::from(home));
    }
    for (name, value) in env::vars_os() {
        let key = name.to_string_lossy().to_ascii_uppercase();
        if key.starts_with("JAVA") && key.ends_with("_HOME") && key != "JAVA_HOME" {
            homes.push(value.into());
        }
    }
    for bin in
        env::split_paths(&env::var_os("PATH").unwrap_or_default()).filter(|p| p.is_absolute())
    {
        if bin.join("java.exe").is_file() {
            paths.push(bin.join("java.exe"));
            if bin
                .file_name()
                .is_some_and(|s| s.eq_ignore_ascii_case("bin"))
                && let Some(home) = bin.parent()
            {
                homes.push(home.into());
            }
        }
    }
    let mut scanned = HashSet::new();
    for home in homes.iter().filter(|p| p.is_absolute()) {
        paths.push(home.join("bin/java.exe"));
        if let Some(parent) = home.parent()
            && scanned.insert(parent)
        {
            installations(parent, &mut paths);
        }
    }
    if let Some(pf) = env::var_os("ProgramFiles") {
        for vendor in [
            "Java",
            "Eclipse Adoptium",
            "Microsoft",
            "Amazon Corretto",
            "Zulu",
        ] {
            installations(&Path::new(&pf).join(vendor), &mut paths);
        }
    }
    if let Some(home) = env::var_os("USERPROFILE") {
        installations(&Path::new(&home).join(".jdks"), &mut paths);
    }
    for (key, sub) in [
        ("APPDATA", ".minecraft/runtime"),
        ("ProgramFiles(x86)", "Minecraft Launcher/runtime"),
    ] {
        if let Some(base) = env::var_os(key)
            && let Ok(entries) = fs::read_dir(Path::new(&base).join(sub))
        {
            for entry in entries.flatten() {
                installations(&entry.path().join("windows-x64"), &mut paths);
            }
        }
    }
    paths
}

// Numeric components precede the build number. Prefer a final build over an EA
// build of the same release, and compare vendor suffixes only through the build.
fn rank(version: &str) -> (Vec<u32>, bool, u32) {
    let (release, build) = version.split_once('+').unwrap_or((version, "0"));
    let (numbers, preview) = release
        .split_once('-')
        .map_or((release, false), |(n, _)| (n, true));
    let mut numbers: Vec<_> = numbers
        .split(['.', '_'])
        .map_while(|n| n.parse::<u32>().ok())
        .collect();
    while numbers.last() == Some(&0) {
        numbers.pop();
    }
    let build = build
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    (numbers, !preview, build)
}

pub fn select(root: &Path, major: u32, explicit: Option<&Path>, acquire: bool) -> Result<Java> {
    if let Some(path) = explicit {
        return probe(std::path::absolute(path)?, major);
    }
    let mut seen = HashSet::new();
    let mut best: Option<Java> = None;
    let cached = crate::paths::under(root, &format!("runtimes/java-{major}/bin/java.exe"))?;
    for path in candidates().into_iter().chain([cached.clone()]) {
        if !path.is_file() || !seen.insert(path.canonicalize().unwrap_or_else(|_| path.clone())) {
            continue;
        }
        if let Ok(found) = probe(path, major)
            && best
                .as_ref()
                .is_none_or(|old| rank(&found.version) > rank(&old.version))
        {
            best = Some(found);
        }
    }
    if let Some(found) = best {
        return Ok(found);
    }
    if acquire {
        let dir = crate::paths::under(root, "runtimes")?;
        fs::create_dir_all(&dir)?;
        let _lock = crate::lock(&crate::paths::under(&dir, "install.lock")?, true)?;
        if !cached.try_exists()? {
            install_jre(&dir, major)?;
        }
        return probe(cached, major);
    }
    bail!("cannot find Windows x64 Java {major}; set JAVA_HOME or pass --java <path-to-java.exe>")
}

fn install_jre(runtimes: &Path, major: u32) -> Result<()> {
    use crate::{
        archive,
        network::{Network, transfer},
        paths::{hash_id, under},
    };
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Release {
        binary: Binary,
    }
    #[derive(Deserialize)]
    struct Binary {
        package: Package,
    }
    #[derive(Deserialize)]
    struct Package {
        link: String,
        checksum: String,
        size: u64,
    }
    let net = Network::new()?;
    let url = format!(
        "https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture=x64&image_type=jre&os=windows&vendor=eclipse"
    );
    let releases: Vec<Release> = net.metadata(&url)?;
    let package = &releases
        .first()
        .with_context(|| format!("no Windows x64 Temurin JRE for Java {major}"))?
        .binary
        .package;
    hash_id(&package.checksum, 64)?;
    eprintln!(
        "Downloading Windows x64 Temurin Java {major} ({} bytes)",
        package.size
    );
    let stage = tempfile::Builder::new()
        .prefix("install-")
        .tempdir_in(runtimes)?;
    let zip_path = stage.path().join("runtime.zip");
    let mut urls = vec![];
    if let Some(name) = package.link.rsplit('/').next().filter(|s| {
        s.starts_with("OpenJDK")
            && s.ends_with(".zip")
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
    }) {
        urls.push(format!(
            "https://mirrors.tuna.tsinghua.edu.cn/Adoptium/{major}/jre/x64/windows/{name}"
        ));
    }
    urls.push(package.link.clone());
    net.download_checked(&urls, &zip_path, Some(package.size), None, None, |file| {
        ensure!(
            crate::store::fingerprint(file)?.sha256 == package.checksum,
            "Temurin package SHA-256 mismatch"
        );
        Ok(())
    })?;
    let mut zip = zip::ZipArchive::new(fs::File::open(zip_path)?)?;
    let (files, _) = archive::index(&mut zip)?;
    let mut homes = files.keys().filter_map(|p| p.strip_suffix("/bin/java.exe"));
    let prefix = homes
        .next()
        .context("runtime archive contains no Java executable")?;
    ensure!(homes.next().is_none(), "ambiguous runtime archive");
    let prefix = format!("{prefix}/");
    let home = stage.path().join("home");
    for (name, index) in files {
        let relative = name
            .strip_prefix(&prefix)
            .context("unexpected file outside JRE home")?;
        let path = under(&home, relative)?;
        fs::create_dir_all(path.parent().unwrap())?;
        let entry = zip.by_index(index)?;
        let size = entry.size();
        transfer(entry, fs::File::create_new(path)?, Some(size), None, None)?;
    }
    probe(home.join("bin/java.exe"), major)?;
    fs::rename(home, under(runtimes, &format!("java-{major}"))?)
        .context("publish verified Java runtime")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::rank;
    #[test]
    #[ignore = "downloads and runs a verified Temurin JRE in a temporary directory"]
    fn provisions_temurin_jre() -> anyhow::Result<()> {
        let root = tempfile::tempdir_in("target")?;
        super::install_jre(root.path(), 17)?;
        let found = super::probe(root.path().join("java-17/bin/java.exe"), 17)?;
        assert!(found.version.starts_with("17"));
        Ok(())
    }
    #[test]
    fn compares_minor_and_build_numbers_numerically() {
        assert!(rank("21.0.12+2") > rank("21.0.9+10-LTS"));
        assert!(rank("21.0.12+11-LTS") > rank("21.0.12+3"));
        assert!(rank("21+1") > rank("21-ea+20"));
    }
}
