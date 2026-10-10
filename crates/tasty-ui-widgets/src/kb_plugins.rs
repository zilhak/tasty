//! 설정 › 단축키 › Plugins 서브탭. 본체와 갤러리가 함께 호출한다.
//!
//! 위에서부터 플러그인 선택 행, `kb-plugin-list-gap`, 명령 행 목록을 쌓는다. 두 행 모두 설정 Row
//! 격자(제목 열 `kb-plugin-title-width` + `kb-plugin-title-gap`)를 따르므로 플러그인 Select와 모든
//! mode Select가 같은 x에서 시작한다. 명령 행은 위아래 `kb-plugin-row-padding-y`, 명령 사이에만
//! 1px `kb-plugin-separator` 선을 둔다. 컨트롤 줄은 최소 높이 `kb-plugin-row-min-height`이며
//! mode Select · slot · Reset(ghost md)이 모두 `kb-plugin-control-height`로 같다. 단 Custom slot 은
//! 다른 단축키 서브탭과 같은 녹화 슬롯([`kb_record_slot`])을 줄 가운데에 쌓으므로 `kb-record-height`다.
//! 상속 결과나 키 해석 실패는 줄 아래 caption(`kb-plugin-caption-gap`)으로 붙는다.
//!
//! 이 view는 override를 해석하거나 저장하지 않는다. 표시할 값과 문구는 호출부가 넘기고,
//! 사용자가 바꾼 것만 돌려준다.

use tasty_type_appearance::theme::Theme;

use crate::button::{Button, ButtonVariant};
use crate::control::ControlSize;
use crate::kb_record::{KbRecordSlot, kb_record_slot};
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
    /// 초안 점 hover tooltip.
    pub draft_hint: &'a str,
    /// 해석하지 못한 키 caption 의 앞부분. 뒤에 그 키를 mono 로 붙인다.
    pub unrecognized: &'a str,
    /// 녹화 중인 슬롯의 안내 문구.
    pub press_key: &'a str,
    /// Custom 인데 키가 하나도 없을 때의 None 슬롯 문구.
    pub no_key: &'a str,
    /// 추가(+) 슬롯 hover tooltip.
    pub add_hint: &'a str,
}

/// 명령 행의 slot. 어느 변형인지가 mode Select 의 값을 정한다.
pub enum KbPluginSlot<'a> {
    /// 호스트 동작을 따른다. `caption` 은 해석된 키를 담은 완성 문구다.
    Inherit {
        sources: &'a [&'a str],
        selected: usize,
        caption: &'a str,
    },
    /// 키를 녹화 슬롯으로 받는다. 바인딩마다 슬롯 하나, 뒤에 추가(+) 슬롯을 둔다.
    /// 키가 없으면 None 슬롯 하나만 둔다.
    Custom {
        /// 표시할 키(호출부가 사용자 표기로 바꾼 것).
        keys: &'a [&'a str],
        /// 이 행에서 녹화 중인 슬롯. `keys.len()` 이면 새 바인딩이다.
        recording: Option<usize>,
        /// 확인 popup 이 떠 있으면 false — 슬롯을 disabled 로 그리고 클릭을 받지 않는다.
        can_record: bool,
        /// 해석하지 못한 첫 키(직접 고친 설정 파일 등).
        error: Option<&'a str>,
    },
    /// 단축키 없음.
    Unassigned,
}

