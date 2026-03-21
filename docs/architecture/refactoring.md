# 리팩토링 분석

코드 개선 가능성을 6가지 카테고리로 분석하고, 우선순위별 리팩토링 로드맵을 제시한다.

---

## 1. God Object 패턴

### model.rs (1,370줄)

`model.rs`는 13개 타입과 70개 이상의 메서드를 단일 파일에 포함한다. 특히 `PaneNode`과 `SurfaceGroupLayout`이 거의 동일한 바이너리 트리 구조와 메서드 집합을 가지고 있다.

#### 공통 BinaryTree trait 추출

`PaneNode`과 `SurfaceGroupLayout`은 둘 다 바이너리 트리(Leaf/Split)이고 다음 메서드가 구조적으로 동일하다:

| 메서드 | PaneNode 줄 | SurfaceGroupLayout 줄 |
|--------|-------------|----------------------|
| `compute_rects()` | 221 | (render_regions, 963) |
| `find_divider_at()` | 343 | 1050 |
| `update_ratio_for_rect()` | 377 | 1080 |
| `all_*_ids()` | 310 | 1004 |
| `process_all()` | 299 | 1038 |

공통 trait 후보:

```rust
trait BinaryTree {
    type LeafId: Copy + Eq;
    type Leaf;

    fn compute_rects(&self, rect: Rect) -> Vec<(Self::LeafId, Rect)>;
    fn find_divider_at(&self, x: f32, y: f32, rect: Rect, threshold: f32) -> Option<DividerInfo>;
    fn update_ratio_for_rect(&mut self, split_rect: Rect, new_ratio: f32, current_rect: Rect) -> bool;
    fn all_leaf_ids(&self) -> Vec<Self::LeafId>;
}
```

**예상 절감**: ~200줄 코드 중복 제거.

#### Visitor 패턴으로 트리 순회 통합

`state.rs`에서 트리 순회 코드가 반복된다:

- `collect_events_from_pane_node` → `collect_events_from_panel` → `collect_events_from_surface_layout` (state.rs:342-387)
- `set_mark_in_pane_node` → `set_mark_in_panel` → `set_mark_in_surface_layout` (state.rs:420-475)
- `read_mark_in_pane_node` → `read_mark_in_panel` → `read_mark_in_surface_layout` (state.rs:493-552)

Visitor 패턴 적용 시:

```rust
trait TerminalVisitor {
    fn visit(&mut self, surface_id: u32, terminal: &mut Terminal);
}

impl PaneNode {
    fn visit_terminals(&mut self, visitor: &mut dyn TerminalVisitor);
}
```

이렇게 하면 `set_mark`, `read_mark`, `collect_events` 각각 3단계 × 3세트 = 9개 함수를 1개 Visitor + 3개 구현으로 줄일 수 있다.

**예상 절감**: ~130줄.

### state.rs (733줄)

`AppState`는 12개 `pub` 필드를 가진 God Object이다:

```rust
pub struct AppState {
    pub workspaces: Vec<Workspace>,      // 50행
    pub active_workspace: usize,          // 52행
    pub default_cols: usize,              // 54행
    pub default_rows: usize,              // 55행
    pub notifications: NotificationStore, // 56행
    pub notification_panel_open: bool,    // 58행
    pub settings: Settings,               // 60행
    pub settings_open: bool,              // 62행
    pub settings_ui_state: SettingsUiState, // 64행
    pub hook_manager: HookManager,        // 66행
    pub sidebar_width: f32,               // 68행
    // (private: next_ids, waker)
}
```

UI 상태(`notification_panel_open`, `settings_open`, `settings_ui_state`)와 도메인 상태(`workspaces`, `notifications`, `hook_manager`)가 혼재한다.

#### 분리 제안

```rust
pub struct AppState {
    pub workspace_manager: WorkspaceManager,  // workspaces + active_workspace + next_ids
    pub notifications: NotificationStore,
    pub hook_manager: HookManager,
    pub settings: Settings,
    pub ui_state: UiState,  // notification_panel_open, settings_open, settings_ui_state, sidebar_width
}
```

