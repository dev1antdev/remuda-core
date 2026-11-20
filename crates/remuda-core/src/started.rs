//! When a process was created, as the OS records it: what tells a process from a later one given the same pid.
//! Unix milliseconds, a floor: on macOS the start `ps -o lstart` prints, on Linux the start since boot (to the kernel's
//! tick) after the boot time the clocks tell, so its second is ps's or the next (ps counts the boot time in whole
//! seconds) and a step of the clock moves it as it moves ps's, and on Windows the creation FILETIME.

use crate::foreground::Foreground;
use crate::runtime::Core;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

impl Core {
    /// What is in front of terminal `id`, with the terminal's program it was read under (its pid and which of the
    /// core's spawns it is, which a replacement given the same pid does not share), which the caller keeps the look for (a
    /// program gone or replaced since, the look is of a terminal that is no more): the facts and the
    /// creation of the process group its holder names, read between two of its answers that name the same group, the
    /// creation read before and after the facts, and on Unix only a process of the terminal's own session (its
    /// program's: every process in front of it is in it). A process that left meanwhile, and with it a pid given to a
    /// process of another terminal, tells nothing this look, though the terminal still names the group it led.
    pub(crate) async fn foreground_now(&self, id: u32) -> (Option<Foreground>, Program) {
        let program = self.lock().reg.terminals.get(&id).map(Program::of).unwrap_or_default();
        let pgid = self.foreground(id).await;
        if pgid <= 0 {
            return (None, program);
        }
        let before = (of(pgid), since_boot(pgid));
        let Some(facts) = crate::foreground::about(pgid) else { return (None, program) };
        let ours = in_session(pgid, program.pid);
        let after = (of(pgid), since_boot(pgid));
        let same = before == after && ours && self.foreground(id).await == pgid;
        (same.then_some(Foreground { started_ms: after.0, started_since_boot: after.1, ..facts }), program)
    }
}

/// A terminal's program: its pid and which of the core's spawns started it (none for one the core found running).
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(crate) struct Program {
    pid: Option<u32>,
    spawn: Option<u64>,
}

impl Program {
    pub(crate) fn of(t: &crate::terminals::Terminal) -> Program {
        Program::at(t.id(), t.pid)
    }

    fn at(id: u32, pid: Option<u32>) -> Program {
        let spawn = SPAWNS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&id).copied();
        Program { pid, spawn }
    }
}

/// The spawn that started each terminal's program, counted across the core's life and never back, unlike a clock.
static SPAWNS: std::sync::Mutex<std::collections::BTreeMap<u32, u64>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Forgets terminal `id`'s spawn as the core removes it: an id is never given out again.
pub(crate) fn forget(id: u32) {
    SPAWNS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&id);
}

/// Notes that the core started a program for terminal `id`.
pub(crate) fn spawned(id: u32) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let spawn = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    SPAWNS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(id, spawn);
}

/// Whether `pid` is in the session `session` leads (a terminal's program, which the holder makes its session's
/// leader); unchecked while the terminal waits for a program, and on Windows, which has no sessions to ask.
#[cfg(unix)]
fn in_session(pid: i32, session: Option<u32>) -> bool {
    session.is_none_or(|leader| unsafe { libc::getsid(pid) } == leader as i32)
}

#[cfg(windows)]
fn in_session(_: i32, _: Option<u32>) -> bool {
    true
}

/// A process's start as the boot it began in counts it, which no step of the wall clock moves: that boot (Linux's
/// `/proc/sys/kernel/random/boot_id`) and the clock ticks since it (`/proc/<pid>/stat`'s starttime).
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct SinceBoot {
    pub boot_id: String,
    pub ticks: u64,
}

/// The process's start since boot, on Linux, if it is still there to read; elsewhere `started_ms` is the creation
/// the OS stored and no step moves it, and this tells nothing.
#[cfg(target_os = "linux")]
pub fn since_boot(pid: i32) -> Option<SinceBoot> {
    Some(SinceBoot { boot_id: crate::platform::boot_id().ok().flatten()?, ticks: start_ticks(pid)? })
}

