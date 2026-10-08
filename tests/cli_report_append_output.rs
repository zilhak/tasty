//! `tasty agent report append` 는 성공하면 아무것도 출력하지 않고, 오류만 표준 오류로 낸다.
//! 이 명령을 부르는 후처리·reduce 셸에서는 표준 출력이 작업의 결과이기 때문이다.
//! 실제 바이너리를 가짜 호스트에 붙여 두 스트림과 종료 코드를 본다. 인스턴스는 띄우지 않으며
//! CLI 는 임시 TASTY_HOME 의 포트 파일로 가짜 호스트를 찾는다.

#[path = "spawn_diag/mod.rs"]
mod spawn_diag;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};

/// 요청 한 줄을 받아 `answer` 로 답하는 가짜 호스트. CLI 가 기본으로 읽는
/// `TASTY_HOME/tasty.port` 에 포트를 쓴다.
fn fake_host(home: &std::path::Path, answer: serde_json::Value) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::fs::write(home.join("tasty.port"), port.to_string()).expect("port file");
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let mut writer = stream.try_clone().expect("clone");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            let req: serde_json::Value = serde_json::from_str(&line).expect("request json");
            let mut resp = answer.clone();
            resp["jsonrpc"] = "2.0".into();
            resp["id"] = req["id"].clone();
            if writer.write_all(format!("{resp}\n").as_bytes()).is_err() {
                return;
            }
            line.clear();
        }
    });
}

fn append(answer: serde_json::Value) -> Output {
    let home = tempfile::tempdir().expect("tempdir");
    fake_host(home.path(), answer);
    Command::new(spawn_diag::instance_bin())
        .env("TASTY_HOME", home.path())
        .env_remove("TASTY_SURFACE_ID")
        .env_remove("TASTY_SESSION_TOKEN")
        .env_remove("TASTY_AGENT_ID")
        .env_remove("TASTY_TASK_REPORT")
        .args([
            "agent",
            "report",
            "append",
            "--address",
            "1/1/run/0123/a",
            "note",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("run tasty")
}

#[test]
fn a_successful_append_prints_nothing() {
    let out = append(serde_json::json!({"result": {"result": "stored", "seq": 1}}));
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stdout.is_empty(), "stdout: {:?}", out.stdout);
    assert!(out.stderr.is_empty(), "stderr: {:?}", out.stderr);
}

#[test]
fn a_rejected_append_reports_only_on_standard_error() {
    let out = append(serde_json::json!({"error": {
        "code": -32018,
        "message": "report append for task a attempt 1 was rejected (Closed)",
        "data": {"reason": "closed", "task_id": "a", "attempt": 1, "state": "failed"},
    }}));
    assert_ne!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stdout.is_empty(), "stdout: {:?}", out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("-32018"), "stderr: {stderr}");
}
