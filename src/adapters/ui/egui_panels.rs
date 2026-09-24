use egui::emath::GuiRounding as _;

use crate::model::PhysicalRect;
use crate::state::AppState;
use crate::theme;

/// 탐색기의 최근 폴더를 RecentFiles에 저장할 kind. 주소창 자동완성에서 다시 읽는다.
const EXPLORER_RECENT_KIND: &str = "directory";

struct EguiPanelInfo {
    pane_id: u32,
    /// If Some, this is a specific surface within a split tab.
    /// If None, this is the entire tab's standalone surface.
    surface_id: Option<u32>,
    logical_x: f32,
    logical_y: f32,
    logical_w: f32,
    logical_h: f32,
    /// 탐색기라면 빈 영역 context menu에 사용할 현재 폴더.
    explorer_cwd: Option<std::path::PathBuf>,
    /// 렌더 중 engine을 빌리고 있으므로 DAG 조회 요청만 모아 두 패스 사이에 처리한다.
    dag_poll: Option<crate::adapters::ui::surface::dag_graph::DagPollRequest>,
}

/// 비터미널 패널을 그린다. 터미널 내용은 별도 GPU 경로에서 처리한다.
#[allow(clippy::cognitive_complexity)] // complexity-exempt: egui 즉시모드 draw — panel kind별 렌더 분기, 클로저 중첩이 구조적
pub fn draw_egui_panels(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    pane_rects: &[(u32, PhysicalRect)],
    scale_factor: f32,
) {
    let mut infos = Vec::new();
    {
        let ws = state.active_workspace(engine);
        let ws_id = ws.id;
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

            let content_rect = PhysicalRect {
                x: pane_rect.x,
                y: pane_rect.y + tab_bar_h,
                width: pane_rect.width,
                height: (pane_rect.height - tab_bar_h)
                    .max(tasty_type_geometry::length::PhysicalPx(1.0)),
            };
            // 점유된 터미널도 GPU가 읽기 전용으로 그린다. 비터미널은 숨기지 않고 내용을 표시한다.
            for r in tab.layout().surface_regions(content_rect) {
                if r.surface.kind() == "terminal" {
                    continue;
                }
                // 일반 surface 메뉴로 대체되지 않도록 탐색기의 현재 폴더를 함께 보관한다.
                let explorer_cwd = r
                    .surface
                    .as_any()
                    .downcast_ref::<crate::model::ExplorerPanel>()
                    .map(|p| p.current_root().to_path_buf());
                let dag_poll = r
                    .surface
                    .as_any()
                    .downcast_ref::<crate::model::DagGraphSurface>()
                    .map(|p| {
                        crate::adapters::ui::surface::dag_graph::DagPollRequest::from_surface(
                            p, ws_id,
                        )
                    });
                let logical = r.rect.to_logical(scale_factor);
                let info = EguiPanelInfo {
                    pane_id,
                    surface_id: Some(r.id),
                    logical_x: logical.x.value().round_ui(),
                    logical_y: logical.y.value().round_ui(),
                    logical_w: logical.width.value().round_ui(),
                    logical_h: logical.height.value().round_ui(),
                    explorer_cwd,
                    dag_poll,
                };
                infos.push(info);
            }
        }
    }

    // 비어 있어도 호출해야 안 보이는 DAG의 폴링 타이머를 정리한다.
    // 실제 저장소 읽기는 500ms 간격 제한을 따른다.
    {
        let requests: Vec<_> = infos.iter().filter_map(|i| i.dag_poll.clone()).collect();
        let mut dag_views = std::mem::take(&mut state.dag_graph_views);
        dag_views.poll(engine, &requests);
        state.dag_graph_views = dag_views;
    }

    let mut pending_empty_action: Option<crate::empty_ui::EmptyAction> = None;
    // engine을 빌린 렌더 루프가 끝난 뒤 탐색기 액션을 적용한다.
    let mut pending_explorer_action: Option<(u32, crate::explorer_ui::ExplorerAction)> = None;

    let explorer_font = engine
        .settings
        .appearance
        .effective_font_for_kind("explorer");
    let explorer_bg = crate::theme::theme().bg_panel().to_egui();

    // Temporarily extract view stores so we can hold a `&mut View` from
    // the store at the same time as `&mut Panel` from `engine.workspaces`.
    let mut explorer_views = std::mem::take(&mut state.explorer_views);
    let mut dag_views = std::mem::take(&mut state.dag_graph_views);
    let explorer_favorites = engine.explorer_favorites.items.clone();
    // cut 대기 경로를 어둡게 표시한다. 복사·붙여넣기 완료·취소 후에는 빈 목록으로 해제된다.
    let explorer_cut_pending: std::collections::HashSet<std::path::PathBuf> = engine
        .explorer_clipboard
        .as_ref()
        .filter(|c| c.cut)
        .map(|c| c.paths.iter().cloned().collect())
        .unwrap_or_default();
    // 렌더 중 state를 빌리기 전에 최근 폴더를 한 번 읽어 둔다.
    let explorer_recent_dirs: Vec<String> = state.recent_files.get(EXPLORER_RECENT_KIND);
    // 타입어헤드 게이트 — 셋 다 `state`/`engine` 을 읽어야 해서 가변 차용 루프에
    // 들어가기 전에 값으로 뽑아 둔다(위 스냅샷들과 같은 이유).
    let focused_surface_id = state.focused_surface_id(engine);
    // 전체화면 무대도 함께 본다 — 무대가 떠 있는 동안 그 뒤 세계로 입력이 새면 안 된다.
    let overlay_open = state.keyboard_overlay_open() || state.fullscreen_stage_active();
    // 바인딩 61 개를 훑어 파싱하므로, explorer 패널이 하나도 없는 프레임에는 짓지 않는다
    // (`explorer_cwd` 는 `ExplorerPanel` 일 때만 채워진다).
    let explorer_shortcut_chars = if infos.iter().any(|i| i.explorer_cwd.is_some()) {
        crate::explorer_ui::type_ahead::unmodified_binding_chars(&engine.settings.keybindings)
    } else {
        std::collections::HashSet::new()
    };

    for info in &infos {
        let id_suffix = info
            .surface_id
            .map_or(format!("pane_{}", info.pane_id), |sid| {
                format!("surface_{}", sid)
            });

        let ws = state.active_workspace_mut(engine);
        let mirror_ws_id = ws.mirror.then_some(ws.id);
        let pane = match ws.pane_layout_mut().find_pane_mut(info.pane_id) {
            Some(p) => p,
            None => continue,
        };
        let tab = match pane.active_tab_mut() {
            Some(t) => t,
            None => continue,
        };

        let surface: &mut dyn crate::model::Surface = if let Some(sid) = info.surface_id {
            match tab.layout_mut().find_leaf_mut(sid) {
                Some(leaf) => leaf.as_mut(),
                None => continue,
            }
        } else {
            tab.surface_mut()
        };

        if let Some(empty) = surface
            .as_any()
            .downcast_ref::<crate::model::EmptySurface>()
        {
            draw_panel_frame_no_margin(ctx, &format!("empty_panel_{}", id_suffix), info, |ui| {
                if let Some(act) = crate::empty_ui::draw_empty(ui, empty) {
                    pending_empty_action = Some(act);
                }
            });
        } else if let Some(ex_panel) = surface
            .as_any_mut()
            .downcast_mut::<crate::model::ExplorerPanel>()
        {
            let view = explorer_views.get_or_init(ex_panel, mirror_ws_id);
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
                        &explorer_favorites,
                        &explorer_cut_pending,
                        &explorer_recent_dirs,
                        mirror_ws_id,
                        &crate::explorer_ui::ExplorerInput {
                            focused: info.surface_id.is_some()
                                && info.surface_id == focused_surface_id,
                            overlay_open,
                            shortcut_chars: &explorer_shortcut_chars,
                        },
                    )
                },
            );
            if let Some(a) = act
                && pending_explorer_action.is_none()
            {
                pending_explorer_action = Some((ex_panel.id, a));
            }
        } else if let Some(dag) = surface
            .as_any_mut()
            .downcast_mut::<crate::model::DagGraphSurface>()
        {
            let view = dag_views.get_or_init(dag.id);
            draw_panel_frame_no_margin(ctx, &format!("dag_panel_{}", id_suffix), info, |ui| {
                let target = crate::adapters::ui::surface::dag_graph::DagTarget {
                    dag_id: &mut dag.dag_id,
                    direction: &mut dag.direction,
                };
                crate::adapters::ui::surface::dag_graph::draw_dag_graph(
                    ui,
                    target,
                    view,
                    crate::adapters::ui::surface::dag_graph::DagChrome::Own,
                );
            });
        } else if let Some(remote) = surface
            .as_any()
            .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>(
        ) {
            // webview 내용은 native overlay가 그린다. 여기서는 URL 부재나 overlay 숨김 때 보일 배경을 그린다.
            if crate::core::surface_registry::webview_kind::is_webview_kind(remote.kind_static) {
                let url = crate::model::Surface::webview_url(remote);
                let nav = remote.nav_state();
                draw_panel_frame(
                    ctx,
                    &format!("webview_chrome_{}", id_suffix),
                    info,
                    0,
                    None,
                    |ui| {
                        crate::webview_chrome_ui::draw_webview_chrome(ui, url.as_deref(), nav);
                    },
                );
            }
        }
    }

    let active_ws = state.active_workspace;
    let tab_bar_h = state.tab_bar_height;
    draw_occupied_overlays(ctx, active_ws, tab_bar_h, engine, pane_rects, scale_factor);

    state.explorer_views = explorer_views;
    state.dag_graph_views = dag_views;

    // engine의 하위 항목을 빌린 동안 모은 원격 목록 조회를 큐로 옮긴다.
    for (sid, req) in state.explorer_views.drain_outbox() {
        engine
            .pending_list_dir_forward
            .push(crate::core::PendingListDirForward {
                local_ws_id: req.local_ws_id,
                request_id: req.request_id,
                dir: req.dir.to_string_lossy().to_string(),
                consumer: Some(sid),
            });
    }

    if let Some((sid, act)) = pending_explorer_action {
        apply_explorer_action(state, engine, sid, act);
    }

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

    emit_surface_menu_fallback(state, ctx, &infos);
}

