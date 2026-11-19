//! remuda's own addition (REMUDA.md): `events.subscribe`'s output watches. A
//! subscription's watches look at their terminals' last rows four times a second,
//! when they printed, and tell of each matching row that came into view.

use crate::events::{Filter, Sink};
use crate::rpc::RpcError;
use crate::runtime::Core;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::mpsc;

pub const MAX_WATCHES: usize = 256;
pub const MAX_LINES: usize = 200;
pub const MAX_PATTERN: usize = 4096;
pub const MAX_ID: usize = 256;
const EVERY: Duration = Duration::from_millis(250);
/// What one look sends at most, well inside a subscriber's queue; the rest waits for the next.
const EVENTS_PER_LOOK: usize = 256;
const BYTES_PER_LOOK: usize = 1 << 20;

/// A watch on a terminal's last rows.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct OutputWatch {
    /// Named in each `output` event (at most 256 bytes).
    #[schemars(length(max = 256))]
    pub id: String,
    /// A terminal here (`t7` or a name); for another machine's, subscribe to its core.
    pub terminal: String,
    #[serde(rename = "match")]
    pub find: OutputMatch,
    /// How many of its last rows it looks at, 1 to 200 (default 40).
    #[serde(default = "default_lines")]
    #[schemars(range(min = 1, max = 200))]
    pub lines: usize,
}

fn default_lines() -> usize {
    40
}

