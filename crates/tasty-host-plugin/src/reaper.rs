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
//!   (고아 허용 — GUI 가시 프로세스라 사용자 인지·종료 가능). **주의**: PDEATHSIG
//!   는 fork 한 *스레드* 종료에 발화하므로 spawn 은 반드시 [`spawn_bound`] 를
//!   거쳐 영속 spawner 스레드에서 일어나야 한다 (linux imp 문서 참조).
//!
//! [`spawn_bound`]: PluginReaper::spawn_bound
//! - **macOS**: PDEATHSIG 등가물이 없어 호스트 측 결박이 불가능하다. `prepare` 가
//!   호스트 PID 를 `TASTY_HOST_PID` env 로 주입하고, 실제 self-exit 는 plugin SDK
//!   런타임(`tasty-plugin-sdk`)의 부모-사망 watchdog 이 수행한다.
//! - **그 외 OS**: 전부 no-op stub.
//!
//! 모든 실패는 호출부에서 `tracing::warn!` 으로 흡수하고 기존 kill 기반 정리로
//! degrade 한다 — 결박 실패가 플러그인 기능이나 호스트를 죽여서는 안 된다.

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command};

    use tasty_reaper::JobObject;
    use windows_sys::Win32::Foundation::HANDLE;

    /// Windows Job Object 기반 reaper. Job Object primitive 는 [`tasty_reaper`] 에서
    /// 공유하며, 본 타입은 그 job 을 **자기 소유**(`PluginManager` 수명에 결박)한다 —
    /// 터미널 셸을 결박하는 전역 job 과는 별개 인스턴스다(둘 다 `KILL_ON_JOB_CLOSE`
    /// 라 프로세스 사망 시 동일하게 정리된다). `job` 이 `None` 이면 결박 비활성.
    pub struct PluginReaper {
        job: Option<JobObject>,
    }

    impl PluginReaper {
        /// Job Object 를 생성한다. 실패 시 Err — 호출부가 warn 후
        /// [`disabled`](Self::disabled) 로 degrade 한다.
        pub fn new() -> io::Result<Self> {
            Ok(Self {
                job: Some(JobObject::new()?),
            })
        }

        /// 결박이 비활성화된 reaper. `adopt` 가 항상 no-op 으로 성공한다.
        pub fn disabled() -> Self {
            Self { job: None }
        }

        /// spawn 전 `Command` 준비. Windows 는 Job assign 을 spawn *후*에 하므로
        /// 여기선 할 일이 없다.
        pub fn prepare(&self, _cmd: &mut Command) {}

        /// 준비된 Command 를 실제 spawn 한다. Windows 는 Job Object 가 프로세스
        /// 수명에 결박되므로 호출 스레드에서 직접 spawn 해도 안전하다
        /// (Linux 판의 스레드 우회는 PDEATHSIG 전용 — 그쪽 문서 참조).
        pub fn spawn_bound(&self, mut cmd: Command) -> io::Result<Child> {
            cmd.spawn()
        }

        /// spawn 직후 자식 프로세스를 Job 에 assign 한다. Job 이 비활성이면 no-op.
        pub fn adopt(&self, child: &Child) -> io::Result<()> {
            let Some(job) = self.job.as_ref() else {
                return Ok(());
            };
            // SAFETY: child 는 살아있는 자식 프로세스이고 as_raw_handle 이 반환하는 것은
            // std 가 소유한 유효한 프로세스 핸들이다(spawn 이 부여한 완전 권한 포함) →
            // assign_handle 의 안전 계약 충족.
            unsafe { job.assign_handle(child.as_raw_handle() as HANDLE) }
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};
    use std::sync::OnceLock;
    use std::sync::mpsc::{self, Sender, SyncSender};

    /// Linux PDEATHSIG 기반 reaper. 상태가 없어 unit struct.
    pub struct PluginReaper;

    /// spawner 스레드로 보내는 spawn 요청: (준비된 Command, 결과 반송 채널).
    type SpawnJob = (Command, SyncSender<io::Result<Child>>);

    /// PDEATHSIG 는 부모 *프로세스*가 아니라 **fork 한 스레드**가 종료할 때 발화한다
    /// (man prctl 의 경고). 부트 워커 같은 단명 스레드에서 plugin 을 spawn 하면
    /// 그 스레드가 부팅을 마치고 종료하는 순간 plugin 전원이 SIGKILL 을 받는다
    /// (실측: 매 부팅마다 전 plugin 즉사 → 60s healthcheck 가 재스폰할 때까지
    /// plugin 기능 전멸). 이를 막기 위해 모든 plugin spawn 을 프로세스 수명과
    /// 함께 가는 전용 스레드로 우회시킨다 — PDEATHSIG 가 이 스레드에 결박되므로
    /// 사실상 호스트 프로세스 수명에 결박된다(원래 의도한 시맨틱).
    ///
    /// 반환 `None` = spawner 스레드 생성 실패(리소스 고갈 등). 호출부는 직접
    /// spawn 으로 degrade 한다 — 결박이 스레드 수명에 좌우되는 원래 위험으로
    /// 돌아갈 뿐 spawn 자체는 성공해야 한다.
    fn spawner() -> Option<&'static Sender<SpawnJob>> {
        static TX: OnceLock<Option<Sender<SpawnJob>>> = OnceLock::new();
        TX.get_or_init(|| {
            let (tx, rx) = mpsc::channel::<SpawnJob>();
            let spawned = std::thread::Builder::new()
                .name("plugin-spawner".into())
                .spawn(move || {
                    while let Ok((mut cmd, reply)) = rx.recv() {
                        // 수신측이 먼저 죽었으면(호출부 panic 등) 보낼 곳이 없을 뿐
                        // — spawn 결과 유실은 해당 plugin 시작 실패로 이미 표면화된다.
                        let _ = reply.send(cmd.spawn());
                    }
                });
            match spawned {
                Ok(_) => Some(tx),
                Err(e) => {
                    tracing::warn!(
                        "plugin-spawner thread creation failed — PDEATHSIG binds to \
                         caller thread lifetime instead: {e}"
                    );
                    None
                }
            }
        })
        .as_ref()
    }

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

        /// 준비된 Command 를 실제 spawn 한다. Linux 는 영속 spawner 스레드에서
        /// fork 해 PDEATHSIG 를 프로세스 수명에 결박한다 ([`spawner`] 참조).
        /// 채널 왕복이 불가능한 경우에만 호출 스레드 직접 spawn 으로 degrade.
        pub fn spawn_bound(&self, mut cmd: Command) -> io::Result<Child> {
            let Some(tx) = spawner() else {
                return cmd.spawn();
            };
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            if tx.send((cmd, reply_tx)).is_err() {
                // spawner 스레드가 죽었다(정상 경로에선 발생하지 않음). Command 는
                // 이미 이동했으므로 degrade 불가 — 에러로 표면화한다.
                return Err(io::Error::other("plugin-spawner thread is gone"));
            }
            reply_rx
                .recv()
                .map_err(|_| io::Error::other("plugin-spawner thread dropped reply"))?
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

        /// 준비된 Command 를 실제 spawn 한다. macOS 는 자식 측 watchdog 이 호스트
        /// PID 를 폴링하므로(스레드 무관) 호출 스레드에서 직접 spawn 해도 안전하다.
        pub fn spawn_bound(&self, mut cmd: Command) -> std::io::Result<Child> {
            cmd.spawn()
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

        /// 결박 메커니즘이 없는 플랫폼 — 직접 spawn.
        pub fn spawn_bound(&self, mut cmd: Command) -> std::io::Result<Child> {
            cmd.spawn()
        }

        pub fn adopt(&self, _child: &Child) -> std::io::Result<()> {
            Ok(())
        }
    }
}

