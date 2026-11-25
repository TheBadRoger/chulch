use anyhow::{Context, Result};
use ring::digest::{SHA1_FOR_LEGACY_USE_ONLY, digest};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
use std::{io::Write, process::Command};
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    source: PathBuf,
    root: PathBuf,
    manifest: PathBuf,
    recipe: Value,
}

fn write(base: &Path, path: &str, data: &[u8]) -> Result<()> {
    let file = base.join(path);
    fs::create_dir_all(file.parent().unwrap())?;
    fs::write(file, data)?;
    Ok(())
}

fn identity(contents: &[u8]) -> Value {
    let sha1: String = digest(&SHA1_FOR_LEGACY_USE_ONLY, contents)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    json!({"sha1":sha1,"size":contents.len()})
}

#[test]
fn installed_import_selects_clients_and_keeps_worlds_private() -> Result<()> {
    use std::process::Stdio;
    let f = Fixture::new()?;
    let source = f.source.join(".minecraft");
    let mut vanilla: Value = serde_json::from_slice(&fs::read(f.source.join("vanilla.json"))?)?;
    vanilla["mainClass"] = json!("net.minecraft.client.main.Main");
    write(
        &source,
        "versions/base/base.json",
        &serde_json::to_vec(&vanilla)?,
    )?;
    write(&source, "versions/base/base.jar", b"client")?;
    let mut fabric: Value = serde_json::from_slice(&fs::read(f.source.join("fabric.json"))?)?;
    fabric["inheritsFrom"] = json!("base");
    write(
        &source,
        "versions/中文包/中文包.json",
        &serde_json::to_vec(&fabric)?,
    )?;
    write(&source, "assets/indexes/5.json", br#"{"objects":{}}"#)?;
    let library = "libraries/net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar";
    write(&source.join("versions/中文包"), library, b"loader")?;
    write(&source, "mods/root.jar", b"root")?;
    let isolated = source.join("versions/中文包");
    fs::create_dir_all(isolated.join("defaultconfigs/empty"))?;
    for (path, data) in [
        ("mods/mod.jar", "mod"),
        ("mods/old.jar.disabled", "off"),
        ("saves/World/level.dat", "save"),
        ("saves/Cache/level.dat", "another world"),
        ("config/a.json", "config"),
        ("options.txt", "options"),
        ("resourcepacks/pack.zip", "rp"),
        ("shaderpacks/shader.zip", "shader"),
        ("patchouli_books/b/book.json", "book"),
        ("fancymenu_data/menu.json", "menu"),
        ("schematics/file", "skip"),
        ("PCL/key", "private"),
        ("logs/latest.log", "log"),
    ] {
        write(&isolated, path, data.as_bytes())?;
    }
    // Select all versions, skip extras for vanilla, choose isolated data and a new ID for Fabric.
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcdocker"))
        .arg("--workspace")
        .arg(&f.root)
        .arg("import")
        .arg(&source)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"all\n\n1\ncustom\n1,3\n")?;
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        mcdocker::info(&f.root, "base")?.loader,
        mcdocker::Loader::Vanilla
    );
    let dir = f.root.join("instances/custom/game");
    assert!(dir.join("defaultconfigs/empty").is_dir());
    for path in [
        "mods/mod.jar",
        "saves/World/level.dat",
        "saves/Cache/level.dat",
        "resourcepacks/pack.zip",
        "patchouli_books/b/book.json",
        "fancymenu_data/menu.json",
    ] {
        assert!(dir.join(path).is_file(), "{path}");
    }
    for path in [
        "mods/root.jar",
        "mods/old.jar.disabled",
        "config",
        "options.txt",
        "shaderpacks",
        "schematics",
        "PCL",
        "logs",
    ] {
        assert!(!dir.join(path).exists(), "{path}");
    }
    fs::write(dir.join("saves/World/level.dat"), b"changed")?;
    assert_eq!(fs::read(isolated.join("saves/World/level.dat"))?, b"save");
    assert!(f.blobs()?.iter().all(|p| fs::read(p).unwrap() != b"save"));
    assert!(mcdocker::installed::discover(&f.source).is_err());
    Ok(())
}