#[cfg(not(target_os = "linux"))]
pub fn since_boot(_: i32) -> Option<SinceBoot> {
    None
}

/// `/proc/<pid>/stat`'s starttime: the clock ticks from the boot to the process's start.
#[cfg(target_os = "linux")]
fn start_ticks(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // after the name in brackets (which may hold spaces and brackets), field 3 on: starttime is field 22
    stat.get(stat.rfind(')')? + 2..)?.split(' ').nth(19)?.parse().ok()
}

/// The process's creation in unix ms, if it is still there to read.
#[cfg(target_os = "linux")]
pub fn of(pid: i32) -> Option<u64> {
    let ticks = start_ticks(pid)?;
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if hz <= 0 {
        return None;
    }
    Some(boot_ms()? + ticks * 1000 / hz as u64)
}

/// When this machine booted, unix ms: `/proc/stat`'s whole second, as ps counts it, with the fraction the real clock
/// less the time since boot tells, read between two looks at the latter no more than a millisecond apart (else what
/// was kept, or none).
#[cfg(target_os = "linux")]
fn boot_ms() -> Option<u64> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static KEPT: AtomicU64 = AtomicU64::new(0);
    let whole: u64 = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    let clock = |id| {
        let mut t: libc::timespec = unsafe { std::mem::zeroed() };
        (unsafe { libc::clock_gettime(id, &mut t) } == 0).then(|| t.tv_sec as i128 * 1_000_000_000 + t.tv_nsec as i128)
    };
    let look = || Some((clock(libc::CLOCK_BOOTTIME)?, clock(libc::CLOCK_REALTIME)?, clock(libc::CLOCK_BOOTTIME)?));
    let kept = KEPT.load(Ordering::Relaxed);
    let Some(measured) = bracketed((0..3).map_while(|_| look())) else {
        return (kept != 0 && kept / 1000 == whole).then_some(kept);
    };
    let boot = settle(kept, whole, u64::try_from(measured / 1_000_000).ok()?);
    KEPT.store(boot, Ordering::Relaxed);
    Some(boot)
}

/// The boot time in ns of the first look (since boot, real, since boot again) whose two looks since boot are no
/// more than a millisecond apart: the real clock was read within it. None when every look was held up longer.
#[cfg(any(target_os = "linux", test))]
fn bracketed(looks: impl Iterator<Item = (i128, i128, i128)>) -> Option<i128> {
    looks
        .filter(|(first, _, last)| last - first <= 1_000_000)
        .map(|(first, real, last)| real - (first + last) / 2)
        .next()
}

/// The boot time to tell, given the one kept, ps's whole second and a fresh measure: always in ps's second, so a
/// creation's second is ps's or the next; the kept one while it is in that second and within `STEP_MS` of the
/// measure, so a reading's jitter moves no creation, and the measure when ps's second moved or the clock stepped.
#[cfg(any(target_os = "linux", test))]
fn settle(kept: u64, whole: u64, measured: u64) -> u64 {
    const STEP_MS: u64 = 5;
    let measured = measured.clamp(whole * 1000, whole * 1000 + 999);
    if kept / 1000 == whole && kept.abs_diff(measured) <= STEP_MS { kept } else { measured }
}

#[cfg(target_os = "macos")]
pub fn of(pid: i32) -> Option<u64> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    if unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) } != size {
        return None;
    }
    Some(info.pbi_start_tvsec * 1000 + info.pbi_start_tvusec / 1000)
}

#[cfg(windows)]
pub fn of(pid: i32) -> Option<u64> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{FILETIME, HANDLE};
    use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32) };
    if raw.is_null() {
        return None;
    }
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    let read = unsafe {
        GetProcessTimes(process.as_raw_handle() as HANDLE, &mut created, &mut exited, &mut kernel, &mut user)
    };
    if read == 0 {
        return None;
    }
    let filetime = (created.dwHighDateTime as u64) << 32 | created.dwLowDateTime as u64;
    filetime.checked_sub(116_444_736_000_000_000).map(|since_1970| since_1970 / 10_000)
}

