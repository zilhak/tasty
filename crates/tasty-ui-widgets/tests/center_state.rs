//! CenterState 가 loading·empty·error 사이에서 글리프 위치를 유지하고 토큰 값으로 배치하는지,
//! 액션 버튼이 가운데 정렬에서 빠져 블록 아래에 매달리는지, 높이를 받지 않은 호스트에서
//! 대칭 자연 높이로 액션까지 자기 영역 안에 그리는지 검사한다.

use egui::{Pos2, RawInput, Rect, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{CenterState, CenterStateOutput};

const REGION_H: f32 = 300.0;

/// 두 프레임을 구동하고 마지막 프레임의 출력과 영역 상단 y 를 돌려준다.
fn render(theme: &Theme, make: impl Fn() -> CenterState<'static>) -> (CenterStateOutput, f32) {
    let ctx = egui::Context::default();
    let mut out = None;
    let mut region_top = 0.0;
    // 첫 프레임은 폰트 준비 전이라 두 번 돌린다.
    for _ in 0..2 {
        let _out = ctx.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    region_top = ui.cursor().top();
                    out = Some(make().show(ui, theme, Some(LogicalPx(REGION_H))));
                });
            },
        );
    }
    (out.expect("CenterState was drawn"), region_top)
}

#[test]
fn center_state_reserves_sub_line_so_glyph_does_not_move() {
    let theme = tasty_themes::mocha_fallback();
    let (loading, _) = render(&theme, || CenterState::loading("Loading folder"));
    let (empty, _) = render(&theme, || {
        CenterState::empty(tasty_icons::FOLDER_OPEN, "This folder is empty")
            .sub_line(Some("Files you add here appear in this list."))
    });
    let (error, _) = render(&theme, || {
        CenterState::error("Could not read this folder")
            .sub_line(Some("Permission denied (os error 13)"))
    });
    assert_eq!(loading.glyph.top(), empty.glyph.top());
    assert_eq!(empty.glyph.top(), error.glyph.top());
    assert_eq!(loading.sub_slot.height(), empty.sub_slot.height());
}

#[test]
fn center_state_uses_token_geometry() {
    let theme = tasty_themes::mocha_fallback();
    let (out, region_top) = render(&theme, || {
        CenterState::empty(tasty_icons::FOLDER_OPEN, "This folder is empty")
            .sub_line(Some("Files you add here appear in this list."))
    });
    assert_eq!(out.glyph.width(), 24.0);
    assert_eq!(out.glyph.height(), 24.0);
    assert_eq!(out.title.top() - out.glyph.bottom(), 8.0);
    assert_eq!(out.sub_slot.top() - out.title.bottom(), 4.0);
    assert!(out.sub_slot.width() <= 300.0);
    // 가로·세로 가운데.
    assert!((out.glyph.center().x - out.title.center().x).abs() < 0.5);
    let block = out.glyph.union(out.sub_slot);
    assert!((block.center().y - (region_top + REGION_H * 0.5)).abs() <= 0.5);
}

#[test]
fn center_state_hangs_the_action_below_the_centred_block() {
    let theme = tasty_themes::mocha_fallback();
    let (plain, _) = render(&theme, || {
        CenterState::error("Permission denied").sub_line(Some("Permission denied (os error 13)"))
    });
    let (out, region_top) = render(&theme, || {
        CenterState::error("Permission denied")
            .sub_line(Some("Permission denied (os error 13)"))
            .action("Retry", Some(tasty_icons::REFRESH))
    });
    let button = out.action.expect("action drawn");
    // 시안의 Button size="sm".
    assert_eq!(button.height(), 24.0);
    // 보조 줄 슬롯 끝 → 버튼 = center-state-action-gap(12).
    assert_eq!(
        LogicalPx(button.top() - out.sub_slot.bottom()),
        theme.center_state_action_gap()
    );
    assert_eq!(button.top() - out.sub_slot.bottom(), 12.0);
    // 가로 가운데.
    assert!((button.center().x - out.glyph.center().x).abs() <= 0.5);
    // 가운데 정렬은 글리프·제목·보조 줄만 대상으로 한다. 액션이 있어도 글리프가 움직이지 않는다.
    assert_eq!(plain.glyph.top(), out.glyph.top());
    let block = out.glyph.union(out.sub_slot);
    assert!((block.center().y - (region_top + REGION_H * 0.5)).abs() <= 0.5);
}

