//! 파일 탐색기 렌더링. 모델의 탐색 상태와 ExplorerView의 목록·선택으로 화면을 그린다.
//! 렌더 중에는 engine을 다시 가변 대여할 수 없어 사용자 동작을 모아 호출부에서 처리한다.

pub mod address;
mod commands;
mod create;
mod find;
mod preview;
mod state_screen;
mod thumbs;
pub mod type_ahead;
pub mod view;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tasty_type_geometry::length::LogicalPx;

use tasty_model::{ExplorerPanel, ExplorerViewMode, SortColumn, SortDir};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    PathField, PathFieldOutcome, Table, TableSortDir, tree_row, tree_row_matching,
};

use crate::adapters::ui::icons::{self, Icon};
use crate::core::explorer_favorites as favorites;
use crate::i18n::{t, t_fmt};
use crate::settings::EffectiveFont;
use crate::theme;
use view::{DirEntryInfo, ExplorerView, LoadState, human_size};

/// grid 셀 폭.
const CELL_W: LogicalPx = LogicalPx(80.0);
/// 사이드바 폭 (logical px — design `ExpSidebar` width 196).
const SIDEBAR_W: LogicalPx = LogicalPx(196.0);

mod columns;
mod favorites_pin;

/// `draw_explorer` 가 호스트에 위임하는 액션. 렌더 루프 종료 후 적용된다.
#[derive(Clone, Debug)]
pub enum ExplorerAction {
    /// 파일 열기 (`DomainIntent::DispatchFile`).
    OpenFile(PathBuf),
    /// 활성 탭을 디렉토리로 이동.
    Navigate(PathBuf),
    /// 뒤로/앞으로/위로.
    GoBack,
    GoForward,
    GoUp,
    /// 현재 디렉토리 새로고침.
    Refresh,
    /// 표시 모드 변경.
    SetViewMode(ExplorerViewMode),
    /// detail 정렬 컬럼 클릭(같은 컬럼이면 방향 토글).
    SetSort(SortColumn),
    /// 내부 탭 추가.
    NewTab,
    /// 내부 탭 닫기.
    CloseTab(usize),
    /// 내부 탭 선택.
    SelectTab(usize),
    /// 주소창 입력을 이동하지 않았다. 호스트가 이유를 알린다.
    AddressRejected(address::AddressRejection),
    /// 우클릭 컨텍스트 메뉴 요청 — 호스트가 OS 네이티브 메뉴를 띄운다.
    /// 좌표는 logical px (egui interact pos 기준).
    ContextMenu {
        target: ExplorerMenuTarget,
        /// 현재 디렉토리 (빈 영역 대상의 "경로 복사"·붙여넣기 기준).
        cwd: PathBuf,
        x: f32,
        y: f32,
    },
    /// 이름을 확정한 새 항목을 만든다. 대상 폴더는 명령을 시작할 때 정했다.
    Create {
        dir: PathBuf,
        name: String,
        folder: bool,
    },
    /// 좁은 칸에서 접은 명령 묶음의 메뉴. 좌표는 logical px.
    MoreMenu {
        x: f32,
        y: f32,
    },
    /// 파일 작업 표시(진행·대기열·결과 카드)에서 고른 일.
    Ops(view::ops::OpsAction),
}

/// 컨텍스트 메뉴의 대상 — 우클릭 위치/선택 상태에서 결정 (design §3.3 target rule).
#[derive(Clone, Debug)]
pub enum ExplorerMenuTarget {
    /// 빈 영역 → 현재 디렉토리(cwd) 대상.
    Empty,
    /// 단일 파일/폴더.
    Single { path: PathBuf, is_dir: bool },
    /// 다중 선택.
    Multi { paths: Vec<PathBuf> },
    /// 사이드바 즐겨찾기 항목 → "즐겨찾기에서 제거" 전용 메뉴.
    Favorite { path: PathBuf },
}

/// 타입어헤드가 키를 소비해도 되는지 판단하는 데 필요한, 탐색기 바깥에서 오는 값들.
/// 렌더 루프 안에서는 `state`와 `engine`을 읽을 수 없으므로 프레임마다 한 번 계산해
/// 넘긴다. `egui_panels`에서 즐겨찾기와 잘라내기 목록을 미리 꺼내 두는 것과 같은
/// 이유다.
pub struct ExplorerInput<'a> {
    /// 이 surface가 현재 포커스를 가지고 있는지. egui 이벤트 큐는 전역이라, 이 조건이
    /// 없으면 한 번의 입력이 열려 있는 모든 탐색기의 선택을 동시에 움직인다.
    pub focused: bool,
    /// 팝업·모달·입력 다이얼로그가 떠 있는지. 그런 상태에서도 키 입력은 egui로 들어오므로
    /// (`view::main`의 전달 조건이 `overlay_open || egui_surface`다) 여기서 막지 않으면
    /// 뒤에 있는 탐색기의 선택이 사용자 모르게 움직인다.
    pub overlay_open: bool,
    /// 수식 키 없이 단축키로 등록된 영숫자. 그 글자는 단축키가 가져간다
    /// (`type_ahead::unmodified_binding_chars`).
    pub shortcut_chars: &'a HashSet<char>,
}

/// 한 explorer surface 를 그린다. 사용자 조작이 있었으면 첫 액션을 반환.
#[allow(clippy::too_many_arguments)]
pub fn draw_explorer(
    ui: &mut egui::Ui,
    panel: &ExplorerPanel,
    view: &mut ExplorerView,
    font: &EffectiveFont,
    id_suffix: &str,
    favorites: &[favorites::ExplorerFavorite],
    cut_pending: &HashSet<PathBuf>,
    recent_dirs: &[String],
    mirror_ws_id: Option<u32>,
    input: &ExplorerInput<'_>,
) -> Option<ExplorerAction> {
    let th = theme::theme();
    let theme: &Theme = &th;
    let mut action: Option<ExplorerAction> = None;

    apply_type_ahead(ui, view, input);

    ui.set_min_size(ui.available_size());
    // 하위 위젯이 처리하지 않은 우클릭은 탐색기 전체 영역에서 받는다.
    let surface_rect = ui.max_rect();
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

    ui.vertical(|ui| {
        tab_strip(ui, theme, panel, &mut action);
        let remote = mirror_ws_id.is_some();
        toolbar(
            ui,
            theme,
            panel,
            view,
            id_suffix,
            recent_dirs,
            remote,
            &mut action,
        );
        find::bar(ui, theme, view, remote);
        let (sep_rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), theme.border_width.value()),
            egui::Sense::hover(),
        );
        ui.painter().hline(
            sep_rect.x_range(),
            sep_rect.center().y,
            egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
        );

        let (mut side_ui, line, mut body_ui) = columns::split(ui, SIDEBAR_W, theme.border_width);
        sidebar(
            &mut side_ui,
            theme,
            panel,
            view,
            favorites,
            &mut action,
            mirror_ws_id,
        );
        ui.painter().vline(
            line.center().x,
            line.y_range(),
            egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
        );
        content(
            &mut body_ui,
            theme,
            panel,
            view,
            font,
            id_suffix,
            cut_pending,
            &mut action,
        );
    });

    // 탐색기 빈 영역에서도 탐색기 메뉴를 사용한다. 하위 위젯이 처리한 클릭은 건드리지 않는다.
    // 권한 거부 경로는 붙여넣을 수 없어 제외한다.
    if action.is_none() && !matches!(view.state, LoadState::NoPermission) {
        let pos = ui.input(|i| {
            if i.pointer.secondary_clicked() {
                i.pointer.interact_pos()
            } else {
                None
            }
        });
        if let Some(pos) = pos
            && surface_rect.contains(pos)
        {
            action = Some(ExplorerAction::ContextMenu {
                target: ExplorerMenuTarget::Empty,
                cwd: panel.current_root().to_path_buf(),
                x: pos.x,
                y: pos.y,
            });
        }
    }

    action
}

