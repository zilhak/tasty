/// Type-safe pixel length types to prevent physical/logical pixel confusion at compile time.
///
/// - `PhysicalPx`: actual device pixels (used by GPU, wgpu, winit mouse coordinates)
/// - `LogicalPx`: DPI-independent pixels (used by egui, Theme constants)
///
/// Direct assignment between the two is impossible. Conversion requires an explicit
/// scale factor, making DPI-related bugs a compile error instead of a runtime surprise.

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
}

// ── Arithmetic: PhysicalPx ──

impl std::ops::Add for PhysicalPx {
    type Output = Self;
    fn add(self, rhs: Self) -> Self { Self(self.0 + rhs.0) }
}

impl std::ops::Sub for PhysicalPx {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self { Self(self.0 - rhs.0) }
}

impl std::ops::Mul<f32> for PhysicalPx {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self { Self(self.0 * rhs) }
}

impl std::ops::Div<f32> for PhysicalPx {
    type Output = Self;
    fn div(self, rhs: f32) -> Self { Self(self.0 / rhs) }
}

impl std::ops::Neg for PhysicalPx {
    type Output = Self;
    fn neg(self) -> Self { Self(-self.0) }
}

impl std::ops::AddAssign for PhysicalPx {
    fn add_assign(&mut self, rhs: Self) { self.0 += rhs.0; }
}

impl std::ops::SubAssign for PhysicalPx {
    fn sub_assign(&mut self, rhs: Self) { self.0 -= rhs.0; }
}

// ── Arithmetic: LogicalPx ──

impl std::ops::Add for LogicalPx {
    type Output = Self;
    fn add(self, rhs: Self) -> Self { Self(self.0 + rhs.0) }
}

impl std::ops::Sub for LogicalPx {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self { Self(self.0 - rhs.0) }
}

impl std::ops::Mul<f32> for LogicalPx {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self { Self(self.0 * rhs) }
}

impl std::ops::Div<f32> for LogicalPx {
    type Output = Self;
    fn div(self, rhs: f32) -> Self { Self(self.0 / rhs) }
}

impl std::ops::Neg for LogicalPx {
    type Output = Self;
    fn neg(self) -> Self { Self(-self.0) }
}

impl std::ops::AddAssign for LogicalPx {
    fn add_assign(&mut self, rhs: Self) { self.0 += rhs.0; }
}

impl std::ops::SubAssign for LogicalPx {
    fn sub_assign(&mut self, rhs: Self) { self.0 -= rhs.0; }
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
