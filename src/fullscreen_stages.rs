//! 전체화면 무대의 **메타**(id · 제목 키). 그리기와 갈라 둔 이유가 이 파일의 전부다.
//!
//! 무대 정의(`adapters::ui::fullscreen::StageDef`)는 `draw_fn` 을 필드로 갖는다 —
//! `fn(&mut egui::Ui, ...)` 이라 **표 자체가 gui 타입**이다. 그래서 "어떤 무대가 있나"
//! 라는 순수한 물음조차 gui 빌드에서만 답할 수 있었고, 헤드리스 데몬은 자기 무대 표를
//! 조회할 수단이 없었다([headless-ipc-surface](../docs/dev-guide/headless-ipc-surface.md)).
//!
//! 함수를 복제하지 않고 **표를 갈랐다** — `cell_palette` 를 게이트 밖으로 올린 것과 같은
//! 처방이다([debug-ipc](../docs/dev-guide/debug-ipc.md) "디버그 코드 격리 정책"). 메타는
//! 여기 살고, 그리기는 `StageDef` 가 이 메타를 **참조**해 얹는다. 두 벌이 아니라 한 벌이다.
//!
//! # 가르면 갈라진 것이 어긋난다
//!
//! 표를 둘로 나누는 순간 새 결함 부류가 둘 생긴다. 셋 다 막는 수단이 다르다:
//!
//! - **정의가 메타와 다른 제목을 갖는다** → 불가능하다. `StageDef` 가 메타를 **소유하지
//!   않고 참조**해서, 어긋날 필드가 애초에 없다(타입이 막는다).
//! - **정의는 있는데 메타가 없다** → 정의 표를 세울 때 `meta()` 가 그 자리에서 죽는다.
//!   프로세스 수명 정적 표라, 나중에 조용히 빠지는 것보다 초기화에서 터지는 편이 낫다.
//! - **메타에는 있는데 정의가 없다** → 여기서는 못 막는다(조회에는 나오는데 열리지
//!   않는다). `fullscreen::defs` 의 parity 테스트가 본다. 셋 중 이것만 테스트의 몫이다.

/// 무대 식별자. 정의에 없는 id 는 무대에 올라갈 수 없다.
pub type StageId = &'static str;

/// 무대의 gui 무관 메타. 여기 있는 필드는 **창이 없어도 답이 정의되는 것**만이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageMeta {
    pub id: StageId,
    /// i18n 키. 값이 아니라 키다 — 값을 노출하면 자동 검증이 로케일에 묶인다.
    pub title_key: &'static str,
}

/// 알림 무대. id 상수가 메타 쪽에 사는 이유는 **메타가 id 의 출처**이기 때문이다 —
/// 그리기 모듈이 이것을 참조하지, 그 반대가 아니다.
pub const NOTIFICATIONS_STAGE_ID: StageId = "notifications";

/// 테스트 전용 두 번째 무대. 무대 **교체** 계약은 정의가 둘 이상일 때만 걷힌다.
#[cfg(test)]
pub(crate) const TEST_STAGE_ID: StageId = "__test_second";

/// 테스트 전용 세 번째 무대 — 알림 무대와 **같은 콘텐츠를 다른 id 로** 올린다.
#[cfg(test)]
pub(crate) const TEST_TWIN_STAGE_ID: StageId = "__test_notifications_twin";

/// 프로세스 수명 내내 살아있는 정적 메타 목록. 새 무대는 여기와 `StageDef` 표 양쪽에
/// 올라가야 하고, 한쪽만 올리면 정합 테스트가 잡는다.
pub fn all_metas() -> &'static [StageMeta] {
    #[cfg(not(test))]
    {
        RELEASE_METAS
    }
    #[cfg(test)]
    {
        static ALL: std::sync::OnceLock<Vec<StageMeta>> = std::sync::OnceLock::new();
        ALL.get_or_init(|| {
            let mut v = RELEASE_METAS.to_vec();
            v.push(StageMeta {
                id: TEST_STAGE_ID,
                title_key: "fullscreen.blank.title",
            });
            v.push(StageMeta {
                id: TEST_TWIN_STAGE_ID,
                title_key: "fullscreen.notifications.title",
            });
            v
        })
    }
}

const RELEASE_METAS: &[StageMeta] = &[
    StageMeta {
        id: "blank",
        title_key: "fullscreen.blank.title",
    },
    StageMeta {
        id: NOTIFICATIONS_STAGE_ID,
        title_key: "fullscreen.notifications.title",
    },
];

/// id 로 메타를 찾는다.
pub fn find(id: &str) -> Option<&'static StageMeta> {
    all_metas().iter().find(|m| m.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_is_unique() {
        let mut ids: Vec<StageId> = all_metas().iter().map(|m| m.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "무대 id 가 중복이다: {ids:?}");
        assert!(before >= 2, "메타 표가 비었다 — 0 은 통과가 아니다");
    }

    #[test]
    fn find_reads_the_table() {
        assert_eq!(
            find(NOTIFICATIONS_STAGE_ID).map(|m| m.id),
            Some(NOTIFICATIONS_STAGE_ID)
        );
        assert!(find("__no_such_stage").is_none());
    }
}
