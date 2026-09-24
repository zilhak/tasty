//! IPC 핸들러가 AppState 전체 대신 사용하는 창 연산과 요청별 intent 목록.
//! 창 조회·workspace 변경·이벤트 큐 등 필요한 연산은 IpcWindow로 제공한다.
//! 구조 변경은 도메인 포트 CascadeWindow를 함께 사용한다.
//!
//! 핸들러는 IntentOutbox에 intent를 넣고 진입점이 창 큐 끝으로 옮긴다.
//! 기존 큐 뒤에 요청 순서대로 추가하며, 진입 검사의 intent가 핸들러보다 먼저 들어간다.
//! 이 순서는 handler/intent_order_tests.rs에서 확인한다.
//!
//! 창 자체를 조작하는 GUI·debug 핸들러는 별도 라우터가 EntryWindow를 통해 호출한다(ADR-0002).

use std::path::PathBuf;

use crate::core::CoreState;
use crate::core::cascade_window::CascadeWindow;
use crate::intent::DispatchedIntent;

/// 엔진 핸들러에 필요한 창 연산. state::ipc_window가 구현한다.
pub(crate) trait IpcWindow: CascadeWindow {
    /// 이 창의 활성 workspace 인덱스. 대상 생략 호환 경로나 응답의 활성 표시에서 쓴다.
    /// 명시 대상이 있는 요청은 그 대상의 소속을 우선한다(ADR-0017).
    fn active_workspace_index(&self) -> usize;

    /// 새 워크스페이스의 cwd 상속 원본 — 설정(`inherit_cwd`)과 이 창의 포커스 surface 를 본다.
    fn resolve_inherit_cwd(&self, engine: &CoreState) -> Option<PathBuf>;

    /// workspace 생성 후 이벤트와 활성 선택 조건을 처리한다.
    fn cascade_workspace_created(
        &mut self,
        engine: &mut CoreState,
        origin: &crate::intent::IntentOrigin,
        window_id: u64,
        created: crate::app::dispatch_domain::WorkspaceCreatedCascade,
    );

    /// workspace 이름 등 메타데이터 변경을 알린다.
    fn cascade_workspace_meta_updated(
        &mut self,
        workspace_id: u32,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
    );

    /// 워크스페이스 하나를 닫고 창 쪽 자원과 활성 포인터를 정리한다. 닫았으면 `true`.
    fn close_workspace_at(
        &mut self,
        engine: &mut CoreState,
        ws_idx: usize,
        origin: crate::state::WorkspaceCloseOrigin,
    ) -> bool;

    /// 워크스페이스 순서 이동 뒤 활성 포인터를 따라 옮긴다.
    fn fix_workspace_pointers_after_move(&mut self, from: usize, to: usize);

    /// 호스트 이벤트(plugin event bus · hook 대기 task)를 이 창의 큐에 넣는다.
    fn push_host_event(&mut self, event: crate::state::PendingHostEvent);

    /// 이 창이 기억하는 최근 파일(최신순).
    fn recent_files(&self, kind: &str) -> Vec<String>;

    /// preset 을 이 창에 적용한다. 워크스페이스 preset 은 이 창의 워크스페이스 목록에 붙는다.
    fn apply_preset(
        &mut self,
        core: &crate::core::Core,
        engine: &mut CoreState,
        target: crate::intent::preset::PresetApplyTarget,
        options: crate::state::preset_apply::ApplyOptions,
    ) -> Result<crate::intent::preset::ApplyOutcome, crate::intent::preset::PresetMutationError>;

    /// 발행된 capability 승인 요청을 이 창의 승인 팝업 큐에 넣는다.
    #[cfg(feature = "gui")]
    fn enqueue_approval_popup(
        &mut self,
        engine: &mut CoreState,
        record: &tasty_approval::ApprovalRecord,
    );

    /// 호출 플러그인이 소유한 열린 팝업이 사용자의 확정 입력을 받았는지 확인한다.
    #[cfg(feature = "gui")]
    fn plugin_popup_user_activated(&self, plugin_id: &str, instance_id: u64) -> bool;

    /// 호출 플러그인의 webview에서 기록한 사용자 URL인지 확인하고 기록을 소비한다.
    /// 같은 navigation은 한 번만 사용자 요청 근거로 사용할 수 있다(ADR-0031).
    #[cfg(feature = "gui")]
    fn take_webview_user_navigation(&mut self, plugin_id: &str, surface_id: u32, url: &str)
    -> bool;

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
