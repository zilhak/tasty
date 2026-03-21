# 05. 분할 패인

## 두 가지 분할 유형

tasty는 cmux와 동일하게 두 가지 분할을 지원한다.

### 1. Pane 분할 (물리적 화면 분할)

**단축키**: Ctrl+Shift+E (수직), Ctrl+Shift+O (수평)

화면을 물리적으로 나눈다. 새로운 영역은 **독립적인 탭 바**를 가진다.

```
┌─────────────────────┬──────────────────────┐
│ Pane A              │ Pane B               │
│ ┌──┬──┬──┐          │ ┌──┬──┐              │
│ │T1│T2│T3│ ← 탭 바   │ │T1│T2│ ← 독립 탭 바 │
│ └──┴──┴──┘          │ └──┴──┘              │
│ [Terminal]          │ [Terminal]           │
│                     │                      │
└─────────────────────┴──────────────────────┘
```

- PaneNode 이진 트리로 관리한다.
- 각 Pane은 독립적으로 탭을 전환한다.
- Pane 간 포커스 이동은 Alt+Arrow로 한다.

### 2. SurfaceGroup 분할 (탭 내부 분할)

**단축키**: Ctrl+D (수직), Ctrl+Shift+D (수평)

현재 탭 내부에서 나눈다. 탭 바에서는 **하나의 탭**으로 보인다.

```
┌─────────────────────────────────────────────┐
│ Pane A                                      │
│ ┌──┬──────┬──┐                              │
│ │T1│ T2   │T3│ ← T2가 SurfaceGroup          │
│ └──┴──────┴──┘                              │
│ ┌────────────────┬──────────────────────────┐│
│ │ Terminal 1     │ Terminal 2               ││
│ │ (SurfaceGroup  │ (SurfaceGroup           ││
│ │  내부 분할)     │  내부 분할)              ││
│ └────────────────┴──────────────────────────┘│
└─────────────────────────────────────────────┘
```

- SurfaceGroupLayout 이진 트리로 관리한다.
- Panel::Terminal이 자동으로 Panel::SurfaceGroup으로 변환된다.
- 탭을 전환하면 SurfaceGroup 전체가 함께 전환된다.

## 레이아웃 엔진

### PaneNode (Pane 분할 트리)

```rust
pub enum PaneNode {
    Leaf(Pane),
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}
```

`compute_rects(rect)` 메서드로 각 Pane에 할당할 물리적 픽셀 영역을 재귀적으로 계산한다.

### SurfaceGroupLayout (탭 내부 분할 트리)

```rust
pub enum SurfaceGroupLayout {
    Single(SurfaceNode),
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<SurfaceGroupLayout>,
        second: Box<SurfaceGroupLayout>,
        focus_second: bool,
    },
}
```

`render_regions(rect)` 메서드로 각 터미널 서피스에 할당할 영역을 계산한다.

## 렌더링

1. PaneNode에서 각 Pane의 rect를 계산한다.
2. 각 Pane의 rect 상단에 탭 바를 렌더링한다 (탭이 2개 이상일 때만).
3. 탭 바를 제외한 영역에서 활성 탭의 Panel을 렌더링한다.
4. Panel이 SurfaceGroup이면 내부 분할에 따라 여러 터미널을 렌더링한다.
5. 각 터미널은 scissor rect로 독립 렌더링한다.

## 포커스 관리

포커스 경로: Workspace → focused PaneId → Pane의 active_tab → Panel의 focused_surface

| 수준 | 포커스 대상 | 전환 방법 |
|------|-----------|----------|
| Workspace | active_workspace | Alt+1~9 |
| Pane | focused_pane (PaneId) | Alt+Arrow |
| Tab | active_tab (인덱스) | Ctrl+Tab / Ctrl+Shift+Tab |
| Surface | focused_surface (SurfaceId) | SurfaceGroup 내부 이동 |

## 키보드 단축키

| 단축키 | 동작 |
|--------|------|
| Ctrl+Shift+E | Pane 수직 분할 (새 독립 탭 바) |
| Ctrl+Shift+O | Pane 수평 분할 (새 독립 탭 바) |
| Ctrl+D | SurfaceGroup 수직 분할 (탭 내부) |
| Ctrl+Shift+D | SurfaceGroup 수평 분할 (탭 내부) |
| Alt+Arrow | Pane 간 포커스 이동 |

## 리사이즈

분할/리사이즈 시 모든 터미널의 행/열을 자동 재계산한다.

- PaneNode의 compute_rects로 Pane 영역을 계산한다.
- 탭 바 높이를 빼고 남은 영역에서 Panel의 resize_all을 호출한다.
- SurfaceGroupLayout도 재귀적으로 하위 터미널을 리사이즈한다.
- PTY에 새 크기를 통보한다.

## 구현 현황

- PaneNode 이진 트리 기반 Pane 분할 구현 완료
- SurfaceGroupLayout 이진 트리 기반 탭 내부 분할 구현 완료
- Panel::Terminal → Panel::SurfaceGroup 자동 변환 구현 완료
- scissor rect 기반 독립 렌더링 구현 완료
- 분할/리사이즈 시 모든 터미널 자동 크기 재조정 구현 완료
