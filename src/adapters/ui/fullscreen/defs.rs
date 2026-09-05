//! 모든 무대의 [`StageDef`] 목록. 새 무대를 추가하려면 이 파일에 한 항목만 추가한다.
//!
//! popup 의 `popup::defs::all_defs()` 와 같은 프로세스 수명 정적 테이블이다 —
//! **여기 선언되지 않은 것은 무대에 올라갈 수 없다.**

use std::sync::OnceLock;

use super::{StageAction, StageDef};
use crate::fullscreen_stages::StageMeta;

/// id 로 메타를 집는다. 없는 id 는 **선언 시점에** 죽는다 — 무대는 프로세스 수명 정적
/// 테이블이라 나중에 조용히 빠지는 것보다 여기서 터지는 편이 낫다.
fn meta(id: &str) -> &'static StageMeta {
    crate::fullscreen_stages::find(id).unwrap_or_else(|| {
        panic!("무대 메타에 '{id}' 가 없다 — src/fullscreen_stages.rs 에 먼저 올려라")
    })
}

/// 프로세스 수명 내내 살아있는 정적 무대 정의 목록.
pub fn all_defs() -> &'static [StageDef] {
    static DEFS: OnceLock<Vec<StageDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        #[allow(unused_mut)] // reason: 테스트 빌드에서만 push 한다.
        let mut defs = vec![
            StageDef {
                meta: meta("blank"),
                draw_fn: draw_blank_stage,
                // 상태 미보유 — 정리할 것이 없다.
                on_close: None,
            },
            StageDef {
                meta: meta(super::notifications::NOTIFICATIONS_STAGE_ID),
                draw_fn: super::notifications::draw,
                // 자체 상태(목록 스크롤 위치)를 무대 종료 시 지운다 — 근거는
                // `super::notifications` 모듈 문서.
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

/// id 로 무대 정의를 찾는다. 정의에 없는 id 는 `None` — 무대에 못 올라간다.
pub fn find(id: &str) -> Option<&'static StageDef> {
    all_defs().iter().find(|d| d.id() == id)
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
pub(crate) use crate::fullscreen_stages::TEST_STAGE_ID;

/// [`TEST_STAGE_ID`] 무대의 `on_close` 발화 횟수. 이 무대를 닫는 테스트가 하나뿐이라
/// 병렬 실행에서도 델타가 결정적이다.
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

/// 테스트 전용 세 번째 무대 — **알림 무대와 같은 콘텐츠를 다른 id 로** 올린다.
///
/// 무대 콘텐츠 Ui 는 셸이 `def.id` 로 salt 한다(`super::draw_fullscreen_stage`). 그
/// salt 가 사라져도 무대와 popup 은 서로 다른 `Area` 라 상태가 안 섞이므로 popup 쪽
/// 비교로는 회귀가 드러나지 않는다 — 같은 콘텐츠를 올린 **두 무대**를 비교해야
/// 드러난다. release 빌드에는 존재하지 않는다.
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

    /// 표를 둘로 가른 대가. **메타에는 있는데 정의가 없는 무대**는 조회에는 나오는데
    /// 열리지 않는다 — 반대 방향(`정의는 있는데 메타가 없다`)은 `meta()` 가 선언
    /// 시점에 죽여서 여기까지 오지 않는다.
    #[test]
    fn every_meta_has_a_definition() {
        let metas: BTreeSet<&str> = crate::fullscreen_stages::all_metas()
            .iter()
            .map(|m| m.id)
            .collect();
        let defs: BTreeSet<&str> = all_defs().iter().map(|d| d.id()).collect();
        assert!(metas.len() >= 2, "메타 표가 비었다 — 0 은 통과가 아니다");
        let orphan: Vec<&&str> = metas.difference(&defs).collect();
        assert!(
            orphan.is_empty(),
            "메타에는 있는데 무대 정의가 없다 — 조회에는 나오는데 열리지 않는다: {orphan:?}"
        );
        assert_eq!(metas, defs, "두 표의 id 집합이 다르다");
    }
}
