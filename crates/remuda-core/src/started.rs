//! When a process was created, as the OS records it: what tells a process from a later one given the same pid.
//! Unix milliseconds, a floor: on macOS the start `ps -o lstart` prints, on Linux the start since boot (to the kernel's
//! tick) after the boot time the clocks tell, so its second is ps's or the next (ps counts the boot time in whole
//! seconds) and a step of the clock moves it as it moves ps's, and on Windows the creation FILETIME.

use crate::foreground::Foreground;
use crate::runtime::Core;

impl Core {
    /// What is in front of terminal `id`: the facts and the creation of the process group its holder names, read
    /// between two of its answers that name the same group, the creation read before and after the facts. A process
    /// that left meanwhile, and with it a pid given to a process of another terminal, tells nothing this look.
    pub(crate) async fn foreground_now(&self, id: u32) -> Option<Foreground> {
        let pgid = self.foreground(id).await;
        if pgid <= 0 {
            return None;
        }
        let facts = about(pgid)?;
        (self.foreground(id).await == pgid).then_some(facts)
    }
}

/// The facts about `pid` with its creation, read before and after them: a pid that changed hands meanwhile tells
/// nothing, rather than one process's name with another's start.
pub fn about(pid: i32) -> Option<Foreground> {
    let before = of(pid);
    let facts = crate::foreground::about(pid)?;
    let after = of(pid);
    (before == after).then_some(Foreground { started_ms: after, ..facts })
}

/// The process's creation in unix ms, if it is still there to read.
#[cfg(target_os = "linux")]
pub fn of(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // after the name in brackets (which may hold spaces and brackets), field 3 on: starttime is field 22
    let ticks: u64 = stat.get(stat.rfind(')')? + 2..)?.split(' ').nth(19)?.parse().ok()?;
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if hz <= 0 {
        return None;
    }
    Some(boot_ms()? + ticks * 1000 / hz as u64)
}

/// When this machine booted, unix ms: the real clock less the time since boot, which `/proc/stat` gives in whole
/// seconds alone. Kept until the real clock steps (by over `STEP_MS`), so a reading's jitter moves no creation.
#[cfg(target_os = "linux")]
fn boot_ms() -> Option<u64> {
    use std::sync::atomic::{AtomicU64, Ordering};
    const STEP_MS: u64 = 50;
    static KEPT: AtomicU64 = AtomicU64::new(0);
    let clock = |id| {
        let mut t: libc::timespec = unsafe { std::mem::zeroed() };
        (unsafe { libc::clock_gettime(id, &mut t) } == 0)
            .then(|| t.tv_sec as i128 * 1000 + t.tv_nsec as i128 / 1_000_000)
    };
    let now = u64::try_from(clock(libc::CLOCK_REALTIME)? - clock(libc::CLOCK_BOOTTIME)?).ok()?;
    let kept = KEPT.load(Ordering::Relaxed);
    if kept != 0 && now.abs_diff(kept) <= STEP_MS {
        return Some(kept);
    }
    KEPT.store(now, Ordering::Relaxed);
    Some(now)
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
        assert_eq!(super::about(pid).and_then(|f| f.started_ms), Some(started));
    }
}
