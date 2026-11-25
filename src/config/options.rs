use crate::LaunchArguments;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Settings {
    pub(super) jvm: Jvm,
    pub(super) game: Game,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Jvm {
    min_memory: Option<String>,
    max_memory: Option<String>,
    pub(super) extra: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Game {
    width: Option<u32>,
    height: Option<u32>,
    fullscreen: Option<bool>,
    pub(super) extra: Vec<String>,
}

fn memory_bytes(key: &str, value: &str) -> Result<u64> {
    let (digits, multiplier) = match value.as_bytes().last().map(u8::to_ascii_lowercase) {
        Some(b'k') => (&value[..value.len() - 1], 1 << 10),
        Some(b'm') => (&value[..value.len() - 1], 1 << 20),
        Some(b'g') => (&value[..value.len() - 1], 1 << 30),
        _ => (value, 1),
    };
    let bytes = digits
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(multiplier));
    bytes
        .filter(|n| *n > 0 && digits.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "jvm.{key} must be a positive heap size, for example \"512M\" or \"4G\""
            )
        })
}

impl Settings {
    pub(super) fn from_extra(args: LaunchArguments) -> Self {
        Self {
            jvm: Jvm {
                extra: args.jvm,
                ..Jvm::default()
            },
            game: Game {
                extra: args.game,
                ..Game::default()
            },
        }
    }

    pub(super) fn validate(&self) -> Result<()> {
        let min = self
            .jvm
            .min_memory
            .as_deref()
            .map(|s| memory_bytes("min_memory", s))
            .transpose()?;
        let max = self
            .jvm
            .max_memory
            .as_deref()
            .map(|s| memory_bytes("max_memory", s))
            .transpose()?;
        ensure!(
            !matches!((min, max), (Some(a), Some(b)) if a > b),
            "jvm.min_memory exceeds jvm.max_memory"
        );
        for (key, value) in [("width", self.game.width), ("height", self.game.height)] {
            ensure!(
                value.is_none_or(|n| n > 0 && n <= i32::MAX as u32),
                "game.{key} must be a positive 32-bit integer"
            );
        }
        ensure!(
            !self
                .jvm
                .extra
                .iter()
                .chain(&self.game.extra)
                .any(|s| s.contains('\0')),
            "Arguments cannot contain NUL"
        );
        Ok(())
    }

    pub(super) fn template(&self) -> Result<String> {
        Ok(format!(
            "[jvm]\n# min_memory = \"1G\"\n# max_memory = \"4G\"\n{}\n[game]\n# width = 1280\n# height = 720\n# fullscreen = true\n{}",
            toml::to_string_pretty(&self.jvm)?,
            toml::to_string_pretty(&self.game)?
        ))
    }

    pub(crate) fn resolve(
        &self,
        base: LaunchArguments,
        extra: &LaunchArguments,
    ) -> LaunchArguments {
        let mut result = LaunchArguments::default();
        if let Some(size) = &self.jvm.min_memory {
            result.jvm.push(format!("-Xms{size}"));
        }
        if let Some(size) = &self.jvm.max_memory {
            result.jvm.push(format!("-Xmx{size}"));
        }
        for arg in base.jvm.iter().chain(&self.jvm.extra).chain(&extra.jvm) {
            let min = ["-Xms", "-XX:InitialHeapSize=", "-ms"]
                .iter()
                .any(|prefix| arg.starts_with(prefix));
            let max = ["-Xmx", "-XX:MaxHeapSize=", "-mx"]
                .iter()
                .any(|prefix| arg.starts_with(prefix));
            if !(min && self.jvm.min_memory.is_some() || max && self.jvm.max_memory.is_some()) {
                result.jvm.push(arg.clone());
            }
        }
        for (key, value) in [("--width", self.game.width), ("--height", self.game.height)] {
            if let Some(n) = value {
                result.game.extend([key.to_owned(), n.to_string()]);
            }
        }
        if self.game.fullscreen == Some(true) {
            result.game.push("--fullscreen".to_owned());
        }
        for source in [&base.game, &self.game.extra, &extra.game] {
            self.game.append_extra(source, &mut result.game);
        }
        result
    }
}

impl Game {
    fn append_extra(&self, args: &[String], output: &mut Vec<String>) {
        let mut args = args.iter().peekable();
        while let Some(arg) = args.next() {
            if arg == "--" {
                output.push(arg.clone());
                output.extend(args.cloned());
                break;
            }
            let (key, inline) = arg
                .split_once('=')
                .map_or((arg.as_str(), false), |(k, _)| (k, true));
            match key {
                "--width" | "-width" if self.width.is_some() => {}
                "--height" | "-height" if self.height.is_some() => {}
                "--fullscreen" | "-fullscreen" if self.fullscreen.is_some() => {
                    if !inline {
                        args.next_if(|s| matches!(s.as_str(), "true" | "false"));
                    }
                    continue;
                }
                _ => {
                    output.push(arg.clone());
                    continue;
                }
            }
            if !inline {
                args.next_if(|s| !s.starts_with('-') || s.parse::<i64>().is_ok());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_options_remove_conflicts_without_consuming_adjacent_flags() -> Result<()> {
        let config: Settings = toml::from_str(
            r#"
            [jvm]
            min_memory = "64M"
            extra = ["-XX:InitialHeapSize=32M", "-Xmx256M"]
            [game]
            height = 720
            fullscreen = true
            extra = ["--height", "--label", "two words", "--fullscreen", "-height=480"]
        "#,
        )?;
        config.validate()?;
        let resolved = config.resolve(LaunchArguments::default(), &LaunchArguments::default());
        assert_eq!(resolved.jvm, ["-Xms64M", "-Xmx256M"]);
        assert_eq!(
            resolved.game,
            ["--height", "720", "--fullscreen", "--label", "two words"]
        );
        Ok(())
    }
}