#[cfg(test)]
mod tests {
    #[test]
    fn this_process_began_before_now_and_reads_the_same_each_time() {
        let pid = std::process::id() as i32;
        let started = super::of(pid).expect("this process's creation");
        let now = crate::runtime::now_ms();
        assert!(started <= now && started > now - 3_600_000, "started {started}, now {now}");
        assert_eq!(super::of(pid), Some(started));
        assert_eq!(super::of(i32::MAX), None);
    }

    #[test]
    fn a_start_since_boot_is_told_on_linux_alone_and_reads_the_same_each_time() {
        let pid = std::process::id() as i32;
        let since = super::since_boot(pid);
        assert_eq!(since.is_some(), cfg!(target_os = "linux"), "{since:?}");
        if let Some(since) = &since {
            assert_eq!(since.boot_id.len(), 36, "{since:?}");
            assert_eq!(super::since_boot(pid).as_ref(), Some(since));
        }
        assert_eq!(super::since_boot(i32::MAX), None);
    }

    #[test]
    fn the_boot_time_keeps_ps_whole_second_and_moves_only_with_it_or_a_step() {
        let settle = super::settle;
        assert_eq!(settle(0, 1000, 1_000_800), 1_000_800, "first reading");
        assert_eq!(settle(1_000_800, 1000, 1_000_801), 1_000_800, "a reading's jitter moves nothing");
        assert_eq!(settle(1_000_990, 1001, 1_001_020), 1_001_020, "a small correction across ps's second");
        assert_eq!(settle(1_000_800, 1000, 1_000_840), 1_000_840, "a step past the jitter");
        assert_eq!(settle(1_000_800, 1000, 999_999), 1_000_000, "a measure outside ps's second is held to it");
        assert_eq!(settle(1_000_800, 1000, 1_001_003), 1_000_999);
    }

    #[test]
    fn a_boot_time_measured_while_held_up_is_never_taken() {
        let ms = 1_000_000;
        let (boot, since) = (1_000_800 * ms, 10_000 * ms);
        let held_up = (since, boot + since + 40 * ms, since + 40 * ms);
        let prompt = (since + 50 * ms, boot + since + 50 * ms + ms / 2, since + 51 * ms);
        assert_eq!(super::bracketed([held_up, prompt].into_iter()), Some(boot));
        assert_eq!(super::bracketed([held_up, held_up, held_up].into_iter()), None);
    }

    #[test]
    fn a_program_started_again_under_its_pid_is_another_program() {
        let id = 4_000_000_000;
        super::spawned(id);
        let before = super::Program::at(id, Some(7));
        assert_eq!(super::Program::at(id, Some(7)), before);
        assert_ne!(super::Program::at(id, None), before, "lost");
        super::spawned(id);
        assert_ne!(super::Program::at(id, Some(7)), before, "the same pid started again, whatever the clock says");
        super::forget(id);
        assert_eq!(
            super::Program::at(id, Some(7)),
            super::Program { pid: Some(7), spawn: None },
            "a removed one is kept no more"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_process_of_another_session_is_not_in_front_of_this_ones_terminal() {
        use std::os::unix::process::CommandExt;
        let mut other = std::process::Command::new("sleep");
        other.arg("30");
        unsafe { other.pre_exec(|| if libc::setsid() < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }) };
        let mut other = other.spawn().unwrap();
        let leader = other.id();
        let own = unsafe { libc::getsid(0) } as u32;
        assert!(super::in_session(leader as i32, Some(leader)));
        assert!(!super::in_session(std::process::id() as i32, Some(leader)), "a process of another session taken in");
        assert!(super::in_session(std::process::id() as i32, Some(own)));
        assert!(super::in_session(std::process::id() as i32, None), "a terminal waiting for a program checks nothing");
        let _ = other.kill();
        let _ = other.wait();
    }
}
