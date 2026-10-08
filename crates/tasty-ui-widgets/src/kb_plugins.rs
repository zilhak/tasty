//! 설정 › 단축키 › Plugins 서브탭. 본체와 갤러리가 함께 호출한다.
//!
//! 위에서부터 플러그인 선택 행, `kb-plugin-list-gap`, 명령 행 목록을 쌓는다. 두 행 모두 설정 Row
//! 격자(제목 열 `kb-plugin-title-width` + `kb-plugin-title-gap`)를 따르므로 플러그인 Select와 모든
//! mode Select가 같은 x에서 시작한다. 명령 행은 위아래 `kb-plugin-row-padding-y`, 명령 사이에만
//! 1px `kb-plugin-separator` 선을 둔다. 컨트롤 줄은 최소 높이 `kb-plugin-row-min-height`이며
//! mode Select · slot · Reset(ghost md)이 모두 `kb-plugin-control-height`로 같다. 상속 결과나
//! 키 해석 실패는 줄 아래 caption(`kb-plugin-caption-gap`)으로 붙는다.
//!
//! 이 view는 override를 해석하거나 저장하지 않는다. 표시할 값과 문구는 호출부가 넘기고,
//! 사용자가 바꾼 것만 돌려준다.

use tasty_type_appearance::theme::Theme;

use crate::button::{Button, ButtonVariant};
use crate::chip::{KbdKey, kbd_parts_at, kbd_parts_width, split_keys};
use crate::control::ControlSize;
use crate::input::Input;
use crate::select::select_rect;
use crate::tooltip::{Tooltip, tooltip_hover_delay_elapsed};

/// mode Select 의 선택지 순서. `KbPluginLabels::modes` 와 같은 순서다.
pub const KB_PLUGIN_MODE_INHERIT: usize = 0;
pub const KB_PLUGIN_MODE_CUSTOM: usize = 1;
pub const KB_PLUGIN_MODE_NONE: usize = 2;

/// 서브탭이 쓰는 문구. 본체는 i18n 으로, 갤러리는 시안 문구로 채운다.
#[derive(Clone, Copy)]
pub struct KbPluginLabels<'a> {
    /// 플러그인 선택 행의 제목.
    pub plugin: &'a str,
    /// 단축키를 등록한 플러그인이 없을 때의 한 줄.
    pub empty: &'a str,
    /// mode Select 선택지. Inherit · Custom · None 순서.
    pub modes: [&'a str; 3],
    /// None 모드의 slot 문구.
    pub unassigned: &'a str,
    pub reset: &'a str,
    /// Reset hover tooltip.
    pub reset_hint: &'a str,
    /// 키 입력칸 placeholder.
    pub key_placeholder: &'a str,
    /// 초안 점 hover tooltip.
    pub draft_hint: &'a str,
    /// 해석하지 못한 키 caption 의 앞부분. 뒤에 그 키를 mono 로 붙인다.
    pub unrecognized: &'a str,
    /// 녹화 버튼 대안의 빈 slot 문구.
    pub record: &'a str,
}

/// 명령 행의 slot. 어느 변형인지가 mode Select 의 값을 정한다.
pub enum KbPluginSlot<'a> {
    /// 호스트 동작을 따른다. `caption` 은 해석된 키를 담은 완성 문구다.
    Inherit {
        sources: &'a [&'a str],
        selected: usize,
        caption: &'a str,
    },
    /// 키를 직접 입력한다. `error` 는 해석하지 못한 첫 키다.
    Custom {
        keys: &'a mut String,
        error: Option<&'a str>,
    },
    /// 녹화 버튼 대안(사용자 결정 전 견본). `keys` 는 표시할 첫 조합이다.
    Record { keys: Option<&'a str> },
    /// 단축키 없음.
    Unassigned,
}

impl KbPluginSlot<'_> {
    fn mode(&self) -> usize {
        match self {
            KbPluginSlot::Inherit { .. } => KB_PLUGIN_MODE_INHERIT,
            KbPluginSlot::Custom { .. } | KbPluginSlot::Record { .. } => KB_PLUGIN_MODE_CUSTOM,
            KbPluginSlot::Unassigned => KB_PLUGIN_MODE_NONE,
        }
    }
}

