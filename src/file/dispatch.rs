// 이유: 파일 열기 디스패치의 호출 트리가 전부 gui 라 headless 빌드엔 호출자가 없다. 모듈을
// `#[cfg]` 로 가리지 않는 것은 headless 에서도 타입체크를 받게 하려는 것이다.
#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]
//! 파일 디스패치 helper 잔존 모듈 (D.3.C.G.3 완료 후).
//!
//! mouse.rs ctrl+click, drag&drop, explorer plugin, IPC `file_handler.dispatch`
//! 가 모두 `DomainIntent::DispatchFile` 발화로 통일된다. Core::apply 가
//! `engine.identify_worker.spawn(...)` 호출 → 비동기 detect → AppEvent::IdentifyDone
//! → `Core::apply_identify_result` Method 호출. picker 결과는
//! `App::dispatch_pending_picker_results` → `Core::apply_file_picker_result`.
//!
//! 본 모듈에는 *parse_link* (URI 분류) 와 Core method 가 호출하는 helper
//! (`open_picker`, `execute_handler_action`) 만 남아 있다.

use std::path::PathBuf;

use crate::file::format::{DetectorId, FileTarget};
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

/// Picker popup 을 띄운다. 후보가 비어도 호출 — empty-state UI 가 보여진다.
///
/// `candidates_are_fallback` 이 true 면 `candidates` 는 detector 매칭이 아니라
/// `FileHandlerRegistry::all_handlers()` fallback 목록 — `recent` 와 겹치는 항목은
/// 좌측 후보 열에서 제외한다(중복 표시 방지).
pub(crate) fn open_picker(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    target: FileTarget,
    detector: Option<DetectorId>,
    candidates: Vec<FileHandler>,
    candidates_are_fallback: bool,
    ignore_size_limit: bool,
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
    let cand: Vec<_> = candidates
        .iter()
        .filter(|h| !recent_ids.contains(&h.id))
        .map(handler_to_summary)
        .collect();
    let target_display = target.display();
    state.dialogs.file_handler_picker = Some(FileHandlerPickerData {
        target,
        target_display,
        detector,
        candidates: cand,
        candidates_are_fallback,
        recent,
        selected: None,
        result: None,
        ignore_size_limit,
    });
    #[cfg(feature = "gui")]
    state
        .popups
        .open_centered_focused(crate::adapters::ui::popup::file_handler_picker::PICKER_POPUP_ID);
    #[cfg(not(feature = "gui"))]
    let _ = state; // headless: picker popup unavailable.
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
///
/// `origin_surface_id` 가 Some 이면 OpenSurface 는 그 surface 가 속한 *Pane* 에
/// 새 tab 으로 결과를 추가한다 (focus 독립). None 이면 focused pane 의 새 탭
/// (기존 동작). 다른 action (Ipc / System) 은 origin 영향 없음.
pub fn execute_handler_action(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    handler: &FileHandler,
    target: &FileTarget,
    origin_surface_id: Option<u32>,
    ignore_size_limit: bool,
) {
    match &handler.action {
        HandlerAction::OpenSurface {
            surface_kind,
            param_key,
        } => {
            let path_str = target.as_path().to_string_lossy().into_owned();

            // 대용량 파일 확인 게이트는 **plugin 소유**로 이전됐다: 크기 감지도 확인 팝업도
            // plugin in-process(`crates/tasty-plugin-markdown`)가 소유하고, host 는 파일
            // 크기를 stat 하지 않는다(불가침 원칙 — host 는 특정 kind 의 크기게이트를 모른다).
            // `ignore_size_limit` 은 옛 게이트의 우회 플래그였으므로 게이트 제거 후엔 소비만
            // 한다(dispatch 파이프라인 호출부 시그니처는 그대로 유지).
            let _ = ignore_size_limit;

            let params = serde_json::json!({ param_key.as_str(): path_str });
            open_surface_tab(core, state, engine, surface_kind, params, origin_surface_id);
        }
        HandlerAction::Ipc { method, .. } => {
            // 이 분기는 state.pending_handler_ipc 에 enqueue 만 — core/engine 미사용.
            state
                .pending_handler_ipc
                .push((method.clone(), target.clone()));
        }
        HandlerAction::System => {
            // OS 기본 opener 만 호출 — core/state/engine 미사용.
            let uri = path_to_file_uri(target.as_path());
            #[cfg(feature = "gui")]
            crate::terminal_link::open_uri(&uri);
            #[cfg(not(feature = "gui"))]
            tracing::warn!("HandlerAction::System ignored in headless build: {uri}");
        }
    }
}

/// OpenSurface 결과를 실제 tab 으로 연다. `origin_surface_id` 가 Some 이면 그 surface
/// 의 *Pane* 에 새 tab(focus 독립), None 이면 focused pane 의 새 탭.
pub(crate) fn open_surface_tab(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    surface_kind: &str,
    params: serde_json::Value,
    origin_surface_id: Option<u32>,
) {
    let origin_pane = origin_surface_id.and_then(|sid| engine.find_pane_for_surface(sid));
    match origin_pane {
        Some(pane_id) => {
            // 이 분기는 인텐트 계층을 거치지 않고 Core 로 직접 apply 하므로(링크 클릭 등
            // origin surface 의 pane 에 새 탭), 최근 목록 기록을 여기서 직접 한다. None
            // 분기는 `Intent::NewTab` 으로 위임되어 tab 핸들러가 기록한다. kind 하드코딩
            // 없이 매니페스트 `records_recent` 를 선언한 kind 만 기록(generic per-kind).
            if engine
                .surface_registry
                .get(surface_kind)
                .is_some_and(|d| d.records_recent)
            {
                state.record_recent(surface_kind, &params);
            }
            let intent = crate::core::intent::DomainIntent::CreateTab {
                pane_id,
                cwd: None,
                kind: surface_kind.to_string(),
                name: None,
                surface_params: params,
            };
            if let Err(e) = core.apply(engine, intent) {
                tracing::warn!(
                    pane_id,
                    kind = %surface_kind,
                    "file_dispatch CreateTab failed: {e}",
                );
            }
        }
        None => {
            state.dispatch_intent(
                crate::intent::Intent::NewTab {
                    kind: Some(surface_kind.to_string()),
                    params,
                }
                .from_user_menu("file_dispatch"),
            );
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
