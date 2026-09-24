//! 로그는 워커에서 기록하고, CLI 실행 요청은 HostCommand로 메인 스레드에 전달한다.

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
#[allow(clippy::cognitive_complexity)] // complexity-exempt: tree/log/warn/run_cli 4개 클로저를 순서대로 생성·set, 클로저 내부 에러 매핑(map_err(LuaEngineError::Init))이 정형적으로 반복된다. run_cli 클로저의 TrySendError 3-way match 는 enclosing 함수에 합산됨(lint 는 렉시컬 스코프라 내부 클로저까지 suppress).
pub(crate) fn install(
    lua: &Lua,
    command_tx: SyncSender<HostCommand>,
    snapshot: SharedSnapshot,
) -> Result<(), LuaEngineError> {
    let tasty: Table = lua.globals().get("tasty").map_err(LuaEngineError::Init)?;

    // Lua에는 값을 복사해 반환하므로 호스트 스냅샷을 수정할 수 없다.
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

/// 메인 스레드에서 현재 Tasty 실행 파일을 CLI 인자로 실행한다. 완료는 기다리지 않는다.
pub fn run_tasty_cli(args: &[String]) {
    let Some(exe) = current_exe() else {
        tracing::warn!(target: "tasty_lua", "run_cli: cannot resolve current_exe");
        return;
    };
    let mut cmd = Command::new(exe);
    tasty_utils::process::hide_console(&mut cmd);
    cmd.args(args);
    // 부모의 콘솔 입력·출력에 연결하지 않는다.
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
