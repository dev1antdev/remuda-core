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
/// At most this many history rows one read encodes: they are encoded under the core's lock, so this keeps
/// one read to about what a paint costs, and it is as far above the screen as a client's find asks
/// (remuda reads 5000 rows for a find, its own rows being 2000).
const MAX_ROWS: usize = 5_000;

/// The live screen with up to `lines` of history above it (main screen only, the newest rows whose
/// JSON fits the budget and at most `MAX_ROWS` of them), with colors. None for a finished terminal:
/// what it keeps has none.
pub fn read(core: &Core, id: u32, lines: usize, join: bool) -> Option<Result<ReadResult, RpcError>> {
    // The screen is taken under the lock and encoded after it: encoding walks every cell, and a second
    // terminal's typing must not wait behind a deep read. The snapshot is the one call that sees the
    // screen as it stands.
    let (bytes, scrolled, exact) = {
        let st = core.lock();
        let t = st.reg.terminals.get(&id)?;
        (core.screen_bytes(&st, id)?, t.scrolled, t.screen == "exact")
    };
    let screen = core.restored(&bytes)?;
    let full_screen = screen.alt_screen();
    // A find reads logical rows: the rows a program soft-wrapped are one, so a word broken at the edge is found.
    let screen_rows = if join { screen.styled_screen_joined() } else { screen.styled_screen() };
    let history: Vec<String> =
        if join { screen.styled_history_joined().collect() } else { screen.styled_history().collect() };
    // Asked for more rows than the bound while there were more than that to give: the rest is unread.
    let capped = lines.min(history.len()) > MAX_ROWS;
    let wanted = lines.min(MAX_ROWS);
    let Some((rows, kept, budget_cut)) = fit(screen_rows, history.into_iter().take(wanted), BUDGET) else {
        let hint = "read it as plain text (no format)";
        return Some(Err(RpcError::new("unavailable", "its screen is too large to read with colors", hint)));
    };
    // Rows left unread: the budget's cut, or the row bound standing in for the rest of what was asked.
    let cut = budget_cut || capped;
    // A full-screen program is not scrolled for colors; history it printed and lost is gone.
    let shown = screen.size().1 as usize;
    let lost = cut || lines > kept && (full_screen || scrolled > (kept + shown) as u64);
    let how = if lines == 0 || full_screen { "screen" } else { "history" };
    let complete = exact && !lost;
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
    let (mut rows, mut cut) = (Vec::new(), false);
    for row in newest_first {
        used += json(&row);
        if used > budget {
            cut = true;
            break;
        }
        rows.push(row);
    }
    let kept = rows.len();
    rows.reverse();
    rows.extend(screen);
    Some((rows, kept, cut))
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
