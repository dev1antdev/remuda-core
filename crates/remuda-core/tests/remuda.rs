#![cfg(unix)] // remuda's own changes (REMUDA.md); Windows: tests/windows.rs

mod common;

use common::framed::Viewer;
use common::*;
use remuda_core::wire;
use serde_json::json;
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
fn read_in_vt_says_when_it_stopped_at_its_history_cap() {
    let s = session("0");
    // One line wrapping over far more rows than the screen and the cap: no newline to count.
    let program = "head -c 240000 /dev/zero | tr '\\0' x; printf END; sleep 100";
    let t = s.json(&["run", "--bg", "sh", "-c", program])["address"].as_str().unwrap().to_string();
    wait_until("printed", || text(&s, &t).ends_with("END"));
    let r = s.json(&["read", "--format", "vt", "--lines", "1100", &t]);
    let rows = r["text"].as_str().unwrap().split('\n').count();
    assert_eq!((rows, r["complete"].as_bool()), (1000 + get(&s, &t)["rows"].as_u64().unwrap() as usize, Some(false)));
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
