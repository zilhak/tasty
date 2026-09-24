//! 아이콘·접미 라벨을 붙일 수 있는 한 줄 입력 필드.
//! 포커스와 잘못된 값의 테두리는 애니메이션 없이 바로 표시한다.

use tasty_type_appearance::theme::Theme;

use crate::icon_button::IconPainter;

/// Input 빌더.
pub struct Input<'a> {
    placeholder: &'a str,
    mono: bool,
    invalid: bool,
    enabled: bool,
    /// 고정 폭. `None` 이면 가용 폭을 채운다(디자인 `block`).
    width: Option<f32>,
    icon: Option<IconPainter<'a>>,
    addon: Option<&'a str>,
    /// 텍스트 색 override. `None` 이면 `input_fg`(text-primary).
    text_color: Option<egui::Color32>,
    /// 글자 정렬. 기본은 좌측이고, 숫자 필드만 우측을 쓴다.
    align: egui::Align,
}

impl Default for Input<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Input<'a> {
    pub fn new() -> Self {
        Self {
            placeholder: "",
            mono: false,
            invalid: false,
            enabled: true,
            width: None,
            icon: None,
            addon: None,
            text_color: None,
            align: egui::Align::LEFT,
        }
    }

    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    pub fn mono(mut self, mono: bool) -> Self {
        self.mono = mono;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// 고정 폭(px). 미지정 시 가용 폭을 채운다.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// 입력 앞의 아이콘. 크기와 색은 Theme에서 읽는다.
    pub fn icon(mut self, icon: IconPainter<'a>) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn addon(mut self, addon: &'a str) -> Self {
        self.addon = Some(addon);
        self
    }

    /// 입력 글자색을 지정한다. 기본은 input_fg이며 다른 색도 Theme에서 가져와야 한다.
    pub fn text_color(mut self, color: egui::Color32) -> Self {
        self.text_color = Some(color);
        self
    }

    /// 기본은 왼쪽 정렬. 숫자를 비교하는 필드는 오른쪽 정렬을 사용할 수 있다.
    pub fn align(mut self, align: egui::Align) -> Self {
        self.align = align;
        self
    }

    /// 그리고 TextEdit 응답을 반환한다(`response.changed()` 로 변경 감지).
    pub fn show(self, ui: &mut egui::Ui, theme: &Theme, buf: &mut String) -> egui::Response {
        let height = theme.input_height().value();
        let pad_x = theme.input_padding_x().value();
        let gap = theme.input_gap().value();
        let radius = theme.input_radius().value();
        let bw = theme.border_width.value();
        let body = theme.input_font_size().value();
        // trailing addon 의 mono caption — 대응 input component 토큰 없음(semantic).
        let caption = theme.font_size_caption.value();

        let width = self.width.unwrap_or_else(|| ui.available_width());
        let (outer, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

        ui.painter()
            .rect_filled(outer, radius, theme.input_bg().to_egui());

        let inner = outer.shrink2(egui::vec2(pad_x, 0.0));
        let inner_w = inner.width();

        let icon_glyph = theme.icon_glyph_size_md.value();
        let icon_w = if self.icon.is_some() {
            icon_glyph + gap
        } else {
            0.0
        };
        let addon_font = egui::FontId::monospace(caption);
        let addon_galley = self.addon.map(|a| {
            ui.painter().layout_no_wrap(
                a.to_owned(),
                addon_font.clone(),
                egui::Color32::PLACEHOLDER,
            )
        });
        let addon_w = addon_galley
            .as_ref()
            .map(|g| g.rect.width() + gap)
            .unwrap_or(0.0);
        let te_w = (inner_w - icon_w - addon_w).max(0.0);

        let muted = theme.input_icon_fg().to_egui();
        let resp = ui
            .allocate_new_ui(
                egui::UiBuilder::new()
                    .max_rect(inner)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    if let Some(paint) = self.icon {
                        let (irect, _) = ui.allocate_exact_size(
                            egui::vec2(icon_glyph, icon_glyph),
                            egui::Sense::hover(),
                        );
                        paint(ui, irect, muted);
                    }
                    let font = if self.mono {
                        egui::FontId::monospace(caption)
                    } else {
                        egui::FontId::proportional(body)
                    };
                    let te = egui::TextEdit::singleline(buf)
                        .frame(false)
                        .desired_width(te_w)
                        .hint_text(tasty_egui_theme::hint_text(theme, self.placeholder))
                        .font(font)
                        .horizontal_align(self.align)
                        .text_color(
                            self.text_color
                                .unwrap_or_else(|| theme.input_fg().to_egui()),
                        );
                    let r = ui.add_enabled(self.enabled, te);
                    if let Some(g) = addon_galley {
                        let (arect, _) =
                            ui.allocate_exact_size(g.rect.size(), egui::Sense::hover());
                        ui.painter().galley(arect.min, g, muted);
                    }
                    r
                },
            )
            .inner;

        let border = if self.invalid {
            theme.input_border_invalid().to_egui()
        } else if resp.has_focus() {
            theme.input_border_focus().to_egui()
        } else {
            theme.input_border().to_egui()
        };
        ui.painter().rect_stroke(
            outer,
            radius,
            egui::Stroke::new(bw, border),
            egui::StrokeKind::Inside,
        );
        if resp.has_focus() {
            let ring = if self.invalid {
                theme.input_border_invalid().to_egui()
            } else {
                theme.input_border_focus().to_egui()
            };
            ui.painter().rect_stroke(
                outer.expand(bw),
                radius,
                egui::Stroke::new(bw, ring),
                egui::StrokeKind::Outside,
            );
        }

        // 팝오버 앵커가 아이콘 폭만큼 밀리지 않도록 응답 영역은 필드 전체로 바꾼다.
        // TextEdit의 ID와 포커스 상태는 유지한다.
        let mut resp = resp;
        resp.rect = outer;
        resp
    }
}
