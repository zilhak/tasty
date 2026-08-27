//! 워크스페이스 popup — 디자인 `DagWindow` 의 구조 전사.
//!
//! 560 × 460 한 장 안에서 `DrillDown` 이 목록과 단일 DAG 를 **전면 교체**한다.
//! 목록 쪽은 네 밴드(검색+상태 · 워크스페이스 토글 · 스크롤 목록 · 푸터)로
//! 나뉘고 밴드마다 헤어라인이 있다. 디테일 쪽은 캔버스 + 하단 시트 — popup 은
//! 폭이 상세 도킹 임계값(640) 아래라 **항상** 시트다.

use tasty_dag_layout::Orientation;
use tasty_icons as icons;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    Button, ButtonVariant, IconButton, Input, MultiSelectLabels, checkbox, hspace, margin_sym,
    multi_select,
};

use super::detail::Dock;
use super::rows::Entry;
use super::{canvas, detail, rows};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 시안 확정 크기 — `--tasty-dag-popup-width` / `-height`.
fn popup_size(theme: &Theme) -> egui::Vec2 {
    egui::vec2(
        theme.dag_popup_width().value(),
        theme.dag_popup_height().value(),
    )
}

/// back bar 높이 — `--tasty-drilldown-backbar-height`.
fn backbar_height(theme: &Theme) -> f32 {
    theme.drilldown_backbar_height().value()
}

