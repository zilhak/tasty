//! Grid 칸 폭(디자인 `explorer-grid-cell-width` 80)이 UI 배율을 따르는지 그린 글자 위치로 본다.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tasty_model::ExplorerPanel;

use crate::theme::Theme;

const DIR: &str = "/tasty-test-no-such-dir";

/// `zoom` 배율 Theme 로 Grid 한 줄을 그리고, 이름 글자마다 (문자열, 사각형)을 모은다.
fn painted_grid(zoom: f32) -> (Theme, Vec<(String, egui::Rect)>) {
    crate::i18n::init("en");
    let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, zoom);
    let panel = ExplorerPanel::new(1, PathBuf::from(DIR));
    let mut view = super::ExplorerView::new();
    view.sync(&panel, None);
    view.state = super::LoadState::Ok;
    view.entries = ["a.txt", "b.txt", "c.txt"]
        .iter()
        .map(|n| super::DirEntryInfo {
            path: PathBuf::from(format!("{DIR}/{n}")),
            name: (*n).to_string(),
            is_dir: false,
            ext: "txt".into(),
            ..Default::default()
        })
        .collect();
    let font = crate::settings::EffectiveFont {
        font_family: String::new(),
        font_size: 13.0,
        custom_font_path: String::new(),
        line_height: 1.0,
        font_scale_mode: String::new(),
    };
    let ctx = egui::Context::default();
    let mut drawn = Vec::new();
    // 첫 프레임은 글꼴이 확정되지 않을 수 있어 두 번 그린다.
    for _ in 0..2 {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 600.0),
            )),
            ..Default::default()
        };
        let mut action = None;
        let out = ctx.run(raw, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    super::grid_view(
                        ui,
                        &theme,
                        &mut view,
                        &font,
                        &HashSet::new(),
                        Path::new(DIR),
                        &mut action,
                    );
                });
        });
        drawn = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text().ends_with(".txt") => {
                    Some((t.galley.text().to_owned(), t.visual_bounding_rect()))
                }
                _ => None,
            })
            .collect();
    }
    (theme, drawn)
}

/// 이웃한 두 칸의 이름 가운데 사이는 칸 폭과 칸 간격(`spacing_md`)이다. 칸 폭은 배율 1에서 80,
/// 1.5에서 120 이다.
#[test]
fn the_grid_cell_width_follows_the_ui_zoom() {
    for (zoom, cell_w) in [(1.0, 80.0), (1.5, 120.0)] {
        let (theme, drawn) = painted_grid(zoom);
        let center = |n: &str| {
            drawn
                .iter()
                .find(|(t, _)| t == n)
                .unwrap_or_else(|| panic!("배율 {zoom}: {n} 이 그려진다: {drawn:?}"))
                .1
                .center()
        };
        let (a, b) = (center("a.txt"), center("b.txt"));
        assert!((a.y - b.y).abs() < 1.0, "배율 {zoom}: 같은 줄 {a:?} {b:?}");
        let pitch = b.x - a.x - theme.spacing_md.value();
        assert!(
            (pitch - cell_w).abs() <= 0.5,
            "배율 {zoom}: 칸 폭 {pitch}, 기대 {cell_w}"
        );
    }
}
