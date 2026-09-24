//! 등록된 IPC method는 그대로 두고 params 값의 자리표시자만 채워 순서대로 요청한다.
//! 스텝 결과는 로그에 남기며 호출자에게 반환하지 않는다.

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::time::Duration;

use regex::Regex;
use serde_json::Value;

use super::types::IpcCall;
use tasty_ipc::host_call::{HostIpcInjector, InjectError};

/// 한 스텝 응답을 기다릴 시간. 명령 실행을 취소하는 기한은 아니다.
const STEP_TIMEOUT: Duration = Duration::from_secs(10);

/// 호출자가 준비한 body·소문자 헤더·query 값. 여기서는 HTTP 파싱을 하지 않는다.
#[derive(Debug, Clone, Default)]
pub struct SubstitutionContext {
    pub body: Value,
    pub headers: BTreeMap<String, String>,
    pub query: BTreeMap<String, String>,
}

fn placeholder_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\$\{([^}]+)\}").expect("valid placeholder regex"))
}

/// 문자열 전체가 하나의 자리표시자일 때만 내부 참조를 반환한다.
fn whole_placeholder(s: &str) -> Option<&str> {
    let inner = s.strip_prefix("${")?.strip_suffix('}')?;
    if inner.contains("${") || inner.contains('}') {
        return None;
    }
    Some(inner)
}

fn resolve_json_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in path.split('.') {
        cur = match cur {
            Value::Object(map) => map.get(seg)?,
            Value::Array(arr) => {
                let idx: usize = seg.parse().ok()?;
                arr.get(idx)?
            }
            _ => return None,
        };
    }
    Some(cur)
}

fn resolve_ref(reference: &str, ctx: &SubstitutionContext) -> Option<Value> {
    let (scope, path) = reference.split_once('.')?;
    match scope {
        "body" => resolve_json_path(&ctx.body, path).cloned(),
        "header" => ctx
            .headers
            .get(&path.to_ascii_lowercase())
            .map(|s| Value::String(s.clone())),
        "query" => ctx.query.get(path).map(|s| Value::String(s.clone())),
        _ => None,
    }
}

fn render_embedded(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 전체가 자리표시자이면 JSON 타입을 유지하고 없으면 Null이다.
/// 문장 안의 자리표시자는 문자열로 바꾸며 찾지 못한 값은 빈 문자열이다.
fn substitute_string(s: &str, ctx: &SubstitutionContext) -> Value {
    if let Some(inner) = whole_placeholder(s) {
        return resolve_ref(inner, ctx).unwrap_or(Value::Null);
    }
    let replaced = placeholder_re().replace_all(s, |caps: &regex::Captures| {
        resolve_ref(&caps[1], ctx)
            .as_ref()
            .map(render_embedded)
            .unwrap_or_default()
    });
    Value::String(replaced.into_owned())
}

/// params의 문자열 값만 치환한다. 객체 키나 method는 바꾸지 않는다.
pub fn substitute_params(template: &Value, ctx: &SubstitutionContext) -> Value {
    match template {
        Value::String(s) => substitute_string(s, ctx),
        Value::Array(a) => Value::Array(a.iter().map(|v| substitute_params(v, ctx)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, v)| (k.clone(), substitute_params(v, ctx)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// 앞 스텝의 응답 또는 대기 종료 뒤 다음 요청을 보낸다. 실패해도 계속하며 재시도하지 않는다.
/// 응답을 기다리다 포기해도 이미 들어간 명령은 나중에 실행될 수 있다.
/// 따라서 요청 순서는 유지하지만 이전 명령의 완료를 기다린다고 보장하지는 않는다.
pub fn execute_sequence(
    origin: SequenceOrigin,
    injector: &HostIpcInjector,
    calls: &[IpcCall],
    ctx: &SubstitutionContext,
) {
    for (i, call) in calls.iter().enumerate() {
        let params = substitute_params(&call.params, ctx);
        match injector.dispatch_even_if_abandoned(&call.method, params, STEP_TIMEOUT) {
            Ok(_result) => {
                tracing::debug!("{origin} IpcSequence step {i} ({}) ok", call.method);
            }
            Err(e) => log_step_failure(origin, i, &call.method, &e),
        }
    }
}

/// 로그에 남길 호출 출처. 바인딩 허용 여부를 판단하는 TriggerSource와는 별개다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceOrigin {
    Webhook,
    SurfaceHook,
    Dispatch,
}

impl std::fmt::Display for SequenceOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Webhook => "webhook",
            Self::SurfaceHook => "surface hook",
            Self::Dispatch => "hook_handler.dispatch",
        })
    }
}

