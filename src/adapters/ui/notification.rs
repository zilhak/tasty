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

/// on_close 훅 drain 이 상한 없이 서로를 계속 닫는 논리 오류를 방지하는 라운드
/// 상한. 초과 시 그 라운드는 발화하지 않고 경고 로그 후 중단한다.
const ON_CLOSE_DRAIN_MAX_ROUNDS: u32 = 8;

/// `PopupManager.closed_queue` 를 drain 하며 등록된 `on_close` 훅을 발화한다.
/// 훅이 (재발화 등으로) 다른 popup 을 닫으면 그 close 도 큐에 쌓이므로, 큐가
/// 마를 때까지 반복한다 — 단 훅 2개가 서로를 계속 닫는 등의 논리 오류를 대비해
/// 상한을 둔다.
///
/// 6개 close 경로가 전부 이 큐를 채우는지는 `src/adapters/ui/popup.rs`
/// (경로 2: 외부 클릭)와 `src/intent/popup.rs`(경로 3/4: `ClosePopup`/
/// `TogglePopup`)의 단위 테스트로 개별 확인한다. 경로 1(draw_fn Close)과
/// 경로 5(App 직접 호출)는 둘 다 결국 동일한 `state.popups.close(id)` 호출로
/// 귀결되므로 별도 테스트가 필요 없다 — 아래 `popup::close()` 자체의 단위
/// 테스트가 그 호출 경로를 이미 검증한다. 경로 6(debug IPC)은 `defs::find`
/// 로 popup 존재를 확인한 뒤 `UiIntent::ClosePopup` 을 dispatch 할 뿐이라
/// 구조적으로 경로 3 과 동일하다(`adapters/ipc/handler/debug.rs` 의
/// `handle_debug_host_popup_close` 참고).
fn drain_on_close_hooks(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) {
    drain_on_close_hooks_with_lookup(ctx, state, engine, |id| {
        crate::adapters::ui::popup::defs::find(id).and_then(|def| def.on_close)
    });
}

