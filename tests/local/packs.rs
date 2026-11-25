use super::fixture::{Fixture, pack_zip, write};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

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
