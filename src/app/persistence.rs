//! Layout persistence flush — debounce 만료 시 + shutdown 시 + 창 은퇴 시.

use crate::app::App;
use crate::core::intent::DomainIntent;

impl App {
    /// Flush layout persistence — main + parked engine 마다 독립 발화.
    ///
    /// `force=false`: `Tick::LayoutFlush` 타이머 발화. debounce 판정은 타이머가
    ///   이미 했으므로 여기선 settings.restore_layout + dirty 여부만 본다.
    /// `force=true`: shutdown / quit modal. debounce 무시, `restore_surface_content`
    ///   설정이 켜져 있으면 layout_dirty 가 false 여도 저장.
    ///
    /// 조건 분기 + `layout_dirty.clear()` 는 Core::apply 안에서 처리.
    /// Intent 큐 우회 — *system loop tick / shutdown* 의 부수효과 (D.3.C.D.4 §8.H).
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

    /// 창이 닫혀 engine 이 drop 되기 **직전**에 그 engine 의 슬롯을 마무리한다.
    ///
    /// 슬롯 **점유** 해제는 여기서 하지 않는다 — 점유 집합이 살아있는 engine 의
    /// `layout_slot` 에서 파생되므로(`App::occupied_layout_slots`) engine 이 drop
    /// 되는 순간 자동으로 free 다. 별도 release 호출이 없다는 것은 곧 누락으로
    /// 인한 슬롯 영구 누수가 불가능하다는 뜻이다.
    ///
    /// 이 함수가 하는 일은 [`RetireAction`] 한 가지뿐이다. 보존 분기가 `force=true`
    /// 인 이유는 저장이 debounce 타이머에 매여 있기 때문이다 — `force=false` 면
    /// `Tick::LayoutFlush` 데드라인(500ms)이 와야 쓰므로, 워크스페이스를 추가한
    /// 직후 창을 닫으면 그 변경이 슬롯 파일에 도달하지 못한 채 사라진다. 앱 종료
    /// 경로(`shutdown_machine` 의 전 engine force flush)와 대칭을 맞춰야 "보존" 이
    /// 실제로 성립한다.
    ///
    /// 닫힌 창이 참조하던 scrollback `.bin` 은 따로 지우지 않는다 — 보존 분기에선
    /// 슬롯 파일이 계속 참조하고, 삭제 분기에선 참조가 사라져 다음 부팅의 전 슬롯
    /// union GC 가 회수한다.
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
                // headless engine 처럼 슬롯이 없으면 지울 것도 없다.
                if let Some(slot) = engine.layout_slot {
                    crate::core::layout_persistence::delete_slot(slot);
                }
            }
        }
    }

    /// **flush 로 실제 디스크에 닿을** engine 중 가장 먼저 dirty 가 된 시각.
    /// `None` 이면 이번 프레임에 저장할 것이 없다 — 호스트가 `Tick::LayoutFlush`
    /// 등록을 걷어낸다.
    ///
    /// dirty 여부만 보지 않는다: `apply_save_layout_now` 가 저장을 건너뛰는 조건
    /// (`restore_layout` 꺼짐 / 슬롯 없는 engine)에서는 dirty 가 **영원히** 남으므로,
    /// 그대로 데드라인을 만들면 `dirty_since + debounce` 라는 지난 시각이 매 프레임
    /// 재등록돼 이벤트 루프가 쉬지 못한다. 저장하지 않을 engine 은 애초에 예약
    /// 대상이 아니다.
    ///
    /// dirty 자체는 지우지 않는다 — 사용자가 세션 중에 `restore_layout` 을 다시
    /// 켜면 그때까지 쌓인 변경이 그대로 flush 되어야 한다.
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

    /// `flush_layout_persistence` 의 공용 루프 바디 — main-view engine 과
    /// parked engine 모두 동일 로직(intent 생성 + apply + 실패 warn)이라 통합.
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
            // 이번 세션의 창 구성이 디스크에 남지 않는다 = 다음 부팅에 사용자 작업 소실.
            tracing::error!("SaveLayoutNow({label}) failed ({kind}): {e}");
        }
    }
}

/// 창이 닫힐 때 그 engine 의 슬롯 파일에 할 일.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetireAction {
    /// 슬롯 파일을 최신 상태로 갱신해 **남긴다**(점유만 풀린다). 실수로 닫은 창의
    /// 레이아웃을 새 창이 그 슬롯을 잡으며 되살릴 수 있다.
    Flush,
    /// 슬롯 파일을 **지운다**.
    Delete,
}

