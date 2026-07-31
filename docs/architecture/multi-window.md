# 멀티 윈도우 아키텍처

tasty 는 **단일 프로세스 · 메인 스레드 단일 winit 이벤트 루프**로 여러 OS 윈도우(모달 포함)를 돌린다. `App`(winit `ApplicationHandler`)이 도메인·외부통신·GUI 세 부분을 합성한다.

## 구조

```
App  (1 프로세스, 메인 스레드, winit ApplicationHandler)
├── core: Core         도메인 본체 (워크스페이스/세션/attach/registries) — 항상 빌드
├── hub: Hub           외부 통신 (IPC 서버, 포트 파일) — 항상 빌드
└── view: ViewRegistry GUI 어댑터 — #[cfg(feature = "gui")]
    ├── proxy              winit EventLoopProxy<AppEvent>
    ├── views: HashMap<WindowId, Box<dyn View>>
    ├── active_modal_id: Option<WindowId>   (모달 전역 최대 1)
    └── focused_view_id: Option<WindowId>
```

모든 윈도우(모달 포함)는 단일 `views` 맵에 저장된다. 모달은 별개 엔티티가 아니라 `active_modal_id: Option<WindowId>` 로 식별되는 View 상태이며, 활성 모달은 이 필드로 식별한다. (옛 `Engine` struct 는 삭제됐고 그 역할이 Core/Hub/ViewRegistry 로 분산됐다.)

## Window 트레잇 계층 (`src/view/`)

```text
View (sealed trait, : sealed::Sealed + std::any::Any)
├── ModalView         (supertrait)
│   ├── SettingsView   (설정 모달)
│   ├── QuitView       (종료 확인 모달)
│   └── PluginsView    (plugin 매니저 모달)
└── (그 외 — View + sealed::Sealed 직접 구현)
    ├── MainView       (터미널 호스트 — 워크스페이스/패인/탭/서피스)
    └── PresetView     (프리셋 편집기, modeless)
```

- **`View`**: 공통 인터페이스. `sealed::Sealed` supertrait 으로 크레이트 외부 구현 차단. `Any` 로 downcast.
- **`ModalView`**: 모달 계열의 default 동작 marker(`shown`/`set_shown`/`reveal_after_first_render`/`on_escape`). 그 외 구현체(`MainView`/`PresetView`)는 `View` + `sealed::Sealed` 를 직접 구현한다.
- **`ViewBase`**: 모든 구현체가 `pub base: ViewBase` 로 합성하는 공통 필드(gpu·winit·dirty·modifiers·focused·close_requested).

> 용어: 여기서 "윈도우"는 winit OS-level 윈도우다. tasty 도메인 계층의 Window/Workspace/Pane/Tab/Surface 와 다르다 — [구조 계층](../concepts/hierarchy.md).

## 모달 (Modal modality)

엔진 전역 최대 1개. 설정창·종료 다이얼로그가 대표.

- Modal View 가 열리면 `active_modal_id` 설정, 닫히면 `None`(`src/app/modal.rs`).
- 이벤트 디스패처는 다른 modeless View 에 `modal_active: true` 를 전달하고, 각 View 의 입력 핸들러가 이때 입력을 차단한다(Resized/RedrawRequested/ModifiersChanged/Focused 만 허용).
- 모달도 같은 `views` 맵에 있어 단일 이벤트 디스패처가 전부 처리한다.

## parked states — PTY 생존

모든 윈도우가 닫혀도 PTY 세션을 잃지 않도록, `App.parked_states` 에 `(AppState, CoreState)` 를 보관한다. 새 윈도우 생성 시 옮겨 담거나 IPC 가 직접 쓴다. 윈도우가 0개여도 프로세스(트레이)는 살아 있을 수 있다 — [system-tray 정책](../design/policies/system-tray.md).

## 윈도우 간 GPU·통신

- 각 View 는 자체 wgpu surface + egui 컨텍스트 보유(`ViewBase.gpu`). wgpu adapter/device 는 공유.
- 윈도우 간 직접 통신 없음 — 모두 도메인(Core)·Intent 큐를 경유.

## 단일 프로세스 · 단일 스레드 근거

| 관점 | 근거 |
|------|------|
| 상태 공유 | `Arc`/`Mutex` 없이 Core 상태 직접 공유, 윈도우 간 IPC 불필요 |
| GPU 리소스 | wgpu adapter/device 윈도우 간 공유 가능 |
| winit 호환 | winit 은 프로세스당 이벤트 루프 1개를 전제 |
| 크래시 격리 | 셸은 이미 별도 OS 프로세스(PTY) — 셸 크래시가 tasty 로 전파 안 됨 |
| 리소스 방어 | 스크롤백 상한 + PTY 읽기 채널 버퍼 제한 |

Chrome 의 멀티 프로세스 사유(신뢰 불가 웹 코드 보안 격리)는 tasty 에 해당하지 않는다. (plugin 은 별도 프로세스지만 sandbox 경계는 IPC 권한 게이트로 — [plugins](../concepts/plugins.md).)

## 관련

- [아키텍처 개요](index.md) — headless `gui` feature 분리
- [input-layer](input-layer.md) — 윈도우 내부 마우스 입력 계층
- [concepts/hierarchy](../concepts/hierarchy.md) — 도메인 Window/Workspace/Pane/Tab/Surface
