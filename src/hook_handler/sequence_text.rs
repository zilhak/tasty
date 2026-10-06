//! IpcSequence 를 사람이 읽고 쓰는 여러 줄 문자열로 바꾸고 되돌린다.
//!
//! 한 줄이 호출 하나다. `method` 뒤에 공백을 두고 한 줄짜리 JSON 값을 쓰면 그 값이 params 다.
//! params 를 생략하면 `null` 이다(저장 형식 `IpcCall` 의 serde 기본값과 같다).
//! 앞뒤 공백을 뺀 줄이 비었거나 `#` 으로 시작하면 건너뛴다. 줄 중간의 `#` 은 주석이 아니다.
//! 입력 맨 앞의 BOM 하나는 무시한다(붙여 넣은 텍스트가 BOM 으로 시작할 수 있다).
//!
//! 저장 경로는 method 문법을 검사하지 않으므로 이 형식으로 쓸 수 없는 method 가 저장돼 있을 수
//! 있다(`method_fits_a_line` 참고). 그런 시퀀스에는 `format_sequence` 가 오류를 돌려주고, 편집기는
//! 문자열 편집 대신 CLI(`tasty hook-handler get`/`upsert`)로 편집하라고 안내한다.
//! `format_sequence` 가 성공한 시퀀스는 `parse_sequence` 로 같은 호출 목록이 된다. 직렬화는
//! compact JSON 이라 줄바꿈을 내지 않는다.
//!
//! 반대 방향(문자열 → 호출 → 문자열)은 같은 문자열을 돌려주지 않는다. 다시 열면 주석·빈 줄·
//! 들여쓰기와 공백이 사라지고, params 는 serde_json 이 다시 쓴 모양이 된다: 객체 키는 정렬되고
//! (`preserve_order` 꺼짐), 숫자 표기는 바뀌며(`1e2` → `100.0`), 문자열 escape 도 serde 표기가 된다.
//! 큰 정수와 float 의 정밀도 손실은 이 형식이 아니라 저장 형식(serde_json, `float_roundtrip` 꺼짐)의
//! 성질이며, 저장 경로를 거쳐도 같은 값이 된다.
//!
//! 오류의 `Display` 는 로그·시험용 영어 문장이다. 편집기는 이 문장을 그대로 보이지 않고 `kind` 로
//! 번역 키를 고르고 줄·열 번호를 인자로 넘긴다. `InvalidParams::message` 는 serde 원문이라 번역할
//! 수 없으므로 보조 정보로만 쓴다.

use std::fmt;

use super::types::IpcCall;

const BOM: char = '\u{feff}';

/// 줄 번호(1부터)와 이유를 가진 파싱 오류.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceTextError {
    pub line: usize,
    pub kind: SequenceTextErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceTextErrorKind {
    /// 줄이 메서드 이름 없이 JSON 으로 시작한다.
    MissingMethod,
    /// 메서드 이름에 제어 문자가 있거나 BOM 으로 시작한다. 이 형식으로 다시 쓸 수 없는 이름이다.
    InvalidMethod,
    /// 메서드 뒤의 params 가 JSON 값 하나로 읽히지 않는다. `column` 은 그 줄 기준 글자 수(1부터)다.
    InvalidParams { column: usize, message: String },
}

impl fmt::Display for SequenceTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            SequenceTextErrorKind::MissingMethod => write!(
                f,
                "line {}: expected a method name before the params",
                self.line
            ),
            SequenceTextErrorKind::InvalidMethod => write!(
                f,
                "line {}: the method name has a control character or starts with a byte order mark",
                self.line
            ),
            SequenceTextErrorKind::InvalidParams { column, message } => write!(
                f,
                "line {}, column {column}: invalid params JSON: {message}",
                self.line
            ),
        }
    }
}

impl std::error::Error for SequenceTextError {}

/// 저장된 method 가 이 형식의 한 줄로 쓰이지 않아 직렬화하지 못했다. `index` 는 0부터인 호출 순번이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnrepresentableMethod {
    pub index: usize,
    pub method: String,
}

impl fmt::Display for UnrepresentableMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "call {}: method {:?} cannot be written as a text line",
            self.index + 1,
            self.method
        )
    }
}

impl std::error::Error for UnrepresentableMethod {}

/// 한 줄의 첫 토큰으로 쓰고 다시 읽어 같은 이름이 되는 method 인가.
/// 비었거나, 공백(전각 공백 등 유니코드 공백 포함)·줄바꿈·제어 문자가 있거나, 주석 `#`·JSON
/// 시작 `{` `[` `"`·BOM 으로 시작하면 아니다.
/// IPC 라우터가 아니라 이름 문법 검사라 매개변수를 `method` 로 두지 않는다(그 이름으로 분기하는
/// 함수는 dispatch 명부 가드 `source_guards::dispatch_name_literals` 가 라우터로 센다).
fn method_fits_a_line(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['#', '{', '[', '"', BOM])
        && !name.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// 호출마다 한 줄을 쓴다. params 가 `null` 이면 메서드만 쓴다.
