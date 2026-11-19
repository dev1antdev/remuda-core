//! remuda's own changes (REMUDA.md) to viewers relayed to other cores.

use super::*;

/// A viewer that types without setting the size, on a machine whose core would set it anyway.
pub(super) fn keep_size_refused(line: &Line, m: &Machine, params: &Value) -> Option<RpcError> {
    (params["keep_size"] == true && !line.features.iter().any(|f| f == "keep_size")).then(|| {
        RpcError::new(
            "needs_update",
            format!("{}'s rc is older and can't keep the size for a viewer that types", m.name),
            format!("rc machine update {}", m.name),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    #[tokio::test]
    async fn a_viewer_that_keeps_the_size_goes_only_to_a_core_that_can() {
        for feature in [false, true] {
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
            let (frames, mut rx) = mpsc::unbounded_channel();
            let line = Line::new(frames, &json!({"features": if feature { vec!["keep_size"] } else { vec![] }}));
            let task = tokio::spawn(reattach(core.clone(), machine, line.clone()));
            if feature {
                let request = rx.recv().await.unwrap();
                let request: Value = serde_json::from_slice(&request[wire::HEADER..]).unwrap();
                assert_eq!(request["params"]["keep_size"], true);
                line.take_reply(json!({"id": request["id"], "result": {"channel": 10}}));
                task.await.unwrap();
                assert!(core.lock().relayed.contains_key(&(conn_key(&conn), 7)));
            } else {
                task.await.unwrap();
                assert!(rx.try_recv().is_err(), "asked a core that would take the size");
                assert!(!core.lock().relayed.contains_key(&(conn_key(&conn), 7)));
                assert_eq!(conn.outbox().next_frame().unwrap()[4], wire::CLOSE);
            }
            drop(core);
            let _ = std::fs::remove_dir_all(root);
        }
    }
}
