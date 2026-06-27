use egui::emath::GuiRounding as _;
use winit::keyboard::{Key, NamedKey};

use crate::model::PhysicalRect;
use crate::state::{AppState, PendingKeyEvent};
use crate::theme;

struct EguiPanelInfo {
    pane_id: u32,
    /// If Some, this is a specific surface within a split tab.
    /// If None, this is the entire tab's standalone surface.
    surface_id: Option<u32>,
    logical_x: f32,
    logical_y: f32,
    logical_w: f32,
    logical_h: f32,
    /// Whether this panel is the keyboard target (receives pending_surface_keys).
    is_keyboard_target: bool,
}

/// Render egui-based panels (Markdown, Explorer, Html, Empty).
/// Terminal panels are rendered by the wgpu shader pipeline; these are rendered by egui.
/// Supports both standalone non-terminal tabs and non-terminal leaves within split tabs.
pub fn draw_egui_panels(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    pane_rects: &[(u32, PhysicalRect)],
    scale_factor: f32,
    canvas_cache: &crate::gpu::canvas_texture::CanvasTextureCache,
) {
    // First pass: gather info about egui-rendered panels (read-only).
    let mut infos = Vec::new();
    {
        let ws = state.active_workspace(engine);
        let focused_pane_id = ws.focused_pane;
        let tab_bar_h = state.tab_bar_height;
        for &(pane_id, pane_rect) in pane_rects {
            let pane = match ws.pane_layout().find_pane(pane_id) {
                Some(p) => p,
                None => continue,
            };
            let tab = match pane.tabs.get(pane.active_tab) {
                Some(t) => t,
                None => continue,
            };

            // Collect non-GPU-rendered surfaces from this tab.
            let focused_surface_in_tab = tab.focused_surface;
            let content_rect = PhysicalRect {
                x: pane_rect.x,
                y: pane_rect.y + tab_bar_h,
                width: pane_rect.width,
                height: (pane_rect.height - tab_bar_h)
                    .max(tasty_type_geometry::length::PhysicalPx(1.0)),
            };
            // egui 로 그려지는 surface = terminal 외 모든 종류.
            // attach/detach 작업 J(readonly 정정): 점유된 터미널은 render_pass 가
            // readonly display mirror 로 렌더하므로 egui 는 관여하지 않는다(점유 표시
            // 테두리만 §J-3 오버레이가 그린다). 점유된 비-터미널은 mirror 불가지만
            // **숨기지 않고 내용을 readonly 로 렌더**하되 키 입력만 suppress 한다.
            for r in tab.layout().surface_regions(content_rect) {
                if r.surface.kind() == "terminal" {
                    // free·점유 모두 GPU 렌더(점유는 readonly mirror). egui 미관여.
                    continue;
                }
                // 점유 비-터미널은 조작 차단(키 입력 미적용) — 보기 전용.
                let is_readonly = engine.attach.is_content_hidden(r.id);
                let info = EguiPanelInfo {
                    pane_id,
                    surface_id: Some(r.id),
                    logical_x: (r.rect.x.value() / scale_factor).round_ui(),
                    logical_y: (r.rect.y.value() / scale_factor).round_ui(),
                    logical_w: (r.rect.width.value() / scale_factor).round_ui(),
                    logical_h: (r.rect.height.value() / scale_factor).round_ui(),
                    is_keyboard_target: !is_readonly
                        && pane_id == focused_pane_id
                        && r.id == focused_surface_in_tab,
                };
                infos.push(info);
            }
        }
    }

    // Drain pending keyboard events for non-terminal surfaces.
    // Only the panel that is_keyboard_target will use these.
    let surface_keys: Vec<PendingKeyEvent> = state.pending_surface_keys.drain(..).collect();

    // Second pass: render each egui panel.
    let mut pending_empty_action: Option<crate::empty_ui::EmptyAction> = None;
    // T11: explorer 의 사용자 조작은 deferred 로 모아 렌더 루프 종료 후 적용한다
    // (engine 가변 차용 충돌 회피 — empty action 패턴과 동일). (surface_id, action).
    let mut pending_explorer_action: Option<(u32, crate::explorer_ui::ExplorerAction)> = None;

    let markdown_surface = crate::theme::theme().surface("markdown").clone();
    let markdown_font = engine
        .settings
        .appearance
        .effective_font_for_kind("markdown");
    let explorer_font = engine
        .settings
        .appearance
        .effective_font_for_kind("explorer");
    let explorer_bg = crate::theme::theme().bg_panel().to_egui();

    // Temporarily extract view stores so we can hold a `&mut View` from
    // the store at the same time as `&mut Panel` from `engine.workspaces`.
    // (Same pattern applied to image_views just below.)
    let mut markdown_views = std::mem::take(&mut state.markdown_views);
    let mut image_views = std::mem::take(&mut state.image_views);
    let mut explorer_views = std::mem::take(&mut state.explorer_views);

    for info in &infos {
        let id_suffix = info
            .surface_id
            .map_or(format!("pane_{}", info.pane_id), |sid| {
                format!("surface_{}", sid)
            });

        let ws = state.active_workspace_mut(engine);
        let pane = match ws.pane_layout_mut().find_pane_mut(info.pane_id) {
            Some(p) => p,
            None => continue,
        };
        let tab = match pane.active_tab_mut() {
            Some(t) => t,
            None => continue,
        };

        // Get the surface to render: either a leaf within a split tab, or the tab's surface.
        let surface: &mut dyn crate::model::Surface = if let Some(sid) = info.surface_id {
            match tab.layout_mut().find_leaf_mut(sid) {
                Some(leaf) => leaf.as_mut(),
                None => continue,
            }
        } else {
            tab.surface_mut()
        };

        if let Some(md_panel) = surface
            .as_any_mut()
            .downcast_mut::<crate::model::MarkdownPanel>()
        {
            let scroll_line = 24.0;
            let scroll_page = info.logical_h * 0.8;
            let key_scroll_y = if info.is_keyboard_target {
                let mut dy = 0.0;
                for k in &surface_keys {
                    match &k.key {
                        Key::Named(NamedKey::ArrowUp) => dy += scroll_line,
                        Key::Named(NamedKey::ArrowDown) => dy -= scroll_line,
                        Key::Named(NamedKey::PageUp) => dy += scroll_page,
                        Key::Named(NamedKey::PageDown) => dy -= scroll_page,
                        _ => {}
                    }
                }
                dy
            } else {
                0.0
            };
            let md_bg = if info.is_keyboard_target {
                markdown_surface.focused_bg.to_egui()
            } else {
                markdown_surface.unfocused_bg.to_egui()
            };
            let view = markdown_views.get_or_init(md_panel);
            draw_panel_frame(
                ctx,
                &format!("md_panel_{}", id_suffix),
                info,
                8,
                Some(md_bg),
                |ui| {
                    crate::markdown_ui::draw_markdown(
                        ui,
                        view,
                        key_scroll_y,
                        &id_suffix,
                        &markdown_font,
                    );
                },
            );
        } else if let Some(empty) = surface
            .as_any()
            .downcast_ref::<crate::model::EmptySurface>()
        {
            draw_panel_frame_no_margin(ctx, &format!("empty_panel_{}", id_suffix), info, |ui| {
                if let Some(act) = crate::empty_ui::draw_empty(ui, empty) {
                    pending_empty_action = Some(act);
                }
            });
        } else if let Some(image_panel) = surface
            .as_any_mut()
            .downcast_mut::<crate::model::ImagePanel>()
        {
            let view = image_views.get_or_init(image_panel);
            draw_panel_frame(
                ctx,
                &format!("image_panel_{}", id_suffix),
                info,
                4,
                None,
                |ui| {
                    crate::image_ui::draw_image(ui, image_panel, view);
                },
            );
        } else if let Some(ex_panel) = surface
            .as_any_mut()
            .downcast_mut::<crate::model::ExplorerPanel>()
        {
            let view = explorer_views.get_or_init(ex_panel);
            let act = draw_panel_frame(
                ctx,
                &format!("explorer_panel_{}", id_suffix),
                info,
                0,
                Some(explorer_bg),
                |ui| {
                    crate::explorer_ui::draw_explorer(
                        ui,
                        ex_panel,
                        view,
                        &explorer_font,
                        &id_suffix,
                    )
                },
            );
            if let Some(a) = act {
                if pending_explorer_action.is_none() {
                    pending_explorer_action = Some((ex_panel.id, a));
                }
            }
        } else if let Some(remote) = surface
            .as_any()
            .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>(
        ) {
            draw_panel_frame(
                ctx,
                &format!("remote_panel_{}", id_suffix),
                info,
                4,
                None,
                |ui| {
                    crate::plugin_bridge::ui_tree_render::render_remote_surface(
                        ui,
                        remote,
                        canvas_cache,
                    );
                },
            );
        }
    }

    // attach/detach 작업 J: 점유 surface 의 주황 테두리 + force-detach 오버레이는
    // `draw_occupied_overlays` 가 그린다(§J-3). readonly 정정으로 "내용 숨김
    // placeholder 안내" 는 폐기됐다(내용은 render_pass/위 infos 가 readonly 로 보임).
    let active_ws = state.active_workspace;
    let tab_bar_h = state.tab_bar_height;
    draw_occupied_overlays(ctx, active_ws, tab_bar_h, engine, pane_rects, scale_factor);

    // Restore extracted view stores before any further `state` access below.
    state.markdown_views = markdown_views;
    state.image_views = image_views;
    state.explorer_views = explorer_views;

    // T11: explorer deferred action 적용 (view store 복원 후 — state/engine 가변 차용 가능).
    if let Some((sid, act)) = pending_explorer_action {
        apply_explorer_action(state, engine, sid, act);
    }

    // Apply deferred empty surface action (must happen after render loop due to state mutation).
    if let Some(crate::empty_ui::EmptyAction::OpenConvertPopup(sid)) = pending_empty_action {
        state.dialogs.convert_popup = Some(sid);
        state.dialogs.convert_popup_selected = None;
        state.dispatch_intent(
            crate::intent::UiIntent::OpenPopup {
                id: "convert_surface",
                mode: crate::intent::OpenPopupMode::WithScope(
                    crate::adapters::ui::popup::PopupScope::Surface(sid),
                ),
            }
            .from_user_menu("empty_surface_convert"),
        );
    }

    // T9: host-rendered(egui) surface 우클릭 → surface 컨텍스트 메뉴(잘라내기/이동).
    // egui 가 우클릭을 소비(egui_consumed=true)해 winit `mouse.rs` 경로가 일찍 반환
    // 되는 경우를 커버한다. 전역 포인터 상태만 읽어(별도 click-sense 위젯을 덧대지
    // 않아 markdown 링크·explorer 버튼 등 내부 상호작용을 가로채지 않음) 비-terminal
    // 패널 rect 안의 secondary click 을 잡는다. winit 경로와 **단일 슬롯을 공유**하고,
    // 이미 설정돼 있으면(=winit 경로가 먼저 잡음) 덮지 않는다 → 소비 여부와 무관히
    // 한 메뉴만 표시(중복 발화 없음). 패널 rect 는 logical px, interact_pos 도 logical.
    if state.dialogs.pending_native_menu.is_none() {
        let secondary_pos = ctx.input(|i| {
            if i.pointer.secondary_clicked() {
                i.pointer.interact_pos()
            } else {
                None
            }
        });
        if let Some(pos) = secondary_pos {
            for info in &infos {
                let Some(sid) = info.surface_id else { continue };
                let within = pos.x >= info.logical_x
                    && pos.x <= info.logical_x + info.logical_w
                    && pos.y >= info.logical_y
                    && pos.y <= info.logical_y + info.logical_h;
                if within {
                    state.dialogs.pending_native_menu =
                        Some(crate::state::PendingNativeMenu::Surface {
                            surface_id: sid,
                            x: pos.x,
                            y: pos.y,
                        });
                    break;
                }
            }
        }
    }
}