/// 명령 한 행.
pub struct KbPluginRowView<'a> {
    /// 프레임 사이에 위젯 상태를 고정하는 id. 플러그인 id 와 명령 id 로 만든다.
    pub id: egui::Id,
    pub title: &'a str,
    /// 저장값과 다른 초안이 있다(초안 점).
    pub dirty: bool,
    /// 초안이나 저장값에 override 가 있다. 없으면 Reset 이 disabled 다.
    pub overridden: bool,
    pub slot: KbPluginSlot<'a>,
}

/// 명령 한 행에서 사용자가 한 일과 그린 위치.
#[derive(Clone, Copy, Debug, Default)]
pub struct KbPluginRowOutput {
    /// 새로 고른 mode(`KB_PLUGIN_MODE_*`).
    pub mode: Option<usize>,
    /// 새로 고른 상속 소스 인덱스.
    pub source: Option<usize>,
    /// 키 입력칸 내용이 바뀌었다.
    pub keys_changed: bool,
    pub reset: bool,
    pub record_clicked: bool,
    pub rects: KbPluginRowRects,
}

/// 행 안 컨트롤의 rect. 레이아웃 검사가 읽는다.
#[derive(Clone, Copy, Debug)]
pub struct KbPluginRowRects {
    pub title: egui::Rect,
    pub mode: egui::Rect,
    pub slot: egui::Rect,
    pub reset: egui::Rect,
    /// slot 입력칸의 응답 id(키 입력칸일 때만).
    pub key_input: Option<egui::Id>,
}

impl Default for KbPluginRowRects {
    fn default() -> Self {
        Self {
            title: egui::Rect::NOTHING,
            mode: egui::Rect::NOTHING,
            slot: egui::Rect::NOTHING,
            reset: egui::Rect::NOTHING,
            key_input: None,
        }
    }
}

/// 서브탭 전체.
pub struct KbPluginsView<'a> {
    /// 이름순으로 정렬한 플러그인 이름. 비면 빈 상태 한 줄만 그린다.
    pub plugins: &'a [&'a str],
    pub selected: usize,
    /// 선택한 플러그인의 명령 행.
    pub rows: Vec<KbPluginRowView<'a>>,
    pub labels: KbPluginLabels<'a>,
}

#[derive(Debug, Default)]
pub struct KbPluginsOutput {
    /// 새로 고른 플러그인 인덱스.
    pub plugin: Option<usize>,
    /// `rows` 와 같은 순서.
    pub rows: Vec<KbPluginRowOutput>,
    /// 플러그인 Select rect.
    pub picker: Option<egui::Rect>,
}

/// 서브탭을 그린다. `id_salt` 는 같은 화면에 서브탭이 여럿일 때(갤러리) 서로를 구분한다.
pub fn kb_plugins_subtab(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: impl std::hash::Hash,
    view: KbPluginsView<'_>,
) -> KbPluginsOutput {
    let mut out = KbPluginsOutput::default();
    if view.plugins.is_empty() {
        let galley = text_galley(
            ui,
            theme,
            view.labels.empty,
            theme.font_size_caption.value(),
            theme.text_muted().to_egui(),
            ui.available_width(),
        );
        let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
        ui.painter()
            .galley(rect.min, galley, egui::Color32::PLACEHOLDER);
        return out;
    }

    ui.push_id(id_salt, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut selected = view.selected;
        row_grid(ui, theme, |ui, cell| match cell {
            GridCell::Title(width) => {
                title_cell(ui, theme, view.labels.plugin, false, "", width);
            }
            GridCell::Body => {
                // 시안 picker 행은 `alignItems: center` — Select 를 행 최소 높이 안에서 가운데 둔다.
                let row_h = theme.kb_plugin_row_min_height().value();
                let (picked, rect) = ui
                    .allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), row_h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_height(row_h);
                            select_rect(
                                ui,
                                theme,
                                "kb_plugin_picker",
                                &mut selected,
                                view.plugins,
                                theme.kb_plugin_picker_width().value(),
                                true,
                            )
                        },
                    )
                    .inner;
                if picked {
                    out.plugin = Some(selected);
                }
                out.picker = Some(rect);
            }
        });
        ui.add_space(theme.kb_plugin_list_gap().value());

        let count = view.rows.len();
        for (i, row) in view.rows.into_iter().enumerate() {
            let o = command_row(ui, theme, row, &view.labels);
            out.rows.push(o);
            if i + 1 < count {
                separator(ui, theme);
            }
        }
    });
    out
}

/// 한 행의 칸.
enum GridCell {
    /// 제목 열(폭).
    Title(f32),
    /// 제목 열 오른쪽.
    Body,
}

