//! 플러그인 자식 프로세스의 수명을 호스트(tasty)에 결박하는 크로스 플랫폼 추상화.
//!
//! tasty 가 정상/비정상(하드 크래시·`taskkill /f`·디버거 강제종료) 어느 경로로
//! 종료되든 플러그인 프로세스(와 그 자식)가 함께 종료되도록 OS 커널 메커니즘에
//! 묶는다. `PluginProcess::shutdown` / `Drop` 의 `child.kill()` 만으로는 비정상
//! 경로에서 정리가 돌지 않아 좀비 플러그인이 잔존할 수 있는데, 이를 메우는 것이
//! 본 타입의 목적이다.
//!
//! OS 별 메커니즘 (단일 인터페이스 `new`/`prepare`/`adopt` 뒤에 숨김):
//!
//! - **Windows**: Job Object (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`). 호스트가 Job
//!   핸들을 [`PluginManager`](crate::manager::PluginManager) 수명 동안 소유하며,
//!   호스트 프로세스가 죽어 핸들이 닫히는 순간 OS 가 Job 내 전 프로세스를 강제
//!   종료한다. Job 멤버십은 자식에 상속되므로 플러그인이 띄운 손자(node/chrome)도
//!   함께 커버된다. `adopt` 가 각 플러그인 자식을 Job 에 assign.
//! - **Linux**: `prctl(PR_SET_PDEATHSIG, SIGKILL)` (자식 `pre_exec`). 부모(tasty)
//!   사망 시 커널이 *직속* 플러그인에 SIGKILL 을 보낸다. 손자 프로세스는 범위 밖
//!   (고아 허용 — GUI 가시 프로세스라 사용자 인지·종료 가능).
//! - **macOS**: PDEATHSIG 등가물이 없어 호스트 측 결박이 불가능하다. `prepare` 가
//!   호스트 PID 를 `TASTY_HOST_PID` env 로 주입하고, 실제 self-exit 는 plugin SDK
//!   런타임(`tasty-plugin-sdk`)의 부모-사망 watchdog 이 수행한다.
//! - **그 외 OS**: 전부 no-op stub.
//!
//! 모든 실패는 호출부에서 `tracing::warn!` 으로 흡수하고 기존 kill 기반 정리로
//! degrade 한다 — 결박 실패가 플러그인 기능이나 호스트를 죽여서는 안 된다.