/// What a row must hold: a piece of text, or a match of a regular expression
/// (Rust's `regex` syntax, `\d` and `\w` Unicode's).
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum OutputMatch {
    Substring(#[schemars(length(max = 4096))] String),
    Regex(#[schemars(length(max = 4096))] String),
}

pub struct Watch {
    id: String,
    terminal: u32,
    lines: usize,
    find: Find,
    /// The matching rows told and still standing, each with how many times it stands.
    seen: HashMap<String, usize>,
    /// Its rows told now are the ones already there when it began.
    fresh: bool,
    /// Matches it could not tell in its last look: look again whether or not it printed.
    behind: bool,
    /// Its terminal finished and its last screen was looked at.
    done: bool,
}

enum Find {
    Text(String),
    Pattern(regex::Regex),
}

impl Find {
    fn matches(&self, row: &str) -> bool {
        match self {
            Find::Text(text) => row.contains(text.as_str()),
            Find::Pattern(re) => re.is_match(row),
        }
    }
}

/// Checks and compiles the watches a subscription asks for.
/// `far`: whether a name is a machine this core added (its terminals are watched by its own core).
pub fn compile(
    asked: &[OutputWatch],
    resolve: impl Fn(&str) -> Result<u32, RpcError>,
    far: impl Fn(&str) -> bool,
) -> Result<Vec<Watch>, RpcError> {
    if asked.len() > MAX_WATCHES {
        return Err(RpcError::invalid_params(format!("{} output watches: at most {MAX_WATCHES}", asked.len())));
    }
    asked.iter().map(|w| compile_one(w, &resolve, &far)).collect()
}

fn compile_one(
    w: &OutputWatch,
    resolve: impl Fn(&str) -> Result<u32, RpcError>,
    far: impl Fn(&str) -> bool,
) -> Result<Watch, RpcError> {
    if w.id.len() > MAX_ID {
        return Err(RpcError::invalid_params(format!("a watch's id is over {MAX_ID} bytes")));
    }
    let id = &w.id;
    if !(1..=MAX_LINES).contains(&w.lines) {
        return Err(RpcError::invalid_params(format!("watch {id:?}: lines {} is not 1 to {MAX_LINES}", w.lines)));
    }
    let (OutputMatch::Substring(pattern) | OutputMatch::Regex(pattern)) = &w.find;
    if pattern.len() > MAX_PATTERN {
        return Err(RpcError::invalid_params(format!("watch {id:?}: its pattern is over {MAX_PATTERN} bytes")));
    }
    if w.terminal.split_once('/').is_some_and(|(machine, _)| far(machine)) {
        return Err(RpcError::invalid_params(format!("watch {id:?}: {} is on another machine", w.terminal))
            .with_hint("subscribe to that machine's core for its terminals' output"));
    }
    let find = match &w.find {
        OutputMatch::Substring(text) => Find::Text(text.clone()),
        OutputMatch::Regex(re) => Find::Pattern(
            regex::Regex::new(re).map_err(|e| RpcError::new("invalid_regex", format!("watch {id:?}: {e}"), ""))?,
        ),
    };
    let terminal = resolve(&w.terminal)?;
    Ok(Watch {
        id: id.clone(),
        terminal,
        lines: w.lines,
        find,
        seen: HashMap::new(),
        fresh: true,
        behind: false,
        done: false,
    })
}

/// Looks for the watches' matches while their subscription (`sink`, `filter`) lasts.
pub fn start(core: Arc<Core>, sink: Sink, filter: Filter, mut watches: Vec<Watch>) {
    if watches.is_empty() || matches!(sink, Sink::Waiter(_)) {
        return;
    }
    tokio::spawn(async move {
        let mut printed: HashMap<u32, u64> = HashMap::new();
        let mut first = true;
        while look(&core, &sink, &filter, &mut watches, &mut printed, first) {
            first = false;
            tokio::time::sleep(EVERY).await;
        }
    });
}

/// What is left of one look's room for events.
struct Budget {
    events: usize,
    bytes: usize,
}

impl Budget {
    fn take(&mut self, bytes: usize) -> bool {
        let fits = self.events > 0 && bytes <= self.bytes;
        if fits {
            self.events -= 1;
            self.bytes -= bytes;
        }
        fits
    }
}

/// One look at the terminals that printed since the last; false once the subscription is gone.
fn look(
    core: &Core,
    sink: &Sink,
    filter: &Filter,
    watches: &mut [Watch],
    printed: &mut HashMap<u32, u64>,
    first: bool,
) -> bool {
    let mut st = core.lock();
    let alive = match sink {
        Sink::Framed { conn, channel } => core.subscribed(&st, conn, *channel),
        Sink::Plain { tx, .. } => !tx.is_closed(),
        Sink::Waiter(_) => false,
    };
    if !alive {
        return false;
    }
    let moved: Vec<u32> = watches
        .iter()
        .map(|w| w.terminal)
        .filter(|id| {
            let now = st.output_seq.get(id).copied().unwrap_or_default();
            printed.insert(*id, now) != Some(now)
        })
        .collect();
    let mut rows: HashMap<(u32, usize), Vec<String>> = HashMap::new();
    let mut budget = Budget { events: EVENTS_PER_LOOK, bytes: BYTES_PER_LOOK };
    let mut events = Vec::new();
    for w in watches.iter_mut().filter(|w| !w.done && (first || w.behind || moved.contains(&w.terminal))) {
        let lines = match rows.get(&(w.terminal, w.lines)) {
            Some(lines) => lines,
            None => {
                let Some(lines) = recent(core, &st, w) else { continue };
                rows.entry((w.terminal, w.lines)).or_insert(lines)
            }
        };
        tell(w, lines, &mut budget, &mut events);
        w.done = !w.behind && !st.screens.contains_key(&w.terminal);
    }
    for mut params in events {
        st.events.seq += 1;
        params["seq"] = st.events.seq.into();
        let line = format!("{}\n", json!({"jsonrpc": "2.0", "method": "event", "params": params}));
        match sink {
            Sink::Framed { conn, channel } => {
                let missed = || {
                    format!("{}\n", json!({"jsonrpc": "2.0", "method": "missed", "params": core.snapshot(&st, filter)}))
                };
                conn.with(|out| {
                    if !out.data_lines(*channel, line.as_bytes()) {
                        out.data(*channel, missed().as_bytes());
                    }
                });
            }
            Sink::Plain { tx, behind } => {
                if let Err(mpsc::error::TrySendError::Full(_)) = tx.try_send(line) {
                    behind.store(true, Ordering::SeqCst);
                }
            }
            Sink::Waiter(_) => {}
        }
    }
    true
}

/// A watch's last rows: the live screen's, or once it finished, those of the screen it left.
fn recent(core: &Core, st: &crate::runtime::State, w: &Watch) -> Option<Vec<String>> {
    if let Some(screen) = st.screens.get(&w.terminal) {
        return Some(screen.recent_rows(w.lines));
    }
    st.reg.terminals.get(&w.terminal)?;
    let saved = std::fs::read(core.snapshot_path(w.terminal)).ok()?;
    Some(ghostty_vt::Terminal::restore(&saved, core.history_bytes())?.recent_rows(w.lines))
}

/// Events for the matching lines not told yet, in their order, as far as `budget` goes.
fn tell(w: &mut Watch, lines: &[String], budget: &mut Budget, events: &mut Vec<Value>) {
    let mut standing: HashMap<String, usize> = HashMap::new();
    w.behind = false;
    for line in lines.iter().filter(|l| w.find.matches(l)) {
        let told = standing.entry(line.clone()).or_default();
        if *told < w.seen.get(line).copied().unwrap_or_default() {
            *told += 1;
        } else if !w.behind && budget.take(w.id.len() + line.len() + 128) {
            *told += 1;
            events.push(json!({"type": "output", "terminal": crate::terminals::address(w.terminal),
                "data": {"watch": w.id, "line": line, "fresh": w.fresh}}));
        } else {
            w.behind = true;
        }
    }
    w.seen = standing;
    w.fresh &= w.behind;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watch(find: OutputMatch, lines: usize) -> OutputWatch {
        OutputWatch { id: "w".into(), terminal: "t1".into(), find, lines }
    }

    #[test]
    fn a_regex_reads_digits_and_words_as_unicode_does() {
        let w = compile(&[watch(OutputMatch::Regex(r"^\d+ failed$".into()), 40)], |_| Ok(1), |_| false).unwrap();
        assert!(w[0].find.matches("٣ failed") && !w[0].find.matches("x failed"));
    }

    #[test]
    fn what_a_subscription_cannot_watch_is_refused_by_name() {
        let refused = |asked: &[OutputWatch]| compile(asked, |_| Ok(1), |m| m == "rohan").err().map(|e| e.kind);
        assert_eq!(refused(&[watch(OutputMatch::Regex("(".into()), 40)]).as_deref(), Some("invalid_regex"));
        assert_eq!(refused(&[watch(OutputMatch::Substring("x".into()), 0)]).as_deref(), Some("invalid_params"));
        assert_eq!(refused(&[watch(OutputMatch::Substring("x".into()), 201)]).as_deref(), Some("invalid_params"));
        assert_eq!(refused(&[watch(OutputMatch::Substring("x".repeat(4097)), 40)]).as_deref(), Some("invalid_params"));
        assert_eq!(
            refused(&vec![watch(OutputMatch::Substring("x".into()), 40); 257]).as_deref(),
            Some("invalid_params")
        );
        let far = OutputWatch { terminal: "rohan/t1".into(), ..watch(OutputMatch::Substring("x".into()), 40) };
        assert_eq!(refused(&[far]).as_deref(), Some("invalid_params"));
        let here = OutputWatch { terminal: "box/t1".into(), ..watch(OutputMatch::Substring("x".into()), 40) };
        assert_eq!(refused(&[here]), None, "this core's own name is for resolve to read");
    }

    #[test]
    fn a_look_tells_what_its_budget_holds_and_the_next_one_the_rest() {
        let mut w =
            compile(&[watch(OutputMatch::Substring("FAIL".into()), 40)], |_| Ok(1), |_| false).unwrap().remove(0);
        let lines: Vec<String> = ["a FAIL", "ok", "b FAIL", "a FAIL"].map(String::from).into();
        let told = |events: &[Value]| {
            events.iter().map(|e| (e["data"]["line"].clone(), e["data"]["fresh"].clone())).collect::<Vec<_>>()
        };
        let mut events = Vec::new();
        tell(&mut w, &lines, &mut Budget { events: 2, bytes: 1 << 20 }, &mut events);
        assert_eq!(told(&events), [(json!("a FAIL"), json!(true)), (json!("b FAIL"), json!(true))], "in their order");
        assert!(w.behind);
        events.clear();
        tell(&mut w, &lines, &mut Budget { events: 9, bytes: 1 << 20 }, &mut events);
        assert_eq!(told(&events), [(json!("a FAIL"), json!(true))], "the second a FAIL, still one of the first rows");
        events.clear();
        tell(&mut w, &lines, &mut Budget { events: 9, bytes: 1 << 20 }, &mut events);
        assert!(events.is_empty() && !w.behind && !w.fresh, "rows that stand are not told again");
    }
}
