# Typed Length System

## 원칙

**내부 소스에서 길이 값을 단순 `f32`로 다루는 것은 금지한다.**

모든 길이 값은 `PhysicalPx` 또는 `LogicalPx` 타입을 사용해야 한다.
이 규칙은 물리/논리 픽셀 혼동으로 인한 DPI 관련 버그를 컴파일 타임에 차단한다.

## 타입 정의

| 타입 | 의미 | 사용처 |
|------|------|--------|
| `PhysicalPx` | 실제 디바이스 픽셀 | GPU 렌더링, wgpu, winit 마우스 좌표, `Rect` |
| `LogicalPx` | DPI 독립 논리 픽셀 | egui UI, Theme 상수, 사이드바 너비 |

두 타입은 `#[repr(transparent)]`로 선언되어 런타임 오버헤드가 없다 (제로 코스트 추상화).

## 변환 규칙

두 타입 간 직접 대입은 **컴파일 에러**. 반드시 변환 함수를 거쳐야 한다:

```rust
// 논리 → 물리
let physical: PhysicalPx = logical.to_physical(scale_factor);

// 물리 → 논리
let logical: LogicalPx = physical.to_logical(scale_factor);

// 외부 API (egui, wgpu 등)에 f32로 전달할 때
egui::FontId::proportional(th.font_size_body.value());
```

## 적용 범위

### PhysicalPx로 선언하는 값
- `Rect` 필드 (x, y, width, height)
- `PANE_BORDER_WIDTH`, `SURFACE_BORDER_WIDTH`
- `AppState.tab_bar_height`
- winit에서 받는 마우스 좌표
- GPU 뷰포트/시저 좌표

### LogicalPx로 선언하는 값
- Theme 크기 필드 (폰트 크기, 간격, 보더, 코너 반경, 아이템 높이 등)
- `AppState.sidebar_width`
- `AppearanceSettings.sidebar_width`
- egui에 전달하는 모든 좌표/크기

### f32로 유지하는 값
- 비율 (ratio, opacity, scale_factor 등)
- 색상 채널 값
- 외부 라이브러리 API 호출 시 `.value()`로 추출한 값

## 새 코드 작성 시

1. 길이를 나타내는 `f32` 변수/필드를 만들지 않는다.
2. `PhysicalPx`인지 `LogicalPx`인지 판단하여 적절한 타입을 사용한다.
3. 판단 기준: 해당 값이 DPI scale_factor에 의해 달라지면 Physical, 아니면 Logical.
4. 외부 API 경계에서만 `.value()`로 f32를 추출한다.
