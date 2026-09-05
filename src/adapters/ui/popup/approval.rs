//! 휴먼 핸드오프 — approval popup.
//!
//! `state.dialogs.pending_approval_ids` 큐의 head 를 보여주고, 선택지 버튼/단축키
//! 로 응답한다. 응답은 `ApprovalStore::respond(Responder::User)` 로 들어가며
//! 영속 + waiter 깨우기는 store 가 알아서 한다. 큐가 비면 popup 이 닫힌다.
//!
//! Tier 3 분리: AppState/CoreState 비의존인 [`draw_approval_view`] + [`ApprovalProps`]
//! 와, 큐/store mutation 을 담당하는 wrapper [`draw_approval_popup`] 로 나누어
//! gallery 에서 단독 시각 검증 가능.

use tasty_approval::{ApprovalRecord, Responder, Severity};
use tasty_type_geometry::length::LogicalPx;

use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::theme::Theme;
use tasty_ui_widgets::vspace;

pub const APPROVAL_POPUP_ID: &str = "approval";

const DEFAULT_WIDTH: LogicalPx = LogicalPx(480.0);
const MIN_HEIGHT: LogicalPx = LogicalPx(180.0);
const MAX_HEIGHT: LogicalPx = LogicalPx(480.0);

/// PopupDef.title_fn — 큐 head 의 title 을 popup 타이틀로 사용.
pub fn approval_popup_title(state: &AppState, engine: &crate::core::CoreState) -> String {
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
pub fn approval_popup_sizer(state: &AppState, engine: &crate::core::CoreState) -> egui::Vec2 {
    let Some(id) = state.dialogs.pending_approval_ids.front() else {
        return egui::vec2(DEFAULT_WIDTH.value(), MIN_HEIGHT.value());
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
    let body_h = approx_lines * theme::theme().font_size_body.value() * 1.5;
    let buttons_h = (choice_count as f32 / 3.0).ceil() * 32.0;
    let total_h = (popup::title_bar_height()
        + popup::content_margin().scaled(2.0)
        + LogicalPx(body_h)
        + LogicalPx(32.0)
        + LogicalPx(buttons_h)
        + LogicalPx(56.0))
    .min(MAX_HEIGHT)
    .max(MIN_HEIGHT);
    egui::vec2(DEFAULT_WIDTH.value(), total_h.value())
}

/// View 입력 — 한 선택지의 시각/의미 데이터.
#[derive(Debug, Clone)]
pub struct ApprovalChoiceView {
    /// 응답 시 store 로 전달되는 식별자.
    pub key: String,
    /// 버튼에 표시되는 라벨.
    pub label: String,
    /// 위험 강조 표시 여부 (빨간 배경).
    pub destructive: bool,
}

/// View 입력 — popup 한 화면 분의 모든 데이터. AppState/CoreState 비의존.
///
/// `comment_buffer` 는 `&mut String` 으로 외부 상태를 그대로 빌려 받는다 — gallery
/// 에서는 로컬 `String` 의 `&mut` 를 주면 된다.
pub struct ApprovalProps<'a> {
    /// 요청 id 를 헤더에 함께 표시.
    pub id: String,
    pub severity: Severity,
    /// 사전 번역된 severity 라벨 (예: "INFO" / "WARN" / "DANGER").
    pub severity_label: String,
    /// 사전 번역된 comment 입력 위 라벨.
    pub comment_label: String,
    /// 사전 번역된 comment 입력 placeholder.
    pub comment_hint: String,
    /// 본문. None 이면 body block 자체를 그리지 않는다.
    pub body: Option<String>,
    /// 선택지 (1 개 이상). 빈 vec 이면 응답 불가능 상태로 그려진다.
    pub choices: Vec<ApprovalChoiceView>,
    /// 코멘트 입력 버퍼. View 가 `TextEdit` 으로 직접 mutate.
    pub comment_buffer: &'a mut String,
}

/// View 의 출력 — 사용자 의도. wrapper 가 store/queue mutation 으로 변환.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalViewAction {
    None,
    /// 사용자가 한 선택지를 마우스 클릭 또는 1..=9 숫자 키로 선택.
    Chosen {
        key: String,
    },
}

