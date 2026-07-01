//! Explorer (내장 파일 관리자) surface 의 egui 렌더링 (T11).
//!
//! `ExplorerPanel` (model) 의 내비게이션 상태 + `ExplorerView` (host view store) 의
//! 디렉토리 캐시/선택을 받아 한 surface 영역을 그린다. 레이아웃은 갤러리 specimen
//! (`explorer_tab_bar` / `explorer_view_cells` / `explorer_context_menu` 등) 의 구조를
//! 1:1 전사하고, 색·치수·폰트는 전부 `Theme` 토큰에서 가져온다(하드코딩 금지).
//!
//! 렌더 중 발생한 사용자 조작은 즉시 적용하지 않고 [`ExplorerAction`] 으로 모아
//! 호출자(`egui_panels`)가 렌더 루프 종료 후 적용한다(markdown/empty 의 deferred
//! action 패턴과 동일 — 렌더 중 `engine`/`state` 가변 차용 충돌 회피).

pub mod favorites;
pub mod ops;
pub mod view;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tasty_model::{ExplorerPanel, ExplorerViewMode, SortColumn, SortDir};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Table, TableAlign, TableColumn, TableColumnWidth, TableSortDir, tree_row};

use crate::adapters::ui::icons::{self, Icon};
use crate::i18n::{t, t_fmt};
use crate::settings::EffectiveFont;
use crate::theme;
use view::{DirEntryInfo, ExplorerView, LoadState, human_size};

// ── grid 셀 치수 (4px 그리드 — explorer_view_cells specimen 과 동일) ──
/// icon-box 한 변 = 64 (4×16).
const ICON_BOX: f32 = 64.0;
/// grid 셀 폭.
const CELL_W: f32 = 80.0;
/// 사이드바 폭 (logical px, 4px 그리드 · design §3.1).
const SIDEBAR_W: f32 = 220.0;

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
    /// 우클릭 컨텍스트 메뉴 요청 — 호스트가 OS 네이티브 메뉴를 띄운다.
    /// 좌표는 logical px (egui interact pos 기준).
    ContextMenu {
        target: ExplorerMenuTarget,
        /// 현재 디렉토리 (빈 영역 대상의 "경로 복사"·붙여넣기 기준).
        cwd: PathBuf,
        x: f32,
        y: f32,
    },
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

/// 한 explorer surface 를 그린다. 사용자 조작이 있었으면 첫 액션을 반환.
#[allow(clippy::too_many_arguments)]
pub fn draw_explorer(
    ui: &mut egui::Ui,
    panel: &mut ExplorerPanel,
    view: &mut ExplorerView,
    font: &EffectiveFont,
    id_suffix: &str,
    favorites: &[favorites::ExplorerFavorite],
    cut_pending: &HashSet<PathBuf>,
) -> Option<ExplorerAction> {
    let th = theme::theme();
    let theme: &Theme = &th;
    let mut action: Option<ExplorerAction> = None;

    ui.set_min_size(ui.available_size());
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

    ui.vertical(|ui| {
        tab_strip(ui, theme, panel, &mut action);
        toolbar(ui, theme, panel, &mut action);
        // toolbar ↔ content 구분선.
        let (sep_rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().hline(
            sep_rect.x_range(),
            sep_rect.center().y,
            egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
        );

        // 본문: 사이드바 | content.
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), ui.available_height()),
            egui::Layout::left_to_right(egui::Align::Min),
            |ui| {
                // 사이드바 (고정폭).
                ui.allocate_ui_with_layout(
                    egui::vec2(SIDEBAR_W, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| sidebar(ui, theme, panel, view, favorites, &mut action),
                );
                // 사이드바 ↔ content 세로 구분선.
                let (vrect, _) = ui.allocate_exact_size(
                    egui::vec2(1.0, ui.available_height()),
                    egui::Sense::hover(),
                );
                ui.painter().vline(
                    vrect.center().x,
                    vrect.y_range(),
                    egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
                );
                // content.
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        content(
                            ui,
                            theme,
                            panel,
                            view,
                            font,
                            id_suffix,
                            cut_pending,
                            &mut action,
                        )
                    },
                );
            },
        );
    });

    action
}

