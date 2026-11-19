#![cfg(unix)] // remuda's own changes (REMUDA.md); Windows: tests/windows.rs

mod common;

use common::framed::Viewer;
use common::*;
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
fn keys_now_skip_the_pause_and_the_keys_waiting_for_it() {
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
    std::thread::sleep(Duration::from_millis(300));
    let pressed = Instant::now();
    s.connect().call(1, "terminal.keys", json!({"terminal": t, "keys": ["a"], "now": true}));
    person.shows("61");
    assert!(pressed.elapsed() < Duration::from_millis(100), "arrived after {:?}", pressed.elapsed());
    assert!(!person.text().contains("62"), "the key waiting for quiet went first");
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
