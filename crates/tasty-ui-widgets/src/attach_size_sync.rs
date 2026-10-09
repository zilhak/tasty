//! 원격 attach mirror 의 터미널 크기 동기화가 자동 재시도 뒤에도 실패했을 때의 Workspace 배너 내용.
//! 거절 배너와 같은 계열로 행 [글리프 | 제목·본문 | 다시 시도 · 닫기]이며 모든 칸을 위쪽에 맞춘다.
//! 여러 surface 가 함께 실패하면 한 장에 "N surfaces — a, b +n" 으로 묶고 버튼은 Retry all 이 된다.
//! 문자열은 호출자가 주입한다. 큐·표시 범위는 본체 BannerManager가 정한다.

use tasty_type_appearance::theme::Theme;

use crate::banner::banner_shell;
use crate::button::{Button, ButtonVariant};
use crate::control::ControlSize;
use crate::icon_button::{IconButton, IconButtonVariant};
use crate::spinner::Spinner;
use crate::tooltip::{Tooltip, tooltip_hover_delay_elapsed};

/// 본문 줄에 이름으로 보이는 surface 수. 나머지는 `+n` 으로 줄인다.
const SHOWN_NAMES: usize = 2;
/// 제목은 자르지 않으므로 이 줄 수까지 감싼다.
const MAX_TITLE_ROWS: usize = 3;

/// 배너 입력값.
pub struct AttachSizeSyncBannerView<'a> {
    pub title: &'a str,
    /// `{}` 하나에 이름 묶음이 들어가는 본문 번역문.
    pub body: &'a str,
    /// surface 가 둘 이상일 때의 이름 묶음 번역문. 앞 `{}` 는 수, 뒤 `{}` 는 이름이다.
    pub many: &'a str,
    /// 실패한 surface 의 이름(탭 제목). 비어 있으면 배너를 띄우지 않는 것이 호출자의 몫이다.
    pub names: &'a [&'a str],
    pub retry: &'a str,
    pub retry_all: &'a str,
    /// 닫기 버튼의 툴팁.
    pub dismiss: &'a str,
    /// 다시 시도의 응답을 기다리는 중이면 버튼을 비활성으로 두고 앞에 Spinner 를 그린다.
    pub retrying: bool,
}

/// 이번 프레임의 클릭.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttachSizeSyncBannerClicks {
    pub retry: bool,
    pub dismiss: bool,
}

impl AttachSizeSyncBannerView<'_> {
    fn many(&self) -> bool {
        self.names.len() > 1
    }

    fn retry_label(&self) -> &str {
        if self.many() {
            self.retry_all
        } else {
            self.retry
        }
    }
}

/// `text` 가 `max_width` 를 넘으면 끝을 잘라 말줄임표를 붙인다.
fn elide_to_width(ui: &egui::Ui, text: &str, font: &egui::FontId, max_width: f32) -> String {
    let width = |s: &str| {
        ui.fonts(|f| {
            f.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                .rect
                .width()
        })
    };
    if width(text) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut keep = chars.len();
    while keep > 0 {
        keep -= 1;
        let candidate: String = chars[..keep].iter().chain(['\u{2026}'].iter()).collect();
        if width(&candidate) <= max_width {
            return candidate;
        }
    }
    "\u{2026}".to_owned()
}

/// 본문 한 줄의 조각과 색. 이름은 text-secondary, 나머지는 text-muted 다.
fn body_sections(
    ui: &egui::Ui,
    theme: &Theme,
    view: &AttachSizeSyncBannerView<'_>,
) -> Vec<(String, egui::Color32)> {
    let muted = theme.text_muted().to_egui();
    let name_color = theme.text_secondary().to_egui();
    let font = egui::FontId::proportional(theme.banner_body_font_size().value());
    let name_max = theme.attach_sync_name_max_width().value();

    let mut names = Vec::new();
    for (i, name) in view.names.iter().take(SHOWN_NAMES).enumerate() {
        if i > 0 {
            names.push((", ".to_owned(), muted));
        }
        names.push((elide_to_width(ui, name, &font, name_max), name_color));
    }
    let rest = view.names.len().saturating_sub(SHOWN_NAMES);
    if rest > 0 {
        names.push((format!(" +{rest}"), muted));
    }

    let mut subject = Vec::new();
    if view.many() {
        let mut parts = view.many.splitn(3, "{}");
        let before = parts.next().unwrap_or_default();
        let mid = parts.next().unwrap_or_default();
        let after = parts.next().unwrap_or_default();
        subject.push((format!("{before}{}{mid}", view.names.len()), muted));
        subject.extend(names);
        subject.push((after.to_owned(), muted));
    } else {
        subject = names;
    }

    let (before, after) = view.body.split_once("{}").unwrap_or((view.body, ""));
    let mut out = vec![(before.to_owned(), muted)];
    out.extend(subject);
    out.push((after.to_owned(), muted));
    out.retain(|(text, _)| !text.is_empty());
    out
}

