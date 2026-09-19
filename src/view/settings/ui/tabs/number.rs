//! 설정 창의 **숫자 입력 한 모양** — mono `Input` + 필드 밖 정적 suffix.
//!
//! 디자인 `gallery/overlays-windows.jsx` 의 "Numbers in settings — one shape".
//! 설정 안에 숫자 컨트롤이 셋(정적 suffix 를 단 mono Input · drag 숫자 · 제안된
//! stepper) 있었고 **첫째로 통일**했다. 이 모듈이 그 한 모양이고, 설정 창과 plugin
//! 기여 설정의 **모든** 숫자 칸이 여기를 지난다.
//!
//! 세 가지가 이 모양의 내용이다:
//!
//! - **자릿수 우측 정렬** — 설정 열을 내려가며 자리가 맞아야 두 값을 눈으로 견준다.
//! - **commit 때만 clamp** — 치는 동안은 건드리지 않는다. 25~200 칸에 `150` 을 칠 때
//!   `1` 다음 키에서 값이 끌려가면 그 다음 글자를 못 친다. 확정은 blur 와 `↵` 뿐이다.
//! - **범위 밖은 말로 알린다** — danger 테두리 + 한 줄. 그 줄은 범위와 **확정될 값**을
//!   같이 적는다. 막지 않고 무엇이 될지 미리 보여 주는 쪽이다.
//!
//! drag 숫자는 폐기했다 — 스크롤하는 설정 pane 안에서 제스처가 스크롤과 부딪히고,
//! 자기 범위를 넘겨 보기 전까지 그 범위가 안 보인다. stepper 도 폐기했다 — 한 번
//! 정하고 마는 칸에 히트 타깃을 둘 더 만든다.

use crate::i18n::t_args;
use tasty_type_appearance::theme::Theme;

/// 숫자 칸 하나의 계약. 범위·표시 자릿수·단위는 호출처가 정한다.
pub(super) struct NumberSpec<'a> {
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// 확정 시 스냅할 눈금. `None` 이면 스냅하지 않는다.
    pub step: Option<f64>,
    /// 표시 소수 자릿수. `0` 이면 정수 표기.
    pub decimals: usize,
    /// 필드 **밖**에 muted 로 찍는 단위. 타이핑 대상이 아니다.
    pub suffix: Option<&'a str>,
    /// 단위를 mono 로 찍는가. 디자인은 `%` 를 UI 글꼴로, 원격 전송의 `MiB` 를 mono 로
    /// 적는다 — 한 모양 안에서 그 한 축만 호출처가 고른다.
    pub suffix_mono: bool,
    pub enabled: bool,
}

impl<'a> NumberSpec<'a> {
    /// 정수 칸의 기본형 — 눈금·소수 없음.
    pub fn int(min: f64, max: f64) -> Self {
        Self {
            min: Some(min),
            max: Some(max),
            step: None,
            decimals: 0,
            suffix: None,
            suffix_mono: false,
            enabled: true,
        }
    }

    pub fn suffix(mut self, suffix: &'a str) -> Self {
        self.suffix = Some(suffix);
        self
    }

    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }

    pub fn step(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn format(&self, v: f64) -> String {
        format!("{v:.*}", self.decimals)
    }
}

/// 확정(blur / `↵`) 한 번이 내는 답. 치는 동안에는 이 판정을 부르지 않는다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Commit {
    /// 숫자로 안 읽힌다 — 마지막 확정값을 그대로 둔다.
    Unparsed,
    /// 이 값으로 확정한다. 범위·눈금을 이미 지났다.
    Value(f64),
}

/// 친 글자 하나를 확정값으로 옮긴다. 그리기와 무관한 순수 판정이라 단위 테스트가 든다.
pub(super) fn commit(text: &str, spec: &NumberSpec<'_>) -> Commit {
    let Ok(parsed) = text.trim().parse::<f64>() else {
        return Commit::Unparsed;
    };
    if !parsed.is_finite() {
        return Commit::Unparsed;
    }
    Commit::Value(settle(parsed, spec))
}

/// 눈금 스냅 → 범위 clamp. 순서가 이 방향인 이유는 눈금이 범위 밖으로 내보낼 수 있기
/// 때문이다(1.0~10.0 · 눈금 0.5 에서 10.3 은 10.5 로 스냅됐다가 10.0 으로 잘린다).
fn settle(v: f64, spec: &NumberSpec<'_>) -> f64 {
    let mut out = v;
    if let Some(step) = spec.step
        && step > 0.0
    {
        out = (out / step).round() * step;
    }
    if let Some(lo) = spec.min {
        out = out.max(lo);
    }
    if let Some(hi) = spec.max {
        out = out.min(hi);
    }
    out
}

/// 지금 친 글자가 확정되면 값이 끌려가는가. 그럴 때만 경고 줄이 나온다.
fn out_of_range(text: &str, spec: &NumberSpec<'_>) -> Option<f64> {
    match commit(text, spec) {
        Commit::Value(settled) => {
            let typed = text.trim().parse::<f64>().ok()?;
            (settled != typed).then_some(settled)
        }
        Commit::Unparsed => None,
    }
}

/// 경고 한 줄 — 범위와 **확정될 값**을 함께 적는다.
fn range_line(spec: &NumberSpec<'_>, settled: f64) -> String {
    let settled = spec.format(settled);
    match (spec.min, spec.max) {
        (Some(lo), Some(hi)) => t_args(
            "settings.number.range_between",
            &[&spec.format(lo), &spec.format(hi), &settled],
        ),
        (Some(lo), None) => t_args(
            "settings.number.range_at_least",
            &[&spec.format(lo), &settled],
        ),
        (None, Some(hi)) => t_args(
            "settings.number.range_at_most",
            &[&spec.format(hi), &settled],
        ),
        (None, None) => t_args("settings.number.range_snapped", &[&settled]),
    }
}

