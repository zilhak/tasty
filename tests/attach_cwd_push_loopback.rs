//! mirror surface cwd push — 서버가 점유 surface 의 cwd 를 `StreamControl::Cwd` 로 holder 에
//! 보내는지를, 실제로 기동한 서버 인스턴스에 raw `TcpStream` 으로 attach 해 검증한다
//! (frame/handshake 헬퍼는 `tests/attach_common/mod.rs`).
//!
//! 이 채널이 존재하는 이유는 mirror terminal 이 로컬 PTY 가 없어 OSC 7 없이는 cwd 를 모른다는
//! 것이다. 그래서 검증은 **OSC 7 에 기대지 않는 값**으로 한다 — 워크스페이스를 명시 cwd 로 만들고
//! (셸 프로세스의 실제 cwd), 셸에서 `cd` 한 뒤의 값도 본다. 서버의 판정은 OSC 7 캐시가 있으면
//! 그것, 없으면 PTY 프로세스의 OS cwd 조회라 어느 셸이든 같은 값에 수렴한다. 결정 근거는
//! `docs/adr/0267-mirror-surface-cwd-is-pushed-by-the-server.md`.

// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다 — 전수 가드
// (`crates/tasty-doc-guards/tests/let_underscore_documented.rs`)가 테스트 본문을 제외하므로, 여기서 나는
// `let_underscore_must_use` 경고는 정책상 조치 대상이 될 수 없다. 끄지 않으면
// 프로덕션의 진짜 신호가 그 안에 묻힌다 — `docs/dev-guide/error-handling.md`.
#![allow(clippy::let_underscore_must_use)]

mod attach_common;
mod common;

use std::time::{Duration, Instant};

use attach_common::{open_workspace_attach, read_frame, write_workspace_input};
use serde_json::{Value, json};

/// 이 surface 에 대한 `cwd` control 이벤트 중 `expected` 값을 가진 것이 올 때까지 읽는다.
/// 1Hz tick 이라 상한은 넉넉히 둔다. 도중에 본 값은 실패문에 싣는다.
fn wait_for_cwd(
    stream: &mut std::net::TcpStream,
    surface_id: u64,
    expected: &str,
    limit: Duration,
) -> Vec<Value> {
    let start = Instant::now();
    let mut seen = Vec::new();
    while start.elapsed() < limit {
        let (tag, payload) = read_frame(stream);
        if tag != attach_common::TAG_CONTROL {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(&payload) else {
            continue;
        };
        if v["event"] != "cwd" || v["surface_id"].as_u64() != Some(surface_id) {
            continue;
        }
        let hit = v["cwd"].as_str() == Some(expected);
        seen.push(v);
        if hit {
            return seen;
        }
    }
    panic!(
        "surface {surface_id} 의 cwd 가 {limit:?} 안에 {expected:?} 로 push 되지 않았다. 본 값: {seen:?}"
    );
}

fn temp_dir(tag: &str, server_pid: u32) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "tasty_cwd_push_{tag}_{}_{server_pid}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir); // 이전 실행의 잔여물 정리 — 없으면 실패하는 것이 정상이다.
    std::fs::create_dir_all(&dir).unwrap();
    // `/proc/<pid>/cwd` 는 심볼릭 링크를 푼 경로를 돌려준다(macOS 의 `/var` → `/private/var` 등).
    dir.canonicalize().unwrap()
}

#[test]
fn server_pushes_the_occupied_terminal_cwd_and_follows_cd() {
    let server = common::shared();
    let start_dir = temp_dir("start", server.pid());
    let moved_dir = temp_dir("moved", server.pid());

    let created = server.call(
        "workspace.create",
        json!({ "name": "cwd-push", "cwd": start_dir.to_string_lossy() }),
    );
    let ws_id = created["id"].as_u64().expect("workspace id");
    let sid = created["surface_id"].as_u64().expect("surface id");
    server.wait_for_shell(sid);

    let mut stream = open_workspace_attach(server.port(), ws_id);

    // 점유 직후의 초기 push — 셸의 시작 cwd.
    wait_for_cwd(
        &mut stream,
        sid,
        &start_dir.to_string_lossy(),
        Duration::from_secs(10),
    );

    // 셸이 이동하면 다음 tick 들 안에 새 값이 나간다.
    // 점유 중에는 서버 로컬 입력이 막히므로 holder 입력 프레임으로 보낸다.
    write_workspace_input(
        &mut stream,
        sid as u32,
        format!("cd '{}'\r", moved_dir.to_string_lossy()).as_bytes(),
    );
    wait_for_cwd(
        &mut stream,
        sid,
        &moved_dir.to_string_lossy(),
        Duration::from_secs(10),
    );

    let _ = std::fs::remove_dir_all(&start_dir); // 임시 디렉토리 정리 — 실패해도 검증 결과와 무관하다.
    let _ = std::fs::remove_dir_all(&moved_dir); // 임시 디렉토리 정리 — 실패해도 검증 결과와 무관하다.
}
