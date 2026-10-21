//! 고정 문자열 값 몇 개 중 하나를 고르는 설정 행의 드롭다운. 공용 Select 로 그린다.
//! 저장 값이 목록에 없으면(설정 파일 수동 편집 등) 그 값을 "값 (알 수 없음)" 행으로 끝에 덧붙여
//! 선택해 둔다. 이 행을 골라도 값은 그대로이고, 목록의 값을 고르면 0번 옵션이라도 바로 반영된다.

use crate::i18n::t;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::select;

/// 선택지 하나. `value` 는 설정에 저장하는 문자열, `label` 은 표시 문구.
pub(super) struct Choice<'a> {
    pub value: &'a str,
    pub label: &'a str,
}

/// `value` 가 `choices` 의 다른 값으로 바뀌었을 때만 `true`.
pub(super) fn choice_select(
    ui: &mut egui::Ui,
    th: &Theme,
    id_salt: &str,
    value: &mut String,
    choices: &[Choice<'_>],
    width: LogicalPx,
) -> bool {
    let (rows, mut idx) = resolve_rows(choices, value, t("settings.choice_missing_suffix"));
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    if !select(ui, th, id_salt, &mut idx, &refs, width.value(), true) {
        return false;
    }
    match choices.get(idx) {
        Some(c) if c.value != value.as_str() => {
            *value = c.value.to_string();
            true
        }
        // 목록 밖 값의 행 — 값을 건드리지 않는다.
        _ => false,
    }
}

/// (행 라벨들, 선택 인덱스). `current` 가 목록에 없으면 마지막에 `"<current> <suffix>"` 행을 붙인다.
fn resolve_rows(
    choices: &[Choice<'_>],
    current: &str,
    missing_suffix: &str,
) -> (Vec<String>, usize) {
    let mut rows: Vec<String> = choices.iter().map(|c| c.label.to_string()).collect();
    match choices.iter().position(|c| c.value == current) {
        Some(idx) => (rows, idx),
        None => {
            rows.push(format!("{current} {missing_suffix}"));
            (rows, choices.len())
        }
    }
}

/// 설정 탭을 헤드리스 egui 로 그려 Select 트리거를 찾고 클릭을 흉내 낸다. 탭별 시험이 함께 쓴다.
#[cfg(test)]
pub(super) mod probe {
    use crate::view::plugins::ui::text_probe::visible_text_rects;

    pub const SCREEN: egui::Rect = egui::Rect {
        min: egui::Pos2::ZERO,
        max: egui::pos2(900.0, 900.0),
    };

    /// 입력 없이 한 프레임을 그린다.
    pub fn frame(ctx: &egui::Context, draw: &mut dyn FnMut(&mut egui::Ui)) -> egui::FullOutput {
        run(ctx, Vec::new(), draw)
    }

    fn run(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        draw: &mut dyn FnMut(&mut egui::Ui),
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(SCREEN),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| draw(ui));
            },
        )
    }

    /// `pos` 를 누르는 프레임과 떼는 프레임을 그린 뒤, 새로 연 팝업이 보이도록 두 프레임을 더 그린다.
    pub fn click(
        ctx: &egui::Context,
        pos: egui::Pos2,
        draw: &mut dyn FnMut(&mut egui::Ui),
    ) -> egui::FullOutput {
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        run(ctx, vec![egui::Event::PointerMoved(pos), press(true)], draw);
        run(ctx, vec![press(false)], draw);
        frame(ctx, draw);
        frame(ctx, draw)
    }

    pub fn text_rects(output: &egui::FullOutput, text: &str) -> Vec<egui::Rect> {
        visible_text_rects(output, SCREEN)
            .into_iter()
            .filter(|(s, _)| s == text)
            .map(|(_, r)| r)
            .collect()
    }

    /// `text` 를 담은 가장 작은 Select 트리거 상자(select-bg 채움). 없으면 `None`.
    pub fn trigger_around(output: &egui::FullOutput, text: &str) -> Option<egui::Rect> {
        let th = crate::theme::theme();
        let fill = th.select_bg().to_egui();
        let label = *text_rects(output, text).first()?;
        let mut boxes = Vec::new();
        fn walk(shape: &egui::Shape, fill: egui::Color32, out: &mut Vec<egui::Rect>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, fill, out)),
                egui::Shape::Rect(r) if r.fill == fill => out.push(r.rect),
                _ => {}
            }
        }
        for clipped in &output.shapes {
            walk(&clipped.shape, fill, &mut boxes);
        }
        boxes
            .into_iter()
            .filter(|b| b.contains(label.center()))
            .min_by(|a, b| a.area().total_cmp(&b.area()))
    }

    /// 트리거를 눌러 목록을 열고 `option` 행을 누른다. 끝난 뒤의 출력을 돌려준다.
    pub fn pick(
        ctx: &egui::Context,
        draw: &mut dyn FnMut(&mut egui::Ui),
        trigger_text: &str,
        option: &str,
    ) -> egui::FullOutput {
        frame(ctx, draw);
        let out = frame(ctx, draw);
        let trigger = trigger_around(&out, trigger_text)
            .unwrap_or_else(|| panic!("no Select trigger around {trigger_text:?}"));
        let opened = click(ctx, trigger.center(), draw);
        let row = *text_rects(&opened, option)
            .iter()
            .find(|r| !trigger.contains(r.center()))
            .unwrap_or_else(|| panic!("option {option:?} not in the open list"));
        click(ctx, row.center(), draw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHOICES: &[Choice<'static>] = &[
        Choice {
            value: "ask",
            label: "Ask",
        },
        Choice {
            value: "quit",
            label: "Quit",
        },
    ];

    #[test]
    fn a_listed_value_selects_its_row() {
        let (rows, idx) = resolve_rows(CHOICES, "quit", "(unknown)");
        assert_eq!(rows, ["Ask", "Quit"]);
        assert_eq!(idx, 1);
    }

    #[test]
    fn an_unlisted_value_is_appended_and_selected() {
        let (rows, idx) = resolve_rows(CHOICES, "bogus", "(unknown)");
        assert_eq!(rows, ["Ask", "Quit", "bogus (unknown)"]);
        assert_eq!(idx, 2);
    }

    fn draw(value: &mut String) -> impl FnMut(&mut egui::Ui) + '_ {
        move |ui| {
            let th = crate::theme::theme();
            choice_select(ui, &th, "probe", value, CHOICES, th.field_width_lg);
        }
    }

    /// 목록 밖 값은 그리기만으로 바뀌지 않고, 0번 옵션을 고르면 그 값으로 고쳐진다.
    #[test]
    fn an_unlisted_value_survives_drawing_and_the_first_option_fixes_it() {
        let ctx = egui::Context::default();
        let mut value = "bogus".to_string();
        let missing = format!("bogus {}", t("settings.choice_missing_suffix"));
        let out = probe::frame(&ctx, &mut draw(&mut value));
        assert!(probe::trigger_around(&out, &missing).is_some());
        assert_eq!(value, "bogus");
        probe::pick(&ctx, &mut draw(&mut value), &missing, "Ask");
        assert_eq!(value, "ask");
    }

    /// 목록 밖 값의 행을 다시 골라도 값은 그대로다.
    #[test]
    fn picking_the_unlisted_row_keeps_the_value() {
        let ctx = egui::Context::default();
        let mut value = "bogus".to_string();
        let missing = format!("bogus {}", t("settings.choice_missing_suffix"));
        probe::pick(&ctx, &mut draw(&mut value), &missing, &missing);
        assert_eq!(value, "bogus");
    }
}
