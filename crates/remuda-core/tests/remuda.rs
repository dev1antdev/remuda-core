#![cfg(unix)] // remuda's own changes (REMUDA.md); Windows: tests/windows.rs

mod common;

use common::framed::Viewer;
use common::*;
use remuda_core::wire;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn session(quiet_ms: &str) -> Session {
    let mut s = Session::new();
    s.env.push(("RC_QUIET_MS".into(), quiet_ms.into()));
    s
}

fn bytes(s: &Session) -> String {
    s.json(&["run", "--bg", "sh", "-c", BYTES])["address"].as_str().unwrap().to_string()
}

#[test]
fn keys_now_skip_the_pause_a_person_typing_starts() {
    let s = session("3000");
    let t = bytes(&s);
    let mut person = Viewer::attach(&s, &t, 80, 24, json!({}));
    person.typed(b"\x1b[A");
    person.shows("41");
    let pressed = Instant::now();
    s.connect().call(1, "terminal.keys", json!({"terminal": t, "keys": ["a"], "now": true}));
    person.shows("61");
    assert!(pressed.elapsed() < Duration::from_millis(100), "arrived after {:?}", pressed.elapsed());
}

#[test]
fn keys_now_pass_a_keys_call_waiting_for_the_pause() {
    let s = session("3000");
    let t = bytes(&s);
    let mut person = Viewer::attach(&s, &t, 80, 24, json!({}));
    person.typed(b"\x1b[A");
    person.shows("41");
    let waiting = {
        let mut cmd = s.cmd();
        cmd.args(["keys", &t, "b"]);
        std::thread::spawn(move || cmd.output().unwrap())
    };
    std::thread::sleep(Duration::from_millis(150));
    let pressed = Instant::now();
    s.connect().call(2, "terminal.keys", json!({"terminal": t, "keys": ["a"], "now": true}));
    person.shows("61");
    assert!(
        pressed.elapsed() < Duration::from_millis(250),
        "the now key waited for the queued keys: {:?}",
        pressed.elapsed()
    );
    assert!(!waiting.is_finished(), "the regular keys did not wait for the pause");
    assert!(waiting.join().unwrap().status.success());
    person.shows("62");
}

#[test]
fn keys_without_now_still_wait_for_the_pause() {
    let s = session("600");
    let t = bytes(&s);
    let mut person = Viewer::attach(&s, &t, 80, 24, json!({}));
    person.typed(b"\x1b[A");
    person.shows("41");
    let pressed = Instant::now();
    s.json(&["keys", &t, "b"]);
    assert!(pressed.elapsed() >= Duration::from_millis(500), "did not wait: {:?}", pressed.elapsed());
}

#[test]
fn a_viewer_that_keeps_the_size_types_without_taking_it() {
    let s = session("0");
    let t = bytes(&s);
    let mut desk = Viewer::attach(&s, &t, 100, 30, json!({}));
    assert_eq!(get(&s, &t)["cols"], 100);
    let mut phone = Viewer::attach(&s, &t, 40, 20, json!({"keep_size": true, "resize_notifications": true}));
    assert_eq!(get(&s, &t)["cols"], 100, "attaching");
    let grid = std::iter::from_fn(|| phone.conn.stash.pop_front().or_else(|| phone.conn.read(Duration::from_secs(2))))
        .find(|(_, kind, _)| *kind == wire::RESIZE);
    assert_eq!(grid.map(|(_, _, size)| size), Some(vec![100, 0, 30, 0]), "told the terminal's own grid");

    phone.conn.send(phone.channel, wire::RESIZE, &[50, 0, 25, 0]);
    phone.conn.send(phone.channel, wire::FOCUS, &[1]);
    phone.typed(b"x");
    phone.shows("78");
    assert_eq!(get(&s, &t)["cols"], 100, "typing, resizing and focusing");

    desk.conn.send(desk.channel, wire::CLOSE, &[]);
    desk.until("detached", |v| v.closed.is_some());
    assert_eq!(get(&s, &t)["cols"], 100, "the desk left");
    drop(phone);
    wait_until("headless again", || get(&s, &t)["cols"] == 200);
}

#[test]
fn an_observer_asking_to_keep_the_size_is_an_observer() {
    let s = session("0");
    let t = bytes(&s);
    let mut desk = Viewer::attach(&s, &t, 100, 30, json!({}));
    let _watching = Viewer::attach(&s, &t, 40, 20, json!({"observe": true, "keep_size": true}));
    assert_eq!(get(&s, &t)["cols"], 100);
    desk.conn.send(desk.channel, wire::CLOSE, &[]);
    desk.until("detached", |v| v.closed.is_some());
    wait_until("headless with only an observer left", || get(&s, &t)["cols"] == 200);
}

