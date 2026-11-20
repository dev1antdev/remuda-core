//! remuda's own (REMUDA.md): a send that types only while the program it is for leads the terminal, so an
//! agent that left while the send waited for a person's draft never has its message run by the shell behind it.

use crate::holder_proto as proto;
use crate::input::{Sent, Step};
use crate::rpc::RpcError;
use crate::runtime::{Core, State};
use crate::terminals::Terminal;
use std::collections::{HashMap, HashSet};
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

/// Asks terminal `id`'s holder what leads it, the lookup noted to take that holder's answer to this very request,
/// under the caller's lock. None without a link, or, `trusted`, when the holder may answer on a link that did not ask.
pub(crate) fn ask(st: &mut State, id: u32, trusted: bool) -> Option<oneshot::Receiver<i32>> {
    let link = st.link(id)?;
    if trusted && !st.own_replies.contains(&link.instance) {
        return None;
    }
    link.send(&proto::frame(proto::FOREGROUND, id, &[]));
    let holder = link.instance.clone();
    let (tx, rx) = oneshot::channel();
    st.foreground.entry(id).or_default().push((holder, tx));
    Some(rx)
}

/// Notes a holder just linked that answers a lookup only on the link that asked it.
pub(crate) fn linked(own_replies: &mut HashSet<String>, welcome: &crate::holder_proto::Welcome) {
    if welcome.own_replies {
        own_replies.insert(welcome.instance.clone());
    } else {
        own_replies.remove(&welcome.instance);
    }
}

/// Puts a look at the terminal's foreground before the paste and again before its submit, the last step. The send's
/// `if` is checked again after the first look, which waits on the holder: the status may have moved meanwhile.
pub(crate) fn guard(steps: &mut Vec<Step>, pid: i32, only_if: Option<&str>) {
    let submit = steps.len().saturating_sub(1);
    steps.insert(submit, Step::Front { pid, pasted: true, status: None });
    steps.insert(0, Step::Front { pid, pasted: false, status: only_if.map(String::from) });
}

/// Refused at once when the core's last look already saw another program in front, before the send takes the
/// person's draft as ended; the fresh looks of `guard` still decide.
pub(crate) fn last_seen(t: &Terminal, pid: i32) -> Result<(), RpcError> {
    match &t.foreground {
        Some(seen) if seen.pid != pid => Err(moved(t.id(), pid, seen.pid, false)),
        _ => Ok(()),
    }
}

/// A holder of an older build may answer a lookup on a link that did not ask it, after the core retook its link: its
/// terminals take no guarded send.
fn holder_answers_its_own(id: u32, own_replies: Option<bool>) -> Result<(), RpcError> {
    if own_replies == Some(false) {
        let address = crate::terminals::address(id);
        return Err(RpcError::new(
            "needs_update",
            format!("{address} runs in an older rc's holder, whose foreground replies a send cannot rely on"),
            "nothing was typed; a terminal started since rc was updated takes it",
        ));
    }
    Ok(())
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
        if front.is_some() {
            let st = self.lock();
            holder_answers_its_own(id, st.link(id).map(|l| st.own_replies.contains(&l.instance)))?;
        }
        self.send_message(id, text, force, only_if, front, false).await
    }

    /// Ok while process group `pid` leads terminal `id`'s foreground, as its holder answers this very lookup (a
    /// foreground that cannot be read, or a holder that may answer another lookup's, counts as another), and before
    /// the paste while the caller still waits for it and the status is still `status` when the send names one.
    pub(crate) async fn in_front(&self, id: u32, pid: i32, pasted: bool, status: Option<&str>) -> Result<(), RpcError> {
        let asked = ask(&mut self.lock(), id, true);
        let answer = match asked {
            Some(rx) => tokio::time::timeout(std::time::Duration::from_secs(1), rx).await.ok().and_then(Result::ok),
            None => None,
        };
        let now = answer.unwrap_or(-1);
        if now <= 0 || now != pid {
            return Err(moved(id, pid, now, pasted));
        }
        if let Some(want) = status {
            let st = self.lock();
            still(id, st.reg.terminals.get(&id).and_then(|t| t.status.as_ref()).map(|s| s.state.as_str()), want)?;
        }
        if pasted { Ok(()) } else { crate::server::check_cancelled() }
    }
}

/// The send's `if` against the status now, as `send_message` checks it before the look: nothing was typed yet.
fn still(id: u32, word: Option<&str>, want: &str) -> Result<(), RpcError> {
    if word == Some(want) {
        return Ok(());
    }
    let address = crate::terminals::address(id);
    Err(RpcError::new(
        "status_mismatch",
        format!("{address} is {}, not {want}", word.unwrap_or("silent")),
        "nothing was typed",
    ))
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
    fn a_stray_reply_shifts_the_order_so_an_older_holder_takes_no_guarded_send() {
        let mut asked = Asked::new();
        let mut lookup = || {
            let (tx, rx) = oneshot::channel();
            asked.entry(7).or_default().push(("old".to_string(), tx));
            rx
        };
        let (mut watcher, mut send) = (lookup(), lookup());
        answering(&mut asked, 7, "old").unwrap().send(1).unwrap(); // a reply no lookup here asked for
        answering(&mut asked, 7, "old").unwrap().send(11).unwrap(); // the watcher's, sampled earlier
        assert_eq!((watcher.try_recv(), send.try_recv()), (Ok(1), Ok(11)), "the send took another lookup's sample");
        assert_eq!(holder_answers_its_own(7, Some(false)).unwrap_err().kind, "needs_update");
        assert!(holder_answers_its_own(7, Some(true)).is_ok() && holder_answers_its_own(7, None).is_ok());
    }

    #[test]
    fn the_paste_and_its_submit_each_wait_on_a_look() {
        let mut steps =
            vec![Step::Write(b"text".to_vec()), Step::Pause(crate::input::PASTE_GAP), Step::Write(b"\r".to_vec())];
        guard(&mut steps, 42, Some("done"));
        let shape: Vec<String> = steps
            .iter()
            .map(|s| match s {
                Step::Front { pid, pasted, status } => format!("front {pid} {pasted} {status:?}"),
                Step::Write(b) => format!("write {}", String::from_utf8_lossy(b)),
                Step::Pause(_) => "pause".into(),
                _ => "other".into(),
            })
            .collect();
        let looks = ["front 42 false Some(\"done\")", "write text", "pause", "front 42 true None", "write \r"];
        assert_eq!(shape, looks, "only the look before the paste checks the send's if again");
    }

    #[test]
    fn a_status_that_moved_during_the_look_before_the_paste_types_nothing() {
        assert!(still(7, Some("done"), "done").is_ok());
        for now in [Some("working"), None] {
            let refusal = still(7, now, "done").unwrap_err();
            assert_eq!((refusal.kind.as_str(), refusal.hint.as_str()), ("status_mismatch", "nothing was typed"));
        }
    }
}
