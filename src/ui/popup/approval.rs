//! 휴먼 핸드오프 — approval popup.
//!
//! `state.dialogs.pending_approval_ids` 큐의 head 를 보여주고, 선택지 버튼/단축키
//! 로 응답한다. 응답은 `ApprovalStore::respond(Responder::User)` 로 들어가며
//! 영속 + waiter 깨우기는 store 가 알아서 한다. 큐가 비면 popup 이 닫힌다.

use tasty_approval::{ApprovalRecord, Responder, Severity};

use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::{CONTENT_MARGIN, PopupAction, TITLE_BAR_HEIGHT};

pub const APPROVAL_POPUP_ID: &str = "approval";

const DEFAULT_WIDTH: f32 = 480.0;
const MIN_HEIGHT: f32 = 180.0;
const MAX_HEIGHT: f32 = 480.0;
const BODY_FONT_SIZE: f32 = 13.0;

/// PopupDef.title_fn — 큐 head 의 title 을 popup 타이틀로 사용.
pub fn approval_popup_title(state: &AppState, engine: &crate::engine_state::CoreState) -> String {
    let Some(id) = state.dialogs.pending_approval_ids.front() else {
        return t("approval.popup.title").to_string();
    };
    engine
        .approval_store
        .get(id)
        .map(|r| r.request.title)
        .unwrap_or_else(|| t("approval.popup.title").to_string())
}

/// PopupDef.sizer — body 길이 + 선택지 수에 따라 height 추정.
pub fn approval_popup_sizer(
    state: &AppState,
    engine: &crate::engine_state::CoreState,
) -> egui::Vec2 {
    let Some(id) = state.dialogs.pending_approval_ids.front() else {
        return egui::vec2(DEFAULT_WIDTH, MIN_HEIGHT);
    };
    let record = engine.approval_store.get(id);
    let body_len = record
        .as_ref()
        .and_then(|r| r.request.body.as_deref())
        .map(|s| s.chars().count())
        .unwrap_or(0);
    let choice_count = record
        .as_ref()
        .map(|r| r.request.choices.len())
        .unwrap_or(2);
    let approx_lines = (body_len as f32 / 60.0).ceil().max(1.0);
    let body_h = approx_lines * BODY_FONT_SIZE * 1.5;
    let buttons_h = (choice_count as f32 / 3.0).ceil() * 32.0;
    let total_h = (TITLE_BAR_HEIGHT + CONTENT_MARGIN * 2.0 + body_h + 32.0 + buttons_h + 56.0)
        .clamp(MIN_HEIGHT, MAX_HEIGHT);
    egui::vec2(DEFAULT_WIDTH, total_h)
}

/// PopupDef.draw_fn — 큐 head 의 record 를 렌더링하고 선택지 클릭/숫자 키로 응답.
pub fn draw_approval_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
) -> PopupAction {
    let th = theme::theme();
    let ctx = ui.ctx().clone();

    let Some(current_id) = state.dialogs.pending_approval_ids.front().cloned() else {
        return PopupAction::Close;
    };

    let Some(record) = engine.approval_store.get(&current_id) else {
        // record 가 사라졌으면 큐에서도 제거하고 다음.
        state.dialogs.pending_approval_ids.pop_front();
        state.dialogs.approval_comment_buffer.clear();
        if state.dialogs.pending_approval_ids.is_empty() {
            return PopupAction::Close;
        }
        return PopupAction::None;
    };

    // 이미 종료된 record 면 자동 정리.
    if record.state.is_terminal() {
        state.dialogs.pending_approval_ids.pop_front();
        state.dialogs.approval_comment_buffer.clear();
        if state.dialogs.pending_approval_ids.is_empty() {
            return PopupAction::Close;
        }
        return PopupAction::None;
    }

    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(8.0, 4.0));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    // Severity 배지.
    let (sev_label_key, sev_color) = match record.request.severity {
        Severity::Info => ("approval.severity.info", th.subtext0.to_egui()),
        Severity::Warn => ("approval.severity.warn", th.yellow.to_egui()),
        Severity::Danger => ("approval.severity.danger", th.red.to_egui()),
    };
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(sev_label_key))
                .size(11.0)
                .color(sev_color),
        );
        ui.label(
            egui::RichText::new(format!("· {}", record.request.id))
                .size(11.0)
                .color(th.subtext0.to_egui()),
        );
    });
    ui.add_space(6.0);

    // Body.
    if let Some(body) = &record.request.body {
        ui.label(
            egui::RichText::new(body)
                .color(th.text.to_egui())
                .size(BODY_FONT_SIZE),
        );
        ui.add_space(8.0);
    }

    // Comment 입력.
    ui.label(
        egui::RichText::new(t("approval.popup.comment_label"))
            .size(11.0)
            .color(th.subtext0.to_egui()),
    );
    ui.add(
        egui::TextEdit::singleline(&mut state.dialogs.approval_comment_buffer)
            .desired_width(ui.available_width())
            .hint_text(t("approval.popup.comment_hint")),
    );
    ui.add_space(8.0);

    // 선택지 버튼. 1..=9 숫자 키로도 응답 가능.
    let mut chosen_key: Option<String> = None;
    ui.horizontal_wrapped(|ui| {
        for (idx, choice) in record.request.choices.iter().enumerate() {
            let label_text = if idx < 9 {
                format!("{} ({})", choice.label, idx + 1)
            } else {
                choice.label.clone()
            };
            let mut btn = egui::Button::new(egui::RichText::new(label_text).size(13.0));
            if choice.destructive {
                btn = btn.fill(th.red.to_egui().linear_multiply(0.18));
            }
            if ui.add(btn).clicked() {
                chosen_key = Some(choice.key.clone());
            }
        }
    });

    // 단축키. Escape 는 명시적으로 막아 둔다 — 사용자가 실수로 popup 을 닫고
    // 응답을 우회하면 워크플로우가 비정상 종료된다. 숫자 키만 허용.
    let pressed_num = ctx.input(|i| {
        for n in 1..=9u8 {
            let key = match n {
                1 => egui::Key::Num1,
                2 => egui::Key::Num2,
                3 => egui::Key::Num3,
                4 => egui::Key::Num4,
                5 => egui::Key::Num5,
                6 => egui::Key::Num6,
                7 => egui::Key::Num7,
                8 => egui::Key::Num8,
                9 => egui::Key::Num9,
                _ => continue,
            };
            if i.key_pressed(key) {
                return Some(n as usize);
            }
        }
        None
    });
    if let Some(idx) = pressed_num
        && let Some(choice) = record.request.choices.get(idx - 1)
    {
        chosen_key = Some(choice.key.clone());
    }

    if let Some(choice_key) = chosen_key {
        let comment = if state.dialogs.approval_comment_buffer.trim().is_empty() {
            None
        } else {
            Some(state.dialogs.approval_comment_buffer.trim().to_string())
        };
        let store = engine.approval_store.clone();
        match store.respond(&current_id, choice_key, Responder::User, comment) {
            Ok(change) => {
                persist_after_respond(state, &change.record);
                state.dialogs.pending_approval_ids.pop_front();
                state.dialogs.approval_comment_buffer.clear();
                if state.dialogs.pending_approval_ids.is_empty() {
                    return PopupAction::Close;
                }
            }
            Err(e) => {
                tracing::warn!("approval popup respond failed: {e}");
            }
        }
    }

    PopupAction::None
}

