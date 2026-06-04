//! File 도메인의 Core method wrapper (D.3.C.G.3).
//!
//! `Core::apply` 시그니처 (engine 만 받음) 가 *AppState 를 mutate 할 수 없는*
//! 분기들을 본 모듈의 Method 로 흡수한다. `apply_identify_result` /
//! `apply_file_picker_result` 는 popup 오픈 (state.popups) 과 dialogs 슬롯
//! 정리 (state.dialogs.file_handler_picker) 가 필요해 DomainIntent 가 아닌
//! Method call 로 분류 (옵션 C, verify-g3 §2.1).
//!
//! `reload_file_handlers` 는 단순 응답형 Method.

use std::path::PathBuf;

use crate::core::Core;
use crate::core::CoreState;
#[cfg(feature = "gui")]
use crate::file::format::{DetectorId, FileTarget};
#[cfg(feature = "gui")]
use crate::state::{AppState, FileHandlerPickerResult};

/// `Core::reload_file_handlers` 응답.
pub(crate) struct ReloadFileHandlersOutcome {
    pub(crate) path: PathBuf,
    pub(crate) exists: bool,
}

impl Core {
    /// User TOML (`~/.tasty/file-handlers.toml`) 재로드. file_format / file_handler
    /// registry 모두 reload. 응답: `{ path, exists }`.
    pub(crate) fn reload_file_handlers(&self, engine: &CoreState) -> ReloadFileHandlersOutcome {
        let path = user_config_path();
        engine.file_format.reload_user_config(&path);
        engine.file_handler.reload_user_config(&path);
        let exists = path.exists();
        ReloadFileHandlersOutcome { path, exists }
    }

    /// IdentifyWorker 의 비동기 detect 결과 적용. `event_handler` 가
    /// `AppEvent::IdentifyDone` 수신 시 직접 호출. detector 매칭 handler 가
    /// 있으면 1순위 자동 실행, 없으면 picker popup 오픈 (state.dialogs +
    /// state.popups mutate). 옛 `file_dispatch::apply_identify_result` 본문 흡수.
    #[cfg(feature = "gui")]
    pub(crate) fn apply_identify_result(
        &mut self,
        state: &mut AppState,
        engine: &mut CoreState,
        target: FileTarget,
        detector: Option<DetectorId>,
        origin_surface_id: Option<u32>,
    ) {
        let handlers = match &detector {
            Some(d) => engine.file_handler.handlers_for(d),
            None => Vec::new(),
        };
        if handlers.is_empty() {
            crate::file::dispatch::open_picker(state, engine, target, detector, Vec::new());
            return;
        }
        // 정렬 1순위가 자동 선택. 단일 / 복수 동일 — 첫 항목 dispatch.
        let first = handlers.into_iter().next().expect("non-empty checked");
        crate::file::dispatch::execute_handler_action(
            self,
            state,
            engine,
            &first,
            &target,
            origin_surface_id,
        );
    }

    /// Picker popup 결과를 적용 — `redraw.rs` frame end 가 직접 호출.
    /// `Selected(id)` 면 handler 실행 + recent 기록, `Cancelled` 면 no-op.
    /// 옛 `file_dispatch::consume_picker_result` 본문 흡수. dialogs 슬롯 해제는
    /// caller (redraw) 가 본 method 호출 *전에* 처리한다 — 빠른 popup 재오픈
    /// 시에도 결과 중복 처리 없음을 보장하기 위함.
    #[cfg(feature = "gui")]
    pub(crate) fn apply_file_picker_result(
        &mut self,
        state: &mut AppState,
        engine: &mut CoreState,
        target: FileTarget,
        result: FileHandlerPickerResult,
    ) {
        match result {
            FileHandlerPickerResult::Selected(handler_id) => {
                match engine.file_handler.get(&handler_id) {
                    Some(handler) => crate::file::dispatch::execute_handler_action(
                        self, state, engine, &handler, &target, None,
                    ),
                    None => tracing::warn!(
                        handler_id = %handler_id,
                        "apply_file_picker_result: handler id from picker no longer in registry",
                    ),
                }
                engine.record_file_handler_pick(&handler_id);
            }
            FileHandlerPickerResult::Cancelled => {
                // recent 갱신 없음.
            }
        }
    }
}

/// `~/.tasty/file-handlers.toml`. 홈 디렉토리 결정 실패 시 임시 경로 — 그 경우
/// `exists` 가 false 로 돌아오므로 caller 가 인지 가능.
fn user_config_path() -> PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("file-handlers.toml"))
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-file-handlers.toml"))
}