#[test]
fn read_in_vt_gives_each_row_with_its_colors() {
    let s = session("0");
    let program = "printf '\\033]2;title\\007\\033[1;31mred\\033[0m plain\\n'; sleep 100";
    let t = s.json(&["run", "--bg", "sh", "-c", program])["address"].as_str().unwrap().to_string();
    wait_until("printed", || text(&s, &t).contains("red plain"));
    let vt = s.json(&["read", "--format", "vt", &t])["text"].as_str().unwrap().to_string();
    let rows: Vec<&str> = vt.split('\n').collect();
    assert!(rows.iter().all(|r| r.starts_with("\x1b[0m")), "{vt:?}");
    assert!(rows[0].contains("\x1b[0;1;38;5;1mred"), "{vt:?}");
    assert!(!vt.contains("\x1b]") && !vt.contains("\x1b[?") && !vt.contains("\x1b[H"), "{vt:?}");
    assert!(!s.json(&["read", &t])["text"].as_str().unwrap().contains('\x1b'), "plain stays plain");

    s.json(&["kill", &t]);
    wait_until("finished", || get(&s, &t)["state"] == "finished");
    let kept = s.json(&["read", "--format", "vt", &t])["text"].as_str().unwrap().to_string();
    assert!(kept.contains("red plain") && !kept.contains('\x1b'), "{kept:?}");
}

