//! [`LuaEngine`] — Lua VM 인스턴스 + 로드/실행 인터페이스.
//!
//! L1 단계 골격. L2 에서 dispatcher (`tasty.on` 등록 + `fire`) 가 붙는다.

use std::path::{Path, PathBuf};

use mlua::Lua;

/// Lua hook 시스템의 진입 에러.
#[derive(Debug, thiserror::Error)]
pub enum LuaEngineError {
    #[error("lua init failed: {0}")]
    Init(mlua::Error),
    #[error("lua eval failed: {0}")]
    Eval(mlua::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

// thiserror 가 source 도 가져갈 수 있게 From 추가. mlua::Error 의 직접 사용처가
// 많아 명시.
impl From<mlua::Error> for LuaEngineError {
    fn from(e: mlua::Error) -> Self {
        LuaEngineError::Eval(e)
    }
}

/// 호스트 측이 보유하는 Lua VM 핸들. 1개만 존재하며 메인 스레드 1군데서만 사용.
///
/// 동시성: mlua 는 `send` feature 로 Send 지만, hook 콜백 안에서 호스트 state 를
/// 만지려면 single-threaded 가 단순하다. 모든 fire 는 메인 루프에서 호출 — 그
/// 시점에 한해 borrow 안전.
pub struct LuaEngine {
    lua: Lua,
    /// 마지막으로 로드한 스크립트 경로. reload 시 같은 경로 다시 읽음.
    init_path: Option<PathBuf>,
}

impl LuaEngine {
    /// 새 VM 생성 + 샌드박스 정책 적용.
    pub fn new() -> Result<Self, LuaEngineError> {
        let lua = Lua::new();
        crate::sandbox::apply(&lua)?;
        Ok(Self { lua, init_path: None })
    }

    /// `~/.tasty/init.lua` (또는 임의 경로) 를 로드. 파일 없으면 no-op (정상 케이스).
    /// 파싱/런타임 에러는 [`LuaEngineError::Eval`] 로.
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

    /// 같은 init.lua 를 다시 로드. 등록된 hook 들은 호스트 측에서 명시적으로
    /// 비워야 한다 (dispatcher 책임 — L2).
    pub fn reload(&mut self) -> Result<bool, LuaEngineError> {
        let path = self.init_path.clone();
        match path {
            Some(p) => self.load_init(&p),
            None => Ok(false),
        }
    }

    /// 임의 Lua 코드 실행. 테스트 / 디버그 용도.
    pub fn eval(&self, source: &str) -> Result<(), LuaEngineError> {
        self.lua.load(source).exec().map_err(LuaEngineError::Eval)
    }

    /// 내부 VM 핸들 노출. L2 에서 dispatcher 가 hook 콜백 호출에 사용.
    #[allow(dead_code)]
    pub(crate) fn lua(&self) -> &Lua {
        &self.lua
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_engine_runs_basic_lua() {
        let engine = LuaEngine::new().expect("init");
        engine.eval("local x = 1 + 1").expect("eval");
    }

    #[test]
    fn memory_limit_blocks_huge_string() {
        let engine = LuaEngine::new().expect("init");
        // 32MB 초과 — string 생성 도중 OOM 으로 에러.
        let result = engine.eval("local s = string.rep('a', 64 * 1024 * 1024)");
        assert!(result.is_err(), "expected memory limit to abort huge string");
    }

    #[test]
    fn debug_library_removed() {
        let engine = LuaEngine::new().expect("init");
        let result = engine.eval("return debug.getinfo");
        // debug == nil 이므로 indexing 에러 발생.
        assert!(result.is_err(), "expected debug library to be removed");
    }

    #[test]
    fn loadstring_removed() {
        let engine = LuaEngine::new().expect("init");
        let result = engine.eval("loadstring('print(1)')");
        // loadstring == nil → call attempt on nil.
        assert!(result.is_err(), "expected loadstring to be removed");
    }

    #[test]
    fn load_init_handles_missing_file() {
        let mut engine = LuaEngine::new().expect("init");
        let p = std::path::PathBuf::from("/nonexistent/tasty-test/init.lua");
        let loaded = engine.load_init(&p).expect("missing file is not an error");
        assert!(!loaded);
    }

    #[test]
    fn load_init_reads_real_file() {
        let mut engine = LuaEngine::new().expect("init");
        let dir = std::env::temp_dir().join("tasty-lua-test");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("init.lua");
        std::fs::write(&path, "_G.tasty_test_marker = 42").unwrap();
        let loaded = engine.load_init(&path).expect("load");
        assert!(loaded);
        engine.eval("assert(_G.tasty_test_marker == 42)").expect("marker set");
        std::fs::remove_file(&path).ok();
    }
}
