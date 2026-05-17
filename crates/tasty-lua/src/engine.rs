//! [`LuaEngine`] — Lua VM 인스턴스 + `tasty.on` dispatcher.

use std::path::{Path, PathBuf};

use mlua::{Function, Lua, LuaSerdeExt, Table, Value};

/// Lua hook 시스템 진입 에러.
#[derive(Debug, thiserror::Error)]
pub enum LuaEngineError {
    #[error("lua init failed: {0}")]
    Init(mlua::Error),
    #[error("lua eval failed: {0}")]
    Eval(mlua::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl From<mlua::Error> for LuaEngineError {
    fn from(e: mlua::Error) -> Self {
        LuaEngineError::Eval(e)
    }
}

/// 내부 Lua registry 키 — 등록된 hook 함수 목록 보관.
/// 구조: `{ [event_name: string] = { Function, Function, ... } }`.
const HOOKS_REGISTRY_KEY: &str = "tasty_hooks";

/// 호스트 측이 보유하는 Lua VM 핸들. 1 개만 존재하며 메인 스레드 1 군데서만 호출.
pub struct LuaEngine {
    lua: Lua,
    init_path: Option<PathBuf>,
}

impl LuaEngine {
    /// 새 VM 생성 + 샌드박스 적용 + `tasty.on` API 설치.
    pub fn new() -> Result<Self, LuaEngineError> {
        let lua = Lua::new();
        crate::sandbox::apply(&lua)?;
        let engine = Self {
            lua,
            init_path: None,
        };
        engine.install_api()?;
        Ok(engine)
    }

    fn install_api(&self) -> Result<(), LuaEngineError> {
        let registry = self.lua.create_table().map_err(LuaEngineError::Init)?;
        self.lua
            .set_named_registry_value(HOOKS_REGISTRY_KEY, registry)
            .map_err(LuaEngineError::Init)?;

        let tasty_table = self.lua.create_table().map_err(LuaEngineError::Init)?;

        let on = self
            .lua
            .create_function(|lua, (event, cb): (String, Function)| {
                let reg: Table = lua.named_registry_value(HOOKS_REGISTRY_KEY)?;
                let list: Table = match reg.get::<Value>(event.as_str())? {
                    Value::Table(t) => t,
                    _ => {
                        let t = lua.create_table()?;
                        reg.set(event.as_str(), t.clone())?;
                        t
                    }
                };
                list.push(cb)?;
                Ok(())
            })
            .map_err(LuaEngineError::Init)?;
        tasty_table
            .set("on", on)
            .map_err(LuaEngineError::Init)?;

        self.lua
            .globals()
            .set("tasty", tasty_table)
            .map_err(LuaEngineError::Init)?;
        Ok(())
    }

    /// 등록된 hook 들을 모두 비운다. reload 직전에 호출.
    pub fn reset_hooks(&self) -> Result<(), LuaEngineError> {
        let registry = self.lua.create_table().map_err(LuaEngineError::Init)?;
        self.lua
            .set_named_registry_value(HOOKS_REGISTRY_KEY, registry)
            .map_err(LuaEngineError::Init)?;
        Ok(())
    }

    /// `~/.tasty/init.lua` 로드. 파일 없으면 false 리턴 (정상 케이스). 파싱/런타임
    /// 에러는 [`LuaEngineError::Eval`].
    pub fn load_init(&mut self, path: &Path) -> Result<bool, LuaEngineError> {
        if !path.exists() {
            self.init_path = Some(path.to_path_buf());
            return Ok(false);
        }
        let source = std::fs::read_to_string(path)?;
        self.lua
            .load(&source)
            .set_name(path.display().to_string())
            .set_mode(mlua::ChunkMode::Text)
            .exec()
            .map_err(LuaEngineError::Eval)?;
        self.init_path = Some(path.to_path_buf());
        Ok(true)
    }

    /// 같은 init.lua 다시 로드. 기존 hook 등록은 모두 제거 후 재실행.
    pub fn reload(&mut self) -> Result<bool, LuaEngineError> {
        self.reset_hooks()?;
        let path = self.init_path.clone();
        match path {
            Some(p) => self.load_init(&p),
            None => Ok(false),
        }
    }

    /// 이벤트 hook 발화. 등록된 callback 들을 순서대로 호출. 콜백 에러는 warn 로
    /// 기록만 하고 다음 콜백 계속 진행 (한 ill-behaved hook 이 전체를 막지 않게).
    ///
    /// `ctx` 는 JSON 으로 표현 가능한 임의 값 — `tasty-lua` 가 Lua table 로 변환해
    /// 콜백에 전달한다.
    pub fn fire(&self, event: &str, ctx: &serde_json::Value) {
        let reg: Table = match self.lua.named_registry_value(HOOKS_REGISTRY_KEY) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("lua: hook registry missing: {e}");
                return;
            }
        };
        let list: Table = match reg.get::<Value>(event) {
            Ok(Value::Table(t)) => t,
            _ => return,
        };
        let lua_ctx = match self.lua.to_value(ctx) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("lua: ctx serialize failed for '{event}': {e}");
                return;
            }
        };
        let len = list.raw_len();
        for i in 1..=len {
            let cb: Function = match list.get(i) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if let Err(e) = cb.call::<()>(lua_ctx.clone()) {
                tracing::warn!("lua hook for '{event}' failed: {e}");
            }
        }
    }

    /// 임의 Lua 코드 실행. 테스트 / 디버그 용도.
    pub fn eval(&self, source: &str) -> Result<(), LuaEngineError> {
        self.lua.load(source).exec().map_err(LuaEngineError::Eval)
    }

    /// 내부 VM 핸들 노출. 호스트 API 추가 (`tasty.log` 등) 등록에 사용.
    pub(crate) fn lua(&self) -> &Lua {
        &self.lua
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn new_engine_runs_basic_lua() {
        let engine = LuaEngine::new().expect("init");
        engine.eval("local x = 1 + 1").expect("eval");
    }

    #[test]
    fn memory_limit_blocks_huge_string() {
        let engine = LuaEngine::new().expect("init");
        let result = engine.eval("local s = string.rep('a', 64 * 1024 * 1024)");
        assert!(result.is_err());
    }

    #[test]
    fn debug_library_removed() {
        let engine = LuaEngine::new().expect("init");
        assert!(engine.eval("return debug.getinfo").is_err());
    }

    #[test]
    fn loadstring_removed() {
        let engine = LuaEngine::new().expect("init");
        assert!(engine.eval("loadstring('print(1)')").is_err());
    }

    #[test]
    fn load_init_handles_missing_file() {
        let mut engine = LuaEngine::new().expect("init");
        let p = std::path::PathBuf::from("/nonexistent/tasty-test/init.lua");
        assert!(!engine.load_init(&p).expect("missing-is-ok"));
    }

    #[test]
    fn tasty_on_registers_callback() {
        let engine = LuaEngine::new().expect("init");
        engine
            .eval(
                r#"
                tasty.on("test.event", function(ctx)
                    _G.last_received = ctx.value
                end)
                "#,
            )
            .expect("register");

        engine.fire("test.event", &json!({ "value": 42 }));
        engine
            .eval("assert(_G.last_received == 42)")
            .expect("callback fired");
    }

    #[test]
    fn fire_with_no_listeners_is_noop() {
        let engine = LuaEngine::new().expect("init");
        // 등록 없이 fire — 에러 없이 그냥 통과해야.
        engine.fire("nothing.subscribes", &json!({}));
    }

    #[test]
    fn multiple_callbacks_per_event_all_fire() {
        let engine = LuaEngine::new().expect("init");
        engine
            .eval(
                r#"
                _G.counter = 0
                tasty.on("ping", function(_) _G.counter = _G.counter + 1 end)
                tasty.on("ping", function(_) _G.counter = _G.counter + 10 end)
                "#,
            )
            .unwrap();
        engine.fire("ping", &json!({}));
        engine.eval("assert(_G.counter == 11)").unwrap();
    }

    #[test]
    fn callback_error_does_not_abort_others() {
        let engine = LuaEngine::new().expect("init");
        engine
            .eval(
                r#"
                _G.second_ran = false
                tasty.on("e", function(_) error("boom") end)
                tasty.on("e", function(_) _G.second_ran = true end)
                "#,
            )
            .unwrap();
        engine.fire("e", &json!({}));
        engine.eval("assert(_G.second_ran == true)").unwrap();
    }

    #[test]
    fn reset_hooks_clears_registrations() {
        let engine = LuaEngine::new().expect("init");
        engine
            .eval(
                r#"
                _G.fired = false
                tasty.on("x", function(_) _G.fired = true end)
                "#,
            )
            .unwrap();
        engine.reset_hooks().unwrap();
        engine.fire("x", &serde_json::json!({}));
        engine.eval("assert(_G.fired == false)").unwrap();
    }

    #[test]
    fn reload_re_executes_init_and_clears_old_hooks() {
        let mut engine = LuaEngine::new().expect("init");
        let dir = std::env::temp_dir().join("tasty-lua-reload-test");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("init.lua");

        std::fs::write(
            &path,
            r#"
            _G.version = 1
            tasty.on("ev", function(_) _G.from_old = true end)
            "#,
        )
        .unwrap();
        engine.load_init(&path).unwrap();
        engine.eval("assert(_G.version == 1)").unwrap();

        // init.lua 의 내용을 바꿔 reload.
        std::fs::write(
            &path,
            r#"
            _G.version = 2
            _G.from_old = false
            tasty.on("ev", function(_) _G.from_new = true end)
            "#,
        )
        .unwrap();
        engine.reload().unwrap();
        engine.eval("assert(_G.version == 2)").unwrap();

        engine.fire("ev", &serde_json::json!({}));
        // 이전 콜백은 reset 되었고, 새 콜백만 발화해야.
        engine
            .eval("assert(_G.from_old == false and _G.from_new == true)")
            .unwrap();
        std::fs::remove_file(&path).ok();
    }
}
