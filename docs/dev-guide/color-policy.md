# 색 생성 정책

Tasty 의 색 데이터는 **단 두 출처**에서만 만들어진다. 그 외 경로는 컴파일 단계
(`tasty-type-color` newtype) + clippy lint (`clippy.toml::disallowed-methods`)
조합으로 차단된다.

## 원칙

```text
정상 출처:
1. ~/.tasty/themes/<id>.toml          ← 사용자/빌트인 테마 파일
2. tasty-core::theme 의 const         ← MOCHA_FALLBACK_COLORS, derive_overlays

외부 입력 (예외):
- termwiz ANSI true-color escape       (palette.rs)
- 사용자 색 picker                      (settings UI, image controls)
- 디스크 이미지/클립보드 픽셀           (image/view.rs, clipboard.rs)
- 이전 세션 scrollback 직렬화 데이터    (disk_scrollback.rs)
- 테스트 더미                           (모든 #[cfg(test)])
```

외부 입력은 명시적 `dangerously_force_from_array` 호출 + 사유 주석 필수.

## 컴파일 시 강제 — `tasty-type-color` newtype

```rust
// crates/tasty-type-color/src/color.rs
#[repr(transparent)]
pub struct GpuRgba([f32; 4]);  // private field

impl GpuRgba {
    pub const fn as_array(self) -> [f32; 4] { ... }  // 꺼내기 OK
    pub const fn dangerously_force_from_array(arr: [f32; 4]) -> Self { ... }  // 외부 입력 전용
}
```

GPU 버퍼 struct (`BgInstance.bg_color: GpuRgba`) 가 이 newtype 을 받으므로:

```rust
// ❌ 컴파일 에러 — array literal 로 GpuRgba 만들 수 없음
let bg: GpuRgba = [1.0, 0.0, 0.0, 1.0];

// ❌ 컴파일 에러 — private field 직접 접근 불가
let bg = GpuRgba([1.0, 0.0, 0.0, 1.0]);

// ✅ 정상 경로 — theme 에서 변환
let bg = theme().crust.to_gpu_rgba();

// ⚠ 외부 입력 — dangerously 명시 + 사유 주석
// 외부 입력 (termwiz true-color escape).
let bg = GpuRgba::dangerously_force_from_array([srgba.0, srgba.1, srgba.2, srgba.3]);
```

`#[repr(transparent)]` + `bytemuck::Pod` 덕에 byte layout 은 raw `[f32; 4]` 와
정확히 동일. wgpu vertex layout 무수정. 런타임 오버헤드 0.

## clippy 시 강제 — disallowed-methods

`clippy.toml` 의 `disallowed-methods` 가 색 생성 함수의 외부 호출 차단:

| 차단되는 함수 | 차단 이유 |
|--------------|----------|
| `HexColor::from_rgb` / `from_rgba` | theme 색은 TOML 또는 const 에서만 정의 |
| `egui::Color32::from_rgb` | theme().X 또는 .with_alpha(N).to_egui() 사용 |
| `egui::Color32::from_rgba_unmultiplied` | 외부 픽셀 데이터 변환은 #[allow] + 사유 주석 |
| `egui::Color32::from_rgba_premultiplied` | premultiplied 색은 theme().X.to_egui_premultiplied() |
| `egui::Color32::from_gray` | theme 의 회색 톤 사용 |

### 예외 위치

| 위치 | 사유 | 처리 |
|------|------|------|
| `tasty-core::color` | HexColor 생성/변환 헬퍼 본거지 | 모듈 상단 `#![allow]` |
| `tasty-core::theme` | 색상 const 정의 (MOCHA_FALLBACK_COLORS) | 모듈 상단 `#![allow]` |
| `tasty-themes::file/state` tests | 테스트 더미 색 | 테스트 모듈 `#[allow]` |
| settings 색 picker | 사용자 입력 → HexColor | 라인별 `#[allow]` + 주석 |
| 이미지 픽셀 추출 / 그림 브러시 | 외부 입력 | 라인별 `#[allow]` + 주석 |
| plugin protocol hex token (#RRGGBB) | plugin 명시 색 | 라인별 `#[allow]` + 주석 |

## 새 색을 도입할 때

1. **테마 색이면**:
   - 빌트인: `crates/tasty-themes/themes/*.toml` 또는 `tasty-core::theme::MOCHA_FALLBACK_COLORS` 추가
   - 사용자 테마: `~/.tasty/themes/*.toml`
   - UI 코드는 `theme().X.into()` (egui::Color32) 또는 `theme().X.to_gpu_rgba()` (GPU 버퍼)
2. **theme 색의 alpha 변형이면**: `theme().X.with_alpha(N).to_egui()` (UI) 또는 `theme().X.to_gpu_rgba()` 후 alpha 처리
3. **외부 입력이면**: `dangerously_force_from_array` 호출 + 사유 주석 + `#[allow]`

## 추가 가드

- **GPU 버퍼 struct 필드 타입은 항상 newtype** (`GpuRgba`/`GpuRgb`). raw `[f32; 4]` 금지.
- **렌더러 함수 시그니처** 색 인자도 모두 newtype.
- 새 GPU buffer/렌더 시그니처 추가 시 위 원칙 따를 것.

## 관련

- `docs/design/theme-system.md` — 테마 두 레이어 모델
- `crates/tasty-type-color/` — newtype 정의
- `clippy.toml` — disallowed-methods 정책