// ── 내부 탭 strip (explorer_tab_bar specimen 전사) ──────────────────────────
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
        // 탭 라벨은 고정 cwd(프로젝트) 이름 — 현재 폴더는 breadcrumb 이 보여준다.
        let label = tab
            .cwd
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| tab.cwd.to_string_lossy().to_string());
        let galley =
            ui.fonts(|f| f.layout_no_wrap(label.clone(), font.clone(), egui::Color32::WHITE));
        // 폭 = pad + folder + gap + label + gap + close + pad (design ExpTab).
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
                egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
            );
        }

        if is_active {
            ui.painter()
                .rect_filled(tab_rect, 0.0, theme.bg_panel().to_egui());
            // 상단 2px accent 인디케이터 (design ExpTab boxShadow inset 0 2px 0).
            let indicator = egui::Rect::from_min_size(
                tab_rect.min,
                egui::vec2(tab_w, theme.tab_indicator_width.value()),
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
        // folder 아이콘 (라벨 앞, 항상 text-muted — design ExpTab).
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

    // 끝 `＋` 새 탭.
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

// ── 툴바: nav 버튼 + breadcrumb + view-mode segmented ──────────────────────
fn toolbar(
    ui: &mut egui::Ui,
    theme: &Theme,
    panel: &ExplorerPanel,
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

        // 주소표시줄 flex:1, view-mode 토글 flex:none (design ExpToolbar). 토글 실제
        // 폭을 예측 계산해 예약하고, 남은 폭을 주소표시줄에 준 뒤 그 안에서 clip →
        // 어떤 경로 길이에서도 서로 침범하지 않는다.
        let seg_w = seg_toggle_width(theme);
        let gap = theme.spacing_sm.value();
        let addr_w = (ui.available_width() - seg_w - gap).max(0.0);
        ui.allocate_ui_with_layout(
            egui::vec2(addr_w, ui.available_height()),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| address_bar(ui, theme, panel.current_root(), action),
        );
        ui.add_space(gap);
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
/// border-default 1px + radius, active 세그먼트 = surface-active bg + text-primary,
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
                theme.surface_active().to_egui(),
            );
        } else if resp.hovered() {
            ui.painter().rect_filled(
                seg_rect,
                theme.corner_radius_sm.value(),
                theme.overlay_hover().to_egui_premultiplied(),
            );
        }
        let fg = if active {
            theme.text_primary().to_egui()
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

/// 주소표시줄 박스 (design `ExpToolbar` address bar + `Crumb`): surface-raised 배경 +
/// border-default 1px + radius 박스, 앞에 folderOpen 아이콘, 크럼 사이 chevron 아이콘.
/// 내용은 박스 폭으로 clip 되어 긴 경로가 view-mode 토글을 침범하지 않는다.
fn address_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    current: &Path,
    action: &mut Option<ExplorerAction>,
) {
    let h = ui.available_height();
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter().rect(
        rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
    let pad = theme.spacing_xs.value();
    let inner = rect.shrink2(egui::vec2(pad + theme.border_width.value(), 0.0));
    // 내용은 inner 로 clip → 어떤 경로 길이도 박스를 넘지 않는다(design overflow clip).
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(inner);
    child.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

    let icon_sm = theme.icon_glyph_size_sm.value();
    let (fi_rect, _) =
        child.allocate_exact_size(egui::vec2(icon_sm, icon_sm), egui::Sense::hover());
    icons::FOLDER_OPEN
        .image(icon_sm, theme.text_muted().to_egui())
        .paint_at(&child, fi_rect);
    child.add_space(theme.spacing_xs.value());

    let font = egui::FontId::proportional(theme.font_size_body.value());
    let mut crumbs: Vec<PathBuf> = current.ancestors().map(|p| p.to_path_buf()).collect();
    crumbs.reverse();
    let crumbs: Vec<PathBuf> = crumbs
        .into_iter()
        .filter(|c| {
            !c.as_os_str().is_empty()
                && c.file_name()
                    .map(|n| !n.is_empty())
                    .unwrap_or_else(|| !c.to_string_lossy().is_empty())
        })
        .collect();
    let last = crumbs.len().saturating_sub(1);
    for (i, c) in crumbs.iter().enumerate() {
        let name = c
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| c.to_string_lossy().to_string());
        let is_last = i == last;
        let galley =
            child.fonts(|f| f.layout_no_wrap(name.clone(), font.clone(), egui::Color32::WHITE));
        let crumb_pad = theme.spacing_xs.value();
        let (crect, resp) = child.allocate_exact_size(
            egui::vec2(galley.size().x + crumb_pad * 2.0, h),
            egui::Sense::click(),
        );
        let color = if is_last || resp.hovered() {
            theme.text_primary().to_egui()
        } else {
            theme.text_secondary().to_egui()
        };
        child.painter().text(
            egui::pos2(crect.min.x + crumb_pad, crect.center().y),
            egui::Align2::LEFT_CENTER,
            &name,
            font.clone(),
            color,
        );
        if resp.clicked() && !is_last && action.is_none() {
            *action = Some(ExplorerAction::Navigate(c.clone()));
        }
        if !is_last {
            let (srect, _) =
                child.allocate_exact_size(egui::vec2(icon_sm, h), egui::Sense::hover());
            let sep_rect = egui::Rect::from_center_size(
                srect.center(),
                egui::vec2(
                    theme.icon_glyph_size_xs.value(),
                    theme.icon_glyph_size_xs.value(),
                ),
            );
            icons::CHEVRON_RIGHT
                .image(
                    theme.icon_glyph_size_xs.value(),
                    theme.text_muted().to_egui().gamma_multiply(0.7),
                )
                .paint_at(&child, sep_rect);
        }
    }
}

// ── 사이드바: 디렉토리 트리 ───────────────────────────────────────────────
fn sidebar(
    ui: &mut egui::Ui,
    theme: &Theme,
    panel: &ExplorerPanel,
    view: &mut ExplorerView,
    favorites: &[favorites::ExplorerFavorite],
    action: &mut Option<ExplorerAction>,
) {
    let full = ui.available_size();
    ui.painter().rect_filled(
        egui::Rect::from_min_size(ui.cursor().min, full),
        0.0,
        theme.bg_sidebar().to_egui(),
    );
    ui.add_space(theme.spacing_xs.value());
    ui.spacing_mut().item_spacing.y = 0.0;

    egui::ScrollArea::vertical()
        .id_salt("explorer_sidebar")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // 즐겨찾기 섹션 (비어있지 않을 때만).
            if !favorites.is_empty() {
                sidebar_caption(ui, theme, t("explorer.sidebar.favorites"));
                for fav in favorites {
                    favorite_row(ui, theme, fav, action);
                }
                ui.add_space(theme.spacing_xs.value());
            }
            // 섹션 캡션.
            sidebar_caption(ui, theme, t("explorer.sidebar.tree"));
            // 트리 루트는 고정 cwd(프로젝트 루트) — current 가 폴더를 오가도 불변.
            let root = panel.cwd().to_path_buf();
            tree_node(ui, theme, view, &root, 0, action);
        });
}

