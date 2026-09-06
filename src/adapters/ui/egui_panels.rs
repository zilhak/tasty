use egui::emath::GuiRounding as _;

use crate::model::PhysicalRect;
use crate::state::AppState;
use crate::theme;

/// explorer 최근 방문 디렉토리를 담는 `RecentFiles` kind. markdown 이 파일을 kind
/// "markdown" 으로 적재하는 것과 대칭으로, explorer 는 이동 확정한 cwd 를 이 kind 로
/// 적재하고 주소창(PathField) 자동완성 후보로 되읽는다(generic per-kind — 신규 DB 테이블
/// 불필요, 기존 `recent_files(kind, path, opened_at)` 재사용).
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
    /// explorer surface 이면 첫 패스에서 캡처한 `current_root()`(빈영역 메뉴 cwd),
    /// 그 외 surface 는 `None`. fallback 이 explorer 위에서 generic `Surface` 메뉴
    /// 대신 항상 빈영역 explorer 메뉴를 세우게 하는 kind 판별 겸 cwd 운반자다(A-1).
    explorer_cwd: Option<std::path::PathBuf>,
    /// DAG surface 이면 이번 프레임의 폴링 요청. 렌더 루프 안에서는 `engine` 이
    /// workspace/pane/tab 에 배타 차용돼 task store 를 읽을 수 없으므로, 첫 패스에서
    /// "무엇을 읽어야 하는지" 만 캡처해 두 패스 사이에서 읽는다(explorer 스냅샷과
    /// 같은 이유).
    dag_poll: Option<crate::adapters::ui::surface::dag_graph::DagPollRequest>,
}

