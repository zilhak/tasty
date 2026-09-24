//! GUI 없이 조회할 수 있는 전체화면 화면 ID와 제목 키.
//! GUI의 StageDef는 이 메타데이터를 참조하며 두 목록의 누락은 별도 정합 검사가 확인한다.

pub type StageId = &'static str;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageMeta {
    pub id: StageId,
    /// 번역 결과가 아닌 키를 보관해 조회 결과가 현재 언어에 따라 달라지지 않게 한다.
    pub title_key: &'static str,
}

pub const NOTIFICATIONS_STAGE_ID: StageId = "notifications";

/// 화면 교체를 검사할 별도 ID.
#[cfg(test)]
pub(crate) const TEST_STAGE_ID: StageId = "__test_second";

/// 같은 알림 내용에 다른 ID를 붙이는 검사 항목.
#[cfg(test)]
pub(crate) const TEST_TWIN_STAGE_ID: StageId = "__test_notifications_twin";

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

#[cfg(any(feature = "gui", test))]
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
        assert!(before >= 2, "비교할 화면 메타데이터가 2개 이상 필요하다");
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
