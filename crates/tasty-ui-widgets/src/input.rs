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
    /// 지금은 고칠 수 없지만 읽는 값(시안 `readOnly`). disabled와 함께면 무시한다.
    read_only: bool,
    /// 고정 폭. `None` 이면 가용 폭을 채운다(디자인 `block`).
    width: Option<f32>,
    icon: Option<IconPainter<'a>>,
    addon: Option<&'a str>,
    /// 텍스트 색 override. `None` 이면 `input_fg`(text-primary).
    text_color: Option<egui::Color32>,
    /// 글자 정렬. 기본은 좌측이고, 숫자 필드만 우측을 쓴다.
    align: egui::Align,
    /// 입력의 고정 id. `None` 이면 그린 순서로 정해진다. 그리는 자리가 프레임마다 바뀌어도 포커스를
    /// 지켜야 하는 입력이 쓴다.
    id: Option<egui::Id>,
    /// 포커스가 없어도 포커스 테두리를 그린다. 확정한 입력의 결과를 기다리는 동안 쓴다.
    focus_look: bool,
    /// 입력 뒤 정사각 칸과 그 변 길이. 칸을 그리는 painter 는 input-icon-fg 색으로 불린다.
    trailing: Option<(IconPainter<'a>, f32)>,
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
            read_only: false,
            width: None,
            icon: None,
            addon: None,
            text_color: None,
            align: egui::Align::LEFT,
            id: None,
            focus_look: false,
            trailing: None,
        }
    }

    pub fn id(mut self, id: egui::Id) -> Self {
        self.id = Some(id);
        self
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

    /// 읽기 전용. disabled와 같은 중립 상자(input-readonly-bg·border)에 값은
    /// input-readonly-fg(text-secondary)로 그린다. 편집은 막고 포커스·선택·복사는 허용하며,
    /// 포커스는 1px focus 테두리만 두고 ring은 그리지 않는다. `enabled(false)`면 무시한다.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
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

    /// 포커스가 없어도 포커스 테두리를 그린다.
    pub fn focus_look(mut self, focus_look: bool) -> Self {
        self.focus_look = focus_look;
        self
    }

    /// 입력 뒤에 변 길이 `size` 인 칸을 두고 `paint` 로 그린다(예: 기다리는 동안의 Spinner).
    pub fn trailing(mut self, paint: IconPainter<'a>, size: f32) -> Self {
        self.trailing = Some((paint, size));
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

        // disabled는 opacity 없이 disabled 상자 role과 disabled ink를 쓴다.
        let read_only = self.enabled && self.read_only;
        let bg = if !self.enabled {
            theme.state_disabled_fill()
        } else if read_only {
            theme.input_readonly_bg()
        } else {
            theme.input_bg()
        };
        ui.painter().rect_filled(outer, radius, bg.to_egui());

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
        let trailing_w = self.trailing.map_or(0.0, |(_, size)| size + gap);
        let te_w = (inner_w - icon_w - addon_w - trailing_w).max(0.0);

        let muted = if self.enabled {
            theme.input_icon_fg().to_egui()
        } else {
            theme.state_disabled_fg().to_egui()
        };
        // 부모에는 위의 outer 한 칸만 할당한다. allocate_new_ui는 자식 영역(inner)으로 부모 커서를
        // 다시 옮겨 가로 배치에서 다음 위젯이 padding만큼 겹치므로, 할당하지 않는 자식 Ui에 그린다.
        let mut field = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let resp = {
            let ui = &mut field;
            ui.spacing_mut().item_spacing.x = gap;
            if let Some(paint) = self.icon {
                let (irect, _) = ui
                    .allocate_exact_size(egui::vec2(icon_glyph, icon_glyph), egui::Sense::hover());
                paint(ui, irect, muted);
            }
            let font = if self.mono {
                egui::FontId::monospace(caption)
            } else {
                egui::FontId::proportional(body)
            };
            let (hint, text_color) = if read_only {
                (
                    tasty_egui_theme::hint_text(theme, self.placeholder),
                    theme.input_readonly_fg().to_egui(),
                )
            } else if self.enabled {
                (
                    tasty_egui_theme::hint_text(theme, self.placeholder),
                    self.text_color
                        .unwrap_or_else(|| theme.input_fg().to_egui()),
                )
            } else {
                let ink = theme.state_disabled_fg().to_egui();
                (egui::RichText::new(self.placeholder).color(ink), ink)
            };
            // egui의 비활성 Ui는 색을 배경 쪽으로 흐리므로 대신 비대화형 TextEdit로 입력을 막는다.
            // 읽기 전용은 변경할 수 없는 &str 버퍼를 넘겨 편집만 막고 포커스·선택·복사는 둔다.
            let mut view: &str = buf.as_str();
            let text: &mut dyn egui::TextBuffer = if read_only { &mut view } else { buf };
            let te = egui::TextEdit::singleline(text)
                .frame(false)
                .desired_width(te_w)
                .hint_text(hint)
                .font(font)
                .horizontal_align(self.align)
                .text_color(text_color)
                .interactive(self.enabled);
            let te = match self.id {
                Some(id) => te.id(id),
                None => te,
            };
            let r = ui.add(te);
            if let Some(g) = addon_galley {
                let (arect, _) = ui.allocate_exact_size(g.rect.size(), egui::Sense::hover());
                ui.painter().galley(arect.min, g, muted);
            }
            if let Some((paint, size)) = self.trailing {
                let (trect, _) =
                    ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
                paint(ui, trect, muted);
            }
            r
        };
        let focused = resp.has_focus() || (self.focus_look && self.enabled);

        let border = if !self.enabled {
            theme.state_disabled_border().to_egui()
        } else if read_only {
            if resp.has_focus() {
                theme.input_border_focus().to_egui()
            } else {
                theme.input_readonly_border().to_egui()
            }
        } else if self.invalid {
            theme.input_border_invalid().to_egui()
        } else if focused {
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
        if focused && !read_only {
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Frame {
        focused: bool,
        output: egui::FullOutput,
    }

    /// 읽기 전용 Input 하나를 `events`와 함께 한 프레임 그린다.
    fn frame(
        ctx: &egui::Context,
        theme: &Theme,
        buf: &mut String,
        events: Vec<egui::Event>,
    ) -> Frame {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
            events,
            ..Default::default()
        };
        let mut focused = false;
        let output = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let resp = Input::new().mono(true).read_only(true).show(ui, theme, buf);
                focused = resp.has_focus();
            });
        });
        Frame { focused, output }
    }

    fn strokes(output: &egui::FullOutput) -> Vec<egui::Color32> {
        output
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Rect(r) if r.stroke.width > 0.0 => Some(r.stroke.color),
                _ => None,
            })
            .collect()
    }

    /// 읽기 전용 값은 클릭하면 포커스를 받고, 전체 선택·복사는 되지만 입력으로는 바뀌지 않는다.
    /// 포커스 때는 1px focus 테두리 하나만 있고 ring은 없다.
    #[test]
    fn read_only_input_focuses_and_copies_but_does_not_edit() {
        let theme = tasty_themes::mocha_fallback();
        let ctx = egui::Context::default();
        let mut buf = String::from("#89b4fa");

        let idle = frame(&ctx, &theme, &mut buf, Vec::new());
        assert!(!idle.focused);
        assert_eq!(
            strokes(&idle.output),
            vec![theme.input_readonly_border().to_egui()]
        );
        let fills: Vec<_> = idle
            .output
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Rect(r) if r.fill != egui::Color32::TRANSPARENT => Some(r.fill),
                _ => None,
            })
            .collect();
        assert!(
            fills.contains(&theme.input_readonly_bg().to_egui()),
            "{fills:?}"
        );
        let inks: Vec<_> = idle
            .output
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == "#89b4fa" => Some(t.fallback_color),
                _ => None,
            })
            .collect();
        assert_eq!(inks, vec![theme.input_readonly_fg().to_egui()]);

        let pos = egui::pos2(40.0, 8.0 + theme.input_height().value() * 0.5);
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &ctx,
            &theme,
            &mut buf,
            vec![egui::Event::PointerMoved(pos), press(true)],
        );
        let clicked = frame(&ctx, &theme, &mut buf, vec![press(false)]);
        let clicked = if clicked.focused {
            clicked
        } else {
            frame(&ctx, &theme, &mut buf, Vec::new())
        };
        assert!(clicked.focused, "a read-only value takes focus on click");

        let edited = frame(
            &ctx,
            &theme,
            &mut buf,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                egui::Event::Copy,
                egui::Event::Text("x".into()),
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(buf, "#89b4fa");
        assert!(edited.focused);
        let copied = edited
            .output
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "#89b4fa"));
        assert!(copied, "{:?}", edited.output.platform_output.commands);
        assert_eq!(
            strokes(&edited.output),
            vec![theme.input_border_focus().to_egui()]
        );
    }

    /// 가로 배치에서 Input은 outer 한 칸만 차지한다. 다음 위젯은 outer.right + item_spacing.x에서
    /// 시작하고, 필드 안쪽 padding만큼 겹치지 않는다.
    #[test]
    fn input_takes_only_its_outer_rect_in_a_horizontal_layout() {
        let theme = tasty_themes::mocha_fallback();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
        let mut buf = String::from("#89b4fa");
        let mut seen = None;
        for _ in 0..2 {
            let raw = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            // 첫 프레임은 폰트 준비용이다. 출력은 쓰지 않는다.
            drop(ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let field = Input::new()
                            .width(theme.field_width_xs.value())
                            .show(ui, &theme, &mut buf);
                        let next =
                            ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        seen = Some((field.rect, next.0, ui.spacing().item_spacing.x));
                    });
                });
            }));
        }
        let (outer, next, gap) = seen.expect("frame ran");
        assert_eq!(outer.width(), theme.field_width_xs.value());
        assert_eq!(next.left(), outer.right() + gap);
    }
}
