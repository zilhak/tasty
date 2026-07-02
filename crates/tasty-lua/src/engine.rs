//! [`LuaEngine`] — Lua VM 을 소유하는 **워커 스레드** 핸들 (ADR-0031).
//!
//! VM 은 전용 워커 스레드에서만 접근한다. 메인 스레드는 이 핸들을 통해
//! 실행 job 을 보내고(직렬 처리), 워커가 쌓은 [`HostCommand`] 를 drain 하며,
//! 읽기전용 [`LuaSnapshot`] 을 발행한다. 메인과 워커는 이 경계 밖에서 state 를 공유하지 않는다.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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

/// 스크립트 1회 실행 wall-clock deadline 기본값. 초과 시 `set_interrupt` 가 abort.
/// 무한 루프/폭주 스크립트가 워커 스레드를 영원히 점유하는 것을 막는다 (ADR-0031).
/// 정상 스크립트 오탐을 피하려 넉넉히 잡는다 (초과 시 ADR Reconsideration 대상).
const SCRIPT_DEADLINE: Duration = Duration::from_secs(5);

/// 현재 실행 중인 job 의 절대 deadline. worker 가 job 시작 시 설정, 종료 시 clear.
/// `set_interrupt` 훅(같은 워커 스레드)이 읽어 초과를 판정한다.
type SharedDeadline = Arc<Mutex<Option<Instant>>>;

/// 스크립트 실행 완료 추적용 RAII 토큰 (자동실행 재진입 가드 배관).
///
/// 스크립트 실행이 끝나면(성공·에러·deadline abort 무관) — 또는 job 이 큐 포화로
/// drop 되거나 워커가 죽어 실행되지 못하면 — 공유 counter 가 1 증가한다.
/// Drop 기반이라 어떤 경로로도 "완료 신호 누락 → 가드 영구 잠김" 이 생기지 않는다.
pub struct CompletionToken(Arc<AtomicU64>);

impl CompletionToken {
    /// `counter` 를 완료 시 1 증가시키는 토큰 생성.
    pub fn new(counter: Arc<AtomicU64>) -> Self {
        Self(counter)
    }
}

impl Drop for CompletionToken {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// 워커 스레드에 보내는 실행 요청. 워커는 이들을 **직렬**로 처리한다.
enum LuaJob {
    /// 임의 소스 실행 + 결과 회신 (블로킹 호출용 — 테스트/부팅 로드).
    Eval {
        source: String,
        name: Option<String>,
        reply: SyncSender<Result<(), LuaEngineError>>,
    },
    /// 임의 소스 실행 (fire-and-forget — 단축키/디버그/자동실행 트리거).
    Run {
        source: String,
        name: Option<String>,
        /// 실행 종료 시 drop 되어 완료를 알린다 (자동실행 경로만 Some).
        token: Option<CompletionToken>,
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
    worker: Option<JoinHandle<()>>,
}

impl LuaEngine {
    /// 새 VM 생성 + 샌드박스 + API 설치 후 워커 스레드로 이동. 앱 부팅 시 1회.
    pub fn new() -> Result<Self, LuaEngineError> {
        Self::with_deadline(SCRIPT_DEADLINE)
    }

    /// [`LuaEngine::new`] 와 동일하되 스크립트 실행 deadline 을 지정한다 (테스트용 짧은 budget).
    pub fn with_deadline(budget: Duration) -> Result<Self, LuaEngineError> {
        let lua = Lua::new();
        crate::sandbox::apply(&lua)?;

        let (command_tx, command_rx) = sync_channel(COMMAND_QUEUE_CAP);
        let snapshot: SharedSnapshot = Arc::new(Mutex::new(Arc::new(LuaSnapshot::default())));
        let deadline: SharedDeadline = Arc::new(Mutex::new(None));

        install_hook_api(&lua)?;
        install_interrupt(&lua, deadline.clone());
        crate::host_api::install(&lua, command_tx, snapshot.clone())?;

        let (job_tx, job_rx) = sync_channel(JOB_QUEUE_CAP);
        let worker = std::thread::Builder::new()
            .name("tasty-lua-worker".to_string())
            .spawn(move || worker_loop(lua, job_rx, budget, deadline))?;

        Ok(Self {
            job_tx,
            command_rx,
            snapshot,
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
                token: None,
            },
            "run_script",
        );
    }

