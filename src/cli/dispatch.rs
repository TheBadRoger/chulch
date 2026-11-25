use super::{
    AccountAction, AccountMode, ArgumentAction, Cli, Command, Instance, LaunchOptions, ListOptions,
    Resources, Workspace, completion, import_console,
};
use anyhow::{Context, Result};
use clap::Parser;
use mcdocker::workspace;
use std::{
    env,
    path::{Path, PathBuf},
};

fn launch(root: &Path, id: &str, world: Option<&str>, options: LaunchOptions) -> Result<i32> {
    let player;
    let session;
    let identity = match options.account {
        AccountMode::Offline => {
            player = mcdocker::Player {
                name: options.player.unwrap_or_else(|| "Player".into()),
                uuid: options
                    .uuid
                    .unwrap_or_else(|| "00000000-0000-4000-8000-000000000001".into()),
            };
            mcdocker::LaunchIdentity::Offline(&player)
        }
        AccountMode::Microsoft => {
            anyhow::ensure!(
                options.player.is_none() && options.uuid.is_none(),
                "--player and --uuid only apply to offline accounts"
            );
            session = mcdocker::account::session()?;
            mcdocker::LaunchIdentity::Microsoft(&session)
        }
    };
    let extra = mcdocker::LaunchArguments {
        jvm: options.extra.jvm_arg,
        game: options.extra.game_args,
    };
    let status =
        mcdocker::launch_with_identity(root, id, options.java.as_deref(), identity, world, &extra)?;
    eprintln!("Java: {status}");
    Ok(status.code().unwrap_or(1))
}

fn list(root: &Path, options: ListOptions) -> Result<()> {
    if !options.long {
        for id in mcdocker::list(root)? {
            println!("{id}");
        }
        return Ok(());
    }
    let mut rows = vec![
        [
            "INSTANCE",
            "MINECRAFT",
            "LOADER",
            "LOADER VERSION",
            "JAVA",
            "WORLDS",
        ]
        .map(String::from),
    ];
    for info in mcdocker::list_info(root)? {
        let version = if info.loader == mcdocker::Loader::Vanilla {
            "-".into()
        } else {
            info.loader_version
        };
        rows.push([
            info.id,
            info.minecraft,
            info.loader.name().into(),
            version,
            info.java_major.to_string(),
            info.worlds.len().to_string(),
        ]);
    }
    let widths: [usize; 6] =
        std::array::from_fn(|i| rows.iter().map(|row| row[i].len()).max().unwrap_or(0));
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            if index == row.len() - 1 {
                println!("{cell}");
            } else {
                print!("{cell:<width$}  ", width = widths[index]);
            }
        }
    }
    Ok(())
}