fn tab_strip(
    ui: &mut egui::Ui,
    theme: &Theme,
    panel: &ExplorerPanel,
    action: &mut Option<ExplorerAction>,
) {
    let bar_h = theme.item_height_tab.value();
    let pad_x = theme.spacing_sm.value();
    let gap = theme.spacing_xs.value();
    let icon_xs = theme.icon_glyph_size_xs.value();
    let font = egui::FontId::proportional(theme.font_size_body.value());
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, bar_h), egui::Sense::hover());
    let p = ui.painter_at(rect);

    p.rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());
    p.hline(
        rect.x_range(),
        rect.max.y - theme.border_width.value() * 0.5,
        egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
    );

    let active = panel.active;
    let mut x = rect.min.x;
    for (i, tab) in panel.tabs.iter().enumerate() {
        let is_active = i == active;
        // 탭 라벨은 고정 cwd(프로젝트) 이름 — 현재 폴더는 주소창(PathField)이 보여준다.
        let label = tab
            .cwd
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| tab.cwd.to_string_lossy().to_string());
        let galley =
            ui.fonts(|f| f.layout_no_wrap(label.clone(), font.clone(), egui::Color32::WHITE));
        let tab_w = pad_x + icon_xs + gap + galley.size().x + gap + icon_xs + pad_x;
        let tab_rect =
            egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(tab_w, bar_h));
        let resp = ui.interact(
            tab_rect,
            ui.id().with(("explorer_tab", i)),
            egui::Sense::click(),
        );

        if i > 0 && !is_active {
            ui.painter().vline(
                x,
                tab_rect.y_range(),
                egui::Stroke::new(
                    theme.border_width.value(),
                    theme.separator.to_egui_premultiplied(),
                ),
            );
        }

        if is_active {
            ui.painter()
                .rect_filled(tab_rect, 0.0, theme.bg_panel().to_egui());
            let indicator = egui::Rect::from_min_size(
                tab_rect.min,
                egui::vec2(tab_w, theme.tab_indicator_width().value()),
            );
            ui.painter()
                .rect_filled(indicator, 0.0, theme.accent_primary().to_egui());
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(tab_rect, 0.0, theme.overlay_hover().to_egui_premultiplied());
        }

        let fg = if is_active {
            theme.text_primary().to_egui()
        } else {
            theme.text_muted().to_egui()
        };
        let folder_rect = egui::Rect::from_min_size(
            egui::pos2(tab_rect.min.x + pad_x, tab_rect.center().y - icon_xs / 2.0),
            egui::vec2(icon_xs, icon_xs),
        );
        icons::FOLDER
            .image(icon_xs, theme.text_muted().to_egui())
            .paint_at(ui, folder_rect);
        ui.painter().text(
            egui::pos2(tab_rect.min.x + pad_x + icon_xs + gap, tab_rect.center().y),
            egui::Align2::LEFT_CENTER,
            &label,
            font.clone(),
            fg,
        );

        // close ✕ — 활성 상시 / 비활성 hover. 탭이 1개면 표시하지 않음.
        if panel.tabs.len() > 1 && (is_active || resp.hovered()) {
            let close_rect = egui::Rect::from_min_size(
                egui::pos2(
                    tab_rect.max.x - pad_x - icon_xs,
                    tab_rect.center().y - icon_xs / 2.0,
                ),
                egui::vec2(icon_xs, icon_xs),
            );
            let close_resp = ui.interact(
                close_rect,
                ui.id().with(("explorer_tab_close", i)),
                egui::Sense::click(),
            );
            icons::CLOSE
                .image(icon_xs, theme.text_muted().to_egui())
                .paint_at(ui, close_rect);
            if close_resp.clicked() {
                *action = Some(ExplorerAction::CloseTab(i));
            }
        }

        if resp.clicked() && action.is_none() && !is_active {
            *action = Some(ExplorerAction::SelectTab(i));
        }
        x += tab_w;
    }

    let plus_rect = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(bar_h, bar_h));
    let plus_resp = ui.interact(
        plus_rect,
        ui.id().with("explorer_tab_new"),
        egui::Sense::click(),
    );
    if plus_resp.hovered() {
        ui.painter().rect_filled(
            plus_rect,
            0.0,
            theme.overlay_hover().to_egui_premultiplied(),
        );
    }
    let icon = theme.icon_glyph_size_md.value();
    let icon_rect = egui::Rect::from_center_size(plus_rect.center(), egui::vec2(icon, icon));
    icons::PLUS
        .image(icon, theme.text_secondary().to_egui())
        .paint_at(ui, icon_rect);
    if plus_resp.clicked() && action.is_none() {
        *action = Some(ExplorerAction::NewTab);
    }
}

#[allow(clippy::too_many_arguments)]
fn toolbar(
    ui: &mut egui::Ui,
    theme: &Theme,
    panel: &ExplorerPanel,
    view: &mut ExplorerView,
    id_suffix: &str,
    recent_dirs: &[String],
    remote: bool,
    action: &mut Option<ExplorerAction>,
) {
    let h = theme.item_height_interactive.value() + theme.spacing_sm.value() * 2.0;
    let pad = theme.spacing_sm.value();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_panel().to_egui());

    let inner = rect.shrink2(egui::vec2(pad, pad));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    let tab = panel.active_tab();
    child.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();

        if tool_icon(
            ui,
            theme,
            icons::CHEVRON_LEFT,
            tab.can_go_back(),
            t("explorer.nav.back"),
        ) && action.is_none()
        {
            *action = Some(ExplorerAction::GoBack);
        }
        if tool_icon(
            ui,
            theme,
            icons::CHEVRON_RIGHT,
            tab.can_go_forward(),
            t("explorer.nav.forward"),
        ) && action.is_none()
        {
            *action = Some(ExplorerAction::GoForward);
        }
        if tool_icon(
            ui,
            theme,
            icons::CHEVRON_UP,
            tab.can_go_up(),
            t("explorer.nav.up"),
        ) && action.is_none()
        {
            *action = Some(ExplorerAction::GoUp);
        }
        if tool_icon(ui, theme, icons::REFRESH, true, t("explorer.nav.refresh")) && action.is_none()
        {
            *action = Some(ExplorerAction::Refresh);
        }

        ui.add_space(theme.spacing_sm.value());

        // 보기 모드 버튼 폭을 먼저 확보하고 남은 폭에 주소창을 제한한다.
        let seg_w = seg_toggle_width(theme);
        let gap = theme.spacing_sm.value();
        // 주소창 영역 뒤에 가로 item_spacing이 한 번 더 붙으므로 그만큼도 빼야 토글의 오른쪽 여백이
        // 툴바 padding과 같아진다.
        let cmd_w = commands::reserve(ui, theme, view, remote, rect.width());
        let addr_w =
            (ui.available_width() - seg_w - gap - ui.spacing().item_spacing.x - cmd_w).max(0.0);
        let tab_index = panel.active;
        ui.allocate_ui_with_layout(
            egui::vec2(addr_w, ui.available_height()),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                address_bar(
                    ui,
                    theme,
                    panel.current_root(),
                    view,
                    id_suffix,
                    tab_index,
                    recent_dirs,
                    remote,
                    action,
                )
            },
        );
        ui.add_space(gap);
        commands::show(ui, theme, view, &tab.root, remote, rect.width(), action);
        seg_toggle(ui, theme, tab.view_mode, action);
    });
}

