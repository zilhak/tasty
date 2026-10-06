//! Windows 자식을 KILL_ON_JOB_CLOSE Job Object에 등록한다.
//! 등록된 프로세스는 호스트의 마지막 job 핸들이 닫힐 때 OS가 종료한다.
//! 생성·등록 실패는 경고하고 호스트 실행을 계속하므로 모든 자식의 정리를 보장하지는 않는다.
//! 비 Windows에서는 아무 작업도 하지 않는다. 해당 플랫폼의 PTY·자식 종료 처리는 호출자 책임이다.
//! 예외로 [`spawn_bound_to_host`]는 Linux에서 PDEATHSIG로 자식을 호스트 수명에 묶는다.

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

/// 호스트가 비정상 종료해도 남지 않아야 하는 자식(SSH 터널 등)을 실행한다.
///
/// Linux는 부모가 죽으면 자식이 SIGTERM을 받도록 PDEATHSIG를 설정한다. PDEATHSIG는 fork한
/// 스레드의 종료에 반응하므로 짧게 사는 워커 대신 프로세스 수명 동안 남는 spawner 스레드에서
/// 실행한다. Windows는 [`init_host_reaper`]로 만든 호스트 job에 등록한다(초기화하지 않은
/// CLI 등에서는 등록하지 않는다). 다른 OS는 결속하지 않고 그대로 실행한다.
pub fn spawn_bound_to_host(cmd: std::process::Command) -> std::io::Result<std::process::Child> {
    bound::spawn(cmd)
}

#[cfg(target_os = "linux")]
mod bound {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};
    use std::sync::OnceLock;
    use std::sync::mpsc::{self, Sender, SyncSender};

    type SpawnJob = (Command, SyncSender<io::Result<Child>>);

    /// 스레드를 만들지 못하면 None이며, 호출자 스레드에서 실행한다.
    fn spawner() -> Option<&'static Sender<SpawnJob>> {
        static TX: OnceLock<Option<Sender<SpawnJob>>> = OnceLock::new();
        TX.get_or_init(|| {
            let (tx, rx) = mpsc::channel::<SpawnJob>();
            let spawned = std::thread::Builder::new()
                .name("bound-spawner".into())
                .spawn(move || {
                    while let Ok((mut cmd, reply)) = rx.recv() {
                        deliver(&reply, cmd.spawn());
                    }
                });
            match spawned {
                Ok(_) => Some(tx),
                Err(e) => {
                    tracing::warn!(
                        "bound-spawner thread creation failed; PDEATHSIG follows the caller thread: {e}"
                    );
                    None
                }
            }
        })
        .as_ref()
    }

    // 요청자가 떠나 전달하지 못한 자식은 Child drop만으로 회수되지 않으므로 직접 끝낸다.
    fn deliver(reply: &SyncSender<io::Result<Child>>, result: io::Result<Child>) {
        let Err(mpsc::SendError(Ok(mut child))) = reply.send(result) else {
            return;
        };
        let pid = child.id();
        if let Err(error) = child.kill() {
            tracing::warn!(pid, %error, "could not stop an unclaimed bound child");
        }
        if let Err(error) = child.wait() {
            tracing::warn!(pid, %error, "could not reap an unclaimed bound child");
        }
    }

    pub(super) fn spawn(mut cmd: Command) -> io::Result<Child> {
        let parent = std::process::id();
        // SAFETY: pre_exec 클로저는 fork 뒤 exec 전 자식에서 실행된다. 아래 함수는
        // async-signal-safe인 prctl·getppid·_exit만 호출하고 할당하지 않는다.
        unsafe {
            cmd.pre_exec(move || exit_with_parent(parent));
        }
        let Some(tx) = spawner() else {
            return cmd.spawn();
        };
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        if tx.send((cmd, reply_tx)).is_err() {
            return Err(io::Error::other("bound-spawner thread is gone"));
        }
        reply_rx
            .recv()
            .map_err(|_| io::Error::other("bound-spawner thread dropped its reply"))?
    }

    fn exit_with_parent(parent: u32) -> io::Result<()> {
        // SAFETY: fork 뒤 자식에서 인자만 정수인 prctl을 호출한다.
        let set = unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) };
        if set != 0 {
            return Err(io::Error::last_os_error());
        }
        // 설정 전에 부모가 이미 끝났으면 신호가 오지 않는다. 부모가 subreaper(systemd --user 등)로
        // 바뀌므로 1과 비교하지 않고 fork 전 PID와 비교한다.
        // SAFETY: getppid는 실패하지 않는 async-signal-safe 호출이다.
        let now = unsafe { libc::getppid() };
        if u32::try_from(now).ok() != Some(parent) {
            // SAFETY: exec 전 자식을 즉시 끝낸다. 부모의 atexit·버퍼를 실행하지 않는다.
            unsafe { libc::_exit(0) };
        }
        Ok(())
    }
}

#[cfg(windows)]
mod bound {
    pub(super) fn spawn(mut cmd: std::process::Command) -> std::io::Result<std::process::Child> {
        let child = cmd.spawn()?;
        super::adopt_pid(Some(child.id()));
        Ok(child)
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod bound {
    pub(super) fn spawn(mut cmd: std::process::Command) -> std::io::Result<std::process::Child> {
        cmd.spawn()
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

        /// 이미 열린 프로세스 핸들을 job 에 assign 한다.
        ///
        /// # Safety
        /// `process_handle` 은 `PROCESS_SET_QUOTA | PROCESS_TERMINATE` 권한을 가진
        /// 유효한 열린 프로세스 핸들이어야 한다(호출자 보장). 무효 핸들이면
        /// `AssignProcessToJobObject` 가 실패를 반환할 뿐이지만 계약상 유효를 요구한다.
        pub unsafe fn assign_handle(&self, process_handle: HANDLE) -> io::Result<()> {
            // SAFETY: job 핸들은 유효(자기 소유), process_handle 은 호출자 보장 유효.
            let ok = unsafe {
                AssignProcessToJobObject(self.job.as_raw_handle() as HANDLE, process_handle)
            };
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
            // SAFETY: owned 는 방금 OpenProcess 로 PROCESS_SET_QUOTA | PROCESS_TERMINATE
            // 권한으로 연 유효 핸들이므로 assign_handle 의 안전 계약을 충족한다.
            unsafe { self.assign_handle(owned.as_raw_handle() as HANDLE) }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::io;

    /// 비 Windows에서는 OS 결속을 수행하지 않는다. 호출 인터페이스만 동일하게 제공한다.
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
        // 테스트 러너를 job에 등록하지 않는다. 여기서는 빈 job의 생성·해제만 확인한다.
        let job = JobObject::new();
        assert!(job.is_ok(), "JobObject::new failed: {:?}", job.err());
    }

    #[test]
    fn adopt_pid_none_is_noop() {
        adopt_pid(None);
    }

    #[test]
    fn adopt_pid_without_init_is_noop() {
        // 초기화하지 않은 전역 job에는 실제 프로세스를 등록하지 않는다.
        adopt_pid(Some(0xFFFF_FFFF));
    }
}