/// 숫자 칸 한 개를 그린다. `value` 는 확정된 값이고, 확정이 일어난 프레임에만 바뀐다.
///
/// 반환값은 **이 프레임에 확정이 일어났는가**다 — 호출처가 저장·재계산을 거는 자리다.
pub(super) fn number_field(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: impl std::hash::Hash,
    spec: &NumberSpec<'_>,
    value: &mut f64,
) -> bool {
    // 프레임 간 유지되는 편집 버퍼. 포커스가 없는 동안에는 확정값의 표기로 되돌린다.
    let buf_id = egui::Id::new(("settings_number_buf", ui.id(), &id_salt));
    let mut buf = ui
        .data_mut(|d| d.get_temp::<String>(buf_id))
        .unwrap_or_else(|| spec.format(*value));

    let pending = out_of_range(&buf, spec);
    let mut committed = false;

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let resp = tasty_ui_widgets::Input::new()
                .mono(true)
                .align(egui::Align::RIGHT)
                .width(theme.field_width_xs.value())
                .enabled(spec.enabled)
                .invalid(pending.is_some())
                .show(ui, theme, &mut buf);

            if resp.lost_focus() {
                // 확정 — blur 와 `↵` 둘 다 여기로 온다.
                if let Commit::Value(v) = commit(&buf, spec)
                    && v != *value
                {
                    *value = v;
                    committed = true;
                }
                buf = spec.format(*value);
            } else if !resp.has_focus() {
                // 편집 중이 아니면 바깥에서 바뀐 값을 그대로 비춘다.
                let synced = spec.format(*value);
                if buf != synced {
                    buf = synced;
                }
            }

            if let Some(sfx) = spec.suffix {
                let ink = if spec.enabled {
                    theme.text_muted()
                } else {
                    theme.text_disabled()
                };
                // 단위는 **12**(`font_size_term_sm`) 다 — 경고 줄의 11
                // (`font_size_caption`)과 다른 자리다. 디자인 원본이 두 값을 한 행
                // 안에서 갈라 적는다.
                let mut text = egui::RichText::new(sfx)
                    .size(theme.font_size_term_sm.value())
                    .color(ink);
                if spec.suffix_mono {
                    text = text.monospace();
                }
                ui.label(text);
            }
        });

        if let Some(settled) = pending {
            ui.label(
                egui::RichText::new(range_line(spec, settled))
                    .size(theme.font_size_caption.value())
                    .color(theme.accent_danger()),
            );
        }
    });

    ui.data_mut(|d| d.insert_temp(buf_id, buf));
    committed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> NumberSpec<'static> {
        NumberSpec::int(25.0, 200.0)
    }

    #[test]
    fn a_value_inside_the_range_commits_as_typed() {
        assert_eq!(commit("150", &spec()), Commit::Value(150.0));
    }

    #[test]
    fn a_value_past_the_edge_commits_as_the_edge() {
        assert_eq!(commit("420", &spec()), Commit::Value(200.0));
        assert_eq!(commit("1", &spec()), Commit::Value(25.0));
    }

    #[test]
    fn a_half_typed_value_is_not_a_number_and_keeps_the_last_one() {
        // 치는 동안의 중간 상태 — 확정이 여기서 일어나지 않으므로 값이 안 끌려간다.
        assert_eq!(commit("", &spec()), Commit::Unparsed);
        assert_eq!(commit("-", &spec()), Commit::Unparsed);
        assert_eq!(commit("1e", &spec()), Commit::Unparsed);
        assert_eq!(commit("nan", &spec()), Commit::Unparsed);
    }

    #[test]
    fn the_step_snaps_before_the_range_clips() {
        // 1.0~10.0 · 눈금 0.5. 10.3 은 10.5 로 스냅됐다가 위 끝으로 잘린다 — 스냅이
        // 범위 밖으로 내보낼 수 있으므로 clamp 가 뒤에 와야 한다.
        let s = NumberSpec::int(1.0, 10.0).step(0.5).decimals(1);
        assert_eq!(commit("10.3", &s), Commit::Value(10.0));
        assert_eq!(commit("2.3", &s), Commit::Value(2.5));
        assert_eq!(commit("2.2", &s), Commit::Value(2.0));
    }

    #[test]
    fn the_warning_line_appears_only_when_the_value_would_be_pulled() {
        let s = spec();
        assert_eq!(out_of_range("150", &s), None);
        assert_eq!(out_of_range("420", &s), Some(200.0));
        // 숫자로 안 읽히는 중간 상태는 경고가 아니다 — 아직 아무것도 확정 안 됐다.
        assert_eq!(out_of_range("", &s), None);
    }

    #[test]
    fn a_range_with_no_edges_never_warns() {
        let s = NumberSpec {
            min: None,
            max: None,
            step: None,
            decimals: 0,
            suffix: None,
            suffix_mono: false,
            enabled: true,
        };
        assert_eq!(commit("99999", &s), Commit::Value(99999.0));
        assert_eq!(out_of_range("99999", &s), None);
    }

    #[test]
    fn the_display_keeps_the_declared_decimals() {
        assert_eq!(spec().format(150.0), "150");
        assert_eq!(NumberSpec::int(1.0, 10.0).decimals(1).format(2.0), "2.0");
        assert_eq!(NumberSpec::int(0.8, 2.0).decimals(2).format(1.2), "1.20");
    }
}