fn title_galley(
    ui: &egui::Ui,
    theme: &Theme,
    text: &str,
    wrap_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let size = theme.banner_title_font_size().value();
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id: egui::FontId::proportional(size),
            color: theme.banner_fg().to_egui(),
            line_height: Some(size * theme.line_height_ui),
            ..Default::default()
        },
    );
    job.wrap.max_width = wrap_width;
    job.wrap.max_rows = MAX_TITLE_ROWS;
    ui.fonts(|f| f.layout_job(job))
}

/// 본문은 한 줄이다. 넘치면 끝을 말줄임한다.
fn body_galley(
    ui: &egui::Ui,
    theme: &Theme,
    view: &AttachSizeSyncBannerView<'_>,
    wrap_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let size = theme.banner_body_font_size().value();
    let mut job = egui::text::LayoutJob::default();
    for (text, color) in body_sections(ui, theme, view) {
        job.append(
            &text,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(size),
                color,
                line_height: Some(size * theme.line_height_ui),
                ..Default::default()
            },
        );
    }
    job.wrap.max_width = wrap_width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('\u{2026}');
    ui.fonts(|f| f.layout_job(job))
}

/// 다시 시도 버튼과 닫기 버튼 묶음의 크기. 버튼은 `Button`과 같은 식으로 잰다.
fn actions_size(ui: &egui::Ui, theme: &Theme, view: &AttachSizeSyncBannerView<'_>) -> egui::Vec2 {
    let font = egui::FontId::proportional(ControlSize::Sm.font_size(theme));
    let text_w = ui.fonts(|f| {
        f.layout_no_wrap(
            view.retry_label().to_owned(),
            font,
            egui::Color32::PLACEHOLDER,
        )
        .rect
        .width()
    });
    let spinner_w = if view.retrying {
        theme.icon_glyph_size_sm.value() + theme.button_gap().value()
    } else {
        0.0
    };
    let button_w = text_w + spinner_w + 2.0 * ControlSize::Sm.pad_x(theme);
    let close = theme.icon_button_size_sm().value();
    egui::vec2(
        button_w + theme.spacing_xs.value() + close,
        ControlSize::Sm.height(theme).max(close),
    )
}