/// view-mode 아이콘 토글의 총 폭(예약용). design SegToggle: pad + 3seg + 2gap + 2border.
fn seg_toggle_width(theme: &Theme) -> f32 {
    let pad = theme.spacing_xs.value();
    let gap = theme.spacing_xs.value();
    let seg = theme.icon_glyph_size_md.value() + theme.spacing_sm.value();
    pad * 2.0 + seg * 3.0 + gap * 2.0 + theme.border_width.value() * 2.0
}

/// grid/list/detail 아이콘 토글 (design `SegToggle`): 컨테이너 surface-raised +
/// border-default 1px + radius, active 세그먼트 = segtoggle-on-bg 채움 + segtoggle-on-fg,
/// inactive = text-muted. tooltip 은 i18n 라벨(텍스트 라벨 제거 대신 aria/tooltip 유지).
fn seg_toggle(
    ui: &mut egui::Ui,
    theme: &Theme,
    mode: ExplorerViewMode,
    action: &mut Option<ExplorerAction>,
) {
    let pad = theme.spacing_xs.value();
    let gap = theme.spacing_xs.value();
    let h = theme.item_height_interactive.value();
    let seg_w = theme.icon_glyph_size_md.value() + theme.spacing_sm.value();
    let icon = theme.icon_glyph_size_md.value();
    let total_w = seg_toggle_width(theme);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total_w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect(
        rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
    let segs = [
        (ExplorerViewMode::Grid, icons::GRID, "explorer.view.grid"),
        (
            ExplorerViewMode::List,
            icons::LIST_VIEW,
            "explorer.view.list",
        ),
        (
            ExplorerViewMode::Detail,
            icons::DETAIL,
            "explorer.view.detail",
        ),
    ];
    let seg_h = h - pad * 2.0;
    let mut sx = rect.min.x + theme.border_width.value() + pad;
    for (m, ic, key) in segs {
        let seg_rect = egui::Rect::from_min_size(
            egui::pos2(sx, rect.center().y - seg_h / 2.0),
            egui::vec2(seg_w, seg_h),
        );
        let resp = ui
            .interact(
                seg_rect,
                ui.id().with(("exp_seg", key)),
                egui::Sense::click(),
            )
            .on_hover_text(t(key));
        let active = m == mode;
        if active {
            ui.painter().rect_filled(
                seg_rect,
                theme.corner_radius_sm.value(),
                theme.segtoggle_on_bg().to_egui(),
            );
        } else if resp.hovered() {
            ui.painter().rect_filled(
                seg_rect,
                theme.corner_radius_sm.value(),
                theme.overlay_hover().to_egui_premultiplied(),
            );
        }
        let fg = if active {
            theme.segtoggle_on_fg().to_egui()
        } else {
            theme.text_muted().to_egui()
        };
        let ir = egui::Rect::from_center_size(seg_rect.center(), egui::vec2(icon, icon));
        ic.image(icon, fg).paint_at(ui, ir);
        if resp.clicked() && !active && action.is_none() {
            *action = Some(ExplorerAction::SetViewMode(m));
        }
        sx += seg_w + gap;
    }
}

/// 경로를 편집해 Enter·Go로 이동하는 공용 PathField. 최근 디렉터리를 후보로 전달한다.
/// 상태는 surface별로, egui ID는 surface·내부 탭별로 구분한다.
#[allow(clippy::too_many_arguments)]
fn address_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    current: &Path,
    view: &mut ExplorerView,
    id_suffix: &str,
    tab_index: usize,
    recent_dirs: &[String],
    remote: bool,
    action: &mut Option<ExplorerAction>,
) {
    let current_str = current.display().to_string();
    let candidates: Vec<&str> = recent_dirs.iter().map(String::as_str).collect();
    let folder_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        icons::FOLDER_OPEN
            .image(rect.height(), c)
            .paint_at(ui, rect);
    };
    let go_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        icons::ARROW_RIGHT
            .image(rect.height(), c)
            .paint_at(ui, rect);
    };
    let salt = format!("explorer_addr_{id_suffix}_{tab_index}");
    let outcome = PathField::new(&salt)
        .placeholder(t("explorer.address.placeholder"))
        .empty_label(t("explorer.address.empty"))
        .leading_icon(&folder_icon)
        .row_icon(&folder_icon)
        .go_icon(&go_icon)
        .go_tooltip(t("explorer.address.go"))
        .show(
            ui,
            theme,
            &mut view.addr_buffer,
            &mut view.addr_editing,
            &mut view.addr_active,
            &candidates,
            &current_str,
        );
    if let PathFieldOutcome::Navigate(input) = outcome
        && action.is_none()
        && let Some(target) = address::resolve(&input, current, remote)
    {
        *action = Some(match target {
            Ok(dir) => ExplorerAction::Navigate(dir),
            Err(why) => ExplorerAction::AddressRejected(why),
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn sidebar(
    ui: &mut egui::Ui,
    theme: &Theme,
    panel: &ExplorerPanel,
    view: &mut ExplorerView,
    favorites: &[favorites::ExplorerFavorite],
    action: &mut Option<ExplorerAction>,
    mirror_ws_id: Option<u32>,
) {
    let full = ui.available_size();
    ui.painter().rect_filled(
        egui::Rect::from_min_size(ui.cursor().min, full),
        0.0,
        theme.bg_sidebar().to_egui(),
    );
    ui.spacing_mut().item_spacing.y = 0.0;

    let current = panel.current_root().to_path_buf();

    // 트리와 하단 즐겨찾기에 독립 스크롤 영역을 주고 즐겨찾기 높이를 먼저 확보한다.
    // 낮은 본문에서는 즐겨찾기를 빼고 트리가 본문 전체를 쓴다.
    let fav_h = favorites_pin::height(theme, full.y);
    let files_h = fav_h.map_or(full.y, |h| {
        (full.y - h - theme.border_width.value()).max(0.0)
    });

    ui.allocate_ui_with_layout(
        egui::vec2(full.x, files_h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.add_space(theme.spacing_xs.value());
            sidebar_caption(ui, theme, t("explorer.sidebar.tree"));
            let root = panel.cwd().to_path_buf();
            egui::ScrollArea::vertical()
                .id_salt("explorer_sidebar_files")
                .auto_shrink([false, false])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    tree_node(ui, theme, view, &root, 0, &current, action, mirror_ws_id);
                });
        },
    );
    let Some(fav_h) = fav_h else {
        return;
    };

    let (sep, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        sep.x_range(),
        sep.center().y,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui_premultiplied(),
        ),
    );

    ui.allocate_ui_with_layout(
        egui::vec2(full.x, fav_h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            sidebar_caption(ui, theme, t("explorer.sidebar.favorites"));
            egui::ScrollArea::vertical()
                .id_salt("explorer_sidebar_favorites")
                .auto_shrink([false, false])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    if favorites.is_empty() {
                        favorites_empty(ui, theme);
                    } else {
                        for fav in favorites {
                            favorite_row(ui, theme, fav, &current, action);
                        }
                    }
                });
        },
    );
}

