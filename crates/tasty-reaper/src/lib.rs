//! 호스트(tasty)가 spawn 한 자식 프로세스(터미널 셸 등)를 호스트 프로세스 수명에
//! 결박하는 크로스플랫폼 primitive.
//!
//! tasty 가 어떤 경로로 죽든 — 정상 종료 · 하드 크래시 · `taskkill /f` · 디버거 강제
//! stop — 자식 셸 트리가 함께 종료되도록 OS 커널 메커니즘에 묶는다. 이것이 없으면
//! Windows 에서는 부모 사망이 자식을 죽이지 않고 ConPTY 에도 "pseudoconsole 종료 ⇒
//! 자식 종료" 보장이 없어 셸 트리가 고아로 잔존한다(디버그 세션마다 누적).
//!
//! OS 별 메커니즘:
//! - **Windows**: Job Object (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`). job 핸들을 호스트
//!   프로세스 수명 동안 소유하며, 호스트가 죽어 핸들이 닫히는 순간 OS 가 job 내 전
//!   프로세스를 강제 종료한다. 멤버십은 자식에 상속되므로 셸의 손자도 커버된다.
//! - **비-Windows**: no-op. tasty 종료 시 커널이 PTY master fd 를 닫아 셸 foreground
//!   프로세스 그룹에 SIGHUP 이 전달되어 자동 정리되므로 별도 결박이 필요 없다
//!   (portable-pty `CommandBuilder` 가 `pre_exec` 를 노출하지 않아 PDEATHSIG 설치도 불가).
//!
//! 모든 실패는 `tracing` 경고로 흡수한다 — 결박 실패가 호스트나 터미널 기능을 죽여서는
//! 안 된다.

use std::sync::OnceLock;

pub use imp::JobObject;

/// 프로세스 전역 호스트 job. [`init_host_reaper`] 로 부팅 시 1회 채운다.
///
/// `OnceLock<Option<JobObject>>` — 바깥 `None` = 미초기화(테스트/CLI client),
/// 안쪽 `None` = job 생성 실패로 결박 비활성. 어느 경우든 [`adopt_pid`] 는 no-op.
static HOST_JOB: OnceLock<Option<JobObject>> = OnceLock::new();

/// 호스트 프로세스(터미널을 spawn 하는 쪽)에서 **1회** 호출한다. plugin/terminal
/// spawn 보다 먼저 부팅 초기에 호출해야 한다. 중복 호출은 무시된다.
///
/// job 생성에 실패해도 앱은 계속 진행하며 결박만 비활성화된다.
pub fn init_host_reaper() {
    let job = match JobObject::new() {
        Ok(job) => Some(job),
        Err(e) => {
            tracing::warn!(
                "host reaper init failed; child shells may orphan on abnormal exit: {e}"
            );
            None
        }
    };
    if HOST_JOB.set(job).is_err() {
        tracing::warn!("init_host_reaper called more than once; ignoring");
    }
}

