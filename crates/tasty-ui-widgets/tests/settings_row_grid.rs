//! 설정 행 격자: 라벨 열 폭 clamp, 라벨 길이와 무관한 컨트롤 x, 열 안 줄바꿈, 행 아래 caption 간격.

// 이유: 테스트에서는 사용하지 않는 반환값을 버리는 것을 허용한다.
#![allow(clippy::let_underscore_must_use)]
use egui::{Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{SettingsRow, settings_label_column};

const ORIGIN: Pos2 = pos2(10.0, 10.0);
const LONG: &str = "Accept webhook calls from other computers on every network interface \
                    that this machine listens on, including VPN and container bridges:";

fn theme(zoom: f32) -> Theme {
    Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, zoom)
}

fn run(f: impl FnMut(&mut egui::Ui)) {
    let ctx = egui::Context::default();
    let mut f = f;
    // `FullOutput` 불필요 — 이 테스트가 보는 것은 위젯이 보고한 rect 뿐이다.
    let _ = ctx.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0))),
            focused: true,
            ..Default::default()
        },
        |c| {
            egui::Area::new(egui::Id::new("host"))
                .fixed_pos(ORIGIN)
                .show(c, |ui| {
                    ui.set_width(900.0);
                    f(ui);
                });
        },
    );
}

/// 각 행의 (행 rect, 컨트롤 rect).
fn rows(theme: &Theme, labels: &[&str]) -> (LogicalPx, Vec<(Rect, Rect)>) {
    let mut col = LogicalPx(0.0);
    let mut out = Vec::new();
    run(|ui| {
        let rows: Vec<SettingsRow<'_>> = labels.iter().map(|l| SettingsRow::new(l)).collect();
        col = settings_label_column(ui, theme, &rows);
        out.clear();
        for row in rows {
            let mut control = Rect::NOTHING;
            let resp = row.show(ui, theme, col, |ui| {
                control = ui.add(egui::Button::new("x")).rect;
            });
            out.push((resp.rect, control));
        }
    });
    (col, out)
}

#[test]
fn short_labels_take_the_column_floor() {
    for zoom in [1.0_f32, 2.0] {
        let th = theme(zoom);
        let (col, _) = rows(&th, &["On:", "Sound:"]);
        assert_eq!(
            col,
            th.settings_label_width(),
            "zoom {zoom}: 짧은 라벨만 있는데 열이 하한이 아니다"
        );
    }
}

#[test]
fn a_long_label_stops_the_column_at_the_cap() {
    let th = theme(1.0);
    let (col, _) = rows(&th, &["On:", LONG]);
    assert_eq!(col, th.settings_label_max_width());
}

#[test]
fn a_mid_label_sets_the_column_to_its_own_width() {
    let th = theme(1.0);
    let mid = "Inherit working directory for new tabs:";
    let (col, _) = rows(&th, &["On:", mid]);
    assert!(
        col > th.settings_label_width() && col < th.settings_label_max_width(),
        "열 {col:?} 이 하한·상한 사이의 라벨 폭이 아니다"
    );
}

#[test]
fn every_control_starts_at_the_same_x() {
    for zoom in [1.0_f32, 2.0] {
        let th = theme(zoom);
        let (col, out) = rows(&th, &["On:", "Scrollback lines:", LONG]);
        let expected = ORIGIN.x + (col + th.settings_label_gap()).value();
        for (i, (_, control)) in out.iter().enumerate() {
            assert!(
                (control.left() - expected).abs() < 0.5,
                "zoom {zoom}: 행 {i} 컨트롤 x {} ≠ 열+gap {expected} — 라벨 열이 글자 폭으로 줄었다",
                control.left()
            );
        }
    }
}

#[test]
fn an_overlong_label_wraps_inside_the_column() {
    let th = theme(1.0);
    let (_, out) = rows(&th, &["On:", LONG]);
    let (short_row, _) = out[0];
    let (long_row, _) = out[1];
    assert_eq!(short_row.height(), th.settings_row_min_height().value());
    assert!(
        long_row.height() > short_row.height(),
        "열보다 긴 라벨이 줄을 바꾸지 않았다: 높이 {}",
        long_row.height()
    );
}