/// T11: explorer deferred action 적용. 파일 열기/새로고침은 `state` 만, 내비게이션/
/// 뷰모드/탭 조작은 대상 `ExplorerPanel` (origin surface id 로 직접 지정 — 포커스
/// 독립)을 가변 차용해 처리한다.
pub(crate) fn apply_explorer_action(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    sid: u32,
    act: crate::explorer_ui::ExplorerAction,
) {
    use crate::explorer_ui::ExplorerAction as A;
    match &act {
        A::OpenFile(path) => {
            state.dispatch_intent(
                crate::core::intent::DomainIntent::DispatchFile {
                    target: crate::file::format::FileTarget::new(path.clone()),
                    depth: crate::file::format::DetectDepth::Deep,
                    origin_surface_id: Some(sid),
                }
                .from_user_menu("explorer_open_file"),
            );
        }
        A::Refresh => {
            if let Some(v) = state.explorer_views.get_mut(sid) {
                v.request_reload();
            }
        }
        A::ContextMenu { target, cwd, x, y } => {
            use crate::explorer_ui::ExplorerMenuTarget as T;
            let (paths, single_is_dir) = match target {
                T::Empty => (Vec::new(), false),
                T::Single { path, is_dir } => (vec![path.clone()], *is_dir),
                T::Multi { paths } => (paths.clone(), false),
            };
            // explorer 전용 메뉴를 단일 슬롯에 선점 → 이후 generic surface fallback
            // (egui_panels 의 secondary_pos 루프)은 이미 설정됨을 보고 건너뛴다.
            state.dialogs.pending_native_menu =
                Some(crate::state::PendingNativeMenu::Explorer {
                    surface_id: sid,
                    paths,
                    cwd: cwd.clone(),
                    single_is_dir,
                    x: *x,
                    y: *y,
                });
        }
        _ => apply_explorer_panel_action(state, engine, sid, &act),
    }
}

