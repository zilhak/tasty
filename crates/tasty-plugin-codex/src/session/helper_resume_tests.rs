//! 실제 Codex로 원격 TUI 재개의 정책 적용을 확인한다.
//!
//! 사용자의 기존 로그인을 쓰므로 `TASTY_CODEX_NORMAL_HOME_E2E=1`로만 실행한다. 모델은 호출하지
//! 않는다. 시험이 만든 thread 하나만 쓰고 끝에 삭제한다. 서버와 TUI 환경에서는 `TASTY_*` 변수를
//! 지운다. TUI 실행에는 util-linux `script`로 만든 가상 터미널을 쓴다.

use super::*;
use crate::rpc::Client;

const THREAD_START_APPROVAL: &str = "on-request";
const THREAD_START_SANDBOX: &str = "workspace-write";

fn strip_tasty_env(cmd: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("TASTY_") {
            cmd.env_remove(&key);
        }
    }
    cmd.env_remove("CODEX_THREAD_ID");
}

/// helper와 같은 명령으로 소유 서버를 띄우고 준비된 URL을 돌려준다.
fn start_server(dir: &Path, tag: &str) -> (OwnedServer, String) {
    let opened = Opened {
        generation: "0123456789abcdef".into(),
        state_dir: dir.into(),
        listen: format!("unix://{}/{tag}.sock", dir.display()),
        token_file: None,
    };
    let log_path = dir.join(format!("server-{tag}.log"));
    let log = std::fs::File::create(&log_path).unwrap();
    let mut cmd = server_command(&opened);
    strip_tasty_env(&mut cmd);
    cmd.stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    let server = OwnedServer::spawn(cmd).unwrap();
    let mut probe = unix_ready(&opened.listen);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(url) = probe(&log_path) {
            return (server, url);
        }
        assert!(
            Instant::now() < deadline,
            "not ready: {}",
            std::fs::read_to_string(&log_path).unwrap_or_default()
        );
        std::thread::sleep(READY_POLL);
    }
}

/// TUI를 가상 터미널에서 `secs`초 동안 실행한다. (종료 코드, 화면 기록)을 돌려준다.
/// 기한까지 살아 있으면 종료 코드는 timeout의 124다.
fn run_tui(dir: &Path, tag: &str, tui: &Command, secs: u32) -> (Option<i32>, String) {
    let mut line = super::super::command::sh_quote(&tui.get_program().to_string_lossy());
    for arg in tui.get_args() {
        line.push(' ');
        line.push_str(&super::super::command::sh_quote(&arg.to_string_lossy()));
    }
    let typescript = dir.join(format!("tui-{tag}.typescript"));
    let mut cmd = Command::new("timeout");
    cmd.arg(secs.to_string())
        .args(["script", "-q", "-e", "-c", &line])
        .arg(&typescript)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    strip_tasty_env(&mut cmd);
    let status = cmd.status().unwrap();
    let screen =
        String::from_utf8_lossy(&std::fs::read(&typescript).unwrap_or_default()).into_owned();
    (status.code(), screen)
}

/// 실행 중 설정을 다른 연결에서 읽는다. 재정의 없이 재개하면 로드된 thread의 설정을 돌려준다.
fn active_policy(url: &str, thread: &str) -> (String, String) {
    let endpoint = super::super::registry::parse_endpoint(url, None).unwrap();
    let mut client = Client::connect(&endpoint, Duration::from_secs(10)).unwrap();
    let reply = client
        .call(
            "thread/resume",
            serde_json::json!({ "threadId": thread, "excludeTurns": true }),
        )
        .unwrap();
    client
        .call(
            "thread/unsubscribe",
            serde_json::json!({ "threadId": thread }),
        )
        .unwrap();
    client.close();
    (
        reply["approvalPolicy"].as_str().unwrap_or_default().into(),
        reply["sandbox"]["type"].as_str().unwrap_or_default().into(),
    )
}

