//! 단축키 엔트리 행의 녹화 버튼 치수와 행 사이 간격을 실제 egui 프레임에서 잰다.
//! apply_theme_to_egui를 호출해야 제품의 간격·글꼴로 측정한다.

use super::{
    KeyCapture, KeybindingsSubTab, RowLayout, entries::draw_keybinding_entries, entries_for,
};
use crate::settings::Settings;

/// Clipboard 서브탭 엔트리를 그린 마지막 프레임의 도형.
fn frame_shapes() -> Vec<egui::epaint::ClippedShape> {
    let th = crate::theme::theme();
    let ctx = egui::Context::default();
    tasty_egui_theme::install_cjk_fallback(&ctx);
    tasty_egui_theme::apply_theme_to_egui(&th, &ctx);
    let mut settings = Settings::default();
    // Clipboard 서브탭의 실제 엔트리. 라벨이 짧아 줄을 바꾸지 않으므로 행 높이가 버튼 높이다.
    let entries = entries_for(KeybindingsSubTab::Clipboard);
    let mut out = None;
    // 두 번째 프레임은 첫 프레임에서 잰 글자 크기로 배치한다.
    for _ in 0..2 {
        out = Some(ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let layout = RowLayout {
                        general: &settings.general,
                        label_col: th.settings_label_width(),
                    };
                    draw_keybinding_entries(
                        ui,
                        &mut settings.keybindings,
                        layout,
                        &mut None,
                        &mut None,
                        &KeyCapture::None,
                        &entries,
                    );
                });
            },
        ));
    }
    out.expect("두 프레임을 돌렸다").shapes
}

/// 녹화 버튼(쉬는 채움 surface-raised) 도형.
fn record_button_shapes() -> Vec<egui::epaint::RectShape> {
    let fill = crate::theme::theme().surface_raised().to_egui();
    frame_shapes()
        .into_iter()
        .filter_map(|s| match s.shape {
            egui::Shape::Rect(r) if r.fill == fill => Some(r),
            _ => None,
        })
        .collect()
}

/// 녹화 버튼 사각형을 위에서 아래, 왼쪽에서 오른쪽 순으로 돌려준다.
fn record_buttons() -> Vec<egui::Rect> {
    let fill = crate::theme::theme().surface_raised().to_egui();
    let mut rects: Vec<egui::Rect> = frame_shapes()
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.fill == fill => Some(r.rect),
            _ => None,
        })
        .collect();
    rects.sort_by(|a, b| {
        a.top()
            .total_cmp(&b.top())
            .then(a.left().total_cmp(&b.left()))
    });
    rects
}

/// 녹화 버튼을 행(같은 top)별로 묶는다.
fn rows(rects: &[egui::Rect]) -> Vec<Vec<egui::Rect>> {
    let mut rows: Vec<Vec<egui::Rect>> = Vec::new();
    for r in rects {
        match rows.last_mut() {
            Some(row) if row[0].top() == r.top() => row.push(*r),
            _ => rows.push(vec![*r]),
        }
    }
    rows
}

#[test]
fn record_buttons_use_the_kb_record_tokens() {
    let th = crate::theme::theme();
    let rects = record_buttons();
    let rows = rows(&rects);
    assert!(
        rows.len() >= 2,
        "엔트리 행을 둘 이상 찾지 못했다: {rects:?}"
    );
    for r in &rects {
        assert_eq!(
            r.height(),
            th.kb_record_height().value(),
            "버튼 높이: {r:?}"
        );
    }
    // 각 행의 마지막 버튼은 추가(+) 버튼이다. 바인딩이 있는 행에서는 kb-record-add-width 폭이다.
    let add_widths: Vec<f32> = rows
        .iter()
        .filter(|row| row.len() >= 2)
        .map(|row| row.last().expect("빈 행 없음").width())
        .collect();
    assert!(
        !add_widths.is_empty(),
        "추가 버튼이 있는 행이 없다: {rects:?}"
    );
    for w in add_widths {
        assert_eq!(w, th.kb_record_add_width().value(), "추가 버튼 폭");
    }
    // 바인딩 버튼은 글자가 길면 넓어지므로 최소 폭이 kb-record-width 인지 본다.
    let narrowest = rows
        .iter()
        .filter(|row| row.len() >= 2)
        .flat_map(|row| row[..row.len() - 1].iter())
        .map(|r| r.width())
        .fold(f32::INFINITY, f32::min);
    assert_eq!(narrowest, th.kb_record_width().value(), "바인딩 버튼 폭");
}

#[test]
fn shortcut_rows_are_kb_row_gap_apart_and_buttons_space_xs() {
    let th = crate::theme::theme();
    assert_ne!(
        th.kb_row_gap(),
        th.spacing_xs,
        "두 간격이 같으면 이 시험은 행 사이와 행 안을 가르지 못한다"
    );
    let rects = record_buttons();
    let rows = rows(&rects);
    for pair in rows.windows(2) {
        let gap = pair[1][0].top() - pair[0][0].bottom();
        assert_eq!(gap, th.kb_row_gap().value(), "행 사이 간격: {pair:?}");
    }
    for row in &rows {
        for b in row.windows(2) {
            assert_eq!(
                b[1].left() - b[0].right(),
                th.spacing_xs.value(),
                "행 안 버튼 사이 간격: {row:?}"
            );
        }
    }
}

#[test]
fn record_buttons_have_a_border_default_edge_and_mono_caption_text() {
    let th = crate::theme::theme();
    let buttons = record_button_shapes();
    assert!(!buttons.is_empty(), "녹화 버튼을 찾지 못했다");
    for b in &buttons {
        assert_eq!(
            b.stroke,
            egui::Stroke::new(th.border_width.value(), th.border_default().to_egui()),
            "녹화 버튼 테두리: {:?}",
            b.rect
        );
    }
    // 버튼 안 글자의 크기와 글꼴 계열.
    let fonts: Vec<egui::FontId> = frame_shapes()
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t)
                if buttons
                    .iter()
                    .any(|b| b.rect.contains_rect(t.visual_bounding_rect())) =>
            {
                t.galley
                    .job
                    .sections
                    .first()
                    .map(|sec| sec.format.font_id.clone())
            }
            _ => None,
        })
        .collect();
    assert!(!fonts.is_empty(), "녹화 버튼 글자를 찾지 못했다");
    for f in fonts {
        assert_eq!(
            f,
            egui::FontId::monospace(th.font_size_caption.value()),
            "녹화 버튼 글자"
        );
    }
}