/// 즐겨찾기 빈 상태 (design `FavoritesEmpty`): 흐린 별 + "No favorites yet" + 힌트.
fn favorites_empty(ui: &mut egui::Ui, theme: &Theme) {
    let inset = theme.spacing_sm.value();
    ui.add_space(theme.spacing_xs.value());
    ui.horizontal(|ui| {
        ui.add_space(inset);
        ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
        let sz = theme.icon_glyph_size_sm.value();
        let (r, _) = ui.allocate_exact_size(egui::vec2(sz, sz), egui::Sense::hover());
        // 즐겨찾기 별 아이콘 톤. 대응 토큰 없음 — 같은 아이콘이 두 곳에서
        // 서로 다른 값을 쓴다(수렴은 디자인 판단).
        const FAV_STAR_ICON_OPACITY: f32 = 0.55;
        icons::STAR
            .image(
                sz,
                theme
                    .text_muted()
                    .to_egui()
                    .gamma_multiply(FAV_STAR_ICON_OPACITY),
            )
            .paint_at(ui, r);
        ui.label(
            egui::RichText::new(t("explorer.sidebar.favorites_empty"))
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
    });
    ui.horizontal_wrapped(|ui| {
        ui.add_space(inset);
        ui.spacing_mut().item_spacing.x = 0.0;
        let hint = t_fmt(
            "explorer.sidebar.favorites_empty_hint",
            t("explorer.context_menu.add_to_favorites"),
        );
        let action_label = t("explorer.context_menu.add_to_favorites");
        let micro = theme.font_size_caption.value();
        if let Some(pos) = hint.find(action_label) {
            let (before, rest) = hint.split_at(pos);
            let after = &rest[action_label.len()..];
            for (seg, muted) in [(before, false), (action_label, true), (after, false)] {
                if seg.is_empty() {
                    continue;
                }
                let color = if muted {
                    theme.text_muted().to_egui()
                } else {
                    theme.text_placeholder().to_egui()
                };
                ui.label(egui::RichText::new(seg).size(micro).color(color));
            }
        } else {
            ui.label(
                egui::RichText::new(hint)
                    .size(micro)
                    .color(theme.text_placeholder().to_egui()),
            );
        }
    });
    ui.add_space(inset);
}

/// 즐겨찾기 한 행. 클릭 → 해당 경로로 이동, 우클릭 → 컨텍스트 메뉴.
/// 별은 채운 별(STAR_FILL) + accent-warning(골드), 현재 폴더면 surface-active 하이라이트.
fn favorite_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    fav: &favorites::ExplorerFavorite,
    current: &Path,
    action: &mut Option<ExplorerAction>,
) {
    let star = icons::STAR_FILL;
    let star_color = theme.accent_warning().to_egui();
    let selected = fav.path == current;
    let resp = tree_row(
        ui,
        theme,
        0,
        false,
        false,
        Some(&|ui, rect, _c| star.image(rect.height(), star_color).paint_at(ui, rect)),
        &fav.label,
        None,
        selected,
    );
    self::view::drag::note_favorite(ui, &fav.path, resp.rect);
    if resp.clicked() && action.is_none() {
        *action = Some(ExplorerAction::Navigate(fav.path.clone()));
    }
    if resp.secondary_clicked() && action.is_none() {
        let pos = ui
            .input(|i| i.pointer.interact_pos())
            .unwrap_or_else(|| resp.rect.center());
        *action = Some(ExplorerAction::ContextMenu {
            target: ExplorerMenuTarget::Favorite {
                path: fav.path.clone(),
            },
            cwd: fav.path.clone(),
            x: pos.x,
            y: pos.y,
        });
    }
}

fn sidebar_caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add_space(theme.spacing_xs.value());
    ui.horizontal(|ui| {
        ui.add_space(theme.spacing_sm.value());
        ui.label(
            egui::RichText::new(text.to_uppercase())
                .font(egui::FontId::monospace(theme.font_size_micro.value()))
                .color(theme.text_muted().to_egui()),
        );
    });
    ui.add_space(theme.spacing_xs.value());
}

