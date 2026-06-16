# 유비쿼터스 언어

Tasty 프로젝트에서 사용하는 용어 정의. 코드, 문서, IPC API 전체에서 이 용어를 일관되게 사용한다.

## 주체 (Actors)

tasty 는 같은 인스턴스를 여러 주체가 **동시에** 사용하는 것을 전제로 설계된다 (→ [identity.md](../identity.md) 동시성). 주체는 두 부류 — *사람(사용자)* 과 *에이전트* — 로 나뉘고, 사람은 다시 로컬/원격으로 나뉜다. 이 세 주체가 tasty 의 거의 모든 격리·포커스·표면 규칙의 전제다.

### 로컬 사용자 (Local user)

이 머신에서 tasty GUI 를 직접 쓰는 사람. 입력 표면 = 키보드 단축키·마우스·OS 네이티브 입력. **포커스의 주인** (→ 포커스 독립성). 한 인스턴스에 보통 1명.

### AI Agent (에이전트)

자기 작업을 수행하기 위해 tasty 를 조작하는 AI. 입력 표면 = IPC 메서드 / CLI 서브커맨드. 한 인스턴스에 **여럿** 이 동시에 동작하며, 서로 그리고 사용자와 격리된다 — 자기 행동의 부수효과가 사용자 상태(포커스/닫은 항목 히스토리/선택)에 닿지 않는다.

### 원격 접속 사용자 (Remote user)

