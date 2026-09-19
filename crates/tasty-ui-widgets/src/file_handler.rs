//! File handler picker 의 **모델 규칙** — 본체와 갤러리 specimen 이 같은 함수를 부른다.
//!
//! 그리는 코드가 아니라 *무엇을 그릴지* 를 정하는 순수 함수만 둔다. 두 표면이 각자
//! 사본을 들면 값이 같아 보여도 언제든 갈리고, 갈린 뒤에는 어느 쪽이 정본인지 알 수
//! 없다 — `tokens.rs` 가 치수에 대해 같은 이유로 하나로 모여 있는 것과 같다.

use crate::tokens::{
    FH_ID_ELIDE_MAX, FH_ID_ELIDE_TAIL, FH_TARGET_ELIDE_FALLBACK, FH_TARGET_LINE_BOX,
};
use tasty_type_geometry::length::LogicalPx;

/// 행 둘째 줄의 handler id 를 **앞에서** 자른다.
///
/// reverse-DNS id 는 꼬리가 핸들러를 가르고 벤더 접두가 반복되는 부분이라, 잘라야 하면
/// 앞에서 자르고 그 결과를 좌→우로 그린다. `direction: rtl` 류의 뒤집기는 런을 재배열해
/// 정보가 있는 끝을 자른다.
pub fn elide_id_front(id: &str) -> String {
    let n = id.chars().count();
    if n <= FH_ID_ELIDE_MAX {
        return id.to_string();
    }
    let tail: String = id.chars().skip(n - FH_ID_ELIDE_TAIL).collect();
    format!("…{tail}")
}

/// 헤더 경로가 한 줄에 들어갈 **문자 예산**.
///
/// 디자인이 정한 것은 개수가 아니라 **측정**이다 — 라인 박스에 mono 글리프가 몇 개
/// 들어가는가. `mono_char_w` 는 그 폰트의 글리프 한 칸 폭이고, 잴 수 없을 때
/// (폰트 미로드 · 0 · 비유한)만 [`FH_TARGET_ELIDE_FALLBACK`] 로 떨어진다.
///
/// 폴백이 고정 상수인 이유: 못 잰 상태에서 추정하면 그 추정이 틀렸을 때 **잘못 자른
/// 경로**가 나가는데, 경로는 잘린 것이 티가 안 난다(`…/` 가 붙은 결과도 정상으로 보인다).
/// 파생 상한 하나로 떨어지면 적어도 어느 화면에서나 같은 값이다.
pub fn target_budget_chars(mono_char_w: LogicalPx) -> usize {
    let w = mono_char_w.value();
    if !w.is_finite() || w <= 0.0 {
        return FH_TARGET_ELIDE_FALLBACK;
    }
    let fits = (FH_TARGET_LINE_BOX.value() / w).floor();
    if !fits.is_finite() || fits < 1.0 {
        return FH_TARGET_ELIDE_FALLBACK;
    }
    fits as usize
}

/// 헤더 경로의 앞자름 — 파일명이 꼬리이고 그것이 파일을 식별한다.
///
/// 규칙은 두 단이다. 먼저 **앞 조각을 통째로 버리고** `…/` 를 접두해 나머지가 예산에
/// 들어갈 때까지 간다(`…/components/picker.rs`). 조각 하나가 줄보다 길어 그것으로도
/// 안 들어갈 때**만** 문자 단위로 앞을 자르고 맨 앞에 `…` 를 붙인다.
///
/// 조각 경계를 우선하는 이유는 경로가 그렇게 읽히기 때문이다 — 문자 중간에서 잘린
/// 디렉토리 이름은 없는 디렉토리로 읽힌다.
pub fn elide_target_front(s: &str, budget_chars: usize) -> String {
    if s.chars().count() <= budget_chars {
        return s.to_string();
    }
    let parts: Vec<&str> = s.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    for keep in (1..parts.len()).rev() {
        let tail = parts[parts.len() - keep..].join("/");
        if tail.chars().count() + 2 <= budget_chars {
            return format!("…/{tail}");
        }
    }
    elide_chars_front(s, budget_chars)
}

