//! 웹훅 통합 테스트 전용 하네스 (S16).
//!
//! `tests/common` 을 미러링하되 웹훅 E2E 가 필요로 하는 것을 추가한다:
//! - **웹훅 포트 시딩** — `TASTY_HOME/webhooks.toml` 에 미리 `port = N` 을 써
//!   리스너가 결정적으로 그 포트에 bind 하게 한다(테스트마다 free port 로 격리).
//!   포트는 [`PortLease`] 로 잡아 spawn 직전까지 예약을 유지하고, 그래도 남는
//!   경합 구간(자식이 부팅을 마치고 bind 하기까지)은 [`WebhookInstance`] 의
//!   재시도가 처리한다 — 아래 "포트 경합" 참조.
//! - **TASTY_HOME 제어** — `webhooks.toml`/`hook-handlers.toml` 위치를 잡고,
//!   재시작 테스트가 두 인스턴스 간 같은 홈을 공유할 수 있게 한다.
//! - **실 HTTP 클라이언트** — 리스너에 실제 요청을 쏴 ACK/상태변화를 관측한다
//!   (research §5: 로컬 실 HTTP 구동 검증).
//! - **CLI 러너** — 인스턴스와 **같은 바이너리**([`spawn_diag::instance_bin`])를
//!   `--port-file` 로 붙여 CLI→IPC 매핑을
//!   실 바이너리로 검증한다.
//!
//! 기존 `tests/common` 을 건드리지 않으려고 별도 모듈로 둔다(격리).

#![allow(dead_code)]
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다 — 전수 가드
// (`tests/let_underscore_documented.rs`)가 테스트 본문을 제외하므로, 여기서 나는
// `let_underscore_must_use` 경고는 정책상 조치 대상이 될 수 없다. 끄지 않으면
// 프로덕션의 진짜 신호가 그 안에 묻힌다 — `docs/dev-guide/error-handling.md`.
#![allow(clippy::let_underscore_must_use)]

#[path = "../spawn_diag/mod.rs"]
mod spawn_diag;

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

const STDERR_RING_CAPACITY: usize = 256;
const STDERR_TAIL_LINES: usize = 40;

// spawn 2 단계의 상한은 `tests/common` 과 같은 값을 쓴다 — 같은 바이너리를 같은
// 방식으로 띄우므로 잣대가 다를 근거가 없다. 근거는 `tests/spawn_diag`.
use spawn_diag::{SPAWN_PORT_TIMEOUT, SPAWN_SHELL_TIMEOUT};

/// 리스너가 accept 를 시작할 때까지의 상한. 위 두 단계와 달리 웹훅 고유 단계다.
const WEBHOOK_READY_TIMEOUT: Duration = Duration::from_secs(20);

/// 포트를 뺏겼을 때 새 포트로 다시 띄우는 횟수 상한. 경합은 드물고(제3자가 하필 그
/// 순간 같은 번호를 받아야 한다) 한 번의 재시도가 곧 한 번의 전체 부팅(수 초)이라,
/// 무한 재시도로 실패를 늘어뜨리는 대신 두 번으로 끊고 원인을 명시해 실패시킨다.
const PORT_STEAL_RETRIES: usize = 2;

/// 웹훅 리스너 bind 실패 경고의 고정 어구 (`src/webhook/listener.rs` 의 `on_bind_failed`).
/// 이 줄이 stderr 에 있으면 "안 떴다" 가 아니라 "포트를 가져갔다" 가 확정된다.
const BIND_FAILED_MARKER: &str = "webhook listener bind";

/// stderr 한 줄이 **bind 실패**인가. 재시도 여부가 여기서 갈리므로 순수 함수로 떼어
/// 두고 아래 테스트가 제품의 실제 문구 셋과 대조한다 — 성공 줄(`bound`)을 실패로
/// 읽으면 하네스가 정상 인스턴스를 버리고 재시도를 돌게 된다.
fn is_bind_failure(line: &str) -> bool {
    line.contains(BIND_FAILED_MARKER) && line.contains("failed")
}

/// OS 가 배정한 free TCP 포트의 **예약**. 리스너를 살려 둔 채 번호만 알려주므로,
/// 이 값이 살아 있는 동안에는 제3자가 같은 포트를 가져갈 수 없다.
///
/// 예약만으로 경합이 사라지지는 않는다 — 자식 프로세스는 부팅을 마친 뒤에야 웹훅
/// 포트에 bind 하고, 그 시점엔 이 리스너가 이미 풀려 있어야 한다(같은 포트를 두
/// 소켓이 동시에 listen 할 수 없다). 즉 예약은 "시딩~spawn" 구간만 닫고, 남는
/// 구간(=자식 부팅 시간, 수 초)은 [`WebhookInstance::spawn_inner`] 의 재시도가 맡는다.
/// 두 장치가 함께 있어야 이 하네스가 포트 경합에서 자유롭다.
pub struct PortLease {
    /// `None` 이 되면 예약이 풀린 것. spawn 직전에만 푼다.
    listener: Option<std::net::TcpListener>,
    port: u16,
}

