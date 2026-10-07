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
    /// `proc_pidinfo(PROC_PIDTBSDINFO)`의 시작 시각(마이크로초).
    pub(super) fn start_time(pid: u32) -> Option<u64> {
        // SAFETY: proc_bsdinfo 는 정수 필드만 가진 POD 라 0 으로 채운 값이 유효하다.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: info는 proc_bsdinfo 크기의 쓰기 가능한 버퍼이고 size가 그 크기다.
        let written = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        (written == size).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
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
