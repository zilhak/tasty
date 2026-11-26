use super::*;
use crate::catalog::components::painted::{Painted, paint};

fn latte() -> Theme {
    let file = tasty_themes::ThemeFile::parse(tasty_themes::LATTE_TOML_TEXT).expect("latte");
    let (partial, is_light) = file.to_partial();
    let mut colors = tasty_themes::mocha_fallback_colors();
    colors.apply_partial(&partial);
    Theme::with_colors(colors, is_light.unwrap_or(true))
}

/// diff 창 표본만 그린다.
fn painted_diff(theme: &Theme) -> Painted {
    paint(|ui| {
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(480.0, 320.0));
        diff_pane(ui, theme, area);
    })
}

/// 표본의 hunk·추가·삭제 줄 본문.
const HUNK: &str = "@@ -1,4 +1,6 @@ fn draw";
const ADD: &str = "let strip = section();";
const DEL: &str = "let h = old_head();";

/// hunk·추가·삭제 줄을 칠한 채움은 정본 git-diff-*-bg다.
#[test]
fn diff_rows_fill_with_git_diff_tokens() {
    for theme in [tasty_themes::mocha_fallback(), latte()] {
        let p = painted_diff(&theme);
        for (text, fill) in [
            (HUNK, theme.git_diff_hunk_bg()),
            (ADD, theme.git_diff_add_bg()),
            (DEL, theme.git_diff_del_bg()),
        ] {
            assert_eq!(p.fill_under(p.text(text).rect), fill.to_egui(), "{text}");
        }
    }
}