#[test]
fn rc_read_in_vt_leaves_a_programs_own_rc_lines_on_stdout() {
    let s = session("0");
    let t =
        s.json(&["run", "--bg", "sh", "-c", "printf 'starting\\n[rc] build finished\\nartifact.zip\\n'"])["address"]
            .as_str()
            .unwrap()
            .to_string();
    wait_until("finished", || get(&s, &t)["state"] == "finished");
    let out = s.rc(&["read", "--format", "vt", &t]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("starting\n[rc] build finished\nartifact.zip"), "{stdout:?}");
    assert!(out.stderr.is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn read_in_vt_keeps_its_notes_off_the_rows() {
    let s = session("0");
    let program = "printf '\\033[?1049h\\033[31mfull'; sleep 100";
    let t = s.json(&["run", "--bg", "sh", "-c", program])["address"].as_str().unwrap().to_string();
    wait_until("drawn", || text(&s, &t).contains("full"));
    let out = s.rc(&["read", "--format", "vt", "--lines", "5", &t]);
    let rows = String::from_utf8(out.stdout).unwrap();
    assert!(rows.lines().all(|r| r.starts_with("\x1b[0m")), "{rows:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("[rc] incomplete"));
}

#[test]
fn read_in_vt_reads_as_far_above_the_screen_as_it_is_asked() {
    let s = session("0");
    // One line wrapping over far more rows than the screen: no newline to count.
    let program = "head -c 240000 /dev/zero | tr '\\0' x; printf END; sleep 100";
    let t = s.json(&["run", "--bg", "sh", "-c", program])["address"].as_str().unwrap().to_string();
    wait_until("printed", || text(&s, &t).ends_with("END"));
    let screen = get(&s, &t)["rows"].as_u64().unwrap() as usize;
    // The page's find reads deeper than its own rows: what it asks for, then all there is.
    let asked = s.json(&["read", "--format", "vt", "--lines", "500", &t]);
    assert_eq!(asked["text"].as_str().unwrap().split('\n').count(), 500 + screen);
    assert_eq!(asked["complete"].as_bool(), Some(true));
    let all = s.json(&["read", "--format", "vt", "--lines", "5000", &t]);
    assert!(all["text"].as_str().unwrap().contains("xxxx"), "the oldest row of the history came with it");
    assert_eq!(all["complete"].as_bool(), Some(true), "nothing was dropped");
}

#[test]
fn read_in_vt_reaches_2500_rows_above_the_screen() {
    let s = session("0");
    // A narrow terminal nobody types in, so 100000 characters wrap over 2500 rows.
    std::fs::create_dir_all(s.root.join("config/remuda-core")).unwrap();
    std::fs::write(s.root.join("config/remuda-core/config.toml"), "headless = \"40x10\"\n").unwrap();
    // A unique word first, then 2500 wrapped rows: far more than the page's xterm keeps above the screen.
    let program = "printf 'UNIQUE-TOP\\n'; head -c 100000 /dev/zero | tr '\\0' x; printf END; sleep 100";
    let t = s.json(&["run", "--bg", "sh", "-c", program])["address"].as_str().unwrap().to_string();
    wait_until("printed", || text(&s, &t).ends_with("END"));
    let r = s.json(&["read", "--format", "vt", "--lines", "3000", &t]);
    assert!(
        r["text"].as_str().unwrap().contains("UNIQUE-TOP"),
        "2500 rows above the screen, and the read stopped short"
    );
    assert_eq!(r["complete"].as_bool(), Some(true), "nothing was dropped");
}

#[test]
fn read_in_vt_stops_at_its_row_bound_and_says_so() {
    let s = session("0");
    // A history big enough to hold more rows than one read encodes, and far more printed than that.
    std::fs::create_dir_all(s.root.join("config/remuda-core")).unwrap();
    std::fs::write(s.root.join("config/remuda-core/config.toml"), "headless = \"40x10\"\nhistory_mb = 20\n").unwrap();
    let program = "head -c 810000 /dev/zero | tr '\\0' x; printf END; sleep 100";
    let t = s.json(&["run", "--bg", "sh", "-c", program])["address"].as_str().unwrap().to_string();
    wait_until("printed", || text(&s, &t).ends_with("END"));
    let r = s.json(&["read", "--format", "vt", "--lines", "100000", &t]);
    let rows = r["text"].as_str().unwrap().split('\n').count();
    assert_eq!(rows, 5_000 + get(&s, &t)["rows"].as_u64().unwrap() as usize, "one read's row bound");
    assert_eq!(r["complete"].as_bool(), Some(false), "the rows past the bound were not read");
    // The bound is not a lie about short histories: asking past it with nothing there is complete.
    let short =
        s.json(&["run", "--bg", "sh", "-c", "printf 'only\\n'; sleep 100"])["address"].as_str().unwrap().to_string();
    wait_until("printed", || text(&s, &short).contains("only"));
    let nothing = s.json(&["read", "--format", "vt", "--lines", "20001", &short]);
    assert_eq!(nothing["complete"].as_bool(), Some(true), "every row there was read");
}

#[test]
fn read_in_vt_says_when_rows_it_asked_for_are_gone() {
    let s = session("0");
    // A history this small cannot hold what the program prints, so the read must say some is gone.
    std::fs::create_dir_all(s.root.join("config/remuda-core")).unwrap();
    std::fs::write(s.root.join("config/remuda-core/config.toml"), "headless = \"40x10\"\nhistory_mb = 0.05\n").unwrap();
    let t = s.json(&["run", "--bg", "sh", "-c", "seq 1 3000; sleep 100"])["address"].as_str().unwrap().to_string();
    wait_until("printed", || text(&s, &t).ends_with("3000"));
    let all = s.json(&["read", "--format", "vt", "--lines", "5000", &t]);
    assert_eq!(all["complete"].as_bool(), Some(false), "asking past what the history kept");
    let kept = s.json(&["read", "--format", "vt", "--lines", "20", &t]);
    assert_eq!(kept["complete"].as_bool(), Some(true), "well within what the history kept");
}

// One long line, no line break: it wraps over thousands of rows, and once the history overflows the old rows are gone
// though no newline was printed. The read must say so: rows are counted on the write path, not line breaks.
#[test]
fn a_wrapped_line_that_overflowed_the_history_reads_incomplete() {
    let s = session("0");
    std::fs::create_dir_all(s.root.join("config/remuda-core")).unwrap();
    std::fs::write(s.root.join("config/remuda-core/config.toml"), "headless = \"40x10\"\nhistory_mb = 0.05\n").unwrap();
    // 200000 characters in 40 columns: 5000 rows, far more than a history this small keeps, and not one line break.
    let t =
        s.json(&["run", "--bg", "sh", "-c", "head -c 200000 /dev/zero | tr '\\0' x; printf END; sleep 300"])["address"]
            .as_str()
            .unwrap()
            .to_string();
    wait_until("printed", || text(&s, &t).ends_with("END"));
    let vt = s.json(&["read", "--format", "vt", "--lines", "5000", &t]);
    assert_eq!(vt["complete"].as_bool(), Some(false), "rows were dropped, and no line break said so: {vt}");
    let plain = s.json(&["read", "--lines", "5000", &t]);
    assert_eq!(plain["complete"].as_bool(), Some(false), "the plain read too");

    // A history that held everything it printed: complete.
    let short = s.json(&["run", "--bg", "sh", "-c", "printf 'a\\nb\\nc\\n'; sleep 300"])["address"]
        .as_str()
        .unwrap()
        .to_string();
    wait_until("printed", || text(&s, &short).contains("c"));
    let all = s.json(&["read", "--format", "vt", "--lines", "5000", &short]);
    assert_eq!(all["complete"].as_bool(), Some(true), "a short history is all kept: {all}");
}

// A deep history read takes a while. The core runs its tasks on one thread, so the read is worked out off it and its
// lock is held only for the snapshot it starts from: another terminal must keep answering meanwhile, or every pane
// waits for the deepest read in the fleet.
#[test]
fn a_deep_read_leaves_the_other_terminals_answering() {
    let s = session("0");
    std::fs::create_dir_all(s.root.join("config/remuda-core")).unwrap();
    std::fs::write(s.root.join("config/remuda-core/config.toml"), "headless = \"200x50\"\nhistory_mb = 20\n").unwrap();
    let deep = s.json(&["run", "--bg", "sh", "-c", "seq 1 200000; printf END; sleep 300"])["address"]
        .as_str()
        .unwrap()
        .to_string();
    wait_until("printed", || text(&s, &deep).ends_with("END"));
    let other = s.json(&["run", "--bg", "sh", "-c", BYTES])["address"].as_str().unwrap().to_string();

    let mut reader = s.connect();
    let mut fast = s.connect();
    let reading = Arc::new(AtomicBool::new(true));
    let (rows, read, worst) = std::thread::scope(|scope| {
        let done = Arc::clone(&reading);
        let handle = scope.spawn(move || {
            let began = Instant::now();
            let out = reader.call(1, "terminal.read", json!({"terminal": deep, "lines": 5000, "format": "vt"}));
            let read = began.elapsed();
            done.store(false, Ordering::SeqCst);
            (out["result"]["text"].as_str().unwrap_or_default().lines().count(), read)
        });
        std::thread::sleep(Duration::from_millis(2));
        let mut worst = Duration::ZERO;
        while reading.load(Ordering::SeqCst) {
            let t = Instant::now();
            fast.call(2, "terminal.get", json!({"terminal": other}));
            fast.call(3, "terminal.read", json!({"terminal": other, "lines": 0}));
            worst = worst.max(t.elapsed());
        }
        let (rows, read) = handle.join().unwrap();
        (rows, read, worst)
    });
    assert!(rows > 1000, "the read itself came short: {rows} rows");
    assert!(worst * 3 < read, "a terminal waited {worst:?} while another's {read:?} read ran: the read holds the core");
}

#[test]
fn output_watches_tell_of_each_matching_row_as_it_comes() {
    let s = session("0");
    let program = "stty -echo; echo 'standing FAIL'; while read line; do echo \"got $line\"; done";
    let t = s.json(&["run", "--bg", "sh", "-c", program])["address"].as_str().unwrap().to_string();
    wait_until("standing", || text(&s, &t).contains("standing FAIL"));
    let mut events = s.connect();
    let watches = json!([
        {"id": "fail", "terminal": t, "match": {"type": "substring", "value": "FAIL"}},
        {"id": "count", "terminal": t, "match": {"type": "regex", "value": r"^got \d+$"}, "lines": 10},
    ]);
    assert!(events.call(1, "events.subscribe", json!({"output": watches}))["result"]["seq"].is_u64());
    let output = |events: &mut common::Conn| {
        let event = next_event(events, |m| m["params"]["type"] == "output");
        let data = &event["params"]["data"];
        assert_ne!(
            (data["line"].as_str(), data["fresh"].as_bool()),
            (Some("standing FAIL"), Some(false)),
            "told twice"
        );
        (data["watch"].as_str().unwrap().to_string(), data["line"].as_str().unwrap().to_string(), data["fresh"] == true)
    };
    assert_eq!(output(&mut events), ("fail".into(), "standing FAIL".into(), true), "already on the screen");
    s.json(&["send", &t, "x FAIL"]);
    assert_eq!(output(&mut events), ("fail".into(), "got x FAIL".into(), false));
    s.json(&["send", &t, "٣"]);
    assert_eq!(output(&mut events), ("count".into(), "got ٣".into(), false), "a Unicode digit");
    s.json(&["send", &t, "x FAIL"]);
    assert_eq!(output(&mut events), ("fail".into(), "got x FAIL".into(), false), "the same text, again");

    let refused = s.connect().call(
        1,
        "events.subscribe",
        json!({"output": [{"id": "bad", "terminal": t, "match": {"type": "regex", "value": "("}}]}),
    );
    assert_eq!(refused["error"]["data"]["kind"], "invalid_regex", "{refused}");
    assert!(refused["error"]["message"].as_str().unwrap().contains("\"bad\""));
}

#[test]
fn an_output_watch_hears_a_command_that_prints_and_exits_at_once() {
    let s = session("0");
    let go = s.root.join("go");
    let program = format!("while [ ! -e {} ]; do sleep 0.02; done; echo 'last words FAIL'", go.display());
    let t = s.json(&["run", "--bg", "sh", "-c", &program])["address"].as_str().unwrap().to_string();
    let mut events = s.connect();
    let watch = json!([{"id": "fail", "terminal": t, "match": {"type": "substring", "value": "FAIL"}}]);
    events.call(1, "events.subscribe", json!({"output": watch}));
    std::thread::sleep(Duration::from_millis(400));
    std::fs::write(&go, "").unwrap();
    let event = next_event(&mut events, |m| m["params"]["type"] == "output");
    assert_eq!(event["params"]["data"], json!({"watch": "fail", "line": "last words FAIL", "fresh": false}));
}

#[test]
fn an_output_watch_outlives_a_kept_terminal_restarting() {
    let s = session("0");
    // It prints and dies by a signal at once: the look after it finds the terminal between shells.
    let program = "stty -echo; echo 'one FAIL'; read x; echo 'gone FAIL'; kill -9 $$";
    let t = s.json(&["run", "--bg", "--keep", "sh", "-c", program])["address"].as_str().unwrap().to_string();
    wait_until("printed", || text(&s, &t).contains("one FAIL"));
    let mut events = s.connect();
    let watch = json!([{"id": "fail", "terminal": t, "match": {"type": "regex", "value": "^(one|gone|two) FAIL$"}}]);
    events.call(1, "events.subscribe", json!({"output": watch}));
    let line = |events: &mut common::Conn| {
        next_event(events, |m| m["params"]["type"] == "output")["params"]["data"]["line"].as_str().unwrap().to_string()
    };
    assert_eq!(line(&mut events), "one FAIL");
    let old = get(&s, &t)["pid"].as_i64().unwrap();
    s.json(&["send", &t, "go"]);
    replaced(&s, &t, old);
    s.json(&["send", &t, "printf 'two FAIL\\n'"]);
    let mut heard = line(&mut events);
    if heard == "gone FAIL" {
        heard = line(&mut events);
    }
    assert_eq!(heard, "two FAIL");
}

/// The process group in front of `t`, once the core has looked (0 before).
fn in_front(s: &Session, t: &str) -> i64 {
    get(s, t)["foreground"]["pid"].as_i64().unwrap_or(0)
}

/// Waits for keep to give the terminal a new shell in place of the program `old` was.
fn replaced(s: &Session, t: &str, old: i64) {
    wait_until("replacement shell", || {
        let r = get(s, t);
        r["state"] == "live" && r["pid"].as_i64().is_some_and(|p| p != old)
    });
}

/// `terminal.send` naming the process it is for (API only: the CLI has no flag for it).
fn send_for(s: &Session, t: &str, text: &str, front: i64) -> serde_json::Value {
    s.connect().call(1, "terminal.send", json!({"terminal": t, "text": text, "foreground": front}))
}

#[test]
fn a_send_for_a_program_that_left_while_a_draft_waited_types_nothing() {
    let mut s = session("0");
    s.env.push(("RC_DRAFT_MS".into(), "1500".into()));
    s.env.push(("RC_FOREGROUND_MS".into(), "50".into()));
    let t = run(&s, &["sh"]);
    let shell = get(&s, &t)["pid"].as_i64().unwrap();
    wait_until("the shell in front", || in_front(&s, &t) == shell);
    s.json(&["send", &t, "sleep 30"]);
    wait_until("sleep in front", || ![0, shell].contains(&in_front(&s, &t)));
    let sleep = in_front(&s, &t);
    let mut person = Viewer::attach(&s, &t, 80, 24, json!({}));
    person.typed(b"draft");
    person.shows("draft");
    std::thread::scope(|scope| {
        let send = scope.spawn(|| send_for(&s, &t, "echo SENT-$((6*7))", sleep));
        std::thread::sleep(Duration::from_millis(300));
        unsafe { libc::kill(sleep as i32, libc::SIGKILL) };
        let reply = send.join().unwrap();
        assert_eq!(reply["error"]["data"]["kind"], "status_mismatch", "{reply}");
        assert_eq!(reply["error"]["data"]["hint"], "nothing was typed", "{reply}");
    });
    wait_until("the shell in front again", || in_front(&s, &t) == shell);
    person.typed(b"\x15");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!text(&s, &t).contains("SENT"), "typed into the shell: {}", text(&s, &t));
    let reply = send_for(&s, &t, "echo SENT-$((6*7))", shell);
    assert!(reply["error"].is_null(), "{reply}");
    wait_until("a send naming what is in front goes in", || text(&s, &t).contains("SENT-42"));
}

#[test]
fn a_send_naming_done_goes_to_a_program_that_is_done_and_idle_still_does_not() {
    let mut s = session("0");
    s.env.push(("RC_STARTED_MS".into(), "100".into()));
    let program = "r state=done; read -r l; echo got:$l; read -r l; echo got:$l; sleep 100";
    let t = run(&s, &["sh", "-c", &script(&s, program)]);
    wait_until("done", || get(&s, &t)["status"]["state"] == "done");
    let (code, err) = s.err(&["send", "--if", "idle", &t, "nope"]);
    assert_eq!((code, err["error"]["kind"].as_str()), (10, Some("status_mismatch")), "{err}");
    let sent = s.json(&["send", "--if", "done", &t, "yes"]);
    assert_eq!(sent["status"], "done", "{sent}");
    wait_until("the text went in", || text(&s, &t).contains("got:yes"));
    wait_until("the program in front", || in_front(&s, &t) > 0);
    let guarded = json!({"terminal": t, "text": "again", "if": "done", "foreground": in_front(&s, &t)});
    let reply = s.connect().call(1, "terminal.send", guarded);
    assert!(reply["error"].is_null(), "{reply}");
    wait_until("the text checked again after the look went in", || text(&s, &t).contains("got:again"));
    assert!(!text(&s, &t).contains("nope"), "{}", text(&s, &t));
}

#[test]
fn a_send_naming_done_to_a_blocked_program_is_refused_as_blocked_unless_forced() {
    let s = session("0");
    let t = run(&s, &["sh", "-c", &script(&s, "r state=blocked:kind=permission; sleep 100")]);
    wait_until("blocked", || get(&s, &t)["status"]["state"] == "blocked");
    let (code, err) = s.err(&["send", "--if", "done", &t, "nope"]);
    assert_eq!((code, err["error"]["kind"].as_str()), (9, Some("blocked")), "{err}");
    let (code, err) = s.err(&["send", "--force", "--if", "done", &t, "nope"]);
    assert_eq!((code, err["error"]["kind"].as_str()), (10, Some("status_mismatch")), "{err}");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!text(&s, &t).contains("nope"), "{}", text(&s, &t));
}

#[test]
fn a_send_naming_done_types_nothing_once_the_program_works_while_a_draft_waited() {
    let mut s = session("0");
    s.env.push(("RC_DRAFT_MS".into(), "2000".into()));
    let t = run(&s, &["sh", "-c", &script(&s, "r state=done; w go; r state=working; sleep 100")]);
    wait_until("done", || get(&s, &t)["status"]["state"] == "done");
    let mut person = Viewer::attach(&s, &t, 80, 24, json!({}));
    person.typed(b"draft");
    person.shows("draft");
    std::thread::scope(|scope| {
        let send =
            scope.spawn(|| s.connect().call(1, "terminal.send", json!({"terminal": t, "text": "SENT", "if": "done"})));
        std::thread::sleep(Duration::from_millis(300));
        touch(&s, "go");
        let reply = send.join().unwrap();
        assert_eq!(reply["error"]["data"]["kind"], "status_mismatch", "{reply}");
        assert_eq!(reply["error"]["data"]["hint"], "nothing was typed", "{reply}");
    });
    assert_eq!(get(&s, &t)["status"]["state"], "working");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!text(&s, &t).contains("SENT"), "typed into a turn: {}", text(&s, &t));
}

