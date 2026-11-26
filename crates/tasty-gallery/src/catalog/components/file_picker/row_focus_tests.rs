//! Spec `row-focus-and-list-keys` 표본: 행 상태 여덟 칸의 잉크와, 카드마다 선택 막대 · 링 · 이름 칸 · 확정 버튼을
//! 그린 도형으로 본다. 정본은 디자인 `FpRow` · `FilePickerFrame` 이다.
use super::row_focus::{ROW_STATES, stage_cards, views};
use super::{FILES, Row, RowState, Variant, card, row, row_height};
use crate::catalog::components::painted::{Painted, mocha_latte, paint};
use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

/// (선택, hover) 칸의 잉크 — 디자인 `ink(rest, restHover, sel, selHover)`.
fn cell(
    selected: bool,
    hover: bool,
    [rest, rest_hover, sel, sel_hover]: [HexColor; 4],
) -> egui::Color32 {
    match (selected, hover) {
        (false, false) => rest,
        (false, true) => rest_hover,
        (true, false) => sel,
        (true, true) => sel_hover,
    }
    .to_egui()
}

fn seed(name: &str) -> &'static Row {
    FILES.iter().find(|f| f.name == name).expect("seed 행")
}

fn paint_row(th: &Theme, r: &'static Row, selected: bool, hover: bool, focus: bool) -> Painted {
    paint(|ui| {
        ui.set_width(th.fp_specimen_view_width().value());
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        row(
            ui,
            th,
            r,
            RowState {
                multi: false,
                checked: false,
                selected,
                focus,
                hover,
                inert: false,
                file_tooltip: "",
            },
        );
    })
}

fn text_color(p: &Painted, text: &str) -> egui::Color32 {
    let hits: Vec<_> = p.texts.iter().filter(|t| t.text == text).collect();
    assert_eq!(hits.len(), 1, "{text} 하나");
    hits[0].color
}

/// 행 높이 그대로의 테두리 선(링 후보): (도형 순번, 사각형, 반경, 선).
fn rings(p: &Painted, th: &Theme) -> Vec<(usize, egui::Rect, f32, egui::Stroke)> {
    let row_h = row_height(th).value();
    p.shapes
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            egui::Shape::Rect(r)
                if r.stroke.width > 0.0 && (r.rect.height() - row_h).abs() < 0.5 =>
            {
                Some((i, r.rect, f32::from(r.corner_radius.nw), r.stroke))
            }
            _ => None,
        })
        .collect()
}

/// 행 왼쪽 끝의 선택 막대: (도형 순번, 사각형, 채움).
fn bars(p: &Painted, th: &Theme) -> Vec<(usize, egui::Rect, egui::Color32)> {
    let row_h = row_height(th).value();
    let w = th.selection_edge_width.value();
    p.shapes
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            egui::Shape::Rect(r)
                if r.brush.is_none()
                    && (r.rect.width() - w).abs() < 0.01
                    && (r.rect.height() - row_h).abs() < 0.5 =>
            {
                Some((i, r.rect, r.fill))
            }
            _ => None,
        })
        .collect()
}

/// 행 전체 폭 · 높이의 채움(바탕): (도형 순번, 채움).
fn grounds(p: &Painted, th: &Theme) -> Vec<(usize, egui::Color32)> {
    let row_h = row_height(th).value();
    let w = th.fp_specimen_view_width().value();
    p.shapes
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            egui::Shape::Rect(r)
                if r.brush.is_none()
                    && r.stroke.width == 0.0
                    && r.fill != egui::Color32::TRANSPARENT
                    && (r.rect.height() - row_h).abs() < 0.5
                    && r.rect.width() > w * 0.5 =>
            {
                Some((i, r.fill))
            }
            _ => None,
        })
        .collect()
}

