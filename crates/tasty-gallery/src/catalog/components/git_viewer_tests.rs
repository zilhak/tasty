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

/// 맥락 줄 본문.
const CTX: &str = "fn draw(ui) {";

/// 같은 줄로 보는 글자 중심 높이 차. 글자 모양마다 보이는 상자가 달라 중심이 1~2px 어긋나고, 줄 사이는
/// 그보다 훨씬 멀다(caption × 1.65).
const ROW_SLACK: f32 = 4.0;

/// `content` 줄과 같은 높이에 그린 줄 번호(숫자만 든 글자)의 색.
fn line_number_inks(p: &Painted, content: &str) -> Vec<egui::Color32> {
    let row_y = p.text(content).rect.center().y;
    let inks: Vec<egui::Color32> = p
        .texts
        .iter()
        .filter(|t| {
            !t.text.is_empty()
                && t.text.chars().all(|c| c.is_ascii_digit())
                && (t.rect.center().y - row_y).abs() < ROW_SLACK
        })
        .map(|t| t.color)
        .collect();
    assert!(!inks.is_empty(), "{content:?} row has a line number");
    inks
}

/// 줄 번호는 행 바탕의 text-muted 칸이다: 맥락 줄은 text-muted, 채운 추가·삭제 줄은 text-muted-raised.
/// Mocha는 두 칸이 같아 Latte에서 갈리는지 먼저 본다.
#[test]
fn diff_line_numbers_take_the_row_ground_muted_ink() {
    let latte = latte();
    assert_ne!(
        latte.text_muted(),
        latte.text_muted_raised(),
        "Latte must separate the two muted inks"
    );
    for theme in [tasty_themes::mocha_fallback(), latte] {
        let p = painted_diff(&theme);
        for (text, ink) in [
            (CTX, theme.text_muted()),
            (ADD, theme.text_muted_raised()),
            (DEL, theme.text_muted_raised()),
        ] {
            for painted in line_number_inks(&p, text) {
                assert_eq!(painted, ink.to_egui(), "{text}");
            }
        }
    }
}
