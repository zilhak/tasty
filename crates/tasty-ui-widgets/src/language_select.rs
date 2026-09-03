//! `language_select` — Settings › General 의 언어 드롭다운.
//!
//! [`select`] 위에 두 가지 판정을 얹는다.
//!
//! 1. 행 라벨은 표시 이름, 없으면 코드 그대로 — 호출자가 [`LanguageOption::label`] 에
//!    그 폴백을 채워 넘긴다(내장 3 + 발견된 언어팩 N, 순서도 호출자 소유).
//! 2. **현재 값이 목록에 없을 때**(언어팩 디렉토리가 지워진 뒤 등) 값을 덮어쓰지
//!    않는다 — `"<code> <missing_suffix>"` 행을 목록 끝에 붙여 선택 상태로 보여 주고,
//!    사용자가 다른 행을 고르기 전까지 설정값은 그대로 둔다. 그 행을 다시 골라도
//!    값은 변하지 않는다.
//!
//! 문구(`missing_suffix`)는 호출자 주입 — 이 crate 는 i18n 을 모른다
//! (`docs/dev-guide/i18n.md` "공용 위젯의 문자열 — 호출자 주입").

use tasty_type_appearance::theme::Theme;

use crate::select;

/// 콤보 한 행. `code` 는 설정값(`general.language`)에 들어가는 값, `label` 은 표시 이름
/// (없으면 호출자가 `code` 를 그대로 넣는다).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageOption<'a> {
    pub code: &'a str,
    pub label: &'a str,
}

/// 호출자가 주입하는 문구.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageSelectLabels<'a> {
    /// 목록에 없는 현재 코드 뒤에 붙는 표식 (예: `"(not found)"`).
    pub missing_suffix: &'a str,
}

/// 언어 드롭다운. `selected_code` 가 `options` 의 코드로 바뀌었을 때만 `true`.
/// 목록에 없는 현재 코드는 마지막 행으로 노출되며 그 행을 골라도 값은 바뀌지 않는다.
#[allow(clippy::too_many_arguments)]
pub fn language_select(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    selected_code: &mut String,
    options: &[LanguageOption<'_>],
    labels: &LanguageSelectLabels<'_>,
    width: f32,
    enabled: bool,
) -> bool {
    let (rows, mut idx) = resolve_rows(options, selected_code, labels.missing_suffix);
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    if !select(ui, theme, id_salt, &mut idx, &refs, width, enabled) {
        return false;
    }
    match options.get(idx) {
        Some(opt) if opt.code != selected_code.as_str() => {
            *selected_code = opt.code.to_string();
            true
        }
        // 같은 행 재선택, 또는 목록 밖 "missing" 행 — 값을 건드리지 않는다.
        _ => false,
    }
}

/// 순수 판정: (행 라벨들, 선택 인덱스). `current` 가 `options` 에 없으면 마지막에
/// `"<current> <suffix>"` 행을 붙이고 그 인덱스를 돌려준다.
fn resolve_rows(
    options: &[LanguageOption<'_>],
    current: &str,
    missing_suffix: &str,
) -> (Vec<String>, usize) {
    let mut rows: Vec<String> = options.iter().map(|o| o.label.to_string()).collect();
    match options.iter().position(|o| o.code == current) {
        Some(idx) => (rows, idx),
        None => {
            rows.push(format!("{current} {missing_suffix}"));
            let idx = rows.len() - 1;
            (rows, idx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPTS: &[LanguageOption<'static>] = &[
        LanguageOption {
            code: "en",
            label: "English",
        },
        LanguageOption {
            code: "ko",
            label: "한국어",
        },
        LanguageOption {
            code: "xx",
            label: "xx",
        },
    ];

    #[test]
    fn rows_use_labels_and_select_current() {
        let (rows, idx) = resolve_rows(OPTS, "ko", "(not found)");
        assert_eq!(rows, ["English", "한국어", "xx"]);
        assert_eq!(idx, 1);
    }

    #[test]
    fn rows_append_missing_row_for_unknown_current() {
        let (rows, idx) = resolve_rows(OPTS, "zz", "(not found)");
        assert_eq!(rows, ["English", "한국어", "xx", "zz (not found)"]);
        assert_eq!(idx, 3);
    }
}
