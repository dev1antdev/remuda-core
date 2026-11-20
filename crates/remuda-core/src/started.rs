//! When a process was created, as the OS records it: what tells a process from a later one given the same pid.
//! Unix milliseconds, read as `ps -o lstart` reads it on Unix (its second is that one's) and from the creation
//! FILETIME on Windows.

/// The process's creation in unix ms, if it is still there to read.
#[cfg(target_os = "linux")]
pub fn of(pid: i32) -> Option<i64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // after the name in brackets (which may hold spaces and brackets), field 3 on: starttime is field 22
    let ticks: i64 = stat.get(stat.rfind(')')? + 2..)?.split(' ').nth(19)?.parse().ok()?;
    let btime: i64 = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    (hz > 0).then(|| btime * 1000 + ticks * 1000 / hz as i64)
}

#[cfg(target_os = "macos")]
pub fn of(pid: i32) -> Option<i64> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    if unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) } != size {
        return None;
    }
    Some(info.pbi_start_tvsec as i64 * 1000 + info.pbi_start_tvusec as i64 / 1000)
}

#[cfg(windows)]
pub fn of(pid: i32) -> Option<i64> {
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
    let filetime = (created.dwHighDateTime as i64) << 32 | created.dwLowDateTime as i64;
    Some((filetime - 116_444_736_000_000_000) / 10_000)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn this_process_began_before_now_and_reads_the_same_each_time() {
        let pid = std::process::id() as i32;
        let started = super::of(pid).expect("this process's creation");
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64;
        assert!(started <= now && started > now - 3_600_000, "started {started}, now {now}");
        assert_eq!(super::of(pid), Some(started));
        assert_eq!(super::of(i32::MAX), None);
    }
}
