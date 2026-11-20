//! remuda's own changes (REMUDA.md) to calls and viewers relayed to other cores.

use super::*;

/// A viewer that types without setting the size, on a machine whose core would set it anyway.
pub(super) fn keep_size_refused(line: &Line, m: &Machine, params: &Value) -> Option<RpcError> {
    let asked = params["keep_size"] == true && params["observe"] != true;
    (asked && !line.features.iter().any(|f| f == "keep_size"))
        .then(|| older(&m.name, true, "can't keep the size for a viewer that types"))
}

/// A call carrying what an older core would ignore, to its harm, towards a core without the feature: a send that
/// names the process it is for (it would type whoever is in front), a run with any `literal` but false (it would
/// start a template named like the program; a newer core checks the value itself). `added`: a machine this core
/// added, which `rc machine update` reaches; else a core that added us (which is never asked to run: `route`).
pub(super) fn refused(line: &Line, name: &str, added: bool, method: &str, params: &Value) -> Option<RpcError> {
    let (feature, cannot) = match method {
        "terminal.send" if !params["foreground"].is_null() => {
            ("send_foreground", "can't check what is in front before it types")
        }
        "terminal.run" if !matches!(params["literal"], Value::Null | Value::Bool(false)) => {
            ("run_literal", "would start a template named like the program")
        }
        _ => return None,
    };
    (!line.features.iter().any(|f| f == feature)).then(|| older(name, added, cannot))
}

fn older(name: &str, added: bool, cannot: &str) -> RpcError {
    let hint = if added { format!("rc machine update {name}") } else { format!("update rc on {name}") };
    RpcError::new("needs_update", format!("{name}'s rc is older and {cannot}"), hint)
}

