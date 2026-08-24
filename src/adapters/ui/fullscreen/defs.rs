//! 모든 무대의 [`StageDef`] 목록. 새 무대를 추가하려면 이 파일에 한 항목만 추가한다.
//!
//! popup 의 `popup::defs::all_defs()` 와 같은 프로세스 수명 정적 테이블이다 —
//! **여기 선언되지 않은 것은 무대에 올라갈 수 없다.**

use std::sync::OnceLock;

use super::{StageAction, StageDef};

/// 프로세스 수명 내내 살아있는 정적 무대 정의 목록.
pub fn all_defs() -> &'static [StageDef] {
    static DEFS: OnceLock<Vec<StageDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        #[allow(unused_mut)] // reason: 테스트 빌드에서만 push 한다.
        let mut defs = vec![StageDef {
            id: "blank",
            title_key: "fullscreen.blank.title",
            draw_fn: draw_blank_stage,
            // 상태 미보유 — 정리할 것이 없다.
            on_close: None,
        }];
        #[cfg(test)]
        defs.push(test_stage_def());
        defs
    })
}

/// id 로 무대 정의를 찾는다. 정의에 없는 id 는 `None` — 무대에 못 올라간다.
pub fn find(id: &str) -> Option<&'static StageDef> {
    all_defs().iter().find(|d| d.id == id)
}

/// 콘텐츠 없는 기준 무대. 무대 셸(scrim + 제목) 자체를 확인하기 위한 최소 정의이며,
/// 콘텐츠를 가진 무대가 생기면 그 옆에 나란히 등록된다. 셸이 이미 다 그렸으므로
/// 이 함수는 아무것도 그리지 않는다.
fn draw_blank_stage(
    _ui: &mut egui::Ui,
    _state: &mut crate::state::AppState,
    _engine: &mut crate::core::CoreState,
) -> StageAction {
    StageAction::None
}

/// 테스트 전용 두 번째 무대. 무대 **교체**(A 가 정리되고 B 만 남는다) 계약은 정의가 둘
/// 이상일 때만 실제로 걷힌다 — 테이블이 컴파일 타임 고정이라 테스트가 바깥에서 더미
/// 무대를 끼워 넣을 방법이 없어, 여기서 `#[cfg(test)]` 로 하나 더 등록한다. release
/// 빌드에는 존재하지 않는다. `on_close` 가 카운터를 올리므로 훅이 **실제로 발화했는지**
/// 까지 단정할 수 있다.
#[cfg(test)]
pub(crate) const TEST_STAGE_ID: super::StageId = "__test_second";

/// [`TEST_STAGE_ID`] 무대의 `on_close` 발화 횟수. 이 무대를 닫는 테스트가 하나뿐이라
/// 병렬 실행에서도 델타가 결정적이다.
#[cfg(test)]
pub(crate) static TEST_STAGE_CLOSES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
fn test_stage_def() -> StageDef {
    StageDef {
        id: TEST_STAGE_ID,
        title_key: "fullscreen.blank.title",
        draw_fn: draw_blank_stage,
        on_close: Some(|_ctx, _state, _engine| {
            TEST_STAGE_CLOSES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }),
    }
}
