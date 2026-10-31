//! attach holder 의 원본 터미널 읽기(`terminal_read_request`). 서버는 로컬 IPC 와 같은 추출 함수로
//! 자기 터미널을 읽어 같은 holder 연결로 답한다. client 쪽 대기·마크는 [`client`] 에 있다.
//!
//! 읽을 수 있는 대상은 요청 연결이 hard lock 을 가진 터미널뿐이다. workspace 를 점유하면 그 터미널
//! 멤버도 같은 holder 의 lock 을 받는다. 다른 workspace 를 점유했다는 사실만으로 읽지 않는다.

#[cfg(feature = "gui")]
pub(crate) mod client;

use serde_json::{Value, json};
use tasty_ipc::protocol::JsonRpcResponse;
use tasty_ipc::stream::{StreamFrame, StreamTag};
use tasty_ipc::stream_hub::StreamHub;
use tasty_ipc::terminal_read::{
    OUTPUT_CHUNK, RESULT_BUDGET, ReadFrom, ReadOp, ReadOutcome, TerminalReadMsg,
};

use crate::runtime::engine_access::EngineMut;

/// 요청이 이 engine 의 터미널을 가리키면 처리하고 true 다. 대상 터미널이 이 engine 에 없으면 false 를
/// 돌려 호출자가 다음 engine 을 보게 한다. 점유하지 않은 연결의 요청은 어느 engine 에서든 거절한다.
pub(crate) fn handle(
    engine: &mut EngineMut<'_>,
    hub: &StreamHub,
    client_id: u32,
    msg: &TerminalReadMsg,
) -> bool {
    let TerminalReadMsg::TerminalReadRequest {
        request_id,
        surface_id,
        op,
    } = msg
    else {
        return true;
    };
    // 터미널이 있는 engine 을 먼저 찾는다. 다른 창의 engine 이 이 surface 를 모른다고 거절하지 않게 한다.
    if engine.runtime.terminals.get(*surface_id).is_none() {
        return false;
    }
    let outcome = if engine.live.occupancy.holder(*surface_id) != Some(client_id) {
        not_attached(*surface_id)
    } else {
        read(engine, *request_id, *surface_id, op)
    };
    push(hub, client_id, *request_id, outcome);
    true
}

/// 어느 engine 에도 대상 터미널이 없을 때의 답.
pub(crate) fn reply_missing(hub: &StreamHub, client_id: u32, msg: &TerminalReadMsg) {
    if let TerminalReadMsg::TerminalReadRequest {
        request_id,
        surface_id,
        ..
    } = msg
    {
        let outcome = refused(
            -32000,
            format!("remote surface {surface_id} has no terminal"),
            "no_terminal",
        );
        push(hub, client_id, *request_id, outcome);
    }
}

fn not_attached(surface_id: u32) -> ReadOutcome {
    refused(
        -32000,
        format!("this attach connection does not hold remote surface {surface_id}"),
        "not_attached",
    )
}

fn refused(code: i32, message: String, reason: &str) -> ReadOutcome {
    ReadOutcome::Err {
        code,
        message,
        data: json!({ "reason": reason }),
    }
}

fn push(hub: &StreamHub, client_id: u32, request_id: u64, outcome: ReadOutcome) {
    let msg = TerminalReadMsg::TerminalReadResult {
        request_id,
        outcome,
    };
    match serde_json::to_vec(&msg) {
        // 손실·끊김은 허브가 처리한다. client 는 응답을 받지 못하면 상한 시간 뒤 실패로 끝낸다.
        Ok(payload) => {
            if !matches!(
                hub.push(client_id, StreamFrame::new(StreamTag::Control, payload)),
                tasty_ipc::stream_hub::PushResult::Sent
            ) {
                tracing::warn!(client_id, request_id, "terminal read result was not queued");
            }
        }
        Err(e) => tracing::warn!("terminal read result did not serialize: {e}"),
    }
}

