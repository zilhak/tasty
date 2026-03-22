# renderer.rs 분할 계획

`src/renderer.rs` (700줄)를 `src/renderer/` 디렉토리로 분할한다.

## 현재 구조 분석

| 줄 범위 | 내용 |
|---------|------|
| 1-6 | use 선언 (bytemuck, termwiz, wgpu, crate::font, crate::model) |
| 8-35 | GPU 데이터 구조체: `Uniforms`, `BgInstance`, `GlyphInstance` (#[repr(C)] + Pod/Zeroable) |
| 37-133 | WGSL 셰이더 문자열 상수: `BG_SHADER` (줄 39-80), `GLYPH_SHADER` (줄 82-133) |
| 135-176 | 색상 팔레트: `DEFAULT_FG/BG`, `ANSI_COLORS` (16색), `palette_index_to_rgb`, `color_attr_to_rgba` |
| 196-215 | `CellRenderer` struct 정의 |
| 217-498 | `CellRenderer::new` (282줄 — 바인드 그룹, 파이프라인 생성) |
| 500-512 | `CellRenderer::resize` |
| 514-590 | `CellRenderer::prepare` (인스턴스 데이터 빌드) |
| 592-609 | `CellRenderer::render` (2-pass 렌더) |
| 611-629 | `CellRenderer::grid_size`, `grid_size_for_rect` |
| 631-689 | `CellRenderer::prepare_viewport`, `render_scissored` |
| 691-700 | `CellRenderer::cell_width`, `cell_height` |

## 분할 후 구조

```
src/renderer/
├── mod.rs              — CellRenderer struct + impl (핵심 로직)
├── shaders.rs          — WGSL 셰이더 상수
├── palette.rs          — ANSI 색상 팔레트, 색상 변환 함수
└── types.rs            — GPU 데이터 구조체 (Uniforms, BgInstance, GlyphInstance)
```

## 각 파일 상세

### types.rs (~40줄)

GPU에 전송되는 #[repr(C)] 데이터 구조체.

**포함 내용:**
- `Uniforms` struct (줄 12-17) — cell_size, grid_offset, viewport_size, padding
- `BgInstance` struct (줄 20-24) — pos, bg_color
- `GlyphInstance` struct (줄 27-35) — pos, uv_offset, uv_size, fg_color, glyph_offset, glyph_size

모든 구조체에 `#[repr(C)]`, `Pod`, `Zeroable` derive가 필요하다.

**의존:** bytemuck만

### shaders.rs (~100줄)

WGSL 셰이더 소스 문자열.

**포함 내용:**
- `BG_SHADER: &str` (줄 39-80) — 배경 컬러 쿼드 셰이더
- `GLYPH_SHADER: &str` (줄 82-133) — 글리프 텍스처 알파 블렌딩 셰이더

**의존:** 없음 (순수 문자열 상수)

### palette.rs (~60줄)

색상 팔레트와 ColorAttribute → [f32; 4] 변환.

**포함 내용:**
- `DEFAULT_FG`, `DEFAULT_BG` 상수 (줄 137-138)
- `ANSI_COLORS` 16색 배열 (줄 141-158)
- `palette_index_to_rgb` 함수 (줄 160-176) — 256색 팔레트 인덱스 → RGB
- `color_attr_to_rgba` 함수 (줄 178-192) — termwiz ColorAttribute → RGBA

**의존:** termwiz::color::ColorAttribute

### mod.rs (~500줄)

CellRenderer의 핵심 로직.

**포함 내용:**
- `CellRenderer` struct (줄 196-215)
- `CellRenderer::new` (줄 217-498)
- `CellRenderer::resize` (줄 500-512)
- `CellRenderer::prepare` (줄 514-590)
- `CellRenderer::render` (줄 592-609)
- `CellRenderer::grid_size`, `grid_size_for_rect` (줄 611-629)
- `CellRenderer::prepare_viewport` (줄 631-655)
- `CellRenderer::render_scissored` (줄 657-689)
- `CellRenderer::cell_width`, `cell_height` (줄 691-700)

```rust
mod types;
mod shaders;
mod palette;

use types::{Uniforms, BgInstance, GlyphInstance};
use shaders::{BG_SHADER, GLYPH_SHADER};
use palette::{color_attr_to_rgba, DEFAULT_FG, DEFAULT_BG};

pub use types::*;  // 필요한 경우 GPU 타입 노출
```

**의존:**
- wgpu, bytemuck
- `crate::font::{FontConfig, GlyphAtlas, GlyphKey}`
- `crate::model::Rect`
- termwiz::surface::Surface, termwiz::color::ColorAttribute

## 분할 이유

1. **셰이더 분리**: WGSL 상수가 100줄을 차지하며 코드 리뷰 시 노이즈가 된다. 별도 파일로 분리하면 셰이더 변경과 렌더 로직 변경의 diff가 깔끔하게 분리된다.

2. **팔레트 분리**: 색상 매핑 로직은 렌더러와 독립적으로 테스트/수정 가능하다. 향후 테마 시스템 확장 시 palette.rs만 수정하면 된다.

3. **GPU 타입 분리**: `#[repr(C)]` 구조체는 GPU 메모리 레이아웃을 정의하며, 셰이더와 1:1 대응한다. types.rs와 shaders.rs가 쌍으로 관리되면 레이아웃 불일치 버그를 예방할 수 있다.