/// 창 껍데기 — 보더 + 타이틀바. 내용 rect 를 돌려준다.
fn chrome(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, title: &str) -> egui::Rect {
    let radius = theme.corner_radius.value();
    ui.painter()
        .rect_filled(rect, radius, theme.bg_panel().to_egui());
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
        egui::StrokeKind::Inside,
    );

    let bar_h = theme.item_height_interactive.value();
    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), bar_h));
    ui.painter()
        .rect_filled(bar, 0.0, theme.bg_sidebar().to_egui());
    ui.painter().hline(
        bar.x_range(),
        bar.max.y,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui_premultiplied(),
        ),
    );

    let pad = theme.spacing_md.value();
    let gap = theme.spacing_sm.value();
    let icon = theme.icon_glyph_size_sm.value();
    icons::GIT_TREE
        .image(icon, theme.text_muted().to_egui())
        .paint_at(
            ui,
            egui::Rect::from_center_size(
                egui::pos2(bar.min.x + pad + icon / 2.0, bar.center().y),
                egui::vec2(icon, icon),
            ),
        );
    ui.painter().text(
        egui::pos2(bar.min.x + pad + icon + gap, bar.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(theme.font_size_body.value()),
        theme.text_primary().to_egui(),
    );
    let close_w = theme.item_height_tab.value();
    let mut close = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(
                egui::pos2(bar.max.x - theme.spacing_sm.value() - close_w, bar.min.y),
                egui::vec2(close_w, bar_h),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    IconButton::new().show(&mut close, theme, &|ui, rect, c| {
        icons::CLOSE.image(rect.height(), c).paint_at(ui, rect);
    });

    egui::Rect::from_min_max(egui::pos2(rect.min.x, bar.max.y), rect.max)
}

/// 목록 뷰 — 검색+상태 · 토글 · 목록 · 푸터.
fn list_view(ui: &mut egui::Ui, theme: &Theme, body: egui::Rect, entries: &[Entry], salt: &str) {
    // `separator` 는 premultiplied 반투명 색 — `to_egui()` 로 읽으면 알파가 한 번 더
    // 곱해져 배경보다 어두워진다(=선이 사라진다).
    let sep = egui::Stroke::new(
        theme.border_width.value(),
        theme.separator.to_egui_premultiplied(),
    );
    let row_h = theme.item_height_interactive.value();
    let filter_h = row_h + theme.spacing_sm.value() * 2.0;
    let toggle_h = theme.checkbox_size().value() + theme.spacing_xs.value() * 2.0;
    let footer_h = theme.item_height_interactive.value() + theme.spacing_sm.value() * 2.0;

    // ── 검색 + 상태 필터 ──
    let filter = egui::Rect::from_min_size(body.min, egui::vec2(body.width(), filter_h));
    let mut fu = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(filter)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    egui::Frame::NONE
        .inner_margin(margin_sym(theme.spacing_md, theme.spacing_sm))
        .show(&mut fu, |ui| {
            let filter_w = theme.field_width_md.value();
            let search_w = (filter.width()
                - theme.spacing_md.value() * 2.0
                - filter_w
                - theme.spacing_sm.value())
            .max(0.0);
            let mut query = String::new();
            Input::new()
                .icon(&|ui, rect, color| {
                    icons::SEARCH.image(rect.height(), color).paint_at(ui, rect);
                })
                .placeholder("Filter DAGs…")
                .width(search_w)
                .show(ui, theme, &mut query);
            hspace(ui, theme.spacing_sm);
            // 본체 popup 과 같은 위젯·같은 어휘 — rollup 6 종(취소/알 수 없음 없음).
            let mut picked = [false; super::ROLLUP_ORDER.len()];
            let labels: Vec<&str> = super::ROLLUP_ORDER.iter().map(|s| s.label()).collect();
            let summary = MultiSelectLabels {
                none: "Any status",
                some: "{} statuses",
                all: "All statuses",
            };
            multi_select(
                ui,
                theme,
                salt,
                &mut picked,
                &labels,
                None,
                &summary,
                filter_w,
                true,
            );
        });
    ui.painter().hline(body.x_range(), filter.max.y, sep);

    // ── "이 워크스페이스만" ──
    let toggle = egui::Rect::from_min_size(
        egui::pos2(body.min.x, filter.max.y),
        egui::vec2(body.width(), toggle_h),
    );
    let mut tu = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(toggle)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    egui::Frame::NONE
        .inner_margin(margin_sym(theme.spacing_md, theme.spacing_xs))
        .show(&mut tu, |ui| {
            let mut on = false;
            checkbox(ui, theme, &mut on, "This workspace only", true);
        });
    ui.painter().hline(body.x_range(), toggle.max.y, sep);

    // ── 목록 ──
    let list = egui::Rect::from_min_max(
        egui::pos2(body.min.x, toggle.max.y),
        egui::pos2(body.max.x, body.max.y - footer_h),
    );
    let mut lu = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(list)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    lu.set_clip_rect(list);
    rows::list(&mut lu, theme, entries, list.width(), salt);

    // ── 푸터 ──
    ui.painter().hline(body.x_range(), list.max.y, sep);
    let footer = egui::Rect::from_min_max(egui::pos2(body.min.x, list.max.y), body.max);
    let pad = theme.spacing_md.value();
    ui.painter().text(
        egui::pos2(footer.min.x + pad, footer.center().y),
        egui::Align2::LEFT_CENTER,
        format!("{} of {} DAGs", entries.len(), entries.len()),
        egui::FontId::monospace(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );
    let btn_w = theme.field_width_md.value() / 2.0;
    let mut bu = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                egui::pos2(footer.max.x - pad - btn_w, footer.min.y),
                egui::pos2(footer.max.x - pad, footer.max.y),
            ))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    Button::new("Close")
        .variant(ButtonVariant::Secondary)
        .show(&mut bu, theme);
}

