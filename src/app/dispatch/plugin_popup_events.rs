//! plugin popup / banner 렌더 중 수집된 close 사유 forward.

use crate::app::App;
use crate::state::{AppState, FilePickerResult};

/// 닫히는 plugin popup 이 부모인 host `file_picker`(자식)를 함께 정리한다(ADR-0081).
///
/// 그냥 `dialogs.file_picker = None` 으로 지우지 않고 **취소 결과를 채운다** — 그러면
/// 기존 result drain(`app::dispatch::file_picker`)이 평소 경로 그대로 돌아 plugin 에
/// `file_picker.result { cancelled: true }` 를 보낸다. ADR-0058 의 "모든 트리거는
/// 정확히 하나의 결과를 받는다" 계약이 이 정리 경로에서도 깨지지 않는다.
///
/// host 는 plugin id/kind 를 보지 않는다 — 자식이 스스로 기록한 부모 instance_id 만
/// 대조하는 generic 판정이다(핵심 원칙 2).
pub(crate) fn cancel_child_file_picker(
    state: &mut AppState,
    closed: &[(u64, tasty_plugin_protocol::PopupCloseReason)],
) {
    let Some(data) = state.dialogs.file_picker.as_mut() else {
        return;
    };
    let Some(owner) = data.requester.as_ref().and_then(|r| r.owner_popup_instance) else {
        return;
    };
    if !closed.iter().any(|(iid, _)| *iid == owner) {
        return;
    }
    // 이미 사용자가 확정/취소한 뒤라면 그 결과를 존중한다(덮어쓰지 않는다).
    if data.result.is_none() {
        data.result = Some(FilePickerResult::Cancelled);
    }
}

impl App {
    /// plugin popup / banner 렌더 중 감지된 close 사유를 모든 AppState에서 drain해
    /// `PluginManager`로 forward한다. (`close_popup_instance` / `close_banner_instance`)
    pub(crate) fn dispatch_plugin_popup_events(&mut self) {
        let mut drained_closes: Vec<(u64, tasty_plugin_protocol::PopupCloseReason)> = Vec::new();
        let mut drained_banner_closes: Vec<(u64, tasty_plugin_protocol::BannerCloseReason)> =
            Vec::new();
        let mut drained_focus_bumps: Vec<u64> = Vec::new();
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                let closes = std::mem::take(&mut main.state.plugin_popup_closes);
                cancel_child_file_picker(&mut main.state, &closes);
                drained_closes.extend(closes);
                drained_banner_closes.append(&mut main.state.plugin_banner_closes);
                drained_focus_bumps.append(&mut main.state.plugin_popup_focus_bumps);
            }
        }
        for (s, _engine) in &mut self.parked_states {
            let closes = std::mem::take(&mut s.plugin_popup_closes);
            cancel_child_file_picker(s, &closes);
            drained_closes.extend(closes);
            drained_banner_closes.append(&mut s.plugin_banner_closes);
            drained_focus_bumps.append(&mut s.plugin_popup_focus_bumps);
        }
        if drained_closes.is_empty()
            && drained_banner_closes.is_empty()
            && drained_focus_bumps.is_empty()
        {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        // 같은 인스턴스에 대해 close 사유가 여러 번 쌓일 수 있다 (Escape 매 프레임 등).
        // 첫 사유로 close하고 나머지는 무시 — close_popup_instance가 알아서 멱등 처리.
        let mut seen = std::collections::HashSet::new();
        for (instance_id, reason) in drained_closes {
            if seen.insert(instance_id) {
                mgr.close_popup_instance(instance_id, reason);
            }
        }
        // banner close (A3) — host 측 생명주기(TTL/close X)로 닫힌 plugin 배너. 멱등.
        let mut seen_banner = std::collections::HashSet::new();
        for (instance_id, reason) in drained_banner_closes {
            if seen_banner.insert(instance_id) {
                mgr.close_banner_instance(instance_id, reason);
            }
        }
        // z-order 승격(규칙 7 "클릭된 것이 앞") — 같은 instance 가 여러 번 쌓여도
        // touch_popup_instance_z 는 멱등(마지막 호출만 z_seq 를 갱신)이라 dedup 불필요.
        //
        // 서로 다른 instance 가 **한 클릭으로** 함께 쌓이는 일은 없다 — 겹친 popup 중
        // 좌표를 소유한 하나만 bump 를 낸다(`adapters/ui/popup/occlusion.rs`). 그래서
        // 이 순회 순서가 최종 z 순서를 좌우하지 않는다.
        for instance_id in drained_focus_bumps {
            mgr.touch_popup_instance_z(instance_id);
        }
    }
}
