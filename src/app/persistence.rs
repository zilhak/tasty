//! 레이아웃 변경을 타이머·종료·창 닫기 시점에 저장한다.

use crate::app::App;
use crate::core::intent::DomainIntent;

impl App {
    /// 창과 parked engine을 저장한다. debounce는 호출자가 처리한다.
    /// force이면 내용 복원 설정에 따라 dirty가 없어도 저장할 수 있다.
    /// 저장 가능 여부와 dirty 해제는 Core가 판단한다.
    pub(crate) fn flush_layout_persistence(&mut self, force: bool) {
        let label = if force { "final" } else { "tick" };
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                Self::flush_one_engine(
                    &mut self.core,
                    &mut main.core_state,
                    main.state.active_workspace,
                    force,
                    label,
                    "main",
                );
            }
        }
        for (state, engine) in self.parked_states.iter_mut() {
            Self::flush_one_engine(
                &mut self.core,
                engine,
                state.active_workspace,
                force,
                label,
                "parked",
            );
        }
    }

    /// engine을 버리기 전에 저장하거나 슬롯 파일을 지운다.
    /// 슬롯 점유는 살아 있는 engine에서 계산하므로 별도 해제는 필요 없다.
    /// 타이머를 기다릴 수 없으므로 force로 마지막 변경과 복원할 내용을 저장한다.
    /// 참조가 사라진 scrollback 파일은 다음 부팅의 전체 슬롯 GC가 회수한다.
    pub(crate) fn retire_main_engine(
        core: &mut crate::core::Core,
        engine: &mut crate::core::CoreState,
        active_workspace: usize,
    ) {
        match retire_action(engine.settings.general.restore_layout) {
            RetireAction::Flush => {
                Self::flush_one_engine(core, engine, active_workspace, true, "retire", "main");
            }
            RetireAction::Delete => {
                if let Some(slot) = engine.layout_slot {
                    crate::core::layout_persistence::delete_slot(slot);
                }
            }
        }
    }

    /// 저장 가능한 engine 중 가장 이른 변경 시각. 없으면 저장 타이머를 해제한다.
    /// 저장을 꺼도 dirty는 남겨 두어 다시 켤 때 그동안의 변경을 저장한다.
    pub(crate) fn earliest_layout_dirty_since(&self) -> Option<std::time::Instant> {
        self.view
            .views
            .values()
            .filter_map(|w| w.as_main())
            .map(|m| &m.core_state)
            .chain(self.parked_states.iter().map(|(_, e)| e))
            .filter_map(|e| {
                schedulable_dirty_since(
                    e.settings.general.restore_layout,
                    e.layout_slot.is_some(),
                    e.layout_slot_protected,
                    e.layout_dirty.dirty_since(),
                )
            })
            .min()
    }

    fn flush_one_engine(
        core: &mut crate::core::Core,
        engine: &mut crate::core::CoreState,
        active_workspace: usize,
        force: bool,
        label: &str,
        kind: &str,
    ) {
        let intent = DomainIntent::SaveLayoutNow {
            active_workspace,
            force,
        };
        if let Err(e) = core.apply(engine, intent) {
            tracing::error!("SaveLayoutNow({label}) failed ({kind}): {e}");
        }
    }
}

/// 창을 닫을 때 슬롯 파일을 보존할지 지울지 정한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetireAction {
    /// 다음 창이 같은 슬롯을 사용하면 닫기 전 레이아웃을 복원할 수 있다.
    Flush,
    Delete,
}

pub(crate) fn retire_action(restore_layout: bool) -> RetireAction {
    if restore_layout {
        RetireAction::Flush
    } else {
        RetireAction::Delete
    }
}

#[cfg(test)]
mod tests {
    use super::{RetireAction, retire_action};

    #[test]
    fn retire_action_follows_restore_layout_setting() {
        assert_eq!(retire_action(true), RetireAction::Flush);
        assert_eq!(retire_action(false), RetireAction::Delete);
    }
}

/// 저장을 건너뛰면서 dirty를 남기는 engine에는 저장 타이머를 예약하지 않는다.
/// apply_save_layout_now의 저장 꺼짐·슬롯 없음·보호된 슬롯 조건과 맞춰야 한다.
pub(crate) fn schedulable_dirty_since(
    restore_layout: bool,
    has_slot: bool,
    slot_protected: bool,
    dirty_since: Option<std::time::Instant>,
) -> Option<std::time::Instant> {
    if restore_layout && has_slot && !slot_protected {
        dirty_since
    } else {
        None
    }
}

#[cfg(test)]
mod schedulable_dirty_tests {
    use super::schedulable_dirty_since;
    use std::time::Instant;

    #[test]
    fn a_dirty_engine_that_will_never_save_is_not_schedulable() {
        let t = Instant::now();
        assert_eq!(
            schedulable_dirty_since(false, true, false, Some(t)),
            None,
            "저장 꺼짐"
        );
        assert_eq!(
            schedulable_dirty_since(true, false, false, Some(t)),
            None,
            "슬롯 없음"
        );
        assert_eq!(schedulable_dirty_since(false, false, false, Some(t)), None);
    }

    #[test]
    fn a_dirty_engine_whose_slot_is_locked_is_not_schedulable() {
        let t = Instant::now();
        assert_eq!(
            schedulable_dirty_since(true, true, true, Some(t)),
            None,
            "보호된 슬롯은 저장 예약에서 제외해야 한다"
        );
    }

    #[test]
    fn a_dirty_engine_that_will_save_keeps_its_deadline() {
        let t = Instant::now();
        assert_eq!(schedulable_dirty_since(true, true, false, Some(t)), Some(t));
    }

    #[test]
    fn a_clean_engine_is_never_schedulable() {
        assert_eq!(schedulable_dirty_since(true, true, false, None), None);
        assert_eq!(schedulable_dirty_since(false, true, false, None), None);
    }
}