SSH 너머에서 surface/workspace 를 **client-side mirror** 로 attach 해 쓰는 사람. *사람* 이므로 행동 분류는 로컬 사용자와 같다(사용자 행동). tasty 는 자체 원격 프로토콜이 없고 SSH 에 위임한다 — attach/remote/mirror 의 정의는 아래 [Attach / Remote / Mirror](#attach--remote--mirror), 메커니즘은 [`../dev-guide/attach-behavior.md`](../dev-guide/attach-behavior.md).

### 행동 축 — 사용자 행동 ↔ 에이전트 행동

| | 로컬 사용자 | 원격 접속 사용자 | AI Agent |
|---|---|---|---|
| 부류 | 사람 | 사람 | 에이전트 |
| 입력 표면 | 키보드/마우스/OS | (mirror 위) 키보드/마우스 | IPC / CLI |
| 행동 분류 | **사용자 행동** | **사용자 행동** | **에이전트 행동** |
| 동시 수 | 보통 1 | 0..N | 0..N |

이 "사용자 행동 ↔ 에이전트 행동" 분리가 tasty 의 soul 이며, 모든 API 설계가 그 위에 얹힌다 (→ [identity.md](../identity.md) §2.1).

## 계층 구조

```
Engine
├── View (여러 개. winit Window 와 1:1 매핑된 *render target*)
│   ├── Modality: Modal  (전역 최대 1개)
│   │   ├── SettingsView
│   │   ├── QuitView
│   │   └── PluginsView
│   │
│   ├── Modality: Modeless / TerminalHostView 계열 (터미널 Surface 호스팅)
│   │   └── MainView  (현재 유일한 구현체)
│   │       └── Workspace
│   │           └── 상위 레이아웃 (탭과 무관하게 고정)
│   │               └── Pane (독립적인 탭 바)
│   │                   └── Tab (= 탭 하나)
│   │                       └── 하위 레이아웃 (탭 전환 시 함께 전환)
│   │                           └── Surface (Terminal / Markdown / Image / Html / Empty / plugin RemoteSurface)
│   │
│   └── Modality: Modeless / EditorView 계열 (modeless 에디터)
│       └── PresetView
│
├── Popup (view 내부 가상 창)
└── Toast (view 내부 휘발성 알림)
```

View 는 단일 상위 개념이며 **modality**(Modeless/Modal)와 **계열**(ModalView/
TerminalHostView/EditorView)을 갖는다. Modal은 별개의 엔티티가 아니라 View 의 한 형태다.

## 용어 정의

### Engine (엔진)

프로그램의 진입점이자 서버. IPC 포트를 소유하고, 모든 윈도우의 생명주기를 관리한다. `tasty` CLI 명령의 요청을 받아 처리하는 주체.

### Window (윈도우) vs View (뷰)

- **Window** — 엔진이 관리하는 독립 OS 윈도우 (winit `winit::window::Window`
  기반). 사용자에게 보이는 *OS-level frame*. 식별자는 `WindowId` (winit struct).
- **View** — Tasty 의 *render target* 추상화 (`crate::view::ui::View` trait).
  현재는 OS 윈도우와 1:1 매핑되지만 Phase E (headless) / Phase F (remote attach)
  에서는 OS 윈도우 없이 존재하는 View 도 도입 예정.

코드 레벨에서:
- `Window` 어휘는 *winit OS-level concept* 전용 (`winit::window::Window`,
  `winit::event::WindowEvent`, `WindowId`).
- Tasty 의 trait/struct 이름은 모두 *View* — `View` trait, `ViewBase` struct,
  `ViewAction` enum, `ViewCtx` struct, `ModalView` / `TerminalHostView` /
  `EditorView` 의 3 supertrait.

모든 View 구현체는 공통 필드 (gpu, winit, dirty, modifiers, focused,
close_requested) 를 담은 `ViewBase`를 composition 하며, **modality** 와
**계열** 을 속성으로 갖는다.

- **Modality**: `Modeless` 또는 `Modal`. 한 엔진에서 `Modal` modality 를 가진
  View 는 최대 1개.
- **계열 (supertrait)**: `ModalView` (모달 전용 공통 동작) / `TerminalHostView`
  (터미널 계열 Surface 를 호스팅하는 일반 윈도우) / `EditorView` (modeless
  에디터 윈도우).

구현 레벨에서 `View` 는 sealed trait 이며, 외부에서 직접 구현할 수 없다. 반드시
`ModalView` / `TerminalHostView` / `EditorView` 중 하나를 거쳐야 한다.

**Phase E.C 완료 상태**: 구현체 struct 이름은 모두 `*View` (`MainView`,
`SettingsView`, `QuitView`, `PresetView`, `PluginsView`). View trait 와 구현체
어휘 일치.

### Modal modality / ModalView (모달)

설정창·종료 다이얼로그 등 전역적으로 최대 1개만 존재하는 View 의 특수 상태.
Modal modality 를 가진 View (= `ModalView` 구현체) 가 활성화되면 다른 모든
View 의 입력이 차단되며, 닫아야만 다시 입력이 재개된다.

- 구현체: `SettingsView`, `QuitView`, `PluginsView`
- 공통 default 동작: 첫 프레임 렌더 후 가시화, Esc 로 닫기, 포커스 탈취

Modal 은 별개의 엔티티가 아니라 View 의 한 형태라는 점에 주의한다.

### TerminalHostView (터미널 호스트 뷰)

터미널 계열 Surface (Terminal / Markdown / Explorer / Html / Empty) 를 호스팅
하는 View 계열. Modal 이 아닌 *터미널 계열* 윈도우가 여기에 속한다.

- 현재 구현체: `MainView` (워크스페이스/사이드바/탭 전체를 가진 메인 View)
- 미래 구현체 (계획): `StandaloneSurfaceView` (독립 Surface 하나만 가진 View),
  `StandaloneWorkspaceView` (워크스페이스 1개 고정).

### EditorView (에디터 뷰)

Modeless 에디터 계열 View. modal 입력 차단 / Esc auto-close 가 없는 별 윈도우.

- 현재 구현체: `PresetView` (workspace/tab/pane preset 편집기)
- 미래 구현체 (계획): 키바인딩 에디터, 테마 에디터 등.

### Popup (팝업)

View 내부에 존재하는 가상 창. Modal/Modeless modality와 무관하게 모든 View 가
팝업을 가질 수 있다. `PopupManager`를 통해 관리되며, 타이틀바(중앙 제목 + 우측 닫기 버튼) + 콘텐츠 영역 구조를 가진다. 타이틀바 드래그로 이동 가능하며, 다중 팝업 시 z-order로 정렬된다. 팝업은 **스코프(scope)**를 가지며, 스코프에 따라 가시성 규칙과 경계 제약이 결정된다: View(항상 보임, view 경계), Workspace(해당 워크스페이스 활성 시), Pane(해당 페인 영역 내), Tab(해당 탭 활성 시), Surface(해당 서피스 영역 내). 팝업의 포커스 정책은 기본(바깥 클릭 시 언포커스)과 **고정(sticky, 닫기 전까지 키보드 포커스 유지)**의 두 가지가 있다. 상세 규칙은 `docs/design/systems/popup.md` 참조.

### Toast (토스트)

View 내부에 짧게 떠올랐다가 자동으로 사라지는 휘발성 알림. 사용자의 동작(복사 등)에 대한 즉각 피드백을 제공한다. Popup과 달리 **포커스를 받지 않으며 입력 이벤트를 소비하지 않는다.** 타이틀바·닫기 버튼 없이 본문만 표시되며 일정 시간 후 자동 소멸한다. 스코프(View/Workspace/Pane/Surface)는 떠오를 위치 앵커 용도이며, 같은 스코프 내에서 새 토스트는 아래에서 위로 쌓인다. **사용자 행동에서만 발사**되며 CLI/IPC를 통한 에이전트 동작은 토스트를 발사하지 않는다. 상세 규칙은 `docs/design/systems/toast.md` 참조.

### Workspace (워크스페이스)

`MainView`에만 존재하는 최상위 컨테이너. 하나의 MainView에 여러 워크스페이스를
가질 수 있으며, 사이드바에서 전환한다. 미래의 `StandaloneWorkspaceView`는 정확히
1개의 워크스페이스를 고정 보유한다.

### Pane (페인)

**독립적인 탭 바**를 가진 화면 영역. 여러 Tab을 탭 방식으로 전환할 수 있다. 워크스페이스의 **상위 레이아웃**에 의해 화면 내 위치가 결정된다. 상위 레이아웃은 탭 전환과 무관하게 고정된다. 기존 터미널에 대응하는 개념이 없는 Tasty 고유의 설계.

### Tab (탭)

Pane 내의 하나의 탭. 내부에 Surface들의 **하위 레이아웃**을 가진다. 탭을 전환하면 해당 Tab의 하위 레이아웃 전체가 함께 전환된다.

### Surface (서피스)

Tab 내의 최하위 컨테이너. **타입**을 가지며, 타입에 따라 콘텐츠가 달라진다. 모든 Surface는 고유한 `surface_id`를 가지며, 닫기/포커스/리스트 등의 동작이 타입에 관계없이 동일하게 적용된다.

| 타입 | 출처 | 설명 | 렌더링 |
|------|------|------|--------|
| Terminal | host built-in | 쉘 세션 (bash, zsh 등). PTY와 연결 | GPU 셰이더 |
| Markdown | host built-in | 마크다운 파일 뷰어 | egui |
| Image | host built-in (kind 는 `com.tasty.image` plugin 이 contribute) | 이미지 뷰어/편집기 | egui + 텍스처 |
| Html | host built-in | HTML/웹 뷰어 | 네이티브 WebView |
| Empty | host built-in | 빈 surface. 타입 전환 버튼 표시 | egui |
| Explorer / 기타 | plugin contribute | plugin 이 `[[contributes.surface_kinds]]` 로 등록한 kind. host 에는 `RemoteSurface` 로 보관 | egui (plugin UI DSL) |

Terminal은 기본 Surface 타입이며 PTY(가상 터미널)와 연결된다. plugin contribute 한 surface (예: explorer) 는 plugin 프로세스가 UI tree DSL 로 정의하고 host 가 그것을 egui 로 렌더링한다.

## Attach / Remote / Mirror

한 인스턴스의 surface/workspace 를 다른 client 가 점유해 실시간 입출력하는 기능군의 용어. 정확한 동작 명세는 [dev-guide/attach-behavior.md](../dev-guide/attach-behavior.md).

### Attach (어태치)

한 인스턴스(**server**)의 터미널 surface 또는 workspace 를 다른 인스턴스/CLI(**client**)가 **배타 점유**해 입출력을 잇는 동작. 점유 중 server 의 로컬 입력은 차단되고 client 입력만 PTY 에 도달한다. detach(연결 종료/force-detach)하면 lock 이 free 환원되지만 **server 의 PTY 세션은 생존**한다(server-owns-PTY persistence).

### Server / Client (attach 문맥)

- **Server** — 점유당하는 쪽. PTY/grid 의 권위 owner. **transport 를 모르고 항상 `127.0.0.1` 로만 client 를 받는다.** 로컬이든 SSH 너머든 server 입장에선 전부 loopback 접속이다.
- **Client** — 점유하는 쪽. "원격성" 을 전부 흡수한다. 로컬 client 는 포트 파일로 loopback 직결(debug 전용 `tasty debug attach`), 원격 client 는 `ssh -L` 터널을 세운 뒤 그 터널의 localport 로 직결(release `tasty remote attach`).

> **핵심**: "로컬/원격" 은 server 의 속성이 아니라 **client 측 개념**이다. 따라서 "로컬 attach 제거" 는 server 가 아니라 client 의 로컬 진입점만 제거한 것이다.

### Remote (리모트)

attach 의 *클라이언트가 SSH 너머에 있는* 경우. tasty 는 자체 원격 프로토콜을 만들지 않고 시스템 ssh 에 위임한다 — 원격 = "loopback 을 SSH 터널로 잇는 것". CLI 표면은 `tasty remote attach`(원격 attach) / `tasty remote check`(원격 생존 확인). `remote` 는 IPC namespace 가 아니라 `attach.*` 위의 CLI 디스패치 계층이다.

### Mirror (미러)

client 가 받은 원격 출력 바이트를 PTY 없는 `Terminal::new_detached` 에 먹여 server 와 같은 grid 를 재구성한 복제 화면. GUI mirror 는 원격 워크스페이스를 로컬 GUI 에 일반 워크스페이스로 재구성해 띄운 것(사이드바에서 하늘색 dot 으로 구분).

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

## Layout Preset

워크스페이스 / 탭 / 페인 레이아웃의 정적 스냅샷. 새 인스턴스 생성을 위한 템플릿.

### 종류
- **WorkspacePreset**: 워크스페이스 1개 — 상위 레이아웃 + 모든 leaf surface 정의
- **TabPreset**: 탭 1개 — 이름 + 하위 레이아웃 + 각 surface
- **PanePreset**: 페인 1개 — 탭 목록 + 활성 탭

세 종류 모두 `LayoutPreset` trait 를 구현하며 `tasty-presets` 크레이트에 정의된다.

### Preset 과 ClosedItem 의 차이

| 측면 | ClosedItem | LayoutPreset |
|------|-----------|--------------|
| 목적 | 닫힌 항목을 그대로 복원 (Ctrl+Shift+T) | 미리 정의한 템플릿으로부터 신규 생성 |
| 보관 | 인메모리 LIFO 스택 (최대 10개) | 디스크 디렉토리 영구 (수 제한 없음) |
| 데이터 | 스크롤백, screen, restore command 포함 | 구조 + 시작점만 (screen X) |
| 사용 빈도 | 1회성 (복원 후 소비) | 반복 사용 |
| 트리거 | 단축키 / 자동 (close 시) | 단축키 + 우클릭 메뉴 + IPC/CLI |

### PresetView / EditorView

`EditorView` 는 View 의 세 번째 supertrait 계열. ModalView / TerminalHostView 와 동등하지만 modeless 이면서 종류별 1개 인스턴스로 제한된다.

| Supertrait | Modality | 인스턴스 | 예 |
|------------|----------|---------|-----|
| ModalView | Modal | 전역 1개 | SettingsView, QuitView, PluginsView |
| TerminalHostView | Modeless | 다중 | MainView |
| EditorView | Modeless | 종류별 1개 | PresetView |

PresetView 는 사용자가 preset 을 편집하는 EditorView. 엔진 전역에 최대 1개 — 두 번째 열기 요청은 기존 view 포커스 이동.

## 코드 레벨 용어 매핑

| 유비쿼터스 언어 | 코드 (Rust) | 설명 |
|----------------|-------------|------|
| Engine | `App` + `engine::Engine` | 메인 프로세스, IPC/View 생명주기 관리 |
| View (상위 개념) | `view::ui::View` sealed trait | 모든 View 의 공통 인터페이스 |
| View 공통 필드 | `view::ViewBase` struct | gpu, winit, dirty, modifiers, focused, close_requested. 각 View 구현체가 `pub base: ViewBase`로 composition |
| ModalView 계열 | `view::ModalView: View` supertrait | Esc 닫기, 첫 프레임 후 reveal 등 default method |
| TerminalHostView 계열 | `view::TerminalHostView: View` supertrait | has_sidebar 등 default method |
| EditorView 계열 | `view::EditorView: View` supertrait | modeless 에디터 marker |
| Modal 구현체 | `view::SettingsView`, `view::QuitView`, `view::PluginsView` | impl View + ModalView |
| Main View 구현체 | `view::MainView` | impl View + TerminalHostView |
| Editor 구현체 | `view::PresetView` | impl View + EditorView |
| Modality | `view::Modality` enum (`Modeless`/`Modal`) | View 의 modality 속성 |
| ViewAction | `view::ViewAction` enum (`None`/`Close`/`CloseWithEvent`) | 이벤트 핸들러 반환값 |
| ViewCtx | `view::ViewCtx<'_>` struct | 이벤트 핸들러에 전달되는 맥락 (event_loop, modal_active, plugin_manager) |
| Workspace | `Workspace` | MainView 의 최상위 컨테이너 |
| 상위 레이아웃 | `PaneNode` (이진 트리 enum: Leaf / Split) | Pane 배치 |
| Pane | `Pane` | 독립적인 탭 바. 탭 목록 보유 |
| Tab | `Tab` → `SurfaceLayout` (이진 트리) | 탭 하나의 내용물. Leaf = 단일 surface, Split = 탭 내부 분할 |
| 하위 레이아웃 | `SurfaceLayout` (이진 트리 enum: Leaf / Split) | Surface 배치 |
| Surface | `Surface` trait. host built-in 구현체: `TerminalSurface`, `MarkdownPanel`, `ImagePanel`, `HtmlPanel`, `EmptySurface`. plugin 제공 surface 는 `RemoteSurface` 로 host 에 보관 | 최하위 컨테이너. 타입별 콘텐츠 |
| Popup | `PopupDef` + `PopupManager` | View 내부 가상 창 |
| Toast | `ToastState` + `ToastManager` | View 내부 휘발성 알림 |
| ViewRegistry.views | `HashMap<WindowId, Box<dyn view::ui::View>>` | 모달 포함 모든 View 단일 저장소. key 는 winit `WindowId` |
| 활성 모달 식별 | `ViewRegistry::active_modal_id: Option<WindowId>` | 최대 1개 불변식
| focused View 식별 | `ViewRegistry::focused_view_id: Option<WindowId>` | IPC/단축키의 기본 라우팅 대상