/// 설정 Row 격자 한 줄 — 제목 열 · gap · 나머지. 위아래 padding 은 호출부가 둔다.
/// 두 칸 모두 위를 맞추고, 각 칸 안에서 첫 줄 높이를 `kb-plugin-row-min-height`로 둔다.
fn row_grid(ui: &mut egui::Ui, theme: &Theme, mut cell: impl FnMut(&mut egui::Ui, GridCell)) {
    let title_w = theme.kb_plugin_title_width().value();
    let row_h = theme.kb_plugin_row_min_height().value();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = theme.kb_plugin_title_gap().value();
        ui.allocate_ui_with_layout(
            egui::vec2(title_w, row_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_width(title_w);
                ui.set_min_height(row_h);
                cell(ui, GridCell::Title(title_w));
            },
        );
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), row_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| cell(ui, GridCell::Body),
        );
    });
}

/// 제목 열 — text-secondary 본문 크기, 열 안에서 줄바꿈하고 말줄임하지 않는다.
/// 초안이면 제목 뒤 `space-sm` 에 `kb-plugin-draft-dot` 점을 둔다.
fn title_cell(
    ui: &mut egui::Ui,
    theme: &Theme,
    text: &str,
    dirty: bool,
    draft_hint: &str,
    width: f32,
) -> egui::Rect {
    let gap = theme.spacing_sm.value();
    let dot = theme.kb_plugin_draft_dot_size().value();
    ui.spacing_mut().item_spacing.x = gap;
    let wrap = if dirty { width - dot - gap } else { width };
    let galley = text_galley(
        ui,
        theme,
        text,
        theme.font_size_body.value(),
        theme.text_secondary().to_egui(),
        wrap.max(dot),
    );
    let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    ui.painter()
        .galley(rect.min, galley, egui::Color32::PLACEHOLDER);
    if dirty {
        let (dot_rect, resp) = ui.allocate_exact_size(egui::vec2(dot, dot), egui::Sense::hover());
        ui.painter().circle_filled(
            dot_rect.center(),
            dot * 0.5,
            theme.kb_plugin_draft_dot().to_egui(),
        );
        if tooltip_hover_delay_elapsed(ui.ctx(), theme, resp.id, resp.hovered()) {
            Tooltip::new(draft_hint)
                .id_source(resp.id)
                .show(ui, theme, dot_rect);
        }
    }
    rect
}

fn command_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    row: KbPluginRowView<'_>,
    labels: &KbPluginLabels<'_>,
) -> KbPluginRowOutput {
    let mut out = KbPluginRowOutput::default();
    let pad = theme.kb_plugin_row_padding_y().value();
    let row_h = theme.kb_plugin_row_min_height().value();
    let KbPluginRowView {
        id,
        title,
        dirty,
        overridden,
        mut slot,
    } = row;
    ui.push_id(id, |ui| {
        ui.add_space(pad);
        row_grid(ui, theme, |ui, cell| match cell {
            GridCell::Title(width) => {
                out.rects.title = title_cell(ui, theme, title, dirty, labels.draft_hint, width);
            }
            GridCell::Body => {
                ui.spacing_mut().item_spacing.y = theme.kb_plugin_caption_gap().value();
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), row_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_height(row_h);
                        ui.spacing_mut().item_spacing.x = theme.kb_plugin_control_gap().value();
                        control_line(ui, theme, &mut slot, overridden, labels, &mut out);
                    },
                );
                caption(ui, theme, &slot, labels);
            }
        });
        ui.add_space(pad);
    });
    out
}