pub fn format_sequence(calls: &[IpcCall]) -> Result<String, UnrepresentableMethod> {
    let mut out = String::new();
    for (index, call) in calls.iter().enumerate() {
        if !method_fits_a_line(&call.method) {
            return Err(UnrepresentableMethod {
                index,
                method: call.method.clone(),
            });
        }
        out.push_str(&call.method);
        if !call.params.is_null() {
            out.push(' ');
            out.push_str(&call.params.to_string());
        }
        out.push('\n');
    }
    Ok(out)
}

/// 문자열을 호출 목록으로 읽는다. 첫 오류에서 멈추고 그 줄 번호를 돌려준다.
pub fn parse_sequence(text: &str) -> Result<Vec<IpcCall>, SequenceTextError> {
    let text = text.strip_prefix(BOM).unwrap_or(text);
    let mut calls = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (method, rest) = match trimmed.find(char::is_whitespace) {
            Some(at) => (&trimmed[..at], trimmed[at..].trim_start()),
            None => (trimmed, ""),
        };
        if method.starts_with(['{', '[', '"']) {
            return Err(SequenceTextError {
                line,
                kind: SequenceTextErrorKind::MissingMethod,
            });
        }
        // 읽은 호출은 모두 다시 쓸 수 있어야 하므로 직렬화와 같은 규칙으로 거른다.
        if !method_fits_a_line(method) {
            return Err(SequenceTextError {
                line,
                kind: SequenceTextErrorKind::InvalidMethod,
            });
        }
        let params = if rest.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(rest).map_err(|e| {
                let rest_start = raw.len() - raw.trim_start().len() + (trimmed.len() - rest.len());
                SequenceTextError {
                    line,
                    kind: SequenceTextErrorKind::InvalidParams {
                        column: raw[..rest_start].chars().count() + char_column(rest, &e),
                        message: without_serde_position(&e),
                    },
                }
            })?
        };
        calls.push(IpcCall {
            method: method.to_string(),
            params,
        });
    }
    Ok(calls)
}

/// serde 의 열은 바이트 기준이다. rest 는 한 줄이므로 그 바이트 열까지 시작한 글자 수로 바꾼다.
fn char_column(rest: &str, e: &serde_json::Error) -> usize {
    rest.char_indices()
        .take_while(|(at, _)| *at < e.column())
        .count()
}