---

## 2. 모듈 간 결합도 문제

### gpu.rs → settings_ui 직접 호출

`gpu.rs:174` — `settings_ui::draw_settings_window()` 직접 호출.

`GpuState::render()`가 GPU 렌더링과 UI 로직을 함께 처리한다. 렌더링 오케스트레이션이 GPU 모듈에 있어야 하지만, 설정 UI 로직까지 호출하는 것은 관심사 혼합이다.

**개선안**: `gpu.rs`의 `render()` 내 egui 프레임 로직을 별도 함수 또는 `ui.rs`로 이동.

```rust
// ui.rs에 통합
pub fn draw_all(ctx: &egui::Context, state: &mut AppState, scale_factor: f32, pane_rects: &[(u32, Rect)]) {
    draw_ui(ctx, state, scale_factor);
    draw_pane_tab_bars(ctx, state, pane_rects, scale_factor);
    draw_notification_panel(ctx, state);
    if state.settings_open {
        settings_ui::draw_settings_window(ctx, &mut state.settings, &mut state.settings_open, &mut state.settings_ui_state);
    }
}
```

### state.rs pub 필드 과다

`AppState`의 12개 필드 중 10개가 `pub`이다. 어떤 모듈이든 직접 필드에 접근하여 불변량을 깨뜨릴 수 있다.

**영향받는 코드**:
- `main.rs:170` — `state.notification_panel_open = !state.notification_panel_open` (직접 토글).
- `main.rs:194` — `state.settings_open = !state.settings_open`.
- `gpu.rs:150` — `state.sidebar_width` 직접 읽기.
- `gpu.rs:172` — `state.settings.clone()`, `state.settings_open` 직접 읽기/쓰기.

**개선안**: 필드를 `pub(crate)` 또는 private으로 변경하고, 메서드로 접근 제공.

---

## 3. 관심사 분리 위반

### Dead Code — 설정의 미반영 필드

`settings.rs`에 정의된 필드 중 런타임에 사용되지 않거나 미구현인 것들:

| 필드 | 줄 | 상태 |
|------|-----|------|
| `appearance.font_family` | 28 | UI에 표시만, GPU 렌더러에 미반영 (CellRenderer는 시스템 기본 모노스페이스 사용) |
| `appearance.font_size` | 29 | GPU 초기화 시 14.0 하드코딩 (`gpu.rs:88`), 설정값 미사용 (`main.rs:321` 주석) |
| `appearance.theme` | 30 | UI에 라디오 버튼만 있고, 색상 팔레트에 미반영 |
| `appearance.background_opacity` | 31 | UI 슬라이더만 있고, wgpu clear color에 미반영 |
| `clipboard.macos_style` | 38 | 클립보드 기능 미구현 |
| `clipboard.linux_style` | 39 | 클립보드 기능 미구현 |
| `clipboard.windows_style` | 40 | 클립보드 기능 미구현 |
| `notification.sound` | 48 | UI 체크박스만 있고, 사운드 재생 미구현 |
| `keybindings.*` | 55-61 | 모든 6개 필드가 UI에 없고, `main.rs`에서 하드코딩된 단축키 사용 |
| `general.startup_command` | 22 | UI에 편집 필드만 있고, 시작 시 실행 미구현 |

### 개선안
1. 미구현 기능의 설정 필드에 `#[serde(skip)]` 또는 주석으로 상태 명시.
2. 또는 기능 구현과 함께 반영.
3. `font_size` 하드코딩 (gpu.rs:88)을 설정값으로 교체.

---

## 4. 코드 중복

### PaneNode / SurfaceGroupLayout 중복 메서드

1절에서 분석한 바이너리 트리 메서드 외에도, 다음 메서드 쌍이 구조적으로 동일하다:

| PaneNode | SurfaceGroupLayout | 패턴 |
|----------|-------------------|------|
| `find_pane()` / `find_pane_mut()` (239, 255행) | `find_terminal()` / `find_terminal_mut()` (927, 943행) | ID로 리프 탐색 |
| `all_pane_ids()` (310행) | `all_surface_ids()` (1004행) | 모든 리프 ID 수집 |
| `all_terminals()` / `all_terminals_mut()` (275, 287행) | `collect_terminals()` / `collect_terminals_mut()` (1016, 1027행) | 모든 터미널 수집 |
| `process_all()` (299행) | `process_all()` (1038행) | 모든 터미널 process |
| `first_pane()` (213행) | `first_terminal()` / `first_surface_id()` (911, 919행) | 첫 번째 리프 |
| `next_pane_id()` / `prev_pane_id()` (322, 332행) | (SurfaceGroupNode의) `move_focus_forward()` / `move_focus_backward()` (807, 820행) | 포커스 이동 |

**예상 절감**: BinaryTree trait로 통합하면 ~300줄 중복 제거.

### set_mark_in_* / read_mark_in_* 중복

`state.rs`의 마크 관련 함수 6개 (420-552행):

```
set_mark_in_pane_node  → set_mark_in_panel  → set_mark_in_surface_layout
read_mark_in_pane_node → read_mark_in_panel → read_mark_in_surface_layout
```

이 두 세트는 트리 순회 패턴이 동일하고 리프에서의 작업만 다르다.
`collect_events_from_*` (342-387행)도 같은 패턴이다.

총 9개 함수, ~210줄이 동일한 순회 패턴을 반복한다.

### default_shell() 중복

셸 경로 감지 로직이 두 곳에 존재한다:

1. `terminal.rs:626-635` — `Terminal::default_shell()`.
2. `settings.rs:88-97` — `GeneralSettings::detect_shell()`.

두 함수의 구현이 완전히 동일하다 (`COMSPEC` / `SHELL` 환경 변수).

**개선안**: 하나로 통합하고 다른 쪽에서 import.

---

## 5. 확장성 저해 요소

### 단일 CellRenderer

`gpu.rs:19` — `GpuState`가 `renderer: CellRenderer` 하나만 소유한다. 모든 서피스가 동일한 CellRenderer를 공유하며, `prepare_viewport()` 호출 시마다 유니폼 버퍼를 덮어쓴다 (`renderer.rs:641-650`).

**문제**: 멀티 서피스 렌더링이 순차적이다. 각 서피스마다 `prepare_viewport` + `render_scissored`를 반복해야 하므로, 서피스 수에 비례하여 draw call이 증가한다.

**개선안**:
- 서피스별 유니폼을 배열이나 동적 오프셋으로 관리.
- 또는 인스턴스 데이터에 뷰포트 오프셋을 포함시켜 단일 draw call로 모든 서피스를 렌더.

### 고정 아틀라스 크기

`font.rs:113` — `ATLAS_SIZE: u32 = 2048`. 고정 크기이며, 가득 차면 전체 캐시를 초기화한다 (273행).

**문제**: 많은 유니코드 문자 (CJK, 이모지 등)를 사용하면 아틀라스가 자주 리셋되어 성능 저하.

**개선안**:
- 다중 아틀라스 페이지 (새 텍스처 할당).
- 동적 아틀라스 크기 (GPU 제한에 맞춰 증가).
- LRU 캐시로 사용 빈도 낮은 글리프 교체.

### Pane/Tab 삭제 — 완료

`PaneNode::close_pane()`, `Pane::close_tab()`, `Pane::close_active_tab()`, `SurfaceGroupLayout::close_surface()`, `SurfaceGroupNode::close_surface()` API가 구현되었다. `AppState`에 `close_active_tab()`, `close_active_pane()`, `close_active_surface()` 메서드가 추가되었고, IPC/CLI에 `tab.close`, `pane.close`, `surface.close` 명령이 추가되었다. 키보드 단축키: Ctrl+W(탭 닫기), Ctrl+Shift+W(패인 닫기).