#[test]
fn import_flat_client_asks_before_downloading_and_keeps_source_intact() -> Result<()> {
    let f = Fixture::new()?;
    let source = f.source.join(".minecraft");
    let mut vanilla: Value = serde_json::from_slice(&fs::read(f.source.join("vanilla.json"))?)?;
    vanilla["mainClass"] = json!("net.minecraft.client.main.Main");
    vanilla["assetIndex"]["url"] = json!("https://piston-meta.mojang.com/an-index");
    write(&source, "base.json", &serde_json::to_vec(&vanilla)?)?;
    write(&source, "base.jar", b"client")?;
    let (clients, warnings) = mcdocker::installed::discover(&source)?;
    assert!(warnings.is_empty());
    let asked = std::cell::RefCell::new(Vec::new());
    let res = mcdocker::installed::import(
        &f.root,
        &clients[0],
        "check",
        0,
        &Default::default(),
        |file| {
            asked.borrow_mut().push(file.to_owned());
            Ok(false)
        },
    );
    assert!(res.is_err());
    assert_eq!(*asked.borrow(), ["assets/indexes/5.json"]);
    assert!(!source.join("assets").exists());
    assert!(mcdocker::list(&f.root)?.is_empty());
    write(&source, "assets/indexes/5.json", br#"{"objects":{}}"#)?;
    mcdocker::installed::import(
        &f.root,
        &clients[0],
        "check",
        0,
        &Default::default(),
        |_| panic!("unexpected download"),
    )?;
    Ok(())
}

#[test]
fn bootstrap_import_requires_generated_minecraft_classes() -> Result<()> {
    let mut f = Fixture::new()?;
    f.recipe.as_object_mut().unwrap().remove("fabric");
    f.recipe["loader"] = json!("neoforge");
    f.recipe["loaderVersion"] = json!("v1");
    fs::write(&f.manifest, serde_json::to_vec(&f.recipe)?)?;
    let profile = json!({"id":"neoforge-v1","inheritsFrom":"1.20.1","mainClass":"cpw.mods.bootstraplauncher.BootstrapLauncher",
        "libraries":[],"arguments":{"game":["--fml.neoFormVersion","maps"]}});
    write(&f.source, "fabric.json", &serde_json::to_vec(&profile)?)?;
    let generated = "libraries/net/minecraft/client/1.20.1-maps/client-1.20.1-maps";
    write(&f.source, &format!("{generated}-extra.jar"), b"assets")?;
    for classifier in ["client", "universal"] {
        write(
            &f.source,
            &format!("libraries/net/neoforged/neoforge/v1/neoforge-v1-{classifier}.jar"),
            b"patch",
        )?;
    }
    let error = f.install("test").unwrap_err();
    assert!(format!("{error:#}").contains("-srg.jar"));
    assert!(mcdocker::list(&f.root)?.is_empty());
    write(&f.source, &format!("{generated}-srg.jar"), b"classes")?;
    f.install("test")?;
    Ok(())
}

#[test]
fn archive_loaders_keep_distinct_launch_plans_for_the_same_version() -> Result<()> {
    let f = Fixture::new()?;
    let mc = "1.20.1";
    let version = "1.2.3";
    for (from, to) in [
        ("vanilla.json", "versions/1.20.1/1.20.1.json"),
        ("client.jar", "versions/1.20.1/1.20.1.jar"),
    ] {
        write(&f.source, to, &fs::read(f.source.join(from))?)?;
    }
    for (kind, id, format) in [
        ("forge", "1.20.1-forge-1.2.3", "manifest.json"),
        ("neoforge", "neoforge-1.2.3", "modrinth.index.json"),
    ] {
        let main = format!("example.{kind}.Client");
        let profile = json!({"id":id,"inheritsFrom":mc,"mainClass":main,
            "libraries":[{"name":"example:platform:1:client"}],
            "arguments":{"game":["--launchTarget",kind],"jvm":[{"rules":[{"action":"allow","os":{"name":"windows"}}],"value":["-Ddir=${library_directory}"]},{"rules":[{"action":"allow","os":{"name":"linux"}}],"value":"-Dwrong=true"}]}});
        write(
            &f.source,
            &format!("versions/{id}/{id}.json"),
            &serde_json::to_vec(&profile)?,
        )?;
        write(
            &f.source,
            "libraries/example/platform/1/platform-1-client.jar",
            b"client library",
        )?;
        let manifest = if kind == "forge" {
            json!({"manifestType":"minecraftModpack","manifestVersion":1,"minecraft":{"version":mc,"modLoaders":[{"id":format!("forge-{version}")}]},"files":[]})
        } else {
            json!({"formatVersion":1,"game":"minecraft","dependencies":{"minecraft":mc,"neoforge":version},"files":[]})
        };
        let pack = f._dir.path().join(format!("{kind}.zip"));
        pack_zip(&pack, format, &manifest, &[])?;
        mcdocker::install_pack(&f.root, kind, &pack, Some(&f.source), None, false)?;
        let clone = format!("{kind}-copy");
        mcdocker::install_pack(&f.root, &clone, &pack, None, None, false)?;
        let info = mcdocker::info(&f.root, &clone)?;
        assert_eq!(info.loader.name().to_ascii_lowercase(), kind);
        assert_eq!(info.loader_version, version);
        let reference: Value = serde_json::from_slice(&fs::read(
            f.root.join("instances").join(&clone).join("profile.json"),
        )?)?;
        let generation: Value =
            serde_json::from_slice(&fs::read(f.root.join("generations").join(format!(
                "{}.json",
                reference["generation"].as_str().unwrap()
            )))?)?;
        assert_eq!(generation["mainClass"], main);
        assert!(
            generation["arguments"]["jvm"]
                .as_array()
                .unwrap()
                .contains(&json!("-Ddir=${library_directory}"))
        );
        assert!(
            !generation["arguments"]["jvm"]
                .as_array()
                .unwrap()
                .contains(&json!("-Dwrong=true"))
        );
        assert_eq!(
            generation["arguments"]["game"].as_array().unwrap().last(),
            Some(&json!(kind))
        );
    }
    Ok(())
}

#[test]
#[ignore = "set MCDOCKER_TEST_JAVA to java.exe from a Windows x64 JDK 17"]
fn shared_packs_reach_the_client_on_launch_and_quick() -> Result<()> {
    let a = Fixture::new()?;
    let b = Fixture::new()?;
    let tmp = a._dir.path();
    let java = PathBuf::from(std::env::var_os("MCDOCKER_TEST_JAVA").context("MCDOCKER_TEST_JAVA")?);
    let bin = java.parent().unwrap();
    fs::write(
        tmp.join("KnotClient.java"),
        r#"
package net.fabricmc.loader.impl.launch.knot;
import java.nio.file.*;
import java.util.Arrays;
public class KnotClient {
    public static void main(String[] args) throws Exception {
        boolean active = Files.exists(Path.of("resourcepacks/Test.zip"));
        boolean shader = Files.exists(Path.of("shaderpacks/Sky/shaders/basic.fsh"));
        if (active != shader || active != Boolean.parseBoolean(System.getenv("EXPECT_PACKS")))
            throw new AssertionError("shared packs in game directory");
        if (Arrays.asList(args).contains("--quickPlaySingleplayer") != Files.exists(Path.of("saves/World")))
            throw new AssertionError("quick play arguments");
        Files.writeString(Path.of("observed-jvm.txt"), System.getProperty("fixture.jvm", ""));
        Files.write(Path.of("jvm-options.txt"), java.lang.management.ManagementFactory.getRuntimeMXBean().getInputArguments());
        Files.write(Path.of("observed-game.txt"), Arrays.asList(args));
    }
}"#,
    )?;
    assert!(
        Command::new(bin.join("javac.exe"))
            .current_dir(tmp)
            .args(["-d", ".", "KnotClient.java"])
            .status()?
            .success()
    );
    let jar = "libraries/net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar";
    assert!(
        Command::new(bin.join("jar.exe"))
            .current_dir(tmp)
            .arg("cf")
            .arg(a.source.join(jar))
            .arg("net")
            .status()?
            .success()
    );
    fs::copy(a.source.join(jar), b.source.join(jar))?;
    a.install("first")?;
    b.install("second")?;
    write(
        &b.root,
        "instances/second/game/saves/World/level.dat",
        b"save",
    )?;
    let pool = tmp.join("public packs");
    let run = |root: &Path, args: &[&str], active: bool| -> Result<String> {
        let output = Command::new(env!("CARGO_BIN_EXE_mcdocker"))
            .arg("--workspace")
            .arg(root)
            .args(args)
            .env("APPDATA", tmp.join("profile"))
            .env("EXPECT_PACKS", active.to_string())
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    };
    run(
        &a.root,
        &["resources", "use", pool.to_str().unwrap()],
        false,
    )?;
    let displayed = run(&b.root, &["resources", "show"], false)?;
    assert_eq!(Path::new(&displayed).canonicalize()?, pool.canonicalize()?);
    write(&pool, "resourcepacks/Test.zip", b"test")?;
    write(&pool, "shaderpacks/Sky/shaders/basic.fsh", b"shader")?;
    let java = java.to_str().unwrap();
    write(
        &a.root,
        "instances/first/launch-args.toml",
        br#"[jvm]
max_memory = "256M"
extra = ["-Dfixture.jvm=saved","-Xmx512M"]
[game]
width = 1280
fullscreen = false
extra = ["--width=800", "--fullscreen", "--label", "saved value"]
"#,
    )?;
    run(
        &a.root,
        &[
            "launch",
            "first",
            "--java",
            java,
            "--jvm-arg=-Dfixture.jvm=one shot",
            "--jvm-arg=-XX:MaxHeapSize=768M",
            "--",
            "--width",
            "640",
            "--test",
            "two words",
        ],
        true,
    )?;
    let first = a.root.join("instances/first/game");
    assert_eq!(
        fs::read_to_string(first.join("observed-jvm.txt"))?,
        "one shot"
    );
    let args = fs::read_to_string(first.join("observed-game.txt"))?;
    assert_eq!(args.lines().filter(|a| *a == "--width").count(), 1);
    assert!(
        args.lines()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|p| p == ["--width", "1280"])
    );
    assert!(
        !args
            .lines()
            .any(|a| matches!(a, "640" | "--width=800" | "--fullscreen"))
    );
    let jvm = fs::read_to_string(first.join("jvm-options.txt"))?;
    assert_eq!(
        jvm.lines()
            .filter(|s| s.starts_with("-Xmx") || s.starts_with("-XX:MaxHeapSize="))
            .collect::<Vec<_>>(),
        ["-Xmx256M"]
    );
    assert!(args.lines().collect::<Vec<_>>().ends_with(&[
        "--label",
        "saved value",
        "--test",
        "two words"
    ]));
    run(
        &b.root,
        &[
            "quick",
            "second",
            "World",
            "--java",
            java,
            "--jvm-arg=-Dfixture.jvm=quick",
            "--",
            "--quick-test",
        ],
        true,
    )?;
    let second = b.root.join("instances/second/game");
    assert_eq!(
        fs::read_to_string(second.join("observed-jvm.txt"))?,
        "quick"
    );
    assert!(
        fs::read_to_string(second.join("observed-game.txt"))?
            .lines()
            .collect::<Vec<_>>()
            .ends_with(&["--quickPlaySingleplayer", "World", "--quick-test"])
    );
    run(&b.root, &["resources", "clear"], false)?;
    run(&a.root, &["launch", "first", "--java", java], false)?;
    assert_eq!(fs::read_to_string(first.join("observed-jvm.txt"))?, "saved");
    assert!(!fs::read_to_string(first.join("observed-game.txt"))?.contains("two words"));
    assert!(pool.join("resourcepacks/Test.zip").is_file());
    Ok(())
}

