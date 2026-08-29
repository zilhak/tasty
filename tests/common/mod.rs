//! IPC e2e 테스트 하네스 — 실 tasty 바이너리를 spawn 해 JSON-RPC 로 조작한다.
//!
//! 진입점이 둘이다.
//!
//! * [`shared()`] — **test binary 하나가 공유하는** 인스턴스. 첫 호출에서만
//!   프로세스를 띄우고 이후 호출은 같은 핸들을 돌려준다. 테스트별 격리는
//!   [`TastyInstance::create_workspace`] 로 자기 workspace 를 만들어 확보한다.
//! * [`TastyInstance::spawn`] — 전용 인스턴스. 프로세스 기동 시점 설정이
//!   달라야 하거나(`spawn_with_inherit_cwd`), 프로세스 전체를 외부에서
//!   측정해야 할 때(soak) 쓴다.
//!
//! 격리 전략과 timeout 정책은 [`docs/dev-guide/e2e-tests.md`].

// 다중 test binary 가 공유하는 test-support 모듈 — binary 마다 사용하는 부분집합이
// 달라 개별 binary 기준 dead_code 판정이 무의미하다 (의도된 superset API).
#![allow(dead_code)]

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

const STDERR_RING_CAPACITY: usize = 256;
const STDERR_TAIL_LINES: usize = 30;

// dev cold path worst-case ~4 s + plugin/theme/gpu init 마진 + dev profile
// (~3.5x release) + self-hosted runner 변동 폭을 흡수하기 위한 timeout.
// S1=port file 작성 (init_app_state 후), S2=first surface PTY prompt.
const SPAWN_PORT_TIMEOUT: Duration = Duration::from_secs(30);
const SPAWN_SHELL_TIMEOUT: Duration = Duration::from_secs(15);

// ───── 공유 인스턴스 (test binary 단위) ─────

static SHARED_INSTANCE: OnceLock<TastyInstance> = OnceLock::new();
/// `shared()` 가 실제로 프로세스를 띄운 횟수 — 하네스 자체 검증용(항상 1 이어야).
static SHARED_SPAWN_COUNT: AtomicUsize = AtomicUsize::new(0);
/// 첫 spawn 이 panic 으로 끝났는지. `OnceLock::get_or_init` 은 초기화 클로저가
/// panic 하면 미초기화 상태로 남아 **다음 테스트가 그대로 재시도**한다 — 부팅이
/// timeout 되는 상황(과부하 머신 등)에서 테스트 수만큼 GUI 프로세스가 더 뜨는
/// 증폭을 막기 위해, 한 번 실패하면 이후 호출은 즉시 실패시킨다.
static SHARED_SPAWN_FAILED: AtomicBool = AtomicBool::new(false);

/// 이 test binary 가 공유하는 tasty 인스턴스. 첫 호출에서만 프로세스를 spawn 하고
/// 이후 호출은 같은 핸들을 돌려준다.
///
/// **공유 범위는 "test binary 하나"다 — `cargo test` 전체가 아니다.** `OnceLock` 은
/// 프로세스 로컬 정적 상태이고 cargo 는 test 타겟마다 별도 프로세스를 띄우므로,
/// 이 하네스로 도달 가능한 하한은 *바이너리당 인스턴스 1개*다. 저장소 전체를
/// 1개로 줄이려면 test binary 개수 자체를 줄여야 한다.
///
/// **lock 을 잡지 않는다.** `gui_common::shared()` 는 `MutexGuard` 를 돌려줘 테스트를
/// 완전 직렬화하지만(실제 데스크톱 마우스/포커스를 뺏는 입력 주입을 쓰므로 직렬화가
/// 필수다), 이쪽은 IPC 만 쓴다 — IPC 서버는 연결마다 별도 스레드로 받아 mpsc 로
/// 큐잉하므로 동시 호출이 안전하고, 테스트 간 격리는 [`TastyInstance::create_workspace`]
/// 로 각자 자기 workspace 를 잡아 확보한다(attach 점유도 workspace/surface 단위 lock).
/// 따라서 `&'static` 핸들만 공유하고 테스트는 그대로 병렬 실행한다.
///
/// **주의 — workspace 로 격리되지 않는 전역 상태가 있다.** headless PTY(`pty.*`),
/// `global_hook.*`, notification 은 전역 목록이라 같은 binary 의 다른 테스트가 만든
/// 항목까지 함께 조회된다. 공유 인스턴스 위에서 목록을 검증할 때는 "내 것이 있는가"
/// (`any`) 형태로 쓰고, 길이나 `[0]` 번째를 assert 하지 않는다.
pub fn shared() -> &'static TastyInstance {
    SHARED_INSTANCE.get_or_init(|| {
        assert!(
            !SHARED_SPAWN_FAILED.swap(true, Ordering::SeqCst),
            "공유 인스턴스 spawn 이 이미 실패했다 — 재시도하지 않는다 (첫 실패의 panic 메시지에 stderr tail 이 붙어 있다)"
        );
        let instance = TastyInstance::spawn();
        SHARED_SPAWN_FAILED.store(false, Ordering::SeqCst);
        SHARED_SPAWN_COUNT.fetch_add(1, Ordering::Relaxed);
        // 정적 저장이라 `Drop` 이 영원히 돌지 않는다 — 정리 경로를 프로세스 종료
        // 시점으로 분리한다. libtest 는 `process::exit` 로 끝나므로 atexit 가 돈다.
        // SAFETY: atexit 는 process-lifetime callback 등록. `on_exit` 는 'static fn
        // 포인터이고, `get_or_init` 안이라 중복 등록되지 않는다.
        unsafe {
            libc::atexit(on_shared_exit);
        }
        instance
    })
}

