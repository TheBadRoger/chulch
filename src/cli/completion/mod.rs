mod powershell;

use super::Cli;
use anyhow::{Result, ensure};
use clap::CommandFactory;
use clap_complete::{
    Shell,
    engine::{ArgValueCandidates, CompletionCandidate},
};
use std::{env, io, path::PathBuf};

pub fn script(shell: Shell) -> Result<()> {
    if shell == Shell::PowerShell {
        use io::Write;
        io::stdout().write_all(powershell::REGISTER.as_bytes())?;
    } else {
        clap_complete::generate(shell, &mut Cli::command(), "chulch", &mut io::stdout());
    }
    Ok(())
}

fn workspace_option(words: &[String]) -> Option<PathBuf> {
    let mut words = words.iter().map(String::as_str);
    let mut path = None;
    while let Some(word) = words.next() {
        if matches!(word, "--workspace" | "--root") {
            path = words.next().map(PathBuf::from);
        } else if let Some(value) = word
            .strip_prefix("--workspace=")
            .or_else(|| word.strip_prefix("--root="))
        {
            path = Some(value.into());
        }
    }
    path
}

pub use powershell::complete as powershell;

fn candidates(words: Vec<String>) -> Result<Vec<CompletionCandidate>> {
    ensure!(words.len() >= 2, "Expected command and completion prefix");
    let previous = &words[1..words.len() - 1];
    if previous.iter().any(|s| s == "--") {
        return Ok(Vec::new());
    }
    let workspace = workspace_option(previous);
    let instances = ArgValueCandidates::new(move || {
        chulch::workspace::resolve(workspace.clone())
            .and_then(|root| chulch::list(&root))
            .unwrap_or_default()
            .into_iter()
            .map(|id| CompletionCandidate::new(id).help(Some("Installed instance".into())))
            .collect()
    });
    let mut cli = Cli::command();
    for name in [
        "use",
        "config",
        "info",
        "java",
        "launch",
        "uninstall",
        "quick",
    ] {
        let arg = if name == "quick" { "id_or_world" } else { "id" };
        cli = cli.mut_subcommand(name, |cmd| {
            cmd.mut_args(|a| {
                if a.get_id() == arg {
                    a.add(instances.clone())
                } else {
                    a
                }
            })
        });
    }
    cli = cli.mut_subcommand("args", |cmd| {
        cmd.mut_subcommand("clear", |clear| clear.mut_arg("id", |a| a.add(instances)))
    });
    let index = words.len() - 1;
    Ok(clap_complete::engine::complete(
        &mut cli,
        words.into_iter().map(Into::into).collect(),
        index,
        Some(&env::current_dir()?),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_flags_with_help_and_stops_at_game_arguments() -> Result<()> {
        let words = |items: &[&str]| items.iter().map(|s| s.to_string()).collect();
        let flags = candidates(words(&["chulch", "run", "--jvm"]))?;
        assert!(
            flags
                .iter()
                .any(|f| f.get_value() == "--jvm-arg" && f.get_help().is_some())
        );
        assert!(candidates(words(&["chulch", "run", "--", "--jvm"]))?.is_empty());
        Ok(())
    }
}
