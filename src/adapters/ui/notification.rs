use std::time::Instant;

use crate::i18n::{t, t_fmt};
use crate::state::AppState;
use crate::theme;

/// Draw notification panel content inside a popup Ui.
/// Called by the notifications popup's `draw_fn` (see popup_defs).
pub(crate) fn draw_notification_content_inner(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) {
    let th = theme::theme();

    // Header with mark-all-read button
    ui.horizontal(|ui| {
        let unread = engine.notifications.unread_count();
        ui.label(
            egui::RichText::new(t_fmt(
                "notification_panel.unread_count",
                &unread.to_string(),
            ))
            .small()
            .color(th.subtext0),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(t("button.mark_all_read")).clicked() {
                state.dispatch_intent(
                    crate::core::intent::DomainIntent::MarkAllNotificationsRead
                        .from_user_menu("notification_panel.mark_all_read"),
                );
            }
        });
    });
    ui.separator();

    // Scrollable notification list (newest first)
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .drag_to_scroll(false)
        .show(ui, |ui| {
            let notification_count = engine.notifications.all().len();
            if notification_count == 0 {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new(t("notification_panel.empty_message"))
                            .color(th.subtext0),
                    );
                });
                return;
            }

            let now = Instant::now();
            let entries: Vec<_> = engine
                .notifications
                .all()
                .rev()
                .map(|n| {
                    let elapsed = now.duration_since(n.timestamp);
                    let time_str = if elapsed.as_secs() < 60 {
                        format!("{}s ago", elapsed.as_secs())
                    } else if elapsed.as_secs() < 3600 {
                        format!("{}m ago", elapsed.as_secs() / 60)
                    } else {
                        format!("{}h ago", elapsed.as_secs() / 3600)
                    };

                    let ws_name = engine
                        .workspaces
                        .iter()
                        .find(|ws| ws.id == n.source_workspace)
                        .map(|ws| ws.name.as_str())
                        .unwrap_or("Unknown");

                    (
                        n.id,
                        n.read,
                        n.title.clone(),
                        n.body.clone(),
                        time_str,
                        ws_name.to_string(),
                        n.source_workspace,
                    )
                })
                .collect();

            let mut mark_read_id = None;
            let mut jump_to_ws = None;

            for (id, read, title, body, time_str, ws_name, ws_id) in &entries {
                let bg = if *read {
                    egui::Color32::TRANSPARENT
                } else {
                    // unread 알림 항목 배경: theme blue 의 살짝 깔린 톤.
                    crate::theme::theme().blue.with_alpha(20).to_egui()
                };

                egui::Frame::new()
                    .fill(bg)
                    .inner_margin(egui::Margin::same(4))
                    .corner_radius(4.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if !*read {
                                ui.label(
                                    egui::RichText::new("*").color(th.accent_primary()).strong(),
                                );
                            }
                            ui.label(egui::RichText::new(title).strong().small());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(time_str).small().color(th.subtext0),
                                    );
                                },
                            );
                        });

                        if !body.is_empty() {
                            ui.label(egui::RichText::new(body).small());
                        }

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(ws_name)
                                    .small()
                                    .color(th.accent_primary()),
                            );

                            if ui
                                .small_button(t("button.jump_to_workspace"))
                                .on_hover_text(t("tooltip.jump_to_workspace"))
                                .clicked()
                            {
                                jump_to_ws = Some(*ws_id);
                                mark_read_id = Some(*id);
                            }
                        });
                    });

                ui.add_space(2.0);
            }

            if let Some(id) = mark_read_id {
                state.dispatch_intent(
                    crate::core::intent::DomainIntent::MarkNotificationRead { id }
                        .from_user_menu("notification_panel.mark_read"),
                );
            }
            if let Some(ws_id) = jump_to_ws
                && let Some(idx) = engine.workspaces.iter().position(|ws| ws.id == ws_id)
            {
                state.switch_workspace(engine, idx);
            }
        });
}

