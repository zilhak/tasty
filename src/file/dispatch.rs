//! 파일 디스패치 공통 진입점 (D.3.C.G.3 — 점진 폐기 중).
//!
//! mouse.rs ctrl+click, drag&drop, explorer plugin, IPC `file_handler.dispatch`
//! 가 모두 `DomainIntent::DispatchFile` 발화로 통일된다. Core::apply 가
//! `engine.identify_worker.spawn(...)` 호출 → 비동기 detect → AppEvent::IdentifyDone
//! → `event_handler` 가 `Core::apply_identify_result` Method 호출.
//!
//! 본 모듈에는 *parse_link* (URI 분류) 와 `apply_identify_result` /
//! `consume_picker_result` 가 남아 있다. G.3.b/G.3.c 에서 후자 둘은 Core 안으로
//! 흡수 + free function 폐기 예정.

use std::path::PathBuf;

use crate::file::format::{DetectDepth, DetectorId, FileTarget};
use crate::file::handler::{FileHandler, HandlerAction, HandlerId};
use crate::state::{AppState, FileHandlerPickerData, PickerHandlerSummary};

/// 클릭/드롭된 URI 의 종류.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkKind {
    /// 식별 가능한 파일/디렉토리 경로 (file:// URI 또는 plain absolute path).
    FileTarget(PathBuf),
    /// webbrowser::open 으로 위임할 외부 URI (http, https, mailto, ftp, ssh, …).
    External(String),
}

/// URI 를 두 종류 중 하나로 분류. 경로 존재 검증은 안 함 — 호출자(`terminal_link::
/// resolve_path`) 가 이미 검증한 결과를 받는다.
pub fn parse_link(uri: &str) -> LinkKind {
    if let Some(rest) = uri.strip_prefix("file://") {
        // `file://` URI 규약:
        //   Unix:    file:///abs/path   → "/abs/path"
        //   Windows: file:///C:/path    → "/C:/path" — drive 문자 앞 / 한 개 strip
        // 양쪽 모두 앞 "/" 가 1개 더 붙어 있을 수 있다. Windows 만 drive 문자가
        // 뒤따르면 strip.
        let path_str = if cfg!(windows) && looks_like_windows_drive_uri(rest) {
            rest.trim_start_matches('/')
        } else {
            rest
        };
        let decoded = percent_decode_lossy(path_str);
        return LinkKind::FileTarget(PathBuf::from(decoded));
    }
    LinkKind::External(uri.to_string())
}

/// `/<letter>:/...` 모양인지 (Windows file URI 의 drive prefix).
fn looks_like_windows_drive_uri(rest: &str) -> bool {
    let bytes = rest.as_bytes();
    bytes.len() >= 4
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
        && (bytes[3] == b'/' || bytes[3] == b'\\')
}

