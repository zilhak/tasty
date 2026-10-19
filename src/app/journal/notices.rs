//! 복원·저장 중 사용자에게 알릴 사실. App이 알림 항목과 창 오른쪽 위의 비포커스 안내로 옮긴다.
//! 일부 자료만 잃은 경우는 경고, 복원 자료 전체를 읽지 못했거나 저장이 멈춘 경우는 오류와 안내다.
use crate::notification::NotificationLevel;
use crate::runtime::engine_session::EngineId;

/// 비포커스 안내의 종류. 디자인의 restore(복원 자료를 읽지 못함)와 같은 틀을 쓰는 실행 중
/// halt(저장 실패로 변경 적용을 멈춤)다. halt는 입력도 받지 않으므로 재시작이 필요하다고 적는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoticeVariant {
    Restore,
    Halt,
}

/// 안내 첫 줄의 원인 문구.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoticeCause {
    ReadDenied,
    Corrupt,
    /// 읽을 수는 있지만 열리지 않았고 손상인지 알 수 없다(쓰기 거절 등). 자세한 오류는 본문에 있다.
    /// 임시 저장소로 물러나는 GUI 부팅에서만 만든다.
    #[cfg(feature = "gui")]
    OpenFailed,
    SaveStopped,
}

impl NoticeCause {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::ReadDenied => crate::i18n::t("notice.cause_read_denied"),
            Self::Corrupt => crate::i18n::t("notice.cause_corrupt"),
            #[cfg(feature = "gui")]
            Self::OpenFailed => crate::i18n::t("notice.cause_open_failed"),
            Self::SaveStopped => crate::i18n::t("notice.cause_save_stopped"),
        }
    }
}

/// 창 오른쪽 위에 띄울 안내. `path`는 원본 위치로, 사용자가 선택하거나 복사할 수 있다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoticePopup {
    pub(crate) variant: NoticeVariant,
    pub(crate) cause: NoticeCause,
    pub(crate) path: String,
}

impl NoticePopup {
    pub(crate) fn title(&self) -> &'static str {
        match self.variant {
            NoticeVariant::Restore => crate::i18n::t("notice.restore_failed_title"),
            NoticeVariant::Halt => crate::i18n::t("notice.runtime_halted_title"),
        }
    }

    #[cfg(feature = "gui")]
    pub(crate) fn label(&self) -> &'static str {
        crate::i18n::t("notice.saved_layout_label")
    }

    pub(crate) fn body(&self) -> &'static str {
        match self.variant {
            NoticeVariant::Restore => crate::i18n::t("notice.restore_failed_body"),
            NoticeVariant::Halt => crate::i18n::t("notice.runtime_halted_body"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoveryNotice {
    /// 알림을 받을 엔진. 없으면 첫 창의 엔진이 받는다.
    pub(crate) engine: Option<EngineId>,
    pub(crate) level: NotificationLevel,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) popup: Option<NoticePopup>,
}

impl RecoveryNotice {
    pub(crate) fn warning(engine: Option<EngineId>, title: String, body: String) -> Self {
        Self {
            engine,
            level: NotificationLevel::Warning,
            title,
            body,
            popup: None,
        }
    }

    /// 오류 알림과 비포커스 안내. 알림 본문은 안내와 같은 원인과 원본 위치를 담는다.
    pub(crate) fn critical(engine: Option<EngineId>, popup: NoticePopup, detail: &str) -> Self {
        let mut body = format!("{}\n{}\n{}", popup.cause.text(), popup.body(), popup.path);
        if !detail.is_empty() {
            body.push('\n');
            body.push_str(detail);
        }
        Self {
            engine,
            level: NotificationLevel::Error,
            title: popup.title().to_owned(),
            body,
            popup: Some(popup),
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
        if existing.popup.is_none() {
            existing.popup = notice.popup;
        }
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
