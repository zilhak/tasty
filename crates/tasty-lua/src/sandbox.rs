//! Lua VM 샌드박싱 정책.
//!
//! `apply` 가 [`mlua::Lua`] 인스턴스에 일괄 적용한다.

use mlua::Lua;

use crate::engine::LuaEngineError;

/// Lua VM 메모리 상한. 큰 데이터 처리는 외부 도구에 맡긴다.
pub(crate) const MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024;

/// 위 정책을 [`Lua`] 인스턴스에 적용한다. 새 VM 마다 1회 호출.
pub(crate) fn apply(lua: &Lua) -> Result<(), LuaEngineError> {
    lua.set_memory_limit(MEMORY_LIMIT_BYTES)
        .map_err(LuaEngineError::Init)?;

    let globals = lua.globals();

    // 스크립트가 호출할 수 있는 bytecode 로딩 진입점을 제거한다.
    // 호스트의 Lua::load는 engine에서 text-only 모드로 실행한다.
    for key in ["dofile", "loadfile", "load", "loadstring"] {
        globals
            .set(key, mlua::Value::Nil)
            .map_err(LuaEngineError::Init)?;
    }

    // debug 라이브러리 제거 — registry, upvalue, getlocal 등 native crash 유발 가능.
    globals
        .set("debug", mlua::Value::Nil)
        .map_err(LuaEngineError::Init)?;

    // package가 있으면 native 로더와 검색 경로를 제거한다.
    // 실패를 무시하면 로더가 남으므로 초기화 오류로 전달한다.
    if let Ok(package) = globals.get::<mlua::Table>("package") {
        for (key, value) in [
            ("loadlib", mlua::Value::Nil),
            ("searchers", mlua::Value::Nil),
            ("cpath", mlua::Value::String(lua.create_string("")?)),
        ] {
            package.set(key, value).map_err(LuaEngineError::Init)?;
        }
    }

    Ok(())
}