impl PortLease {
    pub fn port(&self) -> u16 {
        self.port
    }

    /// 자식이 이 포트에 bind 할 수 있도록 예약을 푼다. `Command::spawn` 직전에만 부른다.
    fn release(&mut self) {
        self.listener = None;
    }
}

/// OS 에서 free 포트를 하나 받아 **예약한 채로** 돌려준다. 반환값을 살려 두는 동안
/// 그 포트는 이 프로세스 것이다 — 번호만 받고 버리는 형태(TOCTOU)를 타입으로 막는다.
pub fn free_port() -> PortLease {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind free port");
    let port = listener.local_addr().expect("local_addr").port();
    PortLease {
        listener: Some(listener),
        port,
    }
}

/// 웹훅 리스너가 accept 를 시작했는지 판정한 결과.
enum BindOutcome {
    /// 실제로 connect 가 됐다.
    Bound,
    /// stderr 에 bind 실패 경고가 찍혔다 — 포트를 제3자가 가져갔다는 직접 증거.
    Stolen(String),
    /// 상한 시간 안에 accept 도, bind 실패 경고도 없었다.
    Silent,
}

fn config_toml() -> String {
    let shell_path = if cfg!(windows) {
        "C:/Program Files/Git/bin/bash.exe"
    } else {
        "/bin/sh"
    };
    format!(
        r#"[general]
shell = "{shell}"
shell_mode = "default"
shell_args = ""
startup_command = ""
language = "en"
scrollback_lines = 10000
confirm_close_running = false
click_to_move_cursor = true
inherit_cwd = false
close_behavior = "ask"
restore_layout = false
restore_surface_content = false
link_click_modifier = "ctrl"
"#,
        shell = shell_path
    )
}

/// 웹훅 인스턴스 빌더.
pub struct Builder {
    /// 우리가 고른 포트의 예약. 재시작 시나리오([`WebhookInstance::builder_for_restart`])는
    /// 이미 있는 `webhooks.toml` 의 값을 그대로 써야 하므로 예약이 없다.
    lease: Option<PortLease>,
    /// 이 하네스가 고른 포트. `None` 이면 "홈의 `webhooks.toml` 에 적힌 값을 쓴다" 는 뜻
    /// (재시작 시나리오) — 그 파일이 곧 SoT 라 하네스가 번호를 되풀이해 들고 다니지 않는다.
    webhook_port: Option<u16>,
    /// TASTY_HOME 로 쓸 디렉토리. `None` 이면 고유 temp 를 만들고 Drop 시 삭제.
    /// `Some` 이면 caller 소유(Drop 삭제 안 함) — 재시작 테스트용.
    home: Option<PathBuf>,
    env: Vec<(String, String)>,
    files: Vec<(String, String)>,
}

impl Builder {
    /// caller 소유 홈 디렉토리 지정(재시작 테스트에서 공유). Drop 시 삭제하지 않는다.
    pub fn home(mut self, dir: PathBuf) -> Self {
        self.home = Some(dir);
        self
    }

    pub fn env(mut self, key: &str, val: &str) -> Self {
        self.env.push((key.to_string(), val.to_string()));
        self
    }

    /// TASTY_HOME 아래 추가 파일(예: `hook-handlers.toml`)을 spawn 전에 쓴다.
    pub fn file(mut self, name: &str, content: &str) -> Self {
        self.files.push((name.to_string(), content.to_string()));
        self
    }

    pub fn spawn(self) -> WebhookInstance {
        WebhookInstance::spawn_inner(self)
    }
}

