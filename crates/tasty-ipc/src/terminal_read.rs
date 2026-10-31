//! attach holder 가 mirror 터미널의 화면·원문 출력을 서버 원본에서 읽는 제어 메시지.
//!
//! mirror 터미널은 attach snapshot(보이는 화면만)과 그 뒤의 출력으로 화면을 다시 만든다. 보이는 화면은
//! 원본과 같지만 출력 보관 버퍼에는 attach 이전 원문이 없고 합성 snapshot 바이트가 앞에 붙으며, 손실 뒤
//! 재동기화는 유실을 `skipped` 로 남기지 않는다. 그래서 `surface.screen_text`·`surface.read_since_mark` 는
//! 이 메시지로 서버 터미널을 읽는다. 별도 포트·서비스 없이 기존 holder 연결의 Control 프레임으로 오간다.
//!
//! 구 서버는 이 event 를 모르고 버린다. client 는 attach descriptor 에 [`CAPABILITY`] 가 있는 연결에만 보낸다.

use serde::{Deserialize, Serialize};

/// 서버가 holder 의 원본 터미널 읽기를 받는다는 capability 이름.
pub const CAPABILITY: &str = "ipc.stream.terminal-read";
/// [`CAPABILITY`] 의 버전.
pub const VERSION: u32 = 1;

/// 결과 한 프레임의 JSON 직렬화 상한. 프레임 상한([`crate::stream::MAX_FRAME_LEN`])의 절반이다.
/// 원문은 제어 문자가 JSON 에서 최대 6바이트로 늘어나므로 서버는 읽을 원문 길이를 줄여 이 안에 맞춘다.
/// 화면 읽기가 이 상한을 넘으면 줄 수를 줄이지 않고 `too_large` 로 거절한다.
pub const RESULT_BUDGET: usize = (crate::stream::MAX_FRAME_LEN as usize) / 2;

/// 서버가 한 번에 읽는 원문 바이트 상한. 이어 읽기는 `next_cursor` 로 한다.
pub const OUTPUT_CHUNK: usize = 256 * 1024;

/// client → 서버 요청과 서버 → client 결과.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TerminalReadMsg {
    /// `surface_id` 는 서버 쪽 id 다. 서버는 그 surface 의 workspace 를 이 연결이 점유했을 때만 읽는다.
    TerminalReadRequest {
        request_id: u64,
        surface_id: u32,
        op: ReadOp,
    },
    TerminalReadResult {
        request_id: u64,
        outcome: ReadOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ReadOp {
    /// `surface.screen_text` 와 같은 추출.
    ScreenText {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lines: Option<usize>,
        #[serde(default)]
        show_dim: bool,
    },
    /// `surface.read_since_mark` 와 같은 원문 조회. 서버의 공유 마크는 읽지도 바꾸지도 않는다.
    Output {
        from: ReadFrom,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_bytes: Option<usize>,
        #[serde(default)]
        strip_ansi: bool,
    },
    /// 현재 stream 과 보관 끝 위치. mirror 의 마크를 정한다.
    Position,
}

/// 원문을 어디부터 읽을지. 마크도 client 가 보관한 서버 위치이므로 [`ReadFrom::Cursor`] 로 보낸다.
/// stream 이 다르면 마크든 호출자 cursor 든 같은 `stream_mismatch` 로 거절한다. 처음부터 조용히 읽지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum ReadFrom {
    Cursor {
        cursor: u64,
        stream: String,
    },
    /// 마크를 설정하지 않은 mirror. 로컬 미설정 마크처럼 보관 시작부터 읽고 잃은 앞부분은 `skipped` 로 알린다.
    Start,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ReadOutcome {
    /// 로컬 IPC 응답의 `result` 와 같은 모양. `surface_id` 는 서버 id 이므로 client 가 바꾼다.
    Ok { result: serde_json::Value },
    /// 로컬 IPC 오류와 같은 code·message·data(`reason` 포함).
    Err {
        code: i32,
        message: String,
        #[serde(default)]
        data: serde_json::Value,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_result_round_trip_with_their_event_tags() {
        let req = TerminalReadMsg::TerminalReadRequest {
            request_id: 7,
            surface_id: 515,
            op: ReadOp::Output {
                from: ReadFrom::Cursor {
                    cursor: 10,
                    stream: "s-1".into(),
                },
                max_bytes: Some(4),
                strip_ansi: true,
            },
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["event"], "terminal_read_request");
        assert_eq!(json["op"]["op"], "output");
        assert_eq!(json["op"]["from"]["from"], "cursor");
        assert_eq!(
            serde_json::from_value::<TerminalReadMsg>(json).unwrap(),
            req
        );

        let res = TerminalReadMsg::TerminalReadResult {
            request_id: 7,
            outcome: ReadOutcome::Err {
                code: -32602,
                message: "m".into(),
                data: serde_json::json!({ "reason": "stream_mismatch" }),
            },
        };
        let json = serde_json::to_value(&res).unwrap();
        assert_eq!(json["event"], "terminal_read_result");
        assert_eq!(
            serde_json::from_value::<TerminalReadMsg>(json).unwrap(),
            res
        );
    }

    #[test]
    fn other_control_events_do_not_parse_as_terminal_reads() {
        let other = serde_json::json!({ "event": "git_query_request", "request_id": 1 });
        assert!(serde_json::from_value::<TerminalReadMsg>(other).is_err());
    }
}
