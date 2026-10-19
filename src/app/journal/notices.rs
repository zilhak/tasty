//! 복원·저장 중 사용자에게 알릴 사실. App이 알림 항목으로 옮긴다.
//! 일부 자료만 잃은 경우는 경고, 복원 자료 전체를 읽지 못했거나 저장이 멈춘 경우는 오류다.
use crate::notification::NotificationLevel;
use crate::runtime::engine_session::EngineId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoveryNotice {
    /// 알림을 받을 엔진. 없으면 첫 창의 엔진이 받는다.
    pub(crate) engine: Option<EngineId>,
    pub(crate) level: NotificationLevel,
    pub(crate) title: String,
    pub(crate) body: String,
}

impl RecoveryNotice {
    pub(crate) fn warning(engine: Option<EngineId>, title: String, body: String) -> Self {
        Self {
            engine,
            level: NotificationLevel::Warning,
            title,
            body,
        }
    }
}

/// 한 번에 전달하는 같은 엔진·같은 레벨의 알림을 하나로 묶는다. 알림 저장소가 짧은 간격의 같은
/// 레벨 항목을 합치며 제목을 덮어쓰므로, 묶을 때는 각 제목을 본문 줄에 남긴다.
pub(crate) fn merge_same_level(notices: Vec<RecoveryNotice>) -> Vec<RecoveryNotice> {
    let mut merged: Vec<RecoveryNotice> = Vec::new();
    for notice in notices {
        let Some(existing) = merged
            .iter_mut()
            .find(|item| item.engine == notice.engine && item.level == notice.level)
        else {
            merged.push(notice);
            continue;
        };
        let multiple = crate::i18n::t("recovery.multiple_title");
        if existing.title != multiple {
            existing.body = format!("{}: {}", existing.title, existing.body);
            existing.title = multiple.to_owned();
        }
        existing.body = format!("{}\n{}: {}", existing.body, notice.title, notice.body);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_of_one_level_merge_and_keep_every_title() {
        let merged = merge_same_level(vec![
            RecoveryNotice::warning(None, "A".into(), "a".into()),
            RecoveryNotice::warning(None, "B".into(), "b".into()),
            RecoveryNotice::warning(None, "D".into(), "d".into()),
            RecoveryNotice {
                level: NotificationLevel::Error,
                ..RecoveryNotice::warning(None, "C".into(), "c".into())
            },
        ]);
        assert_eq!(merged.len(), 2);
        assert!(merged[0].body.contains("A: a"), "{}", merged[0].body);
        assert!(merged[0].body.contains("B: b"), "{}", merged[0].body);
        assert_eq!(
            merged[0].body.matches(": ").count(),
            3,
            "{}",
            merged[0].body
        );
        assert_eq!(merged[1].title, "C");
    }
}
