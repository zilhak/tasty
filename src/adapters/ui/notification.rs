use std::time::Instant;

use crate::i18n::{t, t_fmt};
use crate::state::AppState;
use crate::theme;
use tasty_ui_widgets::tokens::STRUCT_GAP_2;
use tasty_ui_widgets::{margin_all, vspace};

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
            .color(th.text_muted()),
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
                            .color(th.text_muted()),
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
                    .inner_margin(margin_all(th.spacing_xs))
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
                                        egui::RichText::new(time_str)
                                            .small()
                                            .color(th.text_muted()),
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

                vspace(ui, STRUCT_GAP_2);
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
/// "더보기" 컨텍스트 메뉴가 열려 있는 배너 스코프(없으면 `None`). popup open 여부는
/// `AppState.popups`, 대상 surface 는 `dialogs` 타깃 필드가 따로 갖고 있어 조립이
/// 필요하다 — `draw_popups` 본문에 인라인하면 인지 복잡도 예산을 넘어 별도 함수로 뺐다.
fn mouse_capture_more_menu_open_for(
    state: &AppState,
) -> Option<crate::adapters::ui::banner::BannerScope> {
    if !state
        .popups
        .is_open(crate::adapters::ui::mouse_capture_menu::MOUSE_CAPTURE_BANNER_MENU_POPUP_ID)
    {
        return None;
    }
    state
        .dialogs
        .mouse_capture_banner_menu_target
        .map(crate::adapters::ui::banner::BannerScope::Surface)
}