/// The CLOSE reason of a relayed stream a core on its path is too old for.
pub(super) fn too_old(e: &RpcError) -> Vec<u8> {
    json!({"error": e.to_json()}).to_string().into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    #[test]
    fn a_send_naming_its_foreground_goes_only_to_cores_that_check_it() {
        let (frames, _rx) = mpsc::unbounded_channel();
        let old = Line::new(frames.clone(), &json!({"features": ["keep_size"]}));
        let current = Line::new(frames, &json!({"features": ["send_foreground"]}));
        let send = json!({"terminal": "t1", "text": "hi", "foreground": 42});
        let refusal = refused(&old, "away", true, "terminal.send", &send).unwrap();
        assert_eq!((refusal.kind.as_str(), refusal.hint.as_str()), ("needs_update", "rc machine update away"));
        let from_a_peer = refused(&old, "lead", false, "terminal.send", &send).unwrap();
        assert_eq!(from_a_peer.hint, "update rc on lead");
        assert!(refused(&current, "away", true, "terminal.send", &send).is_none());
        let plain = json!({"terminal": "t1", "text": "hi"});
        assert!(refused(&old, "away", true, "terminal.send", &plain).is_none());
    }

    #[test]
    fn a_literal_run_goes_only_to_cores_that_skip_templates() {
        let (frames, _rx) = mpsc::unbounded_channel();
        let old = Line::new(frames.clone(), &json!({"features": ["send_foreground"]}));
        let current = Line::new(frames, &json!({"features": ["run_literal"]}));
        let run = json!({"argv": ["npm", "run", "dev"], "literal": true});
        let refusal = refused(&old, "away", true, "terminal.run", &run).unwrap();
        assert_eq!((refusal.kind.as_str(), refusal.hint.as_str()), ("needs_update", "rc machine update away"));
        assert!(refused(&current, "away", true, "terminal.run", &run).is_none());
        let plain = json!({"argv": ["npm", "run", "dev"]});
        assert!(refused(&old, "away", true, "terminal.run", &plain).is_none());
        let unread = json!({"argv": ["npm", "run", "dev"], "literal": "true"});
        assert!(
            refused(&old, "away", true, "terminal.run", &unread).is_some(),
            "an older core would start npm's template"
        );
        assert!(refused(&current, "away", true, "terminal.run", &unread).is_none(), "a newer core refuses it itself");
        let not_literal = json!({"argv": ["npm", "run", "dev"], "literal": false});
        assert!(refused(&old, "away", true, "terminal.run", &not_literal).is_none());
    }

    #[tokio::test]
    async fn a_literal_run_is_never_sent_to_a_core_that_added_us() {
        let (core, root) = test_core("rc-literal-peer");
        core.lock().peers.insert("lead-id".into(), Peer { name: "lead".into(), ..Default::default() });
        let run = json!({"argv": ["npm", "run", "dev"], "literal": true, "on": "lead"});
        let refusal = route(&core, "terminal.run", &run).err().expect("a peer is not run on");
        assert_eq!(refusal.kind, "machine_unknown");
        drop(core);
        let _ = std::fs::remove_dir_all(root);
    }

    fn test_core(what: &str) -> (Arc<Core>, std::path::PathBuf) {
        let root = crate::test_tmp().join(format!("{what}-{}-{}", std::process::id(), crate::platform::random_hex()));
        let paths = Paths {
            socket: root.join("core.sock"),
            holder_socket: root.join("holder.sock"),
            state: root.clone(),
            config: root.join("config.toml"),
        };
        paths.create_dirs().unwrap();
        let core = Core::start(paths, crate::api::CoreInfo { id: "here".into(), name: "here".into() }).unwrap();
        (core, root)
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Path {
        Supported,
        OldHere,
        OldFurther,
    }

    #[tokio::test]
    async fn a_viewer_that_keeps_the_size_goes_only_to_cores_that_can() {
        for path in [Path::Supported, Path::OldHere, Path::OldFurther] {
            let (core, root) = test_core("rc-keep-size");
            let machine = Machine {
                name: "away".into(),
                target: "away".into(),
                via: false,
                temporary: false,
                session: None,
                id: "remote".into(),
                host: "away".into(),
                version: "test".into(),
                keeps_running_after_logout: None,
                logout_linger_user: None,
            };
            // A slow viewer: out of credit, with output still queued.
            let conn = Arc::new(Conn::default());
            conn.with(|out| {
                out.open(7);
                out.data(7, &vec![b'x'; wire::WINDOW]);
                while out.next_frame().is_some() {}
                out.data(7, b"kept");
            });
            let (old_frames, _old_rx) = mpsc::unbounded_channel();
            core.lock().relayed.insert(
                (conn_key(&conn), 7),
                Relayed {
                    automatic_input: false,
                    line: Line::new(old_frames, &json!({"features": ["keep_size"]})),
                    there: 9,
                    conn: conn.clone(),
                    machine: machine.id.clone(),
                    params: json!({"terminal": "t1", "keep_size": true}),
                },
            );
            let features = if path == Path::OldHere { json!([]) } else { json!(["keep_size"]) };
            let (frames, mut rx) = mpsc::unbounded_channel();
            let line = Line::new(frames, &json!({"features": features}));
            let task = tokio::spawn(reattach(core.clone(), machine, line.clone()));
            if path == Path::OldHere {
                task.await.unwrap();
                assert!(rx.try_recv().is_err(), "asked a core that would take the size");
            } else {
                let request = rx.recv().await.unwrap();
                let request: Value = serde_json::from_slice(&request[wire::HEADER..]).unwrap();
                assert_eq!(request["params"]["keep_size"], true);
                let answer = match path {
                    Path::Supported => json!({"id": request["id"], "result": {"channel": 10}}),
                    _ => {
                        let older = RpcError::new("needs_update", "far's rc is older", "rc machine update far");
                        json!({"id": request["id"], "error": older.to_json()})
                    }
                };
                line.take_reply(answer);
                task.await.unwrap();
            }
            let relayed = core.lock().relayed.contains_key(&(conn_key(&conn), 7));
            assert_eq!(relayed, path == Path::Supported);
            if path != Path::Supported {
                conn.with(|out| out.credit(7, wire::WINDOW));
                let kept = conn.outbox().next_frame().unwrap();
                assert_eq!((kept[4], &kept[wire::HEADER..]), (wire::DATA, b"kept".as_slice()), "queued output first");
                let close = conn.outbox().next_frame().unwrap();
                assert_eq!(close[4], wire::CLOSE);
                let reason: Value = serde_json::from_slice(&close[wire::HEADER..]).unwrap();
                assert_eq!(reason["error"]["data"]["kind"], "needs_update", "{reason}");
            }
            drop(core);
            let _ = std::fs::remove_dir_all(root);
        }
    }
}