/// `ExplorerPanel` 을 가변 차용해 내비게이션/뷰모드/내부 탭 조작을 적용한다.
fn apply_explorer_panel_action(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    sid: u32,
    act: &crate::explorer_ui::ExplorerAction,
) {
    let ws = state.active_workspace_mut(engine);
    let pane_ids = ws.pane_layout().all_pane_ids();
    for pid in pane_ids {
        let Some(pane) = ws.pane_layout_mut().find_pane_mut(pid) else {
            continue;
        };
        for tab in pane.tabs.iter_mut() {
            if !tab.contains_surface(sid) {
                continue;
            }
            let Some(leaf) = tab.layout_mut().find_leaf_mut(sid) else {
                continue;
            };
            let Some(ex) = leaf
                .as_any_mut()
                .downcast_mut::<crate::model::ExplorerPanel>()
            else {
                continue;
            };
            apply_to_explorer_panel(ex, act);
            return;
        }
    }
}

fn apply_to_explorer_panel(
    ex: &mut crate::model::ExplorerPanel,
    act: &crate::explorer_ui::ExplorerAction,
) {
    use crate::explorer_ui::ExplorerAction as A;
    match act {
        A::Navigate(p) => {
            ex.active_tab_mut().navigate_to(p.clone());
        }
        A::GoBack => {
            ex.active_tab_mut().go_back();
        }
        A::GoForward => {
            ex.active_tab_mut().go_forward();
        }
        A::GoUp => {
            ex.active_tab_mut().go_up();
        }
        A::SetViewMode(m) => {
            ex.active_tab_mut().view_mode = *m;
        }
        A::SetSort(col) => {
            let tab = ex.active_tab_mut();
            if tab.sort_column == *col {
                tab.sort_dir = tab.sort_dir.toggled();
            } else {
                tab.sort_column = *col;
                tab.sort_dir = crate::model::SortDir::Asc;
            }
        }
        A::NewTab => ex.add_tab(),
        A::CloseTab(i) => ex.close_tab(*i),
        A::SelectTab(i) => {
            if *i < ex.tabs.len() {
                ex.active = *i;
            }
        }
        // OpenFile/Refresh/ContextMenu 는 apply_explorer_action 에서 처리.
        A::OpenFile(_) | A::Refresh | A::ContextMenu { .. } => {}
    }
}