/// mode Select · slot · Reset.
fn control_line(
    ui: &mut egui::Ui,
    theme: &Theme,
    slot: &mut KbPluginSlot<'_>,
    overridden: bool,
    labels: &KbPluginLabels<'_>,
    out: &mut KbPluginRowOutput,
) {
    let slot_w = theme.kb_plugin_slot_width().value();
    let h = theme.kb_plugin_control_height().value();
    let current = slot.mode();
    let mut mode = current;
    let (picked, rect) = select_rect(
        ui,
        theme,
        "kb_plugin_mode",
        &mut mode,
        &labels.modes,
        theme.kb_plugin_mode_width().value(),
        true,
    );
    out.rects.mode = rect;
    if picked && mode != current {
        out.mode = Some(mode);
    }

    match slot {
        KbPluginSlot::Inherit {
            sources, selected, ..
        } => {
            let mut s = *selected;
            let (picked, rect) =
                select_rect(ui, theme, "kb_plugin_source", &mut s, sources, slot_w, true);
            out.rects.slot = rect;
            if picked && s != *selected {
                out.source = Some(s);
            }
        }
        KbPluginSlot::Custom { keys, error } => {
            // 응답 id 는 안쪽 TextEdit 것이고 rect 는 바깥 상자다.
            let resp = Input::new()
                .mono(true)
                .invalid(error.is_some())
                .placeholder(labels.key_placeholder)
                .width(slot_w)
                .show(ui, theme, keys);
            out.keys_changed = resp.changed();
            out.rects.key_input = Some(resp.id);
            out.rects.slot = resp.rect;
        }
        KbPluginSlot::Record { keys } => {
            // block 버튼이 slot 폭만 채우도록 slot 크기의 칸 안에서 그린다.
            let resp = ui
                .allocate_ui(egui::vec2(slot_w, h), |ui| {
                    Button::new(if keys.is_some() { "" } else { labels.record })
                        .variant(ButtonVariant::Secondary)
                        .size(ControlSize::Md)
                        .block(true)
                        .show(ui, theme)
                })
                .inner;
            out.rects.slot = resp.rect;
            out.record_clicked = resp.clicked();
            if let Some(k) = keys {
                let parts: Vec<KbdKey<'_>> = split_keys(k).into_iter().map(KbdKey::Text).collect();
                let w = kbd_parts_width(ui.ctx(), theme, &parts).value();
                kbd_parts_at(
                    ui,
                    theme,
                    &parts,
                    resp.rect.center().x + w * 0.5,
                    resp.rect.center().y,
                );
            }
        }
        KbPluginSlot::Unassigned => {
            let galley = text_galley(
                ui,
                theme,
                labels.unassigned,
                theme.font_size_body.value(),
                theme.kb_plugin_none_fg().to_egui(),
                slot_w,
            );
            let (rect, _) = ui.allocate_exact_size(egui::vec2(slot_w, h), egui::Sense::hover());
            let pos = egui::pos2(rect.left(), rect.center().y - galley.size().y * 0.5);
            ui.painter().galley(pos, galley, egui::Color32::PLACEHOLDER);
            out.rects.slot = rect;
        }
    }

    let resp = Button::new(labels.reset)
        .variant(ButtonVariant::Ghost)
        .size(ControlSize::Md)
        .enabled(overridden)
        .show(ui, theme);
    if overridden && tooltip_hover_delay_elapsed(ui.ctx(), theme, resp.id, resp.hovered()) {
        Tooltip::new(labels.reset_hint)
            .id_source(resp.id)
            .show(ui, theme, resp.rect);
    }
    out.reset = resp.clicked();
    out.rects.reset = resp.rect;
}

/// 줄 아래 caption — Inherit 은 해석된 키(muted), 해석 실패는 danger 에 키를 mono 로.
fn caption(ui: &mut egui::Ui, theme: &Theme, slot: &KbPluginSlot<'_>, labels: &KbPluginLabels<'_>) {
    let size = theme.font_size_caption.value();
    let line_height = Some(size * theme.line_height_ui);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    match slot {
        KbPluginSlot::Inherit { caption, .. } => job.append(
            caption,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(size),
                color: theme.kb_plugin_caption_fg().to_egui(),
                line_height,
                ..Default::default()
            },
        ),
        KbPluginSlot::Custom {
            error: Some(key), ..
        } => {
            let color = theme.kb_plugin_error_fg().to_egui();
            job.append(
                labels.unrecognized,
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::proportional(size),
                    color,
                    line_height,
                    ..Default::default()
                },
            );
            job.append(
                key,
                theme.spacing_xs.value(),
                egui::TextFormat {
                    font_id: egui::FontId::monospace(size),
                    color,
                    line_height,
                    ..Default::default()
                },
            );
        }
        _ => return,
    }
    let galley = ui.fonts(|f| f.layout_job(job));
    let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    ui.painter()
        .galley(rect.min, galley, egui::Color32::PLACEHOLDER);
}

fn separator(ui: &mut egui::Ui, theme: &Theme) {
    let bw = theme.border_width.value();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), bw), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        0.0,
        theme.kb_plugin_separator().to_egui_premultiplied(),
    );
}