/// 비터미널의 보조 버튼 release를 처리한다. 별도 클릭 위젯을 덮지 않아 내부 버튼과 링크를 가리지 않는다.
/// 탐색기가 먼저 만든 메뉴가 있으면 유지한다. 없더라도 탐색기 위에서는 일반 surface 메뉴 대신
/// 현재 폴더의 빈 영역 메뉴를 연다. 좌표는 egui 논리 좌표이며 한 프레임에 메뉴 하나만 선택한다.
fn emit_surface_menu_fallback(state: &mut AppState, ctx: &egui::Context, infos: &[EguiPanelInfo]) {
    if state.dialogs.pending_native_menu.is_some() {
        return;
    }
    let secondary_pos = ctx.input(|i| {
        if i.pointer.secondary_clicked() {
            i.pointer.interact_pos()
        } else {
            None
        }
    });
    let Some(pos) = secondary_pos else { return };
    for info in infos {
        let Some(sid) = info.surface_id else { continue };
        let within = pos.x >= info.logical_x
            && pos.x <= info.logical_x + info.logical_w
            && pos.y >= info.logical_y
            && pos.y <= info.logical_y + info.logical_h;
        if within {
            state.dialogs.pending_native_menu = Some(match &info.explorer_cwd {
                Some(cwd) => crate::state::PendingNativeMenu::Explorer {
                    surface_id: sid,
                    paths: Vec::new(),
                    cwd: cwd.clone(),
                    single_is_dir: false,
                    x: pos.x,
                    y: pos.y,
                },
                None => crate::state::PendingNativeMenu::Surface {
                    surface_id: sid,
                    x: pos.x,
                    y: pos.y,
                },
            });
            break;
        }
    }
}

