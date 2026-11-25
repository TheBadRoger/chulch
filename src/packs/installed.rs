//! Import a local launcher installation without changing its game directory.
use crate::{
    Loader,
    packs::metadata::{self, PackFile, PackRole, Recipe},
    packs::network::Network,
    storage::paths::{component, hash_id, under},
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const GAMEPLAY: &[&str] = &[
    "mods",
    "kubejs",
    "scripts",
    "defaultconfigs",
    "patchouli_books",
    "fancymenu_data",
];
const OPTIONAL: &[&str] = &[
    "saves",
    "config",
    "options.txt",
    "resourcepacks",
    "shaderpacks",
];

#[derive(Default)]
pub struct Options {
    pub saves: bool,
    pub config: bool,
    pub resourcepacks: bool,
    pub shaderpacks: bool,
}

pub struct Client {
    pub name: String,
    pub minecraft: String,
    pub loader: Loader,
    pub version: String,
    pub data_dirs: Vec<PathBuf>,
    root: PathBuf,
    version_dir: PathBuf,
    jar: String,
    game: Option<Value>,
    profile: Value,
}

fn read(path: &Path) -> Result<Value> {
    let file = fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= 16 * 1024 * 1024,
        "oversized version metadata"
    );
    serde_json::from_reader(file)
        .with_context(|| format!("read version manifest {}", path.display()))
}

fn has_data(path: &Path) -> Result<bool> {
    for name in GAMEPLAY.iter().chain(OPTIONAL) {
        if under(path, name)?.try_exists()? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn argument<'a>(profile: &'a Value, key: &str) -> Option<&'a str> {
    profile["arguments"]["game"]
        .as_array()?
        .windows(2)
        .find(|w| w[0].as_str().is_some_and(|s| s.eq_ignore_ascii_case(key)))?[1]
        .as_str()
}

fn loader(profile: &Value) -> Result<(Loader, &str)> {
    if let Some(v) = argument(profile, "--fml.neoForgeVersion") {
        return Ok((Loader::NeoForge, v));
    }
    if let Some(v) = argument(profile, "--fml.forgeVersion") {
        return Ok((Loader::Forge, v));
    }
    for lib in profile["libraries"].as_array().into_iter().flatten() {
        let name = lib["name"].as_str().unwrap_or("");
        if let Some(v) = name.strip_prefix("net.fabricmc:fabric-loader:") {
            return Ok((Loader::Fabric, v));
        }
        if let Some(v) = name.strip_prefix("net.neoforged:neoforge:") {
            return Ok((Loader::NeoForge, v.split(':').next().unwrap_or(v)));
        }
        if let Some((_, v)) = name
            .strip_prefix("net.minecraftforge:forge:")
            .and_then(|v| v.split_once('-'))
        {
            return Ok((Loader::Forge, v.split(':').next().unwrap_or(v)));
        }
    }
    ensure!(
        profile["mainClass"] == "net.minecraft.client.main.Main",
        "unrecognized client loader"
    );
    Ok((Loader::Vanilla, ""))
}

struct Manifest {
    relative: String,
    json: Value,
}

fn jar_version(path: &Path) -> Option<String> {
    let mut jar = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
    let entry = jar.by_name("version.json").ok()?;
    if entry.size() > 128 * 1024 {
        return None;
    }
    let version: Value = serde_json::from_reader(std::io::Read::take(entry, 128 * 1024)).ok()?;
    version["id"].as_str().map(str::to_owned)
}