fn text_galley(
    ui: &egui::Ui,
    theme: &Theme,
    text: &str,
    size: f32,
    color: egui::Color32,
    wrap: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap;
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(size),
            color,
            line_height: Some(size * theme.line_height_ui),
            ..Default::default()
        },
    );
    ui.fonts(|f| f.layout_job(job))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABELS: KbPluginLabels<'static> = KbPluginLabels {
        plugin: "Plugin:",
        empty: "No plugins have registered shortcuts.",
        modes: ["Inherit", "Custom", "None"],
        unassigned: "(Unassigned)",
        reset: "Reset",
        reset_hint: "Clear the override and use the manifest default.",
        key_placeholder: "ctrl+f5",
        draft_hint: "Changed — not saved yet",
        unrecognized: "Unrecognized key:",
        record: "Record shortcut",
    };
    const SOURCES: &[&str] = &["clipboard.copy", "clipboard.paste"];

    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Inherit,
        Custom,
        Record,
        None,
    }

    /// 행마다 (제목, 종류) 로 서브탭을 그리고 마지막 프레임의 출력과 화면 모양을 돌려준다.
    fn draw(
        ctx: &egui::Context,
        theme: &Theme,
        plugin: &str,
        rows: &[(&str, Kind)],
        bufs: &mut [String],
    ) -> (KbPluginsOutput, egui::FullOutput) {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(620.0, 600.0));
        let mut out = None;
        let full = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut bufs_iter = bufs.iter_mut();
                    let views = rows
                        .iter()
                        .enumerate()
                        .map(|(i, (title, kind))| {
                            let buf = bufs_iter.next().expect("행마다 버퍼 하나");
                            KbPluginRowView {
                                id: egui::Id::new((plugin, i)),
                                title,
                                dirty: false,
                                overridden: true,
                                slot: match kind {
                                    Kind::Inherit => KbPluginSlot::Inherit {
                                        sources: SOURCES,
                                        selected: 0,
                                        caption: "Inherited (Ctrl+Shift+C)",
                                    },
                                    Kind::Custom => KbPluginSlot::Custom {
                                        keys: buf,
                                        error: None,
                                    },
                                    Kind::Record => KbPluginSlot::Record {
                                        keys: Some("Ctrl+Shift+H"),
                                    },
                                    Kind::None => KbPluginSlot::Unassigned,
                                },
                            }
                        })
                        .collect();
                    out = Some(kb_plugins_subtab(
                        ui,
                        theme,
                        "test",
                        KbPluginsView {
                            plugins: &["Clipboard Viewer"],
                            selected: 0,
                            rows: views,
                            labels: LABELS,
                        },
                    ));
                });
            },
        );
        (out.expect("그렸다"), full)
    }

    fn settle(theme: &Theme, rows: &[(&str, Kind)]) -> (KbPluginsOutput, egui::FullOutput) {
        let ctx = egui::Context::default();
        let mut bufs = vec![String::from("ctrl+shift+h"); rows.len()];
        // 첫 프레임은 폰트 준비용이다.
        drop(draw(&ctx, theme, "p", rows, &mut bufs));
        draw(&ctx, theme, "p", rows, &mut bufs)
    }

    /// 수정 전 egui 기본 위젯은 ComboBox·TextEdit·small_button 높이가 서로 달랐다.
    #[test]
    fn every_control_on_a_line_shares_the_control_height_and_centre() {
        let theme = tasty_themes::mocha_fallback();
        let h = theme.kb_plugin_control_height().value();
        let rows = [
            ("Open clipboard viewer", Kind::Custom),
            ("Paste last entry as plain text", Kind::Inherit),
            ("Clear history", Kind::None),
            ("Stage hunk", Kind::Record),
        ];
        let (out, _) = settle(&theme, &rows);
        assert_eq!(out.rows.len(), rows.len());
        for (row, (title, _)) in out.rows.iter().zip(rows) {
            let r = row.rects;
            let cy = r.mode.center().y;
            for (name, rect) in [("mode", r.mode), ("slot", r.slot), ("reset", r.reset)] {
                assert!(
                    (rect.height() - h).abs() < 0.5,
                    "{title} {name} height {}",
                    rect.height()
                );
                assert!(
                    (rect.center().y - cy).abs() < 0.5,
                    "{title} {name} centre {} vs {cy}",
                    rect.center().y
                );
            }
        }
    }

    /// 제목 길이와 상관없이 picker 와 모든 mode Select 가 제목 열 + gap 에서 시작한다.
    #[test]
    fn picker_and_mode_selects_start_on_the_title_column() {
        let theme = tasty_themes::mocha_fallback();
        let rows = [
            ("A", Kind::Custom),
            (
                "Paste the last clipboard entry as plain text without formatting",
                Kind::Inherit,
            ),
        ];
        let (out, _) = settle(&theme, &rows);
        let picker_x = out.picker.expect("picker").min.x;
        let col = theme.kb_plugin_title_width().value() + theme.kb_plugin_title_gap().value();
        for row in &out.rows {
            assert!((row.rects.mode.min.x - picker_x).abs() < 0.5);
            assert!((row.rects.mode.min.x - row.rects.title.min.x - col).abs() < 0.5);
            assert!(row.rects.title.width() <= theme.kb_plugin_title_width().value() + 0.5);
        }
        // picker 행(최소 높이) → list gap → 첫 명령 padding 뒤에 첫 컨트롤 줄이 온다.
        let rhythm = theme.kb_plugin_row_min_height().value()
            + theme.kb_plugin_list_gap().value()
            + theme.kb_plugin_row_padding_y().value();
        let first = out.rows[0].rects.mode.min.y - out.picker.expect("picker").min.y;
        assert!((first - rhythm).abs() < 0.5, "{first} vs {rhythm}");
        // 긴 제목은 줄바꿈되어 행이 한 줄 높이보다 높다.
        assert!(out.rows[1].rects.title.height() > out.rows[0].rects.title.height());
    }

    /// 위 행의 mode 가 바뀌어 위젯 수가 달라져도 아래 행 키 입력칸의 id 는 같다.
    #[test]
    fn key_input_id_survives_a_mode_change_above() {
        let theme = tasty_themes::mocha_fallback();
        let ctx = egui::Context::default();
        let mut bufs = vec![String::new(), String::new()];
        drop(draw(
            &ctx,
            &theme,
            "p",
            &[("A", Kind::Custom), ("B", Kind::Custom)],
            &mut bufs,
        ));
        let (before, _) = draw(
            &ctx,
            &theme,
            "p",
            &[("A", Kind::Custom), ("B", Kind::Custom)],
            &mut bufs,
        );
        let (after, _) = draw(
            &ctx,
            &theme,
            "p",
            &[("A", Kind::None), ("B", Kind::Custom)],
            &mut bufs,
        );
        let id = |o: &KbPluginsOutput| o.rows[1].rects.key_input.expect("B 는 입력칸");
        assert_eq!(id(&before), id(&after));
    }

    /// 플러그인을 바꾸면 같은 자리의 입력칸이라도 다른 명령의 것이므로 id 가 달라진다.
    /// 그래야 포커스·커서가 다른 플러그인 명령의 입력칸으로 넘어가지 않는다.
    #[test]
    fn key_input_id_follows_the_command_not_the_position() {
        let theme = tasty_themes::mocha_fallback();
        let ctx = egui::Context::default();
        let mut bufs = vec![String::new()];
        let rows = [("A", Kind::Custom)];
        drop(draw(&ctx, &theme, "x", &rows, &mut bufs));
        let (x, _) = draw(&ctx, &theme, "x", &rows, &mut bufs);
        let (y, _) = draw(&ctx, &theme, "y", &rows, &mut bufs);
        assert_ne!(x.rows[0].rects.key_input, y.rows[0].rects.key_input);
    }

    /// 구분선은 명령 사이에만 있다.
    #[test]
    fn separators_sit_between_commands_only() {
        let theme = tasty_themes::mocha_fallback();
        let sep = theme.kb_plugin_separator().to_egui_premultiplied();
        let rows = [("A", Kind::None), ("B", Kind::None), ("C", Kind::None)];
        let (_, full) = settle(&theme, &rows);
        let count = full
            .shapes
            .iter()
            .filter(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill == sep))
            .count();
        assert_eq!(count, rows.len() - 1);
    }

    /// 플러그인이 없으면 빈 상태 한 줄만 그리고 picker 는 없다.
    #[test]
    fn empty_state_has_no_picker() {
        let theme = tasty_themes::mocha_fallback();
        let ctx = egui::Context::default();
        let mut out = None;
        for _ in 0..2 {
            drop(ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    out = Some(kb_plugins_subtab(
                        ui,
                        &theme,
                        "empty",
                        KbPluginsView {
                            plugins: &[],
                            selected: 0,
                            rows: Vec::new(),
                            labels: LABELS,
                        },
                    ));
                });
            }));
        }
        let out = out.expect("그렸다");
        assert!(out.picker.is_none() && out.rows.is_empty());
    }
}
