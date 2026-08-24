//! plugin popup / banner 렌더 중 수집된 close 사유 forward.

use crate::app::App;
use crate::state::{AppState, FilePickerResult};

/// 닫히는 plugin popup 이 부모인 host `file_picker`(자식)를 함께 정리한다(ADR-0082).
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
        // 우리가 정리를 마쳤으니 부모-자식 관계는 여기서 끝난다. 링크를 남겨두면 다음
        // tick 의 result push 가 "소유 popup 이 사라진 채 결과가 도착했다"(ADR-0082
        // Decision 4)로 오인해 경고를 낸다 — 그 경고는 연쇄 정리가 **실패**했을 때만
        // 나와야 진단 가치가 있다. plugin_id/request_id 는 그대로라 결과는 평소 경로로
        // 정확히 한 번 간다.
        //
        // 사용자가 이미 확정한 결과가 있던 경우(위 분기를 타지 않은 경우)에는 링크를
        // 끊지 않는다 — 고른 파일을 받을 popup 이 사라진 것이라 경고가 진짜 신호다.
        if let Some(req) = data.requester.as_mut() {
            req.owner_popup_instance = None;
        }
    }
}

impl App {
    /// popup instance 하나의 close 를 **렌더가 수집하는 것과 같은 큐**에 넣는다.
    ///
    /// plugin 이 `popup.close` 로 자기 popup 을 닫는 경로와 debug 강제 close 는 원래
    /// `PluginManager::close_popup_instance` 를 직접 쳤다. 그러면 아래 drain 이 태우는
    /// `cancel_child_file_picker` 연쇄 정리를 건너뛰어, 자식 host `file_picker` 가
    /// 부모 없이 떠 있는 고아가 된다 — ADR-0082 Decision 3 이 "부모가 어떤 경로로
    /// 닫히든" 이라고 못박은 계약이 그 경로에서만 깨졌다. 큐로 합류시켜 close 처리
    /// 초크포인트를 하나로 유지한다.
    ///
    /// 큐는 AppState(=window) 별이고 자식 피커도 `AppState.dialogs` 에 있는데, popup
    /// instance 자체는 매니저(전역) 소유라 어느 window 가 그 자식을 들고 있는지 여기서
    /// 알 수 없다. 그래서 **모든** state 에 넣는다 — 매니저 close 는 drain 이
    /// instance_id 로 dedup 하고, `cancel_child_file_picker` 는 그 instance 를 부모로
    /// 신고한 피커에만 반응하므로 중복 push 는 무해하다.
    pub(crate) fn enqueue_plugin_popup_close(
        &mut self,
        instance_id: u64,
        reason: tasty_plugin_protocol::PopupCloseReason,
    ) {
        let mut queued = false;
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                main.state.plugin_popup_closes.push((instance_id, reason));
                queued = true;
            }
        }
        for (s, _engine) in &mut self.parked_states {
            s.plugin_popup_closes.push((instance_id, reason));
            queued = true;
        }
        // 큐를 가진 state 가 하나도 없으면(윈도우 전멸 등) drain 도 돌지 않으므로
        // close 가 통째로 유실된다. 그 경우에만 매니저를 직접 친다 — 정리할 자식
        // 피커도 함께 사라진 상황이라 연쇄 정리 대상이 없다.
        if !queued && let Some(mgr) = self.plugin_manager.as_mut() {
            mgr.close_popup_instance(instance_id, reason);
        }
    }

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