impl Fixture {
    fn new() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("local install");
        let root = dir.path().join("data");
        let manifest = dir.path().join("fixture.json");
        let index = br#"{"objects":{}}"#;
        let mut index_meta = identity(index);
        index_meta["id"] = json!("5");
        let vanilla = json!({"id":"1.20.1","javaVersion":{"majorVersion":17},
            "downloads":{"client":identity(b"client")},"assetIndex":index_meta,"libraries":[
            {"name":"skip:linux:1","rules":[{"action":"allow","os":{"name":"linux"}}]}]});
        write(&source, "vanilla.json", &serde_json::to_vec(&vanilla)?)?;
        write(&source, "client.jar", b"client")?;
        write(&source, "assets/indexes/5.json", index)?;
        let loader = json!({"inheritsFrom":"1.20.1","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient",
            "libraries":[{"name":"net.fabricmc:fabric-loader:0.16.9","rules":null}]});
        write(&source, "fabric.json", &serde_json::to_vec(&loader)?)?;
        write(
            &source,
            "libraries/net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar",
            b"loader",
        )?;
        write(&source, "a.jar", b"mod")?;
        write(&source, "b.jar", b"mod")?;
        write(&source, "options.txt", b"initial")?;
        let recipe = json!({"schemaVersion":1,"platform":"windows-x64","javaMajor":17,"minecraft":"1.20.1",
            "fabric":"0.16.9","libraries":"libraries","assets":"assets","clientJar":"client.jar",
            "clientMetadata":"vanilla.json","loaderMetadata":"fabric.json","packFiles":[
                {"role":"mod","source":"a.jar","target":"mods/a.jar"},
                {"role":"mod","source":"b.jar","target":"mods/b.jar"},
                {"role":"template","source":"options.txt","target":"options.txt"}]});
        fs::write(&manifest, serde_json::to_vec(&recipe)?)?;
        Ok(Self {
            _dir: dir,
            source,
            root,
            manifest,
            recipe,
        })
    }

    fn install(&self, id: &str) -> Result<()> {
        mcdocker::import(&self.root, id, &self.manifest, &self.source)
    }

    fn blobs(&self) -> Result<Vec<PathBuf>> {
        let mut blobs = vec![];
        for prefix in fs::read_dir(self.root.join("store/blobs/sha256"))? {
            for file in fs::read_dir(prefix?.path())? {
                blobs.push(file?.path());
            }
        }
        Ok(blobs)
    }
}

