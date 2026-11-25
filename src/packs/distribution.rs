use crate::{
    instances::{
        self,
        install::{install, separate_source},
    },
    packs::metadata::{self, Layer, PackFile, PackRole, Recipe},
    packs::network::{MAX_FILE, Network, transfer},
    storage::paths::{component, hash_id, under},
    storage::store,
};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
use zip::ZipArchive;

use crate::storage::archive::{MAX_ENTRIES, MAX_TOTAL, index, validate};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Modrinth {
    format_version: u32,
    game: String,
    dependencies: BTreeMap<String, String>,
    files: Vec<Remote>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Remote {
    path: String,
    hashes: Hashes,
    file_size: u64,
    downloads: Vec<String>,
    env: Option<Environment>,
}
#[derive(Deserialize)]
struct Hashes {
    sha1: String,
    sha512: String,
}
#[derive(Deserialize)]
struct Environment {
    client: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Curse {
    manifest_type: String,
    manifest_version: u32,
    minecraft: CurseGame,
    files: Vec<CurseRef>,
    overrides: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseGame {
    version: String,
    mod_loaders: Vec<Loader>,
}
#[derive(Deserialize)]
struct Loader {
    id: String,
}
#[derive(Deserialize)]
struct CurseRef {
    #[serde(rename = "projectID")]
    project: u64,
    #[serde(rename = "fileID")]
    file: u64,
    required: bool,
}
#[derive(Deserialize)]
struct ApiResponse {
    data: CurseFile,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseFile {
    id: u64,
    mod_id: u64,
    file_name: String,
    file_length: u64,
    download_url: Option<String>,
    hashes: Vec<CurseHash>,
}
#[derive(Deserialize)]
struct CurseHash {
    algo: u32,
    value: String,
}

fn public_url(url: &str) -> Option<String> {
    let uri: ureq::http::Uri = url.parse().ok()?;
    let host = uri.authority()?.as_str();
    (uri.scheme_str() == Some("https") && !host.contains('@'))
        .then(|| format!("https://{host}{}", uri.path()))
}

struct Input<'a> {
    zip: ZipArchive<File>,
    entries: BTreeMap<String, usize>,
    files: BTreeMap<String, PackFile>,
    scratch: &'a Path,
    manual: Option<&'a Path>,
    optional: bool,
    net: &'a Network,
    total: u64,
}

impl Input<'_> {
    fn json<T: serde::de::DeserializeOwned>(&mut self, name: &str) -> Result<T> {
        let mut file = self.zip.by_index(self.entries[name])?;
        ensure!(file.size() <= 16 * 1024 * 1024, "oversized pack manifest");
        let mut bytes = vec![];
        let n = file
            .by_ref()
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(n as u64 == file.size(), "manifest size mismatch");
        Ok(serde_json::from_slice(&bytes)?)
    }
    fn insert(&mut self, path: String, mut file: PackFile, replace: bool) -> Result<()> {
        validate(&path)?;
        let key = path.to_uppercase();
        if let Some(prev) = self.files.get(&key) {
            ensure!(
                replace && prev.target == path,
                "pack path collision: {path}"
            );
        }
        file.role = if path.starts_with("mods/")
            && path.ends_with(".jar")
            && path.matches('/').count() == 1
        {
            PackRole::Mod
        } else {
            PackRole::Override
        };
        self.files.insert(key, file);
        Ok(())
    }
    fn overrides(&mut self, dir: &str) -> Result<()> {
        validate(dir)?;
        let prefix = format!("{dir}/");
        let entries: Vec<_> = self
            .entries
            .range(prefix.clone()..)
            .take_while(|(path, _)| path.starts_with(&prefix))
            .map(|(path, &i)| (path.clone(), i))
            .collect();
        for (path, i) in entries {
            let target = path[prefix.len()..].to_owned();
            let mut entry = self.zip.by_index(i)?;
            let source = format!("zip-{i}");
            let size = entry.size();
            transfer(
                &mut entry,
                File::create_new(self.scratch.join(&source))?,
                Some(size),
                None,
                None,
            )?;
            drop(entry);
            self.insert(
                target.clone(),
                PackFile {
                    source,
                    target,
                    origin: Some(path),
                    source_urls: vec![],
                    role: PackRole::Override,
                    sha1: None,
                    size: Some(size),
                    sha512: None,
                },
                true,
            )?;
        }
        Ok(())
    }
    fn reserve(&mut self, size: u64) -> Result<()> {
        self.total = self.total.checked_add(size).context("pack size overflow")?;
        ensure!(
            size <= MAX_FILE && self.total <= MAX_TOTAL,
            "pack exceeds installation size limit"
        );
        Ok(())
    }
    fn modrinth(&mut self) -> Result<Recipe> {
        let pack: Modrinth = self.json("modrinth.index.json")?;
        ensure!(
            pack.game == "minecraft" && pack.format_version == 1,
            "unsupported Modrinth format"
        );
        let mc = pack
            .dependencies
            .get("minecraft")
            .context("missing Minecraft dependency")?;
        let (loader, version) = [
            ("fabric-loader", crate::Loader::Fabric),
            ("forge", crate::Loader::Forge),
            ("neoforge", crate::Loader::NeoForge),
        ]
        .into_iter()
        .find_map(|(key, kind)| pack.dependencies.get(key).map(|v| (kind, v)))
        .context("expected Fabric, Forge or NeoForge")?;
        ensure!(
            pack.dependencies.len() == 2,
            "unsupported additional pack dependency"
        );
        let recipe = Recipe::installed(mc, loader, version)?;
        ensure!(pack.files.len() <= MAX_ENTRIES, "too many remote files");
        for (i, f) in pack.files.into_iter().enumerate() {
            validate(&f.path)?;
            match f
                .env
                .as_ref()
                .map(|e| e.client.as_str())
                .unwrap_or("required")
            {
                "unsupported" => continue,
                "optional" if !self.optional => continue,
                "optional" | "required" => {}
                _ => bail!("invalid client environment"),
            }
            self.reserve(f.file_size)?;
            hash_id(&f.hashes.sha1, 40)?;
            hash_id(&f.hashes.sha512, 128)?;
            let source = format!("modrinth-{i}");
            let dest = self.scratch.join(&source);
            let local = self.manual.map(|dir| under(dir, &f.path)).transpose()?;
            if let Some(p) = local.filter(|p| p.is_file()) {
                transfer(
                    File::open(p)?,
                    File::create_new(&dest)?,
                    Some(f.file_size),
                    Some(&f.hashes.sha1),
                    Some(&f.hashes.sha512),
                )?;
            } else {
                self.net
                    .mod_file(
                        &f.downloads,
                        &dest,
                        f.file_size,
                        Some(&f.hashes.sha1),
                        Some(&f.hashes.sha512),
                    )
                    .with_context(|| {
                        format!("supply {} under --manual to install offline", f.path)
                    })?;
            }
            // Transport query strings can contain signed credentials.
            let origin = format!("modrinth:{}", f.path);
            let path = f.path;
            self.insert(
                path.clone(),
                PackFile {
                    source,
                    target: path,
                    origin: Some(origin),
                    source_urls: f.downloads.iter().filter_map(|s| public_url(s)).collect(),
                    role: PackRole::Override,
                    sha1: Some(f.hashes.sha1),
                    sha512: Some(f.hashes.sha512),
                    size: Some(f.file_size),
                },
                false,
            )?;
        }
        self.overrides("overrides")?;
        self.overrides("client-overrides")?;
        Ok(recipe)
    }
    fn curse(&mut self) -> Result<Recipe> {
        let pack: Curse = self.json("manifest.json")?;
        ensure!(
            pack.manifest_type == "minecraftModpack" && pack.manifest_version == 1,
            "unsupported CurseForge manifest"
        );
        ensure!(pack.minecraft.mod_loaders.len() == 1, "expected one loader");
        let (kind, version) = pack.minecraft.mod_loaders[0]
            .id
            .split_once('-')
            .context("invalid CurseForge loader")?;
        let loader = match kind {
            "fabric" => crate::Loader::Fabric,
            "forge" => crate::Loader::Forge,
            "neoforge" => crate::Loader::NeoForge,
            _ => bail!("unsupported CurseForge loader {kind}"),
        };
        let recipe = Recipe::installed(&pack.minecraft.version, loader, version)?;
        let api_key = env::var("CURSEFORGE_API_KEY")
            .ok()
            .filter(|s| !s.is_empty());
        ensure!(
            pack.files.len() <= MAX_ENTRIES,
            "too many CurseForge entries"
        );
        for (i, f) in pack.files.into_iter().enumerate() {
            if !f.required && !self.optional {
                continue;
            }
            let ids = format!("{}/{}", f.project, f.file);
            let supplied = self.manual.map(|dir| under(dir, &ids)).transpose()?;
            let local = if let Some(dir) = supplied.filter(|dir| dir.is_dir()) {
                let mut jars = vec![];
                for entry in fs::read_dir(&dir)? {
                    let entry = entry?;
                    if entry
                        .path()
                        .extension()
                        .is_some_and(|s| s.eq_ignore_ascii_case("jar"))
                    {
                        let name = entry
                            .file_name()
                            .into_string()
                            .map_err(|_| anyhow::anyhow!("invalid JAR name"))?;
                        let path = under(&dir, &name)?;
                        ensure!(path.is_file(), "expected a JAR file");
                        jars.push((name, path));
                    }
                }
                ensure!(jars.len() <= 1, "supply exactly one JAR for {ids}");
                jars.pop()
            } else {
                None
            };
            let metadata = api_key
                .as_deref()
                .map(|key| self.net.curse_file::<ApiResponse>(f.project, f.file, key))
                .transpose()?
                .map(|r| r.data);
            if let Some(m) = &metadata {
                ensure!(
                    m.id == f.file && m.mod_id == f.project,
                    "CurseForge file identity mismatch"
                );
            }
            let sha1 = metadata
                .as_ref()
                .and_then(|m| m.hashes.iter().find(|h| h.algo == 1))
                .map(|h| h.value.to_ascii_lowercase());
            let source = format!("curse-{i}");
            if let Some(m) = &metadata {
                ensure!(
                    m.hashes.is_empty() || sha1.is_some(),
                    "unsupported source checksum for CurseForge {ids}"
                );
            }
            let dest = self.scratch.join(&source);
            let help = format!(
                "missing CurseForge {ids}: set CURSEFORGE_API_KEY or supply --manual <dir> with {ids}/<filename>.jar"
            );
            let source_urls = metadata
                .as_ref()
                .and_then(|m| m.download_url.as_deref())
                .and_then(public_url)
                .into_iter()
                .collect();
            let (name, size) = if let Some((name, path)) = local {
                let size = metadata
                    .as_ref()
                    .map(|m| m.file_length)
                    .unwrap_or(fs::metadata(&path)?.len());
                if let Some(m) = &metadata {
                    ensure!(m.file_name == name, "unexpected file name for {ids}");
                }
                self.reserve(size)?;
                transfer(
                    File::open(path)?,
                    File::create_new(dest)?,
                    Some(size),
                    sha1.as_deref(),
                    None,
                )?;
                (name, size)
            } else {
                let m = metadata.context(help.clone())?;
                let url = m.download_url.context(help)?;
                self.reserve(m.file_length)?;
                self.net
                    .mod_file(&[url], &dest, m.file_length, sha1.as_deref(), None)?;
                (m.file_name, m.file_length)
            };
            component(&name)?;
            ensure!(
                name.to_ascii_lowercase().ends_with(".jar"),
                "unsupported CurseForge non-mod file {ids}"
            );
            let target = format!("mods/{name}");
            self.insert(
                target.clone(),
                PackFile {
                    source,
                    target,
                    role: PackRole::Mod,
                    origin: Some(format!("curseforge:{ids}")),
                    source_urls,
                    sha1,
                    size: Some(size),
                    sha512: None,
                },
                false,
            )?;
        }
        self.overrides(pack.overrides.as_deref().unwrap_or("overrides"))?;
        Ok(recipe)
    }
}

pub fn install_pack(
    root: &Path,
    id: &str,
    archive: &Path,
    source: Option<&Path>,
    manual: Option<&Path>,
    optional: bool,
) -> Result<()> {
    if let Some(dir) = source {
        separate_source(root, dir)?;
    }
    let archive = File::open(archive).context("open modpack archive")?;
    ensure!(
        archive.metadata()?.len() <= MAX_TOTAL,
        "oversized pack archive"
    );
    install(root, id, |scratch| {
        let net = Network::new()?;
        let mut input = Input {
            zip: ZipArchive::new(archive)?,
            entries: BTreeMap::new(),
            files: BTreeMap::new(),
            scratch,
            manual,
            optional,
            net: &net,
            total: 0,
        };
        (input.entries, input.total) = index(&mut input.zip)?;
        let mut recipe = match (
            input.entries.contains_key("modrinth.index.json"),
            input.entries.contains_key("manifest.json"),
        ) {
            (true, false) => input.modrinth()?,
            (false, true) => input.curse()?,
            _ => bail!("expected one Modrinth or CurseForge manifest"),
        };
        recipe.pack_files = input.files.into_values().collect();
        if let Some(source) = source {
            return metadata::resolve(root, source, &recipe, scratch);
        }
        for existing in crate::list(root)? {
            let mut locked = instances::generation(root, &instances::instance(root, &existing)?)?;
            if locked.minecraft == recipe.minecraft
                && locked.loader == recipe.loader
                && locked.loader_version == recipe.loader_version
                && locked.layers.len() == 3
            {
                for layer in &locked.layers[..2] {
                    let layer: Layer = store::record(root, "layers", layer)?;
                    for entry in layer.entries {
                        store::copy_checked(root, &entry.blob, std::io::sink())?;
                    }
                }
                locked.layers[2] = metadata::pack_layer(root, scratch, &recipe, &locked.layers[1])?;
                return Ok(locked);
            }
        }
        if let Some(appdata) = env::var_os("APPDATA") {
            let local = PathBuf::from(appdata).join(".minecraft");
            if recipe.available_in(&local)? {
                return metadata::resolve(root, &local, &recipe, scratch);
            }
        }
        let base = scratch.join("base");
        fs::create_dir(&base)?;
        recipe.download_base(root, &base, &net)?;
        metadata::resolve(root, &base, &recipe, scratch)
    })
}
