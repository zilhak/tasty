# 유비쿼터스 언어

Tasty 프로젝트에서 사용하는 용어 정의. 코드, 문서, IPC API 전체에서 이 용어를 일관되게 사용한다.

## 계층 구조

```
Engine
├── Window (여러 개)
│   ├── Modality: Modal  (전역 최대 1개)
│   │   ├── SettingsWindow
│   │   └── QuitWindow
│   │
│   └── Modality: Modeless
│       └── TerminalHostWindow 계열 (터미널 Surface 호스팅)
│           └── MainWindow  (현재 유일한 구현체)
│               └── Workspace
│                   └── 상위 레이아웃 (탭과 무관하게 고정)
│                       └── Pane (독립적인 탭 바)
│                           └── Tab (= 탭 하나)
│                               └── 하위 레이아웃 (탭 전환 시 함께 전환)
│                                   └── Surface (Terminal / Markdown / Explorer / Html / Empty)
│
├── Popup (window 내부 가상 창)
└── Toast (window 내부 휘발성 알림)
```

Window는 단일 상위 개념이며 **modality**(Modeless/Modal)와 **계열**(ModalWindow/
TerminalHostWindow)을 갖는다. Modal은 별개의 엔티티가 아니라 Window의 한 형태다.

## 용어 정의

### Engine (엔진)

프로그램의 진입점이자 서버. IPC 포트를 소유하고, 모든 윈도우의 생명주기를 관리한다. `tasty` CLI 명령의 요청을 받아 처리하는 주체.

### Window (윈도우)

엔진이 관리하는 독립 OS 윈도우. Tasty의 최상위 UI 엔티티이며, `Window` trait
구현체로 표현된다. 모든 Window는 공통 필드(gpu, winit, dirty, modifiers, focused,
close_requested)를 담은 `WindowBase`를 composition하며, **modality**와 **계열**을
속성으로 갖는다.

- **Modality**: `Modeless` 또는 `Modal`. 한 엔진에서 `Modal` modality를 가진 Window는
  최대 1개.
- **계열(supertrait)**: `ModalWindow`(모달 전용 공통 동작) 또는
  `TerminalHostWindow`(터미널 계열 Surface를 호스팅하는 일반 윈도우).

구현 레벨에서 `Window`는 sealed trait이며, 외부에서 직접 구현할 수 없다. 반드시
`ModalWindow` 또는 `TerminalHostWindow` 중 하나를 거쳐야 한다.

### Modal modality / ModalWindow (모달)

설정창·종료 다이얼로그 등 전역적으로 최대 1개만 존재하는 Window의 특수 상태.
Modal modality를 가진 Window(= `ModalWindow` 구현체)가 활성화되면 다른 모든 Window의
입력이 차단되며, 닫아야만 다시 입력이 재개된다.

- 구현체: `SettingsWindow`, `QuitWindow`
- 공통 default 동작: 첫 프레임 렌더 후 가시화, Esc로 닫기, 포커스 탈취

Modal은 별개의 엔티티가 아니라 Window의 한 형태라는 점에 주의한다.

### TerminalHostWindow (터미널 호스트 윈도우)

터미널 계열 Surface(Terminal / Markdown / Explorer / Html / Empty)를 호스팅하는
Window 계열. Modal이 아닌 모든 일반 윈도우가 여기에 속한다.

- 현재 구현체: `MainWindow` (워크스페이스/사이드바/탭 전체를 가진 메인 윈도우)
- 미래 구현체 (계획): `StandaloneSurfaceWindow`(독립 Surface 하나만 가진 윈도우),
  `StandaloneWorkspaceWindow`(워크스페이스 1개 고정).

### Popup (팝업)