/// [`drain_on_close_hooks`] 의 실제 루프 — 훅 조회를 `lookup` 클로저로 분리해
/// 실제 `defs::all_defs()` 정적 레지스트리에 의존하지 않고 단위 테스트할 수 있게
/// 한다(레지스트리는 컴파일 타임 고정이라 테스트 전용 더미 popup 을 못 끼워 넣음).
fn drain_on_close_hooks_with_lookup(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    lookup: impl Fn(
        crate::adapters::ui::popup::PopupId,
    ) -> Option<fn(&egui::Context, &mut AppState, &mut crate::core::CoreState)>,
) {
    let mut round = 0u32;
    loop {
        let queue = state.popups.take_closed_queue();
        if queue.is_empty() {
            return;
        }
        round += 1;
        if round > ON_CLOSE_DRAIN_MAX_ROUNDS {
            tracing::warn!(
                "popup on_close hook drain exceeded {ON_CLOSE_DRAIN_MAX_ROUNDS} rounds — \
                 aborting (hooks may be closing each other in a loop)"
            );
            return;
        }
        for id in queue {
            if let Some(hook) = lookup(id) {
                hook(ctx, state, engine);
            }
        }
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
    // `enforce_foreground_z_order`(`src/gfx/gpu/egui_bridge.rs`)가 이번 프레임 popup
    // Area 들을 순서대로 최상단으로 올릴 때 읽는다.
    state.popup_layers = draw_result.layers;

    state.popups = popups;

    // Close popups requested by draw dispatch or X button / outside click.
    // popup self-close (draw_fn 이 Close 반환 / X 버튼 / 외부 클릭) 는 popup 시스템
    // 자체의 lifecycle. Intent 큐를 거치면 시각적 close 가 1프레임 지연되어 X 버튼
    // 클릭이 즉시 반응하지 않는 UX 결함이 생긴다.
    for id in dispatch_closed.iter().chain(draw_result.closed.iter()) {
        state.popups.close(id); // intent-exempt: popup self-close lifecycle.
    }

    // `PopupManager::close()` 는 모든 close 경로(draw_fn Close / X버튼·외부클릭 /
    // UiIntent::ClosePopup·TogglePopup / App 직접 호출 / debug IPC)가 거치는
    // 유일한 지점이므로, 여기서 `on_close` 훅을 drain 하면 기존 뒷정리(아래 9개
    // 블록, 이 시점 기준 dispatch_closed/draw_result.closed 경로 2개만 커버)가
    // 놓치던 나머지 경로까지 전부 잡힌다. 현재는 등록된 훅이 0개라 동작 변화 없음
    // — 이관은 후속 TODO.
    drain_on_close_hooks(ctx, state, engine);

    // 마우스 캡처 배너 "더보기" 메뉴 — outside click/Esc로 닫히면(액션 클릭이 아니라)
    // 대상 필드를 정리한다(`draw_popups` 의 인지 복잡도 예산을 넘지 않도록 helper 로 분리).
    cleanup_mouse_capture_menu_target(state, &dispatch_closed, &draw_result.closed);

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
    state.banner_layer = Some(banner_result.layer);
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
    state.modifier_hint_layer = hint_result.layer;

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

#[cfg(test)]
mod on_close_drain_tests {
    use super::*;
    use crate::state::tests::test_state;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn ctx() -> egui::Context {
        egui::Context::default()
    }

    /// `defs::all_defs()` 는 컴파일 타임에 고정된 정적 레지스트리라 테스트 전용
    /// 더미 popup 을 끼워 넣을 수 없다 — `drain_on_close_hooks_with_lookup` 을
    /// 직접 호출해 `lookup` 을 이 HashMap 으로 대체함으로써 실제 레지스트리와
    /// 무관하게 drain 루프(재진입/상한) 자체를 검증한다.
    type Lookup = HashMap<
        crate::adapters::ui::popup::PopupId,
        fn(&egui::Context, &mut AppState, &mut crate::core::CoreState),
    >;

    fn lookup_from(
        map: Lookup,
    ) -> impl Fn(
        crate::adapters::ui::popup::PopupId,
    ) -> Option<fn(&egui::Context, &mut AppState, &mut crate::core::CoreState)> {
        move |id| map.get(id).copied()
    }

    static PLAIN_HOOK_FIRES: AtomicU32 = AtomicU32::new(0);
    fn plain_hook(
        _ctx: &egui::Context,
        _state: &mut AppState,
        _engine: &mut crate::core::CoreState,
    ) {
        PLAIN_HOOK_FIRES.fetch_add(1, Ordering::SeqCst);
    }

    /// 큐에 담긴 id 하나당 훅이 정확히 1회 발화한다.
    #[test]
    fn drain_fires_hook_once_for_queued_close() {
        PLAIN_HOOK_FIRES.store(0, Ordering::SeqCst);
        let (mut state, mut engine) = test_state();
        state.popups.open("notifications"); // close() 는 open 이었던 popup 만 큐에 push.
        state.popups.close("notifications");

        let mut map: Lookup = HashMap::new();
        map.insert("notifications", plain_hook);
        drain_on_close_hooks_with_lookup(&ctx(), &mut state, &mut engine, lookup_from(map));

        assert_eq!(PLAIN_HOOK_FIRES.load(Ordering::SeqCst), 1);
    }

    /// 훅이 다른 popup 을 닫으면(재진입) 그 훅도 같은 drain 호출 안에서 발화한다.
    #[test]
    fn reentrant_close_from_hook_fires_the_other_hook() {
        static A_FIRES: AtomicU32 = AtomicU32::new(0);
        static B_FIRES: AtomicU32 = AtomicU32::new(0);
        A_FIRES.store(0, Ordering::SeqCst);
        B_FIRES.store(0, Ordering::SeqCst);

        fn hook_a(
            _ctx: &egui::Context,
            state: &mut AppState,
            _engine: &mut crate::core::CoreState,
        ) {
            A_FIRES.fetch_add(1, Ordering::SeqCst);
            // 재진입 표현 — "notifications" 훅이 발화하며 다른 popup("search_bar")
            // 을 닫아 그 훅(hook_b)도 같은 drain 호출 안에서 연쇄 발화하게 한다.
            state.popups.close("search_bar");
        }
        fn hook_b(
            _ctx: &egui::Context,
            _state: &mut AppState,
            _engine: &mut crate::core::CoreState,
        ) {
            B_FIRES.fetch_add(1, Ordering::SeqCst);
        }

        let (mut state, mut engine) = test_state();
        state.popups.open("search_bar"); // hook_a 가 닫을 대상 — 먼저 열어둬야 close() 가 큐에 push.
        state.popups.open("notifications"); // 최초 트리거 대상도 open 이어야 close() 가 큐에 push.
        state.popups.close("notifications"); // 최초 트리거.

        let mut map: Lookup = HashMap::new();
        map.insert("notifications", hook_a);
        map.insert("search_bar", hook_b);
        drain_on_close_hooks_with_lookup(&ctx(), &mut state, &mut engine, lookup_from(map));

        assert_eq!(A_FIRES.load(Ordering::SeqCst), 1);
        assert_eq!(B_FIRES.load(Ordering::SeqCst), 1);
    }

    /// 훅이 자기 자신을 매 라운드 재오픈+재닫음하면 무한 재진입이 되므로,
    /// `ON_CLOSE_DRAIN_MAX_ROUNDS` 를 넘기면 경고 후 중단해야 한다(무한루프 방지).
    #[test]
    fn self_reopening_hook_is_capped_by_max_rounds() {
        static LOOP_FIRES: AtomicU32 = AtomicU32::new(0);
        LOOP_FIRES.store(0, Ordering::SeqCst);

        fn looping_hook(
            _ctx: &egui::Context,
            state: &mut AppState,
            _engine: &mut crate::core::CoreState,
        ) {
            LOOP_FIRES.fetch_add(1, Ordering::SeqCst);
            // 매번 재오픈 후 재닫음 — dedup 가드(open 이었을 때만 push)를 매 라운드
            // 통과시켜 큐를 계속 채운다(무한 재진입 시뮬레이션).
            state.popups.open("notifications");
            state.popups.close("notifications");
        }

        let (mut state, mut engine) = test_state();
        state.popups.open("notifications"); // close() 는 open 이었던 popup 만 큐에 push.
        state.popups.close("notifications"); // 최초 트리거 — 1라운드째 큐에 이미 있음.

        let mut map: Lookup = HashMap::new();
        map.insert("notifications", looping_hook);
        drain_on_close_hooks_with_lookup(&ctx(), &mut state, &mut engine, lookup_from(map));

        // 정확히 ON_CLOSE_DRAIN_MAX_ROUNDS 라운드만큼만 발화하고 중단해야 한다
        // (그 이상이면 상한이 실제로 작동하지 않는 것).
        assert_eq!(LOOP_FIRES.load(Ordering::SeqCst), ON_CLOSE_DRAIN_MAX_ROUNDS);
        // 상한을 넘긴 마지막 배치는 `take_closed_queue()` 로 이미 꺼내진 뒤(그래야
        // 그 라운드가 "비어있지 않음"을 판정할 수 있다) 처리 없이 버려진다 — 큐 자체는
        // 빈 채로 남는다(유실된 배치가 재시도 대상으로 남지 않음, 순수 backstop).
        assert!(state.popups.take_closed_queue().is_empty());
    }
}