/// fp-row-focus-row-states: 여덟 칸 × 폴더 · 파일 행 × Mocha · Latte 에서 바탕 · 이름 · 메타 · 글리프 · 선택 막대 · 링이
/// `FpRow` 의 칸이다. 막대는 바탕 뒤, 링은 행을 다 그린 뒤 행 안쪽 `fp-row-focus-ring-width` · `radius-sm` 선이다.
/// 한 칸(테마 · 상태 · 행)의 바탕 · 이름 · 메타 · 글리프 · 선택 막대 · 링을 단정한다.
fn assert_row_cells(
    theme_name: &str,
    th: &Theme,
    (state, selected, hover, focus): (&str, bool, bool, bool),
    name: &str,
) {
    let r = seed(name);
    let case = format!("{theme_name} {state} {name}");
    let p = paint_row(th, r, selected, hover, focus);
    let ground = grounds(&p, th);
    let want_ground = tasty_ui_widgets::RowState::of(true, selected, hover, false).fill(th);
    if selected || hover {
        assert_eq!(ground.len(), 1, "{case}: 바탕 하나 {ground:?}");
        assert_eq!(ground[0].1, want_ground, "{case}: 바탕");
    } else {
        assert!(
            ground.is_empty(),
            "{case}: 쉬는 행은 바탕이 없다 {ground:?}"
        );
    }
    assert_eq!(
        text_color(&p, name),
        cell(
            selected,
            hover,
            [
                th.text_secondary(),
                th.text_secondary_raised(),
                th.text_on_active(),
                th.text_on_active_hover()
            ]
        ),
        "{case}: 이름"
    );
    let meta = cell(
        selected,
        hover,
        [
            th.text_muted(),
            th.fp_row_meta_fg_hover(),
            th.text_on_active(),
            th.text_on_active_hover(),
        ],
    );
    assert_eq!(text_color(&p, r.size), meta, "{case}: 크기");
    assert_eq!(text_color(&p, r.modified), meta, "{case}: 날짜");
    let glyph = if r.folder {
        cell(
            selected,
            hover,
            [
                th.accent_primary_raised(),
                th.accent_primary(),
                th.accent_primary_raised(),
                th.accent_primary_on_active(),
            ],
        )
    } else {
        cell(
            selected,
            hover,
            [
                th.text_muted(),
                th.fp_row_glyph_fg_hover(),
                th.text_muted(),
                th.text_on_active(),
            ],
        )
    };
    assert_eq!(p.glyphs.len(), 1, "{case}: 글리프 하나");
    assert_eq!(p.glyphs[0].1, glyph, "{case}: 글리프");
    let bar = bars(&p, th);
    if selected {
        assert_eq!(bar.len(), 1, "{case}: 선택 막대 {bar:?}");
        assert_eq!(
            bar[0].2,
            th.accent_primary_on_active().to_egui(),
            "{case}: 막대 색"
        );
        assert!(bar[0].0 > ground[0].0, "{case}: 막대는 바탕 위");
    } else {
        assert!(bar.is_empty(), "{case}: 선택 막대 없음 {bar:?}");
    }
    assert_ring(&case, th, &p, selected, hover, focus);
}

/// 행 포커스 링: 포커스 칸에만, 바탕 칸의 링 토큰 · `fp-row-focus-ring-width` · `radius-sm` 으로 맨 마지막에 그린다.
fn assert_ring(case: &str, th: &Theme, p: &Painted, selected: bool, hover: bool, focus: bool) {
    let ring = rings(p, th);
    if focus {
        assert_eq!(ring.len(), 1, "{case}: 링 하나 {ring:?}");
        let (i, _, radius, stroke) = ring[0];
        let ink = match (selected, hover) {
            (true, true) => th.fp_row_focus_ring_selected_hover(),
            (true, false) => th.fp_row_focus_ring_selected(),
            (false, _) => th.fp_row_focus_ring(),
        };
        assert_eq!(stroke.color, ink.to_egui(), "{case}: 링 색");
        assert_eq!(
            stroke.width,
            th.fp_row_focus_ring_width().value(),
            "{case}: 링 굵기"
        );
        assert_eq!(radius, th.corner_radius_sm.value(), "{case}: 링 모서리");
        assert_eq!(i, p.shapes.len() - 1, "{case}: 링은 마지막");
    } else {
        assert!(ring.is_empty(), "{case}: 링 없음 {ring:?}");
    }
}

