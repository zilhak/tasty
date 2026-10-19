//! `surface.html_script_allow` — 에이전트가 html surface의 현재 문서 스크립트를 허용한다.
//!
//! 배너의 허용 버튼과 같은 `HtmlScriptState::allow_current`로 허용 기록을 남기고 재로드를 요청한다.
//! 대상은 surface ID로만 받고 포커스·선택·활성 탭은 바꾸지 않는다. 조회한 문서와 허용할 문서가
//! 달라지는 경쟁은 `surface.html_script` 응답의 `url`·`fingerprint`를 인자로 받아 막는다.

use serde_json::{Value, json};
use tasty_ipc::protocol::JsonRpcResponse;
use tasty_model::html_script::{AllowError, HtmlScriptState};

use super::html_script::find_html;

/// 허용을 거절한 상태 오류. 사유는 `error.data.reason`으로 구분한다.
pub(crate) const HTML_SCRIPT_ALLOW_REFUSED: i32 = -32022;

/// 허용 요청을 판정한 결과.
#[derive(Debug, PartialEq)]
enum Outcome {
    Allowed,
    AlreadyAllowed,
    Refused(&'static str, String),
}

fn decide(st: &mut HtmlScriptState, url: Option<&str>, fingerprint: Option<&str>) -> Outcome {
    if st.loading_before_commit() {
        return Outcome::Refused("loading", AllowError::Loading.to_string());
    }
    let Some(doc) = st.current() else {
        return Outcome::Refused("no_document", AllowError::NoDocument.to_string());
    };
    let current_fp = doc.scan.map(|s| s.fingerprint.to_hex());
    let url_changed = url.is_some_and(|u| u != doc.url);
    let fp_changed =
        fingerprint.is_some_and(|f| current_fp.as_deref() != Some(f.to_ascii_lowercase().as_str()));
    if url_changed || fp_changed {
        return Outcome::Refused(
            "document_changed",
            "the current document is not the one given by url/fingerprint; \
             read surface.html_script again"
                .to_string(),
        );
    }
    if st.current_is_allowed() {
        return Outcome::AlreadyAllowed;
    }
    match st.allow_current() {
        Ok(()) => Outcome::Allowed,
        Err(e @ AllowError::NoFingerprint) => Outcome::Refused("no_fingerprint", e.to_string()),
        Err(e @ AllowError::NoDocument) => Outcome::Refused("no_document", e.to_string()),
        Err(e @ AllowError::Loading) => Outcome::Refused("loading", e.to_string()),
    }
}

fn optional_str<'a>(
    params: &'a Value,
    key: &str,
    id: &Value,
) -> Result<Option<&'a str>, JsonRpcResponse> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(JsonRpcResponse::invalid_params(
            id.clone(),
            format!("'{key}' must be a string"),
        )),
    }
}

pub(crate) fn handle_html_script_allow(
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let (url, fingerprint) = match (
        optional_str(params, "url", &id),
        optional_str(params, "fingerprint", &id),
    ) {
        (Ok(u), Ok(f)) => (u, f),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let (surface_id, rs) = match find_html(engine, &id, params) {
        Ok(found) => found,
        Err(e) => return e,
    };
    let (outcome, document) = rs.with_html_script(|st| {
        let outcome = decide(st, url, fingerprint);
        let document = st.current().map(|doc| {
            json!({
                "url": doc.url,
                "detection": doc.scan.map(|s| s.detection.as_str()),
                "fingerprint": doc.scan.map(|s| s.fingerprint.to_hex()),
            })
        });
        (outcome, document)
    });
    match outcome {
        Outcome::Allowed | Outcome::AlreadyAllowed => {
            let allowed_now = outcome == Outcome::Allowed;
            tracing::info!(surface_id, allowed_now, "html scripts allowed by an agent");
            JsonRpcResponse::success(
                id,
                json!({
                    "surface_id": surface_id,
                    "result": if allowed_now { "allowed" } else { "already_allowed" },
                    "reload": allowed_now,
                    "document": document,
                }),
            )
        }
        Outcome::Refused(reason, message) => {
            tracing::warn!(
                surface_id,
                reason,
                "surface.html_script_allow refused: {message}"
            );
            JsonRpcResponse::error_with_data(
                id,
                HTML_SCRIPT_ALLOW_REFUSED,
                message,
                json!({ "reason": reason, "surface_id": surface_id, "document": document }),
            )
        }
    }
}

#[cfg(test)]
mod tests;