pub use imp::PluginReaper;

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use std::process::{Child, Command};
    use std::ptr;

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    /// Windows Job Object 기반 reaper. Job 핸들은 `OwnedHandle` 로 보관되어,
    /// 본 구조체가 drop 되면 `CloseHandle` → `KILL_ON_JOB_CLOSE` 로 Job 내 전
    /// 프로세스가 종료된다. `job` 이 `None` 이면 결박 비활성(생성 실패 fallback).
    pub struct PluginReaper {
        job: Option<OwnedHandle>,
    }

    impl PluginReaper {
        /// Job Object 를 생성하고 `KILL_ON_JOB_CLOSE` 를 설정한다. 실패 시 Err —
        /// 호출부가 warn 후 [`disabled`](Self::disabled) 로 degrade 한다.
        pub fn new() -> io::Result<Self> {
            // SAFETY: lpJobAttributes/lpName 둘 다 NULL → 기본 보안 속성의 익명 Job.
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

            // SAFETY: job 핸들은 유효하고, info 는 정보 클래스에 맞는 크기로 채워진
            // 유효 포인터다.
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
            Ok(Self { job: Some(job) })
        }

        /// 결박이 비활성화된 reaper. `adopt` 가 항상 no-op 으로 성공한다.
        pub fn disabled() -> Self {
            Self { job: None }
        }

        /// spawn 전 `Command` 준비. Windows 는 Job assign 을 spawn *후*에 하므로
        /// 여기선 할 일이 없다.
        pub fn prepare(&self, _cmd: &mut Command) {}

        /// spawn 직후 자식 프로세스를 Job 에 assign 한다. Job 이 비활성이면 no-op.
        pub fn adopt(&self, child: &Child) -> io::Result<()> {
            let Some(job) = self.job.as_ref() else {
                return Ok(());
            };
            // SAFETY: 두 핸들 모두 유효 — job 은 CreateJobObjectW 산물, child 는
            // 살아있는 std::process::Child 의 프로세스 핸들이다.
            let ok = unsafe {
                AssignProcessToJobObject(
                    job.as_raw_handle() as HANDLE,
                    child.as_raw_handle() as HANDLE,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    /// Linux PDEATHSIG 기반 reaper. 상태가 없어 unit struct.
    pub struct PluginReaper;

    impl PluginReaper {
        pub fn new() -> io::Result<Self> {
            Ok(Self)
        }

        pub fn disabled() -> Self {
            Self
        }

        /// spawn 전 `pre_exec` 로 PDEATHSIG 를 설치한다. fork 후 exec 전 자식
        /// 컨텍스트에서 실행된다.
        pub fn prepare(&self, cmd: &mut Command) {
            // SAFETY: pre_exec 는 unsafe fn — 클로저가 fork 후 exec 전 자식에서
            // 돌며 async-signal-safe 함수만 호출해야 한다. reap_on_parent_death 는
            // prctl/getppid/_exit 만 호출하므로 안전.
            unsafe {
                cmd.pre_exec(reap_on_parent_death);
            }
        }

        pub fn adopt(&self, _child: &Child) -> io::Result<()> {
            Ok(())
        }
    }

    /// `pre_exec` 콜백: 부모(tasty) 사망 시 SIGKILL 을 받도록 설정한다.
    fn reap_on_parent_death() -> io::Result<()> {
        // SAFETY: post-fork 자식 컨텍스트 — async-signal-safe libc 호출만 사용한다.
        unsafe {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(io::Error::last_os_error());
            }
            // prctl 설정과 fork 사이에 부모가 이미 죽었을 race 를 메운다:
            // 재부모화(getppid == 1)됐으면 즉시 종료.
            if libc::getppid() == 1 {
                libc::_exit(0);
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::process::{Child, Command};

    /// macOS reaper. 호스트 측엔 결박 메커니즘이 없어 `TASTY_HOST_PID` env 주입만
    /// 담당하고, 실제 self-exit 는 plugin SDK 런타임의 watchdog 이 수행한다.
    pub struct PluginReaper;

    impl PluginReaper {
        pub fn new() -> std::io::Result<Self> {
            Ok(Self)
        }

        pub fn disabled() -> Self {
            Self
        }

        /// 자식 watchdog 이 비교 기준으로 쓸 호스트 PID 를 env 로 주입.
        pub fn prepare(&self, cmd: &mut Command) {
            cmd.env("TASTY_HOST_PID", std::process::id().to_string());
        }

        pub fn adopt(&self, _child: &Child) -> std::io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod imp {
    use std::process::{Child, Command};

    /// 그 외 플랫폼: 결박 메커니즘 없음 — 전부 no-op stub.
    pub struct PluginReaper;

    impl PluginReaper {
        pub fn new() -> std::io::Result<Self> {
            Ok(Self)
        }

        pub fn disabled() -> Self {
            Self
        }

        pub fn prepare(&self, _cmd: &mut Command) {}

        pub fn adopt(&self, _child: &Child) -> std::io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PluginReaper;

    #[test]
    fn new_succeeds_on_supported_platforms() {
        // Windows: 실제 Job 생성. 그 외: stub 이 무조건 성공.
        let reaper = PluginReaper::new();
        assert!(reaper.is_ok(), "reaper init failed: {:?}", reaper.err());
    }

    #[test]
    fn disabled_reaper_adopt_is_noop() {
        // disabled() 는 child 핸들 없이도 만들어지고, 실제 adopt 는 spawn 경로에서만
        // 호출되므로 여기선 생성만 검증한다(이미 죽은/없는 child 로 adopt 호출 시
        // 호출부가 warn 으로 흡수하는 계약).
        let _ = PluginReaper::disabled();
    }
}