impl KbPluginSlot<'_> {
    fn mode(&self) -> usize {
        match self {
            KbPluginSlot::Inherit { .. } => KB_PLUGIN_MODE_INHERIT,
            KbPluginSlot::Custom { .. } => KB_PLUGIN_MODE_CUSTOM,
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
    /// 누른 녹화 슬롯. `keys.len()` 이면 추가(+) 또는 None 슬롯이다.
    pub record: Option<usize>,
    pub reset: bool,
    pub rects: KbPluginRowRects,
}

/// 행 안 컨트롤의 rect. 레이아웃 검사가 읽는다.
#[derive(Clone, Copy, Debug)]
pub struct KbPluginRowRects {
    pub title: egui::Rect,
    pub mode: egui::Rect,
    pub slot: egui::Rect,
    pub reset: egui::Rect,
    /// Custom 의 녹화 슬롯 rect 들(추가·None 슬롯 포함). `slot` 은 이것들의 합이다.
    pub record_slots: [egui::Rect; MAX_RECORDED_RECTS],
    /// `record_slots` 중 채운 수.
    pub record_slot_count: usize,
}

/// 레이아웃 검사가 읽는 녹화 슬롯 rect 의 최대 수. 그 뒤 슬롯은 `slot` 합에만 들어간다.
pub const MAX_RECORDED_RECTS: usize = 4;

impl Default for KbPluginRowRects {
    fn default() -> Self {
        Self {
            title: egui::Rect::NOTHING,
            mode: egui::Rect::NOTHING,
            slot: egui::Rect::NOTHING,
            reset: egui::Rect::NOTHING,
            record_slots: [egui::Rect::NOTHING; MAX_RECORDED_RECTS],
            record_slot_count: 0,
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
        KbPluginSlot::Custom {
            keys,
            recording,
            can_record,
            ..
        } => record_slots(ui, theme, keys, *recording, *can_record, labels, out),
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

/// Custom 의 녹화 슬롯 줄 — 다른 단축키 서브탭(`entries`)과 같은 슬롯 모양·폭·슬롯 사이 간격.
fn record_slots(
    ui: &mut egui::Ui,
    theme: &Theme,
    keys: &[&str],
    recording: Option<usize>,
    can_record: bool,
    labels: &KbPluginLabels<'_>,
    out: &mut KbPluginRowOutput,
) {
    let width = theme.kb_record_width();
    let add_width = theme.kb_record_add_width();
    let len = keys.len();
    let push = |out: &mut KbPluginRowOutput, resp: &egui::Response, idx: usize| {
        out.rects.slot = out.rects.slot.union(resp.rect);
        if out.rects.record_slot_count < MAX_RECORDED_RECTS {
            out.rects.record_slots[out.rects.record_slot_count] = resp.rect;
            out.rects.record_slot_count += 1;
        }
        if resp.clicked() {
            out.record = Some(idx);
        }
    };
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
        for (idx, key) in keys.iter().enumerate() {
            let slot = if recording == Some(idx) {
                KbRecordSlot::Recording(labels.press_key)
            } else {
                KbRecordSlot::Binding(key)
            };
            let resp = kb_record_slot(ui, theme, slot, width, can_record);
            push(out, &resp, idx);
        }
        // 키가 없는 행은 None 슬롯 하나만 두고 + 를 따로 두지 않는다.
        let (slot, w) = match (recording == Some(len), len) {
            (true, 0) => (KbRecordSlot::Recording(labels.press_key), width),
            (true, _) => (KbRecordSlot::Recording(labels.press_key), add_width),
            (false, 0) => (KbRecordSlot::Empty(labels.no_key), width),
            (false, _) => (KbRecordSlot::Add, add_width),
        };
        let resp = kb_record_slot(ui, theme, slot, w, can_record);
        if can_record && tooltip_hover_delay_elapsed(ui.ctx(), theme, resp.id, resp.hovered()) {
            Tooltip::new(labels.add_hint)
                .id_source(resp.id)
                .show(ui, theme, resp.rect);
        }
        push(out, &resp, len);
    });
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
        draft_hint: "Changed — not saved yet",
        unrecognized: "Unrecognized key:",
        press_key: "Press a key combination...",
        no_key: "None",
        add_hint: "Add a shortcut",
    };
    const SOURCES: &[&str] = &["clipboard.copy", "clipboard.paste"];

    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Inherit,
        /// 키 목록과 녹화 중인 슬롯.
        Custom(&'static [&'static str], Option<usize>),
        None,
    }

    /// 행마다 (제목, 종류) 로 서브탭을 그리고 마지막 프레임의 출력과 화면 모양을 돌려준다.
    fn draw(
        ctx: &egui::Context,
        theme: &Theme,
        plugin: &str,
        rows: &[(&str, Kind)],
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
                    let views = rows
                        .iter()
                        .enumerate()
                        .map(|(i, (title, kind))| KbPluginRowView {
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
                                Kind::Custom(keys, recording) => KbPluginSlot::Custom {
                                    keys,
                                    recording: *recording,
                                    can_record: true,
                                    error: None,
                                },
                                Kind::None => KbPluginSlot::Unassigned,
                            },
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
        // 첫 프레임은 폰트 준비용이다.
        drop(draw(&ctx, theme, "p", rows));
        draw(&ctx, theme, "p", rows)
    }

    const ONE_KEY: &[&str] = &["Ctrl+Shift+H"];

    /// 수정 전 egui 기본 위젯은 ComboBox·TextEdit·small_button 높이가 서로 달랐다.
    /// Custom 의 녹화 슬롯은 다른 서브탭과 같은 `kb-record-height` 이고 같은 줄 가운데에 선다.
    #[test]
    fn every_control_on_a_line_shares_the_control_height_and_centre() {
        let theme = tasty_themes::mocha_fallback();
        let h = theme.kb_plugin_control_height().value();
        let record_h = theme.kb_record_height().value();
        let rows = [
            ("Open clipboard viewer", Kind::Custom(ONE_KEY, None)),
            ("Paste last entry as plain text", Kind::Inherit),
            ("Clear history", Kind::None),
            ("Stage hunk", Kind::Custom(&[], None)),
        ];
        let (out, _) = settle(&theme, &rows);
        assert_eq!(out.rows.len(), rows.len());
        for (row, (title, kind)) in out.rows.iter().zip(rows) {
            let r = row.rects;
            let cy = r.mode.center().y;
            let slot_h = if matches!(kind, Kind::Custom(..)) {
                record_h
            } else {
                h
            };
            for (name, rect, want) in [
                ("mode", r.mode, h),
                ("slot", r.slot, slot_h),
                ("reset", r.reset, h),
            ] {
                assert!(
                    (rect.height() - want).abs() < 0.5,
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
            ("A", Kind::Custom(ONE_KEY, None)),
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

    /// Custom 은 키마다 녹화 슬롯 하나와 추가(+) 슬롯을 두고, 키가 없으면 None 슬롯 하나만 둔다.
    /// 슬롯 폭은 다른 서브탭과 같은 `kb-record-width`·`kb-record-add-width` 이상이고 Reset 은 그 뒤에 온다.
    #[test]
    fn custom_keys_become_record_slots_with_one_trailing_slot() {
        let theme = tasty_themes::mocha_fallback();
        let width = theme.kb_record_width().value();
        let add = theme.kb_record_add_width().value();
        let rows = [
            ("two", Kind::Custom(&["Ctrl+A", "Ctrl+B"], None)),
            ("none", Kind::Custom(&[], None)),
            ("adding", Kind::Custom(ONE_KEY, Some(1))),
        ];
        let (out, _) = settle(&theme, &rows);
        let slots = |i: usize| {
            let r = out.rows[i].rects;
            r.record_slots[..r.record_slot_count].to_vec()
        };
        let two = slots(0);
        assert_eq!(two.len(), 3);
        assert!(two[0].width() >= width - 0.5 && two[1].width() >= width - 0.5);
        assert!((two[2].width() - add).abs() < 0.5, "{}", two[2].width());
        // 슬롯 사이는 다른 서브탭의 한 행 안 간격(space-xs)이다.
        let gap = theme.spacing_xs.value();
        assert!((two[1].min.x - two[0].max.x - gap).abs() < 0.5);
        assert!(out.rows[0].rects.reset.min.x > two[2].max.x);

        let none = slots(1);
        assert_eq!(none.len(), 1);
        assert!(none[0].width() >= width - 0.5);

        // 새 바인딩을 녹화 중이면 추가 슬롯 자리가 안내 문구로 넓어진다.
        let adding = slots(2);
        assert_eq!(adding.len(), 2);
        assert!(adding[1].width() > add + 0.5, "{}", adding[1].width());
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
