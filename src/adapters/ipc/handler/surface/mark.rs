use serde_json::json;

use tasty_ipc::output_cursor;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;
use crate::ipc::handler::params::opt_int;
use tasty_terminal::OUTPUT_RETENTION_MAX_BYTES;

pub(crate) fn handle_set_mark(
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let _ = engine; // handler 는 enqueue 만. cascade 가 적용.
    out.push(crate::core::intent::DomainIntent::SetTerminalMark { surface_id }.from_agent_ipc());
    JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": surface_id }))
}

/// `surface.read_since_mark` — 마크 이후, 또는 **호출자가 든 위치 이후**의 원문 출력.
///
/// 두 형태가 한 이름 아래 있다.
///
/// - `cursor` 를 안 주면 서버가 든 마크(`surface.set_mark`)부터 읽는다. 마크는
///   surface 당 하나라 여럿이 같은 창을 보고, `set_mark` 하는 쪽이 안 하는 쪽을 민다.
/// - `cursor` 를 주면 **서버는 그 소비자를 위해 아무것도 안 든다.** 위치는 호출자가
///   들고 매번 가져오므로 소비자가 몇이든 서로를 안 민다. 같은 모양이
///   `events.fetch` 다
///   (`docs/adr/0633-event-feed-delivery.md`).
///   이 결정은
///   `docs/adr/0634-output-cursor-contract.md`.
///
/// 둘 다 응답에 보존 구간(`retention_start`/`retention_end`)·다음 위치
/// (`next_cursor`)·잃은 바이트(`skipped`)·스트림 표지(`stream`)를 싣는다. `text` 는
/// 예나 지금이나 같은 값이다 — 더해진 것은 **말해 주는 칸**이다.
///
/// `cursor` 에는 `stream` 이 따라야 한다. surface id 는 닫혔다 열리면 재사용되고
/// `surface.respawn_terminal` 은 id 를 그대로 둔 채 터미널을 갈아 끼우므로, 표지가
/// 없으면 옛 위치가 **남의 출력**에 조용히 적용된다.
pub(crate) fn handle_read_since_mark(
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let OutputReadParams { req, continuing } = match OutputReadParams::parse(params, &id) {
        Ok(v) => v,
        Err(e) => return *e,
    };

    match engine.read_output(surface_id, &req) {
        Some(Ok(read)) => answered(id, surface_id, read),
        Some(Err(tasty_terminal::OutputReadError::StreamMismatch { expected, actual })) => refused(
            id,
            -32602,
            format!(
                "'stream' was given as '{expected}' but surface {surface_id} is \
                 streaming '{actual}' — the surface id was reused or its terminal \
                 was respawned. Read once without 'cursor' to start over on this \
                 stream"
            ),
            "stream_mismatch",
            json!({ "stream": actual }),
        ),
        Some(Err(tasty_terminal::OutputReadError::AheadOfStream {
            cursor,
            retention_end,
        })) => refused(
            id,
            -32602,
            format!(
                "'cursor' was given as {cursor} but surface {surface_id} has only \
                 produced {retention_end} bytes. Answering it empty would leave \
                 the consumer waiting for a continuation that, once that much \
                 output arrives, is unrelated to what it last saw"
            ),
            "cursor_ahead_of_stream",
            json!({ "retention_end": retention_end }),
        ),
        // 터미널이 없는 surface. `cursor`/`stream` 을 준 호출자는 이어 읽기를
        // 물었으므로 빈 답이 "새 출력이 없다" 로 읽혀선 안 된다 — 거절한다.
        // 아무것도 안 준 호출자에게는 예전 그대로 빈 text 로 답한다.
        None if continuing => refused(
            id,
            -32000,
            format!("surface {surface_id} has no terminal to read a position from"),
            "no_terminal",
            serde_json::Value::Null,
        ),
        None => JsonRpcResponse::success(id, json!({ "text": "", "surface_id": surface_id })),
    }
}

