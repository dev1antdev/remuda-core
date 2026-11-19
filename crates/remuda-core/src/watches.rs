//! remuda's own addition (REMUDA.md): `events.subscribe`'s output watches. A
//! subscription's watches look at their terminals' last rows four times a second,
//! when they printed, and tell of each matching row that came into view.

use crate::events::{Filter, Sink};
use crate::rpc::RpcError;
use crate::runtime::Core;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::mpsc;

pub const MAX_WATCHES: usize = 256;
pub const MAX_LINES: usize = 200;
pub const MAX_PATTERN: usize = 4096;
const EVERY: Duration = Duration::from_millis(250);

/// A watch on a terminal's last rows.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct OutputWatch {
    /// Named in each `output` event.
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
    /// The matching rows at the last look, each with how many times it stood there.
    seen: HashMap<String, usize>,
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
    Ok(Watch { id: id.clone(), terminal: resolve(&w.terminal)?, lines: w.lines, find, seen: HashMap::new() })
}

/// Looks for the watches' matches while their subscription (`sink`, `filter`) lasts.
pub fn start(core: Arc<Core>, sink: Sink, filter: Filter, mut watches: Vec<Watch>) {
    if watches.is_empty() || matches!(sink, Sink::Waiter(_)) {
        return;
    }
    tokio::spawn(async move {
        let mut printed: HashMap<u32, u64> = HashMap::new();
        let mut fresh = true;
        while look(&core, &sink, &filter, &mut watches, &mut printed, fresh) {
            fresh = false;
            tokio::time::sleep(EVERY).await;
        }
    });
}

/// One look at the terminals that printed since the last; false once the subscription is gone.
fn look(
    core: &Core,
    sink: &Sink,
    filter: &Filter,
    watches: &mut [Watch],
    printed: &mut HashMap<u32, u64>,
    fresh: bool,
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
            fresh || printed.insert(*id, now) != Some(now)
        })
        .collect();
    let mut rows: HashMap<(u32, usize), Vec<String>> = HashMap::new();
    let mut events = Vec::new();
    for w in watches.iter_mut().filter(|w| moved.contains(&w.terminal)) {
        let Some(screen) = st.screens.get(&w.terminal) else { continue };
        let lines = rows.entry((w.terminal, w.lines)).or_insert_with(|| screen.recent_rows(w.lines));
        let mut now: HashMap<String, usize> = HashMap::new();
        for line in lines.iter().filter(|l| w.find.matches(l)) {
            *now.entry(line.clone()).or_default() += 1;
        }
        for (line, &count) in &now {
            for _ in w.seen.get(line).copied().unwrap_or_default()..count {
                events.push(json!({"type": "output", "terminal": crate::terminals::address(w.terminal),
                    "data": {"watch": w.id, "line": line, "fresh": fresh}}));
            }
        }
        w.seen = now;
    }
    if fresh {
        for w in watches.iter() {
            printed.insert(w.terminal, st.output_seq.get(&w.terminal).copied().unwrap_or_default());
        }
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
}
