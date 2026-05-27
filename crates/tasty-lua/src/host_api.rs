//! Lua 측에 노출하는 호스트 API.
//!
//! `tasty.log` / `tasty.notify` / `tasty.run_cli` 를 `tasty` table 에 추가한다.

use std::path::PathBuf;
use std::process::Command;

use mlua::{Lua, Table, Value};

use crate::engine::LuaEngineError;

/// `tasty` 글로벌 테이블에 호스트 API 를 설치한다.
///
/// `tasty.on` 은 이미 `LuaEngine::install_api` 에서 설치됨. 이 함수는 추가 메서드들만
/// 등록한다 — 분리 이유는 `tasty.on` 이 dispatcher 와 강하게 결합된 반면 여기 API 는
/// 단순 OS shim 이라 격리할 수 있어서.
pub(crate) fn install(lua: &Lua) -> Result<(), LuaEngineError> {
    let tasty: Table = lua.globals().get("tasty").map_err(LuaEngineError::Init)?;

    let log = lua
        .create_function(|_, msg: String| {
            tracing::info!(target: "tasty_lua", "{msg}");
            Ok(())
        })
        .map_err(LuaEngineError::Init)?;
    tasty.set("log", log).map_err(LuaEngineError::Init)?;

    let warn = lua
        .create_function(|_, msg: String| {
            tracing::warn!(target: "tasty_lua", "{msg}");
            Ok(())
        })
        .map_err(LuaEngineError::Init)?;
    tasty.set("warn", warn).map_err(LuaEngineError::Init)?;

    let notify = lua
        .create_function(|_, (title, body): (String, Option<String>)| {
            let mut n = notify_rust::Notification::new();
            n.summary(&title);
            if let Some(b) = body.as_deref() {
                n.body(b);
            }
            if let Err(e) = n.show() {
                tracing::warn!(target: "tasty_lua", "notify failed: {e}");
            }
            Ok(())
        })
        .map_err(LuaEngineError::Init)?;
    tasty.set("notify", notify).map_err(LuaEngineError::Init)?;

    let run_cli = lua
        .create_function(|_, args: Value| {
            let args = match value_to_args(args) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(target: "tasty_lua", "run_cli args: {e}");
                    return Ok(());
                }
            };
            let Some(exe) = current_exe() else {
                tracing::warn!(target: "tasty_lua", "run_cli: cannot resolve current_exe");
                return Ok(());
            };
            let mut cmd = Command::new(exe);
            cmd.args(&args);
            // stdio inherited 면 Lua hook 콘솔이 노이즈로 차므로 분리.
            cmd.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            match cmd.spawn() {
                Ok(_child) => {
                    // detach — exit code 확인 안 함. 사용자 hook 은 발사 후 잊는다.
                }
                Err(e) => {
                    tracing::warn!(target: "tasty_lua", "run_cli spawn failed: {e}");
                }
            }
            Ok(())
        })
        .map_err(LuaEngineError::Init)?;
    tasty
        .set("run_cli", run_cli)
        .map_err(LuaEngineError::Init)?;

    Ok(())
}

fn current_exe() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Lua 측에서 받은 args 값을 `Vec<String>` 으로 변환. 허용 형식:
/// - `string` → 단일 인자
/// - `table` (sequence) → 각 원소가 string
fn value_to_args(v: Value) -> Result<Vec<String>, &'static str> {
    match v {
        Value::String(s) => Ok(vec![s.to_str().map_err(|_| "non-utf8")?.to_string()]),
        Value::Table(t) => {
            let len = t.raw_len();
            let mut out = Vec::with_capacity(len);
            for i in 1..=len {
                let item: Value = t.get(i).map_err(|_| "table get failed")?;
                match item {
                    Value::String(s) => out.push(s.to_str().map_err(|_| "non-utf8")?.to_string()),
                    Value::Integer(n) => out.push(n.to_string()),
                    Value::Number(n) => out.push(n.to_string()),
                    Value::Boolean(b) => out.push(b.to_string()),
                    _ => return Err("args table must contain string/number/boolean"),
                }
            }
            Ok(out)
        }
        Value::Nil => Ok(Vec::new()),
        _ => Err("args must be string or table"),
    }
}

#[cfg(test)]
mod tests {
    use super::value_to_args;
    use crate::LuaEngine;
    use mlua::Lua;

    #[test]
    fn log_function_callable() {
        let engine = LuaEngine::new().unwrap();
        engine
            .eval("tasty.log('hello'); tasty.warn('warn')")
            .unwrap();
    }

    // `tasty.run_cli` 를 Lua 에서 직접 호출하는 테스트는 두지 않는다 —
    // `current_exe()` 가 test binary (e.g. `tasty_lua-<hash>`) 일 때,
    // spawn 된 자식이 또 동일 test binary 를 실행하며 재귀적으로 자기 자신을
    // spawn 하는 fork bomb 이 발생한다. detach + orphan 조합이라 cargo test
    // 종료 후에도 영구 자가복제 한다.
    //
    // args 파싱은 `value_to_args` 단위 테스트로 커버한다.
    #[test]
    fn value_to_args_string() {
        let lua = Lua::new();
        let v = lua.create_string("hello").unwrap();
        assert_eq!(
            value_to_args(mlua::Value::String(v)).unwrap(),
            vec!["hello"]
        );
    }

    #[test]
    fn value_to_args_table() {
        let lua = Lua::new();
        let t = lua
            .load(r#"return {"list", "info", 42, true}"#)
            .eval::<mlua::Table>()
            .unwrap();
        assert_eq!(
            value_to_args(mlua::Value::Table(t)).unwrap(),
            vec!["list", "info", "42", "true"]
        );
    }

    #[test]
    fn value_to_args_nil_is_empty() {
        assert!(value_to_args(mlua::Value::Nil).unwrap().is_empty());
    }

    #[test]
    fn value_to_args_rejects_other_types() {
        assert!(value_to_args(mlua::Value::Boolean(true)).is_err());
    }
}
