//! Lua hook 발화 + 엔진 부트스트랩.

/// Lua hook 1회 발사 헬퍼. lua 가 None 이거나 직렬화 실패 시 silent no-op.
pub(crate) fn fire<T: serde::Serialize>(
    lua: Option<&tasty_lua::LuaEngine>,
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
}

/// Lua hook 엔진 부트스트랩. `~/.tasty/init.lua` 가 있으면 로드.
/// 초기화/로드 실패는 warn 로만 남기고 None 반환 — 호스트 부팅을 막지 않는다.
pub(crate) fn init_engine() -> Option<tasty_lua::LuaEngine> {
    let mut engine = match tasty_lua::LuaEngine::new() {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("lua engine init failed: {e}");
            return None;
        }
    };
    if let Some(home) = tasty_core::paths::tasty_home() {
        let init_path = home.join("init.lua");
        match engine.load_init(&init_path) {
            Ok(true) => tracing::info!(
                target: "tasty_lua",
                "loaded init.lua from {}",
                init_path.display(),
            ),
            Ok(false) => {}
            Err(e) => tracing::warn!("lua: failed to load init.lua: {e}"),
        }
    }
    Some(engine)
}
