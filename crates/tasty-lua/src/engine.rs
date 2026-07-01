//! [`LuaEngine`] — Lua VM 을 소유하는 **워커 스레드** 핸들 (ADR-0031).
//!
//! VM 은 전용 워커 스레드에서만 접근한다. 메인 스레드는 이 핸들을 통해
//! 실행 job 을 보내고(직렬 처리), 워커가 쌓은 [`HostCommand`] 를 drain 하며,
//! 읽기전용 [`LuaSnapshot`] 을 발행한다. 메인과 워커는 이 경계 밖에서 state 를 공유하지 않는다.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use mlua::{Function, Lua, LuaSerdeExt, Table, Value};

use crate::bridge::{HostCommand, LuaSnapshot, SharedSnapshot};

/// Lua hook 시스템 진입 에러.
#[derive(Debug, thiserror::Error)]
pub enum LuaEngineError {
    #[error("lua init failed: {0}")]
    Init(mlua::Error),
    #[error("lua eval failed: {0}")]
    Eval(mlua::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// 워커 스레드가 종료되어 job 을 처리할 수 없음.
    #[error("lua worker unavailable")]
    WorkerGone,
}

impl From<mlua::Error> for LuaEngineError {
    fn from(e: mlua::Error) -> Self {
        LuaEngineError::Eval(e)
    }
}

/// 내부 Lua registry 키 — 등록된 hook 함수 목록 보관.
/// 구조: `{ [event_name: string] = { Function, Function, ... } }`.
const HOOKS_REGISTRY_KEY: &str = "tasty_hooks";

/// 메인→워커 실행 job 큐 용량. 초과 시 fire-and-forget job 은 drop + warn(backpressure).
const JOB_QUEUE_CAP: usize = 256;

/// 워커→메인 커맨드 큐 용량. host_api 쪽(`bridge`/`host_api`)이 공유.
pub(crate) const COMMAND_QUEUE_CAP: usize = 256;

/// 워커 스레드에 보내는 실행 요청. 워커는 이들을 **직렬**로 처리한다.
enum LuaJob {
    /// 임의 소스 실행 + 결과 회신 (블로킹 호출용 — 테스트/부팅 로드).
    Eval {
        source: String,
        name: Option<String>,
        reply: SyncSender<Result<(), LuaEngineError>>,
    },
    /// 임의 소스 실행 (fire-and-forget — 단축키/디버그 트리거).
    Run {
        source: String,
        name: Option<String>,
    },
    /// 이벤트 hook 발화 (observe-only, fire-and-forget).
    Fire {
        event: String,
        ctx: serde_json::Value,
    },
    /// 등록 hook 전부 제거 + 결과 회신.
    ResetHooks {
        reply: SyncSender<Result<(), LuaEngineError>>,
    },
    /// 워커 루프 종료.
    Shutdown,
}

/// 호스트 측 Lua 워커 핸들. VM 자체는 워커 스레드가 소유하며, 메인은 이 핸들로만 통신한다.
pub struct LuaEngine {
    job_tx: SyncSender<LuaJob>,
    command_rx: Receiver<HostCommand>,
    snapshot: SharedSnapshot,
    init_path: Option<PathBuf>,
    worker: Option<JoinHandle<()>>,
}

impl LuaEngine {
    /// 새 VM 생성 + 샌드박스 + API 설치 후 워커 스레드로 이동. 앱 부팅 시 1회.
    pub fn new() -> Result<Self, LuaEngineError> {
        let lua = Lua::new();
        crate::sandbox::apply(&lua)?;

        let (command_tx, command_rx) = sync_channel(COMMAND_QUEUE_CAP);
        let snapshot: SharedSnapshot = Arc::new(Mutex::new(Arc::new(LuaSnapshot::default())));

        install_hook_api(&lua)?;
        crate::host_api::install(&lua, command_tx, snapshot.clone())?;

        let (job_tx, job_rx) = sync_channel(JOB_QUEUE_CAP);
        let worker = std::thread::Builder::new()
            .name("tasty-lua-worker".to_string())
            .spawn(move || worker_loop(lua, job_rx))?;

        Ok(Self {
            job_tx,
            command_rx,
            snapshot,
            init_path: None,
            worker: Some(worker),
        })
    }