#[test]
fn concurrent_imports_share_bytes_and_preserve_private_files() -> Result<()> {
    let f = Fixture::new()?;
    std::thread::scope(|s| -> Result<()> {
        let a = s.spawn(|| f.install("alpha"));
        f.install("beta")?;
        a.join().unwrap()
    })?;
    assert_eq!(mcdocker::list(&f.root)?, ["alpha", "beta"]);
    assert_eq!(f.blobs()?.len(), 5);
    let alpha = f.root.join("instances/alpha/game");
    let beta = f.root.join("instances/beta/game");
    fs::write(alpha.join("mods/a.jar"), b"changed")?;
    fs::write(alpha.join("options.txt"), b"settings")?;
    write(&alpha, "saves/world/level.dat", b"save")?;
    assert_eq!(fs::read(alpha.join("mods/b.jar"))?, b"mod");
    assert_eq!(fs::read(beta.join("mods/a.jar"))?, b"mod");
    assert_eq!(fs::read(beta.join("options.txt"))?, b"initial");
    assert!(!beta.join("saves/world/level.dat").exists());
    assert!(f.install("alpha").is_err());
    assert_eq!(fs::read(alpha.join("options.txt"))?, b"settings");
    Ok(())
}

#[test]
fn maintenance_counts_shared_objects_once_and_names_affected_instances() -> Result<()> {
    let f = Fixture::new()?;
    f.install("alpha")?;
    f.install("beta")?;
    let blobs = f.blobs()?;
    let expected = blobs
        .iter()
        .map(|p| fs::metadata(p).unwrap().len())
        .sum::<u64>();
    let original = mcdocker::maintenance::usage(&f.root)?;
    write(
        &f.root,
        "instances/alpha/game/saves/test/level.dat",
        b"save",
    )?;
    write(&f.root, "work/run-test/client.jar", b"client")?;
    write(&f.root, "work/install-test/file", b"unrelated")?;
    let usage = mcdocker::maintenance::usage(&f.root)?;
    assert_eq!(
        (usage.store_bytes, usage.referenced_bytes, usage.saved_bytes),
        (expected, expected, expected)
    );
    assert_eq!(usage.runtime_copy_bytes, 6);
    assert_eq!(
        usage.private_bytes_by_instance["alpha"],
        original.private_bytes_by_instance["alpha"] + 4
    );
    assert_eq!(
        usage.private_bytes_by_instance["beta"],
        original.private_bytes_by_instance["beta"]
    );
    let report = mcdocker::maintenance::verify(&f.root)?;
    assert_eq!(report.checked_objects, blobs.len());
    assert!(report.issues.is_empty());

    fs::remove_file(&blobs[0])?;
    let mut corrupt = fs::read(&blobs[1])?;
    corrupt[0] ^= 1;
    fs::write(&blobs[1], corrupt)?;
    let output = Command::new(env!("CARGO_BIN_EXE_mcdocker"))
        .arg("--workspace")
        .arg(&f.root)
        .args(["verify", "--json"])
        .output()?;
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout)?;
    let issues = report["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 2);
    for issue in issues {
        assert_eq!(issue["instances"], json!(["alpha", "beta"]));
    }
    fs::write(f.root.join("instances/alpha/profile.json"), b"invalid")?;
    let report = mcdocker::maintenance::verify(&f.root)?;
    assert_eq!(report.issues.len(), 3);
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.file.ends_with("alpha/profile.json"))
    );
    Ok(())
}

