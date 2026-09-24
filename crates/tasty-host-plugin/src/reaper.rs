//! 호스트 종료에 맞춰 플러그인 프로세스를 정리하기 위한 플랫폼별 지원.
//! Windows는 Job Object에 등록한 프로세스를, Linux는 PDEATHSIG를 설정한 직속 자식을 대상으로 한다.
//! Linux에서는 영속 spawner 스레드를 써야 짧게 실행되는 호출 스레드의 종료 영향을 피할 수 있다.
//! macOS는 TASTY_HOST_PID를 전달하고 SDK의 부모 감시에 의존한다. 나머지 OS는 no-op이다.
//! 초기화·등록 실패나 SDK 미사용까지 포함한 종료 보장은 아니다.

// 이유: FFI의 연결된 포인터·핸들 연산을 같은 unsafe 블록에서 처리한다.
// 이 예외는 새 unsafe 연산도 가리므로 안전한 래퍼로 옮길 때 다시 검토한다.
#![allow(clippy::multiple_unsafe_ops_per_block)]

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command};

    use tasty_reaper::JobObject;
    use windows_sys::Win32::Foundation::HANDLE;

    /// 매니저가 소유한 별도 Job Object. None이면 등록하지 않는다.
    pub struct PluginReaper {
        job: Option<JobObject>,
    }

    impl PluginReaper {
        /// Job Object를 만든다. 실패하면 호출자가 경고하고 disabled를 사용한다.
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

        /// Windows에서는 호출 스레드에서 실행하고 이후 adopt로 Job에 등록한다.
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

    /// PDEATHSIG가 일시적인 호출 스레드의 종료에 반응하지 않도록 영속 스레드에서 실행한다.
    /// 스레드를 만들지 못하면 None을 반환하며, 호출자는 자기 스레드에서 실행한다.
    fn spawner() -> Option<&'static Sender<SpawnJob>> {
        static TX: OnceLock<Option<Sender<SpawnJob>>> = OnceLock::new();
        TX.get_or_init(|| {
            let (tx, rx) = mpsc::channel::<SpawnJob>();
            let spawned = std::thread::Builder::new()
                .name("plugin-spawner".into())
                .spawn(move || {
                    while let Ok((mut cmd, reply)) = rx.recv() {
                        // 의도적 무시: 호출자가 사라졌으면 실행 결과를 전달할 곳이 없다.
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

        /// 영속 spawner에서 실행한다. 스레드 생성 실패 때만 호출 스레드로 대신한다.
        /// 작업 전달이나 결과 수신 실패는 오류로 반환한다.
        pub fn spawn_bound(&self, mut cmd: Command) -> io::Result<Child> {
            let Some(tx) = spawner() else {
                return cmd.spawn();
            };
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            if tx.send((cmd, reply_tx)).is_err() {
                // Command를 이미 전달했으므로 다시 실행할 수 없다.
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
            // 부모가 먼저 종료돼 PID 1 아래로 이동했다면 즉시 종료한다.
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

    /// 부모 PID를 전달한다. 종료 감시는 SDK 런타임이 맡는다.
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

        /// 호출 스레드에서 실행한다. SDK는 부모 프로세스의 PID를 확인한다.
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
// 테스트의 정리 작업에서 결과를 의도적으로 무시한다.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::PluginReaper;

    #[test]
    fn new_succeeds_on_supported_platforms() {
        // Windows: 실제 Job 생성. 그 외: stub 이 무조건 성공.
        let reaper = PluginReaper::new();
        assert!(reaper.is_ok(), "reaper init failed: {:?}", reaper.err());
    }

    /// 짧게 실행되는 스레드에서 spawn_bound를 호출해도 그 스레드의 종료가
    /// 자식에 PDEATHSIG를 보내지 않아야 한다.
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
        // 호출 스레드가 끝난 뒤에도 자식이 살아 있는지 잠시 후 확인한다.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let state = child.try_wait().expect("try_wait failed");
        // 테스트 종료 전 정리 (살아있을 때만 의미 있음).
        let _ = child.kill(); // 정리 시도이며 종료 상태는 아래에서 확인한다.
        let _ = child.wait();
        assert!(
            state.is_none(),
            "child died after caller thread exit (PDEATHSIG bound to thread): {state:?}"
        );
    }

    #[test]
    fn disabled_reaper_adopt_is_noop() {
        // 자식 핸들 없이 비활성 reaper를 만들 수 있는지만 확인한다.
        let _ = PluginReaper::disabled(); // 의도적 무시: 생성 가능 여부만 확인한다.
    }
}
