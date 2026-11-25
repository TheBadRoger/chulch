use crate::{generation, instance, lock, paths::under};
mod options;
use anyhow::{Context, Result, bail, ensure};
use options::Settings;
use std::{
    env, fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    process::Command,
};

fn open_editor(path: &Path) -> Result<()> {
    let configured = ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(|key| env::var_os(key))
        .find(|value| !value.is_empty());
    if let Some(value) = configured {
        let mut command = if Path::new(&value).is_file() {
            Command::new(value)
        } else {
            let text = value.to_str().context("Invalid editor command")?;
            let words = shlex::split(text).context("Unbalanced quotes in editor command")?;
            let (program, args) = words.split_first().context("Empty editor command")?;
            let mut command = Command::new(program);
            command.args(args);
            command
        };
        let status = command
            .arg(path)
            .status()
            .context("Could not start the configured editor")?;
        ensure!(status.success(), "Editor exited with {status}");
        return Ok(());
    }

    let fallback = if cfg!(windows) { "notepad.exe" } else { "vi" };
    for program in ["vim", "nano", fallback] {
        match Command::new(program).arg(path).status() {
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            result => {
                let status = result.with_context(|| format!("Could not start {program}"))?;
                ensure!(status.success(), "{program} exited with {status}");
                return Ok(());
            }
        }
    }
    bail!("No text editor found. Set VISUAL or EDITOR to an installed editor.")
}

fn contents(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("Read {}", path.display())),
    }
}

const FILE_NAME: &str = "launch-args.toml";

#[derive(PartialEq)]
enum Source {
    Toml(Vec<u8>),
    Legacy(Vec<u8>),
    Empty,
}

impl Source {
    fn load(dir: &Path) -> Result<Self> {
        if let Some(bytes) = contents(&under(dir, FILE_NAME)?)? {
            return Ok(Self::Toml(bytes));
        }
        Ok(contents(&under(dir, "launch-args.json")?)?.map_or(Self::Empty, Self::Legacy))
    }

    fn settings(&self) -> Result<Settings> {
        let settings = match self {
            Self::Toml(bytes) => {
                let table = toml_table(bytes)?;
                if flat(&table) {
                    ensure!(
                        table
                            .keys()
                            .all(|key| matches!(key.as_str(), "jvm" | "game")),
                        "Unknown field in legacy launch configuration"
                    );
                    Settings::from_extra(table.try_into()?)
                } else {
                    table.try_into()?
                }
            }
            Self::Legacy(bytes) => Settings::from_extra(
                serde_json::from_slice(bytes).context("Invalid legacy launch-args.json")?,
            ),
            Self::Empty => Settings::default(),
        };
        settings.validate()?;
        Ok(settings)
    }
}

fn toml_table(bytes: &[u8]) -> Result<toml::Table> {
    Ok(toml::from_str(
        std::str::from_utf8(bytes)?.trim_start_matches('\u{feff}'),
    )?)
}

fn flat(table: &toml::Table) -> bool {
    ["jvm", "game"]
        .iter()
        .any(|key| table.get(*key).is_some_and(toml::Value::is_array))
}

pub(crate) fn read_settings(dir: &Path) -> Result<Settings> {
    Source::load(dir)?
        .settings()
        .with_context(|| format!("Read launch configuration in {}", dir.display()))
}

pub(crate) fn replace_groups(
    dir: &Path,
    jvm: Option<Vec<String>>,
    game: Option<Vec<String>>,
) -> Result<()> {
    let mut settings = if jvm.is_some() && game.is_some() {
        Settings::default()
    } else {
        read_settings(dir)?
    };
    if let Some(extra) = jvm {
        settings.jvm = options::Jvm::default();
        settings.jvm.extra = extra;
    }
    if let Some(extra) = game {
        settings.game = options::Game::default();
        settings.game.extra = extra;
    }
    settings.validate()?;
    let path = under(dir, FILE_NAME)?;
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    file.write_all(settings.template()?.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

/// Edit a draft so a failed edit leaves the live config intact.
pub fn edit(root: &Path, id: &str) -> Result<PathBuf> {
    let lock_path = under(root, "instances.lock")?;
    let global = lock(&lock_path, true)?;
    let dir = instance(root, id)?;
    generation(root, &dir)?;
    let before = Source::load(&dir)?;
    let profile = fs::read(under(&dir, "profile.json")?)?;
    drop(global);

    let draft = tempfile::Builder::new()
        .prefix("launch-args-")
        .suffix(".toml")
        .tempfile_in(under(root, "work")?)?
        .into_temp_path();
    match &before {
        Source::Toml(bytes) => {
            let migrated = toml_table(bytes)
                .ok()
                .filter(flat)
                .and_then(|_| before.settings().ok())
                .map(|settings| settings.template())
                .transpose()?;
            fs::write(
                &draft,
                migrated.as_deref().map(str::as_bytes).unwrap_or(bytes),
            )?;
        }
        _ => fs::write(&draft, before.settings()?.template()?)?,
    }
    open_editor(&draft)?;

    crate::paths::no_link(&draft)?;
    let validate = || -> Result<()> {
        let settings: Settings = toml_table(&fs::read(&draft)?)?.try_into()?;
        settings.validate()
    };
    validate().context("Invalid launch configuration (TOML)")?;

    let _global = lock(&lock_path, true)?;
    let dir = instance(root, id)?;
    let path = under(&dir, FILE_NAME)?;
    ensure!(
        fs::read(under(&dir, "profile.json")?)? == profile && Source::load(&dir)? == before,
        "The instance or its configuration changed while editing. Run config again."
    );
    fs::File::options().write(true).open(&draft)?.sync_all()?;
    draft.persist(&path)?;
    Ok(path)
}
