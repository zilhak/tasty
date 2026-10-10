//! 단축키 녹화 슬롯 — 설정 Keybindings 의 바인딩 버튼, 추가(+) 버튼, 바인딩이 없는 행의 None 슬롯.
//! 본체 설정과 갤러리가 같은 그리기를 쓴다.

use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

/// 슬롯 하나의 종류.
#[derive(Debug, Clone, Copy)]
pub enum KbRecordSlot<'a> {
    /// 지정된 바인딩. mono caption 글자, surface-raised 채움.
    Binding(&'a str),
    /// 녹화 중인 슬롯의 안내 문구. surface-hover 채움, disabled 글자.
    Recording(&'a str),
    /// 바인딩을 하나 더하는 버튼. 테두리와 plus 아이콘만 그린다.
    Add,
    /// 바인딩이 없는 행의 슬롯. 테두리와 흐린 caption 글자만 그린다. 누르면 첫 바인딩을 녹화한다.
    Empty(&'a str),
}

/// 쉬는 테두리. TODO(tokens): batch 11 의 `kb-record-border` 접근자가 생기면 그것으로 바꾼다(값 border-default).
fn rest_border(theme: &Theme) -> HexColor {
    theme.border_default()
}

/// 호버 테두리. TODO(tokens): `kb-record-border-hover` 접근자로 바꾼다(값 border-strong).
fn hover_border(theme: &Theme) -> HexColor {
    theme.border_strong()
}

/// None 슬롯 글자. TODO(tokens): `kb-record-empty-fg` 접근자로 바꾼다(값 text-muted).
fn empty_fg(theme: &Theme) -> HexColor {
    theme.text_muted()
}

/// 슬롯을 그린다. 폭은 `width` 이상이고 글자가 더 길면 넓어진다. 높이는 `kb-record-height` 다.
/// 호버하면 채움은 그대로 두고 테두리만 `kb-record-border-hover` 로 바꾼다.
/// 다른 녹화가 대기 중이면(`enabled=false`) disabled 상자 role 과 disabled ink 로 그리고 클릭을 받지 않는다.
pub fn kb_record_slot(
    ui: &mut egui::Ui,
    theme: &Theme,
    slot: KbRecordSlot<'_>,
    width: LogicalPx,
    enabled: bool,
) -> egui::Response {
    let caption = theme.font_size_caption.value();
    let font = match slot {
        KbRecordSlot::Empty(_) => egui::FontId::proportional(caption),
        _ => egui::FontId::monospace(caption),
    };
    let text = match slot {
        KbRecordSlot::Binding(s) | KbRecordSlot::Recording(s) | KbRecordSlot::Empty(s) => s,
        KbRecordSlot::Add => "",
    };
    let galley = ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER));
    let pad_x = theme.spacing_sm.value();
    let size = egui::vec2(
        width.value().max(galley.size().x + 2.0 * pad_x),
        theme.kb_record_height().value(),
    );
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);

    let (fill, border, fg) = if !enabled {
        (
            Some(theme.state_disabled_fill()),
            theme.state_disabled_border(),
            theme.state_disabled_fg(),
        )
    } else {
        let border = if resp.hovered() {
            hover_border(theme)
        } else {
            rest_border(theme)
        };
        match slot {
            KbRecordSlot::Binding(_) => {
                (Some(theme.surface_raised()), border, theme.text_primary())
            }
            KbRecordSlot::Recording(_) => {
                (Some(theme.surface_hover()), border, theme.text_disabled())
            }
            KbRecordSlot::Add => (None, border, theme.text_muted()),
            KbRecordSlot::Empty(_) => (None, border, empty_fg(theme)),
        }
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        theme.corner_radius.value(),
        fill.map_or(egui::Color32::TRANSPARENT, |c| c.to_egui()),
        egui::Stroke::new(theme.border_width.value(), border.to_egui()),
        egui::StrokeKind::Inside,
    );
    if matches!(slot, KbRecordSlot::Add) {
        let icon = theme.icon_glyph_size_sm.value();
        let at = egui::Rect::from_center_size(rect.center(), egui::vec2(icon, icon));
        tasty_icons::PLUS.image(icon, fg.to_egui()).paint_at(ui, at);
    } else {
        let pos = rect.center() - galley.size() * 0.5;
        painter.galley(pos, galley, fg.to_egui());
    }
    resp
}
