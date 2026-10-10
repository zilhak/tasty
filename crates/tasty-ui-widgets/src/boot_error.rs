//! 부팅 실패 화면. 본체와 갤러리가 함께 호출한다.
//!
//! 부팅 로딩·첫 실행 셸 설정 화면과 같은 구조다. `bg-app` 채움 위에 로고 락업 → `space-xl` →
//! `boot-form-width` 폭 폼을 세로 가운데에 쌓는다. 카드와 그림자는 없다. 폼은 제목 줄(alertCircle
//! `boot-error-glyph` + 제목 text-primary) · 본문(body, text-secondary) · 안내(caption, text-muted) ·
//! 오른쪽 정렬 Quit(Button secondary md)이며 항목 사이는 `space-sm`이다. 본문과 안내의 백틱 구간
//! (CLI 명령·옵션)은 code run으로 그린다.
//!
//! 문구는 자르지 않는다. 쌓은 높이가 창 높이 − 2 × `space-xl`보다 크면 락업을 먼저 빼고, 그래도
//! 넘치면 제목·본문·안내를 스크롤하고 Quit은 그 아래에 고정한다.

use tasty_type_appearance::theme::Theme;

use crate::brand::draw_wordmark;
use crate::button::{Button, ButtonVariant};
use crate::control::ControlSize;
use crate::tokens::STRUCT_GAP_1;
use crate::ui_code::ui_copy;

/// 화면 문구. i18n은 호출부가 한다.
pub struct BootErrorView<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub hint: &'a str,
    pub quit: &'a str,
}

/// 지난 패스에서 잰 높이. 문구 줄바꿈에 따라 바뀌므로 다음 패스의 배치에 쓴다.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Measured {
    lockup: f32,
    text: f32,
}

/// 이번 패스의 배치. 창 높이와 잰 높이로 정한다.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Plan {
    show_lockup: bool,
    /// 제목·본문·안내 영역에 줄 높이. 내용보다 작으면 그 영역이 스크롤된다.
    text_height: f32,
    /// 쌓은 전체의 위쪽 y.
    top: f32,
}

/// Quit 줄과 그 위 간격의 높이. 시안은 폼 gap 위에 `space-sm`을 한 번 더 둔다.
fn quit_block_height(theme: &Theme) -> f32 {
    theme.spacing_sm.value() * 2.0 + ControlSize::Md.height(theme)
}

fn plan(theme: &Theme, area: egui::Rect, measured: Measured) -> Plan {
    let xl = theme.spacing_xl.value();
    let limit = (area.height() - xl * 2.0).max(0.0);
    let quit = quit_block_height(theme);
    let form = measured.text + quit;
    let with_lockup = measured.lockup + xl + form;
    let show_lockup = with_lockup <= limit;
    let (total, text_height) = if show_lockup {
        (with_lockup, measured.text)
    } else if form <= limit {
        (form, measured.text)
    } else {
        (limit, (limit - quit).max(0.0))
    };
    Plan {
        show_lockup,
        text_height,
        top: area.center().y - total / 2.0,
    }
}

/// `ui`의 가용 영역 전체를 `bg-app`으로 채우고 락업과 폼을 가운데에 그린다.
/// 종료 버튼이 눌렸으면 true다. 키 입력은 호출부가 판정한다.
pub fn boot_error_screen(ui: &mut egui::Ui, theme: &Theme, view: &BootErrorView<'_>) -> bool {
    let area = ui.available_rect_before_wrap();
    ui.painter()
        .rect_filled(area, 0.0, theme.bg_app().to_egui());

    let measured_id = ui.id().with("boot_error_measured");
    let known = ui
        .data(|d| d.get_temp::<Measured>(measured_id))
        .unwrap_or_default();
    let plan = plan(theme, area, known);

    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                egui::pos2(area.left(), plan.top.max(area.top())),
                area.max,
            ))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    content.spacing_mut().item_spacing = egui::Vec2::ZERO;

    let mut measured = known;
    // 락업을 뺀 패스에서도 높이를 알아야 다시 넣을지 정할 수 있다. 빠진 패스는 지난 값을 유지한다.
    if plan.show_lockup || known.lockup == 0.0 {
        let top = content.cursor().top();
        if plan.show_lockup {
            draw_lockup(&mut content, theme);
            measured.lockup = content.cursor().top() - top;
            content.add_space(theme.spacing_xl.value());
        } else {
            measured.lockup = lockup_height(&content, theme);
        }
    }

    let width = theme.boot_form_width().value().min(area.width());
    let (quit, text) = content
        .allocate_ui_with_layout(
            egui::vec2(width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(width);
                draw_form(ui, theme, view, plan.text_height)
            },
        )
        .inner;
    measured.text = text;

    if measured != known {
        ui.data_mut(|d| d.insert_temp(measured_id, measured));
        ui.ctx().request_discard("boot error layout changed");
    }
    ui.allocate_rect(area, egui::Sense::hover());
    quit
}