pub(crate) fn run() -> Result<i32> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Account { action } => {
            match action {
                AccountAction::Login { client_id } => {
                    let id = client_id.clone().or_else(|| env::var("MCDOCKER_MICROSOFT_CLIENT_ID").ok()).context("Supply --client-id or set MCDOCKER_MICROSOFT_CLIENT_ID to your registered public application ID")?;
                    println!("Signed in as {}", mcdocker::account::login(&id)?);
                }
                AccountAction::Status => match mcdocker::account::status()? {
                    Some((name, uuid)) => println!("Saved Minecraft profile: {name} ({uuid})"),
                    None => println!("No Microsoft account saved."),
                },
                AccountAction::Logout => {
                    mcdocker::account::logout()?;
                    println!("Local Microsoft sign-in data removed.");
                }
            }
            return Ok(0);
        }
        Command::Completions { shell } => return completion::script(*shell).map(|()| 0),
        Command::Complete => return completion::powershell().map(|()| 0),
        _ => {}
    }
    if let Command::Workspace {
        action: Some(Workspace::Use { path }),
    } = &cli.command
    {
        println!("{}", workspace::set_default(path)?.display());
        return Ok(0);
    }
    let root = workspace::resolve(cli.workspace)?;
    let current = |id: Option<String>| id.map(Ok).unwrap_or_else(|| workspace::selected(&root));
    match cli.command {
        Command::Completions { .. } | Command::Complete | Command::Account { .. } => unreachable!(),
        Command::Import { minecraft } => import_console::run(&root, &minecraft)?,
        Command::Resources { action } => match action.unwrap_or(Resources::Show) {
            Resources::Use { path } => {
                println!("{}", mcdocker::resources::set(&root, &path)?.display())
            }
            Resources::Show => match mcdocker::resources::current()? {
                Some(path) => println!("{}", path.display()),
                None => println!("No shared resource directory configured."),
            },
            Resources::Clear => {
                mcdocker::resources::clear()?;
                println!("Disconnected the shared resource directory.");
            }
        },
        Command::Workspace { .. } => println!("{}", root.display()),
        Command::Verify { json } => {
            let report = mcdocker::maintenance::verify(&root)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "{} instances, {} referenced objects checked, {} issues",
                    report.instances,
                    report.checked_objects,
                    report.issues.len()
                );
                for issue in &report.issues {
                    println!(
                        "[{}] {}: {}",
                        issue.instances.join(", "),
                        issue.file,
                        issue.reason
                    );
                }
            }
            return Ok(i32::from(!report.issues.is_empty()));
        }
        Command::Usage { json } => {
            let usage = mcdocker::maintenance::usage(&root)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&usage)?);
            } else {
                let size =
                    |bytes: u64| format!("{:.2} MiB ({bytes} bytes)", bytes as f64 / 1_048_576.0);
                println!("Object store: {}", size(usage.store_bytes));
                println!(
                    "Objects referenced by installed instances: {}",
                    size(usage.referenced_bytes)
                );
                println!(
                    "Saved by sharing across instances: {}",
                    size(usage.saved_bytes)
                );
                for (id, bytes) in usage.private_bytes_by_instance {
                    println!("Private game files [{id}]: {}", size(bytes));
                }
                println!(
                    "Temporary runtime copies: {}",
                    size(usage.runtime_copy_bytes)
                );
            }
        }
        Command::Use { id } => {
            workspace::select(&root, &id)?;
            println!("{id}");
        }
        Command::List(options)
        | Command::Instance {
            action: Instance::List(options),
        } => list(&root, options)?,
        Command::Config { id } => {
            let path = mcdocker::config::edit(&root, &current(id)?)?;
            println!("Saved {}", path.display());
        }
        Command::Args { action } => match action {
            ArgumentAction::Clear { id } => {
                let id = current(id)?;
                mcdocker::set_saved_args(&root, &id, Some(vec![]), Some(vec![]))?;
                println!("Cleared launch arguments for {id}");
            }
        },
        Command::Info { id, json } => {
            let info = mcdocker::info(&root, &current(id)?)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&info)?);
            } else {
                println!(
                    "{}\nMinecraft {}\n{} Loader {}\nJava {} ({})\nGame directory: {}\nWorlds: {}",
                    info.id,
                    info.minecraft,
                    info.loader.name(),
                    info.loader_version,
                    info.java_major,
                    info.platform,
                    info.game.display(),
                    info.worlds.join(", ")
                );
            }
        }
        Command::Java { id, install } => {
            let major = mcdocker::info(&root, &current(id)?)?.java_major;
            let j = mcdocker::java::select(&root, major, None, install)?;
            println!("{}  {}", j.version, j.executable.display());
        }
        Command::Launch { id, world, options } => {
            return launch(&root, &current(id)?, world.as_deref(), options);
        }
        Command::Quick {
            id_or_world,
            world,
            options,
        } => {
            let (id, save) = match world {
                Some(w) => (id_or_world, w),
                None => (current(None)?, id_or_world),
            };
            return launch(&root, &id, Some(&save), options);
        }
        Command::Uninstall { id, delete_data } => {
            let id = current(id)?;
            match mcdocker::uninstall(&root, &id, delete_data)? {
                Some(saved) => println!("Uninstalled {id}. Data retained in {}", saved.display()),
                None => println!("Uninstalled {id} and deleted its private data."),
            }
        }
        Command::Instance {
            action:
                Instance::Import {
                    id,
                    manifest,
                    source,
                },
        } => {
            mcdocker::import(&root, &id, &manifest, &source)?;
            println!("Installed {id}");
        }
        Command::Install {
            pack,
            id,
            source,
            manual,
            optional,
        } => {
            let id = id.unwrap_or_else(|| {
                pack.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .replace(|c: char| !c.is_ascii_alphanumeric() && c != '_', "-")
            });
            let source = source.or_else(|| env::var_os("MCDOCKER_SOURCE").map(PathBuf::from));
            if pack
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                mcdocker::import(
                    &root,
                    &id,
                    &pack,
                    source
                        .as_deref()
                        .context("a local recipe requires --source or MCDOCKER_SOURCE")?,
                )?;
            } else {
                mcdocker::install_pack(
                    &root,
                    &id,
                    &pack,
                    source.as_deref(),
                    manual.as_deref(),
                    optional,
                )?;
            }
            println!("Installed {id}");
        }
    }
    Ok(0)
}