/// serde 메시지 끝의 ` at line N column M` 은 params 기준 위치라 줄 기준 위치와 어긋나므로 뗀다.
fn without_serde_position(e: &serde_json::Error) -> String {
    let full = e.to_string();
    let suffix = format!(" at line {} column {}", e.line(), e.column());
    full.strip_suffix(&suffix).unwrap_or(&full).to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn call(method: &str, params: serde_json::Value) -> IpcCall {
        IpcCall {
            method: method.into(),
            params,
        }
    }

    fn round_trip(calls: &[IpcCall]) -> Vec<IpcCall> {
        parse_sequence(&format_sequence(calls).unwrap()).unwrap()
    }

    #[test]
    fn a_sequence_survives_format_then_parse() {
        let calls = vec![
            call("system.info", serde_json::Value::Null),
            call(
                "notification.send",
                json!({ "title": "Build", "body": "${body.status}" }),
            ),
            call("workspace.list", json!({})),
            call(
                "surface.send_text",
                json!({ "text": "ls #1\n", "surface_id": 3 }),
            ),
            call("agent.task_create", json!([1, "two", null])),
            call("system.ping", json!("한글 값")),
        ];
        let text = format_sequence(&calls).unwrap();
        assert_eq!(text.lines().count(), calls.len(), "{text}");
        assert_eq!(parse_sequence(&text).unwrap(), calls);
    }

    #[test]
    fn an_empty_sequence_is_an_empty_string_and_back() {
        assert_eq!(format_sequence(&[]).unwrap(), "");
        assert_eq!(parse_sequence("").unwrap(), Vec::new());
    }

    /// 저장 경로가 막지 않는 method 중 한 줄로 쓰면 사라지거나 쪼개지거나 잘리거나 다시 읽을 수
    /// 없는 이름은 직렬화가 거절한다. params 가 있든 없든 같다.
    #[test]
    fn a_method_that_does_not_fit_a_line_is_refused() {
        for method in [
            "",
            " ",
            "#x",
            "a\nb",
            " lead",
            "trail ",
            "a b",
            "a\tb",
            "a\u{3000}b",
            "a\rb",
            "{x",
            "[x",
            "\"x",
            "\u{feff}a",
            "a\0b",
            "a\u{7f}b",
        ] {
            for params in [serde_json::Value::Null, json!({ "k": 1 })] {
                let calls = [call("system.info", json!({})), call(method, params)];
                assert_eq!(
                    format_sequence(&calls),
                    Err(UnrepresentableMethod {
                        index: 1,
                        method: method.to_string()
                    }),
                    "{method:?}"
                );
            }
        }
    }

    /// 공백·제어 문자가 아닌 문자는 method 중간이나 앞에 있어도 왕복한다.
    #[test]
    fn a_method_with_other_characters_round_trips() {
        for method in ["a#b", "시스템.정보", "a{b", "a\u{feff}b", "x.y-z_1"] {
            let calls = [
                call(method, serde_json::Value::Null),
                call(method, json!([])),
            ];
            assert_eq!(round_trip(&calls), calls, "{method:?}");
        }
    }

    #[test]
    fn a_leading_byte_order_mark_is_ignored() {
        assert_eq!(
            parse_sequence("\u{feff}system.info\nworkspace.list {}\n").unwrap(),
            vec![
                call("system.info", serde_json::Value::Null),
                call("workspace.list", json!({})),
            ]
        );
        assert_eq!(
            parse_sequence("\u{feff}# comment\n\u{feff}a\n").unwrap_err(),
            SequenceTextError {
                line: 2,
                kind: SequenceTextErrorKind::InvalidMethod
            }
        );
    }

    #[test]
    fn a_control_character_in_a_typed_method_is_an_error() {
        assert_eq!(
            parse_sequence("system.info\na\u{0}b {}\n").unwrap_err(),
            SequenceTextError {
                line: 2,
                kind: SequenceTextErrorKind::InvalidMethod
            }
        );
    }

    #[test]
    fn blank_lines_comments_and_spacing_are_ignored() {
        let text = "\
# notify then list
  system.info

notification.send   {\"title\":\"x\"}  \r
\t# indented comment
workspace.list {}
";
        assert_eq!(
            parse_sequence(text).unwrap(),
            vec![
                call("system.info", serde_json::Value::Null),
                call("notification.send", json!({ "title": "x" })),
                call("workspace.list", json!({})),
            ]
        );
    }

    /// 다시 쓴 문자열에서는 주석·빈 줄·공백이 빠지고 키는 정렬되며 숫자·escape 는 serde 표기가 된다.
    #[test]
    fn rewriting_parsed_text_keeps_the_calls_but_not_the_spelling() {
        let typed = "# c\n\n  a   {\"z\": 1e2, \"a\": \"\\u0041\"}\n";
        let rewritten = format_sequence(&parse_sequence(typed).unwrap()).unwrap();
        assert_eq!(rewritten, "a {\"a\":\"A\",\"z\":100.0}\n");
        assert_eq!(
            parse_sequence(&rewritten).unwrap(),
            parse_sequence(typed).unwrap()
        );
    }

    #[test]
    fn a_hash_inside_params_is_not_a_comment() {
        assert_eq!(
            parse_sequence("surface.send_text {\"text\":\"echo # hi\"}").unwrap(),
            vec![call("surface.send_text", json!({ "text": "echo # hi" }))]
        );
    }

    #[test]
    fn params_without_a_method_name_the_line() {
        let err = parse_sequence("system.info\n\n{\"title\":\"x\"}\n").unwrap_err();
        assert_eq!(
            err,
            SequenceTextError {
                line: 3,
                kind: SequenceTextErrorKind::MissingMethod
            }
        );
        assert_eq!(
            err.to_string(),
            "line 3: expected a method name before the params"
        );
    }

    fn params_error_column(text: &str) -> usize {
        match parse_sequence(text).unwrap_err().kind {
            SequenceTextErrorKind::InvalidParams { column, .. } => column,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bad_params_json_names_the_line_and_column() {
        let err = parse_sequence("system.info\n  notification.send {\"title\": x}\n").unwrap_err();
        assert_eq!(err.line, 2);
        // 줄은 `  notification.send {"title": x}` 이고 x 는 31번째 글자다.
        assert_eq!(
            err.to_string(),
            "line 2, column 31: invalid params JSON: expected value"
        );
    }

    /// 열은 바이트가 아니라 글자 수다. 멀티바이트 문자가 params·메서드·들여쓰기 어디에 있어도 같다.
    #[test]
    fn the_error_column_counts_characters_not_bytes() {
        // `a {"한글": x}` 의 x 는 10번째 글자다.
        assert_eq!(params_error_column("a {\"한글\": x}"), 10);
        assert_eq!(params_error_column("  한 {\"k\": x}"), 11);
        assert_eq!(params_error_column("\ta {\"k\": x}"), 10);
        // 오류 지점이 멀티바이트 글자 자신이어도 그 글자를 가리킨다: `a {"k": 한}` 의 한은 9번째다.
        assert_eq!(params_error_column("a {\"k\": 한}"), 9);
    }

    #[test]
    fn trailing_text_after_the_params_is_an_error() {
        assert_eq!(params_error_column("workspace.list {} {}"), 19);
    }

    /// 저장 형식(`hook-handler get` 의 calls JSON)을 읽은 값이 그대로 문자열을 오간다.
    #[test]
    fn calls_read_from_the_cli_json_shape_round_trip() {
        let calls: Vec<IpcCall> = serde_json::from_value(json!([
            { "method": "system.info" },
            { "method": "notification.send", "params": { "title": "${header.X-Event}" } },
        ]))
        .unwrap();
        assert_eq!(round_trip(&calls), calls);
    }
}