fn draw_lockup(ui: &mut egui::Ui, theme: &Theme) {
    draw_wordmark(
        ui,
        theme,
        theme.loading_screen_wordmark_icon_size(),
        theme.loading_screen_wordmark_font_size(),
        theme.loading_lockup_tracking(),
    );
}

/// 그리지 않은 락업의 높이. `draw_wordmark`는 마크와 글자 중 큰 쪽을 높이로 잡는다.
fn lockup_height(ui: &egui::Ui, theme: &Theme) -> f32 {
    let font = egui::FontId::monospace(theme.loading_screen_wordmark_font_size().value());
    let text = ui.fonts(|f| f.row_height(&font));
    theme.loading_screen_wordmark_icon_size().value().max(text)
}

/// 폼을 그린다. 반환값은 (Quit 눌림, 제목·본문·안내의 실제 높이)다.
fn draw_form(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &BootErrorView<'_>,
    text_height: f32,
) -> (bool, f32) {
    let gap = theme.spacing_sm.value();
    let width = ui.available_width();
    let scroll = egui::ScrollArea::vertical()
        .id_salt("boot_error_text")
        // 높이를 잰 값으로 고정한다. 줄어들게 두면 스크롤 영역 기본 최소 높이에 걸려 안내가 잘린다.
        .min_scrolled_height(text_height)
        .max_height(text_height)
        .auto_shrink([false, false])
        .drag_to_scroll(false)
        .show(ui, |ui| {
            ui.set_width(width.min(ui.available_width()));
            ui.spacing_mut().item_spacing = egui::vec2(0.0, gap);
            title_row(ui, theme, view.title);
            ui_copy(
                ui,
                theme,
                view.body,
                theme.font_size_body,
                theme.text_secondary().to_egui(),
            );
            ui_copy(
                ui,
                theme,
                view.hint,
                theme.font_size_caption,
                theme.text_muted().to_egui(),
            );
        });
    let text = scroll.content_size.y;

    ui.add_space(gap * 2.0);
    let quit = ui
        .allocate_ui_with_layout(
            egui::vec2(width, ControlSize::Md.height(theme)),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                Button::new(view.quit)
                    .variant(ButtonVariant::Secondary)
                    .size(ControlSize::Md)
                    .show(ui, theme)
                    .clicked()
            },
        )
        .inner;
    (quit, text)
}