/// 실행 중인 한 건 외에 대기할 시퀀스 수. host 명령 큐와 같은 수를 쓰지만 단위는 시퀀스다.
const PENDING_SEQUENCE_LIMIT: usize = tasty_ipc::admission::INJECTED_DEPTH_LIMIT;

struct SequenceJob {
    origin: SequenceOrigin,
    handler_id: String,
    injector: HostIpcInjector,
    calls: Vec<IpcCall>,
    ctx: SubstitutionContext,
}

/// 최초 스레드 생성 실패도 OnceLock에 남겨 이후 자동 재시도하지 않는다.
fn sequence_worker() -> Option<&'static SyncSender<SequenceJob>> {
    static WORKER: OnceLock<Option<SyncSender<SequenceJob>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (tx, rx) = sync_channel::<SequenceJob>(PENDING_SEQUENCE_LIMIT);
            let spawned = std::thread::Builder::new()
                .name("hook-sequence".into())
                .spawn(move || {
                    for job in rx {
                        tracing::debug!("{} IpcSequence '{}' running", job.origin, job.handler_id);
                        execute_sequence(job.origin, &job.injector, &job.calls, &job.ctx);
                    }
                });
            match spawned {
                Ok(_) => Some(tx),
                Err(e) => {
                    tracing::error!("hook IpcSequence worker thread spawn failed: {e}");
                    None
                }
            }
        })
        .as_ref()
}

