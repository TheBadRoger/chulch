use anyhow::{Context, Result, ensure};
use chulch::installed::{self, Options};
use std::{
    cell::Cell,
    collections::BTreeSet,
    io::{self, Write},
    path::Path,
};

fn ask(prompt: &str) -> Result<String> {
    print!("{prompt} ");
    io::stdout().flush()?;
    let mut line = String::new();
    ensure!(
        io::stdin().read_line(&mut line)? > 0,
        "Input ended; import cancelled"
    );
    let value = line.trim();
    ensure!(!value.eq_ignore_ascii_case(":q"), "Import cancelled");
    Ok(value.into())
}

fn select(prompt: &str, count: usize, allow_empty: bool) -> Result<Vec<usize>> {
    loop {
        let line = ask(prompt)?;
        if allow_empty && line.is_empty() {
            return Ok(vec![]);
        }
        // Keep accepting the original localized input alias in existing scripts.
        if line.eq_ignore_ascii_case("all") || line.chars().eq(['\u{5168}', '\u{90e8}']) {
            return Ok((0..count).collect());
        }
        let values = line
            .split([',', '，', ' '])
            .filter(|s| !s.is_empty())
            .map(|s| {
                let n = s.parse::<usize>()?;
                ensure!((1..=count).contains(&n), "Selection is out of range");
                Ok(n - 1)
            })
            .collect::<Result<BTreeSet<_>>>();
        if let Ok(items) = values
            && !items.is_empty()
        {
            return Ok(items.into_iter().collect());
        }
        eprintln!("Enter a number from 1 to {count}, or separate multiple numbers with commas.");
    }
}

pub fn run(root: &Path, source: &Path) -> Result<()> {
    let (clients, warnings) = installed::discover(source)?;
    for w in warnings {
        eprintln!("Skipping {w}");
    }
    ensure!(!clients.is_empty(), "No supported clients found");
    println!("Available clients (enter :q to cancel):");
    for (i, c) in clients.iter().enumerate() {
        println!(
            "{}. {} | Minecraft {} | {} {}",
            i + 1,
            c.name,
            c.minecraft,
            c.loader.name(),
            c.version
        );
        for path in &c.data_dirs {
            println!("   {}", path.display());
        }
    }
    let selected = if clients.len() == 1 {
        vec![0]
    } else {
        select(
            "Select versions by number, or enter all:",
            clients.len(),
            false,
        )?
    };
    for i in selected {
        let c = &clients[i];
        println!("Importing {}", c.name);
        let dir = if c.data_dirs.len() == 1 {
            0
        } else {
            for (n, d) in c.data_dirs.iter().enumerate() {
                println!("{}. {}", n + 1, d.display());
            }
            loop {
                let input =
                    ask("Both directories contain game data. Select one directory by number:")?;
                if let Ok(n) = input.parse::<usize>()
                    && (1..=c.data_dirs.len()).contains(&n)
                {
                    break n - 1;
                }
                println!("Enter a valid directory number.");
            }
        };
        let mut id = c.name.to_ascii_lowercase();
        while let Err(e) = installed::valid_new_id(root, &id) {
            eprintln!("{id}: {e}");
            id = ask("New instance ID (lowercase letters, digits, - or _):")?;
        }
        let fields = select(
            "Optional content: 1 saves, 2 configuration and options.txt, 3 resource packs, 4 shader packs. Enter numbers, all, or press Enter to skip:",
            4,
            true,
        )?;
        let options = Options {
            saves: fields.contains(&0),
            config: fields.contains(&1),
            resourcepacks: fields.contains(&2),
            shaderpacks: fields.contains(&3),
        };
        let accept_all = Cell::new(false);
        println!("Importing {id}...");
        installed::import(root, c, &id, dir, &options, |path| {
            println!("Missing file with a verifiable source: {path}");
            if accept_all.get() {
                return Ok(true);
            }
            loop {
                match ask(
                    "Download via mirror? [y] this file, [a] all for this instance, [N] cancel:",
                )?
                .to_ascii_lowercase()
                .as_str()
                {
                    "y" => return Ok(true),
                    "a" => {
                        accept_all.set(true);
                        return Ok(true);
                    }
                    "" | "n" => return Ok(false),
                    _ => continue,
                }
            }
        })
        .with_context(|| format!("import {id}"))?;
        println!("Installed {id}");
    }
    Ok(())
}
