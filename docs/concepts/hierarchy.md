# 구조 계층 (Structural hierarchy)

tasty 화면 구조는 객체 계층 하나와, 그 위의 **두 레벨 레이아웃** 으로 이뤄진다. 모든 윈도우·surface 기능 문서가 이 용어를 쓴다. (화면을 보는 *주체* 는 [actors.md](actors.md).)

## Window 과 View

- **`winit::window::Window`** — OS 가 주는 창 자원 (창틀 / 이벤트 소스 / 렌더 표면). winit `WindowId` 로 식별.
- **`View`** — tasty 쪽 윈도우 표현. 그 창의 *종류 + 콘텐츠 + 행동*(render / 이벤트 / modality)을 묶은 객체로, winit Window 를 `Arc` 로 소유한다. **1 View : 1 Window.**

> 옛 tasty `Window` trait 이 winit 의 `Window` 와 헷갈려서 **`View` 로 rename** 됐다 (`WindowBase`→`ViewBase`, `*Window`→`*View`). **tasty 쪽 `Window` trait 은 없다** — 지금 `Window` 는 winit OS 창만 가리킨다. 즉 *View = 윈도우의 tasty 쪽 용어*. 같은 맥락에서 옛 gloss **"터미널 윈도우" 도 쓰지 않는다** — 정식 명칭은 `MainView`.

## View 의 종류 (= 윈도우 종류)

`View` trait 의 구현체가 곧 윈도우 종류다. **`MainView` 도 그중 하나** — 터미널을 호스팅하는 View 다. 각 구현체는 별개 OS 윈도우(winit Window)이고, 엔진은 이들을 `HashMap<WindowId, Box<dyn View>>` 로 균일하게 관리한다.

| 구현체 | 계열 (supertrait) | 무엇 |
|--------|-------------------|------|
| **`MainView`** | `TerminalHostView` | 사이드바 + 워크스페이스를 호스팅하는 주 윈도우. 여러 개 가능. ← 이 문서가 주로 다루는 것 |
| `SettingsView` / `PluginsView` / `QuitView` | `ModalView` | 모달 윈도우 — 전역 1개, 활성 시 입력 차단 |
| `PresetView` | `EditorView` | 에디터 윈도우 — modeless |

(**Engine** = 진입점 + 서버. IPC 포트 소유, 모든 윈도우 생명주기 관리. headless 에선 View 없이 Engine 만 동작.)

## MainView 의 내부 구조

`MainView`(**View 의 일종**) 안의 containment 계층 — 이게 "구조 계층" 의 본체다:

```
MainView  (View 의 일종)
└── Workspace   최상위 컨테이너. 여러 개, 사이드바에서 전환.
    └── Pane    독립 탭 바. **상위 레이아웃**이 위치 결정 (탭 무관 고정).
        └── Tab        **하위 레이아웃**(Surface 배치)을 가짐.
            └── Surface   최하위. 타입(Terminal/Markdown/…)을 가짐.
```

- **Workspace** — MainView 의 최상위 컨테이너. 한 MainView 가 여러 워크스페이스를 갖고 사이드바에서 전환한다.
- **Pane** — 독립적인 탭 바를 가진 화면 영역. 위치는 **상위 레이아웃**으로 결정되고 탭 전환과 무관하게 고정된다. tmux/iTerm2 에 대응 개념이 없는 tasty 고유 설계.
- **Tab** — Pane 안의 탭 하나. 내부에 Surface 들의 **하위 레이아웃**을 가진다. 탭 전환 시 하위 레이아웃 전체가 함께 전환된다.
- **Surface** — Tab 안의 최하위 컨테이너. **타입**을 가지며(아래), 고유 `surface_id` 를 갖는다. 닫기/포커스/리스트 동작은 타입과 무관하게 동일하다.

## 두 레벨 레이아웃 (tasty 핵심 설계)

기존 도구는 분할 정책이 하나뿐이다 — tmux 는 분할이 window 에 고정(탭 전환해도 유지), iTerm2 는 분할이 tab 에 종속(탭 전환 시 바뀜). tasty 는 **둘 다** 제공한다:

- **상위 레이아웃 (탭 무관)** — Workspace 안에서 **Pane** 들을 배치. 탭을 전환해도 이 분할은 고정. 화면을 물리 영역으로 나눠 각 영역이 독립적으로 탭을 전환하게 한다.
- **하위 레이아웃 (탭 종속)** — Tab 안에서 **Surface** 들을 배치. 탭 전환 시 이 분할도 함께 전환. 한 탭 안에서 여러 터미널을 동시에 본다.

예: 상위 레이아웃으로 좌우 Pane 분할 — 왼쪽은 Claude Code 전용, 오른쪽은 탭 여럿(logs/build). 오른쪽 탭을 전환해도 왼쪽 Claude 는 영향 없다.

## Surface 타입

| 타입 | 출처 | 콘텐츠 | 렌더 |
|------|------|--------|------|
| Terminal | host (기본) | 쉘 세션 (PTY 연결) | GPU 셰이더 |
| Markdown | host | 마크다운 뷰어 | egui |
| Image | `com.tasty.image` plugin contribute | 이미지 뷰어/편집 | egui + 텍스처 |
| Html | host | HTML/웹 뷰어 | 네이티브 WebView |
| Empty | host | 빈 surface (타입 전환 버튼) | egui |
| Explorer / 기타 | plugin contribute | plugin 이 `[[contributes.surface_kinds]]` 로 등록, host 는 `RemoteSurface` 로 보관 | egui (plugin UI DSL) |

## 관련

- [actors.md](actors.md) — 이 구조를 사용하는 주체 (로컬/AI/원격)
- View 내부 오버레이(Popup / Toast)·모달 계열 View 는 별도 개념 (재작성 예정)
