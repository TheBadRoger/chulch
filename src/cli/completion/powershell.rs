use super::candidates;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io;

// PowerShell owns the syntax tree and the completion menu. Never evaluate the input line.
pub(super) const REGISTER: &str = r#"Register-ArgumentCompleter -Native -CommandName chulch,chulch.exe -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)
    $tokens = @(foreach ($e in $commandAst.CommandElements) {
        $value = if ($e -is [System.Management.Automation.Language.StringConstantExpressionAst]) { $e.Value } else { $e.Extent.Text }
        @{ value = $value; start = $e.Extent.StartOffset; end = $e.Extent.EndOffset }
    })
    $OutputEncoding = [System.Text.UTF8Encoding]::new($false)
    $request = @{ tokens = $tokens; cursor = $cursorPosition; prefix = [string]$wordToComplete }
    $rows = ConvertTo-Json -Compress -Depth 4 $request | chulch _complete | ConvertFrom-Json
    foreach ($row in $rows) {
        [System.Management.Automation.CompletionResult]::new($row.insert, $row.value, 'ParameterValue', $row.help)
    }
}
"#;

#[derive(Deserialize)]
struct Token {
    value: String,
    start: usize,
    end: usize,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Request {
    Cursor {
        tokens: Vec<Token>,
        cursor: usize,
        prefix: String,
    },
    // Sessions opened before upgrading still send a list of words.
    Words(Vec<String>),
}

impl Request {
    fn words(self) -> Vec<String> {
        let (tokens, cursor, prefix) = match self {
            Self::Cursor {
                tokens,
                cursor,
                prefix,
            } => (tokens, cursor, prefix),
            Self::Words(words) => return words,
        };
        let mut words = vec![String::from("chulch")];
        for token in tokens.into_iter().skip(1) {
            if token.start >= cursor {
                break;
            }
            if token.end > cursor {
                // PowerShell supplies the whole word, with offsets measured in UTF-16.
                let units: Vec<_> = prefix.encode_utf16().take(cursor - token.start).collect();
                let partial = String::from_utf16_lossy(&units);
                words.push(match partial.strip_prefix('\'') {
                    Some(quoted) => quoted.replace("''", "'"),
                    None => partial.trim_start_matches('"').to_owned(),
                });
                return words;
            }
            words.push(token.value);
            if token.end == cursor {
                return words;
            }
        }
        words.push(String::new());
        words
    }
}

#[derive(Serialize)]
struct Suggestion {
    value: String,
    insert: String,
    help: String,
}

fn quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || "_./:\\=+-".contains(c))
    {
        value.to_owned()
    } else {
        let mut quoted = String::from("'");
        for ch in value.chars() {
            if matches!(ch, '\'' | '‘' | '’' | '‚' | '‛') {
                quoted.push(ch);
            }
            quoted.push(ch);
        }
        quoted.push('\'');
        quoted
    }
}

pub fn complete() -> Result<()> {
    let request: Request = serde_json::from_reader(io::stdin().lock())?;
    let rows: Vec<_> = candidates(request.words())?
        .into_iter()
        .map(|candidate| {
            let value = candidate.get_value().to_string_lossy().into_owned();
            let help = candidate
                .get_help()
                .map(ToString::to_string)
                .unwrap_or_else(|| value.clone());
            Suggestion {
                insert: quote(&value),
                value,
                help,
            }
        })
        .collect();
    serde_json::to_writer(io::stdout().lock(), &rows)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_inside_a_quoted_word_uses_utf16_offsets() {
        let request: Request = serde_json::from_value(serde_json::json!({
            "tokens": [
                {"value":"chulch", "start":0, "end":6},
                {"value":"import", "start":7, "end":13},
                {"value":"🌲存档 old", "start":14, "end":24}
            ],
            "cursor":18, "prefix":"'🌲存档 old'"
        }))
        .unwrap();
        assert_eq!(request.words(), ["chulch", "import", "🌲存"]);
    }

    #[test]
    fn powershell_inserts_paths_with_spaces_and_quotes_as_one_literal() {
        assert_eq!(quote("a'b’s $(name).zip"), "'a''b’’s $(name).zip'");
        assert_eq!(quote("--jvm-arg"), "--jvm-arg");
    }
}
