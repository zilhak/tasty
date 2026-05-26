//! GPU 색 newtype 정의.
//!
//! private 필드 + 명시적 생성 메서드를 통해 array literal 로부터의 우연한 색
//! 생성을 컴파일 단계에서 차단한다.

use bytemuck::{Pod, Zeroable};

/// GPU 셰이더 입력용 straight RGBA. wgpu vertex buffer 에 그대로 들어가는 표현.
///
/// 생성 경로:
/// - 정상: `HexColor::to_gpu_rgba()` (tasty-core 에서 제공)
/// - 외부 입력: [`GpuRgba::dangerously_force_from_array`]
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct GpuRgba([f32; 4]);

/// GPU 셰이더 입력용 RGB (ANSI 팔레트 등).
///
/// 생성 경로:
/// - 정상: `HexColor::to_gpu_rgb()` (tasty-core 에서 제공)
/// - 외부 입력: [`GpuRgb::dangerously_force_from_array`]
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct GpuRgb([f32; 3]);

impl GpuRgba {
    /// 보관 중인 raw `[f32; 4]` 추출. wgpu vertex layout, JSON 직렬화 등에 사용.
    /// 색을 **새로 만드는** 게 아니라 **꺼내는** 용도.
    #[inline]
    pub const fn as_array(self) -> [f32; 4] {
        self.0
    }

    #[inline]
    pub const fn r(self) -> f32 {
        self.0[0]
    }
    #[inline]
    pub const fn g(self) -> f32 {
        self.0[1]
    }
    #[inline]
    pub const fn b(self) -> f32 {
        self.0[2]
    }
    #[inline]
    pub const fn a(self) -> f32 {
        self.0[3]
    }

    /// ⚠ **외부 입력 전용**.
    ///
    /// 다음 경우에만 사용:
    /// - termwiz `SrgbaTuple` 등 외부 라이브러리가 만든 색 데이터를 GPU 표현으로 받기
    /// - 사용자 픽커/브러시 픽셀 값
    /// - 디스크에서 복원된 scrollback 색
    /// - 테스트 더미
    ///
    /// **theme 색을 만들거나 색을 "디자인" 하는 용도로는 절대 사용 금지.**
    /// 그건 반드시 `~/.tasty/themes/*.toml` 또는 tasty-core 의 const 를 통해야 한다.
    ///
    /// 호출 시 반드시 위 사유 중 하나를 주석으로 명시할 것.
    #[inline]
    pub const fn dangerously_force_from_array(arr: [f32; 4]) -> Self {
        Self(arr)
    }
}

impl GpuRgb {
    /// 보관 중인 raw `[f32; 3]` 추출.
    #[inline]
    pub const fn as_array(self) -> [f32; 3] {
        self.0
    }

    #[inline]
    pub const fn r(self) -> f32 {
        self.0[0]
    }
    #[inline]
    pub const fn g(self) -> f32 {
        self.0[1]
    }
    #[inline]
    pub const fn b(self) -> f32 {
        self.0[2]
    }

    /// ⚠ **외부 입력 전용**. 사용 가이드는 [`GpuRgba::dangerously_force_from_array`] 참고.
    #[inline]
    pub const fn dangerously_force_from_array(arr: [f32; 3]) -> Self {
        Self(arr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_round_trip() {
        let c = GpuRgba::dangerously_force_from_array([0.1, 0.2, 0.3, 0.4]);
        assert_eq!(c.as_array(), [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(c.r(), 0.1);
        assert_eq!(c.g(), 0.2);
        assert_eq!(c.b(), 0.3);
        assert_eq!(c.a(), 0.4);
    }

    #[test]
    fn rgb_round_trip() {
        let c = GpuRgb::dangerously_force_from_array([0.5, 0.6, 0.7]);
        assert_eq!(c.as_array(), [0.5, 0.6, 0.7]);
        assert_eq!(c.r(), 0.5);
        assert_eq!(c.g(), 0.6);
        assert_eq!(c.b(), 0.7);
    }

    #[test]
    fn pod_byte_size_matches_array() {
        // repr(transparent) + Pod 보장: bytemuck::bytes_of 로 raw [f32; 4] 와 동일 표현.
        let c = GpuRgba::dangerously_force_from_array([1.0, 0.5, 0.0, 1.0]);
        let bytes = bytemuck::bytes_of(&c);
        assert_eq!(bytes.len(), 16); // 4 * f32
    }
}
