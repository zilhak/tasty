//! Handler › Hook Handlers sub-tab — 공유 훅 핸들러 레지스트리 매핑 테이블.
//!
//! 디자인 전사 원본: `ui_kits/terminal/overlays/settings_window.jsx` 의
//! `HookHandlers` / `HookRow` (changelog/2026-07-11-settings-handler-tab.md).
//! intro copy + "Add handler" 버튼 → 인라인 draft 카드 → Mono caps 섹션 헤드 →
//! bordered list rows (id · origin Tag · prio · Switch · remove / Shell cmd Input).
//!
//! 스코프(디자이너 확정): 이 sub-tab 은 **핸들러 레지스트리만** 다룬다 — 웹훅
//! 리스너(서버 bind/port/secret)는 CLI / 별도 지면이며 여기 노출하지 않는다.
//!
//! 데이터 소스는 프로세스 전역 [`crate::hook_handler::global()`] 레지스트리
//! (host 기본 + plugin 기여 + user 매핑). 편집 사항은 [`HookHandlerEditDraft`]
//! 에 쌓이고 Settings 의 Save 가 registry commit + `~/.tasty/hook-handlers.toml`
//! atomic write(`save_user_config`) 로 영속화한다. Cancel 시 폐기.
//!
//! 레지스트리 정책에 따른 행별 허용 조작:
//! - enabled 토글: 전 출처 (user-origin `disabled` override 로 기록).
//! - 셸 명령 인라인 편집: `ShellCommand` action 행만 (user-origin action override).
//!   `IpcSequence` 행은 요약 표시만 (v1 은 인라인 편집 없음 — TOML 손편집).
//! - 제거: user-origin 행만 (host/plugin 행은 base contribution 이 남아 있어
//!   제거해도 finalize 가 되살린다 — 파일 핸들러 sub-tab 과 동일 정책).

use std::collections::{BTreeMap, BTreeSet};

use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Input, TagVariant, switch,
    tag, vspace,
};

use crate::adapters::ui::icons;
use crate::hook_handler::config::UserHookHandlerActionDecl;
use crate::hook_handler::registry::{HookHandlerRegistry, UserHookHandlerUpsertDecl};
use crate::hook_handler::types::is_valid_hook_handler_short_name;
use crate::hook_handler::{
    HookHandler, HookHandlerAction, HookHandlerId, HookHandlerOwner, HookSource,
};
use crate::i18n::{t, t_fmt};

/// jsx `HookRow` line 2 의 "Shell cmd:" 라벨 폭 (`width: 74, flex: none`).
const HOOK_CMD_LABEL_W: LogicalPx = LogicalPx(74.0);
/// jsx add-draft 카드 필드 라벨 폭 (`width: 100, flex: none`).
const HOOK_ADD_LABEL_W: LogicalPx = LogicalPx(100.0);
/// 신규 핸들러 priority step (jsx `commitAdd`: `maxPrio + 10`).
const HOOK_PRIORITY_STEP: i32 = 10;

/// Hook Handlers sub-tab 편집 draft. Save 시 [`Self::apply`], Cancel 시 폐기.
///
/// 파일 핸들러 [`super::FileHandlerEditDraft`] 와 같은 "사용자 의도" 모델 —
/// registry 현재 상태와 비교하지 않고 명시적 user-origin override 로 commit.
#[derive(Debug, Clone, Default)]
pub(crate) struct HookHandlerEditDraft {
    /// handler id → 사용자가 원하는 enabled 상태. 없으면 변경 없음.
    enabled: BTreeMap<HookHandlerId, bool>,
    /// ShellCommand 행의 인라인 명령 편집 (전체 명령 문자열).
    cmd_edits: BTreeMap<HookHandlerId, String>,
    /// user-origin entry 삭제 의도.
    remove: BTreeSet<HookHandlerId>,
    /// 새로 추가될 user 핸들러 (short-name, 셸 명령).
    add: Vec<PendingHookAdd>,
    /// "Add handler" 인라인 draft 폼 상태.
    form: AddHookForm,
}

/// draft 에 쌓인 신규 user 핸들러 한 건.
#[derive(Debug, Clone)]
struct PendingHookAdd {
    short: String,
    cmd: String,
    priority: i32,
}

