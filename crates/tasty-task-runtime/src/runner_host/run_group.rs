//! Run 자식 프로세스 묶음. 취소하면 직접 자식뿐 아니라 그 자식이 만든 프로세스까지 끝낸다.
//!
//! 묶는 규칙은 후처리 실행과 같다. Unix 는 자식을 새 프로세스 그룹의 리더로 만들어 그룹 전체에
//! SIGKILL 을 보내고, Windows 는 자식을 job 에 넣어 job 을 끝낸다. 다른 점은 Run 이 호스트가
//! 다시 시작해도 계속 실행된다는 것이다. 그래서 호스트 수명에 묶지 않으며(Linux PDEATHSIG 없음)
//! Windows job 은 닫혀도 안의 프로세스를 끝내지 않는다.
//!
//! Run 의 끝과 취소 때 끝내는 범위는 다르다. 같은 호스트에서 시작한 Run 은 리더가 끝나고 두 출력
//! 파이프가 EOF 일 때 끝난다(watcher). 출력을 닫고 남은 그룹 구성원(데몬)은 Run 이 아니다.
//! 취소는 Run 이 끝나기 전이면 리더가 이미 끝났어도 그룹 전체를 끝내고, 그룹이 빈 것을 확인한 뒤에
//! 점유를 놓는다([`has_ended`]).
//!
//! 끝낼 대상은 저장한 PID 와 시작 시각으로 같은 프로세스임을 확인한 뒤에만 정한다. 그룹 id 는
//! 리더의 PID 이고, 리더나 그룹 구성원이 하나라도 남아 있는 동안 그 id 는 새 프로세스에 쓰이지
//! 않는다. 리더가 끝난 뒤에는 이 호스트의 watcher 가 아직 출력을 기다리는 Run([`adopt`] 부터
//! [`forget`] 까지)에만 그룹 신호를 보낸다. 출력을 쥔 프로세스가 그룹을 떠나 그룹이 비고 그 id 가
//! 다시 쓰이는 경우(PID 가 한 바퀴 돈 뒤)는 막지 않는다. 확인과 신호 사이에 리더가 끝나고 PID 가
//! 다시 쓰이는 짧은 틈도 막지 않는다.
//!
//! 재시작 뒤 넘겨받은 Run 은 출력을 볼 수 없어 리더의 끝을 Run 의 끝으로 본다. 리더가 살아 있을 때
//! 취소하면 그룹 전체를 끝내고 그룹이 빈 것을 확인한다. Windows 는 그룹이 없어 job 으로 묶는다.
//! 호스트가 다시 시작하면 job 을 다시 열 수 없어 직접 자식만 끝내고 리더의 종료만 확인한다
//! (손자는 남을 수 있다).

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use std::process::Command;

use tasty_agent::platform::process_start;

/// 자식을 새 프로세스 그룹의 리더로 실행하게 한다(Unix).
pub(crate) fn configure(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    let _unused = cmd;
}

static WATCHED_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 이 호스트가 시작했고 watcher 가 아직 끝(리더 종료와 출력 EOF)을 보지 못한 Run. 러너가 멈춰도
/// watcher 는 계속 기다리므로 러너 밖(러너가 꺼진 동안의 취소)에서도 본다.
fn with_watched<R>(f: impl FnOnce(&mut HashSet<(u32, u64)>) -> R) -> R {
    static WATCHED: OnceLock<Mutex<HashSet<(u32, u64)>>> = OnceLock::new();
    let mut guard = tasty_utils::poison::recover_mutex(
        WATCHED.get_or_init(Default::default).lock(),
        "agent run watched groups",
        &WATCHED_POISON_REPORTED,
    );
    f(&mut guard)
}

/// 시작한 자식을 묶음에 넣고(Windows job) watcher 가 끝을 볼 때까지 이 호스트의 Run 으로 둔다.
/// 시작 시각을 돌려준다.
pub(crate) fn adopt(pid: u32) -> Option<u64> {
    #[cfg(windows)]
    jobs::adopt(pid);
    let started_at = process_start::start_time(pid);
    if let Some(t) = started_at {
        with_watched(|w| w.insert((pid, t)));
    }
    started_at
}

/// Run 이 끝나(리더 종료와 출력 EOF) 묶음을 더 쓰지 않는다. Windows job 을 닫지만 남은 프로세스는
/// 끝내지 않는다.
pub(crate) fn forget(pid: u32) {
    #[cfg(windows)]
    jobs::forget(pid);
    with_watched(|w| w.retain(|(p, _)| *p != pid));
}

/// 저장한 PID·시작 시각의 프로세스가 아직 살아 있는가. 회수 전 종료 상태(좀비)도 살아 있다고 본다.
pub(crate) fn is_running(pid: u32, started_at: Option<u64>) -> bool {
    process_start::is_same_process(pid, started_at)
}

/// 같은 프로세스가 살아 있거나, 리더는 끝났지만 이 호스트의 watcher 가 아직 출력을 기다리는
/// Run 이면 묶음 전체를 끝낸다. 신호를 보냈으면 `true`.
///
/// 시작 시각이 없는 기록(시작 시각을 남기기 전의 handle)은 같은 프로세스인지 확인할 수 없어
/// 신호를 보내지 않는다. 다른 프로세스를 끝내는 것보다 남겨 두는 편이 안전하다.
pub(crate) fn terminate(pid: u32, started_at: Option<u64>) -> bool {
    let Some(t) = started_at else {
        return false;
    };
    if is_running(pid, started_at) {
        imp::terminate(pid);
        return true;
    }
    with_watched(|w| w.contains(&(pid, t))) && imp::terminate_group(pid)
}