#[test]
fn the_eight_row_states_draw_the_kit_row_cells() {
    for (theme_name, th) in mocha_latte() {
        for cell in ROW_STATES {
            for name in ["configs", "README.md"] {
                assert_row_cells(theme_name, &th, cell, name);
            }
        }
    }
}

/// 카드 하나가 그린 행 이름 중 사각형 `rect` 의 세로 가운데에 놓인 것.
fn row_named(p: &Painted, rect: egui::Rect) -> &'static str {
    let names: Vec<&'static str> = FILES
        .iter()
        .map(|f| f.name)
        .filter(|n| {
            p.texts.iter().any(|t| {
                t.text == *n
                    && rect.y_range().contains(t.rect.center().y)
                    && t.rect.left() > rect.left()
            })
        })
        .collect();
    assert_eq!(names.len(), 1, "행 하나: {names:?}");
    names[0]
}

/// 카드의 기대값 — 선택 행 · 링 행과 그 색 칸 · 그린 글자 · 확정 버튼(라벨, 켜짐).
struct Want {
    selected: &'static [&'static str],
    ring: Option<&'static str>,
    texts: Vec<&'static str>,
    confirm: (&'static str, bool),
}

fn want(label: &str) -> Want {
    use crate::i18n::t;
    let w = |selected, ring, texts: &[&'static str], confirm| Want {
        selected,
        ring,
        texts: texts.to_vec(),
        confirm,
    };
    match label {
        l if l.starts_with("Open · target spec") => w(
            &["package.json"],
            Some("package.json"),
            &["package.json"],
            (t("filepicker.open_button"), true),
        ),
        l if l.starts_with("Save · target spec") => w(
            &[],
            Some("package.json"),
            &["keybindings-2026-09-14.toml"],
            (t("filepicker.save.save_button"), true),
        ),
        l if l.starts_with("After navigation") => w(
            &[],
            None,
            &[t("filepicker.no_file_selected")],
            (t("filepicker.open_button"), false),
        ),
        l if l.starts_with("First ↓") => w(
            &["configs"],
            Some("configs"),
            &["configs is a folder — Open enters it."],
            (t("filepicker.open_button"), true),
        ),
        "fp-row-focus-first-up" => w(
            &[".env"],
            Some(".env"),
            &[".env"],
            (t("filepicker.open_button"), true),
        ),
        l if l.starts_with("Save mode · pointer") => w(
            &[],
            Some("deploy.sh"),
            &["keybindings-2026-09-14.toml"],
            (t("filepicker.save.save_button"), true),
        ),
        l if l.starts_with("Open mode · pointer") => w(
            &["README.md"],
            Some("README.md"),
            &["README.md"],
            (t("filepicker.open_button"), true),
        ),
        l if l.starts_with("Click README.md") => w(
            &["README.md"],
            None,
            &["README.md"],
            (t("filepicker.open_button"), true),
        ),
        l if l.starts_with("Then Space") => w(
            &["README.md"],
            Some("README.md"),
            &["README.md"],
            (t("filepicker.open_button"), true),
        ),
        "fp-row-focus-save-space-folder" => w(
            &["configs"],
            Some("configs"),
            &[
                "keybindings-2026-09-14.toml",
                "Folders aren't save targets — double-click configs to open it.",
            ],
            (t("filepicker.save.save_button"), true),
        ),
        "fp-row-focus-folder-skip" => w(
            &["node_modules"],
            Some("node_modules"),
            &["/Users/maya/projects/node_modules"],
            (t("picker.folder.confirm"), true),
        ),
        "fp-row-focus-folder-clear" => w(
            &[],
            None,
            &["/Users/maya/projects"],
            (t("picker.folder.confirm"), true),
        ),
        other => panic!("기대값 없는 카드 {other}"),
    }
}