    /// [`LuaEngine::run_script`] 와 동일하되 실행 종료 시 `token` 을 drop 해 완료를
    /// 알린다. 자동실행(autofire) 재진입 가드가 이 신호로 in-flight 를 추적한다.
    /// 큐 포화 등으로 job 이 실행되지 못해도 token 은 drop 된다 (가드 잠김 방지).
    pub fn run_script_tracked(&self, source: &str, name: Option<&str>, token: CompletionToken) {
        self.send_ff(
            LuaJob::Run {
                source: source.to_string(),
                name: name.map(str::to_string),
                token: Some(token),
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
///
/// Lua 를 실행하는 job(Eval/Run/Fire)은 `budget` deadline 하에서 돈다 — 초과 시
/// `set_interrupt` 훅이 abort(에러 반환)해 워커만 다음 job 으로 넘어가고 메인은 무영향.
fn worker_loop(lua: Lua, job_rx: Receiver<LuaJob>, budget: Duration, deadline: SharedDeadline) {
    while let Ok(job) = job_rx.recv() {
        match job {
            LuaJob::Eval {
                source,
                name,
                reply,
            } => {
                let result = guarded(&deadline, budget, || {
                    exec_source(&lua, &source, name.as_deref())
                });
                // reply 실패 = 호출자가 대기를 포기(receiver drop) — 무시해도 안전.
                if let Err(e) = reply.send(result) {
                    tracing::trace!(target: "tasty_lua", "eval reply dropped: {e}");
                }
            }
            LuaJob::Run {
                source,
                name,
                token,
            } => {
                let result = guarded(&deadline, budget, || {
                    exec_source(&lua, &source, name.as_deref())
                });
                if let Err(e) = result {
                    tracing::warn!(target: "tasty_lua", "script run failed: {e}");
                }
                // 실행이 끝난 뒤에야 완료를 알린다 — 재진입 가드의 in-flight 판정 기준.
                drop(token);
            }
            LuaJob::Fire { event, ctx } => {
                // fire_hooks 는 콜백 에러를 자체 로그하고 삼키므로 여기 Err 는 사실상
                // 나지 않지만, deadline 배관 일관성을 위해 guarded 로 감싸고 방어적으로 로그.
                if let Err(e) = guarded(&deadline, budget, || {
                    fire_hooks(&lua, &event, &ctx);
                    Ok(())
                }) {
                    tracing::warn!(target: "tasty_lua", "fire '{event}' aborted: {e}");
                }
            }
            LuaJob::ResetHooks { reply } => {
                if let Err(e) = reply.send(reset_hooks(&lua)) {
                    tracing::trace!(target: "tasty_lua", "reset_hooks reply dropped: {e}");
                }
            }
            LuaJob::Shutdown => break,
        }
    }
}

/// N VM 명령마다 호출되는 hook 간격. 낮을수록 deadline 해상도↑·오버헤드↑.
/// 10k 이면 tight loop 에서 sub-ms 해상도로 abort 하면서 정상 실행 오버헤드는 무시할 수준.
const INTERRUPT_EVERY_N_INSTRUCTIONS: u32 = 10_000;

/// deadline 훅 설치. Lua 5.4 는 Luau `set_interrupt` 대신 instruction-count `set_hook` 를
/// 쓴다 (동일 메커니즘 — VM 이 주기적으로 호출하는 훅에서 deadline 초과 시 에러 반환 → abort).
fn install_interrupt(lua: &Lua, deadline: SharedDeadline) {
    let triggers = mlua::HookTriggers::new().every_nth_instruction(INTERRUPT_EVERY_N_INSTRUCTIONS);
    lua.set_hook(triggers, move |_, _| {
        let expired = match deadline.lock() {
            Ok(guard) => guard.is_some_and(|dl| Instant::now() >= dl),
            Err(_) => false,
        };
        if expired {
            Err(mlua::Error::runtime(
                "script exceeded time budget (deadline)",
            ))
        } else {
            Ok(mlua::VmState::Continue)
        }
    });
}

/// job deadline 을 설정한 뒤 `f` 를 실행하고, 종료 시 deadline 을 clear 한다.
fn guarded<F>(deadline: &SharedDeadline, budget: Duration, f: F) -> Result<(), LuaEngineError>
where
    F: FnOnce() -> Result<(), LuaEngineError>,
{
    set_deadline(deadline, Some(Instant::now() + budget));
    let result = f();
    set_deadline(deadline, None);
    result
}

fn set_deadline(deadline: &SharedDeadline, value: Option<Instant>) {
    match deadline.lock() {
        Ok(mut guard) => *guard = value,
        Err(e) => tracing::warn!(target: "tasty_lua", "deadline lock poisoned: {e}"),
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

    // --- deadline / 무한루프 방어 (TODO 07) ---

    #[test]
    fn infinite_loop_aborts_within_deadline() {
        let engine = LuaEngine::with_deadline(Duration::from_millis(50)).expect("init");
        let start = Instant::now();
        let result = engine.eval("while true do end");
        assert!(result.is_err(), "무한 루프는 deadline 으로 abort 되어야 함");
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "deadline 내 즉시 abort (실제 {:?})",
            start.elapsed()
        );
    }

    #[test]
    fn worker_survives_after_deadline_abort() {
        // abort 후 deadline 은 clear 되고 워커는 후속 정상 job 을 처리한다.
        let engine = LuaEngine::with_deadline(Duration::from_millis(50)).expect("init");
        assert!(engine.eval("while true do end").is_err());
        engine
            .eval("return 1 + 1")
            .expect("worker alive after deadline abort");
    }

    #[test]
    fn normal_script_completes_under_deadline() {
        // 정상 스크립트는 오탐 abort 없이 완료 (deadline 여유 충분).
        let engine = LuaEngine::with_deadline(Duration::from_secs(5)).expect("init");
        engine
            .eval("local s = 0; for i = 1, 100000 do s = s + i end")
            .expect("normal loop under budget");
    }

    // --- 완료 추적 (자동실행 재진입 가드 배관) ---

    #[test]
    fn tracked_run_signals_completion_after_execution() {
        let engine = LuaEngine::new().expect("init");
        let counter = Arc::new(AtomicU64::new(0));
        engine.run_script_tracked(
            "local x = 1",
            Some("t"),
            CompletionToken::new(counter.clone()),
        );
        // 워커는 job 을 직렬 처리하므로, 후속 블로킹 eval 이 돌아오면 run 은 끝난 상태.
        engine.eval("return 0").expect("worker alive");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn tracked_run_completes_even_on_deadline_abort() {
        // 자동실행 경로(LuaJob::Run)도 deadline 이 그대로 적용된다 — 무한 루프는
        // abort 되고, abort 여도 완료 token 은 drop 되어 가드가 풀린다.
        let engine = LuaEngine::with_deadline(Duration::from_millis(50)).expect("init");
        let counter = Arc::new(AtomicU64::new(0));
        engine.run_script_tracked(
            "while true do end",
            Some("loop"),
            CompletionToken::new(counter.clone()),
        );
        engine.eval("return 0").expect("worker alive after abort");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