Window 내부에 존재하는 가상 창. Modal/Modeless modality와 무관하게 모든 Window가
팝업을 가질 수 있다. `PopupManager`를 통해 관리되며, 타이틀바(중앙 제목 + 우측 닫기 버튼) + 콘텐츠 영역 구조를 가진다. 타이틀바 드래그로 이동 가능하며, 다중 팝업 시 z-order로 정렬된다. 팝업은 **스코프(scope)**를 가지며, 스코프에 따라 가시성 규칙과 경계 제약이 결정된다: Window(항상 보임, 윈도우 경계), Workspace(해당 워크스페이스 활성 시), Pane(해당 페인 영역 내), Tab(해당 탭 활성 시), Surface(해당 서피스 영역 내). 상세 규칙은 `docs/design/popup-system.md` 참조.

### Toast (토스트)

Window 내부에 짧게 떠올랐다가 자동으로 사라지는 휘발성 알림. 사용자의 동작(복사 등)에 대한 즉각 피드백을 제공한다. Popup과 달리 **포커스를 받지 않으며 입력 이벤트를 소비하지 않는다.** 타이틀바·닫기 버튼 없이 본문만 표시되며 일정 시간 후 자동 소멸한다. 스코프(Window/Workspace/Pane/Surface)는 떠오를 위치 앵커 용도이며, 같은 스코프 내에서 새 토스트는 아래에서 위로 쌓인다. **사용자 행동에서만 발사**되며 CLI/IPC를 통한 에이전트 동작은 토스트를 발사하지 않는다. 상세 규칙은 `docs/design/toast-system.md` 참조.

### Workspace (워크스페이스)

`MainWindow`에만 존재하는 최상위 컨테이너. 하나의 MainWindow에 여러 워크스페이스를
가질 수 있으며, 사이드바에서 전환한다. 미래의 `StandaloneWorkspaceWindow`는 정확히
1개의 워크스페이스를 고정 보유한다.

### Pane (페인)

**독립적인 탭 바**를 가진 화면 영역. 여러 Tab을 탭 방식으로 전환할 수 있다. 워크스페이스의 **상위 레이아웃**에 의해 화면 내 위치가 결정된다. 상위 레이아웃은 탭 전환과 무관하게 고정된다. 기존 터미널에 대응하는 개념이 없는 Tasty 고유의 설계.

### Tab (탭)

Pane 내의 하나의 탭. 내부에 Surface들의 **하위 레이아웃**을 가진다. 탭을 전환하면 해당 Tab의 하위 레이아웃 전체가 함께 전환된다.

### Surface (서피스)

Tab 내의 최하위 컨테이너. **타입**을 가지며, 타입에 따라 콘텐츠가 달라진다. 모든 Surface는 고유한 `surface_id`를 가지며, 닫기/포커스/리스트 등의 동작이 타입에 관계없이 동일하게 적용된다.

| 타입 | 설명 | 렌더링 |
|------|------|--------|
| Terminal | 쉘 세션 (bash, zsh 등). PTY와 연결 | GPU 셰이더 |
| Markdown | 마크다운 파일 뷰어 | egui |
| Explorer | 파일 탐색기 | egui |
| Html | HTML/웹 뷰어 | 네이티브 WebView |
| Empty | 빈 surface. 타입 전환 버튼 표시 | egui |

Terminal은 기본 Surface 타입이며 PTY(가상 터미널)와 연결된다. 다른 Surface 타입은 각각의 렌더링 방식으로 콘텐츠를 표시한다.

## 두 레벨의 레이아웃

Tasty의 핵심 설계 특징. 기존 터미널에는 없는 구조.

### 상위 레이아웃 (Pane 배치)

워크스페이스 내에서 Pane들이 어떻게 배치되는지를 정의한다 (상하분할, 좌우분할 등). **탭을 전환해도 이 레이아웃은 변하지 않는다.**

예: 화면을 좌우로 분할하면, 왼쪽 Pane과 오른쪽 Pane은 각각 독립적으로 탭을 전환할 수 있다.

### 하위 레이아웃 (Surface 배치)