#[test]
fn integrity_failures_never_publish_a_partial_instance() -> Result<()> {
    let f = Fixture::new()?;
    fs::write(f.source.join("client.jar"), b"cut")?;
    assert!(
        f.install("first")
            .unwrap_err()
            .to_string()
            .contains("SHA-1")
    );
    assert!(mcdocker::list(&f.root)?.is_empty());
    fs::write(f.source.join("client.jar"), b"client")?;
    f.install("first")?;
    let blob = f
        .blobs()?
        .into_iter()
        .find(|p| fs::read(p).unwrap() == b"mod")
        .unwrap();
    fs::write(blob, b"bad")?;
    assert!(
        f.install("second")
            .unwrap_err()
            .to_string()
            .contains("damaged blob")
    );
    assert_eq!(mcdocker::list(&f.root)?, ["first"]);
    Ok(())
}

#[test]
fn unsupported_schema_and_unsafe_or_colliding_targets_are_rejected() -> Result<()> {
    let mut f = Fixture::new()?;
    for target in [
        "mods/A.jar",
        "mods/AUX.jar",
        "mods/a.jar:stream",
        "mods/../escape.jar",
        "/outside",
    ] {
        f.recipe["packFiles"][1]["target"] = json!(target);
        fs::write(&f.manifest, serde_json::to_vec(&f.recipe)?)?;
        assert!(f.install("bad").is_err(), "{target}");
        assert!(mcdocker::list(&f.root)?.is_empty());
    }
    f.recipe["packFiles"] = json!([
        {"role":"template","source":"options.txt","target":"config/Example/a.txt"},
        {"role":"template","source":"options.txt","target":"config/example/b.txt"}
    ]);
    fs::write(&f.manifest, serde_json::to_vec(&f.recipe)?)?;
    assert!(
        f.install("bad")
            .unwrap_err()
            .to_string()
            .contains("directory case collision")
    );
    f.recipe["schemaVersion"] = json!(2);
    fs::write(&f.manifest, serde_json::to_vec(&f.recipe)?)?;
    assert!(
        format!("{:#}", f.install("future").unwrap_err()).contains("unsupported schemaVersion 2")
    );
    Ok(())
}