/// A template named after `name`, whose command says so if it ever runs (no first message to wait to send).
fn template_named(s: &Session, name: &str) {
    let dir = s.root.join("config/remuda-core/templates");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.md")), "---\ncommand: sh -c 'echo TEMPLATE-RAN; sleep 30'\n---\n").unwrap();
}

fn run_api(s: &Session, params: serde_json::Value) -> serde_json::Value {
    s.json(&["ls"]);
    s.connect().call(1, "terminal.run", params)
}

#[test]
fn a_literal_run_starts_its_program_though_a_template_has_its_name() {
    let s = session("0");
    template_named(&s, "printf");
    let reply = run_api(&s, json!({"argv": ["printf", "LITERAL-%s\\n", "ok"], "literal": true, "wait": true}));
    assert!(reply["error"].is_null(), "{reply}");
    let result = &reply["result"];
    assert!(result["first_message"].is_null(), "{reply}");
    assert!(result["output"].as_str().unwrap().contains("LITERAL-ok"), "{reply}");
    let templated = run(&s, &["printf"]);
    wait_until("the template runs without literal", || text(&s, &templated).contains("TEMPLATE-RAN"));
}

#[test]
fn a_literal_run_of_a_missing_program_fails_to_spawn_though_a_template_has_its_name() {
    let s = session("0");
    template_named(&s, "rc-no-such-program");
    let reply = run_api(&s, json!({"argv": ["rc-no-such-program"], "literal": true}));
    assert_eq!(reply["error"]["data"]["kind"], "spawn_failed", "{reply}");
}