    /// 메인이 발행하는 최신 읽기전용 스냅샷 교체. read API 가 다음부터 이 값을 읽는다.
    pub fn publish_snapshot(&self, snap: LuaSnapshot) {
        match self.snapshot.lock() {
            Ok(mut guard) => *guard = Arc::new(snap),
            Err(e) => tracing::warn!(target: "tasty_lua", "snapshot publish poisoned: {e}"),
        }
    }

    /// 워커가 쌓아둔 커맨드를 모두 꺼낸다. 메인이 안전지점에서 호출해 적용한다.
    pub fn drain_commands(&self) -> Vec<HostCommand> {
        self.command_rx.try_iter().collect()
    }

    /// 이벤트 hook 발화 (fire-and-forget). 워커가 등록 콜백을 순서대로 호출.
    pub fn fire(&self, event: &str, ctx: &serde_json::Value) {
        self.send_ff(
            LuaJob::Fire {
                event: event.to_string(),
                ctx: ctx.clone(),
            },
            "fire",
        );
    }

    /// 스크립트 소스를 워커에서 실행 (fire-and-forget). 단축키/디버그 트리거용.
    pub fn run_script(&self, source: &str, name: Option<&str>) {
        self.send_ff(
            LuaJob::Run {
                source: source.to_string(),
                name: name.map(str::to_string),
            },
            "run_script",
        );
    }

    /// 임의 Lua 소스 실행 (블로킹 — 완료까지 대기해 결과 반환). 테스트/디버그 용도.
    pub fn eval(&self, source: &str) -> Result<(), LuaEngineError> {
        self.eval_named(source, None)
    }

    fn eval_named(&self, source: &str, name: Option<&str>) -> Result<(), LuaEngineError> {
        let (reply_tx, reply_rx) = sync_channel(1);
        self.job_tx
            .try_send(LuaJob::Eval {
                source: source.to_string(),
                name: name.map(str::to_string),
                reply: reply_tx,
            })
            .map_err(|_| LuaEngineError::WorkerGone)?;
        reply_rx.recv().map_err(|_| LuaEngineError::WorkerGone)?
    }

    /// 등록 hook 을 모두 비운다 (블로킹). reload 직전에 호출.
    pub fn reset_hooks(&self) -> Result<(), LuaEngineError> {
        let (reply_tx, reply_rx) = sync_channel(1);
        self.job_tx
            .try_send(LuaJob::ResetHooks { reply: reply_tx })
            .map_err(|_| LuaEngineError::WorkerGone)?;
        reply_rx.recv().map_err(|_| LuaEngineError::WorkerGone)?
    }

    /// `~/.tasty/init.lua` 로드 (블로킹). 파일 없으면 false. 파싱/런타임 에러는 [`LuaEngineError::Eval`].
    ///
    /// NOTE: init.lua 자동로드는 TODO 09 에서 폐기 예정. 워커 이관 전까지 배관 유지.
    pub fn load_init(&mut self, path: &Path) -> Result<bool, LuaEngineError> {
        self.init_path = Some(path.to_path_buf());
        if !path.exists() {
            return Ok(false);
        }
        let source = std::fs::read_to_string(path)?;
        self.eval_named(&source, Some(&path.display().to_string()))?;
        Ok(true)
    }

    /// 같은 init.lua 다시 로드. 기존 hook 등록은 모두 제거 후 재실행.
    pub fn reload(&mut self) -> Result<bool, LuaEngineError> {
        self.reset_hooks()?;
        match self.init_path.clone() {
            Some(p) => self.load_init(&p),
            None => Ok(false),
        }
    }

    /// fire-and-forget job 전송. 큐 포화/워커 종료 시 drop + warn (메인은 블록되지 않는다).
    fn send_ff(&self, job: LuaJob, what: &str) {
        match self.job_tx.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                tracing::warn!(target: "tasty_lua", "lua job queue full — dropping {what}")
            }
            Err(TrySendError::Disconnected(_)) => {
                tracing::warn!(target: "tasty_lua", "lua worker gone — dropping {what}")
            }
        }
    }
}

