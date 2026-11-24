use crate::{
    Generation, Loader, Version,
    loader::{Arguments, coordinate_path as maven_path},
    paths::{component, hash_id, under},
    store::{self, Blob},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub(crate) const FABRIC_MAIN: &str = "net.fabricmc.loader.impl.launch.knot.KnotClient";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Recipe {
    schema_version: Version,
    pub platform: String,
    pub minecraft: String,
    #[serde(default)]
    pub loader: Loader,
    #[serde(alias = "fabric")]
    pub loader_version: String,
    pub java_major: Option<u32>,
    client_metadata: String,
    client_jar: String,
    loader_metadata: String,
    libraries: String,
    assets: String,
    pub pack_files: Vec<PackFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PackFile {
    pub source: String,
    pub target: String,
    pub role: PackRole,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    #[serde(skip)]
    pub origin: Option<String>,
    #[serde(skip)]
    pub sha512: Option<String>,
    #[serde(skip)]
    pub source_urls: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PackRole {
    Mod,
    Template,
    #[serde(skip)]
    Override,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Role {
    Classpath,
    Library,
    Asset,
    Mod,
    Template,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Entry {
    pub target: String,
    pub role: Role,
    pub blob: Blob,
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha512: Option<String>,
    #[serde(default, rename = "sourceUrls", skip_serializing_if = "Vec::is_empty")]
    source_urls: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Layer {
    schema_version: Version,
    kind: String,
    pub dependencies: Vec<String>,
    pub entries: Vec<Entry>,
}

impl Layer {
    fn new(kind: &str, dependencies: Vec<String>) -> Self {
        Self {
            schema_version: Version::V1,
            kind: kind.to_owned(),
            dependencies,
            entries: vec![],
        }
    }
}

#[derive(Deserialize)]
struct Artifact {
    url: Option<String>,
    path: Option<String>,
    sha1: String,
    size: u64,
}
#[derive(Deserialize)]
struct LibraryDownloads {
    artifact: Artifact,
}
#[derive(Deserialize)]
struct Library {
    url: Option<String>,
    sha1: Option<String>,
    size: Option<u64>,
    name: String,
    downloads: Option<LibraryDownloads>,
    rules: Option<Vec<Rule>>,
    natives: Option<BTreeMap<String, String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    action: String,
    os: Option<Os>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Os {
    name: Option<String>,
    arch: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Vanilla {
    arguments: Option<GameArguments>,
    id: String,
    downloads: ClientDownload,
    asset_index: Index,
    libraries: Vec<Library>,
    java_version: JavaVersion,
}
#[derive(Deserialize, Default)]
struct GameArguments {
    #[serde(default)]
    game: Vec<serde_json::Value>,
    #[serde(default)]
    jvm: Vec<serde_json::Value>,
}

fn selected_args(values: &[serde_json::Value]) -> Result<Vec<String>> {
    use serde_json::Value;
    let mut out = Vec::new();
    for entry in values {
        let value = if let Some(rules) = entry.get("rules") {
            let mut allowed = false;
            for rule in rules.as_array().context("expected an argument rule list")? {
                let os = &rule["os"];
                if os["name"].as_str().is_some_and(|s| s != "windows")
                    || os["arch"]
                        .as_str()
                        .is_some_and(|s| !matches!(s, "amd64" | "x86_64"))
                    || rule["features"]
                        .as_object()
                        .is_some_and(|f| f.values().any(|v| v != false))
                {
                    continue;
                }
                ensure!(os["version"].is_null(), "unsupported Windows version rule");
                allowed = match rule["action"].as_str() {
                    Some("allow") => true,
                    Some("disallow") => false,
                    _ => bail!("invalid launch argument rule"),
                };
            }
            if !allowed {
                continue;
            }
            &entry["value"]
        } else {
            entry
        };
        match value {
            Value::String(s) => out.push(s.clone()),
            Value::Array(args) => {
                for arg in args {
                    out.push(
                        arg.as_str()
                            .context("expected a launch argument")?
                            .to_owned(),
                    );
                }
            }
            _ => bail!("invalid launch argument {value}"),
        }
    }
    Ok(out)
}

fn quick_play_argument(arg: &serde_json::Value) -> bool {
    match arg {
        serde_json::Value::String(s) => s == "--quickPlaySingleplayer",
        serde_json::Value::Array(args) => args.iter().any(quick_play_argument),
        serde_json::Value::Object(rule) => rule.get("value").is_some_and(quick_play_argument),
        _ => false,
    }
}
#[derive(Deserialize)]
struct ClientDownload {
    client: Artifact,
}
#[derive(Deserialize)]
struct Index {
    url: Option<String>,
    id: String,
    sha1: String,
    size: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JavaVersion {
    major_version: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoaderProfile {
    id: Option<String>,
    inherits_from: String,
    main_class: String,
    libraries: Vec<Library>,
    #[serde(default)]
    arguments: GameArguments,
}
#[derive(Deserialize)]
struct AssetIndex {
    objects: BTreeMap<String, Asset>,
}
#[derive(Deserialize)]
struct Asset {
    hash: String,
    size: u64,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_reader(fs::File::open(path).with_context(|| path.display().to_string())?)
        .with_context(|| format!("invalid metadata {}", path.display()))
}

fn windows_library(lib: &Library) -> Result<bool> {
    let rules = lib.rules.as_deref().unwrap_or_default();
    let mut allowed = rules.is_empty();
    for rule in rules {
        let allow = match rule.action.as_str() {
            "allow" => true,
            "disallow" => false,
            other => bail!("unknown rule {other}"),
        };
        if rule.os.as_ref().is_none_or(|os| {
            os.name.as_deref().is_none_or(|n| n == "windows")
                && os
                    .arch
                    .as_deref()
                    .is_none_or(|a| matches!(a, "x86_64" | "amd64"))
        }) {
            allowed = allow;
        }
    }
    let classifier = lib.name.split(':').nth(3).unwrap_or("");
    Ok(allowed
        && (!classifier.starts_with("natives-windows-") || classifier == "natives-windows-x86_64"))
}

struct Importer<'a> {
    root: &'a Path,
    source: &'a Path,
    files: BTreeSet<String>,
    directories: BTreeMap<String, String>,
}

impl Importer<'_> {
    fn file(
        &mut self,
        source: &str,
        target: String,
        role: Role,
        sha1: Option<&str>,
        size: Option<u64>,
    ) -> Result<Entry> {
        let mut prefix = String::new();
        let mut parts = target.split('/').peekable();
        while let Some(part) = parts.next() {
            component(part)?;
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(&part.to_uppercase());
            ensure!(!self.files.contains(&prefix), "target collision: {target}");
            if parts.peek().is_some() {
                let spelling = self
                    .directories
                    .entry(prefix.clone())
                    .or_insert_with(|| part.to_owned());
                ensure!(spelling == part, "directory case collision: {target}");
            }
        }
        ensure!(
            !self.directories.contains_key(&prefix),
            "file/directory collision: {target}"
        );
        self.files.insert(prefix);
        let blob = store::ingest(self.root, &under(self.source, source)?, sha1, size)?;
        Ok(Entry {
            target,
            role,
            blob,
            source: source.to_owned(),
            sha1: sha1.map(str::to_owned),
            sha512: None,
            source_urls: vec![],
        })
    }
}

pub(crate) fn resolve(
    root: &Path,
    source: &Path,
    recipe: &Recipe,
    pack_source: &Path,
) -> Result<Generation> {
    let Version::V1 = recipe.schema_version;
    ensure!(
        recipe.platform == "windows-x64",
        "expected a windows-x64 client"
    );
    component(&recipe.loader_version)?;
    component(&recipe.minecraft)?;
    let mc: Vanilla = read_json(&under(source, &recipe.client_metadata)?)?;
    let profile: LoaderProfile = read_json(&recipe.profile_path(source)?)?;
    ensure!(
        mc.id == recipe.minecraft
            && recipe
                .java_major
                .is_none_or(|major| major == mc.java_version.major_version)
            && profile.inherits_from == mc.id,
        "incompatible Minecraft or loader metadata"
    );
    if recipe.loader == Loader::Fabric {
        let loader = format!("net.fabricmc:fabric-loader:{}", recipe.loader_version);
        ensure!(
            profile.main_class == FABRIC_MAIN
                && profile
                    .libraries
                    .iter()
                    .filter(|lib| lib.name.starts_with("net.fabricmc:fabric-loader:"))
                    .map(|l| &l.name)
                    .eq([&loader]),
            "Fabric Loader coordinate does not match recipe"
        );
    } else {
        let expected = recipe.loader.profile_id(&mc.id, &recipe.loader_version);
        let alias = format!("forge-{}", recipe.loader_version);
        ensure!(profile.id.as_deref().is_some_and(|id| id == expected || (recipe.loader == Loader::Forge && id == alias)), "loader profile does not match {expected}");
        ensure!(
            !profile.main_class.is_empty()
                && profile.main_class.split('.').all(|s| !s.is_empty()
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')),
            "invalid Java main class"
        );
    }
    let mut importer = Importer {
        root,
        source,
        files: BTreeSet::new(),
        directories: BTreeMap::new(),
    };
    let mut game = Layer::new("game", vec![]);
    for lib in mc.libraries {
        if !windows_library(&lib)? {
            continue;
        }
        ensure!(
            lib.natives
                .as_ref()
                .is_none_or(|n| !n.contains_key("windows")),
            "legacy native classifiers are not supported by this adapter"
        );
        let a = lib
            .downloads
            .context("missing Mojang library download")?
            .artifact;
        let path = a.path.context("missing library path")?;
        game.entries.push(importer.file(
            &format!("{}/{path}", recipe.libraries),
            format!("libraries/{path}"),
            Role::Classpath,
            Some(&a.sha1),
            Some(a.size),
        )?);
    }
    let a = mc.downloads.client;
    game.entries.push(importer.file(
        &recipe.client_jar,
        format!("client/{}.jar", mc.id),
        Role::Classpath,
        Some(&a.sha1),
        Some(a.size),
    )?);
    let index = mc.asset_index;
    component(&index.id)?;
    let rel = format!("indexes/{}.json", index.id);
    let entry = importer.file(
        &format!("{}/{rel}", recipe.assets),
        format!("assets/{rel}"),
        Role::Asset,
        Some(&index.sha1),
        Some(index.size),
    )?;
    let assets: AssetIndex = read_json(&store::path(root, &entry.blob)?)?;
    game.entries.push(entry);
    let mut seen = BTreeMap::new();
    for a in assets.objects.into_values() {
        hash_id(&a.hash, 40)?;
        if let Some(size) = seen.insert(a.hash.clone(), a.size) {
            ensure!(size == a.size, "inconsistent asset lengths");
            continue;
        }
        let rel = format!("objects/{}/{}", &a.hash[..2], a.hash);
        game.entries.push(importer.file(
            &format!("{}/{rel}", recipe.assets),
            format!("assets/{rel}"),
            Role::Asset,
            Some(&a.hash),
            Some(a.size),
        )?);
    }
    let game_id = store::publish_json(root, "layers", &game)?;
    let mut loader = Layer::new(
        &recipe.loader.name().to_ascii_lowercase(),
        vec![game_id.clone()],
    );
    // Each layer has its own namespace. A loader can override a vanilla dependency.
    importer.files.clear();
    importer.directories.clear();
    let mut listed = BTreeSet::new();
    for lib in profile.libraries {
        if !windows_library(&lib)? {
            continue;
        }
        let a = lib.downloads.as_ref().map(|d| &d.artifact);
        let path = a
            .and_then(|a| a.path.clone())
            .map_or_else(|| maven_path(&lib.name), Ok)?;
        listed.insert(path.clone());
        loader.entries.push(importer.file(
            &format!("{}/{path}", recipe.libraries),
            format!("libraries/{path}"),
            Role::Classpath,
            a.map(|a| a.sha1.as_str()).or(lib.sha1.as_deref()),
            a.map(|a| a.size).or(lib.size),
        )?);
    }
    let loader_game = selected_args(&profile.arguments.game)?;
    if profile.main_class == "cpw.mods.bootstraplauncher.BootstrapLauncher" {
        for name in crate::loader::runtime_artifacts(
            recipe.loader,
            &mc.id,
            &recipe.loader_version,
            &loader_game,
        )? {
            let path = maven_path(&name)?;
            if listed.insert(path.clone()) {
                loader.entries.push(importer.file(
                    &format!("{}/{path}", recipe.libraries),
                    format!("libraries/{path}"),
                    Role::Library,
                    None,
                    None,
                )?);
            }
        }
    }
    let loader_id = store::publish_json(root, "layers", &loader)?;
    let pack_id = pack_layer(root, pack_source, recipe, &loader_id)?;
    let mut arguments = Arguments::offline_defaults();
    if let Some(base) = &mc.arguments {
        if !base.jvm.is_empty() {
            arguments.jvm = selected_args(&base.jvm)?;
        }
        if !base.game.is_empty() {
            arguments.game = selected_args(&base.game)?;
        }
    }
    arguments.jvm.extend(selected_args(&profile.arguments.jvm)?);
    arguments.game.extend(loader_game);
    Ok(Generation {
        schema_version: Version::V1,
        minecraft: mc.id.clone(),
        platform: recipe.platform.clone(),
        loader: recipe.loader,
        loader_version: recipe.loader_version.clone(),
        main_class: profile.main_class,
        argument_profile: "modern-v1".into(),
        arguments,
        layers: vec![game_id, loader_id, pack_id],
        asset_index: index.id,
        java_major: mc.java_version.major_version,
        quick_play: Some(mc.arguments.as_ref().map_or(mc.id == "1.20.1", |args| {
            args.game.iter().any(quick_play_argument)
        })),
    })
}

pub(crate) fn pack_layer(
    root: &Path,
    source: &Path,
    recipe: &Recipe,
    loader: &str,
) -> Result<String> {
    let mut importer = Importer {
        root,
        source,
        files: BTreeSet::new(),
        directories: BTreeMap::new(),
    };
    let mut pack = Layer::new("pack", vec![loader.to_owned()]);
    for p in &recipe.pack_files {
        let role = match p.role {
            PackRole::Mod => {
                let name = p
                    .target
                    .strip_prefix("mods/")
                    .context("mod target must be mods/<name>.jar")?;
                component(name)?;
                ensure!(
                    name.to_ascii_lowercase().ends_with(".jar"),
                    "mod needs a .jar extension"
                );
                Role::Mod
            }
            PackRole::Template => {
                ensure!(
                    p.target.starts_with("config/") || p.target == "options.txt",
                    "template must target config/ or options.txt"
                );
                Role::Template
            }
            PackRole::Override => Role::Template,
        };
        let mut entry = importer.file(
            &p.source,
            format!("game/{}", p.target),
            role,
            p.sha1.as_deref(),
            p.size,
        )?;
        if let Some(origin) = &p.origin {
            entry.source = origin.clone();
        }
        entry.sha512 = p.sha512.clone();
        entry.source_urls = p.source_urls.clone();
        pack.entries.push(entry);
    }
    store::publish_json(root, "layers", &pack)
}

// Some installer versions ignore --mirror for libraries with an explicit repository.
// Read their own manifests before letting Java run the installation processors.
pub(crate) fn prepare_installer_libraries(
    installer: &Path,
    base: &Path,
    net: &crate::network::Network,
) -> Result<()> {
    use std::io::Read;
    #[derive(Deserialize)]
    struct InstallerProfile {
        json: Option<String>,
        #[serde(default)]
        libraries: Vec<Library>,
    }
    let mut zip = zip::ZipArchive::new(fs::File::open(installer)?)?;
    let mut read = |name: &str| -> Result<InstallerProfile> {
        let entry = zip.by_name(name)?;
        ensure!(
            entry.size() <= 16 * 1024 * 1024,
            "oversized installer manifest"
        );
        Ok(serde_json::from_reader(entry.take(16 * 1024 * 1024))?)
    };
    let install = read("install_profile.json")?;
    let profile = read(
        install
            .json
            .as_deref()
            .unwrap_or("version.json")
            .trim_start_matches('/'),
    )?;
    let mut files = BTreeMap::new();
    for library in install.libraries.into_iter().chain(profile.libraries) {
        let Some(downloads) = library.downloads else {
            continue;
        };
        let a = downloads.artifact;
        let (Some(url), Some(path)) = (a.url, a.path) else {
            continue;
        };
        if !url.starts_with("https://")
            || a.sha1.is_empty()
            || zip.index_for_name(&format!("maven/{path}")).is_some()
        {
            continue;
        }
        hash_id(&a.sha1, 40)?;
        let dest = under(base, &format!("libraries/{path}"))?;
        if !dest.try_exists()? {
            files.entry(dest).or_insert((url, a.size, a.sha1));
        }
    }
    let files: Vec<_> = files.into_iter().collect();
    crate::network::parallel(&files, |(path, (url, size, sha1))| {
        net.game_file(url, path, Some(*size), Some(sha1))
    })
}

impl Recipe {
    fn profile_path(&self, base: &Path) -> Result<std::path::PathBuf> {
        let path = under(base, &self.loader_metadata)?;
        if self.loader == Loader::Forge && !path.try_exists()? {
            let alias = format!("forge-{}", self.loader_version);
            let alt = under(base, &format!("versions/{alias}/{alias}.json"))?;
            if alt.is_file() {
                return Ok(alt);
            }
        }
        Ok(path)
    }

    pub fn available_in(&self, base: &Path) -> Result<bool> {
        Ok(self.profile_path(base)?.is_file()
            && under(base, &self.client_metadata)?.is_file()
            && under(base, &self.client_jar)?.is_file())
    }

    pub fn download_base(
        &self,
        root: &Path,
        base: &Path,
        net: &crate::network::Network,
    ) -> Result<()> {
        #[derive(Deserialize)]
        struct Versions {
            versions: Vec<VersionUrl>,
        }
        #[derive(Deserialize)]
        struct VersionUrl {
            id: String,
            url: String,
            sha1: String,
        }
        let versions: Versions =
            net.metadata("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")?;
        let selected = versions
            .versions
            .iter()
            .find(|v| v.id == self.minecraft)
            .context("Minecraft version not in Mojang manifest")?;
        let path = under(base, &self.client_metadata)?;
        net.game_file(&selected.url, &path, None, Some(&selected.sha1))?;
        let mc: Vanilla = read_json(&path)?;
        ensure!(mc.id == self.minecraft, "Mojang version mismatch");
        let fetch = |a: &Artifact, path: &Path| {
            net.game_file(
                a.url.as_deref().context("missing artifact URL")?,
                path,
                Some(a.size),
                Some(&a.sha1),
            )
        };
        let mut downloads = vec![(&mc.downloads.client, under(base, &self.client_jar)?)];
        for lib in &mc.libraries {
            if !windows_library(lib)? {
                continue;
            }
            ensure!(
                lib.natives
                    .as_ref()
                    .is_none_or(|n| !n.contains_key("windows")),
                "unsupported legacy native classifiers"
            );
            let a = &lib
                .downloads
                .as_ref()
                .context("missing library artifact")?
                .artifact;
            downloads.push((
                a,
                under(
                    base,
                    &format!(
                        "libraries/{}",
                        a.path.as_deref().context("missing artifact path")?
                    ),
                )?,
            ));
        }
        crate::network::parallel(&downloads, |(artifact, path)| fetch(artifact, path))?;
        if self.loader == Loader::Fabric {
            let url = format!(
                "https://meta.fabricmc.net/v2/versions/loader/{}/{}/profile/json",
                self.minecraft, self.loader_version
            );
            let profile = under(base, &self.loader_metadata)?;
            net.game_file(&url, &profile, None, None)?;
            let profile: LoaderProfile = read_json(&profile)?;
            ensure!(
                profile.inherits_from == mc.id && profile.main_class == FABRIC_MAIN,
                "Fabric metadata mismatch"
            );
            for lib in profile.libraries {
                if !windows_library(&lib)? {
                    continue;
                }
                let path = maven_path(&lib.name)?;
                let url = format!(
                    "{}/{path}",
                    lib.url
                        .as_deref()
                        .unwrap_or("https://maven.fabricmc.net")
                        .trim_end_matches('/')
                );
                let dest = under(base, &format!("libraries/{path}"))?;
                // Both profiles may reference the same Maven coordinate.
                if !dest.try_exists()? {
                    let a = lib.downloads.as_ref().map(|d| &d.artifact);
                    let declared = a.map(|a| a.sha1.as_str()).or(lib.sha1.as_deref());
                    let published =
                        if declared.is_none() && url.starts_with("https://maven.fabricmc.net/") {
                            net.installer_checksum(&format!("{url}.sha1")).ok()
                        } else {
                            None
                        };
                    net.game_file(
                        &url,
                        &dest,
                        a.map(|a| a.size).or(lib.size),
                        declared.or(published.as_deref()),
                    )?;
                }
            }
        } else {
            crate::loader::install_client(
                self.loader,
                &mc.id,
                &self.loader_version,
                root,
                base,
                mc.java_version.major_version,
                net,
            )?;
        }
        let index = mc.asset_index;
        component(&index.id)?;
        let dest = under(base, &format!("assets/indexes/{}.json", index.id))?;
        net.game_file(
            index.url.as_deref().context("missing asset index URL")?,
            &dest,
            Some(index.size),
            Some(&index.sha1),
        )?;
        let assets: AssetIndex = read_json(&dest)?;
        let mut objects = BTreeMap::new();
        for a in assets.objects.into_values() {
            hash_id(&a.hash, 40)?;
            if let Some(prev) = objects.insert(a.hash, a.size) {
                ensure!(prev == a.size, "conflicting asset size");
            }
        }
        let objects: Vec<_> = objects.into_iter().collect();
        eprintln!("Downloading {} Minecraft asset objects", objects.len());
        crate::network::parallel(&objects, |(hash, size)| {
            let rel = format!("{}/{hash}", &hash[..2]);
            net.game_file(
                &format!("https://resources.download.minecraft.net/{rel}"),
                &under(base, &format!("assets/objects/{rel}"))?,
                Some(*size),
                Some(hash),
            )
        })
    }

    pub fn installed(mc: &str, loader: Loader, version: &str) -> Result<Self> {
        component(mc)?;
        component(version)?;
        ensure!(
            [mc, version].iter().all(|s| s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))),
            "invalid distribution version"
        );
        let profile = loader.profile_id(mc, version);
        Ok(Self {
            schema_version: Version::V1,
            platform: "windows-x64".into(),
            minecraft: mc.into(),
            loader,
            loader_version: version.into(),
            java_major: None,
            client_metadata: format!("versions/{mc}/{mc}.json"),
            client_jar: format!("versions/{mc}/{mc}.jar"),
            loader_metadata: format!("versions/{profile}/{profile}.json"),
            libraries: "libraries".into(),
            assets: "assets".into(),
            pack_files: vec![],
        })
    }
}