/// `Command::spawn()` 대신 이걸 쓴다(Linux). 그냥 `pre_exec` 로 `PR_SET_PDEATHSIG` 를
/// 걸면 **호출한 스레드**에 묶인다(`man 2 prctl`: "the parent ... is considered to be
/// the thread that created this process") — 지금은 이 하네스가 인스턴스마다 전용
/// spawn 이라 호출 스레드가 인스턴스 수명 내내 살아있어 우연히 안전하지만, 그 우연에
/// 기대지 않는다(공유 진입점이 나중에 생기거나 스레드를 넘나드는 사용이 생기면 바로
/// 깨지는 함정이다 — `tests/common/mod.rs` 의 동명 함수가 실제로 이 증상으로 한 번
/// 깨졌다). fork 자체를 **프로세스 수명 동안 파킹만 하는 전용 스레드**에서 실행해
/// 커널이 추적하는 "부모 스레드" 를 프로세스 수명과 맞춘다.
#[cfg(target_os = "linux")]
fn spawn_with_stable_pdeathsig_anchor(mut command: Command) -> std::io::Result<Child> {
    use std::os::unix::process::CommandExt;
    use std::sync::mpsc;

    // SAFETY: 클로저는 fork 이후 exec 이전, 자식 프로세스 단독 스레드에서
    // 실행된다. 호출하는 prctl/getppid 둘 다 인자를 포인터로 받지 않는
    // async-signal-safe 순수 시스템 콜이라(힙 할당·락 없음) pre_exec 의 제약을
    // 지킨다.
    unsafe {
        command.pre_exec(|| {
            let ret = libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            if ret != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // fork 와 위 prctl 호출 사이에 부모(anchor 스레드)가 이미 죽었을
            // 경우의 레이스 — 그 경우 death 이벤트 자체가 이미 지나가버려
            // 시그널이 오지 않는다. getppid()==1 이면 이미 init 으로
            // reparent 된 것이므로 직접 자결한다.
            if libc::getppid() == 1 {
                return Err(std::io::Error::other("parent already gone before exec"));
            }
            Ok(())
        });
    }

    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("tasty-wh-test-fork-anchor".into())
        .spawn(move || {
            let result = command.spawn();
            let _ = tx.send(result);
            // 절대 반환하지 않는다 — 반환/종료하는 순간이 곧 위 prctl 이
            // 추적하는 "부모 스레드 종료" 라, 그 즉시 방금 띄운 자식이 죽는다.
            loop {
                std::thread::park();
            }
        })
        .expect("spawn fork-anchor thread");
    rx.recv().expect("fork-anchor thread died before replying")
}

/// 한 번의 spawn 에 필요한 입력. [`Builder`] 를 그대로 넘기면 재시도 때마다 소비돼
/// 버려서(홈/파일 목록이 move 된다) 스펙만 빌려 주는 형태로 분리한다.
struct SpawnSpec<'a> {
    /// `None` = 홈의 `webhooks.toml` 에 적힌 값을 쓴다.
    webhook_port: Option<u16>,
    home: Option<PathBuf>,
    env: &'a [(String, String)],
    files: &'a [(String, String)],
}

pub struct WebhookInstance {
    process: Child,
    port: u16,
    webhook_port: u16,
    port_file: PathBuf,
    /// TASTY_HOME (webhooks.toml/hook-handlers.toml 위치).
    home: PathBuf,
    /// HOME (shell rc 격리).
    shell_home: PathBuf,
    /// Drop 시 홈을 삭제할지(빌더에서 명시 home 을 준 재시작 테스트는 false).
    own_home: bool,
    stderr_ring: Arc<Mutex<VecDeque<String>>>,
    /// 마지막 stderr 줄의 시각 — 상한을 넘겼을 때 "느리다" 와 "멈췄다" 를 가른다.
    stderr_last_at: Arc<Mutex<Option<Instant>>>,
    stderr_drain: Option<JoinHandle<()>>,
}

impl WebhookInstance {
    /// 새 포트로 인스턴스를 띄운다. 예약([`free_port`])을 그대로 넘겨받아 spawn 직전까지
    /// 붙들고, 그래도 포트를 뺏기면 새 포트로 다시 띄운다.
    pub fn builder(lease: PortLease) -> Builder {
        Builder {
            webhook_port: Some(lease.port()),
            lease: Some(lease),
            home: None,
            env: Vec::new(),
            files: Vec::new(),
        }
    }

    /// 이미 `webhooks.toml` 이 있는 홈으로 **다시** 띄운다(재시작 시나리오).
    ///
    /// 포트를 인자로 받지 않는다 — 그 값은 홈의 `webhooks.toml` 에 이미 박혀 있고, 웹훅
    /// URL 이 재시작 간 고정이어야 하므로 하네스가 다른 번호를 고를 수도 없다. 호출부가
    /// 번호를 따로 들고 다니면 1 차 인스턴스가 포트를 재시도로 바꿨을 때 그 값이 조용히
    /// 낡는다(실제로 그렇게 한 번 어긋났다). 파일을 SoT 로 두어 그 형태를 없앤다.
    /// [`Builder::home`] 지정이 필수다.
    pub fn builder_for_restart() -> Builder {
        Builder {
            lease: None,
            webhook_port: None,
            home: None,
            env: Vec::new(),
            files: Vec::new(),
        }
    }