/// Render egui-based panels (Markdown, Explorer, Html, DAG, Empty).
/// Terminal panels are rendered by the wgpu shader pipeline; these are rendered by egui.
/// Supports both standalone non-terminal tabs and non-terminal leaves within split tabs.
#[allow(clippy::cognitive_complexity)] // complexity-exempt: egui 즉시모드 draw — panel kind별 렌더 분기, 클로저 중첩이 구조적
pub fn draw_egui_panels(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    pane_rects: &[(u32, PhysicalRect)],
    scale_factor: f32,
) {
    // First pass: gather info about egui-rendered panels (read-only).
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

            // Collect non-GPU-rendered surfaces from this tab.
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
                // explorer surface 는 빈영역 메뉴 cwd(=current_root)를 미리 캡처해
                // fallback 이 explorer-aware 하게 동작하도록 한다(A-1). catch-all 이
                // 어떤 이유로 슬롯을 못 세워도 fallback 이 generic 메뉴 대신 explorer
                // 빈영역 메뉴를 세운다(불가침 §1·§2).
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

    // 두 패스 사이 — DAG surface 의 데이터를 필요하면 새로 읽는다. 500ms 게이트는
    // 스토어 쪽에 있어 프레임마다 호출해도 실제 읽기는 그 주기로만 일어난다.
    //
    // **requests 가 비어도 반드시 호출한다.** 빈 목록은 "이 창에 보이는 DAG 뷰가
    // 없다" 는 뜻이고, `poll` 이 그때 `visible` 을 비워야 호스트가 폴링 타이머를
    // 걷는다. 건너뛰면 배경 탭으로 밀린(=닫히지는 않은) surface 의 옛 데드라인이
    // 계속 남아 이벤트 루프가 쉬지 못한다.
    {
        let requests: Vec<_> = infos.iter().filter_map(|i| i.dag_poll.clone()).collect();
        let mut dag_views = std::mem::take(&mut state.dag_graph_views);
        dag_views.poll(engine, &requests);
        state.dag_graph_views = dag_views;
    }

    // Second pass: render each egui panel.
    let mut pending_empty_action: Option<crate::empty_ui::EmptyAction> = None;
    // T11: explorer 의 사용자 조작은 deferred 로 모아 렌더 루프 종료 후 적용한다
    // (engine 가변 차용 충돌 회피 — empty action 패턴과 동일). (surface_id, action).
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
    // 즐겨찾기는 전역(engine 보유)이라 루프에서 engine 이 가변 차용되는 동안엔
    // 읽을 수 없다 → 프레임당 1회 스냅샷(항목 소수, clone 비용 무시 가능).
    let explorer_favorites = engine.explorer_favorites.items.clone();
    // cut-pending 집합(잘라내기 대기 경로) — 셀 디밍용. 클립보드가 cut 모드일 때만
    // 채워지고, 붙여넣기 완료/복사/취소로 클립보드가 비거나 copy 가 되면 빈 집합이
    // 되어 디밍이 자동 해제된다(프레임당 1회 스냅샷, 항목 소수).
    let explorer_cut_pending: std::collections::HashSet<std::path::PathBuf> = engine
        .explorer_clipboard
        .as_ref()
        .filter(|c| c.cut)
        .map(|c| c.paths.iter().cloned().collect())
        .unwrap_or_default();
    // 최근 방문 디렉토리(주소창 자동완성 후보) — 프레임당 1회 스냅샷(≤10, clone 무시 가능).
    // 루프 안에서 state 가 가변 차용되는 동안 읽을 수 없어 owned Vec 로 뽑아 둔다.
    let explorer_recent_dirs: Vec<String> = state.recent_files.get(EXPLORER_RECENT_KIND).to_vec();

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

        // Get the surface to render: either a leaf within a split tab, or the tab's surface.
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
                crate::adapters::ui::surface::dag_graph::draw_dag_graph(ui, target, view);
            });
        } else if let Some(remote) = surface
            .as_any()
            .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>(
        ) {
            // webview-kind(rendering="webview", 예: html) surface 는 native WebView
            // overlay 가 콘텐츠를 그리므로 host 는 chrome 만 페인트한다(placeholder=URL
            // 미지정 / boundary=overlay backdrop). overlay 가 보일 땐 이 chrome 을
            // 덮고, overlay 가 숨겨지거나(메뉴/팝업) URL 이 없을 때 노출된다.
            // UiNode(tree) surface 렌더 경로는 제거됨(C1) — webview kind 만 그린다.
            if crate::core::surface_registry::webview_kind::is_webview_kind(remote.kind_static) {
                let url = crate::model::Surface::webview_url(remote);
                // RemoteSurface mirror(host sync_webviews 가 native nav_state 를 복사)에서
                // navigation 상태를 읽어 loading/error chrome 분기에 쓴다.
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

    // attach/detach 작업 J: 점유 surface 의 주황 테두리 + force-detach 오버레이는
    // `draw_occupied_overlays` 가 그린다(§J-3). readonly 정정으로 "내용 숨김
    // placeholder 안내" 는 폐기됐다(내용은 render_pass/위 infos 가 readonly 로 보임).
    let active_ws = state.active_workspace;
    let tab_bar_h = state.tab_bar_height;
    draw_occupied_overlays(ctx, active_ws, tab_bar_h, engine, pane_rects, scale_factor);

    // Restore extracted view stores before any further `state` access below.
    state.explorer_views = explorer_views;
    state.dag_graph_views = dag_views;

    // (ADR-0059) 렌더 루프 중 쌓인 explorer mirror list_dir 요청을 engine 큐로
    // 옮긴다 — 루프 안에서는 `engine` 이 이미 `ws`/`pane`/`tab`/`surface` 로 배타 차용
    // 중이라 직접 push 할 수 없다(outbox 패턴, `pending_explorer_action` 과 동형).
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

    // T9: 비-terminal surface 컨텍스트 메뉴의 **단일 생산자**(release 시점).
    emit_surface_menu_fallback(state, ctx, &infos);
}

/// 비-terminal surface(explorer/empty/markdown/image/webview/remote) 우클릭 →
/// surface 컨텍스트 메뉴(잘라내기/여기로 이동 + copy surface id)의 **단일 생산자**.
///
/// winit `mouse.rs` 경로는 terminal 전용으로 축소됐고(비-terminal 은 위임만 함), 이
/// egui 프레임이 release 시점 `secondary_clicked()` 로 비-terminal 컨텍스트 메뉴를
/// 유일하게 생산한다. 전역 포인터 상태만 읽어(별도 click-sense 위젯을 덧대지 않아
/// markdown 링크·explorer 버튼 등 내부 상호작용을 가로채지 않음) 비-terminal 패널
/// rect 안의 secondary click 을 잡는다.
///
/// `is_none()` 가드는 explorer 를 위해 유지한다: explorer 는 이 호출 앞선 line 206
/// `apply_explorer_action` 이 위치별 `Explorer`/`ExplorerFavorite` 메뉴를 먼저 슬롯에
/// 선점하므로, 여기 fallback 은 이미 설정됨을 보고 건너뛴다("winit 이 먼저"가 아니라
/// "explorer apply 가 먼저"). 한 프레임 한 메뉴(중복 발화 없음). 패널 rect 는 logical
/// px, interact_pos 도 logical.
///
/// **A-1(explorer-aware fallback):** catch-all(`draw_explorer` line 167-185)이 어떤
/// 이유로 explorer 슬롯을 못 세우고 이 fallback 이 발화하더라도, explorer surface
/// (`info.explorer_cwd.is_some()`) 위에서는 generic `Surface` 메뉴 대신 항상 빈영역
/// `Explorer` 메뉴를 세운다. 그래서 explorer 위에는 절대 "터미널 ID 복사" 같은
/// surface-op 메뉴가 노출되지 않고(불가침 §1·§2), 사용자는 항상 explorer 메뉴를
/// 받는다. OS 무관 순수 로직이라 `#[cfg]` 불필요 — explorer 위 generic 메뉴는 원래
/// 어느 OS 에서도 뜨면 안 되므로 macOS 정상 경로 회귀도 불가능하다.
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
            // A-1: explorer surface 위에서는 generic `Surface` 메뉴("터미널 ID
            // 복사"/"잘라내기")를 절대 세우지 않는다 — catch-all 이 어떤 이유로
            // explorer 슬롯을 못 세워도 항상 빈영역 explorer 메뉴를 세운다(kind-blind
            // 구멍 원천 차단, 불가침 §1·§2). 빈영역 메뉴는 catch-all 의 `T::Empty`
            // 분기(paths 빈 vec + cwd = current_root + single_is_dir=false)와 동일.
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
            // (ADR-0059 Decision 3) 원격 mirror explorer 는 browse-only — 파일 내용
            // fetch(더블클릭 열기)는 스코프 밖이라 트리거하지 않고 toast 로 안내한다.
            // 로컬 surface 는 기존과 동일하게 동작한다.
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
            // 대상 패널에 반영하고, "마지막 view mode" 를 Settings 에 영속한다 —
            // 새로 생성되는 explorer 가 이 형태로 열리도록(재시작/새 창은 disk 로드).
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
            // explorer 전용 메뉴를 단일 슬롯에 선점 → 이후 generic surface fallback
            // (egui_panels 의 secondary_pos 루프)은 이미 설정됨을 보고 건너뛴다.
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
            // 사용자가 이동 확정한 디렉토리를 "최근 디렉토리" 후보로 적재(주소창 자동완성).
            // `A::Navigate` 는 주소창 타이핑·트리·즐겨찾기 클릭 등 실제 사용자 입력에서만
            // emit 되고, 에이전트 IPC 의 cwd 변경은 다른 경로(`set_explorer_cwd`)라 자연히
            // 제외된다(identity 경계). 중복/최신순/상한은 `RecentFiles::add` 가 처리.
            if let A::Navigate(p) = &act {
                state
                    .recent_files
                    .add(EXPLORER_RECENT_KIND, p.display().to_string());
            }
            // cwd/내부 탭이 바뀔 수 있는 액션은 주소창 편집을 취소한다 — surface 단위
            // `ExplorerView` 의 addr 버퍼가 다른 내부 탭/경로로 새지 않도록(다음 sync 가
            // 새 cwd 로 재동기화). SetViewMode/SetSort 는 cwd 불변이라 제외.
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
            }
        }
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

