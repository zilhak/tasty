//! Lua hook 발화 + 엔진 부트스트랩.

/// [`fire`] 에 넘기는 자동실행 컨텍스트 — 등록 목록 + 재진입 가드.
///
/// 모든 fire 지점이 이것을 조립해 넘긴다 — 새 fire 지점을 추가하면 자동실행
/// 배선도 함께 강제되도록 시그니처에 박아둔 것(Option 아님).
pub(crate) struct AutofireCtx<'a> {
    pub scripts: &'a tasty_settings::ScriptRegistry,
    pub guard: &'a mut super::autofire::AutofireGuard,
}

/// Lua hook 1회 발사 헬퍼. lua 가 None 이거나 직렬화 실패 시 silent no-op.
///
/// 두 채널을 순서대로 태운다: ① observe-hook(`tasty.on` 등록 콜백) 발화,
/// ② 자동실행(autofire) — `event` 를 트리거로 등록한 스크립트 실행.
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

/// Lua 워커 엔진 부트스트랩 (ADR-0031). VM 을 전용 워커 스레드에서 기동한다.
///
/// 부팅 시 임의 Lua 자동로드(`init.lua`)는 폐기됐다 — 스크립트는 등록 목록에서
/// 명시 트리거(단축키)로만 실행된다. 초기화 실패는 warn 로만 남기고 None 반환
/// (호스트 부팅을 막지 않는다).
pub(crate) fn init_engine() -> Option<tasty_lua::LuaEngine> {
    match tasty_lua::LuaEngine::new() {
        Ok(e) => Some(e),
        Err(e) => {
            tracing::warn!("lua engine init failed: {e}");
            None
        }
    }
}