#[cfg(windows)]
#[test]
#[ignore = "set MCDOCKER_TEST_JAVA to a Windows x64 Java 17 executable"]
fn failed_java_releases_run_lock_and_leaves_worlds_in_place() -> Result<()> {
    let f = Fixture::new()?;
    f.install("alpha")?;
    let java = PathBuf::from(std::env::var_os("MCDOCKER_TEST_JAVA").expect("MCDOCKER_TEST_JAVA"));
    let player = mcdocker::Player {
        name: "Player".into(),
        uuid: "00000000-0000-4000-8000-000000000001".into(),
    };
    let launch = |world| mcdocker::launch(&f.root, "alpha", &java, &player, world);
    let instance = f.root.join("instances/alpha");
    let held = fs::File::create(instance.join("run.lock"))?;
    held.lock()?;
    assert!(launch(None).unwrap_err().to_string().contains("busy"));
    drop(held);
    assert!(launch(Some("../other")).is_err());
    assert!(
        launch(Some("Missing"))
            .unwrap_err()
            .to_string()
            .contains("no saved world")
    );
    write(
        &instance,
        "game/saves/Test World/level.dat",
        b"private save",
    )?;
    // The fixture JAR has no main class, so the real JVM exits unsuccessfully.
    for _ in 0..2 {
        assert!(!launch(Some("Test World"))?.success());
        assert_eq!(fs::read_dir(f.root.join("work"))?.count(), 0);
        assert_eq!(
            fs::read(instance.join("game/saves/Test World/level.dat"))?,
            b"private save"
        );
    }
    Ok(())
}

fn pack_zip(
    path: &Path,
    manifest_name: &str,
    manifest: &Value,
    files: &[(&str, &[u8])],
) -> Result<()> {
    let mut zip = zip::ZipWriter::new(fs::File::create(path)?);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file(manifest_name, options)?;
    zip.write_all(&serde_json::to_vec(manifest)?)?;
    for &(name, bytes) in files {
        zip.start_file(name, options)?;
        zip.write_all(bytes)?;
    }
    zip.finish()?;
    Ok(())
}