/// 명령 큐를 처리하는 스레드에서 응답을 기다리지 않도록 전용 실행기에 넘긴다.
/// 수락 순서로 시퀀스를 처리하며 가득 찼거나 worker가 없으면 실행 전에 오류를 반환한다.
/// 개별 스텝의 대기 종료 뒤 실제 명령은 여전히 실행 중일 수 있다.
pub fn enqueue_sequence(
    origin: SequenceOrigin,
    handler_id: &str,
    injector: &HostIpcInjector,
    calls: &[IpcCall],
    ctx: SubstitutionContext,
) -> Result<(), SequenceNotQueued> {
    let worker = sequence_worker().ok_or(SequenceNotQueued::WorkerUnavailable)?;
    let job = SequenceJob {
        origin,
        handler_id: handler_id.to_string(),
        injector: injector.clone(),
        calls: calls.to_vec(),
        ctx,
    };
    worker.try_send(job).map_err(|e| match e {
        TrySendError::Full(_) => SequenceNotQueued::Full,
        TrySendError::Disconnected(_) => SequenceNotQueued::WorkerStopped,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceNotQueued {
    Full,
    WorkerUnavailable,
    WorkerStopped,
}

impl std::fmt::Display for SequenceNotQueued {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => write!(f, "{PENDING_SEQUENCE_LIMIT} sequences are already waiting"),
            Self::WorkerUnavailable => f.write_str("the sequence worker thread is unavailable"),
            Self::WorkerStopped => f.write_str("the sequence worker thread has stopped"),
        }
    }
}

/// 큐에 넣지 못한 오류와 요청 이후 오류를 구분해 기록한다. 재시도는 하지 않는다.
fn log_step_failure(origin: SequenceOrigin, i: usize, method: &str, e: &InjectError) {
    if e.nothing_ran() {
        tracing::error!(
            "{origin} IpcSequence step {i} ({method}) not run — the host did not queue it: {e}"
        );
    } else {
        tracing::error!("{origin} IpcSequence step {i} ({method}) failed: {e}");
    }
}

/// command·args로 직접 프로세스를 실행하고 작업 스레드에서 output을 기다린다.
/// 셸을 자동으로 거치지 않는다. 비정상 exit status는 별도로 검사하지 않고 결과도 반환하지 않는다.
pub fn spawn_shell(command: String, args: Vec<String>, env: Vec<(String, String)>) {
    if let Err(e) = std::thread::Builder::new()
        .name("hook-shell".into())
        .spawn(move || {
            let mut cmd = std::process::Command::new(&command);
            cmd.args(&args).envs(env);
            match tasty_utils::process::hide_console(&mut cmd).output() {
                Ok(_) => tracing::debug!("hook shell '{command}' ran"),
                Err(e) => tracing::warn!("hook shell '{command}' spawn failed: {e}"),
            }
        })
    {
        tracing::warn!("hook shell thread spawn failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn capture_logs(f: impl FnOnce()) -> String {
        use std::sync::{Arc, Mutex};
        #[derive(Clone)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buf = Buf(Arc::new(Mutex::new(Vec::new())));
        let sink = buf.clone();
        let sub = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || sink.clone())
            .finish();
        // 다른 검사가 먼저 등록한 tracing 콜사이트도 현재 구독자로 수집하도록 캐시를 갱신한다.
        tracing::subscriber::with_default(sub, || {
            tracing::callsite::rebuild_interest_cache();
            f()
        });
        let out = buf.0.lock().unwrap().clone();
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn step_logs_name_the_origin_that_fired_the_sequence() {
        use std::sync::{Arc, mpsc};
        use tasty_ipc::server::IpcCommand;

        for (origin, label) in [
            (SequenceOrigin::Webhook, "webhook"),
            (SequenceOrigin::SurfaceHook, "surface hook"),
            (SequenceOrigin::Dispatch, "hook_handler.dispatch"),
        ] {
            let (tx, rx) = mpsc::channel::<IpcCommand>();
            let injector = HostIpcInjector::new(tx, Arc::new(|| {}));
            let answerer = std::thread::spawn(move || {
                for ok in [true, false] {
                    let cmd = rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("a step reaches the queue");
                    let id = cmd.request.id.clone().unwrap_or(Value::Null);
                    let resp = if ok {
                        tasty_ipc::protocol::JsonRpcResponse::success(id, json!({}))
                    } else {
                        tasty_ipc::protocol::JsonRpcResponse::internal_error(id, "probe refused")
                    };
                    cmd.response_tx.send(resp).expect("answer the step");
                }
            });
            let calls = [
                IpcCall {
                    method: "probe.ok".into(),
                    params: json!({}),
                },
                IpcCall {
                    method: "probe.refused".into(),
                    params: json!({}),
                },
            ];
            let logs = capture_logs(|| {
                execute_sequence(origin, &injector, &calls, &SubstitutionContext::default())
            });
            answerer.join().expect("answerer thread");
            assert!(
                logs.contains(&format!("{label} IpcSequence step 0 (probe.ok) ok")),
                "{origin:?}: {logs}"
            );
            assert!(
                logs.contains(&format!(
                    "{label} IpcSequence step 1 (probe.refused) failed"
                )),
                "{origin:?}: {logs}"
            );
            if origin != SequenceOrigin::Webhook {
                assert!(!logs.contains("webhook"), "{origin:?}: {logs}");
            }
        }
    }

    fn ctx() -> SubstitutionContext {
        let mut headers = BTreeMap::new();
        headers.insert("x-signature".to_string(), "abc123".to_string());
        let mut query = BTreeMap::new();
        query.insert("token".to_string(), "qtok".to_string());
        SubstitutionContext {
            body: json!({"message": "hi", "nested": {"n": 42}, "arr": [1, 2, 3]}),
            headers,
            query,
        }
    }

    #[test]
    fn whole_placeholder_preserves_type() {
        let out = substitute_params(&json!("${body.nested.n}"), &ctx());
        assert_eq!(out, json!(42));
    }

    #[test]
    fn whole_placeholder_string_value() {
        let out = substitute_params(&json!("${body.message}"), &ctx());
        assert_eq!(out, json!("hi"));
    }

    #[test]
    fn embedded_placeholder_is_textual() {
        let out = substitute_params(&json!("msg: ${body.message}!"), &ctx());
        assert_eq!(out, json!("msg: hi!"));
    }

    #[test]
    fn header_and_query_refs() {
        let out = substitute_params(
            &json!({"sig": "${header.X-Signature}", "tok": "${query.token}"}),
            &ctx(),
        );
        assert_eq!(out, json!({"sig": "abc123", "tok": "qtok"}));
    }

    #[test]
    fn array_index_path() {
        let out = substitute_params(&json!("${body.arr.1}"), &ctx());
        assert_eq!(out, json!(2));
    }

    #[test]
    fn unresolved_ref_is_null_or_empty() {
        assert_eq!(
            substitute_params(&json!("${body.missing}"), &ctx()),
            json!(null)
        );
        assert_eq!(
            substitute_params(&json!("x=${body.missing}"), &ctx()),
            json!("x=")
        );
    }

    #[test]
    fn object_keys_are_never_substituted() {
        let out = substitute_params(&json!({"${body.message}": "v"}), &ctx());
        assert_eq!(out, json!({"${body.message}": "v"}));
    }

    /// 직접 프로세스 실행 경로에서 환경변수가 전달되는지 확인한다.
    #[test]
    fn spawn_shell_exposes_env_to_child() {
        use std::time::{Duration, Instant};
        let dir = tempfile::tempdir().expect("tempdir");
        let marker = dir.path().join("env.txt");
        // 셸 확장을 검사하려고 셸 자체를 실행한다. 임시 경로에 공백이 없다는 전제가 있다.
        let (command, args) = if cfg!(windows) {
            (
                "cmd".to_string(),
                vec![
                    "/C".to_string(),
                    format!("echo %TASTY_HOOK_EVENT%> {}", marker.display()),
                ],
            )
        } else {
            (
                "sh".to_string(),
                vec![
                    "-c".to_string(),
                    format!("echo \"$TASTY_HOOK_EVENT\" > {}", marker.display()),
                ],
            )
        };
        spawn_shell(
            command,
            args,
            vec![("TASTY_HOOK_EVENT".to_string(), "user/envtest".to_string())],
        );
        // 리다이렉트는 쓰기 전에 파일을 만들 수 있으므로 존재가 아닌 내용을 기다린다.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut content = String::new();
        while Instant::now() < deadline {
            content = std::fs::read_to_string(&marker).unwrap_or_default();
            if !content.trim().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(
            content.trim(),
            "user/envtest",
            "5초 안에 기대한 환경변수 출력을 읽지 못했다; marker exists={}",
            marker.exists()
        );
    }

    #[test]
    fn nested_object_and_array_recurse() {
        let out = substitute_params(
            &json!({"a": ["${body.message}", {"b": "${body.nested.n}"}]}),
            &ctx(),
        );
        assert_eq!(out, json!({"a": ["hi", {"b": 42}]}));
    }

    #[test]
    fn agent_stream_turn_correlation_sequence_substitutes_only_value_slots() {
        // request_id와 message만 치환하고 등록된 surface 값은 유지하는 입력이다.
        let ctx = SubstitutionContext {
            body: json!({"request_id": "req-8f3a", "prompt": "summarize the build log"}),
            ..Default::default()
        };
        let turn_start_params = substitute_params(
            &json!({"surface": 42, "request_id": "${body.request_id}"}),
            &ctx,
        );
        assert_eq!(
            turn_start_params,
            json!({"surface": 42, "request_id": "req-8f3a"}),
            "request_id 는 값 슬롯에서 치환되고 surface 리터럴은 그대로다"
        );
        let tell_params =
            substitute_params(&json!({"message": "${body.prompt}", "surface": 42}), &ctx);
        assert_eq!(
            tell_params,
            json!({"message": "summarize the build log", "surface": 42})
        );
        let key_shaped = substitute_params(&json!({"${body.request_id}": "x"}), &ctx);
        assert_eq!(
            key_shaped,
            json!({"${body.request_id}": "x"}),
            "key 위치는 절대 치환하지 않는다"
        );
    }
}
