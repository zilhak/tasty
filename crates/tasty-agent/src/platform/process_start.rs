//! 프로세스 시작 시각. PID 는 프로세스가 끝나면 다른 프로세스에 다시 쓰일 수 있어, 저장한 PID 가
//! 같은 프로세스를 가리키는지 PID 와 시작 시각을 함께 비교해 확인한다.
//!
//! 단위는 플랫폼마다 다르며 같은 플랫폼에서 비교하는 데만 쓴다. Linux 는 부팅 이후 clock tick 이라
//! 재부팅 사이의 비교는 보장하지 않는다.

/// PID 의 프로세스 시작 시각. 프로세스가 없거나 읽을 수 없으면 None.
pub fn start_time(pid: u32) -> Option<u64> {
    imp::start_time(pid)
}

/// 저장한 PID·시작 시각이 지금도 같은 프로세스를 가리키는가. 시작 시각을 저장하지 않은 옛
/// 기록은 PID 생존만 본다.
pub fn is_same_process(pid: u32, started_at: Option<u64>) -> bool {
    match started_at {
        Some(t) => start_time(pid) == Some(t),
        None => super::process_alive::is_alive(pid),
    }
}

#[cfg(target_os = "linux")]
mod imp {
    /// `/proc/<pid>/stat`의 22번째 필드(부팅 이후 clock tick).
    pub(super) fn start_time(pid: u32) -> Option<u64> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        parse_start_time(&stat)
    }

    /// 2번째 필드(comm)는 공백·괄호를 담을 수 있으므로 마지막 `)` 뒤부터 센다.
    pub(super) fn parse_start_time(stat: &str) -> Option<u64> {
        let (_, rest) = stat.rsplit_once(')')?;
        rest.split_whitespace().nth(19)?.parse().ok()
    }
}

#[cfg(target_os = "macos")]
mod imp {
    /// `struct kinfo_proc`의 크기(arm64·x86_64 공통). libc 크레이트가 macOS 용 정의를 주지 않는다.
    const KINFO_PROC_SIZE: usize = 648;

    /// `sysctl(KERN_PROC_PID)`의 `kp_proc.p_starttime`(마이크로초). `proc_pidinfo(PROC_PIDTBSDINFO)`
    /// 와 같은 값이지만, 그것은 회수 전 종료 상태(좀비)와 다른 사용자의 프로세스에서 실패한다.
    pub(super) fn start_time(pid: u32) -> Option<u64> {
        let pid = libc::c_int::try_from(pid).ok()?;
        let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
        // timeval 을 읽을 수 있게 8 바이트로 정렬한 버퍼.
        let mut buf = [0u64; KINFO_PROC_SIZE / 8];
        let mut len = KINFO_PROC_SIZE;
        // SAFETY: buf 는 len 바이트의 쓰기 가능한 버퍼이고 mib 는 길이 4 의 배열이다.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as libc::c_uint,
                buf.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        // 없는 PID 는 성공하면서 0 바이트를 돌려준다.
        if rc != 0 || len != KINFO_PROC_SIZE {
            return None;
        }
        // SAFETY: kinfo_proc 의 맨 앞(kp_proc.p_un.p_starttime)이 struct timeval 이고 buf 는 그 정렬을
        // 만족한다.
        let start = unsafe { buf.as_ptr().cast::<libc::timeval>().read() };
        let sec = u64::try_from(start.tv_sec).ok()?;
        let usec = u64::try_from(start.tv_usec).ok()?;
        Some(sec * 1_000_000 + usec)
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// `GetProcessTimes`의 생성 시각(100ns FILETIME).
    pub(super) fn start_time(pid: u32) -> Option<u64> {
        // SAFETY: OpenProcess 는 실패하면 NULL 을 돌려준다. 그 경우 바로 None 이다.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: handle 은 위에서 연 유효한 핸들이고 출력 버퍼는 지역 변수다.
        let ok =
            unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
        // SAFETY: 위 OpenProcess 의 NULL 이 아닌 핸들이다.
        unsafe {
            CloseHandle(handle);
        }
        (ok != 0)
            .then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod imp {
    pub(super) fn start_time(_pid: u32) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_has_a_start_time_and_is_itself() {
        let pid = std::process::id();
        let t = start_time(pid).expect("own start time");
        assert!(is_same_process(pid, Some(t)));
        assert!(!is_same_process(pid, Some(t.wrapping_add(1))));
    }

    /// 회수 전 종료 상태(좀비)도 같은 프로세스로 보고, 회수한 뒤에는 없다고 본다.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_macos_start_time_holds_until_the_process_is_reaped() {
        let mut child = std::process::Command::new("sleep")
            .arg("0.2")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        let t = start_time(pid).expect("start time of the running child");
        std::thread::sleep(std::time::Duration::from_millis(600));
        assert_eq!(
            start_time(pid),
            Some(t),
            "zombie child keeps its start time"
        );
        child.wait().expect("reap");
        assert_eq!(start_time(pid), None);
    }

    /// 다른 사용자(root)의 프로세스도 읽는다.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_macos_start_time_reads_another_users_process() {
        assert!(start_time(1).is_some(), "launchd start time");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_linux_start_time_skips_a_command_name_with_spaces_and_parens() {
        let mut fields = vec!["S".to_string()];
        fields.extend((4..=21).map(|n| n.to_string()));
        fields.push("987654".into());
        fields.push("0".into());
        let stat = format!("42 (we (ird) name) {}", fields.join(" "));
        assert_eq!(imp::parse_start_time(&stat), Some(987654));
    }
}
