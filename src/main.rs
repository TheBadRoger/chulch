use anyhow::{Context, Result};
mod completion;
mod import_console;
use clap::{Args, Parser, Subcommand};
use mcdocker::workspace;
use std::{
    env,
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Parser)]
#[command(
    bin_name = "mcdocker",
    version,
    about = "Manage Minecraft instances from the terminal",
    after_help = "Examples:\n  mcdocker list -l\n  mcdocker config <instance>\n  mcdocker launch <instance>\n\nRun 'mcdocker <command> --help' for options.",
    arg_required_else_help = true
)]
struct Cli {
    /// Data workspace for this command (also MCDOCKER_WORKSPACE).
    #[arg(long, visible_alias = "root", global = true)]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct LaunchOptions {
    /// Override automatic Java discovery with this executable.
    #[arg(long)]
    java: Option<PathBuf>,
    #[arg(long, default_value = "Player")]
    player: String,
    #[arg(long, default_value = "00000000-0000-4000-8000-000000000001")]
    uuid: String,
    #[command(flatten)]
    extra: ExtraArgs,
}

#[derive(Args)]
struct ExtraArgs {
    /// Additional JVM argument (repeatable, e.g. --jvm-arg=-Xmx4G).
    #[arg(long, value_name = "ARG", allow_hyphen_values = true)]
    jvm_arg: Vec<String>,
    /// Additional Minecraft arguments, following --.
    #[arg(last = true, value_name = "GAME_ARG")]
    game_args: Vec<String>,
}

#[derive(Args)]
struct ListOptions {
    /// Show a table with Minecraft, loader, Java and world details.
    #[arg(short = 'l', long, visible_alias = "details")]
    long: bool,
}

#[derive(Subcommand)]
enum ArgumentAction {
    /// Clear both groups of saved arguments.
    Clear { id: Option<String> },
}

#[derive(Subcommand)]
enum Command {
    /// Generate a shell completion script.
    Completions { shell: clap_complete::Shell },
    #[command(hide = true, name = "_complete")]
    Complete,
    /// Interactively import installed clients from a .minecraft directory.
    Import { minecraft: PathBuf },
    /// Show the current workspace or set the persistent workspace.
    Workspace {
        #[command(subcommand)]
        action: Option<Workspace>,
    },
    /// Manage shader packs and resource packs shared by all workspaces.
    Resources {
        #[command(subcommand)]
        action: Option<Resources>,
    },
    /// Select the current instance in this workspace.
    Use { id: String },
    /// List installed instances.
    #[command(visible_alias = "ls")]
    List(ListOptions),
    /// Edit an instance's default JVM and Minecraft arguments in a text editor.
    Config { id: Option<String> },
    /// Reset saved launch arguments.
    Args {
        #[command(subcommand)]
        action: ArgumentAction,
    },
    /// Check the manifests and shared objects referenced by all installed instances.
    Verify {
        #[arg(long)]
        json: bool,
    },
    /// Show object storage, private game data and temporary runtime copies.
    Usage {
        #[arg(long)]
        json: bool,
    },
    /// Show versions, requirements, data directory and saved worlds.
    Info {
        id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show the best installed Java for an instance.
    Java {
        id: Option<String>,
        /// Download a matching JRE if none is installed.
        #[arg(long)]
        install: bool,
    },
    /// Launch an instance, optionally into a named save.
    #[command(visible_alias = "run")]
    Launch {
        id: Option<String>,
        #[arg(long)]
        world: Option<String>,
        #[command(flatten)]
        options: LaunchOptions,
    },
    /// Open a save: quick <world> or quick <instance> <world>.
    Quick {
        id_or_world: String,
        world: Option<String>,
        #[command(flatten)]
        options: LaunchOptions,
    },
    /// Install a .mrpack, CurseForge export ZIP, or local recipe JSON.
    Install {
        pack: PathBuf,
        #[arg(long)]
        id: Option<String>,
        /// Use a prepared Minecraft installation for the base files.
        #[arg(long)]
        source: Option<PathBuf>,
        /// Directory of manually supplied files.
        #[arg(long)]
        manual: Option<PathBuf>,
        /// Include optional client files.
        #[arg(long)]
        optional: bool,
    },
    /// Unregister an instance and retain its data under removed/.
    Uninstall {
        id: Option<String>,
        /// Permanently delete this instance's private files, including saves.
        #[arg(long)]
        delete_data: bool,
    },
    /// Legacy instance commands.
    Instance {
        #[command(subcommand)]
        action: Instance,
    },
}

#[derive(Subcommand)]
enum Workspace {
    Show,
    Use { path: PathBuf },
}
#[derive(Subcommand)]
enum Resources {
    /// Show the current shared directory.
    Show,
    /// Select a directory and create its resourcepacks and shaderpacks folders.
    Use { path: PathBuf },
    /// Disconnect the shared directory and keep its files.
    Clear,
}
#[derive(Subcommand)]
enum Instance {
    List(ListOptions),
    Import {
        id: String,
        manifest: PathBuf,
        #[arg(long)]
        source: PathBuf,
    },
}

fn launch(root: &Path, id: &str, world: Option<&str>, options: LaunchOptions) -> Result<i32> {
    let player = mcdocker::Player {
        name: options.player,
        uuid: options.uuid,
    };
    let extra = mcdocker::LaunchArguments {
        jvm: options.extra.jvm_arg,
        game: options.extra.game_args,
    };
    let status =
        mcdocker::launch_with_args(root, id, options.java.as_deref(), &player, world, &extra)?;
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

fn run() -> Result<i32> {
    let cli = Cli::parse();
    match &cli.command {
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
        Command::Completions { .. } | Command::Complete => unreachable!(),
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
fn main() -> ExitCode {
    match run() {
        Ok(0) => ExitCode::SUCCESS,
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("mcdocker: {err:#}");
            ExitCode::FAILURE
        }
    }
}
