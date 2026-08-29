//! 웹훅 통합 테스트 전용 하네스 (S16).
//!
//! `tests/common` 을 미러링하되 웹훅 E2E 가 필요로 하는 것을 추가한다:
//! - **웹훅 포트 시딩** — `TASTY_HOME/webhooks.toml` 에 미리 `port = N` 을 써
//!   리스너가 결정적으로 그 포트에 bind 하게 한다(테스트마다 free port 로 격리).
//! - **TASTY_HOME 제어** — `webhooks.toml`/`hook-handlers.toml` 위치를 잡고,
//!   재시작 테스트가 두 인스턴스 간 같은 홈을 공유할 수 있게 한다.
//! - **실 HTTP 클라이언트** — 리스너에 실제 요청을 쏴 ACK/상태변화를 관측한다
//!   (research §5: 로컬 실 HTTP 구동 검증).
//! - **CLI 러너** — `CARGO_BIN_EXE_tasty` 를 `--port-file` 로 붙여 CLI→IPC 매핑을
//!   실 바이너리로 검증한다.
//!
//! 기존 `tests/common` 을 건드리지 않으려고 별도 모듈로 둔다(격리).

#![allow(dead_code)]

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

const SPAWN_PORT_TIMEOUT: Duration = Duration::from_secs(40);
const SPAWN_SHELL_TIMEOUT: Duration = Duration::from_secs(20);
const WEBHOOK_READY_TIMEOUT: Duration = Duration::from_secs(20);

/// OS 가 배정하는 free TCP 포트를 하나 잡아 반환한다(즉시 해제). 리스너가 곧 이
/// 포트에 bind 하므로 race 창은 짧다. 테스트마다 고유 포트로 격리하기 위한 것.
pub fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind free port");
    listener.local_addr().expect("local_addr").port()
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
    webhook_port: u16,
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
    stderr_drain: Option<JoinHandle<()>>,
}

impl WebhookInstance {
    pub fn builder(webhook_port: u16) -> Builder {
        Builder {
            webhook_port,
            home: None,
            env: Vec::new(),
            files: Vec::new(),
        }
    }

    fn spawn_inner(builder: Builder) -> Self {
        let unique = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let port_file = std::env::temp_dir().join(format!("tasty-wh-test-{unique}.port"));

        let (home, own_home) = match builder.home {
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
        let webhooks_toml = home.join("webhooks.toml");
        if !webhooks_toml.exists() {
            std::fs::write(&webhooks_toml, format!("port = {}\n", builder.webhook_port))
                .expect("seed webhooks.toml");
        }
        for (name, content) in &builder.files {
            std::fs::write(home.join(name), content).expect("write extra tasty file");
        }

        let mut command = Command::new(env!("CARGO_BIN_EXE_tasty"));
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
            .env("RUST_LOG", "tasty=info")
            .stderr(Stdio::piped());
        for (k, v) in &builder.env {
            command.env(k, v);
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

        let start = Instant::now();
        let port = loop {
            if start.elapsed() > SPAWN_PORT_TIMEOUT {
                panic!(
                    "tasty failed to start within {SPAWN_PORT_TIMEOUT:?}.\n--- stderr ---\n{}",
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
            webhook_port: builder.webhook_port,
            port_file,
            home,
            shell_home,
            own_home,
            stderr_ring: Arc::clone(&stderr_ring),
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
                    "shell produced no output within {SPAWN_SHELL_TIMEOUT:?}.\n--- stderr ---\n{}",
                    stderr_tail(&instance.stderr_ring, STDERR_TAIL_LINES)
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        instance
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

    pub fn wait_webhook_ready(&self) {
        let start = Instant::now();
        let addr = format!("127.0.0.1:{}", self.webhook_port);
        loop {
            if TcpStream::connect(&addr).is_ok() {
                return;
            }
            if start.elapsed() > WEBHOOK_READY_TIMEOUT {
                panic!(
                    "webhook listener not accepting on {addr} within {WEBHOOK_READY_TIMEOUT:?}.\n--- stderr ---\n{}",
                    stderr_tail(&self.stderr_ring, STDERR_TAIL_LINES)
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
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

    /// `CARGO_BIN_EXE_tasty <args> --port-file <this instance>` 를 CLI 클라이언트로
    /// 실행한다(실 바이너리 → 실 IPC). stdout/stderr/exit 을 담아 반환.
    pub fn cli(&self, args: &[&str]) -> Output {
        // `--port-file` 은 전역(top-level) 플래그라 서브커맨드 **앞**에 와야 한다.
        Command::new(env!("CARGO_BIN_EXE_tasty"))
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
