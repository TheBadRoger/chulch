use anyhow::{Context, Result, bail, ensure};
use ring::digest::{self, SHA1_FOR_LEGACY_USE_ONLY, SHA512};
use serde::de::DeserializeOwned;
use std::{
    env, fs,
    io::{Read, Write},
    path::Path,
    time::Duration,
};
use tempfile::NamedTempFile;

pub(crate) const MAX_FILE: u64 = 512 * 1024 * 1024;
const META: u64 = 16 * 1024 * 1024;
const BMCLAPI: &str = "https://bmclapi2.bangbang93.com";

pub(crate) struct Network {
    agent: ureq::Agent,
    mirror: String,
}

impl Network {
    pub fn new() -> Result<Self> {
        let mirror = env::var("MCDOCKER_MIRROR").unwrap_or_else(|_| BMCLAPI.into());
        let mirror = mirror.trim_end_matches('/');
        let uri: ureq::http::Uri = mirror.parse().context("invalid MCDOCKER_MIRROR")?;
        ensure!(
            uri.scheme_str() == Some("https")
                && uri.authority().is_some_and(|a| !a.as_str().contains('@'))
                && uri.query().is_none()
                && !mirror.contains('#'),
            "MCDOCKER_MIRROR must be an HTTPS base URL"
        );
        Ok(Self {
            mirror: mirror.to_owned(),
            agent: ureq::Agent::config_builder()
                .https_only(true)
                .timeout_connect(Some(Duration::from_secs(15)))
                .timeout_recv_response(Some(Duration::from_secs(45)))
                .timeout_per_read(Some(Duration::from_secs(60)))
                .max_idle_connections_per_host(16)
                .build()
                .into(),
        })
    }
    pub fn maven_mirror(&self) -> String {
        format!("{}/maven/", self.mirror)
    }
    fn body(&self, url: &str) -> Result<ureq::Body> {
        let uri: ureq::http::Uri = url.parse().context("invalid download URL")?;
        ensure!(
            uri.scheme_str() == Some("https")
                && uri.authority().is_some_and(|a| !a.as_str().contains('@')),
            "a public HTTPS download URL is required"
        );
        self.agent
            .get(url)
            .header("User-Agent", "mcdocker/0.1")
            .call()
            .map(|r| r.into_body())
            .with_context(|| format!("request to {} failed", uri.host().unwrap_or("upstream")))
    }
    pub fn metadata<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let mut body = self.body(url)?;
        Ok(body.with_config().limit(META).read_json()?)
    }
    pub fn installer_checksum(&self, url: &str) -> Result<String> {
        let text = self.body(url)?.with_config().limit(256).read_to_string()?;
        let hash = text
            .split_whitespace()
            .next()
            .context("empty installer checksum")?
            .to_ascii_lowercase();
        crate::paths::hash_id(&hash, 40)?;
        Ok(hash)
    }
    pub fn curse_file<T: DeserializeOwned>(&self, project: u64, file: u64, key: &str) -> Result<T> {
        let url = format!("https://api.curseforge.com/v1/mods/{project}/files/{file}");
        let mut response = self
            .agent
            .get(&url)
            .config()
            .max_redirects(0)
            .build()
            .header("x-api-key", key)
            .call()
            .map_err(|_| anyhow::anyhow!("CurseForge lookup failed for {project}/{file}"))?;
        ensure!(
            response.status().is_success(),
            "CurseForge lookup failed for {project}/{file}"
        );
        Ok(response.body_mut().with_config().limit(META).read_json()?)
    }
    pub fn download(
        &self,
        urls: &[String],
        dest: &Path,
        size: Option<u64>,
        sha1: Option<&str>,
        sha512: Option<&str>,
    ) -> Result<()> {
        self.download_checked(urls, dest, size, sha1, sha512, |_| Ok(()))
    }

    pub fn download_checked(
        &self,
        urls: &[String],
        dest: &Path,
        size: Option<u64>,
        sha1: Option<&str>,
        sha512: Option<&str>,
        verify: impl Fn(&Path) -> Result<()>,
    ) -> Result<()> {
        let parent = dest.parent().context("missing file parent")?;
        fs::create_dir_all(parent)?;
        let mut last = String::from("no download URLs");
        for (index, url) in urls.iter().enumerate() {
            let fetch = || -> Result<()> {
                let mut tmp = NamedTempFile::new_in(parent)?;
                transfer(self.body(url)?.as_reader(), &mut tmp, size, sha1, sha512)?;
                verify(tmp.path())?;
                tmp.persist_noclobber(dest)?;
                Ok(())
            };
            let mut result = fetch();
            if let Err(e) = &result
                && transient(e)
            {
                result = fetch();
            }
            match result {
                Ok(()) => {
                    if (index > 0 || env::var_os("MCDOCKER_TRACE_DOWNLOADS").is_some())
                        && let Ok(uri) = url.parse::<ureq::http::Uri>()
                    {
                        eprintln!("Downloaded from {}", uri.host().unwrap_or("upstream"));
                    }
                    return Ok(());
                }
                Err(e) => {
                    last = e.to_string();
                    eprintln!("Download candidate failed: {last}");
                }
            }
        }
        bail!("cannot obtain {}: {last}", dest.display())
    }
    pub fn game_file(
        &self,
        url: &str,
        dest: &Path,
        size: Option<u64>,
        sha1: Option<&str>,
    ) -> Result<()> {
        let mut urls = game_urls(url, sha1.is_some(), &self.mirror);
        if sha1.is_some()
            && let Some(path) = url.strip_prefix("https://libraries.minecraft.net/")
        {
            urls.push(format!("https://repo.maven.apache.org/maven2/{path}"));
        }
        self.download(&urls, dest, size, sha1, None)
    }

    pub fn mod_file(
        &self,
        urls: &[String],
        dest: &Path,
        size: u64,
        sha1: Option<&str>,
        sha512: Option<&str>,
    ) -> Result<()> {
        let mut candidates = Vec::new();
        for url in urls {
            if (sha1.is_some() || sha512.is_some())
                && let Some(mirror) = mod_mirror(url)
            {
                candidates.push(mirror);
            }
            candidates.push(url.clone());
        }
        self.download(&candidates, dest, Some(size), sha1, sha512)
    }

    pub fn loader_installer(
        &self,
        loader: crate::Loader,
        version: &str,
        url: &str,
        dest: &Path,
    ) -> Result<()> {
        // The checksum must come from a distribution channel independent of BMCLAPI.
        let cdn = url
            .strip_prefix("https://maven.neoforged.net/releases/")
            .map(|path| format!("https://neoforged.forgecdn.net/releases/{path}"));
        let checksum = |base: &str| self.installer_checksum(&format!("{base}.sha1"));
        let hash = match &cdn {
            Some(distribution) => checksum(distribution).or_else(|_| checksum(url))?,
            None => checksum(url)?,
        };
        let mut urls = game_urls(url, true, &self.mirror);
        if loader == crate::Loader::NeoForge {
            urls.insert(
                0,
                format!(
                    "{}/neoforge/version/{version}/download/installer.jar",
                    self.mirror
                ),
            );
        }
        self.download(&urls, dest, None, Some(&hash), None)
    }
}