/// 재귀 트리 노드. `dir` 자체 행을 그리고, 펼쳐져 있으면 하위 디렉토리도.
/// `current` 와 같은 노드는 surface-active 로 하이라이트(design TreeNode active).
#[allow(clippy::too_many_arguments)]
fn tree_node(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &mut ExplorerView,
    dir: &Path,
    depth: u16,
    current: &Path,
    action: &mut Option<ExplorerAction>,
    mirror_ws_id: Option<u32>,
) {
    let open = view.expanded.contains(dir);
    let has_children = !view.tree_children_of(dir, mirror_ws_id).is_empty();
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.to_string_lossy().to_string());
    let folder = icons::FOLDER;
    let folder_color = theme.text_muted().to_egui();
    let selected = dir == current;
    let resp = tree_row(
        ui,
        theme,
        depth,
        has_children,
        open,
        Some(&|ui, rect, _c| folder.image(rect.height(), folder_color).paint_at(ui, rect)),
        &name,
        None,
        selected,
    );
    self::view::drag::note_tree(ui, dir, resp.rect, open);
    // chevron 영역은 펼치기, 나머지 영역은 해당 경로로 이동한다.
    if resp.clicked() {
        let toggle_zone = resp.rect.left() + depth as f32 * theme.spacing_md.value() + 24.0;
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let is_toggle = has_children && pointer.map(|p| p.x <= toggle_zone).unwrap_or(false);
        if is_toggle {
            if open {
                view.expanded.remove(dir);
            } else {
                view.expanded.insert(dir.to_path_buf());
            }
        } else if action.is_none() {
            *action = Some(ExplorerAction::Navigate(dir.to_path_buf()));
        }
    }
    // 트리 우클릭은 본문 선택을 바꾸지 않고 해당 폴더의 메뉴를 연다.
    if resp.secondary_clicked() && action.is_none() {
        let pos = ui
            .input(|i| i.pointer.interact_pos())
            .unwrap_or_else(|| resp.rect.center());
        *action = Some(ExplorerAction::ContextMenu {
            target: ExplorerMenuTarget::Single {
                path: dir.to_path_buf(),
                is_dir: true,
            },
            cwd: dir.to_path_buf(),
            x: pos.x,
            y: pos.y,
        });
    }
    if open {
        let children: Vec<PathBuf> = view
            .tree_children_of(dir, mirror_ws_id)
            .iter()
            .map(|e| e.path.clone())
            .collect();
        for child in children {
            tree_node(
                ui,
                theme,
                view,
                &child,
                depth + 1,
                current,
                action,
                mirror_ws_id,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn content(
    ui: &mut egui::Ui,
    theme: &Theme,
    panel: &ExplorerPanel,
    view: &mut ExplorerView,
    font: &EffectiveFont,
    id_suffix: &str,
    cut_pending: &HashSet<PathBuf>,
    action: &mut Option<ExplorerAction>,
) {
    let mode = panel.active_tab().view_mode;
    let root = panel.current_root().to_path_buf();

    let status_h = theme.item_height_interactive.value();
    let body_h = (ui.available_height() - status_h).max(0.0);
    let body = ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), body_h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            let ui = &mut preview::split(ui, theme, view, id_suffix);
            if state_screen::show_for(ui, theme, view, &root, action)
                || state_screen::show_find(ui, theme, view, action)
            {
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt(format!("explorer_content_{id_suffix}"))
                .auto_shrink([false, false])
                // 기본 최소 높이(64)는 낮은 칸에서 본문을 늘려 상태줄을 칸 밖으로 민다.
                .min_scrolled_height(0.0)
                .drag_to_scroll(false)
                .show(ui, |ui| match mode {
                    ExplorerViewMode::Grid => {
                        grid_view(ui, theme, view, font, cut_pending, &root, action)
                    }
                    ExplorerViewMode::List => {
                        list_view(ui, theme, view, cut_pending, &root, action)
                    }
                    ExplorerViewMode::Detail => detail_view(
                        ui,
                        theme,
                        panel,
                        view,
                        id_suffix,
                        cut_pending,
                        &root,
                        action,
                    ),
                });
        },
    );

    // 빈 영역 우클릭 → cwd 메뉴 (권한 거부 상태는 제외 — 붙여넣기 불가).
    if !matches!(view.state, LoadState::NoPermission) {
        handle_background_context(ui, view, body.response.rect, &root, action);
    }
    self::view::ops::footer(ui, theme, view, body.response.rect, action, status_line);
}

fn status_line(ui: &mut egui::Ui, theme: &Theme, view: &ExplorerView) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme.item_height_interactive.value()),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
    );
    let text = view.status_text();
    ui.painter().text(
        egui::pos2(rect.left() + theme.spacing_md.value(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );
}

/// 우클릭 대상 확정 후 `ContextMenu` 액션을 만든다 (design §3.3 target rule):
/// 선택 밖 항목이면 그 항목만 선택, 선택 안이면 현재 선택 유지.
fn emit_entry_context(
    view: &mut ExplorerView,
    entry: &DirEntryInfo,
    pos: egui::Pos2,
    root: &Path,
    action: &mut Option<ExplorerAction>,
) {
    if !view.selected.contains(&entry.path) {
        view.select_only(&entry.path);
    }
    let target = if view.selected.len() > 1 {
        let mut paths: Vec<PathBuf> = view.selected.iter().cloned().collect();
        paths.sort();
        ExplorerMenuTarget::Multi { paths }
    } else {
        ExplorerMenuTarget::Single {
            path: entry.path.clone(),
            is_dir: entry.is_dir,
        }
    };
    if action.is_none() {
        *action = Some(ExplorerAction::ContextMenu {
            target,
            cwd: root.to_path_buf(),
            x: pos.x,
            y: pos.y,
        });
    }
}

/// grid/list 엔트리 우클릭 핸들러 (Response 기반). 처리했으면 true.
fn handle_entry_context(
    view: &mut ExplorerView,
    entry: &DirEntryInfo,
    resp: &egui::Response,
    root: &Path,
    action: &mut Option<ExplorerAction>,
) -> bool {
    if !resp.secondary_clicked() {
        return false;
    }
    let pos = resp.interact_pointer_pos().unwrap_or_default();
    emit_entry_context(view, entry, pos, root, action);
    true
}

/// content 빈 영역 우클릭 → cwd 대상 메뉴 (선택 해제).
fn handle_background_context(
    ui: &egui::Ui,
    view: &mut ExplorerView,
    rect: egui::Rect,
    root: &Path,
    action: &mut Option<ExplorerAction>,
) {
    if action.is_some() {
        return;
    }
    let pos = ui.input(|i| {
        if i.pointer.secondary_clicked() {
            i.pointer.interact_pos()
        } else {
            None
        }
    });
    if let Some(pos) = pos
        && rect.contains(pos)
    {
        view.clear_selection();
        *action = Some(ExplorerAction::ContextMenu {
            target: ExplorerMenuTarget::Empty,
            cwd: root.to_path_buf(),
            x: pos.x,
            y: pos.y,
        });
    }
}

/// 단일/토글/범위 선택 처리 (modifiers 반영). 더블클릭이면 열기/이동.
fn handle_entry_interaction(
    ui: &egui::Ui,
    view: &mut ExplorerView,
    entry: &DirEntryInfo,
    resp: &egui::Response,
    action: &mut Option<ExplorerAction>,
) {
    self::view::drag::note(ui, entry, resp.rect);
    if resp.double_clicked() {
        if entry.is_dir {
            if action.is_none() {
                *action = Some(ExplorerAction::Navigate(entry.path.clone()));
            }
        } else if action.is_none() {
            *action = Some(ExplorerAction::OpenFile(entry.path.clone()));
        }
        return;
    }
    if resp.clicked() {
        let mods = ui.input(|i| i.modifiers);
        view.click_select(&entry.path, mods.command || mods.ctrl, mods.shift);
    }
}

/// current 에 부모가 있으면(파일시스템 루트 아님) `..` 상위 이동 대상 경로.
fn parent_nav_target(current: &Path) -> Option<PathBuf> {
    current.parent().map(|p| p.to_path_buf())
}

/// 확장자가 이미지 파일인지 — design 은 이미지 glyph 를 accent-info 로 강조한다.
pub(crate) fn is_image_ext(ext: &str) -> bool {
    matches!(
        ext,
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "svg"
            | "bmp"
            | "ico"
            | "tif"
            | "tiff"
            | "avif"
            | "heic"
    )
}

/// 엔트리의 아이콘 + glyph 색 (design GridCell/DetailRow/ExpListMini):
/// 폴더/파일 = text-muted, 이미지 파일 = IMAGE 아이콘 + accent-info.
fn entry_icon(theme: &Theme, e: &DirEntryInfo) -> (Icon, egui::Color32) {
    if e.is_dir {
        (icons::FOLDER, theme.text_muted().to_egui())
    } else if is_image_ext(&e.ext) {
        (icons::IMAGE, theme.accent_info().to_egui())
    } else {
        (icons::FILE, theme.text_muted().to_egui())
    }
}

/// 이번 프레임에 들어온 문자 입력을 타입어헤드로 처리해 선택할 항목과 스크롤 대상을
/// 정한다.
///
/// 이벤트를 큐에서 빼지는 않는다. 주소창의 `TextEdit`도 같은 큐를 읽지만, 주소창을
/// 편집 중일 때는 아래 `addr_editing` 조건이 이 함수 전체를 막으므로 입력이 두 번
/// 처리되지 않는다. 큐를 직접 건드리면 오히려 다른 위젯의 입력을 삼킬 수 있다.
fn apply_type_ahead(ui: &egui::Ui, view: &mut ExplorerView, input: &ExplorerInput<'_>) {
    // 스크롤 대상은 한 프레임만 유지한다. 남겨두면 매 프레임 다시 스크롤해서, 사용자가
    // 휠로 다른 곳을 보는 동안에도 화면이 끌려간다. 방금 만든 항목은 목록에 나타난 프레임에 한 번 스크롤한다.
    view.scroll_to = view.take_reveal();

    if !input.focused
        || input.overlay_open
        || view.text_input_active()
        || !matches!(view.state, LoadState::Ok)
        || view.entries.is_empty()
    {
        // 조건이 맞지 않는 프레임에서는 버퍼도 비운다. 그대로 두면 다시 돌아왔을 때
        // 예전 접두사가 이어져, 사용자가 입력하지 않은 글자로 검색하게 된다.
        view.type_ahead.reset();
        return;
    }

    let typed: Vec<char> = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Text(text) => type_ahead::type_ahead_char(text),
                _ => None,
            })
            .collect()
    });
    if typed.is_empty() {
        return;
    }

    let shown = view.shown_entries();
    let names: Vec<String> = shown.iter().map(|e| e.name.clone()).collect();
    let now = std::time::Instant::now();
    for ch in typed {
        if input.shortcut_chars.contains(&ch.to_ascii_lowercase()) {
            continue;
        }
        let selected = single_selection_index(view);
        if let Some(i) = view.type_ahead.feed(ch, now, &names, selected) {
            let path = shown[i].path.clone();
            view.select_only(&path);
            view.scroll_to = Some(path);
        }
    }
}

