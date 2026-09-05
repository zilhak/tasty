//! Toast 카드 그리기 공통 헬퍼.
//!
//! `widgets/toast.rs` (단일 카드 데모) 와 `components/toast.rs` (스택 데모) 가
//! 각각 중복 정의하던 `ToastKind` / `accent_color` / 카드 chrome (bg rect + border +
//! accent bar + text galley) 을 한곳으로 통합한다.
//!
//! 색·폰트·치수 토큰은 모두 호출부가 `Theme` 에서 뽑아 그대로 넘기므로 시각 무변경.
//! 스택 데모는 alpha 를 미리 곱한 색을 넘기고, 단일 카드 데모는 alpha=1.0 (=곱 항등)
//! 으로 같은 헬퍼를 호출한다.
//!
//! 카드 구조 치수는 본체와 **같은 상수**를 읽는다 — `tasty-ui-widgets::tokens` 가
//! 단일 출처다. 예전에는 여기서 같은 값을 다시 정의했는데, 값이 같아 보여도 정의가
//! 둘이면 언제든 갈릴 수 있고 갈린 뒤엔 어느 쪽이 정본인지 알 수 없다.

use tasty_type_appearance::theme::Theme;

pub use tasty_ui_widgets::tokens::{
    TOAST_ACCENT_BAR_WIDTH as ACCENT_BAR_WIDTH, TOAST_PADDING_X as PADDING_X,
    TOAST_PADDING_Y as PADDING_Y,
};

/// 본체 정본 `crates/tasty-model/src/toast_kind.rs::ToastKind` 와 **kind-for-kind
/// 동일**해야 한다 — 본체가 만들 수 없는 종류를 갤러리가 전시하면 demo=main 등가성이
/// 한 방향으로 깨진다(`docs/design/policies/gallery-completeness.md`). 정본을 그대로
/// import 하지 않는 이유는 그 크레이트가 termwiz/터미널 모델까지 끌고 오기 때문이고,
/// 본체 binary 의존 때문이 아니다.
///
/// 그 "동일해야 한다" 를 **이 파일 맨 아래의 `mod tests` 가 강제한다** — 두
/// [`ToastKind::ALL`] 을 런타임에 열거해 양방향으로 대조하므로, 어느 한쪽에만 변종을
/// 더하면 실패한다. 대조는 `dev-dependencies` 의 정본 크레이트로 하므로 갤러리
/// 산출물에는 termwiz 가 들어가지 않는다.
#[derive(Clone, Copy, Debug)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

impl ToastKind {
    /// 모든 변종 — 정본과의 대조 축. 변종을 더하면 여기에도 더한다.
    pub const ALL: &'static [ToastKind] = &[
        ToastKind::Info,
        ToastKind::Success,
        ToastKind::Warning,
        ToastKind::Error,
    ];
}

/// kind → accent 색. 본체 `toast.rs` 와 동일한 accent 매핑.
pub fn accent_color(kind: ToastKind, theme: &Theme) -> egui::Color32 {
    match kind {
        ToastKind::Info => theme.accent_primary().into(),
        ToastKind::Success => theme.accent_success().into(),
        ToastKind::Warning => theme.accent_warning().into(),
        ToastKind::Error => theme.accent_danger().into(),
    }
}

/// 카드 chrome 색 묶음 (호출부가 alpha 반영 후 최종 색을 채워 넘긴다).
#[derive(Clone, Copy)]
pub struct CardColors {
    /// 카드 배경.
    pub bg: egui::Color32,
    /// 카드 border.
    pub border: egui::Color32,
    /// 좌측 accent bar.
    pub accent: egui::Color32,
    /// galley fallback 텍스트 색 (galley 자체도 이 색으로 layout 됨).
    pub text: egui::Color32,
}

/// 토스트 카드 1장의 chrome 을 `rect` 안에 그린다.
///
/// 호출부가 (alpha 반영 후) 최종 색을 `CardColors` 로 넘긴다 — 단일 카드 데모는
/// alpha=1.0, 스택 데모는 `gamma_multiply(alpha)` 한 색을 그대로 전달. galley 의
/// 텍스트 색도 호출부가 결정해 layout 해 둔 것을 사용한다.
pub fn draw_card(
    painter: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    colors: CardColors,
    galley: std::sync::Arc<egui::Galley>,
) {
    painter.rect_filled(rect, theme.corner_radius.value(), colors.bg);
    painter.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), colors.border),
        egui::StrokeKind::Inside,
    );

    let bar_rect = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.min.x + ACCENT_BAR_WIDTH, rect.max.y),
    );
    let bar_radius = egui::CornerRadius {
        nw: theme.corner_radius.value() as u8,
        sw: theme.corner_radius.value() as u8,
        ne: 0,
        se: 0,
    };
    painter.rect_filled(bar_rect, bar_radius, colors.accent);

    let text_pos = egui::pos2(
        rect.min.x + ACCENT_BAR_WIDTH + PADDING_X,
        rect.min.y + PADDING_Y,
    );
    painter.galley(text_pos, galley, colors.text);
}

