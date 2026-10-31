//! client 쪽 원본 터미널 읽기. IPC 핸들러가 요청을 넣고 응답을 미루면, 메인 루프가 holder 연결로 보내고
//! 결과를 받아 둔다. 미룬 응답은 [`reply`] 가 거둔다.
//!
//! mirror 마크는 client 메모리에 (workspace, 원격 surface) 기준으로 서버 stream·위치를 둔다. 서버의 공유
//! 마크는 읽지도 바꾸지도 않는다. 결과는 보낸 연결 세대와 같을 때만 받는다.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tasty_ipc::protocol::JsonRpcResponse;
use tasty_ipc::terminal_read::{ReadFrom, ReadOp, ReadOutcome, TerminalReadMsg};
use tasty_remote::connection::ConnectionEpoch;

/// 미룬 응답의 상한. 결과 프레임을 잃으면 이 시간 뒤 `remote_timeout` 으로 끝난다.
pub(crate) const REPLY_LIMIT: Duration = Duration::from_secs(10);

/// 핸들러가 넣는 요청.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Request {
    Screen {
        lines: Option<usize>,
        show_dim: bool,
    },
    /// `cursor` 가 없으면 mirror 마크(없으면 보관 시작)부터 읽는다.
    Output {
        cursor: Option<(u64, String)>,
        max_bytes: Option<usize>,
        strip_ansi: bool,
    },
    /// `surface.set_mark`. 서버 위치를 받은 뒤에만 로컬 마크와 mirror 마크를 함께 바꾼다.
    Mark,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReadError {
    pub code: i32,
    pub message: String,
    pub data: Value,
}

fn failure(reason: &str, message: impl Into<String>) -> ReadError {
    ReadError {
        code: -32000,
        message: message.into(),
        data: json!({ "reason": reason }),
    }
}

struct Sent {
    epoch: ConnectionEpoch,
    remote: u32,
    /// 이 읽기가 mirror 마크에서 시작했다. stream 이 다르면 마크를 지우고 다시 설정하라고 알린다.
    from_mark: bool,
}

struct Pending {
    workspace: u32,
    local: u32,
    request: Request,
    started: Instant,
    sent: Option<Sent>,
    /// 받은 결과. 마크 요청은 로컬 마크를 적용한 뒤에만 채운다.
    done: Option<Result<Value, ReadError>>,
    /// 받은 마크 위치를 적용하기 전에 보관한다.
    position: Option<(String, u64)>,
}

#[derive(Default)]
pub(crate) struct Client {
    next_id: u64,
    /// workspace → (서버가 지원하는가, 연결 상태가 Connected 인가).
    sessions: HashMap<u32, (bool, bool)>,
    pending: HashMap<u64, Pending>,
    marks: HashMap<(u32, u32), (String, u64)>,
}

static CLIENT: LazyLock<Mutex<Client>> = LazyLock::new(|| Mutex::new(Client::default()));

pub(crate) fn client() -> MutexGuard<'static, Client> {
    CLIENT.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("attach terminal read state was poisoned; keeping its entries");
        poisoned.into_inner()
    })
}

fn unsupported() -> ReadError {
    failure(
        "remote_read_unsupported",
        "the remote Tasty does not support reading its terminal over attach, and this mirror's \
         own copy lacks output from before the attach; update the remote Tasty",
    )
}

/// 세션이 원본 읽기를 지원하지 않거나 연결되지 않았을 때 핸들러가 바로 답할 오류.
pub(crate) fn unavailable(workspace: u32) -> Option<ReadError> {
    match client().sessions.get(&workspace).copied() {
        None => Some(failure(
            "remote_disconnected",
            "the remote attach session of this mirror is not connected",
        )),
        Some((false, _)) => Some(unsupported()),
        Some((true, false)) => Some(failure(
            "remote_disconnected",
            "the remote attach session of this mirror is reconnecting; retry after it connects",
        )),
        Some((true, true)) => None,
    }
}

pub(crate) fn start(workspace: u32, local: u32, request: Request, now: Instant) -> u64 {
    client().begin(workspace, local, request, now)
}

/// 한 세션의 연결 상태. 메인 루프가 세션마다 매번 알린다.
pub(crate) struct SessionView<'a> {
    pub workspace: u32,
    pub capable: bool,
    pub connected: bool,
    pub epoch: ConnectionEpoch,
    pub remote_to_local: &'a HashMap<u32, u32>,
}