/// Pure 시각 view. AppState/CoreState 비의존.
///
/// Escape 는 의도적으로 받지 않는다 — 응답 우회로 워크플로우가 끊기지 않도록.
/// 단축키 1..=9 는 view 내부에서 처리해 mouse 와 동일한 의도로 변환.
pub fn draw_approval_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    props: &mut ApprovalProps<'_>,
) -> ApprovalViewAction {
    let sev_color = match props.severity {
        Severity::Info => theme.text_muted().to_egui(),
        Severity::Warn => theme.accent_warning().to_egui(),
        Severity::Danger => theme.accent_danger().to_egui(),
    };
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(&props.severity_label)
                .size(theme.font_size_caption.value())
                .color(sev_color),
        );
        ui.label(
            egui::RichText::new(format!("· {}", props.id))
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
    });
    // 6→8 스냅 (그리드 정합 — 헤더 블록/본문 섹션 간격).
    vspace(ui, theme.spacing_sm);

    if let Some(body) = &props.body {
        ui.label(
            egui::RichText::new(body)
                .color(theme.text_primary().to_egui())
                .size(theme.font_size_body.value()),
        );
        vspace(ui, theme.spacing_sm);
    }

    ui.label(
        egui::RichText::new(&props.comment_label)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
    ui.add(
        egui::TextEdit::singleline(props.comment_buffer)
            .desired_width(ui.available_width())
            .hint_text(props.comment_hint.clone()),
    );
    vspace(ui, theme.spacing_sm);

    let mut action = ApprovalViewAction::None;
    ui.horizontal_wrapped(|ui| {
        for (idx, choice) in props.choices.iter().enumerate() {
            let label_text = if idx < 9 {
                format!("{} ({})", choice.label, idx + 1)
            } else {
                choice.label.clone()
            };
            let mut btn = egui::Button::new(
                egui::RichText::new(label_text).size(theme.button_font_size().value()),
            );
            if choice.destructive {
                btn = btn.fill(theme.accent_danger().to_egui().linear_multiply(0.18));
            }
            if ui.add(btn).clicked() {
                action = ApprovalViewAction::Chosen {
                    key: choice.key.clone(),
                };
            }
        }
    });

    // 숫자 키 1..=9 단축키. 단축키 → 선택지 index 매핑은 버튼 라벨과 동일.
    let pressed_num = ui.ctx().input(|i| {
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
        && let Some(choice) = props.choices.get(idx - 1)
    {
        action = ApprovalViewAction::Chosen {
            key: choice.key.clone(),
        };
    }

    action
}

/// `ApprovalRecord` → `ApprovalProps` 변환 (i18n 까지 미리 해결).
///
/// 별도 함수로 분리해 view 와 무관하게 단위 테스트 가능.
fn props_from_record<'a>(
    record: &ApprovalRecord,
    comment_buffer: &'a mut String,
) -> ApprovalProps<'a> {
    let severity_label_key = match record.request.severity {
        Severity::Info => "approval.severity.info",
        Severity::Warn => "approval.severity.warn",
        Severity::Danger => "approval.severity.danger",
    };
    ApprovalProps {
        id: record.request.id.to_string(),
        severity: record.request.severity,
        severity_label: t(severity_label_key).to_string(),
        comment_label: t("approval.popup.comment_label").to_string(),
        comment_hint: t("approval.popup.comment_hint").to_string(),
        body: record.request.body.clone(),
        choices: record
            .request
            .choices
            .iter()
            .map(|c| ApprovalChoiceView {
                key: c.key.clone(),
                label: c.label.clone(),
                destructive: c.destructive,
            })
            .collect(),
        comment_buffer,
    }
}

