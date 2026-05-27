//! Appearance schema/primitives for Tasty.
//!
//! **Type-layer crate** — `tasty-type-*` 그룹 내부 (`tasty-type-geometry` 등) 에만
//! 의존할 수 있다. 도메인/IO crate (`tasty-core`, `tasty-themes`, `tasty-settings`,
//! 본 바이너리) 의존은 금지. type-\* 그룹 내 순환도 금지.
//!
//! 제공 타입:
//! - [`color::HexColor`] — `#RRGGBB(AA)` 직렬화 색상 래퍼 (straight RGBA u8)
//! - [`color::GpuRgba`] — wgpu vertex buffer 에 직접 들어가는 straight RGBA `[f32; 4]` newtype
//! - [`color::GpuRgb`]  — 3채널 변형 (ANSI 팔레트용)
//! - [`color::SurfaceColors`] — surface 종류별 focused/unfocused 묶음 schema
//! - [`theme::ThemeColors`] / [`theme::PartialColors`] — 47 필드 색상 풀세트 schema + Option-wrap partial
//! - [`theme::ThemeSizing`] / [`theme::SIZING`] — UI 폭/높이/간격 공통 const
//! - [`theme::Theme`] — 평평한 인스턴스 (색 + sizing + 도출 overlay + is_light)
//!
//! GPU newtype 들은 `#[repr(transparent)]` + `bytemuck::Pod` 라 셰이더 layout /
//! GPU buffer 의 byte 표현이 raw `[f32; N]` 과 정확히 동일. 런타임 오버헤드 0.
//!
//! ## 색 생성 정책
//!
//! - **정상 경로**: theme 색 → `HexColor` → `to_gpu_rgba()`. theme 색은
//!   `~/.tasty/themes/*.toml` 또는 `tasty-themes` 의 const 에서만 정의.
//! - **외부 입력 전용**: [`color::GpuRgba::dangerously_force_from_array`].
//!   - termwiz ANSI true-color escape
//!   - 사용자 픽커/브러시 픽셀 값
//!   - 디스크에서 복원된 scrollback 색 데이터
//!   - 테스트 더미
//!
//! 색을 "디자인" 하거나 "새로 만들기" 위해 `dangerously_force_*` 를 사용하면 안 된다.
//! 항상 theme 파일 또는 tasty-themes 의 fallback const 를 통해야 한다.
//!
//! 상세는 `docs/dev-guide/color-policy.md` 참고.

pub mod color;
pub mod theme;