#[test]
fn a_literal_run_takes_argv_not_a_line_and_a_boolean() {
    let s = session("0");
    for params in [
        json!({"line": "echo hi", "literal": true}),
        json!({"literal": true}),
        json!({"argv": ["printf", "x"], "literal": "true"}),
        json!({"argv": ["printf", "x"], "literal": null}),
    ] {
        let reply = run_api(&s, params);
        assert_eq!(reply["error"]["data"]["kind"], "invalid_params", "{reply}");
    }
}

#[test]
fn a_terminals_record_says_whether_it_was_run_literally() {
    let s = session("0");
    let literal = run_api(&s, json!({"argv": ["sleep", "100"], "literal": true}));
    assert_eq!(literal["result"]["literal"], true, "{literal}");
    assert_eq!(get(&s, literal["result"]["address"].as_str().unwrap())["literal"], true);
    let plain = run_api(&s, json!({"argv": ["sleep", "100"]}));
    assert_eq!(plain["result"]["literal"], false, "{plain}");
}

#[test]
fn the_shell_keep_gives_a_lost_literal_run_is_not_literal() {
    let s = session("0");
    let reply = run_api(&s, json!({"argv": ["sleep", "100"], "literal": true, "keep": true}));
    let t = reply["result"]["address"].as_str().unwrap().to_string();
    let old = get(&s, &t)["pid"].as_i64().unwrap();
    let holder = s.json(&["server", "status"])["holder"]["pid"].as_i64().unwrap();
    unsafe { libc::kill(holder as i32, libc::SIGKILL) };
    replaced(&s, &t, old);
    assert_eq!(get(&s, &t)["literal"], false);
}