/// "Add handler" 인라인 draft 폼 (jsx `adding`/`draftId`/`draftCmd`).
#[derive(Debug, Clone, Default)]
struct AddHookForm {
    open: bool,
    id_input: String,
    cmd_input: String,
    error: Option<String>,
}

impl HookHandlerEditDraft {
    pub fn has_changes(&self) -> bool {
        !self.enabled.is_empty()
            || !self.cmd_edits.is_empty()
            || !self.remove.is_empty()
            || !self.add.is_empty()
    }

    /// draft 를 registry 에 commit 한다. 디스크 영속(`save_user_config`)은 호출측
    /// (Settings Save) 책임 — 파일 핸들러 sub-tab 과 동일 분업.
    pub fn apply(self, reg: &HookHandlerRegistry) {
        for (id, enabled) in &self.enabled {
            reg.set_user_handler_disabled(id, !enabled);
        }
        for (id, cmd) in &self.cmd_edits {
            // 인라인 편집은 전체 명령 문자열 하나 — `command` 에 그대로 담고
            // args 는 비운다 (hook 트리거 실행 경로가 셸 경유로 해석).
            // ShellCommand 는 source=hook 불변식이 있어 명시적으로 함께 적는다.
            let decl = UserHookHandlerUpsertDecl {
                id: id.as_str().to_string(),
                source: Some(HookSource::Hook),
                priority: None,
                display_name_i18n_key: None,
                disabled: None,
                action: Some(UserHookHandlerActionDecl::ShellCommand {
                    command: cmd.clone(),
                    args: Vec::new(),
                }),
            };
            if let Err(e) = reg.upsert_user_handler(decl) {
                tracing::warn!("hook_handlers tab: cmd edit upsert failed: {e}");
            }
        }
        for id in &self.remove {
            reg.remove_user_handler(id);
        }
        for add in self.add {
            let decl = UserHookHandlerUpsertDecl {
                id: format!("user/{}", add.short),
                source: Some(HookSource::Hook),
                priority: Some(add.priority),
                display_name_i18n_key: None,
                disabled: Some(false),
                action: Some(UserHookHandlerActionDecl::ShellCommand {
                    command: add.cmd,
                    args: Vec::new(),
                }),
            };
            if let Err(e) = reg.upsert_user_handler(decl) {
                tracing::warn!("hook_handlers tab: add upsert failed: {e}");
            }
        }
    }
}

/// Hook Handlers sub-tab 콘텐츠 (jsx `HookHandlers` 전사).
pub(super) fn draw_hook_handlers(ui: &mut egui::Ui, hh: &mut HookHandlerEditDraft) {
    let th = crate::theme::theme();
    let reg = crate::hook_handler::global();
    vspace(ui, th.spacing_xs);

    // ── intro row: 설명 paragraph(flex 1, measure-md) + "Add handler" 버튼 ──
    ui.horizontal_top(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if Button::new(t("settings.file_handler.hook_handlers.add_button"))
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Sm)
                .leading_icon(&|ui, rect, c| {
                    icons::PLUS.image(rect.width(), c).paint_at(ui, rect);
                })
                .show(ui, &th)
                .clicked()
            {
                hh.form = AddHookForm {
                    open: true,
                    ..AddHookForm::default()
                };
            }
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_max_width(th.measure_md.value());
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(t("settings.file_handler.hook_handlers.description"))
                            .size(th.font_size_term_sm.value())
                            .color(th.text_muted()),
                    )
                    .wrap(),
                );
            });
        });
    });
    vspace(ui, th.spacing_md);

    // ── "Add handler" 인라인 draft 카드 (jsx `adding && …`) ──
    if hh.form.open {
        draw_add_card(ui, &th, hh, reg);
        vspace(ui, th.spacing_md);
    }

    // ── Mono caps 섹션 헤드 (jsx `headStyle`) ──
    mono_caps_head(
        ui,
        &th,
        t("settings.file_handler.hook_handlers.section_head"),
    );
    vspace(ui, th.spacing_xs);

    // ── 등록 핸들러 rows (registry 정렬순: priority↑ → owner → id) + draft 추가분 ──
    let rows = reg.all_handlers_including_disabled();
    if rows.is_empty() && hh.add.is_empty() {
        ui.label(
            egui::RichText::new(t("settings.file_handler.hook_handlers.empty"))
                .size(th.font_size_term_sm.value())
                .color(th.text_muted()),
        );
        return;
    }
    let mut toggle: Option<(HookHandlerId, bool)> = None;
    let mut cmd_edit: Option<(HookHandlerId, String)> = None;
    let mut remove_toggle: Option<HookHandlerId> = None;
    for h in &rows {
        draw_hook_row(
            ui,
            &th,
            hh,
            h,
            &mut toggle,
            &mut cmd_edit,
            &mut remove_toggle,
        );
    }
    // draft 로 추가된 행 — jsx 는 commitAdd 가 목록에 바로 push 하므로 pending add
    // 도 동일 비주얼의 행으로 노출한다 (Save 전까지는 draft 에만 존재).
    let mut remove_add: Option<usize> = None;
    for (i, add) in hh.add.iter_mut().enumerate() {
        draw_pending_add_row(ui, &th, i, add, &mut remove_add);
    }
    if let Some((id, enabled)) = toggle {
        hh.enabled.insert(id, enabled);
    }
    if let Some((id, cmd)) = cmd_edit {
        hh.cmd_edits.insert(id, cmd);
    }
    if let Some(id) = remove_toggle
        && !hh.remove.remove(&id)
    {
        hh.remove.insert(id);
    }
    if let Some(i) = remove_add {
        hh.add.remove(i);
    }
}