fn game_urls(url: &str, verified: bool, base: &str) -> Vec<String> {
    let mut urls = vec![];
    if verified && !url.contains(['?', '#']) {
        for (prefix, route) in [
            ("https://piston-data.mojang.com/", "/"),
            ("https://piston-meta.mojang.com/", "/"),
            ("https://libraries.minecraft.net/", "/maven/"),
            ("https://resources.download.minecraft.net/", "/assets/"),
            ("https://maven.fabricmc.net/", "/maven/"),
            ("https://maven.minecraftforge.net/", "/maven/"),
            ("https://maven.neoforged.net/releases/", "/maven/"),
        ] {
            if let Some(suffix) = url.strip_prefix(prefix) {
                urls.push(format!("{base}{route}{suffix}"));
                break;
            }
        }
        if let Some(path) = url.strip_prefix("https://maven.neoforged.net/releases/") {
            urls.push(format!("https://neoforged.forgecdn.net/releases/{path}"));
        }
    }
    urls.push(url.to_owned());
    urls
}

fn mod_mirror(url: &str) -> Option<String> {
    if url.contains(['?', '#']) {
        return None;
    }
    let path = url
        .strip_prefix("https://cdn.modrinth.com/data/")
        .map(|p| format!("data/{p}"))
        .or_else(|| {
            url.strip_prefix("https://edge.forgecdn.net/files/")
                .map(|p| format!("files/{p}"))
        })?;
    Some(format!("https://mod.mcimirror.top/{path}"))
}

