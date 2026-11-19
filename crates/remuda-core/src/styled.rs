//! remuda's own addition (REMUDA.md): `terminal.read` with its colors.

use crate::api::ReadResult;
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

/// The live screen with up to `lines` of history above it (main screen only), with
/// colors. None for a finished terminal: what it keeps has none.
pub fn read(core: &Core, id: u32, lines: usize) -> Option<ReadResult> {
    let st = core.lock();
    let (screen, t) = (st.screens.get(&id)?, st.reg.terminals.get(&id)?);
    let full_screen = screen.alt_screen();
    let rows = screen.styled_rows(lines);
    let shown = screen.size().1 as usize;
    let kept = rows.len().saturating_sub(shown);
    // A full-screen program is not scrolled for colors; history it printed and lost is gone.
    let lost = lines > kept && (full_screen || t.printed > (kept + shown) as u64);
    let how = if lines == 0 || full_screen { "screen" } else { "history" };
    Some(ReadResult { text: rows.join("\n"), complete: t.screen == "exact" && !lost, how: how.into(), why: None })
}