/// jsx `headStyle` — mono 10 uppercase, letter-spacing caps, text-muted.
/// (egui 는 letter-spacing 미지원 — 기존 전사 관례대로 mono micro uppercase 로 전사.)
fn mono_caps_head(ui: &mut egui::Ui, th: &tasty_type_appearance::theme::Theme, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .monospace()
            .size(th.font_size_micro.value())
            .color(th.text_muted()),
    );
}

/// 한 핸들러 행 (jsx `HookRow` 전사) — 2행 컬럼 + 하단 separator + disabled 시
/// row 전체 opacity.
fn draw_hook_row(
    ui: &mut egui::Ui,
    th: &tasty_type_appearance::theme::Theme,
    hh: &HookHandlerEditDraft,
    h: &HookHandler,
    toggle: &mut Option<(HookHandlerId, bool)>,
    cmd_edit: &mut Option<(HookHandlerId, String)>,
    remove_toggle: &mut Option<HookHandlerId>,
) {
    let on = hh.enabled.get(&h.id).copied().unwrap_or(!h.disabled);
    let pending_remove = hh.remove.contains(&h.id);
    let is_shell = matches!(h.action, HookHandlerAction::ShellCommand { .. });
    let cmd_display = hh
        .cmd_edits
        .get(&h.id)
        .cloned()
        .unwrap_or_else(|| action_display(&h.action));

    let resp = ui.scope(|ui| {
        if !on {
            ui.set_opacity(th.opacity_disabled());
        }
        // jsx padding: sm(상하) xs(좌우), 내부 행간 gap xs.
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: th.spacing_xs.value() as i8,
                right: th.spacing_xs.value() as i8,
                top: th.spacing_sm.value() as i8,
                bottom: th.spacing_sm.value() as i8,
            })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
                // ── line 1: id · origin Tag · prio · (우측) Switch + remove ──
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // RTL: 먼저 추가 = 가장 우측. jsx 순서 Switch → remove(우측 끝).
                        if matches!(h.owner, HookHandlerOwner::User)
                            && IconButton::new()
                                .variant(IconButtonVariant::Ghost)
                                .size(ControlSize::Sm)
                                .show(ui, th, &|ui, rect, c| {
                                    icons::TRASH.image(rect.width(), c).paint_at(ui, rect);
                                })
                                .clicked()
                        {
                            *remove_toggle = Some(h.id.clone());
                        }
                        if pending_remove {
                            ui.label(
                                egui::RichText::new(t(
                                    "settings.file_handler.common.pending_remove",
                                ))
                                .size(th.font_size_caption.value())
                                .color(th.text_muted()),
                            );
                        }
                        let mut checked = on;
                        if switch(ui, th, &mut checked, None, true).changed() {
                            *toggle = Some((h.id.clone(), checked));
                        }
                        // 좌측 나머지 (LTR 로 되돌림): id + Tag + prio.
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(h.id.as_str())
                                        .monospace()
                                        .strong()
                                        .size(th.font_size_body.value())
                                        .color(th.text_primary()),
                                )
                                .truncate(),
                            );
                            let (label, variant) = origin_tag(&h.owner);
                            tag(ui, th, label, variant, false);
                            ui.label(
                                egui::RichText::new(t_fmt(
                                    "settings.file_handler.hook_handlers.prio",
                                    &h.priority.to_string(),
                                ))
                                .monospace()
                                .size(th.font_size_micro.value())
                                .color(th.text_muted()),
                            );
                        });
                    });
                });
                // ── line 2: action — 셸 명령 인라인 Input / IpcSequence 요약 ──
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                    row_label(
                        ui,
                        th,
                        if is_shell {
                            t("settings.file_handler.hook_handlers.shell_cmd_label")
                        } else {
                            t("settings.file_handler.hook_handlers.action_label")
                        },
                    );
                    if is_shell {
                        let mut buf = cmd_display.clone();
                        if Input::new()
                            .mono(true)
                            .enabled(on)
                            .show(ui, th, &mut buf)
                            .changed()
                        {
                            *cmd_edit = Some((h.id.clone(), buf));
                        }
                    } else {
                        ui.label(
                            egui::RichText::new(cmd_display)
                                .monospace()
                                .size(th.font_size_term_sm.value())
                                .color(th.text_secondary()),
                        );
                    }
                });
            });
    });
    row_separator(ui, th, resp.response.rect);
}