fn transient(error: &anyhow::Error) -> bool {
    if let Some(e) = error.downcast_ref::<ureq::Error>() {
        return matches!(
            e,
            ureq::Error::Timeout(_)
                | ureq::Error::ConnectionFailed
                | ureq::Error::Io(_)
                | ureq::Error::StatusCode(500..=599)
        );
    }
    error.downcast_ref::<std::io::Error>().is_some_and(|e| {
        matches!(
            e.kind(),
            std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::ConnectionReset
        )
    })
}

pub(crate) fn parallel<T: Sync>(
    items: &[T],
    process: impl Fn(&T) -> Result<()> + Sync,
) -> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let mut workers = vec![];
        for _ in 0..items.len().min(12) {
            workers.push(scope.spawn(|| -> Result<()> {
                while let Some(item) = items.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if let Err(err) = process(item) {
                        next.store(items.len(), Ordering::Relaxed);
                        return Err(err);
                    }
                }
                Ok(())
            }));
        }
        for worker in workers {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("download worker failed"))??;
        }
        Ok(())
    })
}

pub(crate) fn transfer(
    mut input: impl Read,
    mut out: impl Write,
    size: Option<u64>,
    sha1: Option<&str>,
    sha512: Option<&str>,
) -> Result<()> {
    let limit = size.unwrap_or(MAX_FILE);
    ensure!(limit <= MAX_FILE, "file exceeds installation limit");
    let mut h1 = sha1.map(|_| digest::Context::new(&SHA1_FOR_LEGACY_USE_ONLY));
    let mut h512 = sha512.map(|_| digest::Context::new(&SHA512));
    let mut buffer = [0; 65536];
    let mut count = 0;
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        ensure!(
            count <= limit,
            "file exceeds declared size or installation limit"
        );
        if let Some(h) = &mut h1 {
            h.update(&buffer[..n]);
        }
        if let Some(h) = &mut h512 {
            h.update(&buffer[..n]);
        }
        out.write_all(&buffer[..n])?;
    }
    ensure!(size.is_none_or(|s| s == count), "truncated file");
    for (hasher, expected) in [(h1, sha1), (h512, sha512)] {
        if let (Some(h), Some(expected)) = (hasher, expected) {
            let actual: String = h
                .finish()
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            ensure!(
                actual.eq_ignore_ascii_case(expected),
                "source checksum mismatch"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod mirror_tests {
    use super::*;
    #[test]
    fn mod_routes_only_cover_public_cdn_paths() {
        assert_eq!(
            mod_mirror("https://cdn.modrinth.com/data/abc/file.jar").as_deref(),
            Some("https://mod.mcimirror.top/data/abc/file.jar")
        );
        assert_eq!(
            mod_mirror("https://edge.forgecdn.net/files/1/2/m.jar").as_deref(),
            Some("https://mod.mcimirror.top/files/1/2/m.jar")
        );
        for url in [
            "https://api.curseforge.com/v1/mods/1",
            "https://mediafilez.forgecdn.net/files/1/x.jar",
            "https://cdn.modrinth.com/data/x.jar?token=abc",
        ] {
            assert!(mod_mirror(url).is_none());
        }
    }

    #[test]
    fn verified_public_files_prefer_the_configured_mirror_and_keep_the_origin() {
        let base = "https://mirror.example/minecraft";
        let url = "https://resources.download.minecraft.net/ab/abcdef";
        assert_eq!(
            game_urls(url, true, base),
            [format!("{base}/assets/ab/abcdef"), url.to_owned()]
        );
        for upstream in [
            "https://maven.minecraftforge.net/",
            "https://maven.fabricmc.net/",
        ] {
            let url = format!("{upstream}group/artifact.jar");
            assert_eq!(
                game_urls(&url, true, base),
                [format!("{base}/maven/group/artifact.jar"), url]
            );
        }
        let neo = "https://maven.neoforged.net/releases/group/tool.jar";
        assert_eq!(
            game_urls(neo, true, base),
            [
                format!("{base}/maven/group/tool.jar"),
                "https://neoforged.forgecdn.net/releases/group/tool.jar".to_owned(),
                neo.to_owned()
            ]
        );
        assert_eq!(game_urls(url, false, base), [url]);
        for url in [
            "https://cdn.modrinth.com/data/a.jar",
            "https://libraries.minecraft.net/a.jar?signature=123",
        ] {
            assert_eq!(game_urls(url, true, base), [url]);
        }
    }
}
