//! `fs.*` IPC — generic 파일시스템 자원 위임(host 프로세스 전용).
//!
//! `fs.pick_file`: native OS 파일 선택 다이얼로그(`rfd`)를 host 프로세스에서 열고 선택
//! 경로를 회신한다. plugin 은 자기 프로세스에서 native 다이얼로그(host UI 스레드 자원)를
//! 직접 열 수 없으므로 host 에 위임한다 — 이 위임이 "파일열기 팝업 플러그인化"의 유일한
//! generic 인프라 갭이다(ADR-0042). host 는 특정 kind/plugin(markdown 등)을 **모른 채**
//! filters/start_dir 만 받아 파일 선택을 대행한다(불가침 원칙 준수).
//!
//! # 스레드 모델 (블로킹 안전성)
//!
//! IPC 는 winit 메인 루프(`about_to_wait` → `process_ipc` / `process_plugin_ipc_calls`)
//! 에서 **동기 dispatch** 된다. plugin 의 `host.call("fs.pick_file")` 는
//! `handle_ipc_default_dispatch` 가 `dispatch_with_caller` 를 inline 호출해 그 반환값을
//! `send_ipc_result` 로 회신하는 구조라, 이 핸들러가 반환하는 순간이 곧 응답이다. 따라서
//! 모달 블로킹인 `rfd::FileDialog::pick_file()` 를 이 메인 스레드에서 **동기로** 열어도
//! 안전하다(macOS 의 "native 다이얼로그는 main 스레드" 요구 충족). 결과가 dispatch 결과로
//! inline 회신되므로 별도의 비동기/oneshot 회신 배관이 필요 없다. caller plugin 은 자기
//! `host.call` 이 응답할 때까지 대기할 뿐이라 데드락도 없다.

use serde::Deserialize;
use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;

/// `fs.pick_file { filters?, start_dir? }` 요청.
#[derive(Deserialize)]
struct PickFileReq {
    /// 확장자 필터 목록(선택). 비면 모든 파일. caller 가 자기 관심 확장자를 채운다.
    #[serde(default)]
    filters: Vec<PickFilter>,
    /// 다이얼로그 시작 디렉토리(선택). 존재하지 않으면 rfd 가 무시한다.
    #[serde(default)]
    start_dir: Option<String>,
}

/// 확장자 필터 한 항목 — `{ name, exts }`.
#[derive(Deserialize)]
struct PickFilter {
    /// 다이얼로그에 노출될 필터 이름(예: "Markdown").
    name: String,
    /// 확장자 목록(점 없이, 예: `["md", "markdown"]`).
    exts: Vec<String>,
}

/// `fs.pick_file` — native 파일 선택 다이얼로그를 열어 선택 경로를 회신한다.
///
/// 응답: `{ "path": <선택 경로> | null }`. 취소하면 `path` 는 `null`.
///
/// 메인 스레드에서 호출됨(모듈 doc "스레드 모델" 참조) — 모달 동안 UI 는 블로킹되나
/// native 모달이 자기 run loop 를 돌리므로 앱이 완전히 얼지 않는다(기존 host 파일열기
/// 팝업과 동일한 동기 pick 패턴).
pub fn handle_pick_file(id: serde_json::Value, params: &serde_json::Value) -> JsonRpcResponse {
    let req: PickFileReq = match serde_json::from_value(params.clone()) {
        Ok(r) => r,
        Err(e) => return JsonRpcResponse::error(id, -32602, format!("invalid params: {e}")),
    };

    let mut dialog = rfd::FileDialog::new();
    for f in &req.filters {
        let exts: Vec<&str> = f.exts.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(&f.name, &exts);
    }
    if let Some(dir) = req.start_dir.as_deref() {
        dialog = dialog.set_directory(dir);
    }

    let path = dialog.pick_file().map(|p| p.to_string_lossy().into_owned());

    JsonRpcResponse::success(id, json!({ "path": path }))
}