/// Draw all popups via the PopupManager. Called from egui_bridge.
pub fn draw_popups(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    pane_rects: &[(u32, crate::model::PhysicalRect)],
    terminal_rect: crate::model::PhysicalRect,
    scale_factor: f32,
) {
    // Build scope context for popup visibility/clamping
    let draw_ctx = build_layout_context(state, engine, pane_rects, terminal_rect, scale_factor);

    // Refresh popup titles (i18n) and dynamic sizes each frame. Sizers read
    // in-memory caches so this is cheap.
    // intent-exempt: 매 프레임 i18n title / size 재계산은 mutation 이 아닌 draw-prep.
    // Intent 큐로 보내면 1프레임 지연 + 매 프레임 enqueue 라 부적절.
    for def in crate::adapters::ui::popup::defs::all_defs() {
        let new_title = if let Some(title_fn) = def.title_fn {
            (title_fn)(state, engine)
        } else {
            crate::i18n::t(def.title_key).to_string()
        };
        let new_size = def.sizer.map(|f| f(state, engine));
        if let Some(p) = state.popups.get_mut(def.id) {
            p.title = new_title;
            // 사용자가 직접 리사이즈한 팝업은 sizer 가 크기를 되돌리지 않는다
            // (size_user_overridden 가드 — popup close 시 리셋되어 다음 open 에 복원).
            if let Some(sz) = new_size
                && !p.size_user_overridden
            {
                p.size = sz;
            }
        }
    }

    // Temporarily take the popup manager to avoid borrow conflicts with AppState.
    let mut popups = std::mem::replace(&mut state.popups, crate::adapters::ui::PopupManager::new());

    let mut dispatch_closed: Vec<&'static str> = Vec::new();
    let draw_result = popups.draw(
        ctx,
        &mut |id, ui| {
            if let Some(def) = crate::adapters::ui::popup::defs::find(id)
                && matches!(
                    (def.draw_fn)(ui, state, engine),
                    crate::adapters::ui::PopupAction::Close
                )
            {
                dispatch_closed.push(def.id);
            }
        },
        Some(&draw_ctx),
    );

    // Update input layer state: popup hover blocks mouse events to lower layers
    state.popup_hovered = draw_result.hovered;

    state.popups = popups;

    // Close popups requested by draw dispatch or X button / outside click.
    // popup self-close (draw_fn 이 Close 반환 / X 버튼 / 외부 클릭) 는 popup 시스템
    // 자체의 lifecycle. Intent 큐를 거치면 시각적 close 가 1프레임 지연되어 X 버튼
    // 클릭이 즉시 반응하지 않는 UX 결함이 생긴다.
    for id in dispatch_closed.iter().chain(draw_result.closed.iter()) {
        state.popups.close(id); // intent-exempt: popup self-close lifecycle.
    }

    // Clean up convert_surface dialog state when closed
    let convert_closed = dispatch_closed.contains(&"convert_surface")
        || draw_result.closed.contains(&"convert_surface");
    if convert_closed {
        state.dialogs.convert_popup = None;
        state.dialogs.convert_popup_selected = None;
    }

    // Clean up rename dialog state when closed (X button)
    let rename_closed =
        dispatch_closed.contains(&"rename") || draw_result.closed.contains(&"rename");
    if rename_closed {
        state.dialogs.rename = None;
    }

    // rail_category 팝업 — 닫히면 대상 카테고리 참조 정리.
    let rail_cat_closed = dispatch_closed
        .contains(&crate::adapters::ui::popup::rail_category::RAIL_CATEGORY_POPUP_ID)
        || draw_result
            .closed
            .contains(&crate::adapters::ui::popup::rail_category::RAIL_CATEGORY_POPUP_ID);
    if rail_cat_closed {
        state.dialogs.rail_category_popup = None;
    }

    // confirm_delete_category 팝업 — 닫히면(취소/외부/Escape) 삭제 대상 정리.
    let del_cat_closed = dispatch_closed.contains(
        &crate::adapters::ui::popup::confirm_delete_category::CONFIRM_DELETE_CATEGORY_POPUP_ID,
    ) || draw_result.closed.contains(
        &crate::adapters::ui::popup::confirm_delete_category::CONFIRM_DELETE_CATEGORY_POPUP_ID,
    );
    if del_cat_closed {
        state.dialogs.pending_category_delete = None;
    }

    // file_handler_picker — X 버튼 또는 외부 닫기는 dispatch 없이 닫힘으로 간주.
    // dispatch 결과는 picker draw_fn 안에서 미리 채워두므로, 여기서는 추가 처리 없음.
    // 호스트 본체 layer 가 result 를 소비한 뒤 None 으로 리셋한다.
    let picker_closed = dispatch_closed
        .contains(&crate::adapters::ui::popup::file_handler_picker::PICKER_POPUP_ID)
        || draw_result
            .closed
            .contains(&crate::adapters::ui::popup::file_handler_picker::PICKER_POPUP_ID);
    if picker_closed
        && let Some(p) = state.dialogs.file_handler_picker.as_mut()
        && p.result.is_none()
    {
        // X 버튼 등 외부 경로로 닫힘 — Cancelled 로 명시.
        p.result = Some(crate::state::FileHandlerPickerResult::Cancelled);
    }

    // approval popup: 외부 닫기/X 발생 시 큐 head 만 비운다 (정책상 X 는 본문에서
    // 막아 두지만 다른 경로로 닫힐 수 있다). 큐가 남아 있으면 다음 head 로 다시 연다.
    let approval_closed = dispatch_closed
        .contains(&crate::adapters::ui::popup::approval::APPROVAL_POPUP_ID)
        || draw_result
            .closed
            .contains(&crate::adapters::ui::popup::approval::APPROVAL_POPUP_ID);
    if approval_closed {
        state.dialogs.approval_comment_buffer.clear();
        if !state.dialogs.pending_approval_ids.is_empty() {
            // 다음 approval head 를 위해 popup 재발화. Intent dedup 이 이미 열려 있을
            // 때 무시하므로 안전.
            state.dispatch_intent(
                crate::intent::UiIntent::OpenPopup {
                    id: crate::adapters::ui::popup::approval::APPROVAL_POPUP_ID,
                    mode: crate::intent::OpenPopupMode::WithScope(
                        crate::adapters::ui::popup::PopupScope::Window,
                    ),
                }
                .from_agent_ipc(),
            );
        }
    }

    // Toast 렌더링 (popup 위 레이어). 같은 LayoutContext를 공유한다.
    let reduced_motion = engine.settings.accessibility.reduced_motion;
    state.toasts.draw(ctx, &draw_ctx, reduced_motion);

    // Banner 렌더링 (toast 와 동일 LayoutContext). 배너는 스코프 콘텐츠 최상단(탭바
    // 아래)에 뜨며 자기 영역의 마우스를 소비한다 — `banner_hovered` 로 하위 레이어
    // 전파를 막는다(포커스는 받지 않음). View 스코프 배너는 각 View 가 지정한
    // 플레이스홀더에 뜬다 — 화면 상단(탭바 아래)을 기본 플레이스홀더로 둔다.
    let th = theme::theme();
    let screen = ctx.screen_rect();
    let view_placeholder = Some(egui::Rect::from_min_max(
        egui::pos2(screen.left(), screen.top() + th.tab_bar_height.value()),
        screen.max,
    ));
    let banner_result = state
        .banners
        .draw(ctx, &draw_ctx, &th, view_placeholder, reduced_motion);
    state.banner_hovered = banner_result.hovered;
}