/// 즐겨찾기 한 행. 클릭 → 해당 경로로 이동, 우클릭 → "즐겨찾기에서 제거" 메뉴.
fn favorite_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    fav: &favorites::ExplorerFavorite,
    action: &mut Option<ExplorerAction>,
) {
    let star = icons::STAR;
    let resp = tree_row(
        ui,
        theme,
        0,
        false,
        false,
        Some(&|ui, rect, c| star.image(rect.height(), c).paint_at(ui, rect)),
        &fav.label,
        None,
        false,
        true,
    );
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
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(theme.font_size_micro.value())
            .color(theme.text_muted().to_egui()),
    );
    ui.add_space(theme.spacing_xs.value());
}

/// 재귀 트리 노드. `dir` 자체 행을 그리고, 펼쳐져 있으면 하위 디렉토리도.
fn tree_node(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &mut ExplorerView,
    dir: &Path,
    depth: u16,
    action: &mut Option<ExplorerAction>,
) {
    let open = view.expanded.contains(dir);
    let has_children = !view.tree_children_of(dir).is_empty();
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.to_string_lossy().to_string());
    let folder = icons::FOLDER;
    let resp = tree_row(
        ui,
        theme,
        depth,
        has_children,
        open,
        Some(&|ui, rect, c| folder.image(rect.height(), c).paint_at(ui, rect)),
        &name,
        None,
        false,
        true,
    );
    // 클릭: chevron 영역(좌측)은 펼침 토글, 그 외는 이동. 단순화를 위해 좌측
    // chevron 슬롯폭(≈20) 안이면 토글, 아니면 navigate.
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
    // 트리 폴더 우클릭 → 단일 폴더 컨텍스트 메뉴(우측 목록과 동일 + 새 탭/루트 설정).
    // content 선택집합과 무관하므로 Single target 을 직접 구성(view.selected 미조작).
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
            .tree_children_of(dir)
            .iter()
            .map(|e| e.path.clone())
            .collect();
        for child in children {
            tree_node(ui, theme, view, &child, depth + 1, action);
        }
    }
}

