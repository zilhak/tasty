# 구조 계층 (Structural hierarchy)

tasty 화면 구조는 객체 계층 하나와, 그 위의 **두 레벨 레이아웃** 으로 이뤄진다. 모든 윈도우·surface 기능 문서가 이 용어를 쓴다. (화면을 보는 *주체* 는 [actors.md](actors.md).)

## 객체 계층

```
Engine                  진입점 + 서버. IPC 포트 소유, 모든 Window 생명주기 관리.
└── Window / View       OS frame(Window, winit) ↔ render target(View), 1:1.
    └── MainView        터미널 계열 View. 사이드바 + 워크스페이스를 호스팅. (= "터미널 윈도우")
        └── Workspace   MainView 의 최상위 컨테이너. 여러 개 가능, 사이드바에서 전환.
            └── Pane    독립 탭 바를 가진 영역. **상위 레이아웃**이 위치 결정.
                └── Tab        Pane 안의 탭 하나. **하위 레이아웃**(Surface 배치)을 가짐.
                    └── Surface   최하위 컨테이너. 타입(Terminal/Markdown/…)을 가짐.
```

- **Engine** — 진입점이자 서버. IPC 포트를 소유하고 모든 윈도우의 생명주기를 관리한다. headless 에서도 Engine 은 동작한다 — View 가 없을 뿐.
- **Window vs View** — Window = OS-level frame (winit `WindowId`), View = 그 위의 render target. 1:1 매핑.
- **MainView** — 터미널 계열 Surface 를 호스팅하는 View. 사이드바·탭·워크스페이스 전체를 가진다. **우리가 "터미널 윈도우" 라 부르는 것.** (설정창 같은 모달 계열 View, preset 에디터 같은 에디터 계열 View 는 별도.)
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