/// 검색을 시작할 선택 인덱스를 구한다. 선택된 항목이 없거나 여러 개면 `None`을
/// 돌려준다. 어느 항목 다음부터 찾을지 정할 수 없으므로 목록 처음부터 찾는다.
fn single_selection_index(view: &ExplorerView) -> Option<usize> {
    if view.selected.len() != 1 {
        return None;
    }
    let sel = view.selected.iter().next()?;
    view.shown_entries().iter().position(|e| &e.path == sel)
}

/// 합성 `..` 엔트리. **렌더 전용** — `view.entries`/선택/상태줄/컨텍스트 메뉴에는 절대
/// 넣지 않는다. 각 뷰가 목록 앞에 특수 행으로 그리고 `Navigate(parent)` 만 emit 한다.
fn dotdot_entry(parent: PathBuf) -> DirEntryInfo {
    DirEntryInfo {
        path: parent,
        name: "..".to_string(),
        is_dir: true,
        size: 0,
        modified: None,
        ext: String::new(),
        link: Default::default(),
    }
}

fn grid_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &mut ExplorerView,
    font: &EffectiveFont,
    cut_pending: &HashSet<PathBuf>,
    root: &Path,
    action: &mut Option<ExplorerAction>,
) {
    ui.add_space(theme.spacing_md.value());
    let entries = view.shown_entries();
    let query = view.find_query().to_owned();
    let parent = parent_nav_target(root).filter(|_| view.search_root().is_none());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing =
            egui::vec2(theme.spacing_md.value(), theme.spacing_md.value());
        if let Some(p) = &parent {
            let dd = dotdot_entry(p.clone());
            let resp = grid_cell(ui, theme, &dd, (false, false), font, None, "");
            if resp.double_clicked() && action.is_none() {
                *action = Some(ExplorerAction::Navigate(p.clone()));
            }
        }
        create::name_row(ui, theme, view, create::Slot::Grid, action);
        for e in &entries {
            let selected = view.selected.contains(&e.path);
            let cut = cut_pending.contains(&e.path);
            let thumb = view.thumbs.texture(ui.ctx(), e);
            let resp = grid_cell(ui, theme, e, (selected, cut), font, thumb.as_ref(), &query);
            let resp = view.hit_tooltip(e, resp);
            if ui.is_rect_visible(resp.rect) {
                view.thumbs.want(e, view.is_remote());
            }
            if view.scroll_to.as_deref() == Some(e.path.as_path()) {
                resp.scroll_to_me(Some(egui::Align::Center));
            }
            if !handle_entry_context(view, e, &resp, root, action) {
                handle_entry_interaction(ui, view, e, &resp, action);
            }
        }
    });
}

/// 그리드 셀. 잘라내기 대기 중에는 전경만 흐리게 하며 선택·호버 배경은 유지한다.
fn grid_cell(
    ui: &mut egui::Ui,
    theme: &Theme,
    e: &DirEntryInfo,
    (selected, cut): (bool, bool),
    font: &EffectiveFont,
    thumb: Option<&egui::TextureHandle>,
    query: &str,
) -> egui::Response {
    let slot = theme.explorer_grid_thumb_size().value();
    let label_font = font.font_size.max(1.0).min(theme.font_size_caption.value());
    let label_line_h = (label_font * 1.3).round();
    // 고정 3줄 예약 — 짧은 이름도 3줄분 높이를 잡아 그리드 행 정렬을 균일하게 유지.
    let label_h = label_line_h * 3.0;
    let cell_h = theme.spacing_sm.value()
        + slot
        + theme.spacing_xs.value()
        + label_h
        + theme.spacing_sm.value();
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(CELL_W.value(), cell_h), egui::Sense::click());
    let p = ui.painter_at(rect);

    if selected {
        p.rect_filled(
            rect,
            theme.corner_radius.value(),
            theme.surface_active().to_egui(),
        );
    } else if resp.hovered() {
        p.rect_filled(
            rect,
            theme.corner_radius.value(),
            theme.overlay_hover().to_egui_premultiplied(),
        );
    }

    let fg_dim = |c: egui::Color32| {
        if cut {
            c.gamma_multiply(theme.cut_pending_opacity())
        } else {
            c
        }
    };
    let (icon, glyph_color) = entry_icon(theme, e);
    let glyph_rect = egui::Rect::from_center_size(
        egui::pos2(
            rect.center().x,
            rect.top() + theme.spacing_sm.value() + slot / 2.0,
        ),
        egui::vec2(slot, slot),
    );
    thumbs::paint_slot(ui, theme, glyph_rect, thumb, icon, fg_dim(glyph_color));

    // 이름은 위에서부터 최대 세 줄로 표시하고 넘치면 끝을 줄인다.
    let label_color = fg_dim(if selected {
        theme.text_primary().to_egui()
    } else {
        theme.text_secondary().to_egui()
    });
    let mut job = find::name_job(
        theme,
        &e.name,
        query,
        egui::FontId::proportional(label_font),
        label_color,
        Some(label_line_h),
    );
    job.halign = egui::Align::Center;
    job.wrap = egui::text::TextWrapping {
        max_width: (CELL_W - theme.spacing_xs.scaled(2.0)).value(),
        max_rows: 3,
        overflow_character: Some('…'),
        ..Default::default()
    };
    let galley = ui.fonts(|f| f.layout_job(job));
    p.galley(
        egui::pos2(
            rect.center().x,
            glyph_rect.bottom() + theme.spacing_xs.value(),
        ),
        galley,
        label_color,
    );

    resp
}