/// PopupDef::on_close entry point — 외부 닫기/X 발생 시 큐 head 만 비운다(정책상
/// X 는 본문에서 막아 두지만 다른 경로로 닫힐 수 있다). 큐가 남아 있으면 다음
/// head 를 위해 popup 을 재발화한다. `state.dispatch_intent` 는 즉시 반영이 아니라
/// 큐잉이므로(dedup: 이미 열려 있으면 무시) 다음 intent 드레인 때 실제로 열린다.
pub fn on_close_approval_popup(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) {
    state.dialogs.approval_comment_buffer.clear();
    if !state.dialogs.pending_approval_ids.is_empty() {
        state.dispatch_intent(
            crate::intent::UiIntent::OpenPopup {
                id: APPROVAL_POPUP_ID,
                mode: crate::intent::OpenPopupMode::WithScope(
                    crate::adapters::ui::popup::PopupScope::Window,
                ),
            }
            .from_agent_ipc(),
        );
    }
}

/// PopupDef.draw_fn — 큐 head 의 record 를 렌더링하고 선택지 클릭/숫자 키로 응답.
///
/// AppState/CoreState 어댑터 wrapper: props 추출 → view 호출 → action 처리.
pub fn draw_approval_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
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

    let th = theme::theme();
    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(th.spacing_sm.value(), th.spacing_xs.value()));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    let mut props = props_from_record(&record, &mut state.dialogs.approval_comment_buffer);
    let view_action = draw_approval_view(ui, &theme::theme(), &mut props);

    if let ApprovalViewAction::Chosen { key: choice_key } = view_action {
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
    let result =
        state.with_memory(|m| m.put(tasty_memory::HOST_OWNER, &scope, &key, &value, &opts));
    if let Err(e) = result {
        tracing::warn!("approval popup: memory put failed: {e}");
    }
}

