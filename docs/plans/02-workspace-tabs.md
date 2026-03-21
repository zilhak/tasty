# 02. 워크스페이스 & 탭

## 계층 구조

cmux 분석을 바탕으로 설계한 tasty의 데이터 모델 계층.

```
Workspace (사이드바 항목)
  └── PaneLayout (물리적 화면 분할 - 이진 트리)
       ├── Pane (독립적인 탭 바를 가진 화면 영역)
       │    ├── Tab → Panel::Terminal (단일 터미널)
       │    ├── Tab → Panel::SurfaceGroup (탭 내부 분할)
       │    │          ├── Panel::Terminal
       │    │          └── Panel::Terminal
       │    └── Tab → Panel::Terminal
       └── Pane (독립적인 탭 바를 가진 화면 영역)
            └── Tab → Panel::Terminal
```

### 핵심 개념

| 개념 | 설명 |
|------|------|
| Workspace | 사이드바의 한 항목. PaneLayout을 포함한다. |
| PaneLayout (PaneNode) | Pane들의 이진 분할 트리. 물리적 화면 분할을 담당한다. |
| Pane | 자신만의 **독립적인 탭 바**를 가진 화면 영역. 여러 Tab을 포함한다. |
| Tab | Pane의 탭 바에 있는 하나의 탭. Panel에 매핑된다. |
| Panel | 콘텐츠 타입 enum. Terminal 또는 SurfaceGroup이다. |
| SurfaceGroup | 탭 하나 안에서 콘텐츠를 여러 Panel로 분할하는 이진 트리. Pane의 탭 바에서는 **하나의 탭**으로 보이지만, 내부적으로 여러 터미널을 렌더링한다. |

### 두 가지 분할 유형

1. **Pane 분할** (Ctrl+Shift+E/O): 화면을 물리적으로 나눈다. 각 새 영역은 자체 탭 바를 가진다. 탭이 독립적으로 전환된다.
2. **SurfaceGroup 분할** (Ctrl+D / Ctrl+Shift+D): 현재 탭 내부에서 나눈다. 탭 바에서는 하나의 탭으로 보인다. 탭을 전환하면 내부 서피스 전체가 함께 전환된다.

## 데이터 모델

```rust
pub type WorkspaceId = u32;
pub type PaneId = u32;
pub type TabId = u32;
pub type SurfaceId = u32;

/// Workspace - 사이드바 항목 하나
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub pane_layout: PaneNode,     // Pane들의 이진 분할 트리
    pub focused_pane: PaneId,      // 현재 포커스된 Pane
}

/// Pane의 이진 분할 트리 (물리적 화면 분할)
pub enum PaneNode {
    Leaf(Pane),
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

/// 독립적인 탭 바를 가진 화면 영역
pub struct Pane {
    pub id: PaneId,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
}

/// Pane 탭 바의 탭 하나
pub struct Tab {
    pub id: TabId,
    pub name: String,
    pub panel: Panel,
}

/// 콘텐츠 타입
pub enum Panel {
    Terminal(SurfaceNode),              // 단일 터미널
    SurfaceGroup(SurfaceGroupNode),     // 탭 내부 분할
}

/// 단일 터미널 인스턴스
pub struct SurfaceNode {
    pub id: SurfaceId,
    pub terminal: Terminal,
}

/// 탭 내부 분할 (하나의 탭으로 보이지만 여러 터미널을 렌더링)
pub struct SurfaceGroupNode {
    pub layout: SurfaceGroupLayout,
    pub focused_surface: SurfaceId,
}

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

## 탭 전환 동작

### Pane 수준
- Ctrl+Tab / Ctrl+Shift+Tab으로 포커스된 Pane의 탭을 전환한다.
- 각 Pane의 탭은 **독립적으로** 전환된다. Pane A의 탭을 바꿔도 Pane B에 영향 없다.

### SurfaceGroup 수준
- SurfaceGroup은 탭 바에서 하나의 탭으로 표시된다.
- 탭을 전환하면 SurfaceGroup 전체가 표시/숨김된다.
- SurfaceGroup 내부 서피스 간 포커스 이동은 Alt+Arrow로 한다.

### Workspace 수준
- Alt+1~9로 워크스페이스를 전환한다.
- 각 워크스페이스는 완전히 독립적인 PaneLayout, 포커스 상태를 가진다.

## UI 구현

### 사이드바
egui SidePanel로 워크스페이스 목록을 렌더링한다. 워크스페이스 이름, 활성 표시, 추가 버튼을 포함한다.

### 탭 바
글로벌 탭 바가 아닌 **Pane별 탭 바**를 렌더링한다. egui Area를 각 Pane의 rect 상단에 배치한다. 탭이 하나뿐인 Pane은 탭 바를 숨긴다.

### 키보드 단축키

| 단축키 | 동작 |
|--------|------|
| Ctrl+Shift+N | 새 워크스페이스 |
| Ctrl+Shift+T | 포커스된 Pane에 새 탭 |
| Ctrl+Tab | 다음 탭 (포커스된 Pane) |
| Ctrl+Shift+Tab | 이전 탭 (포커스된 Pane) |
| Alt+1~9 | 워크스페이스 전환 |
| Ctrl+Shift+E | Pane 수직 분할 |
| Ctrl+Shift+O | Pane 수평 분할 |
| Ctrl+D | SurfaceGroup 수직 분할 |
| Ctrl+Shift+D | SurfaceGroup 수평 분할 |
| Alt+Arrow | Pane 간 포커스 이동 |

## 구현 현황

- Workspace / PaneNode / Pane / Tab / Panel / SurfaceGroupNode 계층 데이터 모델 구현 완료
- egui 사이드바 + Pane별 탭 바 렌더링 구현 완료
- Pane 분할 (Ctrl+Shift+E/O) 구현 완료
- SurfaceGroup 분할 (Ctrl+D / Ctrl+Shift+D) 구현 완료
- 탭 전환 (Ctrl+Tab / Ctrl+Shift+Tab) 구현 완료
- Pane 간 포커스 이동 (Alt+Arrow) 구현 완료
