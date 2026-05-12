//! Plugin → 호스트 동기 IPC 호출 헬퍼.
//!
//! SDK 메시지 루프가 메인(receiver) + worker(dispatcher)로 갈라져 있을 때만
//! 의미가 있다. worker가 [`HostHandle::call`]을 호출하면 IpcCall event가
//! 송신되고, 메인 스레드가 호스트로부터 받은 `ipc.result`를 oneshot channel로
//! 결과를 push한다. worker는 그 결과가 올 때까지 block.

use std::collections::HashMap;
use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tasty_plugin_protocol::PluginEvent;

use crate::error::PluginError;

/// 호스트 호출 결과. 워크스페이스 plugin이 `match` 분기에 쓰지 않고 `?`로만
/// 흘려보내는 경우가 대부분이라, 표준 [`PluginError`]로 합쳤다.
pub(crate) type HostCallResult = Result<Value, PluginError>;
pub(crate) type PendingCalls = Arc<Mutex<HashMap<u64, mpsc::Sender<HostCallResult>>>>;

/// 옛 이름 호환을 위한 alias. 신규 코드는 [`PluginError`] 사용.
#[deprecated(note = "PluginError로 통합됨. 새 코드는 PluginError 사용.")]
pub type HostCallError = PluginError;

/// Plugin 코드가 호스트 IPC 메서드를 동기로 호출하는 진입점.
///
/// `Clone` 가능 — plugin이 자기 스레드로 옮겨도 동작한다. 내부의 writer는
/// `Arc<Mutex<TcpStream>>`로 보호되어 동시 호출 안전.
#[derive(Clone)]
pub struct HostHandle {
    writer: Arc<Mutex<TcpStream>>,
    pending: PendingCalls,
    next_call_id: Arc<AtomicU64>,
    /// host call의 최대 대기 시간. 기본 60초. 호출 전에 변경 가능.
    pub timeout: Duration,
}

impl HostHandle {
    pub(crate) fn new(writer: Arc<Mutex<TcpStream>>, pending: PendingCalls) -> Self {
        Self {
            writer,
            pending,
            next_call_id: Arc::new(AtomicU64::new(1)),
            timeout: Duration::from_secs(60),
        }
    }

    /// 호스트 IPC 메서드를 동기로 호출한다. 응답까지 [`Self::timeout`]만큼 block.
    pub fn call(
        &self,
        method: impl Into<String>,
        params: Value,
    ) -> Result<Value, PluginError> {
        let method_str = method.into();
        let call_id = self.next_call_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel::<HostCallResult>();
        {
            let mut p = self
                .pending
                .lock()
                .map_err(|_| PluginError::LockPoisoned("host pending"))?;
            p.insert(call_id, tx);
        }
        let event = PluginEvent::IpcCall {
            call_id,
            method: method_str.clone(),
            params,
        };
        let payload = serde_json::json!({ "event": event });
        let line = serde_json::to_string(&payload)?;
        {
            let mut w = self
                .writer
                .lock()
                .map_err(|_| PluginError::LockPoisoned("host writer"))?;
            writeln!(*w, "{line}")?;
            w.flush()?;
        }
        match rx.recv_timeout(self.timeout) {
            Ok(result) => result,
            Err(_) => {
                if let Ok(mut p) = self.pending.lock() {
                    p.remove(&call_id);
                }
                Err(PluginError::HostCallTimeout {
                    method: method_str,
                    timeout: self.timeout,
                })
            }
        }
    }
}

/// 메인 recv 스레드가 호스트로부터 받은 `ipc.result` 요청을 처리한다.
/// `result`/`error`를 매칭되는 oneshot sender로 전달.
///
/// 메서드 이름은 pending map에 저장하지 않으므로 에러 표시에는 `call#<id>`를
/// 쓴다. 호출자가 [`HostHandle::call`]의 반환 에러를 보면 timeout variant에는
/// method가 들어있다 (HostCall variant는 호스트 에러 응답일 때만 사용).
pub(crate) fn deliver_ipc_result(
    pending: &PendingCalls,
    call_id: u64,
    result: Option<Value>,
    error: Option<String>,
) {
    let sender = pending.lock().ok().and_then(|mut p| p.remove(&call_id));
    if let Some(tx) = sender {
        let out = match error {
            Some(message) => Err(PluginError::HostCall {
                method: format!("call#{call_id}"),
                message,
            }),
            None => Ok(result.unwrap_or(Value::Null)),
        };
        let _ = tx.send(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트 전용: 더미 stream으로 HostHandle 생성. 실제로 call하면 write
    /// 실패하므로 호출은 하지 말 것. timeout 동작 검증 등 internal state 테스트용.
    fn dummy_handle() -> HostHandle {
        // TcpStream::connect로 실제 소켓을 안 만들고 가짜를 만들려면 listener
        // 페어가 필요 — 여기서는 PendingCalls만 검증한다.
        let pending: PendingCalls = Arc::new(Mutex::new(HashMap::new()));
        let listener =
            std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind localhost");
        let port = listener.local_addr().unwrap().port();
        let accept_handle = std::thread::spawn(move || {
            let _ = listener.accept();
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        accept_handle.join().unwrap();
        HostHandle::new(Arc::new(Mutex::new(stream)), pending)
    }

    #[test]
    fn deliver_unblocks_waiter() {
        let handle = dummy_handle();
        handle.pending.lock().unwrap().insert(7, {
            let (tx, _rx) = mpsc::channel::<HostCallResult>();
            tx
        });
        // 모르는 call_id는 조용히 무시.
        deliver_ipc_result(&handle.pending, 999, Some(Value::Null), None);
        assert!(handle.pending.lock().unwrap().contains_key(&7));

        // 알려진 call_id는 pending에서 제거.
        deliver_ipc_result(&handle.pending, 7, Some(Value::Null), None);
        assert!(!handle.pending.lock().unwrap().contains_key(&7));
    }

    #[test]
    fn deliver_passes_error_string() {
        let handle = dummy_handle();
        let (tx, rx) = mpsc::channel::<HostCallResult>();
        handle.pending.lock().unwrap().insert(5, tx);
        deliver_ipc_result(&handle.pending, 5, None, Some("denied".into()));
        let err = rx.recv().unwrap().unwrap_err();
        match err {
            PluginError::HostCall { message, .. } => assert_eq!(message, "denied"),
            other => panic!("expected HostCall variant, got {other:?}"),
        }
    }

    #[test]
    fn deliver_passes_result_value() {
        let handle = dummy_handle();
        let (tx, rx) = mpsc::channel::<HostCallResult>();
        handle.pending.lock().unwrap().insert(11, tx);
        deliver_ipc_result(&handle.pending, 11, Some(serde_json::json!({"ok": 1})), None);
        let v = rx.recv().unwrap().unwrap();
        assert_eq!(v, serde_json::json!({"ok": 1}));
    }
}
