# `tasty-ui-widgets` — 본체·갤러리 공유 UI primitive

`crates/tasty-ui-widgets/` 는 본체 (`tasty`) 와 갤러리 (`tasty-gallery`) 가 공유하는 *egui layout / 위젯 primitive* 를 모은 라이브러리 크레이트다. 본체의 settings modal 등 UI 코드와 갤러리의 데모가 **동일 함수를 호출** 하여 시각 100% 동기화를 보장한다.

## 위치와 의존 방향

```
tasty-egui-theme     ← egui 어댑터 + 색·폰트 토큰 (선행 분리)
       ↑
tasty-ui-widgets     ← layout / 위젯 primitive (본 문서 대상)
       ↑                ↑
   tasty (본체)    tasty-gallery
```

- 의존: `egui`, `tasty-egui-theme`, `tasty-type-appearance` (Theme schema), `tasty-type-geometry` (LogicalPx/PhysicalPx).
- **본체 (`tasty`) 미의존** — widgets crate 는 본체 state · plugin · 글로벌 `theme()` 호출을 모른다. 모든 함수는 `&Theme` 을 명시적 인자로 받는다.
- 갤러리는 본체 빌드와 분리됨 (`cargo build` 기본 타겟이 `tasty-gallery` 를 의존하지 않음) — widgets crate 도입 후에도 같다. `tasty-gallery` 는 `cargo run -p tasty-gallery` 로만 실행되는 standalone 데모 바이너리.

## 현 위젯 카탈로그

| 함수 | 역할 | 사용 사이트 |
|------|------|------------|
| `two_depth_layout(ui, theme, available_height, left, content)` | 좌측 sub-menu Frame + 우측 content vertical idiom | 본체 settings (Appearance, Keybindings), 갤러리 `layout_2depth::draw_split` |
| `horizontal_tab_bar_with_arrows(ui, id_salt, tabs, active)` | 가로 ScrollArea + chevron overlay (콘텐츠 폭 > viewport 시) | 본체 settings modal 상단 탭, 갤러리 `layout_2depth::draw_top_tabs` |
| `tab_content_frame(ui, content)` | 탭 컨텐츠 4 면 16px inner_margin wrapper | 본체 settings ScrollArea 내부, 갤러리 `layout_2depth::draw_content` |

## Layout 토큰

`tasty-ui-widgets::tokens` 모듈은 위젯이 사용하는 *layout-level* 상수 (색·폰트가 아닌 폭·패딩·corner 등) 를 단일 진실로 보관한다. 색·폰트는 `tasty-type-appearance::theme::Theme` 토큰에서 온다.

SIZING 과 의미가 겹치는 값은 매직넘버로 재정의하지 않고 `tasty-type-appearance::theme::SIZING` 을 단일 소스로 참조한다 (이름은 "이 위치에서 어떤 토큰을 쓰는지" 의미론을 보존).

| 상수 | 값 | SIZING 출처 | 설명 |
|------|----|------|------|
| `SUB_TAB_PANEL_WIDTH` | 150.0 | `tab_width` | `two_depth_layout` 좌측 패널 고정 폭 (logical px) |
| `PANEL_INNER_MARGIN` | 8 (i8) | `spacing_sm` | 좌측 패널 Frame symmetric inner margin |
| `PANEL_CORNER_RADIUS` | 4.0 | `corner_radius` | 좌측 패널 Frame corner radius |
| `PANEL_STROKE_WIDTH` | 1.0 | `border_width` | 좌측 패널 Frame stroke |
| `PANEL_SPACING` | 8.0 | `spacing_sm` | 좌·우 사이 horizontal spacing |
| `TAB_CONTENT_PADDING` | 16 (i8) | `spacing_lg` | `tab_content_frame` inner margin (4 면 동일) |

상수 변경 시 본체와 갤러리 양쪽이 자동으로 동기화된다. 본체 settings 의 sub-tab 폭 정책을 100 → 150 으로 올리던 작업도, 새 widgets crate 도입 후에는 `SUB_TAB_PANEL_WIDTH` 한 줄만 바꾸면 끝난다.

## 사용 idiom

### `two_depth_layout` 호출 패턴 — borrow snapshot

본체 `appearance.rs` / `keybindings_tab.rs` 처럼 *좌측 클릭 시 sub_tab 갱신 + 우측은 현재 sub_tab 분기* 인 경우, 좌·우 두 클로저가 `&mut sub_tab` 을 동시에 캡처할 수 없어 다음 snapshot 패턴을 사용한다:

```rust
let current = sub_tab.clone();         // Copy 면 *sub_tab
let mut selected_new: Option<SubTab> = None;
tasty_ui_widgets::two_depth_layout(
    ui, &theme, available_height,
    |ui| {
        for (tab, label) in &sub_tabs {
            let selected = &current == tab;
            if ui.selectable_label(selected, label).clicked() {
                selected_new = Some(tab.clone());
            }
        }
    },
    |ui| match &current {
        // 우측 분기 ...
    },
);
if let Some(new) = selected_new {
    *sub_tab = new;
}
```

탭 전환에 1 프레임 (~16ms) 지연이 생기지만 사용자 인지 불가 수준이며, layout primitive 의 closure 모델로 인한 자연스러운 비용.

### `horizontal_tab_bar_with_arrows` 호출 패턴

```rust
tasty_ui_widgets::horizontal_tab_bar_with_arrows(
    ui,
    "settings_tabs_scroll",  // ScrollArea state 분리용 unique id
    &tabs,                   // &[(T, &str)] — T: Copy + PartialEq
    &mut ui_state.active_tab,
);
```

chevron 아이콘은 widgets crate 내 `assets/icons/chevron-{left,right}.svg` 사본으로 보관 (본체 `assets/icons/` 원본은 다른 사용처 용도로 유지). 호출자는 `egui_extras::install_image_loaders` 가 미리 호출됐다고 가정.

### `tab_content_frame` 호출 패턴

```rust
egui::ScrollArea::vertical()
    .auto_shrink([false, false])
    .show(ui, |ui| {
        tasty_ui_widgets::tab_content_frame(ui, |ui| match active_tab {
            SettingsTab::General => draw_general_tab(ui, &mut draft),
            // ...
        });
    });
```

## 확장 가이드

위젯을 새로 추가할 때:

1. **본체와 갤러리 양쪽에 같은 idiom 이 이미 ≥ 2 곳 있는지 확인**. 1 곳에서만 쓰이는 함수를 widgets crate 로 끌어올리면 단일 사용처용 abstraction 이 됨.
2. **시그니처는 글로벌 의존 0**. `theme: &Theme` 인자로 받고 `crate::theme::theme()` 직접 호출 금지.
3. **magic number 는 `tokens` 모듈에**. 함수 본문에 `f32` 리터럴 직접 박지 말 것.
4. **borrow 충돌은 호출자에서 snapshot 패턴으로 해결**. widget 함수는 `impl FnOnce(&mut egui::Ui)` 클로저 1~2 개를 받는 단순 시그니처 유지.

## 관련 문서

- [`tasty-egui-theme`](../../crates/tasty-egui-theme/src/lib.rs) — Theme → egui Visuals/Style 변환 어댑터.
- [`tasty-gallery`](../../crates/tasty-gallery/) — widgets crate 의 호출 사이트 + 데모 카탈로그.
- [`design/systems/theme.md`](../design/systems/theme.md) — 4px 그리드 / 1px 보더 등 UI 디자인 규칙.