/// 응답이 store 에 반영된 직후, IPC 핸들러와 같은 영속 경로를 호출한다.
/// (도메인 layer 는 영속을 호스트에게 위임하므로 직접 호출해야 한다.)
/// `state.memory` 의 Arc clone (Core 와 같은 allocation) 으로 영속한다 — UI
/// thread 가 dispatcher cascade 없이 단발 호출.
fn persist_after_respond(state: &AppState, record: &ApprovalRecord) {
    use tasty_memory::{MemoryValue, PutOpts, Scope};
    let scope = match record.request.workspace_id {
        Some(wid) => Scope::Workspace(wid),
        None => Scope::Global,
    };
    let key = format!("tasty.approval.{}", record.request.id);
    let value = match serde_json::to_value(record) {
        Ok(v) => MemoryValue::Json(v),
        Err(e) => {
            tracing::warn!("approval popup: serialize failed: {e}");
            return;
        }
    };
    let opts = PutOpts {
        expires_at: None,
        cas: None,
    };
    let mut guard = match state.memory.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Err(e) = guard.put(tasty_memory::HOST_OWNER, &scope, &key, &value, &opts) {
        tracing::warn!("approval popup: memory put failed: {e}");
    }
}

/// 새 approval 이 생성되면 호출. 큐에 push 하고 popup 이 닫혀 있으면 연다.
/// danger severity 는 priority 가 높지만 현재 PopupManager 는 priority API 가
/// 없으므로 동일 popup_id 로 처리 — 추후 popup-implementation 확장 시 분기.
pub fn enqueue_approval(
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    record: &ApprovalRecord,
) {
    // 같은 id 가 이미 큐에 있으면 중복 push 회피.
    if state
        .dialogs
        .pending_approval_ids
        .iter()
        .any(|id| id == &record.request.id)
    {
        return;
    }
    state
        .dialogs
        .pending_approval_ids
        .push_back(record.request.id.clone());
    // 첫 항목이면 popup 을 즉시 연다. 이미 열려 있으면 Intent dedup 으로 무시.
    // approval 큐는 agent/plugin 발화이므로 origin 은 agent_ipc 로 통일.
    state.dispatch_intent(
        crate::intent::Intent::OpenPopup {
            id: APPROVAL_POPUP_ID,
            mode: crate::intent::OpenPopupMode::WithScope(crate::ui::popup::PopupScope::Window),
        }
        .from_agent_ipc(),
    );

    // 알림 채널 동시 발화.
    let severity_prefix = match record.request.severity {
        Severity::Info => "",
        Severity::Warn => "[WARN] ",
        Severity::Danger => "[DANGER] ",
    };
    let body = record
        .request
        .body
        .clone()
        .unwrap_or_else(|| t("approval.notification.body_fallback").to_string());
    let workspace = record.request.workspace_id.unwrap_or(0);
    let surface = record.request.surface_id.unwrap_or(0);
    let _ = engine.notifications.add(
        workspace,
        surface,
        format!("{severity_prefix}{}", record.request.title),
        body,
    );
}
