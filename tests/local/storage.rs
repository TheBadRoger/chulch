use super::fixture::{Fixture, write};
use anyhow::Result;
use serde_json::{Value, json};
use std::{fs, process::Command};

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