/// 자식 프로세스(PID)를 전역 호스트 job 에 결박한다. best-effort — 미초기화이거나
/// job 비활성이거나 `pid` 가 `None` 이면 조용히 no-op. 비-Windows 에서도 no-op.
pub fn adopt_pid(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };
    let Some(Some(job)) = HOST_JOB.get() else {
        return;
    };
    if let Err(e) = job.assign_pid(pid) {
        tracing::warn!("reaper adopt pid {pid} failed: {e}");
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use std::ptr;

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    /// Windows Job Object 기반 결박 primitive. `job` 은 `OwnedHandle` 로 보관되어,
    /// 본 구조체가 drop 되면 `CloseHandle` 되고 `KILL_ON_JOB_CLOSE` 로 job 내 전
    /// 프로세스가 종료된다.
    pub struct JobObject {
        job: OwnedHandle,
    }

    impl JobObject {
        /// Job 을 생성하고 `KILL_ON_JOB_CLOSE` 를 설정한다.
        pub fn new() -> io::Result<Self> {
            // SAFETY: lpJobAttributes/lpName 둘 다 NULL → 기본 보안 속성의 익명 job.
            let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: raw 는 CreateJobObjectW 가 막 반환한 유효·단독 소유 핸들이며,
            // OwnedHandle 이 소유권을 가져가 drop 시 CloseHandle 한다.
            let job = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };

            // SAFETY: JOBOBJECT_EXTENDED_LIMIT_INFORMATION 은 POD 이며 all-zero 가
            // 유효한 초기 상태다.
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            // SAFETY: job 핸들은 유효하고, info 는 정보 클래스에 맞는 크기의 유효 포인터다.
            let ok = unsafe {
                SetInformationJobObject(
                    job.as_raw_handle() as HANDLE,
                    JobObjectExtendedLimitInformation,
                    ptr::addr_of!(info).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { job })
        }

        /// 이미 열린 프로세스 핸들을 job 에 assign 한다. 핸들은
        /// `PROCESS_SET_QUOTA | PROCESS_TERMINATE` 권한을 가져야 한다.
        pub fn assign_handle(&self, process_handle: HANDLE) -> io::Result<()> {
            // SAFETY: job 핸들은 유효(자기 소유), process_handle 은 호출자 보장 유효.
            let ok =
                unsafe { AssignProcessToJobObject(self.job.as_raw_handle() as HANDLE, process_handle) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        /// PID 로 프로세스를 열어 job 에 assign 한다. portable-pty 처럼 `std::process::Child`
        /// 핸들을 직접 못 얻는 자식(터미널 셸)용 경로.
        pub fn assign_pid(&self, pid: u32) -> io::Result<()> {
            // SAFETY: OpenProcess 는 실패 시 NULL 을 반환하며 아래에서 검사한다.
            let raw = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: raw 는 OpenProcess 가 막 반환한 유효·단독 소유 핸들. OwnedHandle 이
            // 소유권을 가져가 이 함수 종료 시 CloseHandle 한다(assign 은 job 이 자체 참조를
            // 잡으므로 이후 핸들을 닫아도 무방).
            let owned = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
            self.assign_handle(owned.as_raw_handle() as HANDLE)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::io;

    /// 비-Windows: 결박 메커니즘 없음(SIGHUP 자동정리에 의존) — 전부 no-op stub.
    /// 타입/시그니처는 Windows 와 동일해 호출부가 `#[cfg]` 분기 없이 쓴다.
    pub struct JobObject;

    impl JobObject {
        pub fn new() -> io::Result<Self> {
            Ok(Self)
        }

        pub fn assign_pid(&self, _pid: u32) -> io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_object_new_succeeds() {
        // Windows: 실제 job 생성 / 비-Windows: stub 이 항상 Ok.
        // 주: 현재 프로세스를 assign 하지 않는다 — KILL_ON_JOB_CLOSE job 이 drop 되면
        // 테스트 러너 자신이 종료되기 때문. 여기선 생성/파괴 경로만 검증한다.
        let job = JobObject::new();
        assert!(job.is_ok(), "JobObject::new failed: {:?}", job.err());
        // drop 시 CloseHandle — 멤버가 없으므로 아무것도 죽지 않는다.
    }

    #[test]
    fn adopt_pid_none_is_noop() {
        // pid 가 None 이면 전역 job 초기화 여부와 무관하게 즉시 반환(패닉/부수효과 없음).
        adopt_pid(None);
    }

    #[test]
    fn adopt_pid_without_init_is_noop() {
        // init_host_reaper 미호출(테스트에선 호출 안 함) → HOST_JOB 미설정 →
        // 존재하지 않는 pid 라도 OpenProcess 조차 시도하지 않고 조용히 no-op.
        adopt_pid(Some(0xFFFF_FFFF));
    }
}
