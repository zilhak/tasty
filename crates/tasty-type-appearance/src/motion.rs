//! 시간(모션 지속시간·지연)을 단위와 함께 나르는 타입.
//!
//! `PhysicalPx`/`LogicalPx` 가 두 좌표계를 섞는 것을 컴파일러가 막게 하듯,
//! [`Millis`] 는 **밀리초와 초가 섞이는 것**을 막는다. 시간 값은 이 레포에서 셋으로
//! 흩어져 있다 — 밀리초 `f32`(디자인 토큰), 초 `f32`/`f64`(egui `animate_bool_with_time`),
//! [`std::time::Duration`](std::time::Duration)(타이머). 셋이 전부 맨 실수이거나 이름에만
//! 단위가 실려 있어서, 섞이면 컴파일은 통과하고 **값만 1000 배 틀린다.**
//!
//! 그게 가정이 아니라 실제로 일어났다: 모션 값을 찾으려고 만든 스캐너가
//! `SPIN_PERIOD: f64 = 0.9`(초, 900ms)를 **0.9ms 로 환산했다.** 이름에만 있는 단위는
//! 도구도 못 읽는다.
//!
//! # 범위 — 경계 하나만
//!
//! `Millis` 는 **`Theme` 경계를 건너는 값**에만 쓴다. egui·winit·`Duration` 이 맨
//! 실수를 받는 가장자리는 그대로 두고 **여기서 변환해 넘긴다.** 전면 도입이 아니다.
//!
//! # 벗기는 통로에 단위를 박아 둔 이유
//!
//! `value()` 같은 무단위 접근자를 두지 않는다. 길이 쪽은 egui·wgpu 가 맨 `f32` 를
//! 요구하는 자리가 수백 개라 `.value()` 로 벗기는 것이 일상이고, 그래서 벗긴 뒤의
//! 수동 산술을 잡는 별도 가드(`src/dpi_conversion_guard.rs`)가 필요했다. 시간은
//! 경계가 좁다 — 필요한 변환이 초와 `Duration` 둘뿐이라 [`Self::to_secs_f32`] ·
//! [`Self::to_duration`] 이 그것을 덮는다. 그래도 원시 밀리초가 필요하면
//! [`Self::to_millis_f32`] 로 나가는데, **이름이 단위를 지고 있어** 초로 오인될 수
//! 없고 텍스트로 셀 수 있다. 즉 시간 축에는 `dpi_conversion_guard` 같은 두 번째
//! 가드가 필요 없다 — 흔한 오용 형태(`ms / 1000.0`)는 타입이 곧장 컴파일 에러로
//! 막고, 남는 탈출구는 하나이며 이름이 붙어 있다.

/// 밀리초 단위 시간.
///
/// 디자인 토큰의 `duration` 이 이 단위다(DTCG `120ms` → `Millis(120.0)`).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
#[repr(transparent)]
pub struct Millis(pub f32);

impl Millis {
    /// 초 단위 `f32` — egui 의 애니메이션 API(`animate_bool_with_time` 등)가 받는 형태.
    #[inline]
    pub fn to_secs_f32(self) -> f32 {
        self.0 / 1000.0
    }

    /// 초 단위 `f64` — `f64` 시계로 위상을 계산하는 자리용.
    #[inline]
    pub fn to_secs_f64(self) -> f64 {
        f64::from(self.0) / 1000.0
    }

    /// [`std::time::Duration`] — 타이머·백데이트 등 표준 라이브러리 경계용.
    ///
    /// 음수는 `Duration` 이 표현할 수 없으므로 0 으로 자른다(모션 지속시간에 음수가
    /// 들어오는 것은 토큰 오류이며, 여기서 패닉으로 바꾸면 렌더 경로가 죽는다).
    ///
    /// **나노초로 반올림해서 만든다.** `from_secs_f32(ms / 1000.0)` 는 이진 실수 오차로
    /// 900ms 를 899ms 로 만든다(실측 — 이 파일의 테스트가 그렇게 실패했다).
    /// `Duration` 은 자르지 반올림하지 않으므로, 자르기 전에 우리가 반올림한다.
    #[inline]
    pub fn to_duration(self) -> std::time::Duration {
        let nanos = (f64::from(self.0.max(0.0)) * 1_000_000.0).round();
        std::time::Duration::from_nanos(nanos as u64)
    }

    /// 원시 밀리초. **이름이 단위를 진다** — 초로 오인될 수 없고, 이 탈출구를 쓰는
    /// 자리는 텍스트로 셀 수 있다(모듈 doc 참조).
    #[inline]
    pub const fn to_millis_f32(self) -> f32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1000 배 착오가 이 타입이 존재하는 이유다 — 세 변환이 같은 값을 가리키는지 본다.
    #[test]
    fn the_three_conversions_agree() {
        let m = Millis(900.0);
        assert_eq!(m.to_millis_f32(), 900.0);
        assert!((m.to_secs_f32() - 0.9).abs() < f32::EPSILON);
        assert!((m.to_secs_f64() - 0.9).abs() < 1e-9);
        assert_eq!(m.to_duration().as_millis(), 900);
    }

    #[test]
    fn a_negative_duration_clamps_instead_of_panicking() {
        assert_eq!(Millis(-1.0).to_duration(), std::time::Duration::ZERO);
    }

    /// 0ms 는 정당한 값이다 — reduced_motion 과 터미널 콘텐츠 정책이 그것을 쓴다.
    #[test]
    fn zero_is_a_real_value() {
        assert_eq!(Millis(0.0).to_secs_f32(), 0.0);
        assert_eq!(Millis(0.0).to_duration(), std::time::Duration::ZERO);
    }
}
