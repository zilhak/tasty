//! 원격 attach 채널로 forward 된 `StructuralOp::ConvertSurface` 가 변환 결과 surface 의
//! cwd 를 잃지 않는지를 loopback `TcpStream` 으로 실제 실행 중인 서버 인스턴스에 대해
//! 검증한다. explorer 로 변환했을 때 서버가 회신하는 `structural_delta` 의
//! `surfaces[].root`(= mirror client 가 `ExplorerPanel::new(id, root)` 에 쓰는 값,
//! `src/core/attach_runtime.rs`) 가 절대경로인지를 본다 — GUI 없이도 최종 사용자
//! 가시 값과 동일한 것을 확인할 수 있는 지점이다.
//!
//! 두 경로를 각각 밟는다:
//! - `convert_forwards_client_cwd_to_explorer_root` — op 에 실려 온 cwd (client 가
//!   원격 셸의 OSC 7 로 해석한 값). 공유 인스턴스 + 자기 workspace.
//! - `convert_without_cwd_resolves_server_side_explorer_root` — cwd 없는 op (구버전
//!   client / OSC 7 미방출 셸). 서버가 대상 터미널의 실제 PTY 기준으로 직접 resolve
//!   한다. `inherit_cwd` 게이트를 타므로 **전용 인스턴스**가 필요하다.
//!
//! frame/handshake 헬퍼는 `tests/attach_common/mod.rs` 를 공유한다 — attach client 는
//! 실제 GUI 앱이 아니라 raw `TcpStream` 으로 직접 핸드셰이크한다.

// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다 — 전수 가드
// (`tests/let_underscore_documented.rs`)가 테스트 본문을 제외하므로, 여기서 나는
// `let_underscore_must_use` 경고는 정책상 조치 대상이 될 수 없다. 끄지 않으면
// 프로덕션의 진짜 신호가 그 안에 묻힌다 — `docs/dev-guide/error-handling.md`.
#![allow(clippy::let_underscore_must_use)]

mod attach_common;
mod common;

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use attach_common::{TAG_CONTROL, open_workspace_attach, read_frame, write_control_frame};
use common::TastyInstance;
use serde_json::{Value, json};

/// convert op 을 보내고, 회신되는 `structural_delta` 에서 `role == "explorer"` 인
/// surface 의 `root` 를 돌려준다.
fn convert_and_read_explorer_root(
    stream: &mut TcpStream,
    surface_id: u64,
    cwd: Option<&Path>,
) -> String {
    let mut op = json!({
        "kind": "convert_surface",
        "surface_id": surface_id,
        "surface_kind": "explorer",
        "params": {},
    });
    // cwd 키 자체를 생략하는 경우(구버전 client)를 그대로 재현한다.
    if let Some(cwd) = cwd {
        op["cwd"] = json!(cwd.to_string_lossy());
    }
    write_control_frame(
        stream,
        &json!({ "event": "structural_op", "op_id": 1, "op": op }),
    );

    loop {
        let (tag, payload) = read_frame(stream);
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        match v.get("event").and_then(|e| e.as_str()) {
            Some("structural_result") => {
                assert_eq!(v["ok"], true, "convert rejected: {v:?}");
            }
            Some("structural_delta") => {
                let surfaces = v["surfaces"].as_array().expect("surfaces array");
                let explorer = surfaces
                    .iter()
                    .find(|s| s["role"] == "explorer")
                    .unwrap_or_else(|| panic!("no explorer surface in delta: {surfaces:?}"));
                return explorer["root"].as_str().expect("root string").to_string();
            }
            _ => continue,
        }
    }
}

/// `/proc`(Linux)·`proc_pidinfo`(macOS) 는 심볼릭 링크가 풀린 경로를 돌려주므로
/// (macOS `/var/folders` → `/private/var/...`) 기대값도 같은 기준으로 맞춘다.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// op 에 cwd 가 실려 오면 서버는 그 값을 그대로 explorer root 로 쓴다.
#[test]
fn convert_forwards_client_cwd_to_explorer_root() {
    let server = common::shared();
    let ws = server.create_workspace("convert-client-cwd");

    let dir = std::env::temp_dir().join(format!(
        "tasty_convert_cwd_wire_{}_{}",
        std::process::id(),
        server.pid()
    ));
    // 이전 실행 잔여물 제거 — 없으면 NotFound 라 실패가 정상 경로다.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut stream = open_workspace_attach(server.port(), ws.id);
    let root = convert_and_read_explorer_root(&mut stream, ws.surface_id, Some(&dir));

    assert_eq!(
        Path::new(&root),
        dir.as_path(),
        "wire 로 온 cwd 가 explorer root 여야 한다 (relative fallback 금지)"
    );

    // 정리 — 실패해도 임시 디렉토리가 남을 뿐이라 테스트 결과에 영향이 없다.
    let _ = std::fs::remove_dir_all(&dir);
}

/// cwd 를 싣지 않은 op 도 서버가 대상 터미널의 실제 PTY cwd 를 직접 resolve 해
/// 절대경로 root 를 만든다 — 원격 셸이 OSC 7 을 방출하지 않는 경우의 커버리지.
///
/// **이 테스트만 전용 인스턴스를 쓴다.** 검증 대상 동작이 `inherit_cwd` 설정에
/// 게이트되는데, 그 값은 격리 HOME 의 `config.toml` 에 미리 써넣는 *프로세스 기동
/// 시점* 설정이라 이미 떠 있는 공유 인스턴스에 런타임으로 바꿔 끼울 수 없다
/// (`tests/common/mod.rs` 의 `spawn_with_inherit_cwd`).
#[test]
fn convert_without_cwd_resolves_server_side_explorer_root() {
    let server = TastyInstance::spawn_with_inherit_cwd(true);
    // 전용 인스턴스라 workspace/surface 가 각각 하나뿐이다 — 공유 인스턴스에서
    // 금지되는 "첫 workspace 가정" 이 여기서는 성립한다.
    let workspaces = server.call("workspace.list", json!({}));
    let ws_id = workspaces.as_array().unwrap()[0]["id"].as_u64().unwrap();
    let surface_id = server.first_surface_id();

    let dir = std::env::temp_dir().join(format!(
        "tasty_convert_cwd_server_{}_{}",
        std::process::id(),
        server.pid()
    ));
    // 이전 실행 잔여물 제거 — 없으면 NotFound 라 실패가 정상 경로다.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let expected = canonical(&dir);

    // 셸을 그 디렉토리로 이동시켜 PTY child 의 실제 cwd 를 바꾼다. 완료 마커는
    // `printf` 로 조립해 **에코된 명령줄에는 나타나지 않게** 한다 — 그래야
    // "cd 가 실제로 실행된 뒤"를 기다리는 동기점이 된다(레이스 방지).
    server.send_text(
        surface_id,
        &format!("cd {} && printf 'TASTY_%s\\n' CDOK\n", dir.display()),
    );
    server.wait_for_output(surface_id, "TASTY_CDOK", Duration::from_secs(10));

    let mut stream = open_workspace_attach(server.port(), ws_id);
    let root = convert_and_read_explorer_root(&mut stream, surface_id, None);

    assert_eq!(
        canonical(Path::new(&root)),
        expected,
        "서버가 대상 터미널의 PTY cwd 를 resolve 해 explorer root 로 써야 한다"
    );

    // 정리 — 실패해도 임시 디렉토리가 남을 뿐이라 테스트 결과에 영향이 없다.
    let _ = std::fs::remove_dir_all(&dir);
}