/// 점유된 surface 의 **tier 별 테두리 + force-detach 오버레이**(ADR-0040 / 작업 02).
///
/// 점유 tier 를 색으로 구분해 1px 테두리로 표시한다(하나의 시각 채널 = surface 테두리):
/// - **soft**(협조 신호, write 제한 없음) → green(`accent-occupied-soft`), force-detach 없음.
/// - **hard**(readonly + mirror-observe, 기존 remote-attach 흡수) → peach
///   (`accent-occupied-hard`) + 우상단 force-detach 버튼.
///
/// **focus 와 무관**하게 점유 중이면 항상 그린다(focus 해도 사라지지 않음). readonly
/// 정정으로 내용은 보이므로(render_pass/egui), 테두리는 *점유 표식* + (hard 한정)
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

    /// 점유 surface 의 logical rect + tier(읽기 단계 수집물).
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
                // content-hidden(workspace 점유 멤버)은 ADR-0040 상 hard 계열이라 lock
                // 유무와 무관하게 hard(peach)로 표시한다. 그 외에는 occupancy_of 의 tier
                // 로 분기: hard=peach, soft=green(협조 마커). 점유 아니면 skip.
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

    // tier 별 1px 테두리 — surface 전체를 덮는 interactable Area 대신 순수 페인트
    // (`ctx.layer_painter`)로 그린다. `divider.rs`의 `draw_pane_dividers`/
    // `draw_surface_highlights_view`와 동일 패턴: `Areas::layer_id_at`의 순회
    // 대상(interactable 레이어)에 아예 안 잡히므로 점유 surface 위 마우스
    // 클릭/드래그/휠을 막지 않는다(egui 0.31.1 `memory/mod.rs`의 `Areas::layer_id_at`
    // 순회 로직으로 확인) — interactable Area 로 그렸다면 이 장식용 테두리가
    // 자체적으로 hover/hit-test 를 가로채, 점유 표시와 무관하게 그 위치의 마우스
    // 입력(surface 조작·인접 divider 드래그)을 부수적으로 막아버렸을 것이다.
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("occupied_overlays_border"),
    ));
    for o in &occ {
        // soft=green(협조 신호), hard=peach(readonly). 둘 다 Theme 토큰(하드코딩 없음).
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

    // force-detach 버튼은 hard 점유(readonly)에서만. soft 는 협조 신호라 회수
    // 진입점 없음. 버튼은 실제 클릭 가능한 위젯이라 surface 전체가 아닌 버튼
    // 크기에 딱 맞는 작은 Area로 분리 — 이 Area만 interactable로 남는다.
    // 클릭은 deferred 적용(engine 가변 차용).
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
        // tier 공용 해제(ADR-0040): hard(workspace 멤버·surface lock) 든 soft 든 로컬
        // 사용자가 끊는다. workspace 점유면 멤버 일괄(D6), soft 는 holder 통지 없이 제거.
        engine.release_occupancy(sid);
    }
}
