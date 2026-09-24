//! attach의 ConvertSurface 뒤 explorer root에 절대 cwd가 전달되는지 structural_delta로 확인한다.
//! 클라이언트가 cwd를 준 경우와 생략해 서버가 PTY에서 찾는 경우를 각각 시험한다.
//! GUI의 최종 화면이 아니라 클라이언트가 패널 생성에 사용하는 응답 값을 확인한다.

// 이유: 시험의 정리용 결과 무시는 제품 코드의 오류 처리 목록과 구분한다.
#![allow(clippy::let_underscore_must_use)]

mod attach_common;
mod common;

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use attach_common::{TAG_CONTROL, open_workspace_attach, read_frame, write_control_frame};
use common::TastyInstance;
use serde_json::{Value, json};

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
    // 구버전 클라이언트처럼 cwd 필드 자체를 생략하는 경우도 시험한다.
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

/// 서버의 실제 cwd 조회처럼 심볼릭 링크를 해석한 경로로 기대값을 맞춘다.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[test]
fn convert_forwards_client_cwd_to_explorer_root() {
    let server = common::shared();
    let ws = server.create_workspace("convert-client-cwd");

    let dir = std::env::temp_dir().join(format!(
        "tasty_convert_cwd_wire_{}_{}",
        std::process::id(),
        server.pid()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut stream = open_workspace_attach(server.port(), ws.id);
    let root = convert_and_read_explorer_root(&mut stream, ws.surface_id, Some(&dir));

    assert_eq!(
        Path::new(&root),
        dir.as_path(),
        "wire 로 온 cwd 가 explorer root 여야 한다 (relative fallback 금지)"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// inherit_cwd는 시작 설정이므로 미리 설정한 전용 인스턴스로 cwd 생략 동작을 확인한다.
#[test]
fn convert_without_cwd_resolves_server_side_explorer_root() {
    let server = TastyInstance::spawn_with_inherit_cwd(true);
    // 전용 인스턴스의 초기 workspace를 사용한다. 공유 서버에서는 각 시험이 workspace를 만들어야 한다.
    let workspaces = server.call("workspace.list", json!({}));
    let ws_id = workspaces.as_array().unwrap()[0]["id"].as_u64().unwrap();
    let surface_id = server.first_surface_id();

    let dir = std::env::temp_dir().join(format!(
        "tasty_convert_cwd_server_{}_{}",
        std::process::id(),
        server.pid()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let expected = canonical(&dir);

    // 완료 표지를 printf로 조립해 명령 에코를 실행 완료로 오인하지 않도록 한다.
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

    let _ = std::fs::remove_dir_all(&dir);
}
