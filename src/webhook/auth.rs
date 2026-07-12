//! 웹훅별 **선택적** 인증 — 가벼운 발신자 확인(고정 공유 토큰).
//!
//! 위협 모델(research §4.2): 유일 위협은 opaque URL 로 들어오는 외부 악의적 요청.
//! 인증은 4중 방어선 중 두 번째("가벼운 발신자 확인, HMAC 불요")다 — 핸들러가
//! OS 무관 tasty IPC 만 트리거하므로 서명 검증까지는 과하고, 발신자가 owner 가
//! 심어둔 고정 토큰을 제시하는지만 확인한다.
//!
//! - **미설정(`None`) 시 무인증 통과** — 인증은 opt-in.
//! - 토큰 위치는 [`AuthLocation`] 4종(쿼리/Bearer/바디필드/임의헤더).
//! - 비교는 [`ct_eq`] 상수시간 — 타이밍 부채널로 토큰을 유추당하지 않도록.
//! - 불일치/미제시 시 401(리스너가 [`super::ack::AckStatus::Unauthorized`] 로 응답).
//!
//! 단방향 불변식 유지: 인증은 **ACK 상태코드 선택에만** 관여하고 실행 경로·응답
//! 바디에 내부 데이터를 싣지 않는다.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 요청에서 토큰을 어디서 뽑을지 지정한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "location", rename_all = "snake_case")]
pub enum AuthLocation {
    /// 쿼리 파라미터 `?<key>=<token>` 의 값.
    QueryKey { key: String },
    /// `Authorization: Bearer <token>` 헤더의 토큰부.
    BearerHeader,
    /// 요청 바디 JSON 의 `<field>`(점 구분 경로) 위치 문자열 값.
    BodyField { field: String },
    /// 임의 헤더 `<name>: <token>` 의 값.
    HeaderKey { name: String },
}

/// 웹훅 인증 설정 — 위치 + 기대 토큰(고정 공유 비밀).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookAuth {
    pub location: AuthLocation,
    /// 기대하는 고정 토큰. **조회 응답(list/info)에 절대 노출하지 않는다.**
    pub token: String,
}

impl WebhookAuth {
    /// 요청에서 위치별로 토큰을 뽑아 기대값과 상수시간 비교한다.
    ///
    /// `headers` 는 소문자 정규화된 이름→값, `query` 는 파라미터 이름→값, `body` 는
    /// 파싱된 JSON(비-JSON/파싱실패면 `Null`)이다. 토큰 미제시/불일치면 `false`.
    pub fn verify(
        &self,
        headers: &BTreeMap<String, String>,
        query: &BTreeMap<String, String>,
        body: &Value,
    ) -> bool {
        match self.presented_token(headers, query, body) {
            Some(presented) => ct_eq(presented.as_bytes(), self.token.as_bytes()),
            None => false,
        }
    }

    /// 요청에서 제시된 토큰을 위치별로 추출한다(없으면 `None`).
    fn presented_token(
        &self,
        headers: &BTreeMap<String, String>,
        query: &BTreeMap<String, String>,
        body: &Value,
    ) -> Option<String> {
        match &self.location {
            AuthLocation::QueryKey { key } => query.get(key).cloned(),
            AuthLocation::BearerHeader => {
                // 헤더 이름은 소문자 정규화되어 저장됨. 스킴은 대소문자 무시.
                let raw = headers.get("authorization")?;
                strip_bearer(raw).map(str::to_string)
            }
            AuthLocation::BodyField { field } => {
                resolve_body_string(body, field).map(str::to_string)
            }
            AuthLocation::HeaderKey { name } => headers.get(&name.to_ascii_lowercase()).cloned(),
        }
    }
}

/// `Bearer <token>` 에서 토큰부를 뽑는다(스킴 대소문자 무시). 스킴이 아니면 `None`.
fn strip_bearer(raw: &str) -> Option<&str> {
    let rest = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?;
    Some(rest.trim())
}

