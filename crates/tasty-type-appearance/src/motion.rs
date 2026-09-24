//! Theme를 거치는 시간을 밀리초로 구분한다.
//! egui나 표준 라이브러리에 넘길 때는 단위를 명시한 변환 함수를 사용한다.
//! 원시 값으로 꺼낸 뒤의 단위 착오까지 타입이 방지하지는 못한다.

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

    /// 음수는 0으로 제한하고 나노초로 반올림해 Duration을 만든다.
    /// 밀리초를 먼저 f32 초로 나눌 때 생기는 오차를 피한다.
    #[inline]
    pub fn to_duration(self) -> std::time::Duration {
        let nanos = (f64::from(self.0.max(0.0)) * 1_000_000.0).round();
        std::time::Duration::from_nanos(nanos as u64)
    }

    /// 원시 밀리초 값. 초가 필요한 API에는 to_secs_f32/to_secs_f64를 사용한다.
    #[inline]
    pub const fn to_millis_f32(self) -> f32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 초와 Duration 변환이 같은 시간을 나타내는지 확인한다.
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

    /// 모션을 끄는 데 쓰는 0ms도 허용한다.
    #[test]
    fn zero_is_a_real_value() {
        assert_eq!(Millis(0.0).to_secs_f32(), 0.0);
        assert_eq!(Millis(0.0).to_duration(), std::time::Duration::ZERO);
    }
}