/// 무대와 View 의 카드 — (기대값 열쇠, 변형). 설명이 없는 카드는 View 이름으로 찾는다.
fn all_cards() -> Vec<(&'static str, Variant)> {
    let mut out: Vec<_> = stage_cards().into_iter().collect();
    for (name, _, cards) in views() {
        out.extend(
            cards
                .into_iter()
                .map(|(cap, v)| (if cap.is_empty() { name } else { cap }, v)),
        );
    }
    out
}

/// 카드마다 `pick` · `focusRow` · `hoverRow` 가 디자인 `FilePickerFrame` 의 결과다: 선택 막대 행, 링 하나와 그 바탕
/// 칸의 잉크, 이름 칸 · 폴더 줄 · 경로 글자, 확정 버튼 라벨과 켜짐(꺼지면 `button-disabled-fg`).
#[test]
fn every_row_focus_card_draws_its_pick_ring_and_footer() {
    let cards = all_cards();
    assert_eq!(cards.len(), 2 + 10, "무대 둘 + View 카드 열");
    for (theme_name, th) in mocha_latte() {
        for (key, v) in &cards {
            let case = format!("{theme_name} {key}");
            let w = want(key);
            let p = paint(|ui| card(ui, &th, *v));
            let mut selected: Vec<&str> = bars(&p, &th)
                .into_iter()
                .filter(|(_, _, c)| *c == th.accent_primary_on_active().to_egui())
                .map(|(_, r, _)| row_named(&p, r))
                .collect();
            selected.sort_unstable();
            let mut want_sel = w.selected.to_vec();
            want_sel.sort_unstable();
            assert_eq!(selected, want_sel, "{case}: 선택 막대");
            let ring: Vec<_> = rings(&p, &th)
                .into_iter()
                .filter(|(_, _, radius, stroke)| {
                    *radius == th.corner_radius_sm.value()
                        && stroke.width == th.fp_row_focus_ring_width().value()
                })
                .collect();
            match w.ring {
                Some(name) => {
                    assert_eq!(ring.len(), 1, "{case}: 링 하나 {ring:?}");
                    assert_eq!(row_named(&p, ring[0].1), name, "{case}: 링 행");
                    let ink = match (w.selected.contains(&name), v.hover_row == Some(name)) {
                        (true, true) => th.fp_row_focus_ring_selected_hover(),
                        (true, false) => th.fp_row_focus_ring_selected(),
                        (false, _) => th.fp_row_focus_ring(),
                    };
                    assert_eq!(ring[0].3.color, ink.to_egui(), "{case}: 링 색");
                }
                None => assert!(ring.is_empty(), "{case}: 링 없음 {ring:?}"),
            }
            // 행 이름이기도 한 글자는 행과 이름 칸에 한 번씩, 나머지는 한 번 그린다.
            for text in &w.texts {
                let rows = usize::from(FILES.iter().any(|f| f.name == *text));
                assert_eq!(
                    p.texts.iter().filter(|t| t.text == *text).count(),
                    1 + rows,
                    "{case}: {text}"
                );
            }
            let (label, enabled) = w.confirm;
            // 폴더 모드는 제목도 Select folder 라 맨 아래 글자(footer 버튼)를 본다.
            let confirm = p
                .texts
                .iter()
                .filter(|t| t.text == label)
                .max_by(|a, b| a.rect.center().y.total_cmp(&b.rect.center().y))
                .unwrap_or_else(|| panic!("{case}: {label} 버튼"));
            assert_eq!(
                confirm.color == th.button_disabled_fg().to_egui(),
                !enabled,
                "{case}: {label} 켜짐 {enabled}"
            );
        }
    }
}