/// 읽기 하나를 wire 모양으로 싼다.
///
/// **`next_cursor` 와 `raw_bytes` 는 버퍼가 준 값을 그대로 싣는다.** 여기서 `text` 로부터
/// 다시 계산하면 안 된다 — 손실 디코딩이 U+FFFD 를 넣고 `strip_ansi` 가 바이트를 빼므로
/// 그 길이는 읽은 원문 구간의 길이가 아니고, 호출자가 그것으로 전진하면 스트림과
/// 어긋난다. 이 함수를 따로 둔 이유가 그 자리를 시험이 잡을 수 있게 하는 것이다.
fn answered(
    id: serde_json::Value,
    surface_id: u32,
    read: tasty_terminal::OutputRead,
) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        json!({
            "text": read.text,
            "surface_id": surface_id,
            "stream": read.stream,
            "cursor": read.cursor,
            "next_cursor": read.next_cursor,
            "raw_bytes": read.raw_bytes,
            "retention_start": read.retention_start,
            "retention_end": read.retention_end,
            "skipped": read.skipped,
        }),
    )
}

/// `surface.read_since_mark` 의 인자. 파싱을 한 자리에 모아 두 dispatch 경로
/// (gui · headless)가 같은 규칙을 쓰고, 규칙 자체가 상태 없이 시험된다.
struct OutputReadParams {
    req: tasty_terminal::OutputReadRequest,
    /// 호출자가 **이어 읽기**를 물었는가(`cursor` 나 `stream` 중 하나라도 줬는가).
    /// 대상 터미널이 없을 때 빈 답을 줄지 거절할지가 여기서 갈린다.
    continuing: bool,
}

impl OutputReadParams {
    fn parse(
        params: &serde_json::Value,
        id: &serde_json::Value,
    ) -> Result<Self, Box<JsonRpcResponse>> {
        let strip_ansi = params
            .get("strip_ansi")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        // 인자 이름은 협상 목록이 싣는 판과 같은 모듈에서 온다 — 여기서 리터럴로 다시
        // 적으면 선언과 배선이 갈린다(`tasty_ipc::output_cursor`).
        let cursor = opt_int::<u64>(params, output_cursor::PARAM_CURSOR, id).map_err(Box::new)?;
        // 상한은 보존 크기다 — 그보다 큰 `max_bytes` 는 답을 한 바이트도 못 늘린다.
        // 값을 여기 따로 박으면 보존을 키우는 날 이 상한만 남아 조용히 어긋난다.
        let max_bytes = opt_int::<usize>(params, output_cursor::PARAM_MAX_BYTES, id)
            .map_err(Box::new)?
            .unwrap_or(OUTPUT_RETENTION_MAX_BYTES);
        let stream = params
            .get(output_cursor::PARAM_STREAM)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        if cursor.is_some() && stream.is_none() {
            return Err(Box::new(refused(
                id.clone(),
                -32602,
                "'cursor' was given without 'stream'. A surface id is reused when a \
                 surface closes and another opens, and surface.respawn_terminal \
                 replaces the terminal under an id that does not change, so without \
                 the token a stale position would be applied to another stream's \
                 output in silence. Read once without 'cursor' to get both",
                "cursor_without_stream",
                serde_json::Value::Null,
            )));
        }

        Ok(Self {
            continuing: cursor.is_some() || stream.is_some(),
            req: tasty_terminal::OutputReadRequest {
                from: match cursor {
                    Some(at) => tasty_terminal::OutputCursor::At(at),
                    None => tasty_terminal::OutputCursor::Mark,
                },
                max_bytes,
                strip_ansi,
                expect_stream: stream,
            },
        })
    }
}

/// 거절에 **기계가 읽는 사유**를 함께 싣는다. 호출자가 다음에 할 일이 사유마다
/// 다른데(위치를 버리고 다시 시작 · 인자를 고침 · 대상이 없음), 메시지 문자열을
/// 파싱하게 두면 그 판정이 문구에 묶인다.
fn refused(
    id: serde_json::Value,
    code: i32,
    message: impl Into<String>,
    reason: &str,
    extra: serde_json::Value,
) -> JsonRpcResponse {
    let mut data = json!({ "reason": reason });
    if let (Some(obj), Some(more)) = (data.as_object_mut(), extra.as_object()) {
        for (k, v) in more {
            obj.insert(k.clone(), v.clone());
        }
    }
    JsonRpcResponse::error_with_data(id, code, message, data)
}