    /// 포트를 뺏기면 새 포트로 다시 띄운다. 재시도가 가능한 것은 **우리가 포트를
    /// 골라 시딩했을 때뿐**이다 — 재시작 시나리오는 `webhooks.toml` 의 값이 곧 계약이라
    /// (웹훅 URL 이 재시작 간 고정) 하네스가 번호를 바꿀 수 없다.
    fn spawn_inner(builder: Builder) -> Self {
        let Builder {
            mut lease,
            mut webhook_port,
            home,
            env,
            files,
        } = builder;
        assert!(
            webhook_port.is_some() || home.is_some(),
            "builder_for_restart 는 기존 webhooks.toml 이 있는 홈(.home(..))이 필요하다"
        );
        for attempt in 0..=PORT_STEAL_RETRIES {
            let seeded;
            let instance = {
                let spec = SpawnSpec {
                    webhook_port,
                    home: home.clone(),
                    env: &env,
                    files: &files,
                };
                let (inst, did_seed) = Self::spawn_once(spec, lease.as_mut());
                seeded = did_seed;
                inst
            };
            match instance.wait_webhook_bound() {
                BindOutcome::Bound => return instance,
                BindOutcome::Stolen(line) if seeded && attempt < PORT_STEAL_RETRIES => {
                    // 우리가 고른 포트를 남이 가져갔다. 우리가 시딩한 값이므로 다시 고를
                    // 수 있다 — 인스턴스를 접고(Drop 이 자식을 회수한다) 새 포트로 재시도.
                    spawn_diag::init_test_tracing();
                    tracing::warn!(
                        "webhook port {} was taken; retrying with a new port ({line})",
                        instance.webhook_port
                    );
                    let seeded_file = instance.home.join("webhooks.toml");
                    drop(instance);
                    // 지우지 못하면 다음 시도가 같은 포트를 다시 쓴다 — 그 경우에도
                    // 재시도가 무해하고(같은 실패로 끝난다) 아래 panic 이 원인을 알린다.
                    if let Err(e) = std::fs::remove_file(&seeded_file) {
                        tracing::warn!("could not reset {seeded_file:?}: {e}");
                    }
                    let fresh = free_port();
                    webhook_port = Some(fresh.port());
                    lease = Some(fresh);
                }
                outcome => panic!("{}", instance.bind_failure_report(&outcome, seeded)),
            }
        }
        unreachable!("재시도 루프는 return 또는 panic 으로만 빠져나간다");
    }