/// alertCircle 글리프와 제목. 글리프는 제목 첫 줄 위쪽에 붙고 제목은 남은 폭에서 줄바꿈한다.
fn title_row(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    let glyph = theme.icon_glyph_size_md.value();
    let size = theme.font_size_max.value();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        ui.vertical(|ui| {
            ui.add_space(STRUCT_GAP_1.value());
            let (rect, _) = ui.allocate_exact_size(egui::vec2(glyph, glyph), egui::Sense::hover());
            tasty_icons::ALERT_CIRCLE
                .image(glyph, theme.boot_error_glyph().to_egui())
                .paint_at(ui, rect);
        });
        // 600 굵기는 굵은 UI 글꼴이 없어 크기와 색으로 근사한다(디자인 정합 지침 §타이포그래피).
        ui.add(
            egui::Label::new(
                egui::RichText::new(title)
                    .size(size)
                    .line_height(Some(size * theme.line_height_ui))
                    .color(theme.text_primary().to_egui()),
            )
            .wrap(),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
    }

    const LONG_PATH: &str = "/Users/hana/Library/Application Support/tasty-workspaces/personal/profile-default/a/very/long/folder/name/that/keeps/going/and/going/without/any/spaces/at/all/x";

    /// 화면을 `screen` 크기로 세 번 그려 배치를 안정시키고, 마지막 패스의 잰 높이와 위젯 사각형을 돌려준다.
    fn settle(screen: egui::Vec2, view: &BootErrorView<'_>) -> (Measured, Vec<egui::Rect>) {
        let (m, rects, _) = settle_texts(screen, view);
        (m, rects)
    }

    /// `settle`과 같고, 잘리지 않고 보이는 글자 도형의 문장도 돌려준다.
    fn settle_texts(
        screen: egui::Vec2,
        view: &BootErrorView<'_>,
    ) -> (Measured, Vec<egui::Rect>, Vec<String>) {
        let theme = theme();
        let ctx = egui::Context::default();
        let mut measured = Measured::default();
        let mut rects = Vec::new();
        let mut texts = Vec::new();
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        let id = ui.id().with("boot_error_measured");
                        boot_error_screen(ui, &theme, view);
                        measured = ui.data(|d| d.get_temp::<Measured>(id)).unwrap_or_default();
                    });
            });
            rects = out
                .shapes
                .iter()
                .map(|s| s.shape.visual_bounding_rect())
                .filter(|r| r.is_positive())
                .collect();
            texts = out
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t)
                        if s.clip_rect.contains_rect(s.shape.visual_bounding_rect()) =>
                    {
                        Some(t.galley.job.text.clone())
                    }
                    _ => None,
                })
                .collect();
        }
        (measured, rects, texts)
    }

    fn view(body: &str) -> BootErrorView<'_> {
        // 본문은 시험마다 다르고 나머지 문구는 데이터 폴더 사용 중 화면의 것이다.
        BootErrorView {
            title: "Data folder already in use",
            body,
            hint: "Use the Tasty that is already running, or start this one with a different `TASTY_HOME`.",
            quit: "Quit",
        }
    }

    #[test]
    fn a_tall_window_keeps_the_lockup_and_the_whole_text() {
        let theme = theme();
        let screen = egui::vec2(1280.0, 720.0);
        let (m, _) = settle(screen, &view(LONG_PATH));
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, screen);
        let p = plan(&theme, area, m);
        assert!(m.lockup > 0.0 && m.text > 0.0, "{m:?}");
        assert!(p.show_lockup, "{p:?}");
        assert_eq!(p.text_height, m.text);
    }

    #[test]
    fn title_body_hint_and_quit_are_all_visible_when_they_fit() {
        let body = format!("Another Tasty is already using this data folder: {LONG_PATH}");
        let v = view(&body);
        let (_, _, texts) = settle_texts(egui::vec2(640.0, 480.0), &v);
        for want in [v.title, "Another Tasty", "Use the Tasty", v.quit] {
            assert!(
                texts.iter().any(|t| t.contains(want)),
                "보이지 않는 문구: {want} / 보이는 문구: {texts:?}"
            );
        }
    }

    #[test]
    fn a_short_window_drops_the_lockup_and_keeps_quit_visible() {
        let body = format!("Another Tasty is already using this data folder: {LONG_PATH}");
        let v = view(&body);
        let (_, _, texts) = settle_texts(egui::vec2(640.0, 160.0), &v);
        assert!(
            !texts.iter().any(|t| t.contains("tasty.") || t == "tasty"),
            "낮은 창에서도 락업이 남았다: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains(v.quit)),
            "Quit 이 보이지 않는다: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("Use the Tasty")),
            "넘친 안내는 스크롤 아래에 있어야 한다: {texts:?}"
        );
    }

    #[test]
    fn a_long_path_wraps_inside_the_form_width() {
        let theme = theme();
        let screen = egui::vec2(640.0, 480.0);
        let (_, rects) = settle(screen, &view(LONG_PATH));
        let form = theme.boot_form_width().value();
        let left = (screen.x - form) / 2.0;
        // 배경 채움(화면 전체)을 뺀 모든 도형이 폼 폭 안에 있다. 줄 첫머리·끝의 code run 채움은
        // 글자 밖으로 `ui-code-padding-x`만큼 나갈 수 있다.
        let slack = crate::ui_code::UiCodeTokens::of(&theme).padding_x.value() + 1.0;
        let inside: Vec<_> = rects.iter().filter(|r| r.width() < screen.x).collect();
        assert!(!inside.is_empty());
        for r in inside {
            assert!(
                r.left() >= left - slack && r.right() <= left + form + slack,
                "폼 밖으로 나간 도형: {r:?}"
            );
        }
    }

    #[test]
    fn a_short_window_drops_the_lockup_first_then_scrolls_the_text() {
        let theme = theme();
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
        let quit = quit_block_height(&theme);
        let xl = theme.spacing_xl.value();
        let limit = area.height() - xl * 2.0;

        let fits = Measured {
            lockup: 64.0,
            text: limit - quit - 64.0 - xl,
        };
        assert!(plan(&theme, area, fits).show_lockup);

        let no_lockup = Measured {
            lockup: 64.0,
            text: limit - quit,
        };
        let p = plan(&theme, area, no_lockup);
        assert!(!p.show_lockup);
        assert_eq!(p.text_height, no_lockup.text);

        let scrolls = Measured {
            lockup: 64.0,
            text: limit * 2.0,
        };
        let p = plan(&theme, area, scrolls);
        assert!(!p.show_lockup);
        assert_eq!(p.text_height, limit - quit);
        assert_eq!(p.top, area.top() + xl);
    }
}
