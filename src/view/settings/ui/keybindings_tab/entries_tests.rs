//! 단축키 엔트리 행의 녹화 버튼 치수와 행 사이 간격을 실제 egui 프레임에서 잰다.
//! apply_theme_to_egui를 호출해야 제품의 간격·글꼴로 측정한다.

use super::{
    KeyCapture, KeybindingsSubTab, RowLayout, entries::draw_keybinding_entries, entries_for,
};
use crate::settings::Settings;

/// Clipboard 서브탭 엔트리를 그린 마지막 프레임의 도형. `edit` 로 설정을 바꾸고 `pointer` 가 있으면
/// 그 자리에 포인터를 둔다.
fn frame_shapes_with(
    edit: impl FnOnce(&mut Settings),
    pointer: Option<egui::Pos2>,
) -> Vec<egui::epaint::ClippedShape> {
    let th = crate::theme::theme();
    let ctx = egui::Context::default();
    tasty_egui_theme::install_cjk_fallback(&ctx);
    tasty_egui_theme::apply_theme_to_egui(&th, &ctx);
    // 추가 버튼의 plus 아이콘(SVG)을 그리려면 본체처럼 이미지 로더가 있어야 한다.
    egui_extras::install_image_loaders(&ctx);
    tasty_icons::install_texture_loader(&ctx);
    let mut settings = Settings::default();
    edit(&mut settings);
    // Clipboard 서브탭의 실제 엔트리. 라벨이 짧아 줄을 바꾸지 않으므로 행 높이가 버튼 높이다.
    let entries = entries_for(KeybindingsSubTab::Clipboard);
    let mut out = None;
    // 두 번째 프레임은 첫 프레임에서 잰 글자 크기로 배치한다. 셋째 프레임은 포인터 위치의 호버를 그린다.
    for _ in 0..3 {
        out = Some(ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 600.0),
                )),
                events: pointer.map(egui::Event::PointerMoved).into_iter().collect(),
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
    out.expect("세 프레임을 돌렸다").shapes
}

fn frame_shapes() -> Vec<egui::epaint::ClippedShape> {
    frame_shapes_with(|_| {}, None)
}

/// 녹화 슬롯(바인딩·추가·None) 도형 — 1px 쉬는 테두리나 호버 테두리를 가진 사각형.
fn slot_shapes(shapes: &[egui::epaint::ClippedShape]) -> Vec<egui::epaint::RectShape> {
    let th = crate::theme::theme();
    let edges = [
        th.kb_record_border().to_egui(),
        th.kb_record_border_hover().to_egui(),
    ];
    shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(r)
                if r.stroke.width == th.border_width.value() && edges.contains(&r.stroke.color) =>
            {
                Some(r.clone())
            }
            _ => None,
        })
        .collect()
}

/// 바인딩 버튼(쉬는 채움 surface-raised) 도형.
fn record_button_shapes() -> Vec<egui::epaint::RectShape> {
    let fill = crate::theme::theme().surface_raised().to_egui();
    slot_shapes(&frame_shapes())
        .into_iter()
        .filter(|r| r.fill == fill)
        .collect()
}