impl Drop for LuaEngine {
    fn drop(&mut self) {
        // 큐가 가득 차 있어도 Shutdown 은 반드시 도달해야 하므로 블로킹 send 사용.
        // 이미 워커가 죽어 disconnect 여도 정상 — 아래 join 이 정리한다.
        if let Err(e) = self.job_tx.send(LuaJob::Shutdown) {
            tracing::trace!(target: "tasty_lua", "shutdown send skipped (worker gone): {e}");
        }
        if let Some(handle) = self.worker.take()
            && let Err(e) = handle.join()
        {
            tracing::warn!(target: "tasty_lua", "lua worker join failed: {e:?}");
        }
    }
}

/// 워커 스레드 본체 — job 을 직렬로 처리한다. `Shutdown` 또는 채널 disconnect 시 종료.
fn worker_loop(lua: Lua, job_rx: Receiver<LuaJob>) {
    while let Ok(job) = job_rx.recv() {
        match job {
            LuaJob::Eval {
                source,
                name,
                reply,
            } => {
                // reply 실패 = 호출자가 대기를 포기(receiver drop) — 무시해도 안전.
                if let Err(e) = reply.send(exec_source(&lua, &source, name.as_deref())) {
                    tracing::trace!(target: "tasty_lua", "eval reply dropped: {e}");
                }
            }
            LuaJob::Run { source, name } => {
                if let Err(e) = exec_source(&lua, &source, name.as_deref()) {
                    tracing::warn!(target: "tasty_lua", "script run failed: {e}");
                }
            }
            LuaJob::Fire { event, ctx } => fire_hooks(&lua, &event, &ctx),
            LuaJob::ResetHooks { reply } => {
                if let Err(e) = reply.send(reset_hooks(&lua)) {
                    tracing::trace!(target: "tasty_lua", "reset_hooks reply dropped: {e}");
                }
            }
            LuaJob::Shutdown => break,
        }
    }
}

/// `tasty.on` dispatcher API 설치 (VM 생성 시 1회). host_api 는 별도.
fn install_hook_api(lua: &Lua) -> Result<(), LuaEngineError> {
    let registry = lua.create_table().map_err(LuaEngineError::Init)?;
    lua.set_named_registry_value(HOOKS_REGISTRY_KEY, registry)
        .map_err(LuaEngineError::Init)?;

    let tasty_table = lua.create_table().map_err(LuaEngineError::Init)?;

    let on = lua
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
    tasty_table.set("on", on).map_err(LuaEngineError::Init)?;

    lua.globals()
        .set("tasty", tasty_table)
        .map_err(LuaEngineError::Init)?;
    Ok(())
}

/// 등록된 hook 을 모두 비운다 (워커 스레드 컨텍스트).
fn reset_hooks(lua: &Lua) -> Result<(), LuaEngineError> {
    let registry = lua.create_table().map_err(LuaEngineError::Init)?;
    lua.set_named_registry_value(HOOKS_REGISTRY_KEY, registry)
        .map_err(LuaEngineError::Init)?;
    Ok(())
}

/// 임의 소스를 text-only 모드로 실행 (워커 스레드 컨텍스트).
fn exec_source(lua: &Lua, source: &str, name: Option<&str>) -> Result<(), LuaEngineError> {
    let mut chunk = lua.load(source).set_mode(mlua::ChunkMode::Text);
    if let Some(n) = name {
        chunk = chunk.set_name(n);
    }
    chunk.exec().map_err(LuaEngineError::Eval)
}