### DECSET / DECRST — 완료

DECSET/DECRST가 구현되었다. 지원 모드: DECCKM(1), StartBlinkingCursor(12), DECTCEM(25), EnableAlternateScreen(47/1047), ClearAndEnableAlternateScreen(1049), SaveCursor(1048), MouseTracking(1000), ButtonEventMouse(1002), AnyEventMouse(1003), FocusTracking(1004), SGRMouse(1006), BracketedPaste(2004). 대체 화면 버퍼(primary_surface/alternate_surface)가 구현되어 vim, htop, less, nano 등 TUI 앱이 동작한다.

### 클립보드 미구현

`settings.rs:36-41` — `ClipboardSettings`가 정의되어 있지만, 실제 클립보드 읽기/쓰기 기능이 없다. `arboard` 크레이트 등으로 구현 필요.

---

## 6. 우선순위별 리팩토링 로드맵

### P0 (긴급) — 기능 정상 작동에 필요

| 항목 | 영향 범위 | 예상 작업량 | 상태 |
|------|-----------|------------|------|
| DECSET/DECRST 구현 | terminal.rs | 중 (200-300줄) | 완료 |
| font_size 설정 반영 | gpu.rs:88 하드코딩 제거 | 소 (10줄) | 미착수 |
| Pane/Tab 삭제 API | model.rs, state.rs | 중 (150줄) | 완료 |

### P1 (높음) — 코드 품질 개선

| 항목 | 영향 범위 | 예상 작업량 |
|------|-----------|------------|
| default_shell() 통합 | terminal.rs, settings.rs | 소 (5줄) |
| Visitor 패턴으로 트리 순회 통합 | state.rs | 중 (150줄 절감) |
| AppState pub 필드 → 메서드 접근 | state.rs, main.rs, gpu.rs, ui.rs | 중 (100줄) |
| 즉시 분리 가능 크레이트 추출 (protocol, hooks, notification) | 프로젝트 구조 | 소 (경로 변경만) |

### P2 (중간) — 아키텍처 개선

| 항목 | 영향 범위 | 예상 작업량 |
|------|-----------|------------|
| BinaryTree trait 추출 | model.rs | 대 (300줄 리팩토링) |
| GPU 렌더링과 UI 로직 분리 | gpu.rs, ui.rs | 중 (100줄) |
| AppState 분리 (WorkspaceManager + UiState) | state.rs | 중 (150줄) |
| Dead code 설정 필드 정리/구현 | settings.rs, gpu.rs, main.rs | 분야별 상이 |

### P3 (낮음) — 확장성/성능

| 항목 | 영향 범위 | 예상 작업량 |
|------|-----------|------------|
| 다중 아틀라스 페이지 | font.rs | 대 (200줄) |
| 단일 CellRenderer → 멀티 서피스 최적화 | renderer.rs, gpu.rs | 대 (300줄) |
| Terminal trait 추출 + model.rs 분리 | model.rs, terminal.rs | 대 (500줄) |
| TerminalSurface trait 추출 + 렌더러 분리 | renderer.rs, font.rs | 대 (400줄) |
| 클립보드 구현 | 새 파일 | 중 (200줄) |
| 키바인딩 커스터마이징 | settings.rs, main.rs | 대 (300줄) |

---

## 핵심 우선순위 요약

1. **즉시 해결**: `font_size` 하드코딩 제거, `default_shell()` 통합.
2. **완료된 단기 목표**: DECSET/DECRST 구현 (완료), Pane/Tab 삭제 API (완료).
3. **남은 단기 목표**: 즉시 분리 가능 크레이트 추출.
4. **중기 목표**: BinaryTree trait, Visitor 패턴, AppState 분리.
5. **장기 목표**: Terminal trait 추출, 렌더러 분리, 아틀라스 개선.
