#![forbid(unsafe_code)]

//! Geometry primitives for Tasty.
//!
//! **Leaf crate** — 다른 어떤 `tasty-*` crate 도 의존하지 않는다. 도메인 모델
//! (Theme / Workspace 같은 도메인 의미 있는 타입) 은 절대 여기 들어오지 않아야
//! 순환 위험이 0 으로 유지된다.
//!
//! 제공 타입:
//! - [`length::LogicalPx`] — DPI-independent pixels (egui, Theme 상수)
//! - [`length::PhysicalPx`] — actual device pixels (GPU/wgpu/winit 마우스 좌표)
//! - [`rect::PhysicalRect`] / [`rect::LogicalRect`] — 픽셀 좌표 사각형 (짝 타입)
//! - [`rect::DividerInfo`] — 분할 보더 정보 (geometry + direction 묶음)
//! - [`direction::SplitDirection`] / [`direction::FocusDirection`] — 방향 enum
//!
//! length 두 타입은 컴파일 단계에서 서로 직접 대입 불가 — `to_logical(sf)` /
//! `to_physical(sf)` 변환을 통해서만 변환 가능. DPI 관련 버그를 런타임이 아닌
//! 컴파일 에러로 만든다.

pub mod direction;
pub mod length;
pub mod rect;