/// 모아 둔 탐색기 액션을 원래 surface ID에 적용한다.
pub(crate) fn apply_explorer_action(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    sid: u32,
    act: crate::explorer_ui::ExplorerAction,
) {
    use crate::explorer_ui::ExplorerAction as A;
    match &act {
        A::OpenFile(path) => {
            // 원격 탐색기는 목록 조회만 지원하므로 파일 열기는 안내로 대신한다.
            if engine.is_mirror_surface(sid) {
                state.toasts.push(
                    crate::i18n::t("explorer.state.remote_open_unsupported").to_string(),
                    crate::adapters::ui::ToastKind::Info,
                    crate::adapters::ui::ToastScope::Window,
                );
                return;
            }
            state.dispatch_intent(
                crate::core::intent::DomainIntent::DispatchFile {
                    target: crate::file::format::FileTarget::new(path.clone()),
                    depth: crate::file::format::DetectDepth::Deep,
                    origin_surface_id: Some(sid),
                    dispatch_origin: crate::file::dispatch::FileDispatchOrigin::User,
                    ignore_size_limit: false,
                }
                .from_user_menu("explorer_open_file"),
            );
        }
        A::Refresh => {
            if let Some(v) = state.explorer_views.get_mut(sid) {
                v.request_reload();
            }
        }
        A::SetViewMode(m) => {
            apply_explorer_panel_action(state, engine, sid, &act);
            let mode = m.as_str().to_string();
            if engine.settings.general.explorer_view_mode != mode {
                engine.settings.general.explorer_view_mode = mode;
                if let Err(e) = engine.settings.save() {
                    tracing::warn!("failed to persist explorer view mode: {e}");
                }
            }
        }
        A::ContextMenu { target, cwd, x, y } => {
            use crate::explorer_ui::ExplorerMenuTarget as T;
            let menu = match target {
                T::Favorite { path } => crate::state::PendingNativeMenu::ExplorerFavorite {
                    surface_id: sid,
                    path: path.clone(),
                    x: *x,
                    y: *y,
                },
                _ => {
                    let (paths, single_is_dir) = match target {
                        T::Empty => (Vec::new(), false),
                        T::Single { path, is_dir } => (vec![path.clone()], *is_dir),
                        T::Multi { paths } => (paths.clone(), false),
                        T::Favorite { .. } => unreachable!(),
                    };
                    crate::state::PendingNativeMenu::Explorer {
                        surface_id: sid,
                        paths,
                        cwd: cwd.clone(),
                        single_is_dir,
                        x: *x,
                        y: *y,
                    }
                }
            };
            state.dialogs.pending_native_menu = Some(menu);
        }
        _ => {
            apply_explorer_panel_action(state, engine, sid, &act);
            // 사용자 탐색만 최근 폴더에 기록한다. 에이전트 cwd 변경은 별도 경로다.
            if let A::Navigate(p) = &act {
                state
                    .recent_files
                    .add(EXPLORER_RECENT_KIND, p.display().to_string());
            }
            // 폴더·탭이 바뀌면 주소창 편집을 취소해 이전 버퍼가 다른 대상에 적용되지 않게 한다.
            if matches!(
                act,
                A::Navigate(_)
                    | A::GoBack
                    | A::GoForward
                    | A::GoUp
                    | A::NewTab
                    | A::CloseTab(_)
                    | A::SelectTab(_)
            ) && let Some(v) = state.explorer_views.get_mut(sid)
            {
                v.cancel_addr_edit();
                // 타입어헤드 버퍼도 같은 이유로 버린다. `sync` 의 재적재 경로만으로는
                // cwd 가 같은 내부 탭 사이 전환이 안 잡힌다.
                v.reset_type_ahead();
            }
        }
    }
}

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
        A::OpenFile(_) | A::Refresh | A::ContextMenu { .. } => {}
    }
}