/// Build LayoutContext from current AppState and layout info.
fn build_layout_context(
    state: &AppState,
    engine: &crate::core::CoreState,
    pane_rects: &[(u32, crate::model::PhysicalRect)],
    terminal_rect: crate::model::PhysicalRect,
    scale_factor: f32,
) -> crate::adapters::ui::LayoutContext {
    let active_workspace = state.active_workspace;

    // Convert physical pixel pane rects to logical pixel egui rects
    let pane_rects_logical: Vec<(u32, egui::Rect)> = pane_rects
        .iter()
        .map(|(id, r)| {
            (
                *id,
                egui::Rect::from_min_size(
                    egui::pos2(r.x.value() / scale_factor, r.y.value() / scale_factor),
                    egui::vec2(
                        r.width.value() / scale_factor,
                        r.height.value() / scale_factor,
                    ),
                ),
            )
        })
        .collect();

    // Compute surface rects using surface_regions
    let mut surface_rects = Vec::new();
    for (_pane_id, _pane_rect, regions) in state.surface_regions(engine, terminal_rect) {
        for r in regions {
            surface_rects.push((
                r.id,
                egui::Rect::from_min_size(
                    egui::pos2(
                        r.rect.x.value() / scale_factor,
                        r.rect.y.value() / scale_factor,
                    ),
                    egui::vec2(
                        r.rect.width.value() / scale_factor,
                        r.rect.height.value() / scale_factor,
                    ),
                ),
            ));
        }
    }

    // Collect active tab indices
    let mut active_tabs = Vec::new();
    let ws = state.active_workspace(engine);
    for &pid in &ws.pane_layout().all_pane_ids() {
        if let Some(pane) = ws.pane_layout().find_pane(pid) {
            active_tabs.push((pid, pane.active_tab));
        }
    }

    crate::adapters::ui::LayoutContext {
        active_workspace,
        pane_rects: pane_rects_logical,
        surface_rects,
        active_tabs,
    }
}

/// PopupDef::draw_fn for the notifications panel.
pub fn draw_notification_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> crate::adapters::ui::popup::PopupAction {
    draw_notification_content_inner(ui, state, engine);
    crate::adapters::ui::popup::PopupAction::None
}