fn list_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &mut ExplorerView,
    cut_pending: &HashSet<PathBuf>,
    root: &Path,
    action: &mut Option<ExplorerAction>,
) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let entries = view.shown_entries();
    let query = view.find_query().to_owned();
    if let Some(p) = parent_nav_target(root).filter(|_| view.search_root().is_none()) {
        let up = icons::FOLDER;
        let resp = tree_row(
            ui,
            theme,
            0,
            false,
            false,
            Some(&|ui, rect, c| up.image(rect.height(), c).paint_at(ui, rect)),
            "..",
            None,
            false,
        );
        if resp.double_clicked() && action.is_none() {
            *action = Some(ExplorerAction::Navigate(p));
        }
    }
    create::name_row(ui, theme, view, create::Slot::List, action);
    for e in &entries {
        let (icon, glyph_color) = entry_icon(theme, e);
        let selected = view.selected.contains(&e.path);
        let cut = cut_pending.contains(&e.path);
        // cut-pending 행은 행 전체를 cut_pending_opacity(50%) 로 디밍(스코프 opacity 로 통째 디밍).
        let resp = ui
            .scope(|ui| {
                if cut {
                    ui.set_opacity(theme.cut_pending_opacity());
                }
                tree_row_matching(
                    ui,
                    theme,
                    0,
                    false,
                    false,
                    Some(&|ui, rect, _c| icon.image(rect.height(), glyph_color).paint_at(ui, rect)),
                    &e.name,
                    &query,
                    None,
                    selected,
                )
            })
            .inner;
        let resp = view.hit_tooltip(e, resp);
        if view.scroll_to.as_deref() == Some(e.path.as_path()) {
            resp.scroll_to_me(Some(egui::Align::Center));
        }
        if !handle_entry_context(view, e, &resp, root, action) {
            handle_entry_interaction(ui, view, e, &resp, action);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn detail_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    panel: &ExplorerPanel,
    view: &mut ExplorerView,
    id_suffix: &str,
    cut_pending: &HashSet<PathBuf>,
    root: &Path,
    action: &mut Option<ExplorerAction>,
) {
    let tab = panel.active_tab();
    let columns = tasty_ui_widgets::explorer_detail_columns(
        theme,
        tasty_ui_widgets::ExplorerDetailHeads {
            name: (t("explorer.column.name"), SortColumn::Name),
            size: (t("explorer.column.size"), SortColumn::Size),
            modified: (t("explorer.column.modified"), SortColumn::Modified),
            kind: (t("explorer.column.type"), SortColumn::Type),
            folder: view.search_root().map(|_| t("explorer.type.folder")),
        },
    );
    let dir = match tab.sort_dir {
        SortDir::Asc => TableSortDir::Asc,
        SortDir::Desc => TableSortDir::Desc,
    };
    let search_root = view.search_root().map(Path::to_path_buf);
    // 하위 폴더 검색 결과는 목록을 대신하므로 `..` 를 두지 않는다.
    let parent = parent_nav_target(root).filter(|_| search_root.is_none());
    let query = view.find_query().to_owned();
    let mut rows: Vec<DirEntryInfo> = Vec::with_capacity(view.entries.len() + 1);
    if let Some(p) = &parent {
        rows.push(dotdot_entry(p.clone()));
    }
    rows.extend(view.create.as_ref().map(|_| create::placeholder_row(root)));
    rows.extend(view.shown_entries());
    let editor_cell = std::cell::Cell::new(None);
    let selected: HashSet<PathBuf> = view.selected.clone();
    let cut: HashSet<PathBuf> = cut_pending.clone();
    // `Table`은 행의 `Response`를 돌려주지 않고 가상 스크롤도 하지 않으므로, 대상 행을
    // 그리는 자리에서 직접 스크롤을 요청한다. 위젯 쪽은 고치지 않는다.
    let scroll_to: Option<PathBuf> = view.scroll_to.clone();
    let out = Table::new(columns)
        .active_sort(tab.sort_column, dir)
        .header_fill(theme.table_header_bg().to_egui())
        // Size 제목 끝을 본문 Size 값처럼 날짜 열에서 띄운다(design DetailHeader paddingRight).
        .header_pad_right(theme.spacing_sm)
        .selectable(true)
        .id_salt(format!("explorer_detail_{id_suffix}"))
        .show(
            ui,
            theme,
            &rows,
            // `..`(name == "..", read_dir 은 이 이름을 반환하지 않음) 는 선택 대상 아님.
            |row: &DirEntryInfo| row.name != ".." && selected.contains(&row.path),
            |ui, th, row, col| {
                // cut-pending 행은 전경(아이콘+텍스트)을 cut_pending_opacity(50%) 로 디밍.
                // Table 이 그리는 선택/hover 배경은 그대로 유지.
                let dim = |c: egui::Color32| {
                    if cut.contains(&row.path) {
                        c.gamma_multiply(th.cut_pending_opacity())
                    } else {
                        c
                    }
                };
                // 하위 폴더 검색은 Name 뒤에 Folder 를 두고 Type 을 뺀다. 열 번호를 평소 배치로 옮긴다.
                let col = match (&search_root, col) {
                    (Some(_), 1) => 4,
                    (Some(_), c) if c > 1 => c - 1,
                    (_, c) => c,
                };
                match col {
                    0 if row.name.is_empty() => editor_cell.set(Some(ui.max_rect())),
                    0 => {
                        self::view::drag::note_row(ui, row);
                        // `..`는 화면에만 있는 행이라 타입어헤드 대상이 아니다. 경로만
                        // 비교하면 상위 폴더와 겹칠 수 있어 이름도 함께 확인한다.
                        if row.name != ".." && scroll_to.as_deref() == Some(row.path.as_path()) {
                            ui.scroll_to_rect(ui.max_rect(), Some(egui::Align::Center));
                        }
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                            let sz = th.icon_glyph_size_md.value();
                            let (rect, _) =
                                ui.allocate_exact_size(egui::vec2(sz, sz), egui::Sense::hover());
                            let (icon, c) = entry_icon(th, row);
                            icon.image(sz, dim(c)).paint_at(ui, rect);
                            // 선택 행 이름만 text-primary, 나머지는 `table-row-fg`로 그린다.
                            let name_fg = if row.name != ".." && selected.contains(&row.path) {
                                th.text_primary()
                            } else {
                                th.table_row_fg()
                            };
                            ui.label(find::name_job(
                                th,
                                &row.name,
                                &query,
                                egui::FontId::proportional(th.font_size_body.value()),
                                dim(name_fg.to_egui()),
                                None,
                            ));
                        });
                    }
                    // 오른쪽 정렬 셀의 앞 여백으로 날짜 열과 간격을 둔다.
                    1 => {
                        ui.add_space(th.spacing_sm.value());
                        let text = if row.name == ".." {
                            String::new()
                        } else {
                            human_size(row.is_dir, row.size)
                        };
                        ui.label(
                            egui::RichText::new(text)
                                .font(egui::FontId::monospace(th.font_size_caption.value()))
                                .color(dim(th.text_muted().to_egui())),
                        );
                    }
                    2 => {
                        let text = if row.name == ".." {
                            String::new()
                        } else {
                            crate::core::fs_list::format_modified(row.modified)
                        };
                        ui.label(
                            egui::RichText::new(text)
                                .font(egui::FontId::monospace(th.font_size_caption.value()))
                                .color(dim(th.text_muted().to_egui())),
                        );
                    }
                    4 => {
                        let folder = search_root
                            .as_deref()
                            .map(|r| find::hit_folder(r, &row.path));
                        ui.label(
                            egui::RichText::new(folder.unwrap_or_default())
                                .font(egui::FontId::monospace(th.font_size_caption.value()))
                                .color(dim(th.text_muted().to_egui())),
                        );
                    }
                    _ => {
                        let text = if row.name == ".." {
                            String::new()
                        } else {
                            type_label(row)
                        };
                        ui.label(
                            egui::RichText::new(text)
                                .size(th.font_size_caption.value())
                                .color(dim(th.text_muted().to_egui())),
                        );
                    }
                }
            },
        );

    if let Some(key) = out.clicked_sort
        && action.is_none()
    {
        *action = Some(ExplorerAction::SetSort(key));
    }
    create::detail_row(ui, theme, view, editor_cell.get(), action);
    if let Some(i) = out.secondary_clicked_row
        && let Some(e) = rows.get(i).filter(|e| !e.name.is_empty())
        && e.name != ".."
    // `..` 는 컨텍스트 메뉴 대상 아님
    {
        let pos = ui
            .input(|inp| inp.pointer.interact_pos())
            .unwrap_or_default();
        emit_entry_context(view, e, pos, root, action);
    }
    if let Some(i) = out.clicked_row
        && let Some(e) = rows.get(i).filter(|e| !e.name.is_empty())
    {
        let dbl = ui.input(|inp| {
            inp.pointer
                .button_double_clicked(egui::PointerButton::Primary)
        });
        if e.name == ".." {
            // `..` 는 상위 이동만 (선택/열기 대상 아님).
            if dbl && action.is_none() {
                *action = Some(ExplorerAction::Navigate(e.path.clone()));
            }
        } else if dbl {
            if e.is_dir {
                if action.is_none() {
                    *action = Some(ExplorerAction::Navigate(e.path.clone()));
                }
            } else if action.is_none() {
                *action = Some(ExplorerAction::OpenFile(e.path.clone()));
            }
        } else {
            let mods = ui.input(|inp| inp.modifiers);
            view.click_select(&e.path, mods.command || mods.ctrl, mods.shift);
        }
    }
}