    /// 한 번의 spawn. 반환값의 `bool` 은 **이 호출이 `webhooks.toml` 을 새로 썼는지**다
    /// (= 포트를 우리가 정했는지). 재시도 가능 여부의 판정 근거가 된다.
    fn spawn_once(spec: SpawnSpec<'_>, lease: Option<&mut PortLease>) -> (Self, bool) {
        let SpawnSpec {
            webhook_port,
            home: home_arg,
            env: env_args,
            files,
        } = spec;
        let unique = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let port_file = std::env::temp_dir().join(format!("tasty-wh-test-{unique}.port"));

        let (home, own_home) = match home_arg {
            Some(h) => (h, false),
            None => (
                std::env::temp_dir().join(format!("tasty-wh-home-{unique}")),
                true,
            ),
        };
        std::fs::create_dir_all(&home).expect("create TASTY_HOME");

        // shell rc 격리용 HOME 은 항상 고유.
        let shell_home = std::env::temp_dir().join(format!("tasty-wh-shellhome-{unique}"));
        std::fs::create_dir_all(&shell_home).expect("create shell HOME");
        std::fs::write(shell_home.join(".zshrc"), "").ok();
        std::fs::write(shell_home.join(".bashrc"), "").ok();

        // TASTY_HOME 아래 config.toml(shell auto-detect 차단) + webhooks.toml(포트 시딩).
        std::fs::write(home.join("config.toml"), config_toml()).expect("write config.toml");
        // webhooks.toml 가 이미 있으면(재시작 2회차) 덮지 않는다 — 영속 엔트리 보존.
        // `webhooks.toml` 가 이미 있으면 그 파일이 포트의 SoT 다 — 덮지 않고(영속 엔트리
        // 보존) 적힌 값을 읽어 쓴다. 없을 때만 우리가 고른 포트를 시딩한다.
        let webhooks_toml = home.join("webhooks.toml");
        let seeded = !webhooks_toml.exists();
        let webhook_port = if seeded {
            let port = webhook_port.expect("빈 홈에는 하네스가 포트를 골라 주어야 한다");
            std::fs::write(&webhooks_toml, format!("port = {port}\n")).expect("seed webhooks.toml");
            port
        } else {
            read_seeded_port(&webhooks_toml)
        };
        for (name, content) in files {
            std::fs::write(home.join(name), content).expect("write extra tasty file");
        }

        let mut command = Command::new(spawn_diag::instance_bin());
        command
            .arg("--port-file")
            .arg(port_file.to_str().unwrap())
            .env("HOME", &shell_home)
            .env("ZDOTDIR", &shell_home)
            .env("TASTY_HOME", &home)
            .env_remove("OH_MY_ZSH")
            .env_remove("ZSH")
            .env_remove("SHELL")
            .env_remove("TASTY_SURFACE_ID")
            // 실패 tail 을 사람이 읽을 때 "떴는데 connect 실패" 와 "끝내 안 떴다" 를
            // 가르는 리스너 타깃만 `info` 로 올리고 나머지는 공용 필터 그대로 둔다 —
            // stderr ring 이 유한해서 전체 info 를 켜면 진단 줄이 밀려난다.
            // 이름·값의 정의 자리는 `spawn_diag` 하나다.
            .env(spawn_diag::LOG_ENV, spawn_diag::LOG_FILTER_WEBHOOK)
            .stderr(Stdio::piped());
        for (k, v) in env_args {
            command.env(k, v);
        }

        // 예약을 여기서 푼다 — 자식이 같은 포트에 bind 해야 하므로 spawn 전에 반드시
        // 놓아야 하고, 그 전까지는 붙들고 있어야 시딩 구간에서 뺏기지 않는다.
        if let Some(lease) = lease {
            lease.release();
        }

        // 부모(이 test binary)가 어떤 이유로든(SIGKILL 포함) 즉사하면 커널이 이
        // 자식을 대신 죽여준다. 아래 Drop 은 부모가 살아서 unwind 될 때만 자식을
        // 정리하므로, 부모가 그 전에 죽으면 Drop 이 실행되지 않아 자식이 고아로
        // 영구히 남는다 — spawn_with_stable_pdeathsig_anchor 의 doc 을 반드시
        // 함께 읽을 것(스레드 종속성 문제로 naive prctl 은 이 스레드가 먼저
        // 끝나는 사용 패턴을 깨뜨린다).
        #[cfg(target_os = "linux")]
        let mut process =
            spawn_with_stable_pdeathsig_anchor(command).expect("failed to spawn tasty");
        #[cfg(not(target_os = "linux"))]
        let mut process = command.spawn().expect("failed to spawn tasty");

        let stderr_ring: Arc<Mutex<VecDeque<String>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_RING_CAPACITY)));
        // 마지막 줄이 **언제** 왔는지가 "느리다" 와 "멈췄다" 를 가르는 값이다
        // (`spawn_diag::stderr_silence_verdict`). 줄 자체는 링에, 시각은 여기에 남는다.
        let stderr_last_at: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let stderr_drain = process.stderr.take().map(|stderr| {
            let ring = Arc::clone(&stderr_ring);
            let last_at = Arc::clone(&stderr_last_at);
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    *last_at.lock().unwrap() = Some(Instant::now());
                    let mut ring = ring.lock().unwrap();
                    if ring.len() == STDERR_RING_CAPACITY {
                        ring.pop_front();
                    }
                    ring.push_back(line);
                }
            })
        });

        let start = Instant::now();
        let port = loop {
            if start.elapsed() > SPAWN_PORT_TIMEOUT {
                panic!(
                    "{}",
                    spawn_diag::spawn_timeout_message(
                        "tasty failed to start",
                        SPAWN_PORT_TIMEOUT,
                        STDERR_TAIL_LINES,
                        &stderr_tail(&stderr_ring, STDERR_TAIL_LINES),
                        stderr_last_at.lock().unwrap().map(|t| t.elapsed()),
                    )
                );
            }
            if let Ok(content) = std::fs::read_to_string(&port_file)
                && let Ok(port) = content.trim().parse::<u16>()
            {
                break port;
            }
            // 자식이 이미 죽었으면 상한을 기다리지 않는다 (`tests/common` 과 동일).
            if let Ok(Some(status)) = process.try_wait() {
                panic!(
                    "{}",
                    spawn_diag::early_exit_message(
                        &status.to_string(),
                        STDERR_TAIL_LINES,
                        &stderr_tail(&stderr_ring, STDERR_TAIL_LINES),
                    )
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        };

        let instance = Self {
            process,
            port,
            webhook_port,
            port_file,
            home,
            shell_home,
            own_home,
            stderr_ring: Arc::clone(&stderr_ring),
            stderr_last_at: Arc::clone(&stderr_last_at),
            stderr_drain,
        };

        // 셸이 실제 출력을 낼 때까지 대기(surface 준비).
        let start = Instant::now();
        loop {
            let text = instance.screen_text_of(instance.first_surface_id());
            if !text.trim().is_empty() {
                break;
            }
            if start.elapsed() > SPAWN_SHELL_TIMEOUT {
                panic!(
                    "{}",
                    spawn_diag::spawn_timeout_message(
                        "shell produced no output",
                        SPAWN_SHELL_TIMEOUT,
                        STDERR_TAIL_LINES,
                        &stderr_tail(&instance.stderr_ring, STDERR_TAIL_LINES),
                        instance.stderr_last_at.lock().unwrap().map(|t| t.elapsed()),
                    )
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        (instance, seeded)
    }

    pub fn webhook_port(&self) -> u16 {
        self.webhook_port
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn port_file(&self) -> &Path {
        &self.port_file
    }

    /// 웹훅 리스너가 실제로 accept 할 때까지 TCP connect 로 대기한다. bind 실패면
    /// stderr tail 과 함께 panic.
    /// 이 인스턴스의 TASTY_HOME 디렉터리 (config/hook-handlers.toml 등의 루트).
    /// 스폰 후 설정 파일을 다시 써서 `*.reload` IPC 로 반영할 때 쓴다.
    #[allow(dead_code)] // 일부 test binary 만 사용
    pub fn tasty_home(&self) -> &std::path::Path {
        &self.home
    }

    /// 리스너가 accept 를 시작할 때까지 기다린다. spawn 이 이미 같은 대기를 마쳤으므로
    /// 보통 즉시 돌아온다 — 호출부 가독성(무엇을 전제하는지)을 위해 남긴 진입점이다.
    pub fn wait_webhook_ready(&self) {
        match self.wait_webhook_bound() {
            BindOutcome::Bound => {}
            outcome => panic!("{}", self.bind_failure_report(&outcome, false)),
        }
    }

    /// accept 시작 / 포트 도난 / 무응답 셋 중 하나로 판정한다. 도난은 **추측이 아니라**
    /// 리스너가 남긴 bind 실패 경고로 확정한다 — 이 구분이 없으면 두 원인이 똑같이
    /// "웹훅이 안 떴다" 로만 보인다.
    fn wait_webhook_bound(&self) -> BindOutcome {
        let start = Instant::now();
        let addr = format!("127.0.0.1:{}", self.webhook_port);
        loop {
            if TcpStream::connect(&addr).is_ok() {
                return BindOutcome::Bound;
            }
            if let Some(line) = self.stderr_bind_failure() {
                return BindOutcome::Stolen(line);
            }
            if start.elapsed() > WEBHOOK_READY_TIMEOUT {
                return BindOutcome::Silent;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// stderr 링에서 리스너 bind 실패 경고 한 줄을 찾는다.
    fn stderr_bind_failure(&self) -> Option<String> {
        let ring = self.stderr_ring.lock().unwrap();
        ring.iter().find(|line| is_bind_failure(line)).cloned()
    }

    /// 실패 원인을 사람이 바로 읽을 수 있게 조립한다. `seeded` 는 이 인스턴스의 포트를
    /// 하네스가 골랐는지 — 재시도가 가능했는지 여부라 메시지의 결론이 달라진다.
    fn bind_failure_report(&self, outcome: &BindOutcome, seeded: bool) -> String {
        let port = self.webhook_port;
        let head = match outcome {
            BindOutcome::Bound => {
                "웹훅 리스너는 정상 bind 됐다(진단 조립이 잘못 불렸다)".to_string()
            }
            BindOutcome::Stolen(line) if seeded => format!(
                "웹훅 포트 {port} 를 제3자가 가져갔다 — 재시도 {PORT_STEAL_RETRIES} 회를 모두 소진했다. \
                 다른 워크트리/테스트가 같은 순간 같은 번호를 받은 경우다. 리스너 경고: {line}"
            ),
            BindOutcome::Stolen(line) => format!(
                "웹훅 포트 {port} 에 bind 하지 못했다. 이 인스턴스는 이미 있는 webhooks.toml 의 \
                 포트를 그대로 쓰므로(재시작 시나리오 — URL 이 재시작 간 고정이어야 한다) \
                 하네스가 다른 번호를 고를 수 없다. 리스너 경고: {line}"
            ),
            BindOutcome::Silent => format!(
                "웹훅 리스너가 {WEBHOOK_READY_TIMEOUT:?} 안에 포트 {port} 에서 accept 하지 않았다. \
                 bind 실패 경고는 없다 — 포트 경합이 아니라 부팅 지연이나 리스너 init 미호출 쪽이다."
            ),
        };
        format!(
            "{head}\n--- stderr (last {STDERR_TAIL_LINES} lines) ---\n{}",
            stderr_tail(&self.stderr_ring, STDERR_TAIL_LINES)
        )
    }

    // ── IPC ─────────────────────────────────────────────────────────────

    /// JSON-RPC 요청 → result 값. 오류면 panic.
    pub fn call(&self, method: &str, params: Value) -> Value {
        let resp = self.call_raw(method, params);
        if let Some(error) = resp.get("error").filter(|e| !e.is_null()) {
            panic!("IPC error for '{method}': {error}");
        }
        resp.get("result").cloned().unwrap_or(Value::Null)
    }

    /// JSON-RPC 요청 → 전체 응답(오류 포함).
    pub fn call_raw(&self, method: &str, params: Value) -> Value {
        let mut stream =
            TcpStream::connect(format!("127.0.0.1:{}", self.port)).expect("connect to tasty IPC");
        stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
        let request = serde_json::json!({
            "jsonrpc": "2.0", "method": method, "params": params, "id": 1
        });
        let mut msg = serde_json::to_string(&request).unwrap();
        msg.push('\n');
        stream.write_all(msg.as_bytes()).expect("send IPC");
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader.read_line(&mut line).expect("read IPC response");
        serde_json::from_str(&line).expect("valid JSON response")
    }

    pub fn first_surface_id(&self) -> u64 {
        let surfaces = self.call("surface.list", serde_json::json!({}));
        surfaces.as_array().unwrap()[0]["id"].as_u64().unwrap()
    }

    pub fn screen_text_of(&self, surface_id: u64) -> String {
        let result = self.call(
            "surface.screen_text",
            serde_json::json!({ "surface_id": surface_id }),
        );
        result
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string()
    }

    // ── 실 HTTP ─────────────────────────────────────────────────────────

    /// 웹훅 리스너에 실 HTTP 요청을 보낸다. `path` 는 `/{id}` 또는 `id`(둘 다 허용,
    /// 쿼리 포함 가능). 반환: `(status_code, body_string)`.
    pub fn http(&self, method: &str, path: &str, body: &str) -> (u16, String) {
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        let addr = format!("127.0.0.1:{}", self.webhook_port);
        let mut stream = TcpStream::connect(&addr).expect("connect webhook listener");
        stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
            len = body.len()
        );
        stream.write_all(request.as_bytes()).expect("write HTTP");
        stream.flush().ok();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).ok();
        parse_http_response(&raw)
    }

    pub fn post(&self, path: &str, body: &str) -> (u16, String) {
        self.http("POST", path, body)
    }

    /// 헤더를 추가한 HTTP 요청(Bearer/커스텀 헤더 인증 테스트용).
    pub fn http_with_headers(
        &self,
        method: &str,
        path: &str,
        extra_headers: &[(&str, &str)],
        body: &str,
    ) -> (u16, String) {
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        let addr = format!("127.0.0.1:{}", self.webhook_port);
        let mut stream = TcpStream::connect(&addr).expect("connect webhook listener");
        stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
        let mut extra = String::new();
        for (k, v) in extra_headers {
            extra.push_str(&format!("{k}: {v}\r\n"));
        }
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n{extra}Content-Length: {len}\r\nConnection: close\r\n\r\n{body}",
            len = body.len()
        );
        stream.write_all(request.as_bytes()).expect("write HTTP");
        stream.flush().ok();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).ok();
        parse_http_response(&raw)
    }

    // ── CLI ─────────────────────────────────────────────────────────────

    /// `<instance_bin> <args> --port-file <this instance>` 를 CLI 클라이언트로
    /// 실행한다(실 바이너리 → 실 IPC). stdout/stderr/exit 을 담아 반환.
    pub fn cli(&self, args: &[&str]) -> Output {
        // `--port-file` 은 전역(top-level) 플래그라 서브커맨드 **앞**에 와야 한다.
        Command::new(spawn_diag::instance_bin())
            .arg("--port-file")
            .arg(self.port_file.to_str().unwrap())
            .args(args)
            .env_remove("TASTY_SURFACE_ID")
            // 바깥 tasty 세션의 stale 토큰이 상속되면 서버가 거부하므로 제거한다 →
            // token 없는 local caller 로 붙는다(권한 면제).
            .env_remove("TASTY_SESSION_TOKEN")
            .env("TASTY_HOME", &self.home)
            .output()
            .expect("run tasty CLI")
    }

    pub fn shutdown(&self) {
        let _ = self.call_raw("system.shutdown", serde_json::json!({}));
    }
}

impl Drop for WebhookInstance {
    fn drop(&mut self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.shutdown();
        }));
        std::thread::sleep(Duration::from_millis(200));
        #[cfg(target_os = "windows")]
        {
            let pid = self.process.id();
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = self.process.kill();
        }
        let _ = self.process.wait();
        if let Some(handle) = self.stderr_drain.take() {
            let _ = handle.join();
        }
        let _ = std::fs::remove_file(&self.port_file);
        let _ = std::fs::remove_dir_all(&self.shell_home);
        if self.own_home {
            let _ = std::fs::remove_dir_all(&self.home);
        }
    }
}

