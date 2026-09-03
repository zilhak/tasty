# tasty-gallery

Tasty UI 컴포넌트 단독 시각 검증을 위한 별도 바이너리 (Storybook 류).
메인 앱 `tasty` 의 IPC/CLI 표면에 어떤 영향도 주지 않으며, 본체와 동일한
lib crate (`tasty-egui-theme` 등) 의 함수를 직접 호출해 "데모 = 메인 =
같은 코드" 를 보장한다.

## 실행

```sh
cargo run -p tasty-gallery
```

상단 툴바에서 Theme (light/dark base) 와 UI scale 을 즉시 토글할 수
있다. 좌측 사이드바에서 카탈로그 항목을 선택하면 우측에 해당 데모가
렌더링된다.

## 본 phase (Phase 1) 의 카탈로그

Tier 1 — `Theme` 만 의존하는 항목:

- **Theme — Color Swatches**: `ThemeColors` 의 모든 token 을 그룹별로 grid.
- **Typography**: `font_size_caption / body / heading`.
- **Spacing**: `spacing_xs / sm / md / lg / xl` 을 4px grid 격자 위에 시각화.
- **Widget — hint_text**: `tasty_egui_theme::hint_text` 데모.

## 카탈로그 항목 추가하기

1. `src/catalog/<group>.rs` 또는 `src/catalog/widgets/<name>.rs` 에
   `pub fn draw(ui: &mut egui::Ui, theme: &Theme)` 를 작성.
2. `src/catalog.rs::pages()` 의 해당 페이지에 `section(...)` / `spec(...)` 한 줄 추가.

## Phase 2/3 (후속)

Tier 2 — props 만으로 호출 가능한 위젯 (divider, toast 외형 등) 은 본체
함수 가시성 확인 후 추가.

Tier 3 — popup / sidebar / tab_bar 의 props 분리는 컴포넌트별 별도 TODO
로 진행. 갤러리는 분리된 view-only 함수를 받아 호출만 한다.