fn tool_icon(ui: &mut egui::Ui, theme: &Theme, icon: Icon, enabled: bool, tip: &str) -> bool {
    let sz = theme.item_height_interactive.value();
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(sz, sz), sense);
    if enabled && resp.hovered() {
        ui.painter().rect_filled(
            rect,
            theme.corner_radius_sm.value(),
            theme.overlay_hover().to_egui_premultiplied(),
        );
    }
    let glyph = theme.icon_glyph_size_md.value();
    let gr = egui::Rect::from_center_size(rect.center(), egui::vec2(glyph, glyph));
    // disabled 도구 아이콘은 opacity 없이 disabled ink를 쓴다.
    let color = if enabled {
        theme.text_secondary().to_egui()
    } else {
        theme.state_disabled_fg().to_egui()
    };
    icon.image(glyph, color).paint_at(ui, gr);
    let resp = if enabled {
        resp.on_hover_text(tip)
    } else {
        resp
    };
    enabled && resp.clicked()
}

/// 이름 바꾸기 대화상자의 이름 검사. 새 항목 입력과 같은 규칙(빈 이름·금지 글자·앞뒤 공백·예약 이름)이다.
/// 문제가 없으면 `None`, 있으면 보일 문구다. 이미 있는지는 실제 이름 바꾸기가 알린다.
pub(crate) fn rename_name_error(name: &str) -> Option<String> {
    create::check_name(name, cfg!(windows))
        .err()
        .map(|e| e.message(name))
}

/// Properties 와 미리보기 머리의 종류 문구. 폴더는 "Folder", 파일은 [`file_kind_word`] 다.
/// Detail 의 Type 열은 좁은 열이라 [`type_label`] 의 확장자를 그대로 쓴다.
pub(crate) fn kind_word(e: &DirEntryInfo) -> String {
    if e.is_dir {
        t("explorer.type.folder").to_string()
    } else {
        file_kind_word(&e.ext)
    }
}

/// 파일 종류를 낱말로: 그림은 "PNG image", 그 밖은 "ZIP file", 확장자가 없으면 "File".
/// 번역문은 이름 붙은 `{type}`·`{ext}` 자리로 대문자 확장자를 받는다.
pub(crate) fn file_kind_word(ext: &str) -> String {
    if ext.is_empty() {
        return t("explorer.type.file").to_string();
    }
    let upper = ext.to_uppercase();
    if is_image_ext(&ext.to_lowercase()) {
        t("explorer.kind.image").replace("{type}", &upper)
    } else {
        t("explorer.kind.ext_file").replace("{ext}", &upper)
    }
}

pub(crate) fn type_label(e: &DirEntryInfo) -> String {
    if e.is_dir {
        t("explorer.type.folder").to_string()
    } else if e.ext.is_empty() {
        t("explorer.type.file").to_string()
    } else {
        e.ext.to_uppercase()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    /// 낮은 칸에 탐색기를 한 번 그리고 상태줄 글자의 사각형과 그 글자를 자르는 사각형을 돌려준다.
    fn status_rect_in_short_cell(
        state: super::LoadState,
        entries: usize,
        cell_h: f32,
    ) -> Option<(egui::Rect, egui::Rect)> {
        // 상태줄 문구를 그릴 때와 찾을 때 같은 번역을 쓰도록 그리기 전에 전역 번역을 고정한다.
        crate::i18n::init("en");
        let ctx = egui::Context::default();
        let cell = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, cell_h));
        let panel = tasty_model::ExplorerPanel::new(1, PathBuf::from("/tasty-test-no-such-dir"));
        let mut view = super::ExplorerView::new();
        view.state = state;
        view.entries = (0..entries)
            .map(|i| super::DirEntryInfo {
                path: PathBuf::from(format!("/tasty-test-no-such-dir/file-{i}.txt")),
                name: format!("file-{i}.txt"),
                is_dir: false,
                size: 0,
                modified: None,
                ext: "txt".into(),
                link: Default::default(),
            })
            .collect();
        let font = crate::settings::EffectiveFont {
            font_family: String::new(),
            font_size: 13.0,
            custom_font_path: String::new(),
            line_height: 1.0,
            font_scale_mode: String::new(),
        };
        let shortcut_chars = std::collections::HashSet::new();
        let input = super::ExplorerInput {
            focused: false,
            overlay_open: false,
            shortcut_chars: &shortcut_chars,
        };
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(cell),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        super::draw_explorer(
                            ui,
                            &panel,
                            &mut view,
                            &font,
                            "short",
                            &[],
                            &std::collections::HashSet::new(),
                            &[],
                            None,
                            &input,
                        );
                    });
            },
        );
        let status = crate::i18n::t_fmt("explorer.status.items", &entries.to_string());
        out.shapes.iter().find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == status => {
                Some((text.visual_bounding_rect(), clipped.clip_rect))
            }
            _ => None,
        })
    }

    /// 사이드바가 칸보다 커지는 낮은 칸에서도 상태줄은 칸 안에 보인다.
    #[test]
    fn kinds_read_as_words() {
        crate::i18n::init("en");
        assert_eq!(super::file_kind_word("png"), "PNG image");
        assert_eq!(super::file_kind_word("zip"), "ZIP file");
        assert_eq!(super::file_kind_word(""), "File");
    }

    #[test]
    fn a_short_cell_keeps_the_status_line_inside() {
        // 목록이 있으면 ScrollArea 가 본문을 채운다. 없으면 상태 화면이 채운다.
        // 120 은 본문이 ScrollArea 기본 최소 높이(64)보다 낮아지는 칸이다.
        for cell_h in [157.0, 120.0] {
            for (state, entries) in [
                (super::LoadState::NoPermission, 0),
                (super::LoadState::Ok, 0),
                (super::LoadState::Ok, 30),
            ] {
                let label = format!("{cell_h} {state:?} {entries}");
                let (rect, clip) = status_rect_in_short_cell(state, entries, cell_h)
                    .unwrap_or_else(|| panic!("{label}: 상태줄을 그리지 않았다"));
                assert!(
                    rect.max.y <= cell_h && clip.contains_rect(rect),
                    "{label}: 상태줄이 칸 밖에 있거나 잘린다: {rect:?} clip {clip:?}"
                );
            }
        }
    }
}
