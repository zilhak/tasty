# Tasty 테마 시스템

## 규칙: 모든 색상/크기는 테마에서 가져온다

UI 코드에서 `egui::Color32::from_rgb(...)` 등으로 색상을 하드코딩하지 않는다. 반드시 `theme::theme()`에서 가져온다.

```rust
// BAD
let color = egui::Color32::from_rgb(80, 140, 255);

// GOOD
let th = crate::theme::theme();
let color = th.blue;
```

## 테마 구조체 위치

`src/theme.rs` — `Theme` 구조체가 모든 디자인 변수를 담고 있다.

## 색상 팔레트 (Catppuccin Mocha)

### 배경/표면

| 변수 | Hex | 용도 |
|------|-----|------|
| `crust` | `#11111b` | 가장 깊은 배경, 패널 뒤 |
| `mantle` | `#181825` | 사이드바 배경 |
| `base` | `#1e1e2e` | 메인 배경 (터미널, 입력 필드) |
| `surface0` | `#313244` | 카드, 호버 배경, 비활성 보더 |
| `surface1` | `#45475a` | 선택 항목, 활성 보더 |
| `surface2` | `#585b70` | 강조 배경 |

### 오버레이

| 변수 | Hex | 용도 |
|------|-----|------|
| `overlay0` | `#6c7086` | 비활성 텍스트, 힌트 |
| `overlay1` | `#7f849c` | 보조 아이콘 |
| `overlay2` | `#9399b2` | 덜 중요한 텍스트 |

### 텍스트

| 변수 | Hex | 용도 |
|------|-----|------|
| `text` | `#cdd6f4` | 주요 텍스트 |
| `subtext1` | `#bac2de` | 보조 텍스트 |
| `subtext0` | `#a6adc8` | 비활성 텍스트, 설명 |

### 강조색

| 변수 | Hex | 용도 |
|------|-----|------|
| `blue` | `#89b4fa` | 주요 강조, 포커스, 링크, 알림 |
| `green` | `#a6e3a1` | 성공, 확인 |
| `red` | `#f38ba8` | 에러, 위험 |
| `yellow` | `#f9e2af` | 경고 |
| `peach` | `#fab387` | 주의 |
| `mauve` | `#cba6f7` | 보라 강조 |
| `teal` | `#94e2d5` | 정보 |
| `sky` | `#89dceb` | 하늘색 강조 |
| `lavender` | `#b4befe` | 연보라 |
| `pink` | `#f5c2e7` | 분홍 |
| `flamingo` | `#f2cdcd` | 따뜻한 분홍 |
| `maroon` | `#eba0ac` | 어두운 분홍 |
| `rosewater` | `#f5e0dc` | 가장 따뜻한 색 |

### 의미적 색상

| 변수 | 값 | 용도 |
|------|-----|------|
| `hover_overlay` | `rgba(255,255,255,0.08)` | 호버 시 배경 오버레이 |
| `active_overlay` | `rgba(255,255,255,0.12)` | 눌림 시 배경 오버레이 |
| `separator` | `rgba(255,255,255,0.08)` | 구분선 |

## 타이포그래피 (UI 전용, 터미널 폰트 아님)

| 변수 | 값 | 용도 |
|------|-----|------|
| `font_size_caption` | 11px | 캡션, 배지, 상태 |
| `font_size_body` | 13px | 본문, 라벨, 버튼 |
| `font_size_heading` | 13px | 섹션 헤더 (세미볼드로 구분) |
| `font_size_max` | 14px | UI 텍스트 최대 크기 |

## UI 크기

| 변수 | 값 | 용도 |
|------|-----|------|
| `border_width` | 1px | 모든 보더 두께 |
| `corner_radius` | 4px | 기본 둥근 모서리 |
| `item_height_tree` | 22px | 트리 항목 (사이드바 목록) |
| `item_height_interactive` | 28px | 버튼, 입력 필드, 메뉴 항목 |
| `item_height_tab` | 35px | 탭 |

## 간격 (4px 그리드)

| 변수 | 값 | 용도 |
|------|-----|------|
| `spacing_xs` | 4px | 타이트 내부 패딩 |
| `spacing_sm` | 8px | 기본 패딩, 관련 요소 간 |
| `spacing_md` | 12px | 카드 내부, 리스트 항목 |
| `spacing_lg` | 16px | 섹션 패딩 |
| `spacing_xl` | 24px | 주요 섹션 사이 |

## 새 색상/크기 추가 시

1. `src/theme.rs`의 `Theme` 구조체에 필드 추가
2. `Theme::dark()`에 값 설정
3. 이 문서에 해당 변수 추가
4. UI 코드에서 `th.새변수`로 사용
