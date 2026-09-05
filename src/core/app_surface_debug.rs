//! debug 빌드에서만 존재하는 app 층 IPC 표면 중 **창이 없어도 답이 정의되는 것.**
//!
//! [`super::app_surface`] 의 debug 짝이다. 파일을 가른 이유는 debug 핸들러를 별도 파일에
//! 모으는 규약이다([debug-ipc](../../docs/dev-guide/debug-ipc.md) "구현 규칙") — 일반
//! 핸들러 파일 중간에 `#[cfg(debug_assertions)]` 를 끼우지 않는다.
//!
//! 여기 있는 것들이 읽는 것은 `App` 의 `lua_engine` / `plugin_manager` 이고, 둘 다
//! feature 게이트가 없는 필드다. 창·surface·렌더러를 하나도 안 본다. 그런데 이들의
//! dispatch 가 gui 라우터의 debug step(`src/app/ipc/debug_methods.rs`)에만 있어서
//! 헤드리스에서는 `-32601`(그런 메서드 없음)로 끝나고 있었다(2026-09-05 실측).
//!
//! 헤드리스는 CLI 전용 실행 형태이고 이들은 **에이전트가 자기 작업을 검증하는 표면**이라
//! (event bus 관측 · 확장 훅 수동 발화 · Lua 주입), 그 부재는 편의가 아니라
//! [identity](../../docs/identity.md) 원칙 2 의 구멍이다. 메서드별 판정 표는
//! [headless-ipc-surface](../../docs/dev-guide/headless-ipc-surface.md).

use serde_json::Value;

use crate::ipc::protocol::JsonRpcResponse;

/// `debug.lua.eval` — App 소유 Lua 워커에 스크립트를 던진다(fire-and-forget).
///
/// 부수효과는 로그로 관측한다. release 에는 이 경로가 없다(identity 원칙 1: release 는
/// 사용자 키 입력에서만 스크립트를 실행한다 — [ADR-0031](../../docs/adr/0031-lua-hook-engine.md)).
///
/// 엔진을 **인자로 받는다**. 어느 엔진인지 고르는 일이 조합마다 다르기 때문이 아니라
/// (헤드리스도 gui 도 `App` 필드 하나다), 이 함수가 `App` 을 안 알아야 헤드리스 pump 와
/// gui step 이 같은 것을 부를 수 있기 때문이다.
pub(crate) fn lua_eval(
    engine: Option<&tasty_lua::LuaEngine>,
    rpc_id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let source = params.get("source").and_then(|v| v.as_str());
    match (source, engine) {
        (Some(src), Some(eng)) => {
            eng.run_script(src, Some("debug.lua.eval"));
            JsonRpcResponse::success(rpc_id, serde_json::json!({ "scheduled": true }))
        }
        (None, _) => JsonRpcResponse::invalid_params(rpc_id, "Missing 'source'"),
        (_, None) => JsonRpcResponse::error(rpc_id, -32603, "lua engine not initialized"),
    }
}
