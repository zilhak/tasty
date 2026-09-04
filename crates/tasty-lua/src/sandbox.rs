//! Lua VM 샌드박싱 정책.
//!
//! `apply` 가 [`mlua::Lua`] 인스턴스에 일괄 적용한다.

use mlua::Lua;

use crate::engine::LuaEngineError;

/// Lua VM 의 메모리 사용 한계. user script 가 거대한 string/table 을 만들지 못하게.
/// 32MB — 정상적인 hook 작업에는 충분. 데이터 처리는 외부 도구로 위임 권장.
pub(crate) const MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024;

/// 위 정책을 [`Lua`] 인스턴스에 적용한다. 새 VM 마다 1회 호출.
pub(crate) fn apply(lua: &Lua) -> Result<(), LuaEngineError> {
    lua.set_memory_limit(MEMORY_LIMIT_BYTES)
        .map_err(LuaEngineError::Init)?;

    // bytecode 로딩 차단은 user 가 호출할 수 있는 진입점 (load/loadstring/loadfile/
    // dofile) 을 nil 로 만들어 차단. 호스트 측이 직접 `Lua::load` 로 스크립트 소스를
    // 읽을 때만 set_mode("t") 로 text-only 보장 (engine 측에서 처리).

    let globals = lua.globals();

    // native lib 로드 차단. `require` 자체는 두되 (pure-lua 모듈 require 가능),
    // C native lib 로드 경로만 제거. 그리고 `dofile` / `loadfile` / `load` /
    // `loadstring` 처럼 임의 코드를 실행하는 표면 중 binary mode 가능한 것들은
    // 제거. user 가 진짜 외부 lua 파일을 불러야 하면 `dofile` 대신 `require` 권장.
    for key in ["dofile", "loadfile", "load", "loadstring"] {
        globals
            .set(key, mlua::Value::Nil)
            .map_err(LuaEngineError::Init)?;
    }

    // debug 라이브러리 제거 — registry, upvalue, getlocal 등 native crash 유발 가능.
    globals
        .set("debug", mlua::Value::Nil)
        .map_err(LuaEngineError::Init)?;

    // package.loadlib 제거 — native dylib 로드 경로 차단.
    //
    // 실패를 삼키지 않는다. 위 `require` 를 살려 두는 정책 때문에 `package` 테이블은
    // 스크립트에서 계속 닿을 수 있고, 여기 set 이 실패하면 native 로더가 **그대로
    // 남는다** — 샌드박스가 조용히 약해지는 방향이라 "실패해도 무시" 가 성립하지 않는다.
    // 같은 함수의 다른 하드닝(`dofile`/`load`/`debug` 제거)도 전부 전파한다.
    // `package` 테이블 자체가 없으면 제거할 로더도 없으므로 그 경우만 건너뛴다.
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
