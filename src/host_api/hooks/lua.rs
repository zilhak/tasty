//! Lua 이벤트와 등록 스크립트 자동실행을 연결한다.

pub(crate) struct AutofireCtx<'a> {
    pub scripts: &'a tasty_settings::ScriptRegistry,
    pub guard: &'a mut super::autofire::AutofireGuard,
}

/// observe-hook을 먼저 요청하고 자동실행을 따로 요청한다.
/// payload 직렬화 실패는 로그에 남기며 그 경우에도 자동실행은 계속 판단한다.
pub(crate) fn fire<T: serde::Serialize>(
    lua: Option<&tasty_lua::LuaEngine>,
    autofire: AutofireCtx<'_>,
    event: &str,
    payload: &T,
) {
    if let Some(lua) = lua {
        match serde_json::to_value(payload) {
            Ok(v) => lua.fire(event, &v),
            Err(e) => {
                tracing::warn!(target: "tasty_lua", "fire '{event}' serialize failed: {e}")
            }
        }
    }
    super::autofire::dispatch(lua, autofire.scripts, autofire.guard, event);
}

/// worker 초기화 실패는 로그 후 None이다. 호스트 부팅을 여기서 중단하지 않는다.
pub(crate) fn init_engine() -> Option<tasty_lua::LuaEngine> {
    match tasty_lua::LuaEngine::new() {
        Ok(e) => Some(e),
        Err(e) => {
            tracing::warn!("lua engine init failed: {e}");
            None
        }
    }
}
