# Model + Host View 분리 패턴

`tasty-core`의 surface 모델은 GUI-free다 (egui/wgpu 직접 사용 금지). 휘발성 GUI 상태(텍스처, 캐시, 편집 세션, 스크롤, 팝업 버퍼 등)는 호스트 측 **View** 구조체에 둔다. 이 문서는 새 surface 타입을 추가하거나 기존 surface에 뷰 상태를 추가할 때 따라야 할 패턴을 정리한다.

## 왜 분리하나

- **플러그인 호환성**: 모델은 직렬화 가능한 식별 정보만 보유하므로 플러그인 프로세스가 동일 모델을 그대로 쓸 수 있다.
- **테스트 용이성**: 모델 단위 테스트가 GUI 컨텍스트 없이 가능하다.
- **메모리 정리 일관성**: View가 store에 모이면 surface 닫힘 시 한 곳에서 일괄 해제한다.

## 어디에 무엇을 두나

| 종류 | 위치 | 예시 |
|------|------|------|
| 식별 정보 | model | `id`, `file_path`, `url`, `dir_images`, `current_index` |
| 직렬화되는 영속 상태 | model | mtime, 사용자 입력(텍스트 버퍼는 view), 트리 구조 |
| egui 타입 | view | `egui::ColorImage`, `egui::TextureHandle`, `egui_commonmark::CommonMarkCache` |
| 편집 세션 머신 | view | `EditState`, `DragState`, `ActionHistory`, `StrokeBuilder` |
| 휘발성 UI 버퍼 | view | popup 텍스트 버퍼, scroll offset, brush 설정 |
| 디스크 I/O 헬퍼 | view (free fn) | `load_image_from_path`, 픽셀 변환 함수 |

판단 기준: **"이 상태를 플러그인 프로세스가 들고 있을 이유가 있는가?"** 없으면 view, 있으면 model.

## 패턴

### 1. Model — 슬림 식별·탐색 정보만

```rust
// crates/tasty-core/src/model/foo_panel.rs
pub struct FooPanel {
    pub id: u32,
    pub file_path: String,
    last_mtime: Option<SystemTime>,
}

impl FooPanel {
    pub fn new(id: u32, file_path: String) -> Self { /* ... */ }

    /// View가 매 프레임 호출. 외부 변경 감지되면 새 콘텐츠 반환.
    pub fn poll_reload(&mut self) -> Option<String> { /* ... */ }
}

impl Surface for FooPanel {
    fn kind(&self) -> &'static str { "foo" }
    fn type_name(&self) -> &'static str { "Foo" }
    /* ... */
}
```

### 2. View + Store — 호스트 측

```rust
// src/ui/foo_view.rs
use std::collections::HashMap;
use crate::model::{FooPanel, SurfaceId};

pub struct FooView {
    pub content: String,
    pub texture: Option<egui::TextureHandle>,
    /* GUI-bound fields */
}

impl FooView {
    pub fn new(panel: &FooPanel) -> Self { /* ... */ }
    pub fn replace_content(&mut self, new_content: String) { /* ... */ }
}

#[derive(Default)]
pub struct FooViewStore {
    views: HashMap<SurfaceId, FooView>,
}

impl FooViewStore {
    /// 첫 접근 시 생성 + poll_reload 자동 적용.
    pub fn get_or_init(&mut self, panel: &mut FooPanel) -> &mut FooView {
        let view = self.views.entry(panel.id).or_insert_with(|| FooView::new(panel));
        if let Some(c) = panel.poll_reload() {
            view.replace_content(c);
        }
        view
    }

    pub fn drop_view(&mut self, sid: SurfaceId) {
        self.views.remove(&sid);
    }
}
```

### 3. AppState 등록

```rust
// src/state/mod.rs
pub struct AppState {
    /* ... */
    pub foo_views: crate::ui::foo_view::FooViewStore,
}

impl AppState {
    pub(crate) fn cleanup_surface(&mut self, sid: u32) {
        /* ... */
        self.foo_views.drop_view(sid);  // ← 누락하면 close/reopen 시 메모리 누수
    }
}
```

### 4. 렌더 호출 — `mem::take` 패턴

`egui_panels.rs` 같은 디스패치 루프에서 `&mut FooPanel`(state.engine.workspaces 경로)와 `&mut FooView`(state.foo_views 경로)를 동시에 mutable 보유해야 하면 borrow checker가 막는다. 해결: 루프 진입 직전에 store를 일시 추출.

```rust
let mut foo_views = std::mem::take(&mut state.foo_views);

for info in &infos {
    /* surface borrow chain ... */
    if let Some(foo_panel) = surface.as_foo_mut() {
        let view = foo_views.get_or_init(foo_panel);
        crate::foo_ui::draw_foo(ui, foo_panel, view);
    }
}

state.foo_views = foo_views;  // 반드시 복원 (이후 state 접근 전에)
```

같은 패턴이 `clipboard_history` (egui_panels.rs), `markdown_views`/`image_views` (egui_panels.rs), `image_views` (clipboard.rs `paste_to_image`)에서 사용된다.

## 안티패턴 (하지 말 것)

- **Model에 `egui::*` 필드 추가**: 플러그인 호환성을 깬다. View로 옮길 것.
- **store drop_view 누락**: close/reopen 시 텍스처/캐시 누수. `cleanup_surface` 단위 테스트로 강제하기.
- **mem::take 후 복원 누락**: 다음 프레임에서 빈 store가 되어 모든 view 재생성 → flicker / 상태 손실.
- **panel과 view 사이의 양방향 의존**: view는 panel을 읽지만 panel은 view를 모른다. panel 메서드가 view 필드를 인자로 받지 않아야 한다.

## 현재 적용된 surface

| Model | View | Store |
|-------|------|-------|
| `MarkdownPanel` (file_path + mtime) | `MarkdownView` (content, scroll_offset, commonmark_cache) | `AppState::markdown_views` |
| `ImagePanel` (file_path, dir_images, current_index) | `ImageView` (original_image, texture, edit_state, brush, popup buffers) | `AppState::image_views` |
| `HtmlPanel` (url) | (없음 — 모델 자체가 슬림. native WebView는 `MainWindow::webviews`) | — |
| `TerminalSurface` | (없음 — 터미널 자체가 호스트와 분리되어 있고 GPU 렌더링) | — |
| `ExplorerPanel` | (모델 안에 트리 expansion 상태 보유 — 추후 분리 검토) | — |

신규 surface 추가 시 위 표에 줄을 추가하라.
