//! Request-local presentation queries and output collection for engine handlers.
//! RequestScope owns the captured defaults and returned intents; it exposes no View or lifecycle methods.
//! Explicit GUI/debug routes run separately after the engine phase.

use crate::runtime::engine_access::{EngineMut, EngineRef};
use std::path::PathBuf;

use crate::core::CoreState;
use crate::intent::DispatchedIntent;

/// Compatibility query names over request values. Implemented by RequestScope.
pub(crate) trait IpcWindow {
    fn presentation(&self) -> &dyn crate::model::StructurePresentation;
    fn resolve_inherit_cwd_from_surface(
        &self,
        engine: &EngineRef<'_>,
        surface: u32,
    ) -> Option<PathBuf>;
    /// 이 창의 활성 workspace 인덱스. 대상 생략 호환 경로나 응답의 활성 표시에서 쓴다.
    /// 명시 대상이 있는 요청은 그 대상의 소속을 우선한다(ADR-0059).
    fn active_workspace_index(&self, engine: &CoreState) -> usize;

    /// 새 워크스페이스의 cwd 상속 원본 — 설정(`inherit_cwd`)과 이 창의 포커스 surface 를 본다.
    fn resolve_inherit_cwd(&self, engine: &EngineRef<'_>) -> Option<PathBuf>;

    /// 이 창이 기억하는 최근 파일(최신순).
    fn recent_files(&self, kind: &str) -> Vec<String>;

    /// 발행된 capability 승인 요청을 이 창의 승인 팝업 큐에 넣는다.
    #[cfg(feature = "gui")]
    fn enqueue_approval_popup(
        &mut self,
        engine: &CoreState,
        record: &tasty_approval::ApprovalRecord,
    );

    /// 호출 플러그인이 소유한 열린 팝업이 사용자의 확정 입력을 받았는지 확인한다.
    #[cfg(feature = "gui")]
    fn plugin_popup_user_activated(&self, plugin_id: &str, instance_id: u64) -> bool;

    /// 호출 플러그인의 webview에서 기록한 사용자 URL인지 확인하고 기록을 소비한다.
    /// 같은 navigation은 한 번만 사용자 요청 근거로 사용할 수 있다(ADR-0031).
    #[cfg(feature = "gui")]
    fn take_webview_user_navigation(
        &mut self,
        engine: &EngineRef<'_>,
        plugin_id: &str,
        surface_id: u32,
        url: &str,
    ) -> bool;

    /// 요청 하나가 모은 intent 를 이 창의 큐 끝에 순서대로 옮긴다. 진입점만 부른다.
    fn enqueue_intents(&mut self, intents: IntentOutbox);
}

/// 요청별 intent 목록. 핸들러는 여기에 넣고 진입점이 창 큐로 옮긴다.
#[derive(Default, Debug)]
pub(crate) struct IntentOutbox(Vec<DispatchedIntent>);

impl IntentOutbox {
    pub(crate) fn push(&mut self, intent: DispatchedIntent) {
        self.0.push(intent);
    }

    pub(crate) fn into_vec(self) -> Vec<DispatchedIntent> {
        self.0
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