/// 배너 셸 안쪽에 내용을 그린다. 본체 BannerManager처럼 셸을 따로 그리는 호출자가 쓴다.
pub fn attach_size_sync_banner_content(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &AttachSizeSyncBannerView<'_>,
) -> AttachSizeSyncBannerClicks {
    let row_w = ui.available_width();
    let glyph = theme.icon_glyph_size_md.value();
    let gap = theme.banner_gap().value();
    let nudge = theme.banner_glyph_offset().value();
    let text_gap = theme.banner_text_gap().value();
    let actions = actions_size(ui, theme, view);
    let text_w = (row_w - glyph - gap - gap - actions.x).max(0.0);

    let title = title_galley(ui, theme, view.title, text_w);
    let body = body_galley(ui, theme, view, text_w);
    let text_h = title.size().y + text_gap + body.size().y;
    let row_h = (glyph + nudge).max(text_h).max(actions.y);

    let (row, _) = ui.allocate_exact_size(egui::vec2(row_w, row_h), egui::Sense::hover());
    let glyph_rect =
        egui::Rect::from_min_size(row.min + egui::vec2(0.0, nudge), egui::vec2(glyph, glyph));
    tasty_icons::ALERT_TRIANGLE
        .image(glyph, theme.attach_sync_glyph().to_egui())
        .paint_at(ui, glyph_rect);

    let text_x = row.left() + glyph + gap;
    let title_h = title.size().y;
    let painter = ui.painter();
    painter.galley(
        egui::pos2(text_x, row.top()),
        title,
        egui::Color32::PLACEHOLDER,
    );
    painter.galley(
        egui::pos2(text_x, row.top() + title_h + text_gap),
        body,
        egui::Color32::PLACEHOLDER,
    );

    let actions_rect =
        egui::Rect::from_min_size(egui::pos2(row.right() - actions.x, row.top()), actions);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(actions_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = theme.spacing_xs.value();
    let spinner_size = theme.icon_glyph_size_sm;
    let spinner = |ui: &mut egui::Ui, rect: egui::Rect, _: egui::Color32| {
        Spinner::new()
            .size(spinner_size.value())
            .color(theme.spinner_indicator().to_egui())
            .paint_in(ui, theme, rect);
    };
    let mut button = Button::new(view.retry_label())
        .variant(ButtonVariant::Secondary)
        .size(ControlSize::Sm)
        .enabled(!view.retrying);
    if view.retrying {
        button = button
            .leading_icon(&spinner)
            .leading_icon_size(spinner_size);
    }
    let retry = button.show(&mut child, theme).clicked() && !view.retrying;
    let close = IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .show(&mut child, theme, &|ui, r, c| {
            tasty_icons::CLOSE.image(r.height(), c).paint_at(ui, r)
        });
    if tooltip_hover_delay_elapsed(ui.ctx(), theme, close.id, close.hovered()) {
        Tooltip::new(view.dismiss)
            .id_source(close.id)
            .show(ui, theme, close.rect);
    }
    AttachSizeSyncBannerClicks {
        retry,
        dismiss: close.clicked(),
    }
}

/// 셸과 내용을 함께 그린다. 갤러리처럼 BannerManager 없이 배너 한 장을 보이는 곳에서 쓴다.
pub fn attach_size_sync_banner(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &AttachSizeSyncBannerView<'_>,
) -> AttachSizeSyncBannerClicks {
    let mut clicks = AttachSizeSyncBannerClicks::default();
    banner_shell(ui, theme, 1.0, |ui| {
        clicks = attach_size_sync_banner_content(ui, theme, view);
    });
    clicks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view<'a>(names: &'a [&'a str], retrying: bool) -> AttachSizeSyncBannerView<'a> {
        AttachSizeSyncBannerView {
            title: "Couldn't sync the terminal size with the remote",
            body: "{} · The remote may still be using the old size.",
            many: "{} surfaces — {}",
            names,
            retry: "Retry",
            retry_all: "Retry all",
            dismiss: "Dismiss",
            retrying,
        }
    }

    fn theme() -> Theme {
        Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
    }

    /// 배너를 폭 `width`로 한 번 그려 셸 rect와 모든 도형을 돌려준다.
    fn render(width: f32, v: &AttachSizeSyncBannerView<'_>) -> (egui::Rect, Vec<egui::Shape>) {
        let theme = theme();
        let ctx = egui::Context::default();
        let mut rect = egui::Rect::NOTHING;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 400.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let before = ui.cursor().min;
                    attach_size_sync_banner(ui, &theme, v);
                    rect = egui::Rect::from_min_max(before, ui.min_rect().max);
                });
        });
        (rect, out.shapes.into_iter().map(|c| c.shape).collect())
    }

    fn texts(shapes: &[egui::Shape]) -> Vec<String> {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    /// `body_sections` 를 한 프레임 안에서 부른다.
    fn sections(v: &AttachSizeSyncBannerView<'_>) -> Vec<(String, egui::Color32)> {
        let theme = theme();
        let ctx = egui::Context::default();
        let mut out = Vec::new();
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                out = body_sections(ui, &theme, v);
            });
        }));
        out
    }

    #[test]
    fn one_surface_reads_its_name_then_the_fixed_copy_and_offers_retry() {
        let (_, shapes) = render(460.0, &view(&["build"], false));
        let texts = texts(&shapes);
        assert!(
            texts
                .iter()
                .any(|t| t == "build · The remote may still be using the old size."),
            "{texts:?}"
        );
        assert!(texts.iter().any(|t| t == "Retry"), "{texts:?}");
    }

    #[test]
    fn several_surfaces_share_one_card_with_two_names_and_a_count() {
        let names = ["release", "build", "tests"];
        let parts = sections(&view(&names, false));
        let line: String = parts.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            line,
            "3 surfaces — release, build +1 · The remote may still be using the old size."
        );
        let theme = theme();
        let named: Vec<_> = parts
            .iter()
            .filter(|(_, c)| *c == theme.text_secondary().to_egui())
            .map(|(t, _)| t.as_str())
            .collect();
        assert_eq!(named, ["release", "build"]);
        let (_, shapes) = render(460.0, &view(&names, false));
        assert!(texts(&shapes).iter().any(|t| t == "Retry all"));
    }

    #[test]
    fn a_long_name_is_cut_at_the_name_width_and_the_fixed_copy_is_the_last_section() {
        let long = "release-pipeline-watch-logs-eu-west-with-a-very-long-tab-title";
        let parts = sections(&view(&[long, "build"], false));
        let name = &parts
            .iter()
            .find(|(t, _)| t.starts_with("release"))
            .expect("first name")
            .0;
        assert!(name.ends_with('\u{2026}'), "{name}");
        assert!(name.len() < long.len());
        assert!(
            parts
                .last()
                .is_some_and(|(t, _)| t == " · The remote may still be using the old size."),
            "{parts:?}"
        );
    }

    /// 프레임마다 주어진 입력을 넣고 그 프레임의 클릭과 마지막 프레임의 도형을 돌려준다.
    fn run_frames(
        v: &AttachSizeSyncBannerView<'_>,
        frames: &[Vec<egui::Event>],
    ) -> (Vec<AttachSizeSyncBannerClicks>, Vec<egui::Shape>) {
        let theme = theme();
        let ctx = egui::Context::default();
        let mut clicks = Vec::new();
        let mut shapes = Vec::new();
        for (i, events) in frames.iter().enumerate() {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(460.0, 400.0),
                )),
                time: Some(i as f64 * 0.1),
                events: events.clone(),
                ..Default::default()
            };
            let mut frame = AttachSizeSyncBannerClicks::default();
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        frame = attach_size_sync_banner(ui, &theme, v);
                    });
            });
            clicks.push(frame);
            shapes = out.shapes.into_iter().map(|c| c.shape).collect();
        }
        (clicks, shapes)
    }

    /// 라벨 글자를 감싸는 가장 작은 채움 사각형 — Retry 버튼의 rect.
    fn retry_button_rect(shapes: &[egui::Shape]) -> egui::Rect {
        let label = shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Text(t) if t.galley.text() == "Retry" => {
                    Some(s.visual_bounding_rect())
                }
                _ => None,
            })
            .expect("Retry label");
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) if r.rect.contains_rect(label) => Some(r.rect),
                _ => None,
            })
            .min_by(|a, b| a.area().total_cmp(&b.area()))
            .expect("Retry button rect")
    }

    fn click_at(pos: egui::Pos2) -> Vec<Vec<egui::Event>> {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![
            vec![],
            vec![egui::Event::PointerMoved(pos)],
            vec![button(true)],
            vec![button(false)],
        ]
    }

    #[test]
    fn retrying_keeps_the_label_and_draws_the_spinner_inside_the_button() {
        let theme = theme();
        let (_, shapes) = run_frames(&view(&["build"], true), &[vec![]]);
        assert!(texts(&shapes).iter().any(|t| t == "Retry"));
        let button = retry_button_rect(&shapes);
        let label = shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Text(t) if t.galley.text() == "Retry" => {
                    Some(s.visual_bounding_rect())
                }
                _ => None,
            })
            .expect("Retry label");
        // Spinner 는 지름이 Spinner 크기인 바탕 고리와 그 위를 도는 indicator 색 호로 그린다.
        let ink = theme.spinner_indicator().to_egui();
        let size = theme.icon_glyph_size_sm.value();
        let arcs: Vec<egui::Rect> = shapes
            .iter()
            .filter(|s| {
                matches!(s, egui::Shape::Path(p) if p.stroke.color == egui::epaint::ColorMode::Solid(ink))
            })
            .map(|s| s.visual_bounding_rect())
            .collect();
        // 고리 둘레는 호와 같은 반지름이라 bounding box 가 Spinner 크기보다 조금 작다.
        let ring = shapes
            .iter()
            .filter(|s| matches!(s, egui::Shape::Circle(c) if c.fill == egui::Color32::TRANSPARENT))
            .map(|s| s.visual_bounding_rect())
            .find(|r| {
                r.width() <= size + 0.5
                    && r.width() >= size * 0.75
                    && arcs.iter().any(|a| r.expand(0.5).contains_rect(*a))
            })
            .unwrap_or_else(|| panic!("no spinner ring around the arcs {arcs:?}"));
        assert!(button.contains_rect(ring), "{ring:?} outside {button:?}");
        assert!(
            ring.right() <= label.left(),
            "{ring:?} is not before {label:?}"
        );
    }

    #[test]
    fn retrying_does_not_report_a_click_on_retry() {
        // 상태마다 버튼 폭이 달라 그 상태에서 잰 Retry 가운데를 누른다.
        let press = |retrying: bool| {
            let v = view(&["build"], retrying);
            let (_, shapes) = run_frames(&v, &[vec![]]);
            run_frames(&v, &click_at(retry_button_rect(&shapes).center())).0
        };
        assert!(
            press(false).iter().any(|c| c.retry),
            "the click does not reach Retry, so the retrying case proves nothing"
        );
        assert!(press(true).iter().all(|c| !c.retry && !c.dismiss));
    }

    #[test]
    fn a_narrow_banner_stays_inside_its_width() {
        let names = ["release-pipeline-watch-logs-eu-west", "build", "tests"];
        let (rect, shapes) = render(280.0, &view(&names, true));
        for s in &shapes {
            let card_ink = match s {
                egui::Shape::Text(_) => true,
                egui::Shape::Rect(r) => r.blur_width == 0.0,
                _ => false,
            };
            if !card_ink {
                continue;
            }
            let b = s.visual_bounding_rect();
            if b.is_positive() {
                assert!(b.right() <= rect.right() + 0.5, "{b:?} past {rect:?}");
            }
        }
    }
}