#[test]
fn a_literal_run_stays_literal_until_keeps_shell_has_started() {
    use std::os::unix::fs::PermissionsExt;
    let mut s = session("0");
    let shell = s.root.join("shell");
    s.env.push(("SHELL".into(), shell.display().to_string()));
    let reply = run_api(&s, json!({"argv": ["sleep", "100"], "literal": true, "keep": true}));
    let t = reply["result"]["address"].as_str().unwrap().to_string();
    s.json(&["server", "stop"]);
    get(&s, &t);
    std::thread::sleep(Duration::from_secs(2)); // its shell is missing: the first replacement fails
    let r = get(&s, &t);
    assert!(r["pid"].is_null() && r["reason"].is_string() && r["literal"] == true, "{r}");
    std::fs::write(&shell, "#!/bin/sh\nexec /bin/sh\n").unwrap();
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700)).unwrap();
    wait_until_for("keep's shell", Duration::from_secs(30), || get(&s, &t)["pid"].is_number());
    assert_eq!(get(&s, &t)["literal"], false);
}

#[test]
fn a_record_a_core_before_literal_record_wrote_says_nothing_of_literal() {
    let s = session("0");
    run_api(&s, json!({"argv": ["sleep", "100"], "literal": true}));
    let state = std::fs::read_to_string(s.root.join("state/remuda-core/test/terminals.json")).unwrap();
    let mut record = serde_json::from_str::<serde_json::Value>(&state).unwrap()["terminals"]["1"].take();
    assert_eq!(record["literal"], true, "{record}");
    record.as_object_mut().unwrap().remove("literal"); // as a core without the feature saves it
    let loaded: remuda_core::terminals::Terminal = serde_json::from_value(record).unwrap();
    let public = serde_json::to_value(remuda_core::terminals::Public(loaded)).unwrap();
    assert!(public.get("literal").is_none(), "unknown, not false: {public}");
}