/// egui Area와 Frame을 만들고 body의 반환값을 전달한다.
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
    let bg = bg_color.unwrap_or(th.bg_app().into());
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

/// soft/hard 점유를 각각 Theme의 해당 색으로 표시한다.
/// 포커스와 관계없이 표시하며 강제 해제 버튼은 hard 점유에만 제공한다.
fn draw_occupied_overlays(
    ctx: &egui::Context,
    active_ws: usize,
    tab_bar_h: tasty_type_geometry::length::PhysicalPx,
    engine: &mut crate::core::CoreState,
    pane_rects: &[(u32, PhysicalRect)],
    scale_factor: f32,
) {
    let th = theme::theme();

    struct Occ {
        sid: u32,
        hard: bool,
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
                let hard = if engine.attach.is_content_hidden(r.id) {
                    true
                } else {
                    match engine.attach.occupancy_of(r.id) {
                        Some(occ) => occ.tier == crate::core::attach::OccupancyTier::Hard,
                        None => continue,
                    }
                };
                let logical = r.rect.to_logical(scale_factor);
                occ.push(Occ {
                    sid: r.id,
                    hard,
                    x: logical.x.value().round_ui(),
                    y: logical.y.value().round_ui(),
                    w: logical.width.value().round_ui(),
                    h: logical.height.value().round_ui(),
                });
            }
        }
    }
    if occ.is_empty() {
        return;
    }

    // 장식 테두리는 입력을 받는 Area 대신 painter로 그려 터미널·구분선 입력을 가로채지 않는다.
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("occupied_overlays_border"),
    ));
    for o in &occ {
        let border_color = if o.hard {
            th.accent_occupied_hard()
        } else {
            th.accent_occupied_soft()
        };
        let rect = egui::Rect::from_min_size(egui::pos2(o.x, o.y), egui::vec2(o.w, o.h));
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(th.border_width.value(), border_color),
            egui::StrokeKind::Inside,
        );
    }

    // hard 점유의 강제 해제 버튼만 작은 Area로 입력을 받는다. 클릭 적용은 렌더 뒤에 한다.
    let mut pending_force_detach: Option<u32> = None;
    for o in &occ {
        if !o.hard {
            continue;
        }
        let btn_w = (o.w - 8.0).clamp(24.0, 96.0);
        let inset = th.spacing_xs.value();
        let btn_pos = egui::pos2(o.x + o.w - btn_w - inset, o.y + inset);
        let clicked = egui::Area::new(egui::Id::new(format!("attach_force_detach_{}", o.sid)))
            .fixed_pos(btn_pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| -> bool {
                ui.set_min_size(egui::vec2(btn_w, 20.0));
                ui.set_max_size(egui::vec2(btn_w, 20.0));
                ui.button(crate::i18n::t("attach.force_detach"))
                    .on_hover_text(crate::i18n::t("attach.occupied_surface"))
                    .clicked()
            })
            .inner;
        if clicked {
            pending_force_detach = Some(o.sid);
        }
    }

    if let Some(sid) = pending_force_detach {
        engine.release_occupancy(sid);
    }
}