fn client(
    root: &Path,
    name: &str,
    manifest: &Manifest,
    all: &BTreeMap<String, Manifest>,
) -> Result<Client> {
    let data = &manifest.json;
    let mut game = None;
    let mut profile = data.clone();
    let mut parent_dir = None;
    let (kind, version, mc) = if let Some(patches) =
        data["patches"].as_array().filter(|p| !p.is_empty())
    {
        let base = patches
            .iter()
            .find(|p| p["id"] == "game")
            .context("missing game patch")?;
        let mc = base["version"]
            .as_str()
            .context("missing Minecraft version")?;
        let mut extras = patches.iter().filter(|p| p["id"] != "game");
        let (kind, version) = if let Some(patch) = extras.next() {
            ensure!(
                extras.next().is_none(),
                "multiple loader patches are unsupported"
            );
            let kind = match patch["id"].as_str() {
                Some("forge") => Loader::Forge,
                Some("neoforge") => Loader::NeoForge,
                Some("fabric" | "fabric-loader") => Loader::Fabric,
                _ => bail!("unknown loader patch {}", patch["id"]),
            };
            profile = patch.clone();
            (
                kind,
                patch["version"]
                    .as_str()
                    .context("missing loader version")?,
            )
        } else {
            (Loader::Vanilla, mc)
        };
        game = Some(base.clone());
        (kind, version.to_owned(), mc.to_owned())
    } else {
        let (kind, version) = loader(data)?;
        let parent = data["inheritsFrom"].as_str();
        let base = parent.and_then(|p| {
            all.get(p).or_else(|| {
                let mut matches = all.values().filter(|m| m.json["id"].as_str() == Some(p));
                let first = matches.next()?;
                matches.next().is_none().then_some(first)
            })
        });
        if let Some(base) = base {
            ensure!(
                base.json["inheritsFrom"].is_null() && base.json["downloads"]["client"].is_object(),
                "unsupported parent version manifest"
            );
            parent_dir = Some(base.relative.as_str());
            game = Some(base.json.clone());
        } else if parent.is_none() && data["downloads"]["client"].is_object() {
            let mut base = data.clone();
            if kind != Loader::Vanilla {
                // A flattened profile already has the complete JVM/game arguments and classpath.
                base["libraries"] = json!([]);
                profile["arguments"] = json!({});
            }
            game = Some(base);
        }
        let intermediary = data["libraries"]
            .as_array()
            .into_iter()
            .flatten()
            .find_map(|lib| {
                lib["name"]
                    .as_str()?
                    .strip_prefix("net.fabricmc:intermediary:")
            });
        let local_jar = under(root, &format!("{}.jar", manifest.relative))?;
        let embedded = if parent.is_none() && data["clientVersion"].is_null() {
            jar_version(&local_jar)
        } else {
            None
        };
        let mc = argument(data, "--fml.mcVersion")
            .or(data["clientVersion"].as_str())
            .or(intermediary)
            .or(embedded.as_deref())
            .or_else(|| {
                base.and_then(|b| b.json["clientVersion"].as_str().or(b.json["id"].as_str()))
            })
            .or(parent)
            .or_else(|| {
                if kind == Loader::Vanilla {
                    data["id"].as_str()
                } else {
                    None
                }
            })
            .context("cannot determine the Minecraft version from its manifest")?;
        (
            kind,
            if kind == Loader::Vanilla { mc } else { version }.to_owned(),
            mc.to_owned(),
        )
    };
    component(&mc)?;
    let expected = kind.profile_id(&mc, &version);
    if let Some(g) = &mut game {
        ensure!(
            g["downloads"]["client"].is_object() && g["assetIndex"].is_object(),
            "missing client or asset index metadata"
        );
        g["id"] = json!(mc);
    }
    if kind == Loader::Vanilla {
        profile = json!({"mainClass":"net.minecraft.client.main.Main","libraries":[]});
    }
    profile["inheritsFrom"] = json!(mc);
    profile["id"] = json!(expected);
    let jar_name = data["jar"].as_str().unwrap_or(name);
    component(jar_name)?;
    let local = format!("{}.jar", manifest.relative);
    let explicit = format!("versions/{jar_name}/{jar_name}.jar");
    let jar = if data["jar"].is_string() && under(root, &explicit)?.is_file() {
        explicit
    } else if !manifest.relative.contains('/')
        && data["jar"].is_string()
        && under(root, &format!("{jar_name}.jar"))?.is_file()
    {
        format!("{jar_name}.jar")
    } else if under(root, &local)?.is_file() {
        local
    } else if manifest.relative == "version" && under(root, "minecraft.jar")?.is_file() {
        "minecraft.jar".into()
    } else if let Some(rel) = parent_dir {
        format!("{rel}.jar")
    } else if data["inheritsFrom"].is_string() {
        format!("versions/{mc}/{mc}.jar")
    } else {
        local
    };
    let version_dir = under(root, &manifest.relative)?
        .parent()
        .context("invalid version directory")?
        .to_owned();
    let mut data_dirs = vec![];
    if version_dir != root && has_data(&version_dir)? {
        data_dirs.push(version_dir.clone());
    }
    if data_dirs.is_empty() || has_data(root)? {
        data_dirs.push(root.to_owned());
    }
    Ok(Client {
        name: name.to_owned(),
        minecraft: mc,
        loader: kind,
        version,
        root: root.to_owned(),
        version_dir,
        game,
        profile,
        jar,
        data_dirs,
    })
}

