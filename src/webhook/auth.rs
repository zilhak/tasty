//! 웹훅별 공유 토큰 인증. 설정이 없으면 인증 없이 통과한다.
//! 쿼리·Bearer 헤더·JSON 필드·지정 헤더에서 토큰을 읽고, 없거나 다르면 401로 거부한다.
//! 응답에는 토큰이나 실행 결과를 넣지 않는다.

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
    /// 기대 토큰. list/info는 이 값 대신 summary를 사용한다.
    pub token: String,
}

impl WebhookAuth {
    /// 지정 위치의 토큰을 비교한다. headers의 이름은 소문자로 정규화돼 있어야 한다.
    /// body는 파싱된 JSON이며 비-JSON·파싱 실패는 Null이다.
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
                // 헤더 이름은 소문자로 저장된다.
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

/// Bearer 또는 bearer 접두사 뒤의 토큰을 구한다. 다른 대소문자 조합은 받지 않는다.
fn strip_bearer(raw: &str) -> Option<&str> {
    let rest = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?;
    Some(rest.trim())
}

/// 점 구분 경로의 문자열 값을 찾는다. 다른 JSON 타입이면 None이다.
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

/// 길이가 다르면 즉시 반환하고, 같으면 XOR 결과를 누적한다.
/// 컴파일된 코드의 상수시간 실행이나 타이밍 정보 비노출을 보장하지는 않는다.
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