/// draft 로 추가된(아직 Save 전) user 핸들러 행 — 실제 행과 동일 비주얼.
fn draw_pending_add_row(
    ui: &mut egui::Ui,
    th: &tasty_type_appearance::theme::Theme,
    index: usize,
    add: &mut PendingHookAdd,
    remove_add: &mut Option<usize>,
) {
    let resp = ui.scope(|ui| {
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: th.spacing_xs.value() as i8,
                right: th.spacing_xs.value() as i8,
                top: th.spacing_sm.value() as i8,
                bottom: th.spacing_sm.value() as i8,
            })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if IconButton::new()
                            .variant(IconButtonVariant::Ghost)
                            .size(ControlSize::Sm)
                            .show(ui, th, &|ui, rect, c| {
                                icons::TRASH.image(rect.width(), c).paint_at(ui, rect);
                            })
                            .clicked()
                        {
                            *remove_add = Some(index);
                        }
                        ui.label(
                            egui::RichText::new(t(
                                "settings.file_handler.hook_handlers.pending_add",
                            ))
                            .size(th.font_size_caption.value())
                            .color(th.text_muted()),
                        );
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("user/{}", add.short))
                                        .monospace()
                                        .strong()
                                        .size(th.font_size_body.value())
                                        .color(th.text_primary()),
                                )
                                .truncate(),
                            );
                            tag(ui, th, "user", TagVariant::Default, false);
                            ui.label(
                                egui::RichText::new(t_fmt(
                                    "settings.file_handler.hook_handlers.prio",
                                    &add.priority.to_string(),
                                ))
                                .monospace()
                                .size(th.font_size_micro.value())
                                .color(th.text_muted()),
                            );
                        });
                    });
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                    row_label(
                        ui,
                        th,
                        t("settings.file_handler.hook_handlers.shell_cmd_label"),
                    );
                    Input::new().mono(true).show(ui, th, &mut add.cmd);
                });
            });
    });
    row_separator(ui, th, resp.response.rect);
}

/// jsx HookRow line 2 라벨 — `width:74, fontSize caption, text-muted`.
fn row_label(ui: &mut egui::Ui, th: &tasty_type_appearance::theme::Theme, text: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(HOOK_CMD_LABEL_W.value(), th.input_height().value()),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(th.font_size_caption.value())
                    .color(th.text_muted()),
            );
        },
    );
}

/// 행 하단 1px separator (jsx `borderBottom: 1px solid separator`).
fn row_separator(ui: &mut egui::Ui, th: &tasty_type_appearance::theme::Theme, rect: egui::Rect) {
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );
}

/// jsx `HOOK_ORIGIN` — plugin 은 `agent` variant, host/user 는 default Tag.
fn origin_tag(owner: &HookHandlerOwner) -> (&'static str, TagVariant) {
    match owner {
        HookHandlerOwner::Host => ("host", TagVariant::Default),
        HookHandlerOwner::Plugin(_) => ("plugin", TagVariant::Agent),
        HookHandlerOwner::User => ("user", TagVariant::Default),
    }
}

