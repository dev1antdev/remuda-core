//! remuda's own (REMUDA.md): a send that types only while the program it is for leads the terminal, so an
//! agent that left while the send waited for a person's draft never has its message run by the shell behind it.

use crate::input::{Sent, Step};
use crate::rpc::RpcError;
use crate::runtime::Core;
use crate::terminals::Terminal;
use std::collections::HashMap;
use tokio::sync::oneshot;

/// The lookups waiting on each terminal's foreground, each with the holder asked. A holder answers in the order it
/// was asked, so each reply goes to its own lookup: never one sampled before a later lookup was asked.
pub type Asked = HashMap<u32, Vec<(String, oneshot::Sender<i32>)>>;

/// The lookup a reply from `holder` answers: the oldest it was asked (one given up on still takes its place).
pub(crate) fn answering(asked: &mut Asked, id: u32, holder: &str) -> Option<oneshot::Sender<i32>> {
    let waiting = asked.get_mut(&id)?;
    let at = waiting.iter().position(|(asked_of, _)| asked_of == holder)?;
    let (_, tx) = waiting.remove(at);
    if waiting.is_empty() {
        asked.remove(&id);
    }
    Some(tx)
}

/// A holder's link ended: the replies its lookups wait for went with it, and a link taken back starts afresh.
pub(crate) fn link_ended(asked: &mut Asked, holder: &str) {
    asked.retain(|_, waiting| {
        waiting.retain(|(asked_of, _)| asked_of != holder);
        !waiting.is_empty()
    });
}

/// Puts a look at the terminal's foreground before the paste and again before its submit, the last step.
pub(crate) fn guard(steps: &mut Vec<Step>, pid: i32) {
    let submit = steps.len().saturating_sub(1);
    steps.insert(submit, Step::Front { pid, pasted: true });
    steps.insert(0, Step::Front { pid, pasted: false });
}

/// Refused at once when the core's last look already saw another program in front, before the send takes the
/// person's draft as ended; the fresh looks of `guard` still decide.
pub(crate) fn last_seen(t: &Terminal, pid: i32) -> Result<(), RpcError> {
    match &t.foreground {
        Some(seen) if seen.pid != pid => Err(moved(t.id(), pid, seen.pid, false)),
        _ => Ok(()),
    }
}

/// Before the paste: `status_mismatch`, nothing typed. After it: `not_submitted`, the text left where it went.
fn moved(id: u32, pid: i32, now: i32, pasted: bool) -> RpcError {
    let address = crate::terminals::address(id);
    let message = if now > 0 {
        format!("{address} has {now} in front, not {pid}")
    } else {
        format!("{address}'s foreground could not be read, so {pid} may have left")
    };
    if pasted {
        RpcError::new(
            "not_submitted",
            message,
            "the text may have reached the terminal; rc held back its Enter, not a key a person pressed meanwhile: read the terminal before sending again",
        )
    } else {
        RpcError::new("status_mismatch", message, "nothing was typed")
    }
}

impl Core {
    /// `send`, typing only while process group `front` leads the terminal.
    pub async fn send_for(
        &self,
        id: u32,
        text: &str,
        force: bool,
        only_if: Option<&str>,
        front: Option<i32>,
    ) -> Result<Sent, RpcError> {
        self.send_message(id, text, force, only_if, front, false).await
    }

    /// Ok while process group `pid` leads terminal `id`'s foreground (a foreground that cannot be read counts as
    /// another), and before the paste while the caller still waits for it.
    pub(crate) async fn in_front(&self, id: u32, pid: i32, pasted: bool) -> Result<(), RpcError> {
        let now = self.foreground(id).await;
        if now <= 0 || now != pid {
            return Err(moved(id, pid, now, pasted));
        }
        if pasted { Ok(()) } else { crate::server::check_cancelled() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_reply_answers_the_lookup_it_was_asked_for() {
        let mut asked = Asked::new();
        let mut lookup = |holder: &str| {
            let (tx, rx) = oneshot::channel();
            asked.entry(7).or_default().push((holder.to_string(), tx));
            rx
        };
        let (mut gone, mut watcher, mut send) = (lookup("old"), lookup("h1"), lookup("h1"));
        answering(&mut asked, 7, "h1").unwrap().send(11).unwrap();
        assert_eq!(watcher.try_recv(), Ok(11));
        assert!(send.try_recv().is_err(), "an earlier lookup's reply answered a later one");
        link_ended(&mut asked, "old");
        assert!(gone.try_recv().is_err() && !asked[&7].iter().any(|(holder, _)| holder == "old"));
        answering(&mut asked, 7, "h1").unwrap().send(12).unwrap();
        assert_eq!(send.try_recv(), Ok(12));
        assert!(asked.is_empty());
    }

    #[test]
    fn the_paste_and_its_submit_each_wait_on_a_look() {
        let mut steps =
            vec![Step::Write(b"text".to_vec()), Step::Pause(crate::input::PASTE_GAP), Step::Write(b"\r".to_vec())];
        guard(&mut steps, 42);
        let shape: Vec<String> = steps
            .iter()
            .map(|s| match s {
                Step::Front { pid, pasted } => format!("front {pid} {pasted}"),
                Step::Write(b) => format!("write {}", String::from_utf8_lossy(b)),
                Step::Pause(_) => "pause".into(),
                _ => "other".into(),
            })
            .collect();
        assert_eq!(shape, ["front 42 false", "write text", "pause", "front 42 true", "write \r"]);
    }
}
