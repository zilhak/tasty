//! 등록 가능한 전체화면 정의 목록.

use std::sync::OnceLock;

use super::{StageAction, StageDef};
use crate::fullscreen_stages::StageMeta;

/// 없는 메타 ID를 정의에 사용하면 즉시 실패한다.
fn meta(id: &str) -> &'static StageMeta {
    crate::fullscreen_stages::find(id).unwrap_or_else(|| {
        panic!("무대 메타에 '{id}' 가 없다 — src/fullscreen_stages.rs 에 먼저 올려라")
    })
}

pub fn all_defs() -> &'static [StageDef] {
    static DEFS: OnceLock<Vec<StageDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        #[allow(unused_mut)] // reason: 테스트 빌드에서만 push 한다.
        let mut defs = vec![
            StageDef {
                meta: meta("blank"),
                draw_fn: draw_blank_stage,
                on_close: None,
            },
            StageDef {
                meta: meta(super::notifications::NOTIFICATIONS_STAGE_ID),
                draw_fn: super::notifications::draw,
                on_close: Some(super::notifications::on_close),
            },
        ];
        #[cfg(test)]
        defs.push(test_stage_def());
        #[cfg(test)]
        defs.push(test_twin_stage_def());
        defs
    })
}

pub fn find(id: &str) -> Option<&'static StageDef> {
    all_defs().iter().find(|d| d.id() == id)
}

/// 공용 배경과 제목만 확인하는 빈 화면.
fn draw_blank_stage(
    _ui: &mut egui::Ui,
    _state: &mut crate::state::AppState,
    _engine: &mut crate::core::CoreState,
) -> StageAction {
    StageAction::None
}

/// 교체 시 이전 화면의 정리를 확인할 시험용 정의. 제품 빌드에는 포함하지 않는다.
#[cfg(test)]
pub(crate) use crate::fullscreen_stages::TEST_STAGE_ID;

/// 시험용 닫기 훅의 호출 횟수. 이 정의를 닫는 시험은 하나다.
#[cfg(test)]
pub(crate) static TEST_STAGE_CLOSES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
fn test_stage_def() -> StageDef {
    StageDef {
        meta: meta(TEST_STAGE_ID),
        draw_fn: draw_blank_stage,
        on_close: Some(|_ctx, _state, _engine| {
            TEST_STAGE_CLOSES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }),
    }
}

/// 같은 내용을 다른 ID로 그려도 상태가 섞이지 않는지 확인하는 시험용 정의.
#[cfg(test)]
pub(crate) use crate::fullscreen_stages::TEST_TWIN_STAGE_ID;

#[cfg(test)]
fn test_twin_stage_def() -> StageDef {
    StageDef {
        meta: meta(TEST_TWIN_STAGE_ID),
        draw_fn: super::notifications::draw,
        on_close: Some(super::notifications::on_close),
    }
}

#[cfg(test)]
mod parity {
    use super::*;
    use std::collections::BTreeSet;

    // 정의에서 메타 참조는 타입으로 확인되지만 그 반대 방향은 목록을 비교해야 한다.
    #[test]
    fn every_meta_has_a_definition() {
        let metas: BTreeSet<&str> = crate::fullscreen_stages::all_metas()
            .iter()
            .map(|m| m.id)
            .collect();
        let defs: BTreeSet<&str> = all_defs().iter().map(|d| d.id()).collect();
        assert!(metas.len() >= 2, "무대 메타 항목이 최소 2개 있어야 한다");
        let orphan: Vec<&&str> = metas.difference(&defs).collect();
        assert!(
            orphan.is_empty(),
            "메타에는 있는데 무대 정의가 없다 — 조회에는 나오는데 열리지 않는다: {orphan:?}"
        );
        assert_eq!(metas, defs, "두 표의 id 집합이 다르다");
    }
}
