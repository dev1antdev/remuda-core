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
    let program = "head -c 240000 /dev/zero | tr '\\0' x; sleep 100";
    let t = s.json(&["run", "--bg", "sh", "-c", program])["address"].as_str().unwrap().to_string();
    wait_until("printed", || text(&s, &t).ends_with('x'));
    let r = s.json(&["read", "--format", "vt", "--lines", "1100", &t]);
    let rows = r["text"].as_str().unwrap().split('\n').count();
    assert_eq!((rows, r["complete"].as_bool()), (1000 + get(&s, &t)["rows"].as_u64().unwrap() as usize, Some(false)));
}