pub fn discover(root: &Path) -> Result<(Vec<Client>, Vec<String>)> {
    let absolute = std::path::absolute(root)?;
    ensure!(
        absolute
            .file_name()
            .is_some_and(|s| s.eq_ignore_ascii_case(".minecraft"))
            && absolute.is_dir(),
        "specify an existing .minecraft root directory"
    );
    crate::storage::paths::no_link(&absolute)?;
    let versions = under(&absolute, "versions")?;
    let mut inputs = BTreeMap::new();
    let mut warnings = vec![];
    let mut files = vec![];
    if versions.is_dir() {
        for item in fs::read_dir(&versions)? {
            let item = item?;
            if !item.file_type()?.is_dir() {
                continue;
            }
            let id = item
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("invalid version name"))?;
            let rel = format!("versions/{id}/{id}");
            files.push((id, rel));
        }
    }
    for item in fs::read_dir(&absolute)? {
        let path = item?.path();
        if path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("json"))
        {
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .context("invalid version filename")?;
            if path.with_extension("jar").is_file()
                || matches!(id, "minecraft" | "version")
                || id.starts_with(|c: char| c.is_ascii_digit())
            {
                files.push((id.to_owned(), id.to_owned()));
            }
        }
    }
    for (id, relative) in files {
        let path = match under(&absolute, &format!("{relative}.json")) {
            Ok(path) => path,
            Err(e) => {
                warnings.push(format!("{id}: {e:#}"));
                continue;
            }
        };
        if !path.is_file() {
            continue;
        }
        match read(&path) {
            Ok(json) => {
                ensure!(
                    inputs
                        .insert(id.clone(), Manifest { relative, json })
                        .is_none(),
                    "duplicate version name {id}"
                );
            }
            Err(e) => warnings.push(format!("{id}: {e:#}")),
        }
    }
    let mut clients = vec![];
    for (name, manifest) in &inputs {
        match client(&absolute, name, manifest, &inputs) {
            Ok(c) => clients.push(c),
            Err(e) => warnings.push(format!("{name}: {e:#}")),
        }
    }
    Ok((clients, warnings))
}

pub(crate) struct Recovery<'a> {
    scratch: &'a Path,
    pub version_dir: &'a Path,
    confirm: &'a dyn Fn(&str) -> Result<bool>,
    network: std::cell::OnceCell<Network>,
}

impl Recovery<'_> {
    fn network(&self) -> Result<&Network> {
        if self.network.get().is_none() {
            let _ = self.network.set(Network::new()?);
        }
        Ok(self.network.get().expect("initialized network"))
    }
    pub fn missing(
        &self,
        rel: &str,
        url: Option<&str>,
        sha1: Option<&str>,
        size: Option<u64>,
    ) -> Result<PathBuf> {
        let dest = under(self.scratch, rel)?;
        if dest.is_file() {
            return Ok(dest);
        }
        let url = url.with_context(|| {
            format!("missing {rel}; restore it with the original launcher and retry")
        })?;
        let uri: ureq::http::Uri = url.parse()?;
        let maven = matches!(
            uri.host(),
            Some("maven.fabricmc.net" | "maven.minecraftforge.net" | "maven.neoforged.net")
        );
        ensure!(
            uri.scheme_str() == Some("https")
                && uri.query().is_none()
                && (maven
                    || matches!(
                        uri.host(),
                        Some(
                            "libraries.minecraft.net"
                                | "resources.download.minecraft.net"
                                | "piston-data.mojang.com"
                                | "piston-meta.mojang.com"
                                | "launcher.mojang.com"
                                | "launchermeta.mojang.com"
                        )
                    )),
            "missing {rel}; no trusted distribution source is available, so supply it manually"
        );
        ensure!(
            sha1.is_some() || maven,
            "missing {rel}; no verifiable download information"
        );
        if let Some(hash) = sha1 {
            hash_id(hash, 40)?;
        }
        ensure!((self.confirm)(rel)?, "download cancelled for {rel}");
        let net = self.network()?;
        let sidecar;
        let hash = match sha1 {
            Some(hash) => hash,
            None => {
                sidecar = net
                    .installer_checksum(&format!("{url}.sha1"))
                    .with_context(|| {
                        format!("missing official SHA-1 for {rel}; supply the file manually")
                    })?;
                &sidecar
            }
        };
        net.game_file(url, &dest, size, Some(hash))?;
        Ok(dest)
    }
}