/// 은퇴 정책 — 레이아웃 기억 설정 하나로 갈린다. `retire_main_engine` 이
/// `App`/`Core` 에 깊게 얽혀 있어 정책 판정만 순수 함수로 떼어 테스트한다.
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

/// flush 데드라인을 걸어도 되는 dirty 인가.
///
/// 기준은 하나다 — **저장을 건너뛰면서 `layout_dirty` 를 지우지 않는 갈래는 예약 대상이
/// 아니다.** 예약하면 그 dirty 가 영원히 안 풀려 `dirty_since + debounce` 라는 **지난
/// 시각**이 매 프레임 재등록되고 이벤트 루프가 쉬지 못한다(`docs/dev-guide/timer-hub.md`).
///
/// `crate::core::impl_workspace::apply_save_layout_now`(force=false) 가 그런 갈래를
/// **셋** 갖는다. 셋 다 `layout_dirty.clear()` **앞에서** 반환한다:
///
/// ```text
///   restore_layout 이 꺼짐        → should_save=false 로 반환
///   layout_slot 이 None           → let-else 로 반환 (headless engine)
///   layout_slot_protected         → 잠긴 슬롯이라 반환
/// ```
///
/// ★ 셋째가 한때 여기 빠져 있었다. 그때 이 주석은 "같은 판정이다" 라고 적고 있었고
/// **그 문장을 지키는 것이 아무것도 없었다** — 잠긴 슬롯에서 사용자가 레이아웃을 바꾸면
/// 예약은 걸리고 저장은 매번 건너뛰어, 이 주석이 스스로 경고한 스핀이 그대로 났다.
/// 도달 경로도 막혀 있지 않다: 슬롯은 부팅에서 읽기 결과를 알기 전에 claim 되고
/// (`app/boot_machine.rs`), `SlotLoad::Unreadable` 은 `layout_slot` 을 비우지 않으며
/// (`core/state.rs`), 다른 protected 자리는 경고만 한다.
///
/// **같음을 지키는 것은 이 문단이 아니라 아래 단위 테스트다** — 세 갈래마다 하나씩 있고,
/// 저쪽에 갈래가 하나 더 생기면 여기에도 인자가 하나 더 늘어야 한다.
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

    /// **회귀 방지(gate4-22 2차)** — `restore_layout=false` 는 흔한 사용자 설정이고,
    /// 그 상태에서 `apply_save_layout_now` 는 저장을 건너뛰면서 dirty 를 clear 하지
    /// 않는다. 그러니 dirty 가 있어도 예약 대상이 아니다 — 예약하면 스핀한다.
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

    /// **세 번째 갈래** — 슬롯이 잠긴 engine.
    ///
    /// `SlotLoad::Unreadable` 은 디스크의 사용자 레이아웃을 이 세션이 덮어쓰지 않도록
    /// 슬롯을 잠근다. 그러면 `apply_save_layout_now` 는 `layout_dirty.clear()` **앞에서**
    /// 반환하므로 dirty 가 영원히 남는다 — 앞의 두 갈래와 정확히 같은 이유로 예약 대상이
    /// 아니다. 이 갈래가 빠져 있던 동안 그 스핀이 실제로 가능했다.
    #[test]
    fn a_dirty_engine_whose_slot_is_locked_is_not_schedulable() {
        let t = Instant::now();
        assert_eq!(
            schedulable_dirty_since(true, true, true, Some(t)),
            None,
            "잠긴 슬롯은 저장이 항상 건너뛰어지므로 예약하면 스핀한다"
        );
    }

    /// 저장하는 engine 의 dirty 는 그대로 데드라인이 된다.
    #[test]
    fn a_dirty_engine_that_will_save_keeps_its_deadline() {
        let t = Instant::now();
        assert_eq!(schedulable_dirty_since(true, true, false, Some(t)), Some(t));
    }

    /// dirty 가 없으면 어느 설정에서도 예약 없음.
    #[test]
    fn a_clean_engine_is_never_schedulable() {
        assert_eq!(schedulable_dirty_since(true, true, false, None), None);
        assert_eq!(schedulable_dirty_since(false, true, false, None), None);
    }
}