/// 이벤트 hook 발화 — 등록 콜백을 순서대로 호출. 콜백 에러는 warn 로그만 (한 hook 이 전체를 막지 않게).
fn fire_hooks(lua: &Lua, event: &str, ctx: &serde_json::Value) {
    let reg: Table = match lua.named_registry_value(HOOKS_REGISTRY_KEY) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(target: "tasty_lua", "hook registry missing: {e}");
            return;
        }
    };
    let list: Table = match reg.get::<Value>(event) {
        Ok(Value::Table(t)) => t,
        _ => return,
    };
    let lua_ctx = match lua.to_value(ctx) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(target: "tasty_lua", "ctx serialize failed for '{event}': {e}");
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
            tracing::warn!(target: "tasty_lua", "hook for '{event}' failed: {e}");
        }
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
        // fire 는 fire-and-forget 이지만 직렬 워커라 다음 블로킹 eval 이 fire 뒤에 처리됨.
        engine
            .eval("assert(_G.last_received == 42)")
            .expect("callback fired");
    }

    #[test]
    fn fire_with_no_listeners_is_noop() {
        let engine = LuaEngine::new().expect("init");
        engine.fire("nothing.subscribes", &json!({}));
        engine
            .eval("return 1")
            .expect("worker alive after empty fire");
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
        let dir =
            std::env::temp_dir().join(format!("tasty-lua-reload-test-{}", std::process::id()));
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
        engine
            .eval("assert(_G.from_old == false and _G.from_new == true)")
            .unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    // --- 워커 인프라 (TODO 01) ---

    #[test]
    fn run_script_is_serialized_with_eval() {
        // fire-and-forget run_script 후 블로킹 eval 이 그 결과를 관측 → 직렬 처리 보장.
        let engine = LuaEngine::new().expect("init");
        engine.run_script("_G.ran = (_G.ran or 0) + 1", Some("t"));
        engine.run_script("_G.ran = _G.ran + 1", Some("t"));
        engine.eval("assert(_G.ran == 2)").expect("serial order");
    }

    #[test]
    fn write_api_enqueues_host_command() {
        // 쓰기 API (`tasty.run_cli`) 는 spawn 하지 않고 커맨드 큐에 쌓는다.
        let engine = LuaEngine::new().expect("init");
        engine.eval("tasty.run_cli('list')").expect("run_cli");
        let cmds = engine.drain_commands();
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            HostCommand::RunCli(args) => assert_eq!(args, &vec!["list".to_string()]),
        }
    }

    #[test]
    fn publish_snapshot_replaces_value() {
        let engine = LuaEngine::new().expect("init");
        engine.publish_snapshot(LuaSnapshot {
            tree: vec![serde_json::json!({"id": 1})],
        });
        let snap = engine.snapshot.lock().unwrap().clone();
        assert_eq!(snap.tree.len(), 1);
    }

    #[test]
    fn tree_api_returns_published_snapshot() {
        // 워커에서 tasty.tree() 가 메인 발행 스냅샷을 Lua table 로(값복사) 반환.
        let engine = LuaEngine::new().expect("init");
        engine.publish_snapshot(LuaSnapshot {
            tree: vec![
                serde_json::json!({"active": true, "panes": []}),
                serde_json::json!({"active": false, "panes": []}),
            ],
        });
        engine
            .eval(
                r#"
                local t = tasty.tree()
                assert(#t == 2, "two workspaces")
                assert(t[1].active == true, "first active")
                assert(t[2].active == false, "second inactive")
                "#,
            )
            .expect("tree read");
    }

    #[test]
    fn tree_api_empty_when_nothing_published() {
        let engine = LuaEngine::new().expect("init");
        engine
            .eval("assert(#tasty.tree() == 0)")
            .expect("empty tree");
    }

    #[test]
    fn command_queue_backpressure_drops_over_cap() {
        // 큐 용량 초과 발행 → drop(warn), drain 은 CAP 이하로 안전 반환 (패닉 없음).
        let engine = LuaEngine::new().expect("init");
        let src = format!(
            "for i = 1, {} do tasty.run_cli('x') end",
            COMMAND_QUEUE_CAP + 50
        );
        engine.eval(&src).expect("bulk run_cli");
        let cmds = engine.drain_commands();
        assert!(cmds.len() <= COMMAND_QUEUE_CAP);
    }

    #[test]
    fn worker_survives_error_job() {
        // 에러 나는 job 후에도 워커는 다음 job 을 정상 처리 (격리).
        let engine = LuaEngine::new().expect("init");
        assert!(engine.eval("error('boom')").is_err());
        engine
            .eval("return 1 + 1")
            .expect("worker alive after error");
    }
}