/// action 표시 문자열 — ShellCommand 는 전체 명령, IpcSequence 는 method 요약.
fn action_display(action: &HookHandlerAction) -> String {
    match action {
        HookHandlerAction::ShellCommand { command, args } => {
            if args.is_empty() {
                command.clone()
            } else {
                format!("{command} {}", args.join(" "))
            }
        }
        HookHandlerAction::IpcSequence { calls } => {
            let methods: Vec<&str> = calls.iter().map(|c| c.method.as_str()).collect();
            format!("ipc: {}", methods.join(" → "))
        }
    }
}

/// "Add handler" 인라인 draft 카드 (jsx `adding && …` 블록 전사) —
/// surface-raised + 1px border + radius 카드, caps 헤드 + 2 필드 행 + 우측 버튼.
fn draw_add_card(
    ui: &mut egui::Ui,
    th: &tasty_type_appearance::theme::Theme,
    hh: &mut HookHandlerEditDraft,
    reg: &HookHandlerRegistry,
) {
    egui::Frame::new()
        .fill(th.surface_raised().to_egui())
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            th.border_default().to_egui(),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(egui::Margin::same(th.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = th.spacing_sm.value();
            mono_caps_head(ui, th, t("settings.file_handler.hook_handlers.new_head"));
            add_field_row(
                ui,
                th,
                t("settings.file_handler.hook_handlers.field_event_id"),
                t("settings.file_handler.hook_handlers.placeholder_event_id"),
                &mut hh.form.id_input,
            );
            add_field_row(
                ui,
                th,
                t("settings.file_handler.hook_handlers.field_shell_command"),
                t("settings.file_handler.hook_handlers.placeholder_shell_command"),
                &mut hh.form.cmd_input,
            );
            if let Some(err) = &hh.form.error {
                ui.label(
                    egui::RichText::new(err)
                        .size(th.font_size_caption.value())
                        .color(th.accent_danger()),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                // RTL: Add handler(primary, 우측 끝) ← Cancel(ghost).
                let can_add = !hh.form.id_input.trim().is_empty();
                if Button::new(t("settings.file_handler.hook_handlers.add_button"))
                    .variant(ButtonVariant::Primary)
                    .size(ControlSize::Sm)
                    .enabled(can_add)
                    .show(ui, th)
                    .clicked()
                {
                    commit_add(hh, reg);
                }
                if Button::new(t("button.cancel"))
                    .variant(ButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .show(ui, th)
                    .clicked()
                {
                    hh.form = AddHookForm::default();
                }
            });
        });
}

/// add 카드의 라벨(100px) + mono Input 행 (jsx `minHeight settings-row-min-height`).
fn add_field_row(
    ui: &mut egui::Ui,
    th: &tasty_type_appearance::theme::Theme,
    label: &str,
    placeholder: &str,
    buf: &mut String,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_lg.value();
        ui.allocate_ui_with_layout(
            egui::vec2(
                HOOK_ADD_LABEL_W.value(),
                th.settings_row_min_height().value(),
            ),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new(label)
                        .size(th.font_size_body.value())
                        .color(th.text_secondary()),
                );
            },
        );
        Input::new()
            .mono(true)
            .placeholder(placeholder)
            .show(ui, th, buf);
    });
}

