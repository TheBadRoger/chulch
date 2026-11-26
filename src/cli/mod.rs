mod completion;
mod dispatch;
mod import_console;
pub(crate) use dispatch::run;

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    bin_name = "chulch",
    version,
    about = "Manage Minecraft instances from the terminal",
    after_help = "Examples:\n  chulch list -l\n  chulch config <instance>\n  chulch launch <instance>\n\nRun 'chulch <command> --help' for options.",
    arg_required_else_help = true
)]
struct Cli {
    /// Data workspace for this command (also CHULCH_WORKSPACE).
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
    /// Choose the offline identity or the signed-in Microsoft account.
    #[arg(long, value_enum, default_value = "offline")]
    account: AccountMode,
    /// Offline player name (default: Player).
    #[arg(long)]
    player: Option<String>,
    /// Offline player UUID.
    #[arg(long)]
    uuid: Option<String>,
    #[command(flatten)]
    extra: ExtraArgs,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum AccountMode {
    Offline,
    Microsoft,
}

#[derive(Subcommand)]
enum AccountAction {
    /// Sign in on Microsoft's website and save the verified Java account.
    Login {
        /// Public application (client) ID; also reads CHULCH_MICROSOFT_CLIENT_ID.
        #[arg(long)]
        client_id: Option<String>,
    },
    /// Show the saved Minecraft profile.
    Status,
    /// Remove this account's local sign-in data.
    Logout,
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
    /// Manage the Microsoft account used for authenticated launches.
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },
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