fn collect(game: &Path, name: &str, files: &mut Vec<String>, dirs: &mut Vec<String>) -> Result<()> {
    let path = under(game, name)?;
    if !path.try_exists()? {
        return Ok(());
    }
    if path.is_file() {
        files.push(name.to_owned());
        return Ok(());
    }
    dirs.push(name.to_owned());
    for e in fs::read_dir(path)? {
        let item = e?;
        let leaf = item
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("invalid filename"))?;
        let lower = leaf.to_ascii_lowercase();
        let in_saves = name == "saves" || name.starts_with("saves/");
        if matches!(
            lower.as_str(),
            ".git" | ".svn" | ".ds_store" | "thumbs.db" | "desktop.ini"
        ) || (in_saves && lower == "session.lock" && item.file_type()?.is_file())
            || (!in_saves
                && (leaf.starts_with('.')
                    || matches!(
                        lower.as_str(),
                        "logs" | "crash-reports" | "cache" | "caches"
                    )
                    || lower.ends_with(".log")))
        {
            continue;
        }
        if name == "mods" && (!item.file_type()?.is_file() || !lower.ends_with(".jar")) {
            continue;
        }
        collect(game, &format!("{name}/{leaf}"), files, dirs)?;
    }
    Ok(())
}

pub fn valid_new_id(root: &Path, id: &str) -> Result<()> {
    ensure!(
        !crate::instances::instance(root, id)?.try_exists()?,
        "instance {id} already exists"
    );
    Ok(())
}

pub fn import(
    root: &Path,
    client: &Client,
    id: &str,
    data_index: usize,
    options: &Options,
    confirm: impl Fn(&str) -> Result<bool>,
) -> Result<()> {
    crate::instances::install::separate_source(root, &client.root)?;
    let game = client
        .data_dirs
        .get(data_index)
        .context("invalid game directory selection")?;
    let mut recipe = Recipe::installed(&client.minecraft, client.loader, &client.version)?;
    recipe.client_jar.clone_from(&client.jar);
    let mut required = vec![];
    let mut dirs = vec![];
    for dir in GAMEPLAY {
        collect(game, dir, &mut required, &mut dirs)?;
    }
    for target in required {
        recipe.pack_files.push(PackFile {
            role: if target.starts_with("mods/") {
                PackRole::Mod
            } else {
                PackRole::Override
            },
            source: target.clone(),
            target,
            sha1: None,
            size: None,
            origin: None,
            sha512: None,
            source_urls: vec![],
        });
    }
    let mut private = vec![];
    for (dir, selected) in OPTIONAL.iter().zip([
        options.saves,
        options.config,
        options.config,
        options.resourcepacks,
        options.shaderpacks,
    ]) {
        if selected {
            collect(game, dir, &mut private, &mut dirs)?;
        }
    }
    crate::instances::install::install_with_private(
        root,
        id,
        |scratch| {
            let recovery = Recovery {
                scratch,
                version_dir: &client.version_dir,
                confirm: &confirm,
                network: Default::default(),
            };
            let base = if let Some(base) = &client.game {
                base.clone()
            } else {
                let rel = format!("versions/{0}/{0}.json", client.minecraft);
                ensure!(confirm(&rel)?, "download cancelled for {rel}");
                let dest = under(scratch, &rel)?;
                metadata::download_mojang_profile(&client.minecraft, &dest, recovery.network()?)?;
                read(&dest)?
            };
            metadata::resolve_installed(
                root,
                &client.root,
                &recipe,
                game,
                (base, client.profile.clone()),
                &recovery,
            )
        },
        |stage| {
            for dir in dirs {
                fs::create_dir_all(under(stage, &dir)?)?;
            }
            for rel in private {
                let dest = under(stage, &rel)?;
                fs::create_dir_all(dest.parent().context("invalid file path")?)?;
                let mut out = fs::File::create_new(&dest)?;
                std::io::copy(&mut fs::File::open(under(game, &rel)?)?, &mut out)?;
            }
            Ok(())
        },
    )
}
