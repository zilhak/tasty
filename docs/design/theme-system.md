# Tasty 테마 시스템

## 개념

테마는 **미리 정의해놓은 설정값 세트**일 뿐이다. 실제 동작은 사용자의 설정값(`AppearanceSettings`)에 의해 결정된다. 테마 프리셋을 선택하면 해당 설정값이 일괄 적용되고, 이후 사용자가 개별적으로 커스텀할 수 있다.

## 구조

### 두 가지 계층

1. **`Theme` (전역 UI 색상)** — egui 위젯, 사이드바, 탭 바 등 Tasty UI 전체의 색상·크기·간격을 정의. `static RwLock<Theme>`으로 런타임 교체 가능.
2. **`SurfaceColors` (설정값)** — 터미널/마크다운/익스플로러 각각의 focused/unfocused 배경색·글자색. `AppearanceSettings`에 저장되며 사용자가 직접 수정 가능.

### 테마 프리셋 (`ThemePreset`)

`Theme` + 3종 `SurfaceColors`를 묶은 것. `theme::presets()`로 목록을 얻는다.

| 프리셋 ID | 이름 | 계열 |
|-----------|------|------|
| `catppuccin-mocha` | Catppuccin Mocha | 다크 (기본) |
| `catppuccin-latte` | Catppuccin Latte | 라이트 |

프리셋 선택 시 동작:
1. `settings.appearance.theme`에 프리셋 ID 저장
2. `settings.appearance.terminal_colors/markdown_colors/explorer_colors`를 프리셋 기본값으로 초기화
3. 설정 저장 시 `set_theme(preset.theme)`으로 전역 UI 색상 적용

### 흐름

```
프리셋 선택 → settings에 색상값 복사 → 저장 시 set_theme() 호출
                                          ↓
                                    전역 Theme 교체 (UI 색상)
                                    settings 저장 (surface 색상)
                                          ↓
앱 시작 → settings 로드 → 프리셋 ID로 Theme 적용
```

## 규칙

### UI 색상은 항상 `theme()`에서 가져온다

UI 코드에서 `egui::Color32::from_rgb(...)` 등으로 색상을 하드코딩하지 않는다.

```rust
// BAD
let color = egui::Color32::from_rgb(80, 140, 255);

// GOOD
let th = crate::theme::theme();
let color = th.blue;
```

### Surface 배경색·글자색은 설정값에서 가져온다

터미널/마크다운/익스플로러의 배경색·글자색은 `theme()`이 아니라 `settings.appearance.terminal_colors` 등에서 가져온다. 사용자가 커스텀한 값이 반영되어야 하기 때문이다.

```rust
// BAD — 테마에서 직접 가져옴 (사용자 커스텀 무시)
let bg = th.terminal_bg;

// GOOD — 설정값에서 가져옴
let bg = settings.terminal_colors.focused_bg.to_float();
```

### egui premultiplied alpha 주의

```rust
// BAD — RGB가 알파보다 크면 거의 불투명으로 해석됨
Color32::from_rgba_premultiplied(255, 255, 255, 20)

// GOOD — egui가 내부적으로 곱셈 처리
Color32::from_rgba_unmultiplied(255, 255, 255, 20)
```

**규칙: 반투명 색상에는 항상 `from_rgba_unmultiplied`를 사용한다.** `from_rgba_premultiplied`는 이미 곱해진 값을 직접 다루거나 const 문맥에서만 사용.

## `Theme` 필드 레퍼런스

### 배경/표면

| 변수 | 용도 |
|------|------|
| `crust` | 가장 깊은 배경, 패널 뒤 |
| `mantle` | 사이드바 배경 |
| `base` | 메인 배경 |
| `surface0` | 카드, 호버 배경, 비활성 보더 |
| `surface1` | 선택 항목, 활성 보더 |
| `surface2` | 강조 배경 |

### 오버레이

| 변수 | 용도 |
|------|------|
| `overlay0` | 비활성 텍스트, 힌트 |
| `overlay1` | 보조 아이콘 |
| `overlay2` | 덜 중요한 텍스트 |

### 텍스트

| 변수 | 용도 |
|------|------|
| `text` | 주요 텍스트 |
| `subtext1` | 보조 텍스트 |
| `subtext0` | 비활성 텍스트, 설명 |

### 강조색

| 변수 | 용도 |
|------|------|
| `blue` | 주요 강조, 포커스, 링크, 알림 |
| `green` | 성공, 확인 |
| `red` | 에러, 위험 |
| `yellow` | 경고 |
| `peach` | 주의 |
| `mauve` | 보라 강조 |
| `teal` | 정보 |
| `sky` | 하늘색 강조 |
| `lavender` | 연보라 |
| `pink` | 분홍 |
| `flamingo` | 따뜻한 분홍 |
| `maroon` | 어두운 분홍 |
| `rosewater` | 가장 따뜻한 색 |

### 의미적 색상

| 변수 | 용도 |
|------|------|
| `hover_overlay` | 호버 시 배경 오버레이 (~8%) |
| `active_overlay` | 눌림 시 배경 오버레이 (~12%) |
| `separator` | 구분선 (~8%) |

### 타이포그래피 (UI 전용)

| 변수 | 값 | 용도 |
|------|-----|------|
| `font_size_caption` | 11px | 캡션, 배지, 상태 |
| `font_size_body` | 13px | 본문, 라벨, 버튼 |
| `font_size_heading` | 13px | 섹션 헤더 (세미볼드로 구분) |
| `font_size_max` | 14px | UI 텍스트 최대 크기 |

### UI 크기

| 변수 | 값 | 용도 |
|------|-----|------|
| `border_width` | 1px | 모든 보더 두께 |
| `corner_radius` | 4px | 기본 둥근 모서리 |
| `item_height_tree` | 22px | 트리 항목 |
| `item_height_interactive` | 28px | 버튼, 입력 필드, 메뉴 항목 |
| `item_height_tab` | 24px | 탭 |

### 간격 (4px 그리드)

| 변수 | 값 | 용도 |
|------|-----|------|
| `spacing_xs` | 4px | 타이트 내부 패딩 |
| `spacing_sm` | 8px | 기본 패딩, 관련 요소 간 |
| `spacing_md` | 12px | 카드 내부, 리스트 항목 |
| `spacing_lg` | 16px | 섹션 패딩 |
| `spacing_xl` | 24px | 주요 섹션 사이 |

## 새 프리셋 추가 시

1. `src/theme.rs`에 `Theme::NEW_NAME` const 추가
2. `presets()` 함수에 `ThemePreset` 항목 추가
3. 이 문서의 프리셋 표에 추가

## 새 `Theme` 필드 추가 시

1. `Theme` 구조체에 필드 추가
2. 모든 const 프리셋 (`DARK`, `LATTE` 등)에 값 설정
3. 이 문서의 레퍼런스에 추가
4. UI 코드에서 `th.새변수`로 사용
