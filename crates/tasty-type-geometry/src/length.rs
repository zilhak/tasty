//! PhysicalPx는 장치 픽셀, LogicalPx는 DPI와 독립된 논리 픽셀을 나타낸다.
//! 두 타입 사이의 직접 대입을 막고 변환할 때 배율을 명시하게 한다.
//! 잘못된 배율이나 원시 값으로 계산한 오류까지 방지하는 것은 아니다.

/// A length in physical (device) pixels.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
#[repr(transparent)]
pub struct PhysicalPx(pub f32);

/// A length in logical (DPI-independent) pixels.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
#[repr(transparent)]
pub struct LogicalPx(pub f32);

// ── Conversion ──

impl PhysicalPx {
    pub fn to_logical(self, scale_factor: f32) -> LogicalPx {
        LogicalPx(self.0 / scale_factor)
    }

    pub fn value(self) -> f32 {
        self.0
    }

    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    /// 같은 좌표계의 최소·최대 경계로 제한한다.
    /// f32::clamp와 같이 min > max이거나 경계가 NaN이면 패닉한다.
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    pub fn floor(self) -> Self {
        Self(self.0.floor())
    }

    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// Add 트레이트를 호출할 수 없는 const 문맥에서 사용할 덧셈.
    pub const fn plus(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }

    /// `const` 문맥용 뺄셈. 사유는 [`Self::plus`] 와 같다.
    pub const fn minus(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }

    /// const 문맥에서 사용할 배율 곱셈.
    pub const fn scaled(self, k: f32) -> Self {
        Self(self.0 * k)
    }
}

impl LogicalPx {
    pub fn to_physical(self, scale_factor: f32) -> PhysicalPx {
        PhysicalPx(self.0 * scale_factor)
    }

    pub fn value(self) -> f32 {
        self.0
    }

    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    /// 같은 좌표계의 최소·최대 경계로 제한한다.
    /// f32::clamp와 같이 min > max이거나 경계가 NaN이면 패닉한다.
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    pub fn floor(self) -> Self {
        Self(self.0.floor())
    }

    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// Add 트레이트를 호출할 수 없는 const 문맥에서 사용할 덧셈.
    pub const fn plus(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }

    /// `const` 문맥용 뺄셈. 사유는 [`Self::plus`] 와 같다.
    pub const fn minus(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }

    /// const 문맥에서 사용할 배율 곱셈.
    pub const fn scaled(self, k: f32) -> Self {
        Self(self.0 * k)
    }
}

// ── Arithmetic: PhysicalPx ──

impl std::ops::Add for PhysicalPx {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl std::ops::Sub for PhysicalPx {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl std::ops::Mul<f32> for PhysicalPx {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self(self.0 * rhs)
    }
}

impl std::ops::Div<f32> for PhysicalPx {
    type Output = Self;
    fn div(self, rhs: f32) -> Self {
        Self(self.0 / rhs)
    }
}

impl std::ops::Neg for PhysicalPx {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl std::ops::AddAssign for PhysicalPx {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl std::ops::SubAssign for PhysicalPx {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

// ── Arithmetic: LogicalPx ──

impl std::ops::Add for LogicalPx {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl std::ops::Sub for LogicalPx {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl std::ops::Mul<f32> for LogicalPx {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self(self.0 * rhs)
    }
}

impl std::ops::Div<f32> for LogicalPx {
    type Output = Self;
    fn div(self, rhs: f32) -> Self {
        Self(self.0 / rhs)
    }
}

impl std::ops::Neg for LogicalPx {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl std::ops::AddAssign for LogicalPx {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl std::ops::SubAssign for LogicalPx {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

// ── Display ──

impl std::fmt::Display for PhysicalPx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}px", self.0)
    }
}

impl std::fmt::Display for LogicalPx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}lp", self.0)
    }
}

// ── serde (for settings serialization) ──

impl serde::Serialize for LogicalPx {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for LogicalPx {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        f32::deserialize(deserializer).map(Self)
    }
}

impl serde::Serialize for PhysicalPx {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for PhysicalPx {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        f32::deserialize(deserializer).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // const 초기화식으로 사용해 상수 계산이 가능한지도 확인한다.
    const A: LogicalPx = LogicalPx(40.0);
    const B: LogicalPx = LogicalPx(12.0);
    const SUM: LogicalPx = A.plus(B);
    const DIFF: LogicalPx = A.minus(B);
    const QUAD: LogicalPx = B.scaled(4.0);
    const PHYS: PhysicalPx = PhysicalPx(9.0).plus(PhysicalPx(1.0)).scaled(2.0);

    #[test]
    fn const_arithmetic_matches_the_trait_impls() {
        assert_eq!(SUM, A + B);
        assert_eq!(DIFF, A - B);
        assert_eq!(QUAD, B * 4.0);
        assert_eq!(PHYS, (PhysicalPx(9.0) + PhysicalPx(1.0)) * 2.0);
    }

    #[test]
    fn clamp_cuts_at_both_ends_on_both_types() {
        let lo = LogicalPx(10.0);
        let hi = LogicalPx(20.0);
        assert_eq!(LogicalPx(5.0).clamp(lo, hi), lo);
        assert_eq!(LogicalPx(25.0).clamp(lo, hi), hi);
        assert_eq!(LogicalPx(15.0).clamp(lo, hi), LogicalPx(15.0));

        let plo = PhysicalPx(10.0);
        let phi = PhysicalPx(20.0);
        assert_eq!(PhysicalPx(5.0).clamp(plo, phi), plo);
        assert_eq!(PhysicalPx(25.0).clamp(plo, phi), phi);
        assert_eq!(PhysicalPx(15.0).clamp(plo, phi), PhysicalPx(15.0));
    }

    // f32::clamp의 잘못된 경계 처리와 같은지 확인한다.
    #[test]
    #[should_panic(expected = "min > max, or either was NaN")]
    fn clamp_panics_when_the_bounds_are_reversed() {
        // 반환값이 아닌 패닉을 검사한다.
        let _ = LogicalPx(1.0).clamp(LogicalPx(20.0), LogicalPx(10.0));
    }
}
