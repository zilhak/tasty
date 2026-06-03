//! Cross-platform pid liveness probe — Unix `kill(pid, 0)` / Windows
//! `OpenProcess + GetExitCodeProcess == STILL_ACTIVE`.
//!
//! 호스트 재시작 후 Running 잔여 task 의 자식 프로세스 부활 시나리오에서 사용
//! 예정 (Phase H.F 본문에서는 호출 X — helper 만 준비).

#[cfg(unix)]
pub fn is_alive(pid: u32) -> bool {
    // SAFETY: libc::kill(pid, 0) 는 신호를 보내지 않고 권한/생존 여부만 검사 — 부수효과 없음.
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if r == 0 {
        return true;
    }
    // ESRCH (No such process) → false. EPERM (권한 없음, 그러나 프로세스 존재) → true.
    // SAFETY: errno 는 thread-local 변수 — 다른 동시 호출이 덮어쓰기 전 즉시 읽음.
    let errno = unsafe { *libc::__error() };
    errno == libc::EPERM
}

#[cfg(windows)]
pub fn is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: OpenProcess 는 핸들이 NULL 일 수 있음 — 그 경우 false 즉시 반환.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return false;
    }
    let mut code: u32 = 0;
    // SAFETY: 위 OpenProcess 가 성공한 핸들. code 는 stack 위 유효 포인터.
    let ok = unsafe { GetExitCodeProcess(h, &mut code) };
    // SAFETY: 위 OpenProcess 의 핸들. CloseHandle 은 NULL 이 아닌 핸들에서 안전.
    unsafe {
        CloseHandle(h);
    }
    ok != 0 && code == STILL_ACTIVE as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_alive() {
        let pid = std::process::id();
        assert!(is_alive(pid));
    }

    #[test]
    fn nonexistent_pid_is_not_alive() {
        // 일반적으로 매우 큰 pid 는 비활성 — 100% 보장은 안 되지만 테스트 환경에선
        // 충분히 안전.
        assert!(!is_alive(0xFFFF_FFFE));
    }
}