/// `shared()` 가 프로세스를 spawn 한 횟수. 재사용이 실제로 일어났는지(=1) 를
/// 하네스 자체 테스트가 확인하는 용도.
pub fn shared_spawn_count() -> usize {
    SHARED_SPAWN_COUNT.load(Ordering::Relaxed)
}

extern "C" fn on_shared_exit() {
    if let Some(instance) = SHARED_INSTANCE.get() {
        instance.terminate();
    }
}

/// 프로세스 트리 강제 종료. `Drop`(전용 인스턴스)과 atexit(공유 인스턴스)이 함께 쓴다.
fn force_kill(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        // 이미 종료된 pid 면 taskkill 이 실패로 끝난다 — 목적(살아있으면 죽인다)은
        // 어느 쪽이든 달성되므로 status 를 보지 않는다.
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(target_os = "windows"))]
    // SAFETY: SIGKILL 송신은 thread-safe POSIX. pid 가 이미 종료된 상태여도
    // kill 은 errno 만 set 하고 UB 가 없다.
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
}

/// `workspace.create` 응답이 그대로 돌려준 테스트 전용 격리 단위
/// ([`TastyInstance::create_workspace`] 참조).
///
/// 이 workspace 는 테스트가 끝나도 회수하지 않는다 — `workspace.close` IPC 자체가
/// 없고, 공유 인스턴스가 test 프로세스와 함께 죽으므로 회수할 이유도 없다. 대신
/// **각 테스트가 자기 workspace 밖을 건드리지 않는 것**이 격리의 전부다.
#[derive(Debug, Clone, Copy)]
pub struct TestWorkspace {
    /// workspace id — attach 점유/조회의 대상 키.
    pub id: u64,
    /// `engine.workspaces` 내 인덱스 (생성 시점 기준).
    pub index: usize,
    /// 이 workspace 와 함께 생성된 첫 surface id.
    pub surface_id: u64,
}

pub struct TastyInstance {
    process: Child,
    port: u16,
    port_file: PathBuf,
    isolated_home: PathBuf,
    stderr_ring: Arc<Mutex<VecDeque<String>>>,
    stderr_drain: Option<JoinHandle<()>>,
}

