# 유비쿼터스 언어

Tasty 프로젝트에서 사용하는 용어 정의. 코드, 문서, IPC API 전체에서 이 용어를 일관되게 사용한다.

## 계층 구조

```
Engine
├── Modal (최대 1개)
├── Window (여러 개, 타입별)
│   └── [Terminal 타입]
│       └── Workspace
│           └── 상위 레이아웃 (탭과 무관하게 고정)
│               └── Pane Group (탭 방식으로 Pane 전환)
│                   └── Pane (= 탭 하나)
│                       └── 하위 레이아웃 (탭 전환 시 함께 전환)
│                           └── Surface (실제 터미널)
└── Popup (window/modal 내부 가상 창)
```

## 용어 정의

### Engine (엔진)

프로그램의 진입점이자 서버. IPC 포트를 소유하고, 모든 윈도우의 생명주기를 관리한다. `tasty` CLI 명령의 요청을 받아 처리하는 주체.

### Window (윈도우)

엔진이 관리하는 독립 OS 윈도우. 스레드 방식으로 동작하며, 각 윈도우는 독립적인 OS 포커스를 가진다. 윈도우는 타입을 가질 수 있다 (예: Terminal, Notification).

### Modal (모달)

설정창 등 전역적으로 최대 1개만 존재하는 특수 윈도우. 모달이 열리면 모든 윈도우의 포커스를 탈취하며, 모달을 닫아야만 다른 윈도우에 포커스가 돌아간다.

### Popup (팝업)

윈도우 또는 모달 내부에 존재하는 가상 창. 부모 윈도우의 영역을 벗어날 수 없다. `PopupManager`를 통해 관리되며, 타이틀바(중앙 제목 + 우측 닫기 버튼) + 콘텐츠 영역 구조를 가진다. 타이틀바 드래그로 이동 가능하며, 다중 팝업 시 z-order로 정렬된다. 상세 규칙은 `docs/design/popup-system.md` 참조.

### Workspace (워크스페이스)

Terminal 타입 윈도우에만 존재하는 최상위 컨테이너. 하나의 윈도우에 여러 워크스페이스를 가질 수 있으며, 사이드바에서 전환한다.

### Pane Group (페인 그룹)

여러 Pane을 **탭 방식**으로 가지는 컨테이너. 워크스페이스의 **상위 레이아웃**에 의해 화면 내 위치가 결정된다. 상위 레이아웃은 탭 전환과 무관하게 고정된다.

### Pane (페인)

Pane Group 내의 하나의 탭. 내부에 Surface들의 **하위 레이아웃**을 가진다. 탭을 전환하면 해당 Pane의 하위 레이아웃 전체가 함께 전환된다.

### Surface (서피스)

실제 터미널을 나타내는 최하위 컨테이너. 각 Surface 내에서 bash, zsh 등의 쉘 프로세스가 실행된다. PTY와 1:1로 연결된다.

## 두 레벨의 레이아웃

Tasty의 핵심 설계 특징. 기존 터미널에는 없는 구조.

### 상위 레이아웃 (Pane Group 배치)

워크스페이스 내에서 Pane Group들이 어떻게 배치되는지를 정의한다 (상하분할, 좌우분할 등). **탭을 전환해도 이 레이아웃은 변하지 않는다.**

예: 화면을 좌우로 분할하면, 왼쪽 Pane Group과 오른쪽 Pane Group은 각각 독립적으로 탭을 전환할 수 있다.

### 하위 레이아웃 (Surface 배치)

Pane 내에서 Surface들이 어떻게 배치되는지를 정의한다 (상하분할, 좌우분할 등). **탭을 전환하면 이 레이아웃도 함께 전환된다.**

예: 탭 1에서 3개의 Surface를 분할해두고 탭 2로 전환하면, 탭 2의 Surface 배치가 표시된다. 다시 탭 1로 돌아오면 원래의 3분할이 복원된다.

### 기존 터미널과의 차이

| 동작 | tmux | iTerm2 | Tasty |
|------|------|--------|-------|
| 화면 분할 | 분할은 window에 고정 | 분할은 tab에 고정 | **두 레벨 선택 가능** |
| 탭 전환 시 분할 | 분할 유지 (pane은 window 소속) | 분할 전환 (split은 tab 소속) | 상위 분할 유지 + 하위 분할 전환 |

### 용어 대응 관계

| Tasty | tmux | iTerm2 |
|-------|------|--------|
| Workspace | Session | Window |
| Pane Group | — (없음) | — (없음) |
| Pane | Window (탭) | Tab |
| Surface | Pane | Pane (split) |

Pane Group은 기존 터미널에 대응하는 개념이 없다. 이것이 Tasty의 고유한 설계.

## 코드 레벨 용어 매핑

| 유비쿼터스 언어 | 코드 (Rust) | 설명 |
|----------------|-------------|------|
| Engine | `App` (현재), 멀티윈도우 전환 시 `Engine` | 메인 프로세스 |
| Window | `Window` (winit) | OS 윈도우 |
| Workspace | `Workspace` | 최상위 컨테이너 |
| 상위 레이아웃 | `PaneNode` (이진 트리 enum: Leaf / Split) | Pane Group 배치 |
| Pane Group | `PaneNode::Leaf` → `Pane` | 탭 목록 보유 |
| Pane | `Tab` → `Panel` | 탭 하나의 내용물 |
| 하위 레이아웃 | `SurfaceGroupNode` (이진 트리 enum) | Surface 배치 |
| Surface | `Terminal` (tasty-terminal crate) | 실제 터미널 (PTY 연결) |
| Popup | egui `Window` / `Area` | 내부 가상 창 |
