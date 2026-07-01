//! Lua 측에 노출하는 호스트 API.
//!
//! `tasty.log` / `tasty.warn` 는 워커 스레드에서 직접 tracing 에 쓴다 (메인 무관).
//! `tasty.run_cli` 는 프로세스 spawn 이 부수효과이므로 워커에서 직접 하지 않고
//! [`HostCommand`] 로 메인 커맨드 큐에 넣는다 (ADR-0031). 메인이 [`run_tasty_cli`] 로 적용.

use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{SyncSender, TrySendError};

use mlua::{Lua, LuaSerdeExt, Table, Value};

use crate::bridge::{HostCommand, SharedSnapshot};
use crate::engine::LuaEngineError;

/// `tasty` 글로벌 테이블에 호스트 API 를 설치한다 (VM 생성 시 1회, 워커로 이동 전).
///
/// `tasty.on` 은 이미 엔진이 설치함. 이 함수는 추가 메서드를 등록한다.
/// `command_tx` = 워커→메인 커맨드 큐, `snapshot` = 메인→워커 읽기전용 스냅샷.
pub(crate) fn install(
    lua: &Lua,
    command_tx: SyncSender<HostCommand>,
    snapshot: SharedSnapshot,
) -> Result<(), LuaEngineError> {
    let tasty: Table = lua.globals().get("tasty").map_err(LuaEngineError::Init)?;

    // tasty.tree() — 메인이 발행한 최신 스냅샷의 워크스페이스 트리를 Lua table 로 반환.
    // 값 복사(스냅샷 핸들을 Lua 가 쥐지 않음) → read-only. (ADR-0031 읽기 = 스냅샷)
    let tree = lua
        .create_function(move |lua, ()| {
            let snap = match snapshot.lock() {
                Ok(guard) => guard.clone(),
                Err(e) => return Err(mlua::Error::runtime(format!("snapshot poisoned: {e}"))),
            };
            lua.to_value(&snap.tree)
        })
        .map_err(LuaEngineError::Init)?;
    tasty.set("tree", tree).map_err(LuaEngineError::Init)?;

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

    let run_cli = lua
        .create_function(move |_, args: Value| {
            let args = match value_to_args(args) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(target: "tasty_lua", "run_cli args: {e}");
                    return Ok(());
                }
            };
            match command_tx.try_send(HostCommand::RunCli(args)) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    tracing::warn!(target: "tasty_lua", "run_cli: command queue full — dropped")
                }
                Err(TrySendError::Disconnected(_)) => {
                    tracing::warn!(target: "tasty_lua", "run_cli: command queue closed — dropped")
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

/// [`HostCommand::RunCli`] 적용 — 메인 스레드가 안전지점에서 호출한다.
/// tasty 자기 실행파일을 CLI 인자와 함께 detached 로 spawn (발사 후 잊음).
pub fn run_tasty_cli(args: &[String]) {
    let Some(exe) = current_exe() else {
        tracing::warn!(target: "tasty_lua", "run_cli: cannot resolve current_exe");
        return;
    };
    let mut cmd = Command::new(exe);
    tasty_utils::process::hide_console(&mut cmd);
    cmd.args(args);
    // stdio inherited 면 콘솔이 노이즈로 차므로 분리.
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Err(e) = cmd.spawn() {
        tracing::warn!(target: "tasty_lua", "run_cli spawn failed: {e}");
    }
}

fn current_exe() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Lua 측에서 받은 args 값을 `Vec<String>` 으로 변환. 허용 형식:
/// - `string` → 단일 인자
/// - `table` (sequence) → 각 원소가 string/number/boolean
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
    use mlua::Lua;

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
