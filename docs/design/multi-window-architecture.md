# 멀티 윈도우 아키텍처

> **상태: 구현 완료.** `App`이 `Engine` + `HashMap<WindowId, Box<dyn Window>>`로 구성.

## 구조

```
Engine (1개 프로세스, 메인 스레드)
├── IPC Server (TCP JSON-RPC, 단일 포트)
└── HashMap<WindowId, Box<dyn Window>>
    ├── MainView 1   (Modality: Modeless, 계열: TerminalHostView)
    ├── MainView 2
    ├── ...
    └── [선택적] SettingsView 또는 QuitView  (Modality: Modal, 엔진 전역 최대 1개)
```

모든 윈도우(모달 포함)는 단일 `windows` 맵에 저장된다. 현재 활성 모달은
`engine.active_modal_id: Option<WindowId>`로 식별한다. 모달은 별개의 엔티티가 아니라
Window의 `Modality::Modal` 변형이다.

## Window 트레잇 계층

```text
Window (sealed trait, std::any::Any supertrait)
├── ModalView         (supertrait)
│   ├── SettingsView
│   └── QuitView
└── TerminalHostView  (supertrait)
    └── MainView
```

- **Window**: 공통 인터페이스 (`base()`, `handle_event()`, `render()`, `modality()` 등).
  `sealed::Sealed`를 supertrait으로 가져 크레이트 외부에서 직접 구현 불가.
- **ModalView**: 모달 계열 default 동작 (`reveal_after_first_render`, `on_escape`).
- **TerminalHostView**: 일반 윈도우 계열 default (`has_sidebar`).
- **ViewBase**: 모든 구현체가 `pub base: ViewBase`로 composition하는 공통 필드
  (gpu, winit, dirty, modifiers, focused, close_requested).

## 엔진

프로세스의 메인 루프. 다음을 소유한다:

- **IPC 서버**: 단일 포트. CLI(`tasty` 명령)와 외부 프로그램은 엔진에 요청하고, 엔진이 해당 윈도우에 전달.
- **전역 상태**: 워크스페이스, 알림, 훅, 설정 등 윈도우 간 공유 상태 (`AppState`).
- **윈도우 생명주기**: 윈도우 생성/파괴, `active_modal_id`/`focused_window_id` 추적.
- **모달 불변식 유지**: Modal modality Window 추가 시 `active_modal_id` 설정, 종료 시 해제.

## Window 구현체

각 Window는 독립된 OS 윈도우이며, 하나의 프로세스 내에서 메인 스레드 이벤트 루프로 동작한다.

- 자체 wgpu 서피스 + egui 컨텍스트 보유 (`ViewBase.gpu`)
- wgpu adapter/device는 엔진에서 공유
- 윈도우 간 통신은 엔진을 통해 수행 (직접 통신 없음)

### MainView

`TerminalHostView` 계열의 유일한 현재 구현체. 여러 워크스페이스/패인/탭을 가지며
터미널 계열 Surface를 호스팅한다. 미래에는 `StandaloneSurfaceWindow`,
`StandaloneWorkspaceWindow` 등이 같은 계열에 추가될 수 있다.

### SettingsView / QuitView

`ModalView` 계열의 현재 구현체. 각각 설정 UI / 종료 확인 다이얼로그를 담는다.
egui 패널만 렌더하며 터미널 상태는 갖지 않는다.

## 모달 (Modal modality)

엔진 전역에 최대 1개. 설정창 / 종료 다이얼로그가 대표적.

- Modal modality Window가 열리면 엔진이 `active_modal_id`를 설정
- 이벤트 디스패처는 `ViewCtx { modal_active: true }`를 다른 Modeless Window에 전달
- 각 Window의 `handle_event` 구현체는 `ctx.modal_active == true`일 때 입력을 차단
  (Resized/RedrawRequested/ModifiersChanged/Focused만 허용)
- Modal이 닫히면 `active_modal_id`가 `None`으로 설정되어 다른 Window가 다시 입력을 받음
- Modal도 일반 Window와 같은 `windows` HashMap에 저장되므로 단일 이벤트 디스패처가
  모든 Window를 처리할 수 있다

## 프로세스/스레드 선택 근거

멀티 프로세스가 아닌 메인 스레드 단일 이벤트 루프를 채택한다.

| 관점 | 근거 |
|------|------|
| 상태 공유 | `Arc`/`Mutex` 없이 엔진 상태 직접 공유. IPC 불필요 |
| GPU 리소스 | wgpu adapter/device를 윈도우 간 공유 가능 |
| winit 호환 | winit은 프로세스당 하나의 이벤트 루프를 전제로 설계됨 |
| 크래시 격리 | PTY 프로세스(쉘)는 이미 별도 OS 프로세스. 쉘 크래시가 Tasty에 전파되지 않음 |
| 리소스 폭주 방어 | 스크롤백 버퍼 상한 + PTY 읽기 채널 버퍼 제한으로 방어 |

Chrome이 멀티 프로세스인 이유(신뢰할 수 없는 웹 코드의 보안 격리)는 Tasty에 해당하지 않는다.
