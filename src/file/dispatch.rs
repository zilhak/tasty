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

/// 대용량 markdown 확인 게이트의 임계값 (1MB). 이 값을 *초과* 하면 확인 팝업.
pub(crate) const MD_SIZE_LIMIT_BYTES: u64 = 1024 * 1024;

/// `path` 가 크기 게이트를 초과하는 일반 파일이면 그 크기(bytes)를 반환.
///
/// 파일이 없거나 stat 실패 / 정확히 임계값 이하면 `None` — 게이트를 통과시키고
/// 기존 오픈 흐름에 맡긴다(로드 실패는 플러그인이 표시). 경계값(정확히 1MB)은
/// 통과(`None`) — *초과* 만 게이트.
pub(crate) fn exceeds_md_size_limit(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .map(|m| m.len())
        .filter(|&len| len > MD_SIZE_LIMIT_BYTES)
}

/// Picker popup 을 띄운다. 후보가 비어도 호출 — empty-state UI 가 보여진다.
pub(crate) fn open_picker(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    target: FileTarget,
    detector: Option<DetectorId>,
    candidates: Vec<FileHandler>,
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

            // 대용량 markdown 확인 게이트 (01-md-size-confirm-gate). bypass 아니고
            // 1MB 초과면 실제 오픈 대신 확인 팝업을 띄우고 오픈을 보류한다. [열기]
            // 확정 시 `Core::apply_pending_md_open` 이 재개한다. pane/workspace/tab 의
            // 직접 생성 IPC 는 이 함수를 경유하지 않으므로 게이트 대상 밖(항상 즉시 생성).
            #[cfg(feature = "gui")]
            if surface_kind == "markdown"
                && !ignore_size_limit
                && let Some(size) = exceeds_md_size_limit(target.as_path())
            {
                state.dialogs.pending_md_open = Some(crate::state::PendingMdOpen {
                    path: path_str,
                    size,
                    result: None,
                    kind: crate::state::PendingMdOpenKind::NewTab {
                        param_key: param_key.clone(),
                        surface_kind: surface_kind.clone(),
                        origin_surface_id,
                    },
                });
                let scope = match origin_surface_id {
                    Some(sid) => crate::adapters::ui::popup::PopupScope::Surface(sid),
                    None => crate::adapters::ui::popup::PopupScope::Window,
                };
                state.dispatch_intent(
                    crate::intent::UiIntent::OpenPopup {
                        id: "markdown_size_confirm",
                        mode: crate::intent::OpenPopupMode::WithScope(scope),
                    }
                    .from_user_menu("markdown_size_gate"),
                );
                return;
            }
            #[cfg(not(feature = "gui"))]
            let _ = ignore_size_limit; // headless: 확인 팝업 없음 — 게이트 미적용, 값만 소비.

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
/// 의 *Pane* 에 새 tab(focus 독립), None 이면 focused pane 의 새 탭. 게이트 통과분과
/// 대용량 확인 후 재개(`Core::apply_pending_md_open`) 가 공유한다.
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
            let _ = state; // CreateTab 본문이 state mutate (cascade).
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

    #[test]
    fn size_gate_boundary_and_over() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, len: usize| {
            let p = dir.path().join(name);
            std::fs::write(&p, vec![b'x'; len]).unwrap();
            p
        };
        // 미만 → None.
        assert_eq!(exceeds_md_size_limit(&write("small.md", 500 * 1024)), None,);
        // 정확히 임계값 → None (초과만 게이트).
        assert_eq!(
            exceeds_md_size_limit(&write("exact.md", MD_SIZE_LIMIT_BYTES as usize)),
            None,
        );
        // 초과 → Some(len).
        let over = MD_SIZE_LIMIT_BYTES as usize + 1;
        assert_eq!(
            exceeds_md_size_limit(&write("big.md", over)),
            Some(over as u64),
        );
        // 없는 파일 → None (게이트 통과).
        assert_eq!(exceeds_md_size_limit(&dir.path().join("missing.md")), None,);
    }
}