#[cfg(test)]
mod tests {
    //! 미러 ↔ 정본 `ToastKind` 양방향 대조.
    //!
    //! ## 왜 `tests/` 가 아니라 lib 유닛 테스트인가 (관례를 깬 이유)
    //!
    //! 이 크레이트의 다른 검사들처럼 `tests/` 에 통합 테스트로 두면 **실행 채널이
    //! 사라진다** — 통합 테스트는 컴파일만 자동으로 검사되고 실행은 수동이다
    //! (`docs/dev-guide/ci-gates.md`). 그런데 이 가드의 본체는 **런타임 열거**라,
    //! 컴파일만 되고 실행되지 않으면 아무것도 보지 않는다. 어느 한쪽에만 변종을
    //! 더해도 코드는 멀쩡히 컴파일된다 — 그게 정확히 이 가드가 잡으려는 형태다.
    //! lib 유닛 테스트로 두면 `--lib --bins` 를 도는 자동 잡에서 함께 실행된다.
    //!
    //! 단, **채널이 있다는 것과 그 채널이 지금 초록이라는 것은 별개다.** 그 잡이
    //! 다른 이유로 실패 중이면 이 가드도 거기서 결과를 내지 못한다 — 자동 채널을
    //! 근거로 로컬 확인을 건너뛰기 전에 그 잡이 최근에 통과했는지부터 봐라.
    //!
    //! **다음 사람에게**: "관례에 맞춘다" 며 `tests/` 로 되돌리지 마라. 되돌리는
    //! 순간 이 가드는 조용히 자동 채널을 잃는다.
    //!
    //! ## 왜 텍스트 파싱이 아니라 런타임 열거인가
    //!
    //! 선례 `crates/tasty-doc-guards/tests/permission_free_methods_docs_parity.rs` ·
    //! `crates/tasty-doc-guards/tests/contributes_gate_docs_parity.rs` 와 같은 형태다. 소스를 파싱하면
    //! 주석·`#[cfg]`·줄바꿈에 흔들리고 파서 자체가 틀릴 수 있다. 두 `ALL` 배열을
    //! 컴파일된 값으로 열거하면 그 실패 모드가 없다. 대조는 **`Debug` 이름**으로
    //! 한다 — 두 타입은 서로 다른 크레이트의 별개 타입이라 값끼리 비교할 수 없고,
    //! 비교해야 하는 것도 "같은 변종 집합인가" 이기 때문이다. 텍스트를 읽지 않으므로
    //! 경로 구분자·CRLF 같은 플랫폼 차이에도 걸리지 않는다.
    //!
    //! ## 이 가드가 보지 않는 것
    //!
    //! 변종마다 **어떤 색을 칠하는가**는 보지 않는다. 정본 쪽 매핑은 본체
    //! `src/adapters/ui/toast.rs` 에, 미러 쪽은 [`accent_color`] 에 있고 둘 다
    //! `Theme` 접근자를 쓰지만, 같은 접근자를 고르는지는 기계가 판정할 값이 아니라
    //! 사람이 본다. 집합이 같다는 것만 여기서 고정한다.
    //!
    //! `ALL` 자체가 손으로 유지되는 배열이라, 변종을 더하고 `ALL` 을 안 고치면 그쪽은
    //! 이 가드를 통과한다 — 두 `ALL` 의 doc 주석이 그 전제를 적어 둔다.

    use std::collections::BTreeSet;

    use tasty_model::toast_kind::ToastKind as CanonicalKind;

    use super::ToastKind as MirrorKind;

    fn names<T: std::fmt::Debug>(all: &[T]) -> BTreeSet<String> {
        all.iter().map(|k| format!("{k:?}")).collect()
    }

    #[test]
    fn the_gallery_mirror_shows_exactly_the_kinds_the_app_can_produce() {
        let canonical = names(CanonicalKind::ALL);
        let mirror = names(MirrorKind::ALL);

        let only_mirror: Vec<_> = mirror.difference(&canonical).cloned().collect();
        assert!(
            only_mirror.is_empty(),
            "갤러리에만 있는 toast kind: {only_mirror:?} — 본체가 만들 수 없는 것을 \
             카탈로그가 전시한다. 정본(crates/tasty-model/src/toast_kind.rs)에 더하거나 \
             미러에서 빼라."
        );

        let only_canonical: Vec<_> = canonical.difference(&mirror).cloned().collect();
        assert!(
            only_canonical.is_empty(),
            "본체에만 있는 toast kind: {only_canonical:?} — 갤러리가 본체의 일부를 \
             빠뜨렸다(갤러리 완전성: docs/design/policies/gallery-completeness.md). \
             crates/tasty-gallery/src/catalog/toast_card.rs 의 미러에 더하라."
        );
    }

    #[test]
    fn both_all_arrays_actually_enumerate_something() {
        // 스캔 하한 — 어느 한쪽 `ALL` 이 빈 배열이 되면 위 대조가 공허하게 통과한다.
        assert!(!CanonicalKind::ALL.is_empty());
        assert_eq!(CanonicalKind::ALL.len(), MirrorKind::ALL.len());
    }
}