#[test]
fn a_caption_sits_under_its_row_by_the_caption_gap() {
    let th = theme(1.0);
    let text = "Lines scrolled per wheel notch in the terminal. Trackpads scroll by distance \
                and ignore this, so the setting only applies to mouse wheels.";
    let (mut plain, mut captioned, mut caption_h) = (Rect::NOTHING, Rect::NOTHING, 0.0);
    run(|ui| {
        let row = SettingsRow::new("Wheel scroll distance:");
        let col = settings_label_column(ui, &th, [&row]);
        plain = row
            .show(ui, &th, col, |ui| {
                ui.add(egui::Button::new("x"));
            })
            .rect;
        captioned = row
            .caption(text)
            .show(ui, &th, col, |ui| {
                ui.add(egui::Button::new("x"));
            })
            .rect;
        // caption 은 measure-md 폭에서 줄을 바꾼 caption 글자다.
        let font = egui::FontId::proportional(th.font_size_caption.value());
        caption_h = ui.fonts(|f| {
            f.layout(
                text.to_owned(),
                font,
                egui::Color32::WHITE,
                th.measure_md.value(),
            )
            .size()
            .y
        });
    });
    let expected = plain.height() + th.settings_row_caption_gap().value() + caption_h;
    assert!(
        (captioned.height() - expected).abs() < 0.5,
        "caption 행 높이 {} ≠ 행 {} + gap {} + caption {caption_h}",
        captioned.height(),
        plain.height(),
        th.settings_row_caption_gap().value()
    );
    assert!(
        caption_h > th.font_size_caption.value() * 1.5,
        "measure-md 에서 caption 이 줄을 바꾸지 않았다"
    );
}

#[test]
fn a_hint_stays_inside_the_label_column() {
    let th = theme(1.0);
    let (mut col, mut plain_x, mut hinted_x) = (LogicalPx(0.0), 0.0, 0.0);
    run(|ui| {
        let plain = SettingsRow::new("Scrollback disk swap:");
        let hinted = SettingsRow::new("Lazy PTY init:").hint("Defer spawning the shell.");
        col = settings_label_column(ui, &th, [&plain, &hinted]);
        plain.show(ui, &th, col, |ui| {
            plain_x = ui.add(egui::Button::new("x")).rect.left();
        });
        hinted.show(ui, &th, col, |ui| {
            hinted_x = ui.add(egui::Button::new("x")).rect.left();
        });
    });
    assert_eq!(plain_x, hinted_x, "도움말 아이콘이 컨트롤을 밀었다");
}

#[test]
fn the_painted_label_never_crosses_into_the_gap() {
    let th = theme(1.0);
    let ctx = egui::Context::default();
    let mut col = LogicalPx(0.0);
    // Area 의 첫 프레임은 크기만 재고 그리지 않으므로 두 번째 프레임의 shape 을 본다.
    let mut frame = || {
        ctx.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0))),
                focused: true,
                ..Default::default()
            },
            |c| {
                egui::Area::new(egui::Id::new("host"))
                    .fixed_pos(ORIGIN)
                    .show(c, |ui| {
                        let row = SettingsRow::new(LONG).hint("help");
                        col = settings_label_column(ui, &th, [&row]);
                        row.show(ui, &th, col, |ui| {
                            ui.add(egui::Button::new("x"));
                        });
                    });
            },
        )
    };
    // 첫 프레임은 Area 크기를 재는 프레임이라 출력이 필요 없다.
    let _ = frame();
    let out = frame();
    let mut seen = false;
    for clipped in &out.shapes {
        if let egui::Shape::Text(text) = &clipped.shape
            && text.galley.text() == LONG
        {
            seen = true;
            let right = text.pos.x + text.galley.size().x;
            let limit = ORIGIN.x + (col - th.help_hint_gap() - th.icon_glyph_size_sm).value();
            assert!(
                right <= limit + 0.5,
                "라벨 글자 오른쪽 {right} 가 도움말 자리를 남긴 열 끝 {limit} 을 넘는다"
            );
        }
    }
    assert!(seen, "라벨 글자 shape 을 찾지 못했다");
}