fn write_isolated_config(isolated_home: &std::path::Path, inherit_cwd: bool) {
    let tasty_dir = isolated_home.join(".tasty");
    std::fs::create_dir_all(&tasty_dir).expect("failed to create isolated .tasty dir");

    let shell_path = if cfg!(windows) {
        // Git Bash 표준 경로. 미설치면 host 의 auto-detect 로 떨어지지만 본 phase 의
        // primary target 은 macOS/Linux 의 `cargo test --workspace` flaky 해소.
        "C:/Program Files/Git/bin/bash.exe"
    } else {
        // /bin/sh 는 모든 POSIX 환경에서 보장 — 가장 보수적.
        "/bin/sh"
    };

    let config = format!(
        r#"[general]
shell = "{shell}"
shell_mode = "default"
shell_args = ""
startup_command = ""
language = "en"
scrollback_lines = 10000
confirm_close_running = false
click_to_move_cursor = true
inherit_cwd = {inherit_cwd}
close_behavior = "ask"
restore_layout = false
restore_surface_content = false
link_click_modifier = "ctrl"
"#,
        shell = shell_path,
        inherit_cwd = inherit_cwd
    );
    std::fs::write(tasty_dir.join("config.toml"), config)
        .expect("failed to write isolated config.toml");
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

impl TastyInstance {
    /// **전용** 인스턴스를 새로 띄운다. 테스트마다 GUI 창이 하나씩 더 뜨므로,
    /// 전용 프로세스가 꼭 필요한 경우가 아니면 [`shared()`] 를 쓴다
    /// (전용이 맞는 경우: 프로세스 기동 시점 config 이 달라야 하거나
    /// — [`Self::spawn_with_inherit_cwd`] — 프로세스 RSS 를 외부에서 재는 soak 하네스).
    pub fn spawn() -> Self {
        Self::spawn_with_inherit_cwd(false)
    }

    /// `inherit_cwd` 설정만 바꿔 띄우는 변형. 이 설정이 게이트하는 동작(convert /
    /// split 의 cwd carry)을 실제 서버 프로세스 상대로 검증할 때 쓴다.
    ///
    /// **이 경로는 공유 대상이 아니다** — `inherit_cwd` 는 격리 HOME 의
    /// `config.toml` 에 미리 써넣는 *프로세스 기동 시점* 설정이라, 이미 떠 있는
    /// 인스턴스에 런타임으로 바꿔 끼울 수 없다. 값이 다른 인스턴스가 필요하면
    /// 항상 별도 프로세스로 남는다.
    pub fn spawn_with_inherit_cwd(inherit_cwd: bool) -> Self {
        let unique = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let port_file = std::env::temp_dir().join(format!("tasty-test-{}.port", unique));

        // 격리된 HOME — 사용자의 ~/.zshrc / oh-my-zsh / p10k / ~/.tasty/ 등이
        // e2e PTY shell 에 새어들어와 prompt 형태와 ZLE binding 을 바꾸는 일을
        // 차단한다. tasty 본 바이너리의 paths 도 모두 HOME 기반이라 같이 격리됨.
        let isolated_home = std::env::temp_dir().join(format!("tasty-test-home-{}", unique));
        std::fs::create_dir_all(&isolated_home).expect("failed to create isolated home");
        // 빈 .zshrc — zsh 가 사용자 customization 안 보게. ZDOTDIR 이 이 디렉토리를
        // 가리키므로 zsh 는 여기서 rc 를 찾는다.
        std::fs::write(isolated_home.join(".zshrc"), "").ok();
        std::fs::write(isolated_home.join(".bashrc"), "").ok();

        // 격리된 ~/.tasty/config.toml 사전 작성 — shell auto-detect 분기를 결정적으로
        // 차단하여 host /etc/passwd 와 $SHELL 의존을 제거한다. shell_setup_mode 진입
        // 경로를 막아 port file 이 항상 작성되도록 보장한다.
        write_isolated_config(&isolated_home, inherit_cwd);

        let mut process = Command::new(env!("CARGO_BIN_EXE_tasty"))
            .arg("--port-file")
            .arg(port_file.to_str().unwrap())
            .env("HOME", &isolated_home)
            // HOME 만으로는 Windows 에서 격리되지 않는다 — tasty 루트 해석
            // (tasty-utils path.rs)은 directories::BaseDirs(=USERPROFILE) 기반이라
            // 실사용자의 ~/.tasty-debug 를 읽어 세션 복원·설정이 새어든다.
            // TASTY_HOME 이 루트 override 의 SoT 이므로 명시 지정 (전 OS 일관).
            .env("TASTY_HOME", isolated_home.join(".tasty"))
            .env("ZDOTDIR", &isolated_home)
            .env_remove("OH_MY_ZSH")
            .env_remove("ZSH")
            .env_remove("SHELL")
            // 부모 프로세스가 tasty 안에서 실행 중이면 TASTY_SURFACE_ID 가 상속되어
            // child 가 augmented help 만 출력하고 종료한다 (boot/cli_routing.rs:55).
            // 자식은 항상 본 GUI 로 부팅해야 하므로 명시 제거.
            .env_remove("TASTY_SURFACE_ID")
            // host 의 RUST_LOG=debug/trace 가 새어들어와 child 가 polled stderr
            // 보다 빠르게 write 하면 OS pipe buffer 가 가득 차서 child 가 block
            // 될 위험이 있다. drain thread 가 1차 방어, verbosity cap 이 2차.
            .env("RUST_LOG", "tasty=info")
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn tasty");

        // drain stderr into a ring buffer so we can attach a tail to spawn-phase
        // panics. background thread avoids OS pipe backpressure blocking the child
        // (Linux 64 KB / macOS 16 KB default).
        let stderr_ring: Arc<Mutex<VecDeque<String>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_RING_CAPACITY)));
        let stderr_drain = process.stderr.take().map(|stderr| {
            let ring = Arc::clone(&stderr_ring);
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    let mut ring = ring.lock().unwrap();
                    if ring.len() == STDERR_RING_CAPACITY {
                        ring.pop_front();
                    }
                    ring.push_back(line);
                }
            })
        });

        // Wait for port file
        let start = Instant::now();
        let port = loop {
            if start.elapsed() > SPAWN_PORT_TIMEOUT {
                // 아직 `Self` 를 못 만들어 `Drop` 이 없다 — `Child` 의 Drop 은 kill 하지
                // 않으므로 여기서 직접 회수하지 않으면 GUI 프로세스가 그대로 orphan 이
                // 된다(느린 머신에서 부팅이 timeout 을 넘겨 뒤늦게 뜨는 경우).
                force_kill(process.id());
                // 아래 셋 다 실패해도 할 수 있는 게 없다 — 곧바로 panic 으로 테스트를
                // 실패시키므로 회수 실패를 추가로 보고할 자리도 의미도 없다.
                let _ = process.wait();
                // 회수 실패해도 곧바로 panic 이라 보고할 자리가 없다.
                let _ = std::fs::remove_file(&port_file);
                // 위와 동일.
                let _ = std::fs::remove_dir_all(&isolated_home);
                panic!(
                    "tasty failed to start within {:?}.\n--- stderr (last {} lines) ---\n{}",
                    SPAWN_PORT_TIMEOUT,
                    STDERR_TAIL_LINES,
                    stderr_tail(&stderr_ring, STDERR_TAIL_LINES)
                );
            }
            if let Ok(content) = std::fs::read_to_string(&port_file)
                && let Ok(port) = content.trim().parse::<u16>()
            {
                break port;
            }
            std::thread::sleep(Duration::from_millis(100));
        };

        let instance = Self {
            process,
            port,
            port_file,
            isolated_home,
            stderr_ring: Arc::clone(&stderr_ring),
            stderr_drain,
        };

        // Wait until the shell is actually ready (has screen content).
        instance.wait_for_shell(instance.first_surface_id());

        instance
    }

    /// 해당 surface 의 PTY 가 첫 출력(prompt)을 낼 때까지 폴링한다. spawn 직후의
    /// 첫 surface 뿐 아니라, [`Self::create_workspace`] 로 갓 만든 workspace 의
    /// surface 를 쓰기 전에도 호출한다.
    pub fn wait_for_shell(&self, surface_id: u64) {
        let start = Instant::now();
        loop {
            let text = self.screen_text_of(surface_id);
            if !text.trim().is_empty() {
                return;
            }
            if start.elapsed() > SPAWN_SHELL_TIMEOUT {
                panic!(
                    "shell did not produce output within {:?}.\n--- stderr (last {} lines) ---\n{}",
                    SPAWN_SHELL_TIMEOUT,
                    STDERR_TAIL_LINES,
                    stderr_tail(&self.stderr_ring, STDERR_TAIL_LINES)
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Send a JSON-RPC request and return the result value.
    /// Retries on timeout (event loop may be slow when window is unfocused).
    pub fn call(&self, method: &str, params: Value) -> Value {
        for attempt in 0..3 {
            let mut stream = match TcpStream::connect(format!("127.0.0.1:{}", self.port)) {
                Ok(s) => s,
                Err(e) => {
                    if attempt < 2 {
                        std::thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                    panic!("failed to connect for '{}': {}", method, e);
                }
            };
            stream.set_read_timeout(Some(Duration::from_secs(10))).ok();

            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
                "id": 1
            });

            let mut msg = serde_json::to_string(&request).unwrap();
            msg.push('\n');
            if stream.write_all(msg.as_bytes()).is_err() {
                if attempt < 2 {
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
                panic!("failed to send for '{}'", method);
            }

            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(_) => {}
                Err(_) if attempt < 2 => {
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
                Err(e) => panic!("failed to read response for '{}': {}", method, e),
            }

            let resp: Value = serde_json::from_str(&line).expect("invalid JSON response");
            if let Some(error) = resp.get("error") {
                panic!("IPC error for '{}': {}", method, error);
            }
            return resp.get("result").cloned().unwrap_or(Value::Null);
        }
        unreachable!()
    }

    /// Send a JSON-RPC request and return the full response (including errors).
    #[allow(dead_code)] // 일부 test binary 만 사용
    pub fn call_raw(&self, method: &str, params: Value) -> Value {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", self.port))
            .expect("failed to connect to tasty IPC");
        stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });
        let mut msg = serde_json::to_string(&request).unwrap();
        msg.push('\n');
        stream.write_all(msg.as_bytes()).expect("failed to send");
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("failed to read response");
        serde_json::from_str(&line).expect("invalid JSON response")
    }

    /// 테스트 전용 workspace 를 만들고 `workspace.create` 응답의 `id` / `index` /
    /// `surface_id` 를 그대로 돌려준다 — [`shared()`] 인스턴스 위에서 테스트가
    /// 서로의 상태를 밟지 않게 하는 격리 단위다.
    ///
    /// IPC 로 만든 workspace 는 `IntentOrigin::Agent` 라 active 를 전환하지 않으므로
    /// (원칙 1·3), 여러 테스트가 병렬로 호출해도 서로의 active 상태를 흔들지 않는다.
    /// 반환된 surface 의 PTY 가 필요하면 [`Self::wait_for_shell`] 로 기다린다.
    pub fn create_workspace(&self, name: &str) -> TestWorkspace {
        let result = self.call("workspace.create", serde_json::json!({ "name": name }));
        TestWorkspace {
            id: result["id"].as_u64().expect("workspace.create returns id"),
            index: result["index"]
                .as_u64()
                .expect("workspace.create returns index") as usize,
            surface_id: result["surface_id"]
                .as_u64()
                .expect("workspace.create returns surface_id"),
        }
    }

    /// Get the first surface ID from surface.list.
    ///
    /// **전용 인스턴스 전용.** [`shared()`] 위에서 쓰면 다른 테스트가 만든 surface 를
    /// 집어 비결정적이 된다 — 공유 경로에서는 [`Self::first_surface_id_in_workspace`]
    /// 또는 [`TestWorkspace::surface_id`] 를 쓴다.
    pub fn first_surface_id(&self) -> u64 {
        let surfaces = self.call("surface.list", serde_json::json!({}));
        surfaces.as_array().unwrap()[0]["id"].as_u64().unwrap()
    }

    /// Get the first pane ID from pane.list.
    ///
    /// **전용 인스턴스 전용** — 근거는 [`Self::first_surface_id`] 와 같다. 공유
    /// 경로에서는 [`Self::first_pane_id_in_workspace`] 를 쓴다.
    pub fn first_pane_id(&self) -> u64 {
        let panes = self.call("pane.list", serde_json::json!({}));
        panes.as_array().unwrap()[0]["id"].as_u64().unwrap()
    }

    /// 주어진 workspace 에 속한 첫 surface id — 공유 인스턴스용 `first_surface_id`.
    pub fn first_surface_id_in_workspace(&self, workspace_id: u64) -> u64 {
        let surfaces = self.call("surface.list", serde_json::json!({}));
        surfaces
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["workspace_id"].as_u64() == Some(workspace_id))
            .unwrap_or_else(|| panic!("no surface in workspace {workspace_id}"))["id"]
            .as_u64()
            .unwrap()
    }

    /// 주어진 workspace 에 속한 첫 pane id — 공유 인스턴스용 `first_pane_id`.
    pub fn first_pane_id_in_workspace(&self, workspace_id: u64) -> u64 {
        let panes = self.call("pane.list", serde_json::json!({}));
        panes
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["workspace_id"].as_u64() == Some(workspace_id))
            .unwrap_or_else(|| panic!("no pane in workspace {workspace_id}"))["id"]
            .as_u64()
            .unwrap()
    }

    /// Send text to a specific surface.
    pub fn send_text(&self, surface_id: u64, text: &str) {
        self.call(
            "surface.send",
            serde_json::json!({ "surface_id": surface_id, "text": text }),
        );
    }

    /// Set a read mark on a specific surface.
    pub fn set_mark(&self, surface_id: u64) {
        self.call(
            "surface.set_mark",
            serde_json::json!({ "surface_id": surface_id }),
        );
    }

    /// Read output since the last mark, stripping ANSI.
    pub fn read_since_mark(&self, surface_id: u64) -> String {
        let result = self.call(
            "surface.read_since_mark",
            serde_json::json!({ "surface_id": surface_id, "strip_ansi": true }),
        );
        result
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string()
    }

    /// Wait until read_since_mark contains the expected text (with timeout).
    pub fn wait_for_output(&self, surface_id: u64, expected: &str, timeout: Duration) -> String {
        let start = Instant::now();
        loop {
            let output = self.read_since_mark(surface_id);
            if output.contains(expected) {
                return output;
            }
            if start.elapsed() > timeout {
                panic!(
                    "timeout waiting for '{}' in output. Got:\n{}",
                    expected, output
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Get screen text of a specific surface.
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

    /// OS process id — soak 하네스의 외부 측정(프로세스 트리 RSS/핸들 수)용.
    #[allow(dead_code)] // 일부 test binary 만 사용
    pub fn pid(&self) -> u32 {
        self.process.id()
    }

    /// Loopback IPC port — attach stream 핸드셰이크처럼 `call()` 이 감싸지 않는
    /// raw `TcpStream` 을 직접 여는 test binary 용.
    #[allow(dead_code)] // 일부 test binary 만 사용
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Shutdown the instance gracefully.
    pub fn shutdown(&self) {
        let _ = self.call("system.shutdown", serde_json::json!({}));
    }

    /// 공유 인스턴스 정리 경로 — atexit 에서 호출한다.
    ///
    /// `Drop` 과 달리 `&self` 만 가진다(정적 저장이라 `&mut` 를 얻을 수 없다).
    /// 그래서 `Child::kill`/`wait` 대신 pid 기반 kill 을 쓴다 — 어차피 test
    /// 프로세스가 종료하는 중이라 reap 은 init 이 대신한다.
    fn terminate(&self) {
        // graceful shutdown 은 best-effort — 이미 죽었거나 응답이 없으면 `call` 이
        // panic 하는데, 뒤이은 force kill 이 어차피 회수하므로 삼킨다.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.shutdown();
        }));
        std::thread::sleep(Duration::from_millis(200));
        force_kill(self.process.id());
        // atexit 안이라 로깅 대상(테스트 출력)이 이미 닫혀 있을 수 있다 — 회수
        // 실패를 보고할 곳이 없으므로 무시한다.
        let _ = std::fs::remove_file(&self.port_file);
        // 위와 동일.
        let _ = std::fs::remove_dir_all(&self.isolated_home);
    }
}

/// **전용 인스턴스 전용 정리 경로.** [`shared()`] 로 얻은 인스턴스는 `'static` 에
/// 살아 `Drop` 이 돌지 않고, 대신 atexit 가 [`TastyInstance::terminate`] 를 호출한다.
impl Drop for TastyInstance {
    fn drop(&mut self) {
        // Try graceful shutdown
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.shutdown();
        }));
        // Wait briefly, then force kill the entire process tree.
        std::thread::sleep(Duration::from_millis(200));
        force_kill(self.process.id());
        let _ = self.process.wait();
        if let Some(handle) = self.stderr_drain.take() {
            let _ = handle.join(); // join drain thread; ignore panic in drainer
        }
        let _ = std::fs::remove_file(&self.port_file);
        let _ = std::fs::remove_dir_all(&self.isolated_home);
    }
}