/// 공통 egui Area + Frame 껍데기. `margin`만큼 내부 여백을 준다.
/// `bg_color`가 Some이면 해당 색상을, None이면 th.crust를 배경으로 사용한다.
/// body의 반환값을 그대로 전달한다 (None을 리턴하는 기존 호출처는 ()).
fn draw_panel_frame<R, F>(
    ctx: &egui::Context,
    id: &str,
    info: &EguiPanelInfo,
    margin: i8,
    bg_color: Option<egui::Color32>,
    body: F,
) -> R
where
    F: FnOnce(&mut egui::Ui) -> R,
    R: Default,
{
    let th = theme::theme();
    let bg = bg_color.unwrap_or(th.crust.into());
    let mut out: R = R::default();
    egui::Area::new(egui::Id::new(id))
        .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(info.logical_w, info.logical_h));
            ui.set_max_size(egui::vec2(info.logical_w, info.logical_h));
            let panel_rect = ui.max_rect();
            let mut clip_ui = ui.new_child(egui::UiBuilder::new().max_rect(panel_rect));
            clip_ui.set_clip_rect(panel_rect);
            clip_ui.painter().rect_filled(panel_rect, 0.0, bg);
            egui::Frame::new()
                .fill(bg)
                .inner_margin(egui::Margin::same(margin))
                .show(&mut clip_ui, |ui| {
                    out = body(ui);
                });
        });
    out
}