/// `ps -o lstart` of `pid` in UTC, as unix seconds: how a client reading the process itself tells its start.
fn lstart(pid: i64) -> i64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let mut words = text.split_whitespace().skip(1); // the weekday
    let name = words.next().unwrap();
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
        .iter()
        .position(|m| *m == name)
        .unwrap() as u32
        + 1;
    let day: u32 = words.next().unwrap().parse().unwrap();
    let clock: Vec<i64> = words.next().unwrap().split(':').map(|n| n.parse().unwrap()).collect();
    let year: i64 = words.next().unwrap().parse().unwrap();
    // days since 1970 by the civil calendar (Howard Hinnant's days_from_civil)
    let (y, m) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m as i64 + 2) / 5 + day as i64 - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    days * 86_400 + clock[0] * 3600 + clock[1] * 60 + clock[2]
}

#[test]
fn the_foreground_tells_when_its_process_was_created_as_ps_does() {
    let mut s = session("0");
    s.env.push(("RC_FOREGROUND_MS".into(), "50".into()));
    s.json(&["ls"]);
    let hello = s.connect().call(1, "hello", json!({}));
    let features = hello["result"]["features"].as_array().unwrap();
    assert!(["foreground_started", "foreground_since_boot"].iter().all(|f| features.iter().any(|g| g == f)), "{hello}");
    let t = run(&s, &["sh"]);
    let shell = get(&s, &t)["pid"].as_i64().unwrap();
    wait_until("the shell in front", || in_front(&s, &t) == shell);
    let sent = remuda_core::runtime::now_ms() as i64;
    s.json(&["send", &t, "sleep 30"]);
    wait_until("sleep in front", || ![0, shell].contains(&in_front(&s, &t)));
    let front = get(&s, &t)["foreground"].clone();
    let sleep = front["pid"].as_i64().unwrap();
    let started = front["started_ms"].as_i64().unwrap_or_else(|| panic!("no started_ms in {front}"));
    // ps's second, or on Linux the next: ps counts the boot time in whole seconds
    let second = started.div_euclid(1000);
    assert!(second == lstart(sleep) || cfg!(target_os = "linux") && second == lstart(sleep) + 1, "{front}");
    assert!(started >= sent - 20, "the creation to the kernel's tick: sent at {sent}, {front}");
    // on Linux its start as the boot counts it, which no step of the clock moves; elsewhere started_ms is that already
    let since = &front["started_since_boot"];
    if cfg!(target_os = "linux") {
        let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap();
        let stat = std::fs::read_to_string(format!("/proc/{sleep}/stat")).unwrap();
        let ticks: u64 = stat[stat.rfind(')').unwrap() + 2..].split(' ').nth(19).unwrap().parse().unwrap();
        let ns = std::fs::read_link("/proc/self/ns/time").unwrap().display().to_string();
        let told = (since["boot_id"].as_str(), since["time_ns"].as_str(), since["ticks"].as_u64());
        assert_eq!(told, (Some(boot.trim()), Some(ns.as_str()), Some(ticks)), "{front}");
        // a reader in another time namespace sees the ticks shifted by its boot-time offset, and another namespace
        let elsewhere = std::process::Command::new("unshare")
            .args(["--user", "--map-root-user", "--time", "--boottime", "60", "--fork", "sh", "-c"])
            .arg(format!("readlink /proc/self/ns/time; cut -d' ' -f22 /proc/{sleep}/stat"))
            .output();
        match elsewhere {
            Ok(out) if out.status.success() => {
                let out = String::from_utf8_lossy(&out.stdout);
                let (their_ns, their_ticks) = out.trim().split_once('\n').unwrap();
                assert_ne!(their_ns, ns, "{out}");
                assert_ne!(their_ticks.parse::<u64>().unwrap(), ticks, "{out}");
            }
            _ => eprintln!("no time namespace of our own to read from here: the shifted ticks go unchecked"),
        }
    } else {
        assert!(since.is_null(), "{front}");
    }
    let shell_started = {
        unsafe { libc::kill(sleep as i32, libc::SIGKILL) };
        wait_until("the shell in front again", || in_front(&s, &t) == shell);
        get(&s, &t)["foreground"]["started_ms"].as_i64().unwrap()
    };
    assert!(shell_started <= started, "the shell began at {shell_started}, sleep at {started}");
}