Tab 내에서 Surface들이 어떻게 배치되는지를 정의한다 (상하분할, 좌우분할 등). **탭을 전환하면 이 레이아웃도 함께 전환된다.** 하위 레이아웃의 각 leaf는 임의의 Surface 타입을 가질 수 있다 (Terminal, Markdown, Explorer 등 혼합 가능).

예: Tab 1에서 3개의 Surface를 분할해두고 Tab 2로 전환하면, Tab 2의 Surface 배치가 표시된다. 다시 Tab 1로 돌아오면 원래의 3분할이 복원된다.

### 기존 터미널과의 차이

| 동작 | tmux | iTerm2 | Tasty |
|------|------|--------|-------|
| 화면 분할 | 분할은 window에 고정 | 분할은 tab에 고정 | **두 레벨 선택 가능** |
| 탭 전환 시 분할 | 분할 유지 (pane은 window 소속) | 분할 전환 (split은 tab 소속) | 상위 분할 유지 + 하위 분할 전환 |

### 용어 대응 관계

| Tasty | tmux | iTerm2 |
|-------|------|--------|
| Workspace | Session | Window |
| Pane | — (없음) | — (없음) |
| Tab | Window (탭) | Tab |
| Surface (Terminal) | Pane | Pane (split) |

Pane은 기존 터미널에 대응하는 개념이 없다. 이것이 Tasty의 고유한 설계.

## 코드 레벨 용어 매핑

| 유비쿼터스 언어 | 코드 (Rust) | 설명 |
|----------------|-------------|------|
| Engine | `App` + `engine::Engine` | 메인 프로세스, IPC/윈도우 생명주기 관리 |
| Window (상위 개념) | `window::Window` sealed trait | 모든 윈도우의 공통 인터페이스 |
| Window 공통 필드 | `window::WindowBase` struct | gpu, winit, dirty, modifiers, focused, close_requested. 각 Window 구현체가 `pub base: WindowBase`로 composition |
| ModalWindow 계열 | `window::ModalWindow: Window` supertrait | Esc 닫기, 첫 프레임 후 reveal 등 default method |
| TerminalHostWindow 계열 | `window::TerminalHostWindow: Window` supertrait | has_sidebar 등 default method |
| Modal 구현체 | `window::SettingsWindow`, `window::QuitWindow` | impl Window + ModalWindow |
| Main Window 구현체 | `window::MainWindow` | impl Window + TerminalHostWindow |
| Modality | `window::Modality` enum (`Modeless`/`Modal`) | Window의 modality 속성 |
| WindowAction | `window::WindowAction` enum (`None`/`Close`/`CloseWithEvent`) | 이벤트 핸들러 반환값 |
| Workspace | `Workspace` | MainWindow의 최상위 컨테이너 |
| 상위 레이아웃 | `PaneNode` (이진 트리 enum: Leaf / Split) | Pane 배치 |
| Pane | `Pane` | 독립적인 탭 바. 탭 목록 보유 |
| Tab | `Tab` → `Box<dyn Surface>` | 탭 하나의 내용물 |
| 하위 레이아웃 | `SurfaceGroupNode` (이진 트리 enum) | Surface 배치 |
| Surface | `Surface` trait. 구현체: `TerminalSurface`, `SurfaceGroupNode`, `MarkdownPanel`, `ExplorerPanel`, `HtmlPanel`, `EmptySurface` | 최하위 컨테이너. 타입별 콘텐츠 |
| Popup | `PopupDef` + `PopupManager` | Window 내부 가상 창 |
| Toast | `ToastState` + `ToastManager` | Window 내부 휘발성 알림 |
| App.windows | `HashMap<WindowId, Box<dyn Window>>` | 모달 포함 모든 윈도우 단일 저장소 |
| 활성 모달 식별 | `engine::Engine::active_modal_id: Option<WindowId>` | 최대 1개 불변식