/// 여백 없이 Area만 거는 변형. Empty surface처럼 배경을 직접 칠하는 경우에 사용.
fn draw_panel_frame_no_margin<F>(ctx: &egui::Context, id: &str, info: &EguiPanelInfo, body: F)
where
    F: FnOnce(&mut egui::Ui),
{
    egui::Area::new(egui::Id::new(id))
        .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(info.logical_w, info.logical_h));
            ui.set_max_size(egui::vec2(info.logical_w, info.logical_h));
            let panel_rect = ui.max_rect();
            let mut clip_ui = ui.new_child(egui::UiBuilder::new().max_rect(panel_rect));
            clip_ui.set_clip_rect(panel_rect);
            body(&mut clip_ui);
        });
}

/// 점유된 surface 의 **주황 테두리 + force-detach 오버레이**(attach/detach 작업 J-3).
///
/// 서버측에서 client 가 점유한 surface 를 "알림이 온 것처럼" 주황(`th.peach`) 1px
/// 테두리로 표시한다. **focus 와 무관**하게 점유 중이면 항상 그린다(focus 해도 사라지지
/// 않음). readonly 정정으로 내용은 보이므로(render_pass/egui), 테두리는 *점유 표식* +
/// force-detach 진입점 역할만 한다. 색은 Theme 토큰(하드코딩 없음).
fn draw_occupied_overlays(
    ctx: &egui::Context,
    active_ws: usize,
    tab_bar_h: tasty_type_geometry::length::PhysicalPx,
    engine: &mut crate::core::CoreState,
    pane_rects: &[(u32, PhysicalRect)],
    scale_factor: f32,
) {
    let th = theme::theme();

    /// 점유 surface 의 logical rect(읽기 단계 수집물).
    struct Occ {
        sid: u32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    }
    let mut occ: Vec<Occ> = Vec::new();
    {
        let Some(ws) = engine.workspaces.get(active_ws) else {
            return;
        };
        for &(pane_id, pane_rect) in pane_rects {
            let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
                continue;
            };
            let Some(tab) = pane.tabs.get(pane.active_tab) else {
                continue;
            };
            let content_rect = PhysicalRect {
                x: pane_rect.x,
                y: pane_rect.y + tab_bar_h,
                width: pane_rect.width,
                height: (pane_rect.height - tab_bar_h)
                    .max(tasty_type_geometry::length::PhysicalPx(1.0)),
            };
            for r in tab.layout().surface_regions(content_rect) {
                // 터미널 단위 점유(is_attached) 또는 workspace 점유 멤버(is_content_hidden).
                if !engine.attach.is_attached(r.id) && !engine.attach.is_content_hidden(r.id) {
                    continue;
                }
                occ.push(Occ {
                    sid: r.id,
                    x: (r.rect.x.value() / scale_factor).round_ui(),
                    y: (r.rect.y.value() / scale_factor).round_ui(),
                    w: (r.rect.width.value() / scale_factor).round_ui(),
                    h: (r.rect.height.value() / scale_factor).round_ui(),
                });
            }
        }
    }
    if occ.is_empty() {
        return;
    }

    // 주황 테두리 + 우상단 force-detach 버튼. 클릭은 deferred 적용(engine 가변 차용).
    let mut pending_force_detach: Option<u32> = None;
    for o in &occ {
        let clicked = egui::Area::new(egui::Id::new(format!("attach_occupied_{}", o.sid)))
            .fixed_pos(egui::pos2(o.x, o.y))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| -> bool {
                ui.set_min_size(egui::vec2(o.w, o.h));
                ui.set_max_size(egui::vec2(o.w, o.h));
                let rect = ui.max_rect();
                // 1px 주황 테두리(focus 무관, Theme 토큰).
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0, th.peach),
                    egui::StrokeKind::Inside,
                );
                // 우상단 force-detach 버튼(작게). readonly 점유를 회수하는 진입점.
                let btn_w = (o.w - 8.0).clamp(24.0, 96.0);
                let btn_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.right() - btn_w - 4.0, rect.top() + 4.0),
                    egui::vec2(btn_w, 20.0),
                );
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(btn_rect));
                child
                    .button(crate::i18n::t("attach.force_detach"))
                    .on_hover_text(crate::i18n::t("attach.occupied_surface"))
                    .clicked()
            })
            .inner;
        if clicked {
            pending_force_detach = Some(o.sid);
        }
    }

    if let Some(sid) = pending_force_detach {
        // workspace 점유면 멤버 일괄 해제(단계 6 D6), 아니면 surface 단위.
        if let Some(ws) = engine.attach.workspace_of_surface(sid) {
            engine.attach.force_detach_workspace(ws);
        } else {
            engine.attach.force_detach(sid);
        }
    }
}
