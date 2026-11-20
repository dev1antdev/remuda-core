//! remuda's own addition (REMUDA.md): `events.subscribe`'s output watches. A
//! subscription's watches look at their terminals' last rows four times a second,
//! when they printed, and tell of each matching row that came into view.

use crate::events::{Filter, Sink};
use crate::rpc::RpcError;
use crate::runtime::Core;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
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
/// Rows a watch keeps waiting to be told, past which the oldest go (and are told as lost).
const MAX_PENDING: usize = 1000;
const MAX_PENDING_BYTES: usize = 64 << 10;
/// A row told is cut to this, so any one fits a look whatever else waits.
const MAX_LINE: usize = 64 << 10;

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
    /// The matching rows standing at the last look, each with how many times it stands.
    seen: HashMap<String, usize>,
    /// Rows seen and not told yet, each with whether it stood there when the watch began.
    /// Rows seen and not told yet, as told: the text, whether it stood there when
    /// the watch began, whether it was cut.
    pending: VecDeque<(String, bool, bool)>,
    pending_bytes: usize,
    /// Rows dropped from a full `pending`, told with the next row that is sent.
    lost: usize,
    looked: bool,
    /// Its terminal finished (or went) and what it left was looked at.
    done: bool,
}

enum Find {
    Text(String),
    Pattern(regex::Regex),
}

impl Find {
    /// Where in `row` it matches first.
    fn find(&self, row: &str) -> Option<std::ops::Range<usize>> {
        match self {
            Find::Text(text) => row.find(text.as_str()).map(|at| at..at + text.len()),
            Find::Pattern(re) => re.find(row).map(|m| m.range()),
        }
    }
}