impl Client {
    fn begin(&mut self, workspace: u32, local: u32, request: Request, now: Instant) -> u64 {
        self.next_id += 1;
        let request_id = self.next_id;
        self.pending.insert(
            request_id,
            Pending {
                workspace,
                local,
                request,
                started: now,
                sent: None,
                done: None,
                position: None,
            },
        );
        request_id
    }

    fn take_reply(&mut self, request_id: u64, id: Value, now: Instant) -> Option<JsonRpcResponse> {
        let p = self.pending.get_mut(&request_id)?;
        let done = match p.done.take() {
            Some(done) => done,
            None if now.duration_since(p.started) >= REPLY_LIMIT => Err(failure(
                "remote_timeout",
                format!(
                    "the remote terminal read did not answer within {}s",
                    REPLY_LIMIT.as_secs()
                ),
            )),
            None => return None,
        };
        self.pending.remove(&request_id);
        Some(match done {
            Ok(result) => JsonRpcResponse::success(id, result),
            Err(e) => JsonRpcResponse::error_with_data(id, e.code, e.message, e.data),
        })
    }

    /// 세션 목록을 갱신하고 끊긴 세션의 대기 읽기를 끝낸다. 보낼 요청을 (workspace, 메시지)로 돌려준다.
    pub(crate) fn sync(&mut self, sessions: &[SessionView<'_>]) -> Vec<(u32, TerminalReadMsg)> {
        self.sessions = sessions
            .iter()
            .map(|s| (s.workspace, (s.capable, s.connected)))
            .collect();
        let live: Vec<u32> = sessions.iter().map(|s| s.workspace).collect();
        self.marks.retain(|(ws, _), _| live.contains(ws));
        let mut out = Vec::new();
        let marks = &self.marks;
        for (&request_id, p) in self.pending.iter_mut() {
            if p.done.is_some() {
                continue;
            }
            let Some(view) = sessions.iter().find(|s| s.workspace == p.workspace) else {
                p.done = Some(Err(failure(
                    "remote_disconnected",
                    "the remote attach session of this mirror ended before the read answered",
                )));
                continue;
            };
            if let Some(sent) = &p.sent {
                if !view.connected || !sent.epoch.same(&view.epoch) {
                    p.done = Some(Err(failure(
                        "remote_disconnected",
                        "the remote attach connection was lost before the read answered",
                    )));
                }
                continue;
            }
            if !view.capable {
                p.done = Some(Err(unsupported()));
                continue;
            }
            if !view.connected {
                continue;
            }
            let Some(remote) = view
                .remote_to_local
                .iter()
                .find(|(_, l)| **l == p.local)
                .map(|(r, _)| *r)
            else {
                p.done = Some(Err(failure(
                    "no_terminal",
                    format!("surface {} is no longer in the mirror", p.local),
                )));
                continue;
            };
            let (op, from_mark) = to_op(&p.request, marks.get(&(p.workspace, remote)));
            out.push((
                p.workspace,
                TerminalReadMsg::TerminalReadRequest {
                    request_id,
                    surface_id: remote,
                    op,
                },
            ));
            p.sent = Some(Sent {
                epoch: view.epoch.clone(),
                remote,
                from_mark,
            });
        }
        out
    }

    /// 보내지 못한 요청을 실패로 끝낸다.
    pub(crate) fn send_failed(&mut self, request_id: u64) {
        if let Some(p) = self.pending.get_mut(&request_id) {
            p.done = Some(Err(failure(
                "remote_disconnected",
                "the read could not be sent on the remote attach connection",
            )));
        }
    }

    /// 위치를 받은 마크 요청을 (요청, 로컬 surface)로 꺼낸다. 호출자가 로컬 마크를 적용하고 [`Client::mark_applied`] 한다.
    pub(crate) fn marks_to_apply(&self) -> Vec<(u64, u32)> {
        self.pending
            .iter()
            .filter(|(_, p)| p.position.is_some() && p.done.is_none())
            .map(|(id, p)| (*id, p.local))
            .collect()
    }

    /// 로컬 마크를 적용한 결과. 적용하지 못했으면 mirror 마크도 바꾸지 않는다.
    pub(crate) fn mark_applied(&mut self, request_id: u64, applied: bool) {
        let Some(p) = self.pending.get_mut(&request_id) else {
            return;
        };
        let (Some(position), Some(sent)) = (p.position.take(), p.sent.as_ref()) else {
            return;
        };
        if applied {
            self.marks.insert((p.workspace, sent.remote), position);
            p.done = Some(Ok(
                json!({ "ok": true, "surface_id": p.local, "remote": true }),
            ));
        } else {
            p.done = Some(Err(failure(
                "no_terminal",
                format!("surface {} has no mirror terminal to mark", p.local),
            )));
        }
    }

    fn receive(&mut self, epoch: &ConnectionEpoch, request_id: u64, outcome: ReadOutcome) {
        let Some(p) = self.pending.get_mut(&request_id) else {
            return;
        };
        let Some(sent) = &p.sent else {
            return;
        };
        // 재연결 뒤 도착한 이전 세대의 결과는 버린다. 대기 읽기는 끊김으로 이미 끝났다.
        if p.done.is_some() || !sent.epoch.same(epoch) {
            return;
        }
        match (outcome, &p.request) {
            (ReadOutcome::Ok { result }, Request::Mark) => {
                let stream = result["stream"].as_str().map(str::to_owned);
                let end = result["retention_end"].as_u64();
                match (stream, end) {
                    (Some(stream), Some(end)) => p.position = Some((stream, end)),
                    _ => {
                        p.done = Some(Err(failure(
                            "remote_protocol",
                            "the remote position answer had no stream or retention_end",
                        )))
                    }
                }
            }
            (ReadOutcome::Ok { mut result }, _) => {
                if let Some(map) = result.as_object_mut() {
                    map.insert("surface_id".into(), json!(p.local));
                    map.insert("remote_surface_id".into(), json!(sent.remote));
                    map.insert("remote".into(), json!(true));
                }
                p.done = Some(Ok(result));
            }
            (
                ReadOutcome::Err {
                    code,
                    message,
                    data,
                },
                _,
            ) => {
                let stale_mark = sent.from_mark && data["reason"] == "stream_mismatch";
                if stale_mark {
                    self.marks.remove(&(p.workspace, sent.remote));
                }
                let message = if stale_mark {
                    format!(
                        "the remote terminal was replaced since set mark ran on this mirror, so \
                         output in between may be lost. Run set mark again ({message})"
                    )
                } else {
                    message
                };
                p.done = Some(Err(ReadError {
                    code,
                    message,
                    data,
                }));
            }
        }
    }
}

fn to_op(request: &Request, mark: Option<&(String, u64)>) -> (ReadOp, bool) {
    match request {
        Request::Screen { lines, show_dim } => (
            ReadOp::ScreenText {
                lines: *lines,
                show_dim: *show_dim,
            },
            false,
        ),
        Request::Mark => (ReadOp::Position, false),
        Request::Output {
            cursor,
            max_bytes,
            strip_ansi,
        } => {
            let (from, from_mark) = match (cursor, mark) {
                (Some((cursor, stream)), _) => (
                    ReadFrom::Cursor {
                        cursor: *cursor,
                        stream: stream.clone(),
                    },
                    false,
                ),
                (None, Some((stream, cursor))) => (
                    ReadFrom::Cursor {
                        cursor: *cursor,
                        stream: stream.clone(),
                    },
                    true,
                ),
                (None, None) => (ReadFrom::Start, false),
            };
            (
                ReadOp::Output {
                    from,
                    max_bytes: *max_bytes,
                    strip_ansi: *strip_ansi,
                },
                from_mark,
            )
        }
    }
}

/// 받은 결과를 보관한다. `epoch` 은 결과가 도착한 연결 세대다.
pub(crate) fn receive(epoch: &ConnectionEpoch, request_id: u64, outcome: ReadOutcome) {
    client().receive(epoch, request_id, outcome);
}

/// 미룬 응답. 아직이면 None 이다. 상한을 넘으면 `remote_timeout` 으로 끝낸다.
pub(crate) fn reply(request_id: u64, id: Value, now: Instant) -> Option<JsonRpcResponse> {
    client().take_reply(request_id, id, now)
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