/// add 폼 확정 (jsx `commitAdd`) — short-name 검증 후 draft 에 push.
/// priority 는 현재 registry rows + pending adds 의 max + step (jsx `maxPrio + 10`).
fn commit_add(hh: &mut HookHandlerEditDraft, reg: &HookHandlerRegistry) {
    let short = hh.form.id_input.trim().to_string();
    if short.is_empty() {
        hh.form.error = Some(t("settings.file_handler.hook_handlers.err_id_empty").to_string());
        return;
    }
    if !is_valid_hook_handler_short_name(&short) {
        hh.form.error = Some(t("settings.file_handler.hook_handlers.err_id_invalid").to_string());
        return;
    }
    let max_prio = reg
        .all_handlers_including_disabled()
        .iter()
        .map(|h| h.priority)
        .chain(hh.add.iter().map(|a| a.priority))
        .max()
        .unwrap_or(0);
    hh.add.push(PendingHookAdd {
        short,
        cmd: hh.form.cmd_input.trim().to_string(),
        priority: max_prio + HOOK_PRIORITY_STEP,
    });
    hh.form = AddHookForm::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell_add_draft(short: &str, cmd: &str, priority: i32) -> HookHandlerEditDraft {
        HookHandlerEditDraft {
            add: vec![PendingHookAdd {
                short: short.into(),
                cmd: cmd.into(),
                priority,
            }],
            ..Default::default()
        }
    }

    /// draft add → apply → save → 새 registry 로 reload 왕복이 목록에 그대로
    /// 복원되는지 (Settings Save 영속 경로의 유닛 재현).
    #[test]
    fn add_apply_save_reload_roundtrip() {
        let reg = HookHandlerRegistry::new();
        let draft = shell_add_draft("on-deploy", "~/ops/on-deploy.sh", 30);
        draft.apply(&reg);

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hook-handlers.toml");
        reg.save_user_config(&path).expect("save");

        let reg2 = HookHandlerRegistry::new();
        reg2.install_user_config(&path);
        let rows = reg2.all_handlers_including_disabled();
        assert_eq!(rows.len(), 1);
        let h = &rows[0];
        assert_eq!(h.id.as_str(), "user/on-deploy");
        assert_eq!(h.priority, 30);
        assert!(!h.disabled);
        assert!(matches!(
            &h.action,
            HookHandlerAction::ShellCommand { command, .. } if command == "~/ops/on-deploy.sh"
        ));
    }

    /// enabled 토글 draft 가 user-origin disabled override 로 반영·영속되는지.
    #[test]
    fn toggle_disable_roundtrip() {
        let reg = HookHandlerRegistry::new();
        shell_add_draft("noisy", "echo hi", 10).apply(&reg);

        let mut draft = HookHandlerEditDraft::default();
        draft
            .enabled
            .insert(HookHandlerId::new("user/noisy"), false);
        draft.apply(&reg);
        assert!(reg.all_handlers_including_disabled()[0].disabled);

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hook-handlers.toml");
        reg.save_user_config(&path).expect("save");
        let reg2 = HookHandlerRegistry::new();
        reg2.install_user_config(&path);
        assert!(reg2.all_handlers_including_disabled()[0].disabled);
    }

    /// 셸 명령 인라인 편집이 user-origin action override 로 commit 되는지.
    #[test]
    fn cmd_edit_applies_shell_override() {
        let reg = HookHandlerRegistry::new();
        shell_add_draft("greet", "echo hi", 10).apply(&reg);

        let mut draft = HookHandlerEditDraft::default();
        draft
            .cmd_edits
            .insert(HookHandlerId::new("user/greet"), "echo bye".into());
        draft.apply(&reg);

        let rows = reg.all_handlers_including_disabled();
        assert!(matches!(
            &rows[0].action,
            HookHandlerAction::ShellCommand { command, .. } if command == "echo bye"
        ));
    }

    /// user-origin 행 remove draft → registry 에서 사라짐.
    #[test]
    fn remove_user_row() {
        let reg = HookHandlerRegistry::new();
        shell_add_draft("gone", "echo x", 10).apply(&reg);

        let mut draft = HookHandlerEditDraft::default();
        draft.remove.insert(HookHandlerId::new("user/gone"));
        draft.apply(&reg);
        assert!(reg.all_handlers_including_disabled().is_empty());
    }

    /// commit_add 검증 — 잘못된 short-name 은 error, 유효하면 max+step priority.
    #[test]
    fn commit_add_validates_and_steps_priority() {
        let reg = HookHandlerRegistry::new();
        shell_add_draft("base", "echo", 25).apply(&reg);

        let mut hh = HookHandlerEditDraft::default();
        hh.form.id_input = "Bad.Name".into();
        commit_add(&mut hh, &reg);
        assert!(hh.form.error.is_some());
        assert!(hh.add.is_empty());

        hh.form.id_input = "pipeline-done".into();
        hh.form.cmd_input = "tasty notify done".into();
        hh.form.error = None;
        commit_add(&mut hh, &reg);
        assert_eq!(hh.add.len(), 1);
        assert_eq!(hh.add[0].priority, 35); // 25 + step(10)
    }
}
