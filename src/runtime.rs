use crate::{
    generation, instance, lock, materialize,
    metadata::{self, Layer, Role},
    paths::under,
    store,
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeSet,
    env, fs,
    path::Path,
    process::{Command, ExitStatus},
};

pub struct Player {
    pub name: String,
    pub uuid: String,
}

pub fn launch(
    root: &Path,
    id: &str,
    java: &Path,
    player: &Player,
    world: Option<&str>,
) -> Result<ExitStatus> {
    launch_with_java(root, id, Some(java), player, world)
}

pub fn launch_with_java(
    root: &Path,
    id: &str,
    java: Option<&Path>,
    player: &Player,
    world: Option<&str>,
) -> Result<ExitStatus> {
    ensure!(
        (1..=16).contains(&player.name.len())
            && player
                .name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_'),
        "invalid offline player name"
    );
    let valid_uuid = player.uuid.len() == 36
        && player.uuid.bytes().enumerate().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        });
    ensure!(valid_uuid, "expected a hyphenated player UUID");
    let root = std::path::absolute(root)?;
    let instance = instance(&root, id)?;
    let global = lock(&under(&root, "instances.lock")?, true)?;
    let _running = lock(&under(&instance, "run.lock")?, false)?;
    drop(global);
    let gen_info = generation(&root, &instance)?;
    let supported = match gen_info.argument_profile.as_str() {
        "modern-v1" => true,
        "fabric-offline-1.20.1-v1" | "fabric-modern-v1" => {
            gen_info.main_class == metadata::FABRIC_MAIN
        }
        _ => false,
    };
    ensure!(
        supported && gen_info.platform == "windows-x64" && gen_info.layers.len() == 3,
        "unsupported launch profile"
    );
    let game = under(&instance, "game")?;
    if let Some(name) = world {
        ensure!(
            gen_info
                .quick_play
                .unwrap_or(gen_info.minecraft == "1.20.1"),
            "this Minecraft version does not declare singleplayer Quick Play support"
        );
        crate::paths::component(name)?;
        ensure!(
            under(&game, &format!("saves/{name}/level.dat"))?.is_file(),
            "no saved world named {name:?} in {id}"
        );
    }
    under(&game, "mods")?;
    let java = crate::java::select(&root, gen_info.java_major, java, true)?;
    eprintln!("Java {} ({})", java.version, java.executable.display());
    let view = tempfile::Builder::new()
        .prefix("run-")
        .tempdir_in(under(&root, "work")?)?;
    let natives = view.path().join("natives");
    fs::create_dir(&natives)?;
    let mut classpath = vec![];
    let mut inherited = BTreeSet::new();
    // An inherited Maven library is replaced by group, artifact and classifier.
    for idx in [1, 0, 2] {
        let layer: Layer = store::record(&root, "layers", &gen_info.layers[idx])?;
        for entry in layer.entries {
            match entry.role {
                Role::Asset | Role::Library | Role::Classpath => {
                    if gen_info.argument_profile == "modern-v1"
                        && entry.role == Role::Classpath
                        && let Some(key) = dependency_key(&entry.target)
                    {
                        if idx == 1 {
                            inherited.insert((key.0.to_owned(), key.1.to_owned()));
                        } else if idx == 0
                            && inherited.contains(&(key.0.to_owned(), key.1.to_owned()))
                        {
                            continue;
                        }
                    }
                    let path = materialize(&root, view.path(), &entry)?;
                    if entry.role == Role::Classpath {
                        classpath.push(path);
                    }
                }
                Role::Mod | Role::Template => {
                    // Installed privately once. Mods and settings can have changed since then.
                    under(&instance, &entry.target)?;
                }
            }
        }
    }
    let mut command = Command::new(java.executable);
    command.current_dir(&game).arg("-Xmx2G");
    let libraries = view.path().join("libraries");
    let assets = view.path().join("assets");
    let classpath = env::join_paths(classpath)?;
    let expand = |mut input: &str| -> Result<String> {
        let mut output = String::new();
        while let Some((prefix, rest)) = input.split_once("${") {
            let (key, tail) = rest
                .split_once('}')
                .context("unclosed launch placeholder")?;
            output.push_str(prefix);
            let value = match key {
                "library_directory" => libraries.to_str(),
                "classpath_separator" => Some(if cfg!(windows) { ";" } else { ":" }),
                "classpath" => classpath.to_str(),
                "version_name" => Some(gen_info.minecraft.as_str()),
                "game_directory" => game.to_str(),
                "assets_root" => assets.to_str(),
                "natives_directory" => natives.to_str(),
                "assets_index_name" => Some(gen_info.asset_index.as_str()),
                "auth_player_name" => Some(player.name.as_str()),
                "auth_uuid" => Some(player.uuid.as_str()),
                "auth_access_token" => Some("0"),
                "clientid" | "auth_xuid" => Some(""),
                "user_type" => Some("legacy"),
                "version_type" => Some("release"),
                "launcher_name" => Some("mcdocker"),
                "launcher_version" => Some(env!("CARGO_PKG_VERSION")),
                _ => anyhow::bail!("unsupported launch placeholder {key}"),
            };
            output.push_str(value.context("invalid Unicode in launch path")?);
            input = tail;
        }
        output.push_str(input);
        Ok(output)
    };
    let args = if gen_info.argument_profile == "modern-v1" {
        gen_info.arguments
    } else {
        crate::loader::Arguments::offline_defaults()
    };
    for arg in args.jvm {
        command.arg(expand(&arg)?);
    }
    command.arg(&gen_info.main_class);
    for arg in args.game {
        command.arg(expand(&arg)?);
    }
    if let Some(name) = world {
        command.args(["--quickPlaySingleplayer", name]);
    }
    crate::resources::with_packs(&instance, view.path(), || {
        command.status().context("start Minecraft client")
    })
}

fn dependency_key(target: &str) -> Option<(&str, &str)> {
    let path = target.strip_prefix("libraries/")?;
    let (parent, file) = path.rsplit_once('/')?;
    let (artifact_path, version) = parent.rsplit_once('/')?;
    let (_, artifact) = artifact_path.rsplit_once('/')?;
    let suffix = file
        .strip_prefix(artifact)?
        .strip_prefix('-')?
        .strip_prefix(version)?;
    Some((artifact_path, suffix))
}