/// 마우스 캡처 배너 "더보기" 메뉴가 (액션 클릭이든 outside click/Esc 든) 닫혔으면
/// 대상 surface 필드를 비운다. 매번 확인해도 무해(idempotent) — 다른 popup 의
/// close 정리 블록(`rename_closed` 등)과 동일 관례.
fn cleanup_mouse_capture_menu_target(
    state: &mut AppState,
    dispatch_closed: &[&'static str],
    draw_result_closed: &[&'static str],
) {
    let id = crate::adapters::ui::mouse_capture_menu::MOUSE_CAPTURE_BANNER_MENU_POPUP_ID;
    if dispatch_closed.contains(&id) || draw_result_closed.contains(&id) {
        state.dialogs.mouse_capture_banner_menu_target = None;
    }
}

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

    // 마우스 캡처 배너 "더보기" 메뉴 — outside click/Esc로 닫히면(액션 클릭이 아니라)
    // 대상 필드를 정리한다(`draw_popups` 의 인지 복잡도 예산을 넘지 않도록 helper 로 분리).
    cleanup_mouse_capture_menu_target(state, &dispatch_closed, &draw_result.closed);

    // rail_category 팝업 — 닫히면 대상 카테고리 참조 정리.
    let rail_cat_closed = dispatch_closed
        .contains(&crate::adapters::ui::popup::rail_category::RAIL_CATEGORY_POPUP_ID)
        || draw_result
            .closed
            .contains(&crate::adapters::ui::popup::rail_category::RAIL_CATEGORY_POPUP_ID);
    if rail_cat_closed {
        state.dialogs.rail_category_popup = None;
    }

    // (09) transfer_progress 팝업 — 닫히면 진행 상태 정리(backstop; Cancel/완료 경로가
    // 이미 비웠어도 무해).
    let xfer_prog_closed = dispatch_closed
        .contains(&crate::adapters::ui::popup::transfer::TRANSFER_PROGRESS_POPUP_ID)
        || draw_result
            .closed
            .contains(&crate::adapters::ui::popup::transfer::TRANSFER_PROGRESS_POPUP_ID);
    if xfer_prog_closed {
        state.dialogs.transfer_progress = None;
    }

    // (09) transfer_error 팝업 — draw_fn 은 큐가 빌 때만 Close 를 반환하므로, 여기서
    // 닫혔는데 큐가 아직 남아 있으면 scrim/외부 클릭으로 닫힌 것 → head 를 dismiss 하고
    // 남은 실패가 있으면 팝업을 다시 연다(Dismiss 버튼/Esc 는 draw_fn 이 이미 pop).
    let xfer_err_closed = dispatch_closed
        .contains(&crate::adapters::ui::popup::transfer::TRANSFER_ERROR_POPUP_ID)
        || draw_result
            .closed
            .contains(&crate::adapters::ui::popup::transfer::TRANSFER_ERROR_POPUP_ID);
    if xfer_err_closed && !state.dialogs.transfer_error.is_empty() {
        state.dialogs.transfer_error.pop_front();
        if !state.dialogs.transfer_error.is_empty() {
            state.popups.open_centered_focused(
                crate::adapters::ui::popup::transfer::TRANSFER_ERROR_POPUP_ID,
            );
        }
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

    // file_picker (04) — file_handler_picker 와 동일 관례: X 버튼/외부 닫기는
    // dispatch 없이 닫힘으로 간주하고 Cancelled 로 명시, 실제 정리는 호스트
    // 본체의 result-drain(`dispatch_pending_file_picker_results`)이 담당.
    let file_picker_closed = dispatch_closed
        .contains(&crate::adapters::ui::popup::file_picker::FILE_PICKER_POPUP_ID)
        || draw_result
            .closed
            .contains(&crate::adapters::ui::popup::file_picker::FILE_PICKER_POPUP_ID);
    if file_picker_closed
        && let Some(p) = state.dialogs.file_picker.as_mut()
        && p.result.is_none()
    {
        p.result = Some(crate::state::FilePickerResult::Cancelled);
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
    state
        .toasts
        .set_lifetime_ms(engine.settings.overlay.toast_duration_ms);
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
    // "더보기" 컨텍스트 메뉴가 열려 있는 배너 스코프 — 열려 있는 동안 ⋯ 트리거를
    // hover 와 무관하게 active 강조 상태로 유지한다(디자인 확정값 §6-1). `BannerManager`
    // 자신은 popup 시스템을 모르므로 여기서 조립해 넘긴다(helper 로 분리 —
    // `draw_popups` 의 인지 복잡도 예산).
    let more_menu_open_for = mouse_capture_more_menu_open_for(state);
    let banner_result = state.banners.draw(
        ctx,
        &draw_ctx,
        &th,
        view_placeholder,
        reduced_motion,
        more_menu_open_for.as_ref(),
    );
    state.banner_hovered = banner_result.hovered;
    if let Some((scope, trigger_rect)) = banner_result.more_clicked {
        crate::adapters::ui::mouse_capture_menu::open(state, ctx, &scope, trigger_rect);
    }

    // modifier-hint 오버레이 (toast/banner 인접 최상위 레이어). modifier 500ms 홀드 후
    // 표시, 마우스만 소비(키보드 포커스 불가 — 원칙3). 홀드 상태는 winit ModifiersChanged
    // (실사용자 입력)만 반영(원칙1). 놓는 시점의 지오메트리를 UpdateSettings 로 영속한다.
    let hint_result = crate::adapters::ui::modifier_hint_overlay::draw_modifier_hint(
        ctx,
        &mut state.modifier_hint,
        &engine.settings,
        &th,
        reduced_motion,
    );
    state.modifier_hint_hovered = hint_result.hovered;

    // 튜토리얼 오버레이 (마커 오버레이 + 안내 말풍선) — 팝업/toast/banner/modhint 위
    // 최상위 레이어. 마커/scrim 은 hit-transparent, 말풍선만 마우스 소비. 진입·진행은
    // 사용자 클릭으로만(원칙 1). 마커 좌표는 draw_ctx/terminal_rect 로 매 프레임 재해석.
    let content_area = egui::Rect::from_min_size(
        egui::pos2(
            terminal_rect.x.value() / scale_factor,
            terminal_rect.y.value() / scale_factor,
        ),
        egui::vec2(
            terminal_rect.width.value() / scale_factor,
            terminal_rect.height.value() / scale_factor,
        ),
    );
    crate::adapters::ui::tutorial::draw_tutorial_overlay(
        ctx,
        state,
        engine,
        &draw_ctx,
        content_area,
        &th,
    );

    if let Some((pos, size)) = hint_result.persist {
        // 사용자 드래그/리사이즈 결과 → Settings 영속(사이드바 폭 등과 동일 성질,
        // 전역 공유 + last-write-wins). from_user_menu = 사용자 직접 조작 origin.
        let mut new_settings = engine.settings.clone();
        new_settings.modifier_hint.pos = Some(pos);
        new_settings.modifier_hint.size = Some(size);
        state.dispatch_intent(
            crate::core::intent::DomainIntent::UpdateSettings(new_settings)
                .from_user_menu("modifier_hint.geometry"),
        );
    }
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