/// 문자 단위 앞자름 — 맨 앞 `…` 한 글자를 포함해 `max` 문자가 된다.
fn elide_chars_front(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max || max == 0 {
        return s.to_string();
    }
    let tail: String = s.chars().skip(n - (max - 1)).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 확정 시안이 값으로 준 세 표본. 예산은 파생 상한 70 이다.
    const A_FITS: &str = "work/tasty/crates/tasty-gallery/src/catalog/components/file_handler.rs";
    const B_SEGMENT: &str = "/home/maya/src/tasty-main/crates/tasty-gallery/src/catalog/components/file_handler_picker.rs";
    const C_ONE_SEGMENT: &str =
        "quarterly-revenue-reconciliation-draft-final-v3-reviewed-by-finance.xlsx";

    #[test]
    fn a_path_inside_the_budget_is_untouched() {
        assert_eq!(A_FITS.chars().count(), 70);
        assert_eq!(elide_target_front(A_FITS, FH_TARGET_ELIDE_FALLBACK), A_FITS);
    }

    #[test]
    fn a_long_path_loses_whole_leading_segments() {
        assert_eq!(B_SEGMENT.chars().count(), 92);
        let out = elide_target_front(B_SEGMENT, FH_TARGET_ELIDE_FALLBACK);
        assert_eq!(
            out,
            "…/crates/tasty-gallery/src/catalog/components/file_handler_picker.rs"
        );
        assert_eq!(out.chars().count(), 68);
        // 조각 경계에서 잘렸다 — `…/` 뒤는 온전한 디렉토리 이름이다.
        assert!(out.starts_with("…/crates/"));
    }

    #[test]
    fn one_segment_longer_than_the_line_falls_to_a_character_cut() {
        assert_eq!(C_ONE_SEGMENT.chars().count(), 72);
        let out = elide_target_front(C_ONE_SEGMENT, FH_TARGET_ELIDE_FALLBACK);
        assert_eq!(out.chars().count(), 70);
        assert!(out.starts_with('…'));
        assert!(!out.starts_with("…/"));
        assert!(out.ends_with("-reviewed-by-finance.xlsx"));
    }

    #[test]
    fn the_budget_is_measured_when_a_glyph_width_is_available() {
        // 390 / 5.5 = 70.9… → 70. 좁은 글리프면 더 많이 들어간다 — 상수였다면 안
        // 움직였을 값이다.
        assert_eq!(target_budget_chars(LogicalPx(5.5)), 70);
        assert_eq!(target_budget_chars(LogicalPx(3.9)), 100);
    }

    /// 파생 상한은 **파생**이라고 적혀 있으니 그 산식이 참인지를 재야 한다.
    ///
    /// 폴백 시험들이 `FH_TARGET_ELIDE_FALLBACK` 을 기댓값으로 쓰면 상수를 바꿔도
    /// 양변이 함께 움직여 아무 시험도 안 죽는다(실제로 그랬다 — 70 → 71 변이가
    /// 일곱 시험을 전부 살려 보냈다). 여기서 산식과 상수를 맞물려 고정한다:
    /// D2Coding 11px 의 5.5px/자로 라인 박스를 나눈 몫이 그 상수여야 한다.
    #[test]
    fn the_derived_cap_is_what_the_measurement_would_have_given() {
        assert_eq!(FH_TARGET_LINE_BOX.value(), 390.0);
        assert_eq!(
            target_budget_chars(LogicalPx(5.5)),
            FH_TARGET_ELIDE_FALLBACK,
            "폴백은 D2Coding 11px 로 실제로 쟀을 때의 값과 같아야 한다"
        );
    }

    #[test]
    fn an_unmeasurable_glyph_width_falls_back_to_the_derived_cap() {
        assert_eq!(
            target_budget_chars(LogicalPx(0.0)),
            FH_TARGET_ELIDE_FALLBACK
        );
        assert_eq!(
            target_budget_chars(LogicalPx(-1.0)),
            FH_TARGET_ELIDE_FALLBACK
        );
        assert_eq!(
            target_budget_chars(LogicalPx(f32::NAN)),
            FH_TARGET_ELIDE_FALLBACK
        );
        // 글리프가 줄보다 넓으면 한 글자도 안 들어간다 — 그것도 못 잰 것으로 친다.
        assert_eq!(
            target_budget_chars(FH_TARGET_LINE_BOX + LogicalPx(1.0)),
            FH_TARGET_ELIDE_FALLBACK
        );
    }

    #[test]
    fn a_long_id_keeps_its_tail() {
        let id = "net.example.enterprise.documents.attachments/inline-preview-handler";
        let out = elide_id_front(id);
        assert_eq!(out.chars().count(), FH_ID_ELIDE_MAX);
        assert!(out.starts_with('…'));
        assert!(out.ends_with("inline-preview-handler"));
    }

    #[test]
    fn a_short_id_is_untouched() {
        assert_eq!(
            elide_id_front("com.tasty.text/editor"),
            "com.tasty.text/editor"
        );
    }
}
