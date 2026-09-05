//! Type-safe pixel length types to prevent physical/logical pixel confusion at compile time.
//!
//! - `PhysicalPx`: actual device pixels (used by GPU, wgpu, winit mouse coordinates)
//! - `LogicalPx`: DPI-independent pixels (used by egui, Theme constants)
//!
//! Direct assignment between the two is impossible. Conversion requires an explicit
//! scale factor, making DPI-related bugs a compile error instead of a runtime surprise.

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

    pub fn floor(self) -> Self {
        Self(self.0.floor())
    }

    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// `const` 문맥용 덧셈. `Add` impl 과 결과가 같지만 트레이트 impl 은 `const` 가
    /// 아니라 상수 초기화식에서 부를 수 없다(E0015). 그 자리에서 `Self(a.0 + b.0)` 로
    /// 필드를 벗기면 타입이 사라지므로, 벗기지 않고 쓰는 통로를 둔다.
    pub const fn plus(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }

    /// `const` 문맥용 뺄셈. 사유는 [`Self::plus`] 와 같다.
    pub const fn minus(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }

    /// `const` 문맥용 스칼라 배. 사유는 [`Self::plus`] 와 같다.
    ///
    /// `Mul<f32>` 와 달리 계수가 좌변인 형태(`4.0 * LEN`)는 어차피 지원하지 않으므로,
    /// 호출 형태가 `LEN.scaled(4.0)` 하나로 고정된다.
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

    pub fn floor(self) -> Self {
        Self(self.0.floor())
    }

    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    /// `const` 문맥용 덧셈. `Add` impl 과 결과가 같지만 트레이트 impl 은 `const` 가
    /// 아니라 상수 초기화식에서 부를 수 없다(E0015). 그 자리에서 `Self(a.0 + b.0)` 로
    /// 필드를 벗기면 타입이 사라지므로, 벗기지 않고 쓰는 통로를 둔다.
    pub const fn plus(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }

    /// `const` 문맥용 뺄셈. 사유는 [`Self::plus`] 와 같다.
    pub const fn minus(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }

    /// `const` 문맥용 스칼라 배. 사유는 [`Self::plus`] 와 같다.
    ///
    /// `Mul<f32>` 와 달리 계수가 좌변인 형태(`4.0 * LEN`)는 어차피 지원하지 않으므로,
    /// 호출 형태가 `LEN.scaled(4.0)` 하나로 고정된다.
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

    // 이 셋이 `const` 문맥에서 평가된다는 것 자체가 검증 대상이다. 값 비교만 하면
    // `const` 를 떼도 초록이라, 상수 초기화식으로 써서 컴파일 자체를 증거로 삼는다.
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
}