/// A first paint past a stream's queue is dropped whole, and the viewer would get nothing: the newest
/// history rows fill the queue instead, the oldest going first, and the screen always arrives.
#[test]
fn a_first_paint_past_the_queue_cuts_the_oldest_history_and_brings_the_screen() {
    let s = Session::new();
    let config = s.root.join("config/remuda-core/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "history_mb = 64\n").unwrap();
    // 4000 rows of 80 cells, every cell its own colour: past wire::MAX_PENDING whole, whatever the formatter
    // makes of a cell's style.
    let cells = "$(printf '\\033[38;2;255;0;0mX\\033[38;2;0;255;0mY%.0s' $(seq 1 38))";
    let script = format!(
        "row={cells}; i=0; while [ $i -lt 4000 ]; do printf '%04d%s\\n' \"$i\" \"$row\"; i=$((i+1)); done; printf 'READY\\n'; sleep 100"
    );
    let t = run(&s, &["sh", "-c", &script]);
    wait_until_for("the whole history printed", Duration::from_secs(60), || {
        s.json(&["read", &t])["text"].as_str().unwrap_or_default().contains("READY")
    });
    let mut v = Viewer::attach(&s, &t, 80, 24, json!({"history": 4000}));
    v.shows("READY");
    v.drain(Duration::from_millis(300));
    assert!(v.received <= wire::MAX_PENDING, "the paint went past the queue: {} bytes", v.received);
    assert!(
        v.received > wire::MAX_PENDING - (256 << 10),
        "the newest history did not fill the queue: {} bytes",
        v.received
    );
    let raw = String::from_utf8_lossy(&v.raw);
    assert!(!raw.contains("0000"), "the oldest history did not go first");
    assert!(raw.contains("1300"), "the newest history is not what reached the viewer");
    // A request past the rows the terminal keeps is the same as asking for all of them, not a search over
    // numbers that are not there.
    let mut huge = Viewer::attach(&s, &t, 80, 24, json!({"history": u64::MAX}));
    huge.shows("READY");
    huge.drain(Duration::from_millis(300));
    assert!(huge.received <= wire::MAX_PENDING, "a huge request went past the queue: {} bytes", huge.received);
}

/// A full-screen program keeps the primary screen hidden; a viewer that asks for history still gets that
/// screen's rows replayed above it, and the alternate screen's own count of none cuts nothing.
#[test]
fn a_first_paint_while_a_full_screen_program_runs_replays_the_primary_history() {
    let s = Session::new();
    let script = "i=0; while [ $i -lt 300 ]; do echo log-$i; i=$((i+1)); done; printf '\\033[?1049h'; sleep 100";
    let t = run(&s, &["sh", "-c", script]);
    wait_until_for("the full-screen program", Duration::from_secs(30), || {
        s.json(&["read", &t, "--lines", "10"])["how"].as_str() == Some("screen")
    });
    let mut v = Viewer::attach(&s, &t, 80, 24, json!({"history": 400}));
    v.drain(Duration::from_millis(500));
    let raw = String::from_utf8_lossy(&v.raw);
    assert!(raw.contains("log-0"), "the primary screen's history was not replayed above the full-screen one");
}