pub use imp::PluginReaper;

#[cfg(test)]
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다(전수 가드가 제외한다) —
// 여기 경고는 조치 대상이 될 수 없어 프로덕션 신호만 가린다. error-handling.md.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::PluginReaper;

    #[test]
    fn new_succeeds_on_supported_platforms() {
        // Windows: 실제 Job 생성. 그 외: stub 이 무조건 성공.
        let reaper = PluginReaper::new();
        assert!(reaper.is_ok(), "reaper init failed: {:?}", reaper.err());
    }

    /// PDEATHSIG 스레드 결박 회귀 테스트: 단명 스레드에서 spawn_bound 로 띄운
    /// 자식은 그 스레드가 죽어도 살아있어야 한다. spawn_bound 가 영속 spawner
    /// 스레드를 경유하지 않으면(=호출 스레드에서 직접 fork) PDEATHSIG 가 호출
    /// 스레드 종료 시 발화해 자식이 SIGKILL 로 죽는다 — 부트 워커에서 스폰된
    /// plugin 전원이 부팅 직후 전멸하던 실제 버그의 최소 재현.
    #[cfg(target_os = "linux")]
    #[test]
    fn spawn_bound_child_survives_caller_thread_exit() {
        let mut child = std::thread::spawn(|| {
            let reaper = PluginReaper::new().expect("reaper init");
            let mut cmd = std::process::Command::new("sleep");
            cmd.arg("30");
            reaper.prepare(&mut cmd);
            reaper.spawn_bound(cmd).expect("spawn_bound failed")
        })
        .join()
        .expect("spawner-caller thread panicked");
        // 호출 스레드는 위 join 시점에 이미 종료 — PDEATHSIG 가 그 스레드에
        // 결박돼 있었다면 SIGKILL 은 즉시 배달된다. 여유를 두고 관찰.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let state = child.try_wait().expect("try_wait failed");
        // 테스트 종료 전 정리 (살아있을 때만 의미 있음).
        let _ = child.kill(); // 실패는 이미 죽은 경우뿐 — 아래 assert 가 판정.
        let _ = child.wait();
        assert!(
            state.is_none(),
            "child died after caller thread exit (PDEATHSIG bound to thread): {state:?}"
        );
    }

    #[test]
    fn disabled_reaper_adopt_is_noop() {
        // disabled() 는 child 핸들 없이도 만들어지고, 실제 adopt 는 spawn 경로에서만
        // 호출되므로 여기선 생성만 검증한다(이미 죽은/없는 child 로 adopt 호출 시
        // 호출부가 warn 으로 흡수하는 계약).
        let _ = PluginReaper::disabled(); // 생성만 검증 — 반환 Result 의도적 무시(아래 단순 construct 테스트).
    }
}
