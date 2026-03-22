# model.rs 분할 계획

`src/model.rs` (1,775줄)를 `src/model/` 디렉토리로 분할한다.

## 현재 구조 분석

model.rs는 6개 주요 타입과 그 impl 블록, 유틸리티 함수, 테스트로 구성된다.

| 줄 범위 | 내용 |
|---------|------|
| 1 | `use crate::terminal::{Terminal, Waker}` |
| 3-6 | type alias: `WorkspaceId`, `PaneId`, `TabId`, `SurfaceId` |
| 8-71 | `Rect` struct + impl (contains, approx_eq, split) |
| 73-144 | `Workspace` struct + impl |
| 146-478 | `PaneNode` enum + impl (split, close, find, compute_rects, divider 등) |
| 480-675 | `Pane` struct + impl (new, add_tab, close_tab, terminals 등) |
| 677-711 | `Tab` struct + impl (panel, take/put) |
| 713-835 | `Panel` enum + impl (focused_terminal, render_regions, resize, split) |
| 837-841 | `SurfaceNode` struct |
| 843-978 | `SurfaceGroupNode` struct + impl (close, split, focus, resize) |
| 980-1342 | `SurfaceGroupLayout` enum + impl (split_with_node, close_surface, find, render, divider) |
| 1344-1355 | `compute_terminal_rect()` 함수 |
| 1357-1370 | `DividerInfo` struct, `SplitDirection` enum |
| 1372-1775 | `#[cfg(test)] mod tests` |

## 분할 후 구조

```
src/model/
├── mod.rs              — re-exports, Rect, SplitDirection, DividerInfo, compute_terminal_rect, type aliases
├── workspace.rs        — Workspace struct + methods
├── pane.rs             — PaneNode + Pane + Tab
├── panel.rs            — Panel enum + SurfaceNode
├── surface_group.rs    — SurfaceGroupNode + SurfaceGroupLayout
└── tests.rs            — 모든 테스트
```

## 각 파일 상세

### mod.rs (~100줄)

re-export와 공통 타입을 담는다.

**포함 내용:**
- `use crate::terminal::{Terminal, Waker}` (줄 1)
- type alias: `WorkspaceId`, `PaneId`, `TabId`, `SurfaceId` (줄 3-6)
- `Rect` struct + impl (줄 8-71)
- `SplitDirection` enum (줄 1366-1370)
- `DividerInfo` struct (줄 1357-1364)
- `compute_terminal_rect()` 함수 (줄 1344-1355)
- 하위 모듈 선언 및 re-export

```rust
mod workspace;
mod pane;
mod panel;
mod surface_group;
#[cfg(test)]
mod tests;

pub use workspace::Workspace;
pub use pane::{PaneNode, Pane, Tab};
pub use panel::{Panel, SurfaceNode};
pub use surface_group::{SurfaceGroupNode, SurfaceGroupLayout};
```

### workspace.rs (~70줄)

**포함 내용:**
- `Workspace` struct (줄 74-80)
- `impl Workspace` 전체 (줄 82-144): `new`, `new_with_shell`, `pane_layout`, `pane_layout_mut`, `take_pane_layout`, `put_pane_layout`

**의존:**
- `super::{WorkspaceId, PaneId, TabId, SurfaceId}`
- `super::pane::{PaneNode, Pane}`
- `crate::terminal::Waker`

### pane.rs (~500줄)

`PaneNode`, `Pane`, `Tab` 세 타입을 하나의 파일에 둔다.

**포함 내용:**
- `PaneNode` enum (줄 146-156)
- `impl PaneNode` (줄 158-478): `split_pane_in_place`, `close_pane`, `first_pane`, `compute_rects`, `find_pane`, `find_pane_mut`, `all_terminals`, `all_terminals_mut`, `process_all`, `for_each_terminal`, `for_each_terminal_mut`, `all_pane_ids`, `next_pane_id`, `prev_pane_id`, `find_divider_at`, `update_ratio_for_rect`
- `Pane` struct (줄 480-485)
- `impl Pane` (줄 487-675): `new`, `new_with_shell`, `add_tab`, `add_tab_with_shell`, `active_panel`, `active_panel_mut`, `split_active_surface`, `split_active_surface_with_shell`, `close_tab`, `close_active_tab`, `active_terminal`, `active_terminal_mut`, `next_tab`, `prev_tab`, `all_terminals`, `all_terminals_mut`
- `Tab` struct (줄 677-683)
- `impl Tab` (줄 685-711): `panel`, `panel_mut`, `take_panel`, `put_panel`

**Pane+Tab을 같이 두는 이유:**
- `Pane.tabs: Vec<Tab>` 관계 — Pane 메서드가 Tab의 panel에 직접 접근한다 (줄 566, 573, 605-608).
- `add_tab()` (줄 527-560)에서 `Tab` 리터럴을 직접 생성한다.
- `close_tab()` (줄 614-627)에서 `self.tabs` 벡터를 직접 조작한다.
- 분리하면 Tab의 `take_panel`/`put_panel` (현재 `fn` 가시성, 줄 700-710)을 `pub(crate)`로 올려야 하고 캡슐화가 약해진다.

