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
/// 들어가는가. 잴 수 없을 때(폰트 미로드 · 0 · 비유한)만
/// [`FH_TARGET_ELIDE_FALLBACK`] 로 떨어진다.
///
/// ★ `mono_advance` 는 글리프 **한 개의 폭**이 아니라 **한 글자 늘어날 때 깔리는
/// 증분**이다. 두 값은 다르다 — egui 가 advance 를 정수 픽셀로 반올림하기 때문이고,
/// 단일 글자 폭을 넘기면 예산이 8% 넉넉하게 나와 모델이 "들어간다" 고 판정한 경로가
/// 화면에서 잘린다. 값과 근거는 [`crate::tokens::FH_TARGET_MONO_ADVANCE`].
///
/// 폴백이 고정 상수인 이유: 못 잰 상태에서 추정하면 그 추정이 틀렸을 때 **잘못 자른
/// 경로**가 나가는데, 경로는 잘린 것이 티가 안 난다(`…/` 가 붙은 결과도 정상으로 보인다).
/// 파생 상한 하나로 떨어지면 적어도 어느 화면에서나 같은 값이다.
pub fn target_budget_chars(mono_advance: LogicalPx) -> usize {
    let w = mono_advance.value();
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
    use crate::tokens::FH_TARGET_MONO_ADVANCE;

    /// 확정 시안이 값으로 준 세 표본. 시안이 적은 출력은 70 / 92→68 / 72→70 이고,
    /// 그 셋은 전부 **파생 상한이 70 이던 때**의 값이다 — 상한이 65 로 재파생된 지금
    /// 셋 다 다른 자리에서 잘린다. 표본 문자열 자체는 디자인 값이라 여기서 안 바꾼다.
    const A_FITS: &str = "work/tasty/crates/tasty-gallery/src/catalog/components/file_handler.rs";
    const B_SEGMENT: &str = "/home/maya/src/tasty-main/crates/tasty-gallery/src/catalog/components/file_handler_picker.rs";
    const C_ONE_SEGMENT: &str =
        "quarterly-revenue-reconciliation-draft-final-v3-reviewed-by-finance.xlsx";

    /// 예산 안에 드는 경로는 손대지 않는다.
    ///
    /// 표본 A 는 이 갈래를 더 이상 안 태운다(70 자 > 65). 갈래 자체는 남아 있으므로
    /// 시안 표본이 아닌 짧은 경로로 그것을 덮는다 — 표본을 줄여서 통과시키면
    /// 디자인 값을 내가 정하는 것이 된다.
    #[test]
    fn a_path_inside_the_budget_is_untouched() {
        let short = "docs/architecture.md";
        assert!(short.chars().count() <= FH_TARGET_ELIDE_FALLBACK);
        assert_eq!(elide_target_front(short, FH_TARGET_ELIDE_FALLBACK), short);
    }

    /// 표본 A 가 새 예산을 **넘는다**는 사실을 값으로 박는다.
    ///
    /// 이 시험은 고쳐야 할 것을 고정한다 — 디자인이 ≤65 자 표본을 새로 정하면 그때
    /// 같이 움직인다. 지금 조용히 잘린 채로 두면 Spec 이 "fits, untouched" 라고 말하는
    /// 그림에서 실제로는 조각이 버려진 경로가 그려진다.
    #[test]
    fn the_first_design_sample_overflows_the_recomputed_budget() {
        assert_eq!(A_FITS.chars().count(), 70);
        assert_eq!(FH_TARGET_ELIDE_FALLBACK, 65);
        let out = elide_target_front(A_FITS, FH_TARGET_ELIDE_FALLBACK);
        assert_ne!(out, A_FITS, "표본 A 는 이제 잘린다");
        assert_eq!(
            out,
            "…/crates/tasty-gallery/src/catalog/components/file_handler.rs"
        );
    }

    #[test]
    fn a_long_path_loses_whole_leading_segments() {
        assert_eq!(B_SEGMENT.chars().count(), 92);
        let out = elide_target_front(B_SEGMENT, FH_TARGET_ELIDE_FALLBACK);
        assert_eq!(
            out,
            "…/tasty-gallery/src/catalog/components/file_handler_picker.rs"
        );
        assert_eq!(out.chars().count(), 61);
        // 조각 경계에서 잘렸다 — `…/` 뒤는 온전한 디렉토리 이름이다.
        assert!(out.starts_with("…/tasty-gallery/"));
    }

    #[test]
    fn one_segment_longer_than_the_line_falls_to_a_character_cut() {
        assert_eq!(C_ONE_SEGMENT.chars().count(), 72);
        let out = elide_target_front(C_ONE_SEGMENT, FH_TARGET_ELIDE_FALLBACK);
        assert_eq!(out.chars().count(), 65);
        assert!(out.starts_with('…'));
        assert!(!out.starts_with("…/"));
        assert!(out.ends_with("-reviewed-by-finance.xlsx"));
    }

    /// 결과는 어떤 입력에서도 예산을 **넘지 않는다** — `…/` 두 글자도 예산에서 낸다.
    ///
    /// 시안이 준 표본 셋은 이 경계를 안 밟는다. 셋 다 고른 조각마다 여유가 남아,
    /// `+ 2` 를 `+ 1` 로 바꾸는 변이가 같은 조각을 뽑고 아무 시험도 안 죽었다(실측).
    /// 그래서 경계를 직접 만든다: 조각을 하나 버린 꼬리가 예산보다 정확히 한 글자
    /// 짧아, `…/` 를 붙이면 예산을 한 글자 넘기는 경로.
    #[test]
    fn the_ellipsis_prefix_is_paid_for_out_of_the_budget() {
        let budget = FH_TARGET_ELIDE_FALLBACK;
        let mid = "m".repeat(budget - 1 - "/file.rs".chars().count());
        let path = format!("aaa/{mid}/file.rs");
        assert!(path.chars().count() > budget);
        // 조각 하나를 버린 꼬리는 예산에 들어가지만(`budget - 1`), 접두 두 글자까지
        // 세면 안 들어간다 — 그러면 조각을 하나 **더** 버려야 한다.
        assert_eq!(format!("{mid}/file.rs").chars().count(), budget - 1);
        let out = elide_target_front(&path, budget);
        assert!(
            out.chars().count() <= budget,
            "{} 자: {out}",
            out.chars().count()
        );
        assert_eq!(out, "…/file.rs");
    }

    #[test]
    fn the_budget_is_measured_when_a_glyph_width_is_available() {
        // 390 / 6.0 = 65. 좁은 글리프면 더 많이 들어간다 — 상수였다면 안 움직였을 값이다.
        assert_eq!(target_budget_chars(LogicalPx(6.0)), 65);
        assert_eq!(target_budget_chars(LogicalPx(3.9)), 100);
    }

    /// 파생 상한은 **파생**이라고 적혀 있으니 그 산식이 참인지를 재야 한다.
    ///
    /// 폴백 시험들이 `FH_TARGET_ELIDE_FALLBACK` 을 기댓값으로 쓰면 상수를 바꿔도
    /// 양변이 함께 움직여 아무 시험도 안 죽는다(실제로 그랬다 — 70 → 71 변이가
    /// 일곱 시험을 전부 살려 보냈다). 여기서 산식과 상수를 맞물려 고정한다:
    /// 라인 박스를 **깔리는** advance 로 나눈 몫이 그 상수여야 한다.
    ///
    /// 분모가 공칭 advance(5.5556)가 아니라 6.0 인 이유는
    /// [`FH_TARGET_MONO_ADVANCE`] 에 있다 — 공칭으로 나누면 70 이 나오고, 그 70 자는
    /// 깔리면 라인 박스를 29.56px 넘긴다.
    #[test]
    fn the_derived_cap_is_what_the_measurement_would_have_given() {
        assert_eq!(FH_TARGET_LINE_BOX.value(), 390.0);
        assert_eq!(FH_TARGET_MONO_ADVANCE.value(), 6.0);
        assert_eq!(
            target_budget_chars(FH_TARGET_MONO_ADVANCE),
            FH_TARGET_ELIDE_FALLBACK,
            "폴백은 깔리는 advance 로 실제로 쟀을 때의 값과 같아야 한다"
        );
        // 공칭 advance 로 나누면 시안이 적은 70 이 나온다 — 그것이 틀린 이유가
        // 값으로 남아야 다음 사람이 되돌리지 않는다.
        assert_eq!(target_budget_chars(LogicalPx(5.5556)), 70);
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
