//! remuda's own addition (REMUDA.md): `terminal.read` with its colors.

use crate::api::ReadResult;
use crate::rpc::RpcError;
use crate::runtime::Core;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How `terminal.read` gives the text: plain, or vt (rows that each start with a
/// style reset and hold their text, colors and attributes as SGR, nothing else).
#[derive(Clone, Copy, PartialEq, Debug, Default, clap::ValueEnum, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ReadFormat {
    #[default]
    Plain,
    Vt,
}

/// At most this much JSON, half a frame: a reply past a frame ends the line it travels on.
const BUDGET: usize = crate::wire::MAX_PAYLOAD / 2;
/// At most this many history rows: they are encoded under the core's lock.
const HISTORY: usize = crate::api::FINISHED_LINES;

/// The live screen with up to `lines` of history above it (main screen only, at most
/// `HISTORY` rows, the newest that fit the budget), with colors. None for a finished terminal: what
/// it keeps has none.
pub fn read(core: &Core, id: u32, lines: usize) -> Option<Result<ReadResult, RpcError>> {
    let st = core.lock();
    let (screen, t) = (st.screens.get(&id)?, st.reg.terminals.get(&id)?);
    let full_screen = screen.alt_screen();
    let history = screen.styled_history();
    let capped = lines.min(history.len()) > HISTORY;
    let Some((rows, kept, cut)) = fit(screen.styled_screen(), history.take(lines.min(HISTORY)), BUDGET) else {
        let hint = "read it as plain text (no format)";
        return Some(Err(RpcError::new("unavailable", "its screen is too large to read with colors", hint)));
    };
    // A full-screen program is not scrolled for colors; history it printed and lost is gone.
    let shown = screen.size().1 as usize;
    let lost = cut || capped || lines > kept && (full_screen || t.printed > (kept + shown) as u64);
    let how = if lines == 0 || full_screen { "screen" } else { "history" };
    let complete = t.screen == "exact" && !lost;
    Some(Ok(ReadResult { text: rows.join("\n"), complete, how: how.into(), why: None }))
}

/// The screen and the newest history rows whose JSON fits `budget`, oldest first:
/// the rows, how many history rows, whether some were cut. None: the screen alone is too big.
fn fit(
    screen: Vec<String>,
    newest_first: impl Iterator<Item = String>,
    budget: usize,
) -> Option<(Vec<String>, usize, bool)> {
    let json = |row: &str| serde_json::to_string(row).map_or(row.len(), |s| s.len()) + 1;
    let mut used: usize = screen.iter().map(|r| json(r)).sum();
    if used > budget {
        return None;
    }
    let mut rows = Vec::new();
    for row in newest_first {
        used += json(&row);
        if used > budget {
            let kept = rows.len();
            rows.reverse();
            rows.extend(screen);
            return Some((rows, kept, true));
        }
        rows.push(row);
    }
    let kept = rows.len();
    rows.reverse();
    rows.extend(screen);
    Some((rows, kept, false))
}

/// `rc read`'s output: the terminal's `text` and the notes after it. For vt the
/// notes go to stderr, so stdout holds the rows alone.
pub fn print_read(output: &str, text: &str, format: ReadFormat) {
    match output.split_at_checked(text.len()) {
        Some((rows, notes)) if format == ReadFormat::Vt && !notes.is_empty() => {
            println!("{rows}");
            eprintln!("{}", notes.trim_start_matches('\n'));
        }
        _ => println!("{output}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_fills_what_the_screen_leaves_of_the_budget_newest_first() {
        let rows = |r: &[&str]| r.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let screen = rows(&["\x1b[0ms"]);
        // Each row is 13 bytes of JSON with its line break: the quotes, "\u001b[0m" and a letter.
        let fits = fit(screen.clone(), rows(&["\x1b[0mc", "\x1b[0mb", "\x1b[0ma"]).into_iter(), 39);
        assert_eq!(fits, Some((rows(&["\x1b[0mb", "\x1b[0mc", "\x1b[0ms"]), 2, true)));
        let all = fit(screen.clone(), rows(&["\x1b[0mc"]).into_iter(), 39);
        assert_eq!(all, Some((rows(&["\x1b[0mc", "\x1b[0ms"]), 1, false)));
        assert_eq!(fit(screen, std::iter::empty(), 12), None, "the screen alone does not fit");
    }
}