#[test]
fn center_state_error_glyph_is_owned_by_the_part() {
    assert_eq!(
        tasty_ui_widgets::CENTER_STATE_ERROR_GLYPH.uri,
        tasty_icons::ALERT_TRIANGLE.uri
    );
}

#[test]
fn center_state_glyph_scales_with_ui_zoom() {
    let base = tasty_themes::mocha_fallback();
    for (zoom, expected) in [(0.85, 20.0), (1.0, 24.0), (1.2, 29.0)] {
        let theme = Theme::with_colors_and_zoom(base.extract_colors(), false, zoom);
        let (out, _) = render(&theme, || CenterState::loading("Loading folder"));
        assert_eq!(out.glyph.width(), expected, "ui_zoom {zoom}");
    }
}

/// 높이 없이(`show(…, None)`) 그리고 출력과 할당된 영역을 돌려준다.
fn render_natural(
    theme: &Theme,
    make: impl Fn() -> CenterState<'static>,
) -> (CenterStateOutput, Rect) {
    let ctx = egui::Context::default();
    let mut out = None;
    let mut region = Rect::NOTHING;
    for _ in 0..2 {
        let _out = ctx.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let top = ui.cursor().top();
                    out = Some(make().show(ui, theme, None));
                    region = Rect::from_min_max(
                        egui::pos2(ui.min_rect().left(), top),
                        egui::pos2(ui.min_rect().right(), ui.cursor().top()),
                    );
                });
            },
        );
    }
    (out.expect("CenterState was drawn"), region)
}

#[test]
fn center_state_unsized_host_keeps_the_action_inside_a_symmetric_natural_height() {
    let base = tasty_themes::mocha_fallback();
    for zoom in [0.85, 1.0, 1.2] {
        let theme = Theme::with_colors_and_zoom(base.extract_colors(), false, zoom);
        let (plain, plain_region) = render_natural(&theme, || {
            CenterState::error("Could not load scripts")
                .sub_line(Some("~/.config/tasty/scripts is not readable"))
        });
        let (out, region) = render_natural(&theme, || {
            CenterState::error("Could not load scripts")
                .sub_line(Some("~/.config/tasty/scripts is not readable"))
                .action("Retry", Some(tasty_icons::REFRESH))
        });
        let pad = theme.spacing_md.value();
        let band = pad + theme.center_state_action_gap().value() + theme.button_height_sm().value();
        let plain_block = plain.glyph.union(plain.sub_slot);
        let block = out.glyph.union(out.sub_slot);
        // 액션 없음: 블록 위아래 space-md.
        assert!(
            (plain_block.top() - plain_region.top() - pad).abs() <= 0.5,
            "ui_zoom {zoom}: plain top pad"
        );
        assert!(
            (plain_region.bottom() - plain_block.bottom() - pad).abs() <= 0.5,
            "ui_zoom {zoom}: plain bottom pad"
        );
        // 액션 있음: 위아래 같은 띠, 블록은 가운데, 액션은 아래 끝에서 space-md 위에서 끝난다.
        assert!(
            (block.top() - region.top() - band).abs() <= 0.5,
            "ui_zoom {zoom}: top band {} vs {band}",
            block.top() - region.top()
        );
        assert!(
            (block.center().y - region.center().y).abs() <= 0.5,
            "ui_zoom {zoom}: block centred"
        );
        let button = out.action.expect("action drawn");
        assert!(
            button.bottom() <= region.bottom(),
            "ui_zoom {zoom}: action bottom {} overflows region bottom {}",
            button.bottom(),
            region.bottom()
        );
        assert!(
            (region.bottom() - button.bottom() - pad).abs() <= 0.5,
            "ui_zoom {zoom}: space below the action {}",
            region.bottom() - button.bottom()
        );
        // 글리프와 블록 사이 거리는 액션 유무와 무관하다.
        assert!(
            ((out.glyph.top() - region.top())
                - (plain.glyph.top() - plain_region.top())
                - (band - pad))
                .abs()
                <= 0.5,
            "ui_zoom {zoom}: glyph offset within the block"
        );
    }
}