#[test]
fn terminal_workflow_with_persistent_workspace_and_two_archive_formats() -> Result<()> {
    let mut f = Fixture::new()?;
    let appdata = f._dir.path().join("settings");
    let (script, editor, body) = if cfg!(windows) {
        (
            "editor.cmd",
            "cmd /D /C editor.cmd",
            " @copy /Y \"%~1\" opened.json >nul\r\n @copy /Y next.json \"%~1\" >nul\r\n",
        )
    } else {
        (
            "editor.sh",
            "sh editor.sh",
            "cp \"$1\" opened.json && cp next.json \"$1\"\n",
        )
    };
    write(f._dir.path(), script, body.as_bytes())?;
    let cli = |args: &[&str]| -> Result<String> {
        let out = Command::new(env!("CARGO_BIN_EXE_mcdocker"))
            .args(args)
            .current_dir(f._dir.path())
            .env("APPDATA", &appdata)
            .env("VISUAL", editor)
            .env_remove("MCDOCKER_WORKSPACE")
            .env_remove("MCDOCKER_SOURCE")
            .env_remove("CURSEFORGE_API_KEY")
            .output()?;
        anyhow::ensure!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(String::from_utf8(out.stdout)?.trim().to_owned())
    };
    cli(&["workspace", "use", f.root.to_str().unwrap()])?;
    let mut meta: Value = serde_json::from_slice(&fs::read(f.source.join("vanilla.json"))?)?;
    meta["javaVersion"]["majorVersion"] = json!(21);
    fs::write(f.source.join("vanilla.json"), serde_json::to_vec(&meta)?)?;
    f.recipe.as_object_mut().unwrap().remove("javaMajor");
    fs::write(&f.manifest, serde_json::to_vec(&f.recipe)?)?;
    cli(&[
        "install",
        "fixture.json",
        "--source",
        "local install",
        "--id",
        "base",
    ])?;
    let hashes = json!({"sha1":identity(b"mod")["sha1"],"sha512":digest(&ring::digest::SHA512,b"mod").as_ref().iter().map(|b|format!("{b:02x}")).collect::<String>()});
    let entry = json!({"path":"mods/remote.jar","hashes":hashes,"fileSize":3,"downloads":[]});
    let mut server = entry.clone();
    server["path"] = json!("mods/server.jar");
    server["env"] = json!({"client":"unsupported"});
    let mr = json!({"formatVersion":1,"game":"minecraft","dependencies":{"minecraft":"1.20.1","fabric-loader":"0.16.9"},"files":[entry,server]});
    pack_zip(
        &f._dir.path().join("one.mrpack"),
        "modrinth.index.json",
        &mr,
        &[
            ("overrides/config/demo", b"common"),
            ("client-overrides/config/demo", b"client"),
        ],
    )?;
    write(f._dir.path(), "manual/mods/remote.jar", b"bad")?;
    assert!(cli(&["install", "one.mrpack", "--manual", "manual"]).is_err());
    write(f._dir.path(), "manual/mods/remote.jar", b"mod")?;
    cli(&["install", "one.mrpack", "--manual", "manual"])?;
    assert_eq!(
        fs::read(f.root.join("instances/one/game/config/demo"))?,
        b"client"
    );
    assert!(!f.root.join("instances/one/game/mods/server.jar").exists());
    let cf = json!({"manifestType":"minecraftModpack","manifestVersion":1,"minecraft":{"version":"1.20.1","modLoaders":[{"id":"fabric-0.16.9"}]},
        "files":[{"projectID":12,"fileID":34,"required":true},{"projectID":56,"fileID":78,"required":false}],"overrides":"custom"});
    pack_zip(
        &f._dir.path().join("two.zip"),
        "manifest.json",
        &cf,
        &[("custom/options.txt", b"options")],
    )?;
    assert!(
        cli(&["install", "two.zip"])
            .unwrap_err()
            .to_string()
            .contains("12/34")
    );
    write(f._dir.path(), "manual/12/34/demo.jar", b"mod")?;
    cli(&["install", "two.zip", "--manual", "manual"])?;
    assert_eq!(
        cli(&["list"])?.lines().collect::<Vec<_>>(),
        ["base", "one", "two"]
    );
    cli(&["use", "two"])?;
    let table = cli(&["ls", "-l"])?;
    assert!(table.lines().next().unwrap().contains("LOADER VERSION"));
    assert!(
        table
            .lines()
            .any(|row| row.split_whitespace().collect::<Vec<_>>()
                == ["two", "1.20.1", "Fabric", "0.16.9", "21", "0"])
    );
    let legacy = br#"{"jvm":["-Xmx2G"],"game":[]}"#;
    write(&f.root, "instances/two/launch-args.json", legacy)?;
    assert_eq!(mcdocker::saved_args(&f.root, "two")?.jvm, ["-Xmx2G"]);
    let saved = f.root.join("instances/two/launch-args.toml");
    let settings = b"[jvm]\nmax_memory = '3G'\n[game]\n# window size\nwidth = 1280\n";
    write(f._dir.path(), "next.json", settings)?;
    cli(&["config"])?;
    assert_eq!(fs::read(&saved)?, settings);
    let opened: toml::Table =
        toml::from_str(&fs::read_to_string(f._dir.path().join("opened.json"))?)?;
    assert_eq!(opened["jvm"]["extra"][0].as_str(), Some("-Xmx2G"));
    write(f._dir.path(), "next.json", b"[game]\nwidth = -1")?;
    assert!(
        cli(&["config", "two"])
            .unwrap_err()
            .to_string()
            .contains("Invalid launch configuration")
    );
    assert_eq!(fs::read(f._dir.path().join("opened.json"))?, settings);
    assert_eq!(fs::read(&saved)?, settings);
    fs::write(&saved, b"jvm = ['-Xms64M']\ngame = ['--width','800']\n")?;
    assert_eq!(
        mcdocker::saved_args(&f.root, "two")?.game,
        ["--width", "800"]
    );
    write(f._dir.path(), "next.json", settings)?;
    cli(&["config", "two"])?;
    for removed in ["set", "show"] {
        assert!(
            cli(&["args", removed])
                .unwrap_err()
                .to_string()
                .contains("unrecognized subcommand")
        );
    }
    cli(&["args", "clear"])?;
    let cleared = mcdocker::saved_args(&f.root, "two")?;
    assert!(cleared.jvm.is_empty() && cleared.game.is_empty());
    assert!(
        cli(&["quick", "Missing"])
            .unwrap_err()
            .to_string()
            .contains("in two")
    );
    assert!(
        cli(&["quick", "base", "Missing"])
            .unwrap_err()
            .to_string()
            .contains("in base")
    );
    write(
        &f.root,
        "instances/two/game/saves/My World/level.dat",
        b"saved",
    )?;
    let info: Value = serde_json::from_str(&cli(&["info", "--json"])?)?;
    assert_eq!(info["javaMajor"], 21);
    assert_eq!(info["worlds"], json!(["My World"]));
    let held = fs::File::create(f.root.join("instances/two/run.lock"))?;
    held.lock()?;
    assert!(
        cli(&["uninstall"])
            .unwrap_err()
            .to_string()
            .contains("busy")
    );
    drop(held);
    cli(&["uninstall"])?;
    assert!(cli(&["info"]).is_err());
    assert_eq!(cli(&["list"])?.lines().collect::<Vec<_>>(), ["base", "one"]);
    let backup = fs::read_dir(f.root.join("removed"))?
        .next()
        .unwrap()?
        .path();
    assert_eq!(
        fs::read(backup.join("two/game/saves/My World/level.dat"))?,
        b"saved"
    );
    // A non-selected explicit workspace must take precedence over saved settings.
    cli(&["uninstall", "one", "--delete-data"])?;
    assert_eq!(cli(&["list"])?, "base");
    assert_eq!(fs::read_dir(f.root.join("removed"))?.count(), 1);
    assert_eq!(
        cli(&["--root", "some-other-workspace", "instance", "list"])?,
        ""
    );
    Ok(())
}

