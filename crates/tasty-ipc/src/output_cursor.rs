//! 소비자가 보관한 출력 위치(cursor·stream·max_bytes)의 인자 이름과 기능 버전.
//! 구 서버는 이 인자를 버리고 공유 마크를 읽을 수 있어 전송 전에 ipc.output-cursor 지원을 확인한다.
//! 서버 파서·capability 선언·CLI가 이 모듈의 상수를 공유한다.
//! 위치·gap·스트림 변경 규칙은 docs/features/terminal-output/index.md를 따른다.

/// 협상 목록에 오르는 이름.
pub const CAPABILITY: &str = "ipc.output-cursor";

/// 출력 커서 기능 버전. 기존 의미의 변경과 새 기능 추가를 구분한다.
pub const VERSION: u32 = 1;

/// 호출자가 든 위치(원문 바이트 단위, 절대값).
pub const PARAM_CURSOR: &str = "cursor";
/// 그 위치가 속한 스트림의 표지. `cursor` 에는 이것이 따라야 한다.
pub const PARAM_STREAM: &str = "stream";
/// 이번 응답에 담을 원문 바이트 상한.
pub const PARAM_MAX_BYTES: &str = "max_bytes";

/// 이 계약에 속하는 인자 전부. 하나라도 실리면 그 요청은 계약을 요구한다.
pub const PARAMS: [&str; 3] = [PARAM_CURSOR, PARAM_STREAM, PARAM_MAX_BYTES];

/// 이 계약을 쓰는 메서드.
pub const METHOD: &str = "surface.read_since_mark";

/// 해당 메서드에 커서 계약 인자가 하나라도 있으면 true다. 인자가 없는 기존 호출은 요구하지 않는다.
pub fn requested_by(method: &str, params: &serde_json::Value) -> bool {
    method == METHOD
        && PARAMS
            .iter()
            .any(|k| params.get(k).is_some_and(|v| !v.is_null()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_read_without_the_contract_s_arguments_does_not_require_it() {
        assert!(!requested_by(METHOD, &json!({ "surface_id": 1 })));
        assert!(!requested_by(
            METHOD,
            &json!({ "surface_id": 1, "strip_ansi": true, "cursor": null })
        ));
    }

    #[test]
    fn any_one_of_the_three_arguments_requires_it() {
        for k in PARAMS {
            assert!(requested_by(METHOD, &json!({ k: 1 })), "{k}");
        }
    }

    #[test]
    fn another_method_carrying_the_same_names_does_not_require_it() {
        assert!(!requested_by("events.fetch", &json!({ "cursor": 1 })));
    }
}