/// percent-decode (`%20` 등). 잘못된 escape 는 원본 보존.
fn percent_decode_lossy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]));
            if let (Some(a), Some(b)) = h {
                out.push(((a << 4) | b) as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// identify 결과를 받아 handler 선택 (auto 또는 picker) 후 실행.
/// `AppEvent::IdentifyDone` 콜사이트만 사용. G.3.b 에서 Core method 로 이동 예정.
pub fn apply_identify_result(
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    target: FileTarget,
    detector: Option<DetectorId>,
) {
    let handlers = match &detector {
        Some(d) => engine.file_handler.handlers_for(d),
        None => Vec::new(),
    };
    if handlers.is_empty() {
        open_picker(state, engine, target, detector, Vec::new());
        return;
    }
    // 정렬 1순위가 자동 선택. 단일 / 복수 동일 — 첫 항목 dispatch.
    let first = handlers.into_iter().next().expect("non-empty checked");
    execute_handler_action(state, engine, &first, &target);
}

/// Picker popup 을 띄운다. 후보가 비어도 호출 — empty-state UI 가 보여진다.
fn open_picker(
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    target: FileTarget,
    detector: Option<DetectorId>,
    candidates: Vec<FileHandler>,
) {
    let recent_ids: Vec<HandlerId> = engine
        .file_handler_recent
        .list()
        .iter()
        .map(|e| e.handler_id.clone())
        .collect();
    let recent: Vec<_> = recent_ids
        .iter()
        .filter_map(|id| engine.file_handler.get(id))
        .map(|h| handler_to_summary(&h))
        .collect();
    let cand: Vec<_> = candidates.iter().map(handler_to_summary).collect();
    let target_display = target.display();
    state.dialogs.file_handler_picker = Some(FileHandlerPickerData {
        target,
        target_display,
        detector,
        candidates: cand,
        recent,
        selected: None,
        result: None,
    });
    state
        .popups
        .open_centered_focused(crate::adapters::ui::popup::file_handler_picker::PICKER_POPUP_ID);
}

fn handler_to_summary(h: &FileHandler) -> PickerHandlerSummary {
    let display = h
        .display_name_i18n_key
        .as_deref()
        .map(|k| {
            let translated = crate::i18n::t(k);
            if translated == k {
                h.id.as_str().to_string()
            } else {
                translated.to_string()
            }
        })
        .unwrap_or_else(|| h.id.as_str().to_string());
    PickerHandlerSummary {
        id: h.id.clone(),
        display,
    }
}

/// 단일 handler action 을 실행. OpenSurface 는 즉시, Ipc 는 큐로, System 은
/// webbrowser 위임.
pub fn execute_handler_action(
    state: &mut AppState,
    _engine: &mut crate::engine_state::CoreState,
    handler: &FileHandler,
    target: &FileTarget,
) {
    match &handler.action {
        HandlerAction::OpenSurface {
            surface_kind,
            param_key,
        } => {
            let path_str = target.as_path().to_string_lossy().into_owned();
            let params = serde_json::json!({ param_key.as_str(): path_str });
            state.dispatch_intent(
                crate::intent::Intent::NewTab {
                    kind: Some(surface_kind.clone()),
                    params,
                }
                .from_user_menu("file_dispatch"),
            );
        }
        HandlerAction::Ipc { method, .. } => {
            state
                .pending_handler_ipc
                .push((method.clone(), target.clone()));
        }
        HandlerAction::System => {
            let uri = path_to_file_uri(target.as_path());
            crate::terminal_link::open_uri(&uri);
        }
    }
}

/// `Path` → `file://` URI. terminal_link 의 같은 함수가 private 이라 여기 별도 정의.
fn path_to_file_uri(abs: &std::path::Path) -> String {
    let s = abs.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

/// Picker 결과를 소비해 handler 실행 + recent 기록. App 메인 루프 (frame end) 가
/// 매 frame 호출. 결과가 없으면 no-op.
pub fn consume_picker_result(state: &mut AppState, engine: &mut crate::engine_state::CoreState) {
    let Some(data) = state.dialogs.file_handler_picker.as_mut() else {
        return;
    };
    let Some(result) = data.result.take() else {
        return;
    };
    let target = data.target.clone();
    // 데이터 슬롯 즉시 해제 — 사용자가 picker 닫는 사이 빠르게 같은 popup 재개
    // 한 경우에도 결과 중복 처리 안 됨.
    state.dialogs.file_handler_picker = None;

    use crate::state::FileHandlerPickerResult;
    match result {
        FileHandlerPickerResult::Selected(handler_id) => {
            dispatch_by_handler_id(state, engine, &handler_id, &target);
            engine.record_file_handler_pick(&handler_id);
        }
        FileHandlerPickerResult::Cancelled => {
            // recent 갱신 없음.
        }
    }
}

fn dispatch_by_handler_id(
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    id: &HandlerId,
    target: &FileTarget,
) {
    match engine.file_handler.get(id) {
        Some(handler) => execute_handler_action(state, engine, &handler, target),
        None => {
            tracing::warn!(
                handler_id = %id,
                "file_dispatch: handler id from picker no longer in registry",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_link_external_http() {
        assert_eq!(
            parse_link("https://example.com/a?b=1"),
            LinkKind::External("https://example.com/a?b=1".into()),
        );
    }

    #[test]
    fn parse_link_external_mailto() {
        assert_eq!(
            parse_link("mailto:a@b.com"),
            LinkKind::External("mailto:a@b.com".into()),
        );
    }

    #[test]
    fn parse_link_file_unix() {
        assert_eq!(
            parse_link("file:///home/u/a.md"),
            LinkKind::FileTarget(PathBuf::from("/home/u/a.md")),
        );
    }

    #[test]
    fn parse_link_file_percent_decoded() {
        assert_eq!(
            parse_link("file:///home/u/My%20Doc.md"),
            LinkKind::FileTarget(PathBuf::from("/home/u/My Doc.md")),
        );
    }

    #[test]
    fn parse_link_file_percent_passthrough_invalid() {
        // 잘못된 escape 는 그대로 통과 (lossy).
        assert_eq!(
            parse_link("file:///home/%ZZ/a"),
            LinkKind::FileTarget(PathBuf::from("/home/%ZZ/a")),
        );
    }

    #[cfg(windows)]
    #[test]
    fn parse_link_file_windows_drive() {
        assert_eq!(
            parse_link("file:///C:/Users/a.txt"),
            LinkKind::FileTarget(PathBuf::from("C:/Users/a.txt")),
        );
    }
}