/// rt-f25-1 E8 회귀: 재개 TUI에 정책 인자를 주면 0.154가 시작을 거부한다. helper는 소유 서버의
/// `thread/resume`에 정책을 적용하고 구독을 유지한 채 정책 없는 TUI를 붙인다. TUI가 붙은 뒤에도
/// 서버의 실행 설정이 요청한 정책이어야 한다.
#[test]
#[ignore = "실제 codex와 사용자의 기존 로그인을 쓴다. TASTY_CODEX_NORMAL_HOME_E2E=1로만 실행한다"]
fn a_resumed_remote_tui_keeps_the_policy_the_helper_applied() {
    assert_eq!(
        std::env::var("TASTY_CODEX_NORMAL_HOME_E2E").as_deref(),
        Ok("1")
    );
    let dir = tempfile::tempdir().unwrap();
    let evidence = std::env::var_os("TASTY_CODEX_EVIDENCE").map(PathBuf::from);

    // 저장된 thread를 하나 만든다. 첫 정책은 재개 때 요청할 정책과 다르게 둔다.
    let (mut first, url) = start_server(dir.path(), "a");
    let endpoint = super::super::registry::parse_endpoint(&url, None).unwrap();
    let mut client = Client::connect(&endpoint, Duration::from_secs(10)).unwrap();
    let started = client
        .call(
            "thread/start",
            serde_json::json!({
                "cwd": dir.path(),
                "approvalPolicy": THREAD_START_APPROVAL,
                "sandbox": THREAD_START_SANDBOX,
            }),
        )
        .unwrap();
    let thread = started["thread"]["id"].as_str().unwrap().to_string();
    client
        .call(
            "thread/inject_items",
            serde_json::json!({ "threadId": thread, "items": [{
                "type": "message", "role": "user",
                "content": [{ "type": "input_text", "text": "tasty resume probe" }],
            }]}),
        )
        .unwrap();
    client.close();
    first.stop();

    // 새 세대처럼 새 서버에서 재개한다.
    let (mut server, url) = start_server(dir.path(), "b");
    let args = Args {
        resume: Some(thread.clone()),
        tui_args: ["-a", "never", "-s", "read-only"]
            .map(String::from)
            .to_vec(),
        ..Args::default()
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // 원인: 정책 인자를 준 원격 재개 TUI는 시작을 거부한다.
        let mut old = crate::rpc::codex_command();
        old.args([
            "resume",
            "--remote",
            &url,
            "-c",
            "check_for_update_on_startup=false",
        ])
        .args(["-a", "never", "-s", "read-only", &thread]);
        let (old_rc, old_screen) = run_tui(dir.path(), "old", &old, 15);
        assert_eq!(old_rc, Some(1), "{old_screen}");
        assert!(
            old_screen
                .contains("Permission overrides are not supported when resuming a remote task"),
            "{old_screen}"
        );

        // 수정: helper가 정책을 적용하고 구독을 유지한 채 정책 없는 TUI를 붙인다.
        let prepared = prepare_resume(&url, None, &args).unwrap();
        assert_eq!(prepared.resumed.as_deref(), Some(thread.as_str()));
        let tui = tui_command(&url, None, &args, None);
        let (rc, screen) = run_tui(dir.path(), "new", &tui, 10);
        assert_eq!(rc, Some(124), "TUI가 기한까지 살아 있어야 한다: {screen}");
        assert!(!screen.contains("Permission overrides"), "{screen}");
        let active = active_policy(&url, &thread);
        assert_eq!(active, ("never".into(), "readOnly".into()));
        if let Some(path) = &evidence {
            let record = serde_json::json!({
                "thread": thread,
                "start": [THREAD_START_APPROVAL, THREAD_START_SANDBOX],
                "old_tui_rc": old_rc,
                "new_tui_rc": rc,
                "active_after_tui": [active.0, active.1],
            });
            std::fs::write(path, record.to_string()).unwrap();
        }
        drop(prepared.hold);
    }));
    let endpoint = super::super::registry::parse_endpoint(&url, None).unwrap();
    let mut cleanup = Client::connect(&endpoint, Duration::from_secs(10)).unwrap();
    let deleted = cleanup.call("thread/delete", serde_json::json!({ "threadId": thread }));
    cleanup.close();
    server.stop();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    deleted.unwrap();
}