/// 새 approval 이 생성되면 호출. 큐에 push 하고 popup 이 닫혀 있으면 연다.
/// danger severity 는 priority 가 높지만 현재 PopupManager 는 priority API 가
/// 없으므로 동일 popup_id 로 처리 — 추후 popup-implementation 확장 시 분기.
pub fn enqueue_approval(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
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
        crate::intent::UiIntent::OpenPopup {
            id: APPROVAL_POPUP_ID,
            mode: crate::intent::OpenPopupMode::WithScope(
                crate::adapters::ui::popup::PopupScope::Window,
            ),
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
    let _ = engine; // 옛 직접 add 경로 제거 — cascade 가 라우팅 + add + host event 일괄 처리.
    state.dispatch_intent(
        crate::core::intent::DomainIntent::PushNotification {
            ws_id: workspace,
            surface_id: surface,
            title: format!("{severity_prefix}{}", record.request.title),
            body,
            source: "host".to_string(),
        }
        .from_agent_ipc(),
    );
}

#[cfg(test)]
mod props_tests {
    use super::*;
    use tasty_approval::{ApprovalChoice, ApprovalId, ApprovalRequest, ApprovalState, Requester};

    fn mk_record(
        severity: Severity,
        body: Option<&str>,
        choices: Vec<ApprovalChoice>,
    ) -> ApprovalRecord {
        let request = ApprovalRequest {
            id: ApprovalId("test-1".to_string()),
            requester: Requester::Plugin {
                id: "test".to_string(),
            },
            workspace_id: None,
            surface_id: None,
            title: "Test approval".to_string(),
            body: body.map(|s| s.to_string()),
            choices,
            default_choice: None,
            timeout_ms: None,
            severity,
            created_at: 0,
            metadata: serde_json::Value::Null,
        };
        ApprovalRecord {
            request,
            state: ApprovalState::Pending,
            history: vec![],
        }
    }

    #[test]
    fn props_carry_id_and_severity() {
        let mut buf = String::new();
        let rec = mk_record(Severity::Warn, None, vec![ApprovalChoice::approve()]);
        let props = props_from_record(&rec, &mut buf);
        assert_eq!(props.id, "test-1");
        assert_eq!(props.severity, Severity::Warn);
        assert_eq!(props.choices.len(), 1);
        assert_eq!(props.choices[0].key, "approve");
    }

    #[test]
    fn props_preserve_destructive_flag() {
        let mut buf = String::new();
        let rec = mk_record(
            Severity::Danger,
            Some("body text"),
            vec![ApprovalChoice::approve(), ApprovalChoice::deny()],
        );
        let props = props_from_record(&rec, &mut buf);
        assert_eq!(props.body.as_deref(), Some("body text"));
        assert!(!props.choices[0].destructive);
        assert!(props.choices[1].destructive);
    }

    #[test]
    fn props_empty_body_becomes_none() {
        let mut buf = String::new();
        let rec = mk_record(Severity::Info, None, vec![ApprovalChoice::approve()]);
        let props = props_from_record(&rec, &mut buf);
        assert!(props.body.is_none());
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use tasty_themes::mocha_fallback;

    /// 1 frame egui Context 안에서 view 함수를 호출하고 결과 action 을 받는다.
    fn run_view<F>(props_builder: F) -> ApprovalViewAction
    where
        F: FnOnce() -> (Vec<ApprovalChoiceView>, Option<egui::Key>),
    {
        let ctx = egui::Context::default();
        let mut action = ApprovalViewAction::None;
        let (choices, pressed_key) = props_builder();
        let mut buf = String::new();
        let theme = mocha_fallback();

        let mut raw_input = egui::RawInput::default();
        if let Some(key) = pressed_key {
            raw_input.events.push(egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }

        let _full_output = ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut props = ApprovalProps {
                    id: "abc".to_string(),
                    severity: Severity::Info,
                    severity_label: "INFO".to_string(),
                    comment_label: "Comment".to_string(),
                    comment_hint: "Hint".to_string(),
                    body: Some("body".to_string()),
                    choices: choices.clone(),
                    comment_buffer: &mut buf,
                };
                action = draw_approval_view(ui, &theme, &mut props);
            });
        });
        action
    }

    #[test]
    fn no_input_returns_none() {
        let action = run_view(|| {
            (
                vec![ApprovalChoiceView {
                    key: "approve".to_string(),
                    label: "Approve".to_string(),
                    destructive: false,
                }],
                None,
            )
        });
        assert_eq!(action, ApprovalViewAction::None);
    }

    #[test]
    fn num1_key_chooses_first_choice() {
        let action = run_view(|| {
            (
                vec![
                    ApprovalChoiceView {
                        key: "approve".to_string(),
                        label: "Approve".to_string(),
                        destructive: false,
                    },
                    ApprovalChoiceView {
                        key: "deny".to_string(),
                        label: "Deny".to_string(),
                        destructive: true,
                    },
                ],
                Some(egui::Key::Num1),
            )
        });
        assert_eq!(
            action,
            ApprovalViewAction::Chosen {
                key: "approve".to_string()
            }
        );
    }

    #[test]
    fn num2_key_chooses_second_choice() {
        let action = run_view(|| {
            (
                vec![
                    ApprovalChoiceView {
                        key: "approve".to_string(),
                        label: "Approve".to_string(),
                        destructive: false,
                    },
                    ApprovalChoiceView {
                        key: "deny".to_string(),
                        label: "Deny".to_string(),
                        destructive: true,
                    },
                ],
                Some(egui::Key::Num2),
            )
        });
        assert_eq!(
            action,
            ApprovalViewAction::Chosen {
                key: "deny".to_string()
            }
        );
    }

    #[test]
    fn out_of_range_num_key_ignored() {
        let action = run_view(|| {
            (
                vec![ApprovalChoiceView {
                    key: "approve".to_string(),
                    label: "Approve".to_string(),
                    destructive: false,
                }],
                Some(egui::Key::Num5),
            )
        });
        assert_eq!(action, ApprovalViewAction::None);
    }
}
