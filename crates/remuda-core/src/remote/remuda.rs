//! remuda's own changes (REMUDA.md) to viewers relayed to other cores.

use super::*;

/// A viewer that types without setting the size, on a machine whose core would set it anyway.
pub(super) fn keep_size_refused(line: &Line, m: &Machine, params: &Value) -> Option<RpcError> {
    let asked = params["keep_size"] == true && params["observe"] != true;
    (asked && !line.features.iter().any(|f| f == "keep_size")).then(|| {
        RpcError::new(
            "needs_update",
            format!("{}'s rc is older and can't keep the size for a viewer that types", m.name),
            format!("rc machine update {}", m.name),
        )
    })
}

/// The CLOSE reason of a relayed stream a core on its path is too old for.
pub(super) fn too_old(e: &RpcError) -> Vec<u8> {
    json!({"error": e.to_json()}).to_string().into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    #[derive(Clone, Copy, PartialEq)]
    enum Path {
        Supported,
        OldHere,
        OldFurther,
    }

    #[tokio::test]
    async fn a_viewer_that_keeps_the_size_goes_only_to_cores_that_can() {
        for path in [Path::Supported, Path::OldHere, Path::OldFurther] {
            let root = crate::test_tmp().join(format!(
                "rc-keep-size-{}-{}",
                std::process::id(),
                crate::platform::random_hex()
            ));
            let paths = Paths {
                socket: root.join("core.sock"),
                holder_socket: root.join("holder.sock"),
                state: root.clone(),
                config: root.join("config.toml"),
            };
            paths.create_dirs().unwrap();
            let core = Core::start(paths, crate::api::CoreInfo { id: "here".into(), name: "here".into() }).unwrap();
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
            let conn = Arc::new(Conn::default());
            conn.with(|out| out.open(7));
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