/// 바디 JSON 에서 점 구분 경로 위치의 **문자열** 값을 찾는다.
///
/// 인증 토큰은 문자열이어야 하므로 leaf 가 문자열이 아니면 `None`(불일치 처리).
fn resolve_body_string<'a>(body: &'a Value, path: &str) -> Option<&'a str> {
    let mut cur = body;
    for seg in path.split('.') {
        cur = match cur {
            Value::Object(map) => map.get(seg)?,
            Value::Array(arr) => arr.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    cur.as_str()
}

/// 상수시간 바이트 비교 — 일치 시에도 조기반환하지 않아 토큰 내용을 타이밍으로
/// 유추당하지 않게 한다. 길이는 조기 판별한다(길이 정보만 노출, 실용상 무해).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 조회 응답용 요약(**토큰 제외**). location 종류와 참조 키만 노출한다.
pub fn auth_summary(auth: &WebhookAuth) -> Value {
    let (kind, key) = match &auth.location {
        AuthLocation::QueryKey { key } => ("query", Some(key.clone())),
        AuthLocation::BearerHeader => ("bearer", None),
        AuthLocation::BodyField { field } => ("body", Some(field.clone())),
        AuthLocation::HeaderKey { name } => ("header", Some(name.clone())),
    };
    serde_json::json!({ "location": kind, "key": key })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn headers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v.to_string()))
            .collect()
    }

    fn query(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn query_key_match_and_mismatch() {
        let auth = WebhookAuth {
            location: AuthLocation::QueryKey {
                key: "token".into(),
            },
            token: "s3cret".into(),
        };
        assert!(auth.verify(&headers(&[]), &query(&[("token", "s3cret")]), &Value::Null));
        assert!(!auth.verify(&headers(&[]), &query(&[("token", "wrong")]), &Value::Null));
        // 미제시 → false.
        assert!(!auth.verify(&headers(&[]), &query(&[]), &Value::Null));
    }

    #[test]
    fn bearer_header_scheme_case_insensitive() {
        let auth = WebhookAuth {
            location: AuthLocation::BearerHeader,
            token: "abc".into(),
        };
        assert!(auth.verify(
            &headers(&[("Authorization", "Bearer abc")]),
            &query(&[]),
            &Value::Null
        ));
        assert!(auth.verify(
            &headers(&[("Authorization", "bearer abc")]),
            &query(&[]),
            &Value::Null
        ));
        assert!(!auth.verify(
            &headers(&[("Authorization", "Bearer nope")]),
            &query(&[]),
            &Value::Null
        ));
        // 스킴 없는 값 → None → false.
        assert!(!auth.verify(
            &headers(&[("Authorization", "abc")]),
            &query(&[]),
            &Value::Null
        ));
    }

    #[test]
    fn body_field_nested_string() {
        let auth = WebhookAuth {
            location: AuthLocation::BodyField {
                field: "meta.token".into(),
            },
            token: "t0k".into(),
        };
        let body = json!({"meta": {"token": "t0k"}});
        assert!(auth.verify(&headers(&[]), &query(&[]), &body));
        let wrong = json!({"meta": {"token": "no"}});
        assert!(!auth.verify(&headers(&[]), &query(&[]), &wrong));
        // 비-문자열 leaf → 불일치.
        let numeric = json!({"meta": {"token": 42}});
        assert!(!auth.verify(&headers(&[]), &query(&[]), &numeric));
    }

    #[test]
    fn header_key_lookup_case_insensitive() {
        let auth = WebhookAuth {
            location: AuthLocation::HeaderKey {
                name: "X-Webhook-Token".into(),
            },
            token: "hk".into(),
        };
        // 저장은 소문자, 조회 시 name 도 소문자화.
        assert!(auth.verify(
            &headers(&[("X-Webhook-Token", "hk")]),
            &query(&[]),
            &Value::Null
        ));
        assert!(!auth.verify(
            &headers(&[("X-Webhook-Token", "bad")]),
            &query(&[]),
            &Value::Null
        ));
        assert!(!auth.verify(&headers(&[]), &query(&[]), &Value::Null));
    }

    #[test]
    fn ct_eq_basic() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn summary_never_exposes_token() {
        let auth = WebhookAuth {
            location: AuthLocation::QueryKey {
                key: "token".into(),
            },
            token: "super-secret".into(),
        };
        let summary = auth_summary(&auth);
        let text = summary.to_string();
        assert!(!text.contains("super-secret"));
        assert_eq!(summary["location"], json!("query"));
        assert_eq!(summary["key"], json!("token"));
    }

    #[test]
    fn serde_roundtrip_tagged() {
        let auth = WebhookAuth {
            location: AuthLocation::HeaderKey {
                name: "X-Tok".into(),
            },
            token: "x".into(),
        };
        let s = serde_json::to_string(&auth).unwrap();
        let back: WebhookAuth = serde_json::from_str(&s).unwrap();
        assert_eq!(auth, back);
    }
}
