use super::fixture::{Fixture, write};
use anyhow::{Context, Result};
use std::{fs, path::Path, path::PathBuf, process::Command};

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