// ── content: grid / list / detail + 상태 + 상태줄 ──────────────────────────
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

    // content 본문(상태줄 높이 제외).
    let status_h = theme.item_height_interactive.value();
    let body_h = (ui.available_height() - status_h).max(0.0);
    let body = ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), body_h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            // 비-Ok 상태(권한/에러)는 중앙 텍스트로.
            match &view.state {
                LoadState::NoPermission => {
                    centered_state(ui, theme, t("explorer.state.no_permission"));
                    return;
                }
                LoadState::Error(_) => {
                    centered_state(ui, theme, t("explorer.state.empty"));
                    return;
                }
                LoadState::Ok => {}
            }
            if view.entries.is_empty() {
                centered_state(ui, theme, t("explorer.state.no_items"));
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt(format!("explorer_content_{id_suffix}"))
                .auto_shrink([false, false])
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
    status_line(ui, theme, view);
}

fn centered_state(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let h = (ui.available_height() - theme.item_height_interactive.value()).max(0.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), h),
        egui::Layout::centered_and_justified(egui::Direction::TopDown),
        |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(theme.font_size_body.value())
                    .color(theme.text_muted().to_egui()),
            );
        },
    );
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
    let n = view.entries.len();
    let sel = view.selected.len();
    let text = if sel > 0 {
        t_fmt("explorer.status.selected", &sel.to_string())
    } else {
        t_fmt("explorer.status.items", &n.to_string())
    };
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
        view.selected.clear();
        view.anchor = None;
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
        if mods.command || mods.ctrl {
            view.toggle_select(&entry.path);
        } else {
            view.select_only(&entry.path);
        }
    }
}

/// current 에 부모가 있으면(파일시스템 루트 아님) `..` 상위 이동 대상 경로.
fn parent_nav_target(current: &Path) -> Option<PathBuf> {
    current.parent().map(|p| p.to_path_buf())
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
    let entries = view.entries.clone();
    let parent = parent_nav_target(root);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing =
            egui::vec2(theme.spacing_md.value(), theme.spacing_md.value());
        // 목록 최상단 `..` 특수 셀 (파일시스템 루트 아닐 때).
        if let Some(p) = &parent {
            let dd = dotdot_entry(p.clone());
            let resp = grid_cell(ui, theme, &dd, false, false, font);
            if resp.double_clicked() && action.is_none() {
                *action = Some(ExplorerAction::Navigate(p.clone()));
            }
        }
        for e in &entries {
            let selected = view.selected.contains(&e.path);
            let cut = cut_pending.contains(&e.path);
            let resp = grid_cell(ui, theme, e, selected, cut, font);
            if !handle_entry_context(view, e, &resp, root, action) {
                handle_entry_interaction(ui, view, e, &resp, action);
            }
        }
    });
}

