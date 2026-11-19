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