/// 기존 `webhooks.toml` 에서 `port = N` 을 읽는다. 이 파일이 있는 홈에서는 이 값이
/// 리스너가 실제로 bind 할 포트이므로, 하네스도 반드시 같은 값을 봐야 한다.
fn read_seeded_port(path: &Path) -> u16 {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    text.lines()
        .find_map(|line| line.split_once('=').filter(|(k, _)| k.trim() == "port"))
        .and_then(|(_, v)| v.trim().parse::<u16>().ok())
        .unwrap_or_else(|| panic!("{} 에 `port = N` 이 없다:\n{text}", path.display()))
}

fn stderr_tail(ring: &Arc<Mutex<VecDeque<String>>>, n: usize) -> String {
    let ring = ring.lock().unwrap();
    let total = ring.len();
    let start = total.saturating_sub(n);
    ring.iter()
        .skip(start)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

/// 원시 HTTP 응답 바이트에서 `(status_code, body)` 를 뽑는다. 헤더/바디는 첫
/// `\r\n\r\n` 로 나눈다. status 는 첫 줄 `HTTP/1.1 <code> <reason>` 에서 파싱.
fn parse_http_response(raw: &[u8]) -> (u16, String) {
    let text = String::from_utf8_lossy(raw);
    let mut lines = text.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let body = match text.split_once("\r\n\r\n") {
        Some((_, b)) => b.to_string(),
        None => String::new(),
    };
    (code, body)
}

/// CLI Output 의 stdout 을 UTF-8 문자열로.
pub fn stdout_str(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

pub fn stderr_str(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[cfg(test)]
mod bind_detection_tests {
    use super::*;

    /// `src/webhook/listener.rs` 가 실제로 내는 세 문구. 하나라도 오분류되면 재시도
    /// 판정이 뒤집힌다 — 성공을 도난으로 읽으면 멀쩡한 인스턴스를 버리고, 도난을
    /// 침묵으로 읽으면 재시도 없이 실패한다.
    #[test]
    fn only_the_bind_failure_line_counts_as_a_steal() {
        assert!(is_bind_failure(
            "WARN tasty::webhook::listener: webhook listener bind 0.0.0.0:28429 failed: \
             Address already in use (os error 98) — set a free port and check firewall"
        ));
        assert!(!is_bind_failure(
            "INFO tasty::webhook::listener: webhook listener bound on 0.0.0.0:28429"
        ));
        assert!(!is_bind_failure(
            "DEBUG tasty::webhook::listener: webhook listener already bound; skip re-init"
        ));
    }

    /// 재시도 횟수가 0 으로 되돌아가면 도난 대응이 사라진다 — 그 변경에는 아무
    /// 컴파일 오류도 테스트 실패도 따라붙지 않으므로 여기서 고정한다. 상한이 있는
    /// 이유는 도난이 계속되는 상황(누가 그 대역을 계속 쓴다)에서 무한 재시도가
    /// 원인을 감추기 때문이다.
    #[test]
    fn a_stolen_port_is_retried_a_bounded_number_of_times() {
        assert!(
            PORT_STEAL_RETRIES > 0,
            "도난 대응이 없으면 예약만으로는 부족하다"
        );
        assert!(
            PORT_STEAL_RETRIES <= 3,
            "재시도가 길어지면 원인이 로그에 묻힌다"
        );
    }
}