/// [`terminate`] 로 끝낸 묶음이 모두 끝났는가. 리더가 끝나고 그룹에 남은 구성원이 없을 때다.
pub(crate) fn has_ended(pid: u32, started_at: Option<u64>) -> bool {
    !is_running(pid, started_at) && !imp::group_alive(pid)
}

#[cfg(unix)]
mod imp {
    fn pgid(pid: u32) -> Option<libc::pid_t> {
        libc::pid_t::try_from(pid).ok().filter(|p| *p > 0)
    }

    /// 그룹에 구성원(좀비 포함)이 남아 있는가.
    pub(super) fn group_alive(pid: u32) -> bool {
        let Some(pgid) = pgid(pid) else {
            return false;
        };
        // SAFETY: 신호 0 은 보내지 않고 대상만 확인한다.
        if unsafe { libc::kill(-pgid, 0) } == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    /// 리더가 끝난 그룹에 구성원이 남아 있으면 모두 끝낸다. 보냈으면 `true`.
    pub(super) fn terminate_group(pid: u32) -> bool {
        let Some(pgid) = pgid(pid) else {
            return false;
        };
        // SAFETY: kill(2) 은 메모리를 건드리지 않는다. 음수 pid 는 그 그룹의 모든 프로세스다.
        if unsafe { libc::kill(-pgid, libc::SIGKILL) } == 0 {
            return true;
        }
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::ESRCH) {
            tracing::warn!("run kill group {pgid}: {e}");
        }
        false
    }

    pub(super) fn terminate(pid: u32) {
        let Ok(pgid) = libc::pid_t::try_from(pid) else {
            return;
        };
        if pgid <= 0 {
            return;
        }
        // SAFETY: kill(2) 은 메모리를 건드리지 않는다. 음수 pid 는 그 그룹의 모든 프로세스다.
        let rc = unsafe { libc::kill(-pgid, libc::SIGKILL) };
        if rc == 0 {
            return;
        }
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::ESRCH) {
            tracing::warn!("run kill group {pgid}: {e}");
            return;
        }
        // 그룹 리더가 아니다(그룹을 만들기 전에 시작한 Run). 직접 자식만 끝낸다.
        // SAFETY: 위와 같다. 양수 pid 는 그 프로세스 하나다.
        if unsafe { libc::kill(pgid, libc::SIGKILL) } != 0 {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::ESRCH) {
                tracing::warn!("run kill pid {pgid}: {e}");
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};

    /// job 의 남은 구성원은 확인하지 않는다(job 종료가 구성원을 함께 끝낸다).
    pub(super) fn group_alive(_pid: u32) -> bool {
        false
    }

    /// 리더가 끝난 Run 의 job 이 남아 있으면 끝낸다.
    pub(super) fn terminate_group(pid: u32) -> bool {
        super::jobs::terminate(pid)
    }

    pub(super) fn terminate(pid: u32) {
        if super::jobs::terminate(pid) {
            return;
        }
        // SAFETY: OpenProcess 는 실패하면 NULL 을 돌려준다. 그 경우 끝낼 프로세스가 없다.
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
        if handle.is_null() {
            return;
        }
        // SAFETY: handle 은 위에서 PROCESS_TERMINATE 로 연 유효한 핸들이다.
        if unsafe { TerminateProcess(handle, 1) } == 0 {
            tracing::warn!(
                "run terminate pid {pid}: {}",
                std::io::Error::last_os_error()
            );
        }
        // SAFETY: 위 OpenProcess 의 NULL 이 아닌 핸들이다.
        unsafe {
            CloseHandle(handle);
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub(super) fn group_alive(_pid: u32) -> bool {
        false
    }

    pub(super) fn terminate_group(_pid: u32) -> bool {
        false
    }

    pub(super) fn terminate(_pid: u32) {}
}

/// 이 호스트가 시작한 Run 의 job. 러너가 멈췄다 다시 켜져도 같은 호스트 안에서는 남는다.
#[cfg(windows)]
mod jobs {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    use tasty_reaper::JobObject;

    static POISON_REPORTED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    fn with<R>(f: impl FnOnce(&mut HashMap<u32, JobObject>) -> R) -> R {
        static JOBS: OnceLock<Mutex<HashMap<u32, JobObject>>> = OnceLock::new();
        let mut guard = tasty_utils::poison::recover_mutex(
            JOBS.get_or_init(Default::default).lock(),
            "agent run jobs",
            &POISON_REPORTED,
        );
        f(&mut guard)
    }

    pub(super) fn adopt(pid: u32) {
        let job = match JobObject::new_detached() {
            Ok(job) => job,
            Err(e) => {
                tracing::warn!("run job create for pid {pid}: {e}");
                return;
            }
        };
        if let Err(e) = job.assign_pid(pid) {
            tracing::warn!("run job assign pid {pid}: {e}");
            return;
        }
        with(|jobs| jobs.insert(pid, job));
    }

    pub(super) fn forget(pid: u32) {
        with(|jobs| jobs.remove(&pid));
    }

    /// job 이 있으면 끝내고 `true`.
    pub(super) fn terminate(pid: u32) -> bool {
        with(|jobs| match jobs.get(&pid) {
            Some(job) => {
                if let Err(e) = job.terminate() {
                    tracing::warn!("run job terminate pid {pid}: {e}");
                }
                true
            }
            None => false,
        })
    }
}
