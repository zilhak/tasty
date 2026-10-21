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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::settings::ui::select_probe as probe;

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
