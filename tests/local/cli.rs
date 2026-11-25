use super::fixture::{Fixture, identity, pack_zip, write};
use anyhow::Result;
use ring::digest::digest;
use serde_json::{Value, json};
use std::{fs, process::Command};

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
    assert!(cli(&["account", "status"])?.contains("No Microsoft account"));
    for cmd in [
        &["launch", "two", "--account", "microsoft"][..],
        &["quick", "two", "World", "--account", "microsoft"],
    ] {
        assert!(
            cli(cmd)
                .unwrap_err()
                .to_string()
                .contains("No Microsoft account saved")
        );
    }
    assert!(
        cli(&["run", "two", "--account", "microsoft", "--player", "Alex"])
            .unwrap_err()
            .to_string()
            .contains("offline accounts")
    );
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