fn outcome_of(response: JsonRpcResponse) -> ReadOutcome {
    match (response.result, response.error) {
        (_, Some(e)) => ReadOutcome::Err {
            code: e.code,
            message: e.message,
            data: e.data.unwrap_or(Value::Null),
        },
        (Some(result), None) => ReadOutcome::Ok { result },
        (None, None) => ReadOutcome::Ok {
            result: Value::Null,
        },
    }
}

/// 결과 프레임의 JSON 길이. 상한 판단에 쓴다.
fn encoded_len(request_id: u64, outcome: &ReadOutcome) -> usize {
    serde_json::to_vec(&TerminalReadMsg::TerminalReadResult {
        request_id,
        outcome: outcome.clone(),
    })
    .map_or(usize::MAX, |v| v.len())
}

pub(crate) fn read(
    engine: &mut EngineMut<'_>,
    request_id: u64,
    surface_id: u32,
    op: &ReadOp,
) -> ReadOutcome {
    use crate::adapters::ipc::handler::surface;
    match op {
        ReadOp::ScreenText { lines, show_dim } => {
            let mut params = json!({ "surface_id": surface_id, "show_dim": show_dim });
            if let Some(n) = lines {
                params["lines"] = json!(n);
            }
            let outcome = outcome_of(surface::handle_screen_text(
                &engine.as_ref(),
                Value::Null,
                &params,
            ));
            if encoded_len(request_id, &outcome) > RESULT_BUDGET {
                // 줄 수를 몰래 줄이면 `lines` 계약이 깨지므로 거절하고 더 적은 줄을 요청하게 한다.
                return refused(
                    -32000,
                    format!(
                        "the screen text of remote surface {surface_id} does not fit one attach \
                         frame; ask for fewer lines"
                    ),
                    "too_large",
                );
            }
            outcome
        }
        ReadOp::Output {
            from,
            max_bytes,
            strip_ansi,
        } => {
            let (cursor, stream) = match from {
                ReadFrom::Cursor { cursor, stream } => (*cursor, Some(stream.clone())),
                ReadFrom::Start => (0, None),
            };
            let mut budget = max_bytes
                .unwrap_or(tasty_terminal::OUTPUT_RETENTION_MAX_BYTES)
                .clamp(1, OUTPUT_CHUNK);
            loop {
                let req = tasty_terminal::OutputReadRequest {
                    from: tasty_terminal::OutputCursor::At(cursor),
                    max_bytes: budget,
                    strip_ansi: *strip_ansi,
                    expect_stream: stream.clone(),
                };
                let outcome = outcome_of(surface::read_output_response(
                    engine,
                    Value::Null,
                    surface_id,
                    &req,
                    true,
                ));
                // 원문 길이를 줄이면 `next_cursor` 가 남은 위치를 알려 준다. 이어 읽기 계약은 그대로다.
                if encoded_len(request_id, &outcome) <= RESULT_BUDGET {
                    return outcome;
                }
                if budget == 1 {
                    return refused(
                        -32000,
                        format!("one byte of remote surface {surface_id} does not fit a frame"),
                        "too_large",
                    );
                }
                budget /= 2;
            }
        }
        ReadOp::Position => {
            let req = tasty_terminal::OutputReadRequest {
                from: tasty_terminal::OutputCursor::At(0),
                max_bytes: 1,
                strip_ansi: false,
                expect_stream: None,
            };
            match outcome_of(surface::read_output_response(
                engine,
                Value::Null,
                surface_id,
                &req,
                true,
            )) {
                ReadOutcome::Ok { result } => ReadOutcome::Ok {
                    result: json!({
                        "stream": result["stream"],
                        "retention_end": result["retention_end"],
                    }),
                },
                err => err,
            }
        }
    }
}

#[cfg(test)]
#[path = "terminal_read/tests.rs"]
mod tests;