/// `surface.read_since_scan_mark` — 출력 스캐너 전용 커서. 지난 호출 이후 새로 온
/// 출력만 돌려주고 커서를 그만큼 전진시킨다.
///
/// `surface.read_since_mark` 과 **다른 커서**를 쓴다. 같은 커서를 쓰면 에이전트의
/// `surface.set_mark` 이 스캐너의 관측 창을 밀고, 아무도 mark 를 안 세운 surface 에서는
/// 폴링마다 버퍼 전체가 다시 실린다
/// (`docs/adr/0613-terminal-io-and-process-lifetime.md`).
///
/// 읽기가 커서를 전진시키므로 **같은 구간을 두 번 받을 수 없다.** 그래서 이 이름의
/// 소비자는 하나라는 전제 위에 있다 — 둘이 부르면 서로의 바이트를 먹고 그 손실은
/// 조용하다. 그 전제가 위 ADR 의 재검토 조건이다.
pub(crate) fn handle_read_since_scan_mark(
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let strip_ansi = params
        .get("strip_ansi")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let text = engine.take_since_output_scan_mark(surface_id, strip_ansi);
    JsonRpcResponse::success(id, json!({ "text": text, "surface_id": surface_id }))
}

/// `surface.parse_since_mark` — read_since_mark 결과를 `tasty-output` 빌트인
/// 파서들로 분해. `parsers` 가 생략되면 `DEFAULT_PARSER_IDS` 사용. `prompt_boundary`
/// /`exit_code` 같이 ANSI escape 자체가 의미인 파서를 쓸 수 있도록 raw 텍스트
/// (strip_ansi=false) 를 항상 입력으로 한다.
pub(crate) fn handle_parse_since_mark(
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };

    let parser_ids: Vec<String> = match params.get("parsers") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        Some(serde_json::Value::String(s)) => s
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => tasty_output::DEFAULT_PARSER_IDS
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };

    let text = engine.read_since_mark_of(surface_id, false);
    let items = match tasty_output::parse_buffer(&text, parser_ids.iter().map(String::as_str)) {
        Ok(v) => v,
        Err(unknown) => {
            return JsonRpcResponse::invalid_params(id, format!("unknown parser: '{unknown}'"));
        }
    };

    JsonRpcResponse::success(
        id,
        json!({
            "surface_id": surface_id,
            "parsers": parser_ids,
            "items": items,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{OUTPUT_RETENTION_MAX_BYTES, OutputReadParams};
    use serde_json::json;

    fn ok(params: serde_json::Value) -> OutputReadParams {
        OutputReadParams::parse(&params, &json!(1)).expect("정상 인자")
    }

    fn reason(params: serde_json::Value) -> String {
        let refusal = match OutputReadParams::parse(&params, &json!(1)) {
            Ok(_) => panic!("거절할 인자를 받아들였다"),
            Err(e) => *e,
        };
        refusal
            .error
            .expect("에러 몸통")
            .data
            .expect("사유")
            .get("reason")
            .and_then(|v| v.as_str())
            .expect("reason 칸")
            .to_string()
    }

    #[test]
    fn no_cursor_arguments_reads_the_server_held_mark_exactly_as_before() {
        let p = ok(json!({}));
        assert!(matches!(p.req.from, tasty_terminal::OutputCursor::Mark));
        assert!(!p.continuing, "이어 읽기를 안 물었다");
        assert!(p.req.expect_stream.is_none());
        assert!(!p.req.strip_ansi);
        assert_eq!(
            p.req.max_bytes, OUTPUT_RETENTION_MAX_BYTES,
            "상한을 안 주면 보존 전체 — 예전 응답과 같은 text 가 나와야 한다"
        );
    }

    #[test]
    fn a_cursor_without_a_stream_token_is_refused_rather_than_guessed() {
        assert_eq!(reason(json!({ "cursor": 12 })), "cursor_without_stream");
    }

    #[test]
    fn a_stream_token_alone_is_a_continuation_too() {
        // 마크에서 읽되 "그 사이 터미널이 바뀌지 않았다" 를 함께 묻는 형태.
        let p = ok(json!({ "stream": "abc" }));
        assert!(matches!(p.req.from, tasty_terminal::OutputCursor::Mark));
        assert!(p.continuing);
        assert_eq!(p.req.expect_stream.as_deref(), Some("abc"));
    }

    #[test]
    fn an_empty_stream_token_is_the_same_as_not_giving_one() {
        // 빈 문자열은 어떤 스트림과도 안 맞아, 받아들이면 그 호출자는 영원히 거절만
        // 받는다.
        assert_eq!(
            reason(json!({ "cursor": 3, "stream": "" })),
            "cursor_without_stream"
        );
    }

    #[test]
    fn a_position_that_is_not_a_whole_number_is_refused_rather_than_coerced() {
        // 조용히 0 으로 읽으면 호출자가 든 위치를 버리고 보존 구간 전체를 다시 준다.
        for bad in [json!("12"), json!(-1), json!(1.5)] {
            let r = OutputReadParams::parse(&json!({ "cursor": bad, "stream": "s" }), &json!(1));
            assert!(r.is_err(), "cursor={bad} 를 받아들였다");
        }
    }

    #[test]
    fn max_bytes_is_carried_through_and_the_buffer_clamps_it() {
        // 관문은 자르지 않는다 — 자르는 자리를 둘로 두면 두 판정이 갈린다.
        assert_eq!(ok(json!({ "max_bytes": 0 })).req.max_bytes, 0);
        assert_eq!(
            ok(json!({ "max_bytes": 999_999_999u64 })).req.max_bytes,
            999_999_999
        );
    }

    /// 협상 목록이 선언하는 계약의 인자 이름이 **이 파서가 실제로 읽는 이름**이다. 이름이
    /// 갈리면 capability 를 확인한 client 가 보낸 인자를 서버가 조용히 버린다 — 확인이
    /// 통과했으므로 그 버림은 아무 데서도 안 보인다.
    #[test]
    fn the_declared_cursor_contract_names_the_arguments_this_parser_reads() {
        use tasty_ipc::output_cursor::{PARAM_CURSOR, PARAM_MAX_BYTES, PARAM_STREAM};
        let p = ok(json!({ PARAM_CURSOR: 7, PARAM_STREAM: "s", PARAM_MAX_BYTES: 9 }));
        assert!(matches!(p.req.from, tasty_terminal::OutputCursor::At(7)));
        assert_eq!(p.req.expect_stream.as_deref(), Some("s"));
        assert_eq!(p.req.max_bytes, 9);
        assert!(
            tasty_ipc::capability::CAPABILITIES
                .iter()
                .any(|c| c.name == tasty_ipc::output_cursor::CAPABILITY),
            "파서가 읽는 계약이 협상 목록에 없다"
        );
    }

    #[test]
    fn the_wire_carries_the_buffer_s_raw_advance_and_not_the_length_of_the_text() {
        // 손실 디코딩과 strip_ansi 가 지나간 뒤의 text 는 읽은 원문 구간과 길이가
        // 다르다. 응답이 그 길이로 전진을 말하면 호출자가 스트림과 어긋난다.
        let read = tasty_terminal::OutputRead {
            text: "red".to_string(),
            raw_bytes: 12,
            cursor: 100,
            next_cursor: 112,
            retention_start: 0,
            retention_end: 112,
            skipped: 7,
            stream: "s1".to_string(),
        };
        let resp = super::answered(json!(1), 3, read);
        let r = resp.result.expect("성공 응답");
        assert_eq!(r["text"].as_str(), Some("red"));
        assert_eq!(r["raw_bytes"].as_u64(), Some(12));
        assert_eq!(
            r["next_cursor"].as_u64(),
            Some(112),
            "text 는 3 바이트인데 구간은 12 바이트다 — 전진은 구간 쪽이다"
        );
        assert_eq!(r["cursor"].as_u64(), Some(100));
        assert_eq!(r["skipped"].as_u64(), Some(7));
        assert_eq!(r["retention_start"].as_u64(), Some(0));
        assert_eq!(r["retention_end"].as_u64(), Some(112));
        assert_eq!(r["stream"].as_str(), Some("s1"));
        assert_eq!(r["surface_id"].as_u64(), Some(3));
    }
}
