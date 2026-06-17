# 타입 있는 길이 (Typed length)

tasty 내부 소스는 길이 값을 **`f32` 그대로 다루지 않는다.** DPI scale factor 가 개입하는 코드에서 *물리 픽셀* 과 *논리 픽셀* 을 헷갈리면 런타임에야 드러나는 위치/크기 버그가 난다. 그래서 길이는 두 newtype 으로 나뉘고, **둘을 직접 섞는 코드는 컴파일 에러**가 된다 — DPI 혼동을 타입 시스템이 막는다.

- 정의: `crates/tasty-type-geometry/src/length.rs`
- 강제 정책(필수): [`../../CLAUDE.md`](../../CLAUDE.md) "길이 타입" 섹션 — 새 길이 값은 반드시 둘 중 하나.

## 두 타입

| 타입 | 의미 | 쓰는 곳 |
|------|------|---------|
| `PhysicalPx(pub f32)` | 실제 디바이스 픽셀 (scale factor 적용 후) | GPU/wgpu, winit 마우스 좌표, `Rect` 필드, GPU 뷰포트·시저 |
| `LogicalPx(pub f32)` | DPI 독립 논리 픽셀 | egui UI, `Theme` 상수, 사이드바 너비, 모든 egui 좌표/크기 |

둘 다 `#[repr(transparent)]` 이라 런타임 오버헤드가 없다 (제로 코스트). `Add`/`Sub`/`Mul<f32>`/`Div<f32>`/`Neg`/`*Assign` 과 `max`/`min`/`floor`/`abs` 가 **같은 타입끼리만** 정의돼 있어, `PhysicalPx + LogicalPx` 같은 식은 타입 에러다.

## 변환 — scale factor 를 명시적으로 통과

두 타입 간 직접 대입은 불가능하다. 반드시 변환 함수를 거치고, 그때 scale factor 를 넘긴다:

```rust
let physical: PhysicalPx = logical.to_physical(scale_factor); // 논리 → 물리 (× sf)
let logical:  LogicalPx  = physical.to_logical(scale_factor); // 물리 → 논리 (÷ sf)
```

변환에 scale factor 가 *강제 인자* 라는 점이 핵심이다 — "어느 좌표계인지" 를 매번 의식하게 만든다.

## 외부 API 경계에서만 `.value()`

egui·wgpu 등 외부 라이브러리는 `f32` 를 받는다. 그 **경계에서만** `.value()` 로 raw `f32` 를 꺼낸다:

```rust
egui::FontId::proportional(th.font_size_body.value());
```

내부 로직 중간에서 `.value()` 로 빠져나와 `f32` 산술을 하는 것은 안티패턴 — 타입 보호를 스스로 버리는 셈이다.

## `f32` 로 남는 값

길이가 아닌 값은 그대로 `f32` 다 — 비율(ratio/opacity/scale_factor), 색 채널, 외부 API 로 넘길 직전 추출값.

## 새 코드 작성 시

1. 길이를 나타내는 맨 `f32` 필드/변수를 만들지 않는다.
2. **scale factor 에 따라 값이 달라지면 `PhysicalPx`, 아니면 `LogicalPx`.**
3. 외부 API 경계에서만 `.value()` 로 `f32` 추출.

## 관련

- [`../design/systems/theme.md`](../design/systems/theme.md) — Theme 상수는 모두 `LogicalPx`
- 색은 길이와 같은 newtype 정책을 따른다 (`GpuRgba` 등) — theme.md "색 생성 경로 단일화"