/// 디테일 뷰 — back bar + 캔버스 + 하단 시트.
fn detail_view(ui: &mut egui::Ui, theme: &Theme, body: egui::Rect, entry: &Entry, salt: &str) {
    let bar_h = backbar_height(theme);
    let bar = egui::Rect::from_min_size(body.min, egui::vec2(body.width(), bar_h));
    ui.painter().hline(
        bar.x_range(),
        bar.max.y,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.drilldown_backbar_border().to_egui_premultiplied(),
        ),
    );
    let pad = theme.drilldown_backbar_padding_x().value();
    let glyph_w = theme.item_height_tab.value();
    ui.painter().text(
        egui::pos2(bar.min.x + pad + glyph_w / 2.0, bar.center().y),
        egui::Align2::CENTER_CENTER,
        "\u{2039}",
        egui::FontId::proportional(theme.font_size_body.value()),
        theme.text_muted().to_egui(),
    );
    ui.painter().text(
        egui::pos2(
            bar.min.x + pad + glyph_w + theme.drilldown_backbar_gap().value(),
            bar.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        &entry.graph.name,
        egui::FontId::proportional(theme.font_size_body.value()),
        theme.text_primary().to_egui(),
    );

    // 상세 시트는 하단 고정 — popup 폭이 640 아래라 우측 패널 분기가 없다.
    let sheet_h = theme
        .dag_detail_sheet_height()
        .value()
        .min((body.height() - bar_h) / 2.0);
    let rest = egui::Rect::from_min_max(egui::pos2(body.min.x, bar.max.y), body.max);
    let canvas_rect =
        egui::Rect::from_min_max(rest.min, egui::pos2(rest.max.x, rest.max.y - sheet_h));
    let sheet_rect =
        egui::Rect::from_min_max(egui::pos2(rest.min.x, rest.max.y - sheet_h), rest.max);

    let mut sel = Some("unit".to_owned());
    // 미니맵은 popup 에서 빠진다 — 560 은 `dag-minimap-min-surface` 아래다.
    canvas::paint(
        ui,
        theme,
        canvas_rect,
        &entry.graph,
        Orientation::TopDown,
        &mut sel,
        false,
    );
    if let Some(id) = sel {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(sheet_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(sheet_rect);
        child.push_id(salt, |ui| {
            detail::draw_docked(ui, theme, &entry.graph, &id, Dock::Sheet, sheet_rect.size());
        });
    }
}

/// popup 한 장.
fn paint(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    entries: &[Entry],
    open: Option<&Entry>,
    salt: &str,
) {
    let body = chrome(ui, theme, rect, "Task DAGs");
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(body);
    child.push_id(salt, |ui| match open {
        Some(entry) => detail_view(ui, theme, body, entry, salt),
        None => list_view(ui, theme, body, entries, salt),
    });
}

/// `dag-window` 섹션 Spec — 목록 뷰와 디테일 뷰를 나란히.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let entries = rows::entries();
    let size = popup_size(theme);
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
            for (salt, open) in [("list", None), ("detail", Some(&entries[0]))] {
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                paint(ui, theme, rect, &entries, open, salt);
            }
        });
    });
    spec::meta(
        ui,
        theme,
        &[
            ("popup", "560 × 460 · movable · resizable"),
            ("titlebar", "28 · gitTree + name + close"),
            ("filter band", "8/12 · search + status"),
            ("toggle band", "4/12 · this workspace only"),
            ("footer", "8/12 · count + Close"),
            ("detail", "back bar + canvas + 220 sheet"),
        ],
        &[
            TokenChip::new("--tasty-dag-popup-width", "560", theme.bg_panel().to_egui()),
            TokenChip::new(
                "--tasty-separator",
                "band hairlines",
                // 칩도 실제로 칠해지는 색을 보여야 한다 — premultiplied 색을
                // `to_egui()` 로 읽으면 알파가 두 번 곱해져 옆 칩과 다른 톤이 된다.
                theme.separator.to_egui_premultiplied(),
            ),
            TokenChip::new(
                "--tasty-drilldown-backbar-border",
                "back bar rule",
                theme.drilldown_backbar_border().to_egui_premultiplied(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "The popup is workspace-scoped: it hides when another workspace becomes active and comes \
         back — with its selection intact — on return. The list itself is not scoped, though; it \
         spans every workspace and labels each row, because \"which workspace did I leave that in\" \
         is the question people actually arrive with.",
    );
    spec::note(
        ui,
        theme,
        "Detail replaces the whole area rather than opening beside it. At 560px there is no room \
         for a side-by-side split that keeps both readable, and the back bar makes the swap \
         cheap to undo.",
    );
}

/// 무대 높이 참고값 — popup 한 장 그대로.
pub const STAGE_HEIGHT: LogicalPx = LogicalPx(460.0);