/// grid 셀 한 개 (explorer_view_cells specimen 전사).
///
/// `cut` 이면 전경(아이콘 글리프 + 라벨)을 `opacity_cut`(50%) 로 디밍한다 — 선택/hover
/// 배경은 그대로 두어 cut+selected 조합도 선택이 또렷이 보이게 한다(design cell-state
/// matrix "cut (50% opacity) until paste").
fn grid_cell(
    ui: &mut egui::Ui,
    theme: &Theme,
    e: &DirEntryInfo,
    selected: bool,
    cut: bool,
    font: &EffectiveFont,
) -> egui::Response {
    let label_h = theme.font_size_body.value() + theme.spacing_xs.value();
    let cell_h = ICON_BOX + theme.spacing_sm.value() + label_h + theme.spacing_sm.value() * 2.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(CELL_W, cell_h), egui::Sense::click());
    let p = ui.painter_at(rect);

    if selected {
        p.rect_filled(
            rect,
            theme.corner_radius.value(),
            theme.surface_active().to_egui(),
        );
        p.rect_stroke(
            rect,
            theme.corner_radius.value(),
            egui::Stroke::new(theme.border_width.value(), theme.accent_primary().to_egui()),
            egui::StrokeKind::Inside,
        );
    } else if resp.hovered() {
        p.rect_filled(
            rect,
            theme.corner_radius.value(),
            theme.overlay_hover().to_egui_premultiplied(),
        );
    }

    let box_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.center().x - ICON_BOX / 2.0,
            rect.top() + theme.spacing_sm.value(),
        ),
        egui::vec2(ICON_BOX, ICON_BOX),
    );
    p.rect_filled(
        box_rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
    );
    let glyph = theme.icon_glyph_size_md.value() + theme.spacing_sm.value();
    let glyph_rect = egui::Rect::from_center_size(box_rect.center(), egui::vec2(glyph, glyph));
    let icon = if e.is_dir { icons::FOLDER } else { icons::FILE };
    // cut-pending 셀은 전경을 opacity_cut(50%) 로 디밍.
    let fg_dim = |c: egui::Color32| {
        if cut {
            c.gamma_multiply(theme.opacity_cut())
        } else {
            c
        }
    };
    let glyph_color = if e.is_dir {
        theme.accent_primary().to_egui()
    } else {
        theme.text_secondary().to_egui()
    };
    icon.image(glyph, fg_dim(glyph_color))
        .paint_at(ui, glyph_rect);

    p.text(
        egui::pos2(
            rect.center().x,
            box_rect.bottom() + theme.spacing_sm.value() + label_h / 2.0,
        ),
        egui::Align2::CENTER_CENTER,
        truncate(&e.name, 12),
        egui::FontId::proportional(font.font_size.max(1.0).min(theme.font_size_body.value())),
        fg_dim(theme.text_primary().to_egui()),
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
    let entries = view.entries.clone();
    // 목록 최상단 `..` 특수 행 (파일시스템 루트 아닐 때).
    if let Some(p) = parent_nav_target(root) {
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
            true,
        );
        if resp.double_clicked() && action.is_none() {
            *action = Some(ExplorerAction::Navigate(p));
        }
    }
    for e in &entries {
        let icon = if e.is_dir { icons::FOLDER } else { icons::FILE };
        let selected = view.selected.contains(&e.path);
        let cut = cut_pending.contains(&e.path);
        // cut-pending 행은 행 전체를 opacity_cut(50%) 로 디밍(tree_row 가 색을 내부
        // 계산하므로 셀처럼 전경 색만 분리 못 함 → 스코프 opacity 로 통째 디밍).
        let resp = ui
            .scope(|ui| {
                if cut {
                    ui.set_opacity(theme.opacity_cut());
                }
                tree_row(
                    ui,
                    theme,
                    0,
                    false,
                    false,
                    Some(&|ui, rect, c| icon.image(rect.height(), c).paint_at(ui, rect)),
                    &e.name,
                    None,
                    selected,
                    true,
                )
            })
            .inner;
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
    let columns = vec![
        TableColumn {
            title: t("explorer.column.name"),
            width: TableColumnWidth::Remainder {
                at_least: 140.0,
                clip: true,
            },
            align: TableAlign::Left,
            sort_id: Some(SortColumn::Name),
        },
        // design DetailRow gridTemplateColumns: 1fr 80px 132px 92px.
        TableColumn {
            title: t("explorer.column.size"),
            width: TableColumnWidth::Initial {
                initial: 80.0,
                at_least: 64.0,
            },
            align: TableAlign::Right,
            sort_id: Some(SortColumn::Size),
        },
        TableColumn {
            title: t("explorer.column.modified"),
            width: TableColumnWidth::Initial {
                initial: 132.0,
                at_least: 108.0,
            },
            align: TableAlign::Left,
            sort_id: Some(SortColumn::Modified),
        },
        TableColumn {
            title: t("explorer.column.type"),
            width: TableColumnWidth::Initial {
                initial: 92.0,
                at_least: 72.0,
            },
            align: TableAlign::Left,
            sort_id: Some(SortColumn::Type),
        },
    ];
    let dir = match tab.sort_dir {
        SortDir::Asc => TableSortDir::Asc,
        SortDir::Desc => TableSortDir::Desc,
    };
    // `..` 는 렌더 전용 로컬 행으로만 넣는다(`view.entries` 불변). 목록 최상단.
    let parent = parent_nav_target(root);
    let mut rows: Vec<DirEntryInfo> = Vec::with_capacity(view.entries.len() + 1);
    if let Some(p) = &parent {
        rows.push(dotdot_entry(p.clone()));
    }
    rows.extend(view.entries.iter().cloned());
    let selected: HashSet<PathBuf> = view.selected.clone();
    let cut: HashSet<PathBuf> = cut_pending.clone();
    let out = Table::new(columns)
        .active_sort(tab.sort_column, dir)
        .header_fill(theme.bg_sidebar().to_egui())
        .selectable(true)
        .id_salt(format!("explorer_detail_{id_suffix}"))
        .show(
            ui,
            theme,
            &rows,
            // `..`(name == "..", read_dir 은 이 이름을 반환하지 않음) 는 선택 대상 아님.
            |row: &DirEntryInfo| row.name != ".." && selected.contains(&row.path),
            |ui, th, row, col| {
                // cut-pending 행은 전경(아이콘+텍스트)을 opacity_cut(50%) 로 디밍.
                // Table 이 그리는 선택/hover 배경은 그대로 유지.
                let dim = |c: egui::Color32| {
                    if cut.contains(&row.path) {
                        c.gamma_multiply(th.opacity_cut())
                    } else {
                        c
                    }
                };
                match col {
                    0 => {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                            let sz = th.icon_glyph_size_md.value();
                            let (rect, _) =
                                ui.allocate_exact_size(egui::vec2(sz, sz), egui::Sense::hover());
                            let icon = if row.is_dir {
                                icons::FOLDER
                            } else {
                                icons::FILE
                            };
                            let c = if row.is_dir {
                                th.accent_primary().to_egui()
                            } else {
                                th.text_secondary().to_egui()
                            };
                            icon.image(sz, dim(c)).paint_at(ui, rect);
                            ui.label(
                                egui::RichText::new(&row.name)
                                    .size(th.font_size_body.value())
                                    .color(dim(th.text_primary().to_egui())),
                            );
                        });
                    }
                    // Size — mono·11 (font-mono/fontSize 11), 우측 정렬 + 8px 우측 패딩
                    // (design paddingRight 8 → Date 와 시각적 간격). Table 이 Right 컬럼을
                    // right_to_left 로 그리므로 셀 시작의 add_space 가 값을 8px 왼쪽으로 당긴다.
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
                    // Date — mono·11 (design font-mono/fontSize 11).
                    2 => {
                        let text = if row.name == ".." {
                            String::new()
                        } else {
                            fmt_modified(row.modified)
                        };
                        ui.label(
                            egui::RichText::new(text)
                                .font(egui::FontId::monospace(th.font_size_caption.value()))
                                .color(dim(th.text_muted().to_egui())),
                        );
                    }
                    // Type — caption(11) text-muted (design fontSize 12).
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
    if let Some(i) = out.secondary_clicked_row
        && let Some(e) = rows.get(i)
        && e.name != ".."
    // `..` 는 컨텍스트 메뉴 대상 아님
    {
        let pos = ui
            .input(|inp| inp.pointer.interact_pos())
            .unwrap_or_default();
        emit_entry_context(view, e, pos, root, action);
    }
    if let Some(i) = out.clicked_row
        && let Some(e) = rows.get(i)
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
            if mods.command || mods.ctrl {
                view.toggle_select(&e.path);
            } else {
                view.select_only(&e.path);
            }
        }
    }
}

