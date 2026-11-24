use crate::paths::component;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, process::Command};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    #[default]
    Fabric,
    Forge,
    NeoForge,
}

impl Loader {
    pub fn name(self) -> &'static str {
        match self {
            Self::Fabric => "Fabric",
            Self::Forge => "Forge",
            Self::NeoForge => "NeoForge",
        }
    }

    pub(crate) fn profile_id(self, mc: &str, version: &str) -> String {
        match self {
            Self::Fabric => format!("fabric-loader-{version}-{mc}"),
            Self::Forge => format!("{mc}-forge-{version}"),
            Self::NeoForge => format!("neoforge-{version}"),
        }
    }
}

pub(crate) fn install_client(
    loader: Loader,
    mc: &str,
    version: &str,
    root: &Path,
    base: &Path,
    major: u32,
    net: &crate::network::Network,
) -> Result<()> {
    let (repo, artifact, ver) = match loader {
        Loader::Forge => (
            "https://maven.minecraftforge.net/net/minecraftforge",
            "forge",
            format!("{mc}-{version}"),
        ),
        Loader::NeoForge => (
            "https://maven.neoforged.net/releases/net/neoforged",
            "neoforge",
            version.to_owned(),
        ),
        Loader::Fabric => anyhow::bail!("Fabric uses a launcher profile"),
    };
    let url = format!("{repo}/{artifact}/{ver}/{artifact}-{ver}-installer.jar");
    let installer = base.join("installer.jar");
    net.loader_installer(loader, version, &url, &installer)?;
    crate::metadata::prepare_installer_libraries(&installer, base, net)?;
    fs::write(base.join("launcher_profiles.json"), br#"{"profiles":{}}"#)?;
    let java = crate::java::select(root, major, None, true)?;
    let base = std::path::absolute(base)?;
    let status = Command::new(java.executable)
        .current_dir(&base)
        .args([
            "-Djava.awt.headless=true",
            "-jar",
            "installer.jar",
            "--installClient",
            ".",
        ])
        .arg("--mirror")
        .arg(net.maven_mirror())
        .status()?;
    ensure!(
        status.success(),
        "{} client installer exited with {status}",
        loader.name()
    );
    let profile = loader.profile_id(mc, version);
    ensure!(
        crate::paths::under(&base, &format!("versions/{profile}/{profile}.json"))?.is_file(),
        "{} installer did not produce the client profile {profile}",
        loader.name()
    );
    Ok(())
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Arguments {
    #[serde(default)]
    pub jvm: Vec<String>,
    #[serde(default)]
    pub game: Vec<String>,
}

impl Arguments {
    // Older generations and minimal local recipes predate Mojang's argument lists.
    pub fn offline_defaults() -> Self {
        let jvm = [
            "-Djava.library.path=${natives_directory}",
            "-Djna.tmpdir=${natives_directory}",
            "-Dorg.lwjgl.system.SharedLibraryExtractPath=${natives_directory}",
            "-Dio.netty.native.workdir=${natives_directory}",
            "-cp",
            "${classpath}",
        ];
        let game = [
            "--gameDir",
            "${game_directory}",
            "--assetsDir",
            "${assets_root}",
            "--assetIndex",
            "${assets_index_name}",
            "--version",
            "${version_name}",
            "--username",
            "${auth_player_name}",
            "--uuid",
            "${auth_uuid}",
            "--accessToken",
            "${auth_access_token}",
            "--userType",
            "${user_type}",
            "--versionType",
            "${version_type}",
        ];
        Self {
            jvm: jvm.map(String::from).into(),
            game: game.map(String::from).into(),
        }
    }
}

// BootstrapLauncher discovers these generated jars by Maven path at runtime.
// They are absent from the launch profile's classpath list.
pub(crate) fn runtime_artifacts(
    kind: Loader,
    mc: &str,
    version: &str,
    args: &[String],
) -> Result<Vec<String>> {
    let mut files = vec![];
    let mapping = match kind {
        Loader::Fabric => return Ok(files),
        Loader::Forge => "--fml.mcpVersion",
        Loader::NeoForge => "--fml.neoFormVersion",
    };
    let arg = |key: &str| {
        args.windows(2)
            .find(|w| w[0].eq_ignore_ascii_case(key))
            .map(|w| w[1].as_str())
    };
    let mappings = arg(mapping).with_context(|| format!("missing loader argument {mapping}"))?;
    component(mappings)?;
    for classifier in ["srg", "extra"] {
        files.push(format!("net.minecraft:client:{mc}-{mappings}:{classifier}"));
    }
    match kind {
        Loader::Forge => {
            for name in [
                "fmlcore",
                "javafmllanguage",
                "lowcodelanguage",
                "mclanguage",
            ] {
                files.push(format!("net.minecraftforge:{name}:{mc}-{version}"));
            }
            for classifier in ["universal", "client"] {
                files.push(format!(
                    "net.minecraftforge:forge:{mc}-{version}:{classifier}"
                ));
            }
        }
        Loader::NeoForge => {
            for classifier in ["universal", "client"] {
                files.push(format!("net.neoforged:neoforge:{version}:{classifier}"));
            }
        }
        Loader::Fabric => unreachable!(),
    }
    Ok(files)
}

pub(crate) fn coordinate_path(name: &str) -> Result<String> {
    let parts: Vec<_> = name.split(':').collect();
    ensure!(
        matches!(parts.len(), 3 | 4),
        "invalid Maven coordinate {name}"
    );
    for part in parts[0].split('.').chain(parts[1..].iter().copied()) {
        component(part)?;
    }
    let classifier = parts.get(3).map_or(String::new(), |s| format!("-{s}"));
    Ok(format!(
        "{}/{}/{}/{}-{}{}.jar",
        parts[0].replace('.', "/"),
        parts[1],
        parts[2],
        parts[1],
        parts[2],
        classifier
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "downloads the official NeoForge installer and dependencies; set MCDOCKER_TEST_SOURCE to a Minecraft 1.21.1 installation"]
    fn official_neoforge_installer_generates_the_client_files() -> Result<()> {
        let source = std::env::var_os("MCDOCKER_TEST_SOURCE").context("MCDOCKER_TEST_SOURCE")?;
        let tmp = tempfile::tempdir_in(std::path::absolute("target")?)?;
        let base = tmp.path().join("base");
        let vanilla = "versions/1.21.1";
        fs::create_dir_all(base.join(vanilla))?;
        for file in ["1.21.1.jar", "1.21.1.json"] {
            let path = Path::new(vanilla).join(file);
            fs::copy(Path::new(&source).join(&path), base.join(path))?;
        }
        let version = "21.1.228";
        install_client(
            Loader::NeoForge,
            "1.21.1",
            version,
            tmp.path(),
            &base,
            21,
            &crate::network::Network::new()?,
        )?;
        let id = Loader::NeoForge.profile_id("1.21.1", version);
        let profile: serde_json::Value =
            serde_json::from_slice(&fs::read(base.join(format!("versions/{id}/{id}.json")))?)?;
        assert_eq!(profile["id"], id);
        let args: Vec<String> = serde_json::from_value(profile["arguments"]["game"].clone())?;
        for name in runtime_artifacts(Loader::NeoForge, "1.21.1", version, &args)? {
            assert!(
                base.join("libraries")
                    .join(coordinate_path(&name)?)
                    .is_file(),
                "{name}"
            );
        }
        Ok(())
    }
}
