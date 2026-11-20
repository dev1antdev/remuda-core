//! When a process was created, as the OS records it: what tells a process from a later one given the same pid.
//! Unix milliseconds, a floor: read as `ps -o lstart` reads it on Unix (its second is that one's; on Linux the boot
//! time now plus the start since boot, so a step of the clock moves it as it moves ps's) and from the creation
//! FILETIME on Windows.

use crate::foreground::Foreground;

/// The facts about `pid` in front of the terminal whose program is `root`, with its creation, read before and after
/// them: a pid that changed hands meanwhile — to a later process, or on Unix to a process of another terminal's
/// session — tells nothing this look, rather than another process's facts as this terminal's.
pub fn about(pid: i32, root: Option<u32>) -> Option<Foreground> {
    let before = of(pid);
    let facts = crate::foreground::about(pid)?;
    let ours = in_session(pid, root);
    let after = of(pid);
    (ours && before == after).then_some(Foreground { started_ms: after, ..facts })
}

/// Whether `pid` is in the session of the terminal whose program is `root`: on Unix a terminal is its program's
/// session's, which every process in front of it is in. Unchecked where there is nothing to ask: on Windows, where the
/// core picks the program from the terminal's own, and while the terminal waits for a program.
#[cfg(unix)]
fn in_session(pid: i32, root: Option<u32>) -> bool {
    let Some(root) = root else { return true };
    let session = unsafe { libc::getsid(pid) };
    session > 0 && session == unsafe { libc::getsid(root as i32) }
}

#[cfg(windows)]
fn in_session(_: i32, _: Option<u32>) -> bool {
    true
}

/// The process's creation in unix ms, if it is still there to read.
#[cfg(target_os = "linux")]
pub fn of(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // after the name in brackets (which may hold spaces and brackets), field 3 on: starttime is field 22
    let ticks: u64 = stat.get(stat.rfind(')')? + 2..)?.split(' ').nth(19)?.parse().ok()?;
    let btime: u64 = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    (hz > 0).then(|| btime * 1000 + ticks * 1000 / hz as u64)
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
        assert_eq!(super::about(pid, Some(pid as u32)).and_then(|f| f.started_ms), Some(started));
    }

    #[cfg(unix)]
    #[test]
    fn a_process_of_another_terminals_session_is_not_in_front_of_this_one() {
        use std::os::unix::process::CommandExt;
        let mut other = std::process::Command::new("sleep");
        other.arg("30");
        unsafe { other.pre_exec(|| if libc::setsid() < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }) };
        let mut other = other.spawn().unwrap();
        let elsewhere = other.id();
        let pid = std::process::id() as i32;
        assert!(super::about(pid, Some(elsewhere)).is_none(), "a pid read as in front of another session's terminal");
        assert!(super::about(elsewhere as i32, Some(elsewhere)).is_some());
        assert!(super::about(pid, None).is_some(), "a terminal waiting for a program checks nothing");
        let _ = other.kill();
        let _ = other.wait();
    }
}