**PaneNode과 Pane을 같이 두는 이유:**
- `PaneNode::Leaf(Pane)` — PaneNode의 모든 재귀 메서드가 `Pane`에 직접 접근한다 (줄 174, 266, 274, 292, 308, 328, 340, 366, 384, 397-399).
- PaneNode과 Pane을 분리하면 양방향 의존이 발생한다.

**의존:**
- `super::{PaneId, TabId, SurfaceId, Rect, SplitDirection, DividerInfo}`
- `super::panel::{Panel, SurfaceNode}`
- `super::surface_group::SurfaceGroupNode`
- `crate::terminal::{Terminal, Waker}`

### panel.rs (~170줄)

**포함 내용:**
- `SurfaceNode` struct (줄 837-841)
- `Panel` enum (줄 713-719)
- `impl Panel` (줄 721-835): `focused_terminal`, `focused_terminal_mut`, `collect_terminals`, `collect_terminals_mut`, `for_each_terminal`, `for_each_terminal_mut`, `render_regions`, `resize_all`, `split_surface_with_terminal`

**Panel을 따로 분리하는 이유:**
- Panel은 SurfaceNode와 SurfaceGroupNode을 래핑하는 브릿지 역할이다.
- Pane/Tab은 Panel을 "소비"만 하고, Panel은 SurfaceNode/SurfaceGroupNode을 "소비"만 한다.
- 즉 `Pane → Panel → SurfaceGroupNode` 단방향 의존이므로 깔끔하게 분리된다.

**의존:**
- `super::{SurfaceId, Rect, SplitDirection}`
- `super::surface_group::{SurfaceGroupNode, SurfaceGroupLayout}`
- `crate::terminal::Terminal`

### surface_group.rs (~400줄)

**포함 내용:**
- `SurfaceGroupNode` struct (줄 843-850)
- `impl SurfaceGroupNode` (줄 852-978): `layout`, `layout_mut`, `take_layout`, `put_layout`, `close_surface`, `split_surface`, `compute_rects`, `focused_terminal`, `focused_terminal_mut`, `resize_all`, `move_focus_forward`, `move_focus_backward`
- `SurfaceGroupLayout` enum (줄 980-990)
- `impl SurfaceGroupLayout` (줄 992-1342): `split_with_node`, `close_surface`, `first_terminal`, `first_surface_id`, `find_terminal`, `find_terminal_mut`, `render_regions`, `resize_all`, `all_surface_ids`, `collect_terminals`, `collect_terminals_mut`, `process_all`, `for_each_terminal`, `for_each_terminal_mut`, `find_divider_at`, `update_ratio_for_rect`, `find_surface_at`

**의존:**
- `super::{SurfaceId, Rect, SplitDirection, DividerInfo}`
- `super::panel::SurfaceNode`
- `crate::terminal::{Terminal, Waker}`

### tests.rs (~400줄)

**포함 내용:**
- `#[cfg(test)] mod tests` 전체 (줄 1372-1775)
- Rect 테스트 (줄 1383-1461)
- PaneNode 테스트 (줄 1465-1683)
- SurfaceGroupLayout 테스트 (줄 1687-1694)
- Visitor 패턴 테스트 (줄 1698-1739)
- compute_terminal_rect 테스트 (줄 1743-1775)

**의존:**
- `super::*` (모든 pub 타입)

## pub 인터페이스 설계

분할 후 외부에서 사용하는 인터페이스는 변하지 않는다. `mod.rs`의 re-export가 현재의 `model.rs` pub 인터페이스를 그대로 유지한다.

```rust
// 외부 코드에서의 사용법 — 변경 없음
use crate::model::{Workspace, PaneNode, Pane, Tab, Panel, SurfaceNode};
use crate::model::{SurfaceGroupNode, SurfaceGroupLayout};
use crate::model::{Rect, SplitDirection, DividerInfo};
use crate::model::{WorkspaceId, PaneId, TabId, SurfaceId};
use crate::model::compute_terminal_rect;
```

## 내부 가시성 변경

| 현재 | 분할 후 | 이유 |
|------|---------|------|
| `Tab::take_panel` (fn) | `pub(super)` | `Pane::split_active_surface_with_shell` (줄 607)에서 호출 |
| `Tab::put_panel` (fn) | `pub(super)` | 같은 이유 |
| `SurfaceGroupNode::take_layout` (fn) | `pub(super)` | `Panel::split_surface_with_terminal` (줄 827-828)에서 호출 |
| `SurfaceGroupNode::put_layout` (fn) | `pub(super)` | 같은 이유 |