// ── 작은 헬퍼들 ────────────────────────────────────────────────────────────
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
    let color = if enabled {
        theme.text_secondary().to_egui()
    } else {
        theme
            .text_muted()
            .to_egui()
            .gamma_multiply(theme.opacity_disabled())
    };
    icon.image(glyph, color).paint_at(ui, gr);
    let resp = if enabled {
        resp.on_hover_text(tip)
    } else {
        resp
    };
    enabled && resp.clicked()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

/// 수정 시각을 `YYYY-MM-DD` 로. 시스템 시계 의존 포맷은 chrono 없이 epoch 계산.
fn fmt_modified(m: Option<std::time::SystemTime>) -> String {
    let Some(t) = m else { return "—".to_string() };
    let dur = match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d,
        Err(_) => return "—".to_string(),
    };
    let days = dur.as_secs() / 86_400;
    // 1970-01-01 기준 일수 → (y, m, d). 윤년 포함 그레고리력.
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}-{mo:02}-{d:02}")
}

/// Howard Hinnant days→civil 알고리즘 (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn type_label(e: &DirEntryInfo) -> String {
    if e.is_dir {
        t("explorer.type.folder").to_string()
    } else if e.ext.is_empty() {
        t("explorer.type.file").to_string()
    } else {
        e.ext.to_uppercase()
    }
}
