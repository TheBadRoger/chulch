use super::fixture::{Fixture, write};
use anyhow::Result;
use serde_json::{Value, json};
use std::{fs, io::Write, process::Command};

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
    let mut child = Command::new(env!("CARGO_BIN_EXE_chulch"))
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
        chulch::info(&f.root, "base")?.loader,
        chulch::Loader::Vanilla
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
    assert!(chulch::installed::discover(&f.source).is_err());
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
    let (clients, warnings) = chulch::installed::discover(&source)?;
    assert!(warnings.is_empty());
    let asked = std::cell::RefCell::new(Vec::new());
    let res = chulch::installed::import(
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
    assert!(chulch::list(&f.root)?.is_empty());
    write(&source, "assets/indexes/5.json", br#"{"objects":{}}"#)?;
    chulch::installed::import(
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
    assert!(chulch::list(&f.root)?.is_empty());
    write(&f.source, &format!("{generated}-srg.jar"), b"classes")?;
    f.install("test")?;
    Ok(())
}
