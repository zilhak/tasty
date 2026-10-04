//! Decode control payloads into typed mirror events.
use super::*;

/// 제어 프레임을 mirror 이벤트로 변환한다. 어느 파서에서도 인식하지 못하면 무시한다.
pub(super) fn mirror_event_from_control(payload: &[u8]) -> Option<MirrorEvent> {
    match serde_json::from_slice::<StreamControl>(payload) {
        Ok(StreamControl::Resize {
            surface_id,
            cols,
            rows,
        }) => Some(MirrorEvent::Resize(surface_id, cols, rows)),
        Ok(StreamControl::Activity { surface_id, busy }) => {
            Some(MirrorEvent::Activity(surface_id, busy))
        }
        Ok(StreamControl::Attention { surface_id, kind }) => {
            Some(MirrorEvent::Attention(surface_id, kind))
        }
        Ok(StreamControl::Cwd { surface_id, cwd }) => Some(MirrorEvent::Cwd(surface_id, cwd)),
        // 무엇을 잃었는지 알 수 없는 연결 단위 통지라 세션 전체를 재동기화한다.
        Ok(StreamControl::Loss { frames }) => Some(MirrorEvent::Desynced { frames }),
        Ok(StreamControl::StructuralResult {
            ok: false,
            op_id,
            reason,
        }) => Some(MirrorEvent::StructuralFailed(op_id, reason)),
        Ok(StreamControl::StructuralResult {
            ok: true, op_id, ..
        }) => Some(MirrorEvent::StructuralSucceeded(op_id)),
        Ok(StreamControl::StructuralDelta {
            workspace_id,
            tree,
            surfaces,
        }) => Some(MirrorEvent::StructuralDelta {
            workspace_id,
            tree,
            surfaces,
        }),
        Ok(_) | Err(_) => parse_capture_result(payload)
            .or_else(|| parse_list_dir_result(payload))
            .or_else(|| parse_git_query_result(payload))
            .or_else(|| parse_markdown_content_result(payload))
            .or_else(|| parse_markdown_changed(payload)),
    }
}

#[derive(serde::Deserialize)]
pub(super) struct CaptureResultWire {
    ok: bool,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

pub(super) fn parse_capture_result(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("capture_result") {
        return None;
    }
    let wire: CaptureResultWire = serde_json::from_value(value).ok()?;
    Some(MirrorEvent::CaptureResult {
        ok: wire.ok,
        path: wire.path,
        reason: wire.reason,
    })
}

/// modified_unix는 epoch 초이며 DirEntryInfo의 SystemTime으로 변환한다.
#[derive(serde::Deserialize)]
pub(super) struct ListDirEntryWire {
    name: String,
    is_dir: bool,
    size: u64,
    #[serde(default)]
    modified_unix: Option<u64>,
    #[serde(default)]
    ext: String,
}

#[derive(serde::Deserialize)]
pub(super) struct ListDirResultWire {
    request_id: u64,
    ok: bool,
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    entries: Option<Vec<ListDirEntryWire>>,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    reason: Option<String>,
}

pub(super) fn parse_list_dir_result(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("list_dir_result") {
        return None;
    }
    let wire: ListDirResultWire = serde_json::from_value(value).ok()?;
    let entries = wire.entries.map(|es| {
        es.into_iter()
            .map(|e| tasty_remote::client_session::RemoteDirEntry {
                name: e.name,
                is_dir: e.is_dir,
                size: e.size,
                modified: e
                    .modified_unix
                    .map(|secs| std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)),
                ext: e.ext,
            })
            .collect()
    });
    Some(MirrorEvent::ListDirResult {
        request_id: wire.request_id,
        ok: wire.ok,
        dir: wire.dir,
        entries,
        truncated: wire.truncated,
        reason: wire.reason,
    })
}

/// kind별 데이터는 flatten으로 받고 호스트가 해석하지 않은 채 git-viewer로 전달한다.
#[derive(serde::Deserialize)]
pub(super) struct GitQueryResultWire {
    request_id: u64,
    ok: bool,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    truncated_status: bool,
    #[serde(default)]
    truncated_log: bool,
    #[serde(default)]
    truncated_diff: bool,
    #[serde(default)]
    reason: Option<String>,
    #[serde(flatten)]
    rest: serde_json::Map<String, serde_json::Value>,
}

pub(super) fn parse_git_query_result(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("git_query_result") {
        return None;
    }
    let wire: GitQueryResultWire = serde_json::from_value(value).ok()?;
    let truncated = wire.truncated_status || wire.truncated_log || wire.truncated_diff;
    let data = wire.ok.then(|| Value::Object(wire.rest));
    Some(MirrorEvent::GitQueryResult {
        request_id: wire.request_id,
        ok: wire.ok,
        kind: wire.kind,
        data,
        truncated,
        reason: wire.reason,
    })
}

#[derive(serde::Deserialize)]
pub(super) struct MarkdownContentResultWire {
    request_id: u64,
    surface_id: u32,
    ok: bool,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    reason: Option<String>,
}

pub(super) fn parse_markdown_content_result(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("markdown_content_result") {
        return None;
    }
    let wire: MarkdownContentResultWire = serde_json::from_value(value).ok()?;
    Some(MirrorEvent::MarkdownContentResult {
        request_id: wire.request_id,
        surface_id: wire.surface_id,
        ok: wire.ok,
        file: wire.file,
        source: wire.source,
        truncated: wire.truncated,
        reason: wire.reason,
    })
}

pub(super) fn parse_markdown_changed(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("markdown_changed") {
        return None;
    }
    let surface_id = u32::try_from(value.get("surface_id")?.as_u64()?).ok()?;
    Some(MirrorEvent::MarkdownChanged { surface_id })
}