/// `line` as told: whole, or past `MAX_LINE` the `MAX_LINE` around what matched
/// (from its start when the match itself is longer), with whether it was cut.
fn excerpt(line: &str, at: std::ops::Range<usize>) -> (String, bool) {
    if line.len() <= MAX_LINE {
        return (line.to_string(), false);
    }
    let around = MAX_LINE.saturating_sub(at.len()) / 2;
    let start = line.floor_char_boundary(at.start.saturating_sub(around).min(line.len() - MAX_LINE));
    // A start moved back to a character's first byte must not cut the match's end off.
    let reach = if at.len() <= MAX_LINE { at.end } else { 0 };
    let end = line.ceil_char_boundary((start + MAX_LINE).max(reach));
    (line[start..end].to_string(), true)
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
        pending: VecDeque::new(),
        pending_bytes: 0,
        lost: 0,
        looked: false,
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
        let mut turn = 0;
        while look(&core, &sink, &filter, &mut watches, &mut printed, turn) {
            turn += 1;
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

/// One look: the terminals that printed since the last are read, then each
/// watch's waiting rows are told in turn as far as one look goes. False once
/// the subscription is gone.
fn look(
    core: &Core,
    sink: &Sink,
    filter: &Filter,
    watches: &mut [Watch],
    printed: &mut HashMap<u32, u64>,
    turn: usize,
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
    let mut rows: HashMap<(u32, usize), Option<Vec<String>>> = HashMap::new();
    for w in watches.iter_mut().filter(|w| !w.done && (!w.looked || moved.contains(&w.terminal))) {
        if let Some(lines) = rows.entry((w.terminal, w.lines)).or_insert_with(|| recent(core, &st, w)) {
            observe(w, lines);
        }
        if !st.screens.contains_key(&w.terminal) {
            w.done = st.reg.terminals.get(&w.terminal).is_none_or(|t| !t.is_live());
        }
    }
    let events = tell(watches, turn, &mut Budget { events: EVENTS_PER_LOOK, bytes: BYTES_PER_LOOK });
    watches.iter_mut().for_each(trim);
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

/// A watch's last rows: the live screen's, or while it has none, those of the screen it left.
fn recent(core: &Core, st: &crate::runtime::State, w: &Watch) -> Option<Vec<String>> {
    if let Some(screen) = st.screens.get(&w.terminal) {
        return Some(screen.recent_rows(w.lines));
    }
    st.reg.terminals.get(&w.terminal)?;
    let saved = std::fs::read(core.snapshot_path(w.terminal)).ok()?;
    Some(ghostty_vt::Terminal::restore(&saved, core.history_bytes())?.recent_rows(w.lines))
}

/// Queues the matching lines that came into view since the last look, in their order.
fn observe(w: &mut Watch, lines: &[String]) {
    let fresh = !w.looked;
    let mut standing: HashMap<String, usize> = HashMap::new();
    for (line, at) in lines.iter().filter_map(|l| Some((l, w.find.find(l)?))) {
        let count = standing.entry(line.clone()).or_default();
        *count += 1;
        if *count > w.seen.get(line).copied().unwrap_or_default() {
            let (told, cut) = excerpt(line, at);
            w.pending_bytes += told.len();
            w.pending.push_back((told, fresh, cut));
        }
    }
    w.seen = standing;
    w.looked = true;
}

/// What still waits after a look's telling, cut to `MAX_PENDING` rows and
/// `MAX_PENDING_BYTES`, the oldest going first and counted as lost. The newest
/// always stays, so a loss always has a row to be told with.
fn trim(w: &mut Watch) {
    while w.pending.len() > 1 && (w.pending.len() > MAX_PENDING || w.pending_bytes > MAX_PENDING_BYTES) {
        let (gone, ..) = w.pending.pop_front().expect("over a limit, so not empty");
        w.pending_bytes -= gone.len();
        w.lost += 1;
    }
}

/// Each watch's waiting rows in turn, one at a time from each, starting further
/// along at every look, until they are told or `budget` is spent.
fn tell(watches: &mut [Watch], turn: usize, budget: &mut Budget) -> Vec<Value> {
    let mut events = Vec::new();
    let count = watches.len();
    loop {
        let mut told = false;
        for k in 0..count {
            let w = &mut watches[(turn + k) % count];
            let Some((line, fresh, cut)) = w.pending.front() else { continue };
            if !budget.take(w.id.len() + line.len() + 128) {
                return events;
            }
            let mut data = json!({"watch": w.id, "line": line, "fresh": fresh});
            if *cut {
                data["cut"] = true.into();
            }
            // Rows dropped before this one ride on it rather than take a turn of their own.
            if w.lost > 0 {
                data["lost"] = std::mem::take(&mut w.lost).into();
            }
            events.push(json!({"type": "output", "terminal": crate::terminals::address(w.terminal), "data": data}));
            let (line, ..) = w.pending.pop_front().expect("its front was just read");
            w.pending_bytes -= line.len();
            told = true;
        }
        if !told {
            return events;
        }
    }
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
        assert!(w[0].find.find("٣ failed").is_some() && w[0].find.find("x failed").is_none());
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

    fn fail_watch() -> Watch {
        compile(&[watch(OutputMatch::Substring("FAIL".into()), 40)], |_| Ok(1), |_| false).unwrap().remove(0)
    }

    fn lines(rows: &[&str]) -> Vec<String> {
        rows.iter().map(|r| r.to_string()).collect()
    }

    fn told(events: &[Value]) -> Vec<(Value, Value)> {
        events.iter().map(|e| (e["data"]["line"].clone(), e["data"]["fresh"].clone())).collect()
    }

    #[test]
    fn rows_wait_their_turn_and_keep_whether_they_were_there_at_the_start() {
        let mut w = [fail_watch()];
        observe(&mut w[0], &lines(&["a FAIL", "ok", "b FAIL", "a FAIL"]));
        let first = tell(&mut w, 0, &mut Budget { events: 2, bytes: 1 << 20 });
        assert_eq!(told(&first), [(json!("a FAIL"), json!(true)), (json!("b FAIL"), json!(true))], "in their order");
        observe(&mut w[0], &lines(&["a FAIL", "b FAIL", "a FAIL", "c FAIL"]));
        let next = tell(&mut w, 1, &mut Budget { events: 9, bytes: 1 << 20 });
        assert_eq!(told(&next), [(json!("a FAIL"), json!(true)), (json!("c FAIL"), json!(false))]);
        observe(&mut w[0], &lines(&["a FAIL", "b FAIL", "a FAIL", "c FAIL"]));
        assert!(
            tell(&mut w, 2, &mut Budget { events: 9, bytes: 1 << 20 }).is_empty(),
            "standing rows are not told again"
        );
    }

    #[test]
    fn a_busy_watch_leaves_room_for_a_quiet_one() {
        let mut ws = [fail_watch(), fail_watch()];
        let busy: Vec<String> = (0..200).map(|n| format!("{n} FAIL")).collect();
        observe(&mut ws[0], &busy);
        observe(&mut ws[1], &lines(&["server FAIL"]));
        let events = tell(&mut ws, 0, &mut Budget { events: 3, bytes: 1 << 20 });
        assert!(told(&events).contains(&(json!("server FAIL"), json!(true))), "{events:?}");
    }

    #[test]
    fn rows_left_waiting_past_what_a_watch_keeps_go_and_the_next_row_says_so() {
        let mut w = [fail_watch()];
        let flood: Vec<String> = (0..MAX_PENDING + 5).map(|n| format!("{n} FAIL")).collect();
        observe(&mut w[0], &flood);
        assert!(tell(&mut w, 0, &mut Budget { events: 0, bytes: 1 << 20 }).is_empty());
        trim(&mut w[0]);
        let events = tell(&mut w, 0, &mut Budget { events: 2, bytes: 1 << 20 });
        assert_eq!(
            (&events[0]["data"]["line"], &events[0]["data"]["lost"]),
            (&json!("5 FAIL"), &json!(5)),
            "the oldest went"
        );
        assert!(events[1]["data"].get("lost").is_none());
    }

    #[test]
    fn a_last_row_past_the_limit_still_waits_to_be_told() {
        let mut w = [fail_watch()];
        let report = format!("FAIL {}", "x".repeat(MAX_PENDING_BYTES + 1));
        observe(&mut w[0], &lines(&[&report]));
        assert!(tell(&mut w, 0, &mut Budget { events: 0, bytes: 1 << 20 }).is_empty());
        trim(&mut w[0]);
        let events = tell(&mut w, 0, &mut Budget { events: 1, bytes: 1 << 20 });
        assert!(report.starts_with(events[0]["data"]["line"].as_str().unwrap()), "told, cut to MAX_LINE");
    }

    #[test]
    fn a_row_too_long_for_any_look_is_told_cut() {
        let mut ws = [fail_watch(), fail_watch()];
        let huge = format!("FAIL {}", "é".repeat(BYTES_PER_LOOK));
        observe(&mut ws[0], &lines(&[&huge]));
        observe(&mut ws[1], &lines(&["server FAIL"]));
        let events = tell(&mut ws, 0, &mut Budget { events: EVENTS_PER_LOOK, bytes: BYTES_PER_LOOK });
        let line = events[0]["data"]["line"].as_str().unwrap();
        assert!(line.len() <= MAX_LINE + 3 && huge.starts_with(line) && events[0]["data"]["cut"] == true);
        assert_eq!(events[1]["data"]["line"], "server FAIL", "the other watch is not held up");
    }

    #[test]
    fn a_cut_row_keeps_what_matched() {
        let mut w = [compile(&[watch(OutputMatch::Regex(r"http://localhost:\d+".into()), 40)], |_| Ok(1), |_| false)
            .unwrap()
            .remove(0)];
        let log = format!("{} listening on http://localhost:5173 {}", "x".repeat(70_000), "y".repeat(70_000));
        observe(&mut w[0], &lines(&[&log, "short http://localhost:3000"]));
        let events = tell(&mut w, 0, &mut Budget { events: 9, bytes: 1 << 20 });
        let first = &events[0]["data"];
        assert!(first["line"].as_str().unwrap().contains("http://localhost:5173") && first["cut"] == true);
        assert!(first["line"].as_str().unwrap().len() <= MAX_LINE + 3);
        assert_eq!(
            (&events[1]["data"]["line"], events[1]["data"].get("cut")),
            (&json!("short http://localhost:3000"), None)
        );
    }

    #[test]
    fn a_match_that_fits_is_told_whole_whatever_characters_come_before() {
        let line = format!("😀{}OK{}", "x".repeat(MAX_LINE - 4), "tail".repeat(30_000));
        let at = Find::Pattern(regex::Regex::new("x+OK").unwrap()).find(&line).unwrap();
        assert!(at.len() <= MAX_LINE);
        let (told, cut) = excerpt(&line, at.clone());
        assert!(cut && told.contains(&line[at]), "the whole match is in what is told");
    }

    #[test]
    fn a_burst_the_look_can_send_is_sent_whole() {
        let mut w = [fail_watch()];
        let burst: Vec<String> = (0..180).map(|n| format!("{n:03} FAIL {}", "x".repeat(380))).collect();
        assert!(burst.iter().map(String::len).sum::<usize>() > MAX_PENDING_BYTES);
        observe(&mut w[0], &burst);
        let events = tell(&mut w, 0, &mut Budget { events: EVENTS_PER_LOOK, bytes: BYTES_PER_LOOK });
        trim(&mut w[0]);
        assert_eq!(events.len(), 180);
        assert!(events.iter().all(|e| e["data"].get("lost").is_none()));
    }

    #[test]
    fn losses_never_take_the_turns_rows_need() {
        let mut ws: Vec<Watch> = (0..4).map(|_| fail_watch()).collect();
        for w in &mut ws {
            let flood: Vec<String> = (0..MAX_PENDING + 1).map(|n| format!("{n} FAIL")).collect();
            observe(w, &flood);
            trim(w);
        }
        let events = tell(&mut ws, 0, &mut Budget { events: 4, bytes: 1 << 20 });
        assert!(events.iter().all(|e| e["data"]["line"].is_string()), "every turn carries a row");
    }
}