#[test]
fn rejects_zip_traversal_before_creating_an_instance() -> Result<()> {
    let f = Fixture::new()?;
    let archive = f._dir.path().join("bad.zip");
    pack_zip(
        &archive,
        "manifest.json",
        &json!({}),
        &[("overrides/../outside", b"x")],
    )?;
    let err = mcdocker::install_pack(&f.root, "bad", &archive, None, None, false).unwrap_err();
    assert!(err.to_string().contains("unsafe Windows path component"));
    assert!(mcdocker::list(&f.root)?.is_empty());
    Ok(())
}

#[test]
#[ignore = "downloads the Fabric pack in MCDOCKER_TEST_PACK and its base game into a temporary workspace"]
fn installs_public_pack_into_empty_workspace() -> Result<()> {
    let pack =
        PathBuf::from(std::env::var_os("MCDOCKER_TEST_PACK").context("set MCDOCKER_TEST_PACK")?);
    let tmp = tempfile::tempdir_in("target")?;
    let output = Command::new(env!("CARGO_BIN_EXE_mcdocker"))
        .env("APPDATA", tmp.path())
        .env_remove("MCDOCKER_SOURCE")
        .arg("--workspace")
        .arg(tmp.path().join("workspace"))
        .arg("install")
        .arg(pack)
        .args(["--id", "public-pack"])
        .status()?;
    anyhow::ensure!(output.success(), "public pack installation failed");
    assert_eq!(
        mcdocker::list(&tmp.path().join("workspace"))?,
        ["public-pack"]
    );
    Ok(())
}