/// 녹화 슬롯 사각형을 위에서 아래, 왼쪽에서 오른쪽 순으로 돌려준다.
fn record_buttons() -> Vec<egui::Rect> {
    let mut rects: Vec<egui::Rect> = slot_shapes(&frame_shapes())
        .iter()
        .map(|r| r.rect)
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
            egui::Stroke::new(th.border_width.value(), th.kb_record_border().to_egui()),
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

#[test]
fn the_add_slot_is_border_only_with_a_plus_icon() {
    let th = crate::theme::theme();
    let shapes = frame_shapes();
    let slots = slot_shapes(&shapes);
    let mut rects: Vec<egui::Rect> = slots.iter().map(|r| r.rect).collect();
    rects.sort_by(|a, b| {
        a.top()
            .total_cmp(&b.top())
            .then(a.left().total_cmp(&b.left()))
    });
    let adds: Vec<egui::Rect> = rows(&rects)
        .iter()
        .filter(|row| row.len() >= 2)
        .map(|row| *row.last().expect("빈 행 없음"))
        .collect();
    assert!(!adds.is_empty(), "추가 버튼을 찾지 못했다");
    for add in adds {
        let shape = slots
            .iter()
            .find(|r| r.rect == add)
            .expect("추가 버튼 도형");
        assert_eq!(
            shape.fill,
            egui::Color32::TRANSPARENT,
            "추가 버튼은 채움이 없다"
        );
        // egui 는 이미지를 텍스처를 입힌 사각형으로 그리고 채움에 tint 를 싣는다.
        let icon = egui::Vec2::splat(th.icon_glyph_size_sm.value());
        let has_icon = shapes.iter().any(|s| {
            matches!(&s.shape, egui::Shape::Rect(r)
                if add.contains_rect(r.rect)
                    && r.rect.size() == icon
                    && r.fill == th.text_muted().to_egui())
        });
        let has_text = shapes.iter().any(|s| {
            matches!(&s.shape, egui::Shape::Text(t)
                if add.contains_rect(t.visual_bounding_rect()) && !t.galley.is_empty())
        });
        assert!(
            has_icon,
            "추가 버튼 안에 text-muted plus 아이콘(icon-size-sm)이 없다: {add:?}"
        );
        assert!(
            !has_text,
            "추가 버튼은 글자 \"+\" 대신 아이콘만 그린다: {add:?}"
        );
    }
}

#[test]
fn a_row_without_bindings_is_one_none_slot_without_add() {
    let th = crate::theme::theme();
    let shapes = frame_shapes_with(|s| s.keybindings.copy_link.clear(), None);
    let slots = slot_shapes(&shapes);
    let mut rects: Vec<egui::Rect> = slots.iter().map(|r| r.rect).collect();
    rects.sort_by(|a, b| {
        a.top()
            .total_cmp(&b.top())
            .then(a.left().total_cmp(&b.left()))
    });
    let singles: Vec<egui::Rect> = rows(&rects)
        .into_iter()
        .filter(|row| row.len() == 1)
        .map(|row| row[0])
        .collect();
    assert_eq!(
        singles.len(),
        1,
        "바인딩을 지운 한 행만 슬롯이 하나다: {rects:?}"
    );
    let none = singles[0];
    let shape = slots
        .iter()
        .find(|r| r.rect == none)
        .expect("None 슬롯 도형");
    assert_eq!(
        shape.fill,
        egui::Color32::TRANSPARENT,
        "None 슬롯은 채움이 없다"
    );
    assert_eq!(none.width(), th.kb_record_width().value(), "None 슬롯 폭");
    let text = shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if none.contains_rect(t.visual_bounding_rect()) => Some(t.clone()),
            _ => None,
        })
        .expect("None 슬롯 글자");
    assert_eq!(
        text.galley.text(),
        crate::i18n::t("settings.keybindings.hint_none")
    );
    assert_eq!(
        text.fallback_color,
        th.kb_record_empty_fg().to_egui(),
        "None 슬롯 글자색"
    );
}

#[test]
fn hovering_a_slot_turns_only_its_border_strong() {
    let th = crate::theme::theme();
    let first = record_buttons()[0];
    let shapes = frame_shapes_with(|_| {}, Some(first.center()));
    let slots = slot_shapes(&shapes);
    let strong = th.kb_record_border_hover().to_egui();
    let hovered: Vec<egui::Rect> = slots
        .iter()
        .filter(|r| r.stroke.color == strong)
        .map(|r| r.rect)
        .collect();
    assert_eq!(hovered, vec![first], "포인터 아래 슬롯만 호버 테두리다");
    let raised = slots.iter().find(|r| r.rect == first).expect("호버 슬롯");
    assert_eq!(
        raised.fill,
        th.surface_raised().to_egui(),
        "호버해도 채움은 그대로다"
    );
}
