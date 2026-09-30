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
    ├── active_modal: Option<ActiveModal>   (ID·종류, 모달 전역 최대 1, 비공개)
    └── focused_view_id: Option<WindowId>
```

`focused_view_id` 는 대상 없는 IPC 요청이 떨어지는 main 창이다. 창을 등록할 때는 그 창을 사용자가 만들었을 때만 옮긴다 — 에이전트가 만든 창은 옮기지 않는다([포커스 정책](../design/policies/focus.md#에이전트가-만든-창과-포커스), [ADR-0059](../adr/0059-id-targets-and-view-owned-selection.md)).

모든 View는 하나의 `views` 맵에 보관한다. 모달도 별도 객체 집합으로 관리하지 않고 `active_modal`로 활성 View와 종류를 표시한다. 활성 모달의 원본은 이 필드 하나이며 창별 AppState에는 사본이 없다.

## Window 트레잇 계층 (`src/view/`)

```text
View (sealed trait, : sealed::Sealed + std::any::Any)
├── ModalView         (supertrait)
│   ├── SettingsView   (설정 모달)
│   ├── QuitView       (종료 확인 모달)
│   └── PluginsView    (plugin 매니저 모달)
└── (그 외 — View + sealed::Sealed 직접 구현)
    ├── MainView       (터미널 호스트 — 워크스페이스/페인/탭/서피스)
    └── PresetView     (프리셋 편집기, modeless)
```

- **`View`**: 공통 인터페이스. `sealed::Sealed` supertrait 으로 크레이트 외부 구현 차단. `Any` 로 downcast.
- **`ModalView`**: 모달 계열의 default 동작 marker(`shown`/`set_shown`/`reveal_after_first_render`/`on_escape`). 그 외 구현체(`MainView`/`PresetView`)는 `View` + `sealed::Sealed` 를 직접 구현한다.
- **`ViewBase`**: 모든 구현체가 `pub base: ViewBase` 로 합성하는 공통 필드(gpu·winit·dirty·modifiers·focused·close_requested).

> 용어: 여기서 "윈도우"는 winit OS-level 윈도우다. Workspace·Pane·Tab·Surface 도메인 계층과 구분한다 — [구조 계층](../concepts/hierarchy.md).

## 모달 (Modal modality)

엔진 전역 최대 1개. 설정창·종료 다이얼로그가 대표.

- Modal View 가 열리면 `set_active_modal` 로 ID·종류를 세우고, 닫히면 `take_active_modal` 로 비운다(`src/app/modal.rs`).
- 이벤트 디스패처는 다른 modeless View 에 `modal_active: true` 를 전달하고, 각 View 의 입력 핸들러가 이때 입력을 차단한다(Resized/RedrawRequested/ModifiersChanged/Focused 만 허용).
- 모달도 같은 `views` 맵에 있어 단일 이벤트 디스패처가 전부 처리한다.

## parked states — PTY 생존

모든 윈도우가 닫혀도 PTY 세션을 잃지 않도록, `App.parked_states` 에 `ParkedEngine` 을 보관한다. engine 과 그 id(`EngineSession`, `src/runtime/engine_session.rs`)에 창을 다시 만들 때 쓸 AppState 를 붙인 항목이다. 새 윈도우 생성 시 옮겨 담거나 IPC 가 직접 쓴다. 윈도우가 0개여도 프로세스(트레이)는 살아 있을 수 있다 — [system-tray 정책](../design/policies/system-tray.md).

parked 상태에서도 engine은 살아 있으므로 레이아웃 슬롯 점유를 유지한다.

### engine 탐색

engine은 세 자리에 있다. 창(`MainView`), `App.parked_states`, 창에 배정되기 전의 임시 `App.core_state`다. engine만 다루는 App의 순회는 `src/app/window_access.rs`의 `EngineScan`(읽기)·`EngineScanMut`(쓰기)를 거친다. 이 두 핸들은 `App::engines`/`App::engines_mut`로 얻고, 같은 함수에서 `App.core` 같은 다른 필드를 함께 빌려야 하면 `engines_mut!` 매크로로 engine 자리 필드만 빌린다. 창 View 작업(다시 그리기 표시, toast 등)이 함께 필요한 창 루프와, 창 ID로 고른 engine 접근(`DispatchSource::Main`이나 `find_main_with_*`의 결과로 MainView를 찾는 경로)은 MainView의 `core_state`를 직접 쓴다. engine 소유 구조가 바뀌면 두 타입과 `engines_mut!` 매크로, 그리고 이 창 루프와 창 ID 접근을 함께 고친다.

- 방문 순서는 창(`views` 순회 순서) → parked(보관 순서) → 임시 engine이다. 호출부가 필요한 자리만 고른다(`windowed_and_parked`, `windows_and_pending`, `primary` 등). 창 목록의 순서는 `HashMap` 순회 순서라 고정된 의미가 없다.
- 한 engine은 세 자리 중 한 곳에만 있으므로 전체 순회(`all`, `sessions`)는 각 engine을 정확히 한 번 방문한다. 전역 목록 합산(`list_global`)과 슬롯 점유 계산이 같은 자원을 두 번 세지 않는 근거다. 단위 시험은 parked·임시 자리의 순서와 1회 방문을 고정한다. 창 자리는 `MainView`를 단위 시험에서 만들 수 없어 다중 창 라우팅 E2E로 간접 확인한다.
- parked 보관과 복원은 `park`(뒤에 붙인다)·`unpark_first`(가장 먼저 보관한 항목)로 한다.
- engine 은 생성할 때 받은 `EngineId` 를 창·parked·임시 자리를 옮겨 다녀도 유지한다. 특정 parked engine 은 보관 위치가 아니라 이 id 로 찾는다(`parked_session`, `DispatchSource::Parked`, attach mirror 출력 대상). 다른 engine 을 보관하거나 꺼내 위치가 바뀌어도 같은 engine 을 가리킨다. 창에 놓인 engine 의 id 는 MainView 가 보관한다.

engine의 존재 여부는 `views`와 `parked_states`를 함께 확인한다. 창이 없다는 이유만으로 원격 attach 세션을 끊으면 안 된다. mirror 이벤트도 parked engine에 적용하며 창을 복원하면 그 상태를 표시한다([원격 세션 수명](../features/remote-attach/index.md#창-없는-상태parked에서의-세션-수명), [ADR-0061](../adr/0061-external-remote-module-and-attach-sync.md)).

### 창이 스스로 닫히는 자리 — `close_requested`

마지막 워크스페이스가 닫힌 MainView 는 `request_close()` 로 `ViewBase.close_requested` 만 세우고, App 이 그 플래그를 보고 창을 치운다(마지막 main 창이면 파킹, 아니면 은퇴). **플래그는 세운 경로 안에서 소비한다** — 세운 채 이벤트 루프로 돌아가면 워크스페이스가 빈 창이 다음 창 이벤트를 받고, 그것이 `RedrawRequested` 면 렌더 경로가 active workspace 를 묻다 죽는다.

- **창 이벤트 경로**(winit 키 · 마우스 · redraw 안의 메뉴 continuation): `App::dispatch_window_event_to_view` 가 handler 직후 그 창을 치운다.
- **창 이벤트 밖의 App 경로**(`about_to_wait` 의 webview 포워딩 키 단축키 · 네이티브 메뉴 폴링): 그 App 함수가 MainView 를 부른 뒤 같은 함수 안에서 `App::close_self_requesting_windows` 를 부른다.

렌더 경로 쪽을 "빈 워크스페이스면 건너뛴다" 로 누그러뜨리지 않는다 — 그 창은 이미 닫혀야 할 창이고, 건너뛰어도 다음 IPC·입력 핸들러가 같은 전제로 active workspace 를 묻는다. 두 번째 목록의 짝과 `request_close()` 생산자 명부는 `crates/tasty-doc-guards/tests/close_request_consumed_in_place.rs` 가 고정한다.

## 레이아웃 슬롯

창 ↔ engine ↔ **레이아웃 슬롯**은 1:1 이다. 각 `CoreState` 는 자기 슬롯 번호(`layout_slot`)를 들고, 자기 슬롯 파일에만 저장한다. 창마다 워크스페이스 목록이 독립이라는 구조적 사실이 저장소까지 이어진 형태다 — 두 창이 같은 목록을 복제하거나 서로의 저장을 덮어쓰지 않는다.

**점유는 살아있는 engine 에서 파생된다.** 별도 레지스트리도, 디스크 기록도 없다. 점유 집합은 그때그때 `views` 의 MainView 들과 `parked_states` 를 훑어 만든다 — 여기에 `App.core_state` 도 포함된다. 갓 만들어진 engine 은 `views` 에 등록되기 전까지 거기 임시로 머물기 때문에, 그 구간을 빠뜨리면 같은 슬롯이 두 번 배정된다. 따라서

- engine이 실제로 drop되면 그 슬롯은 그 순간 free 가 된다 — 해제 호출이 없으니 해제 누락도 없다.
- **parked engine 은 슬롯을 계속 쥔다.** 창이 없어도 engine 이 살아 있으므로 점유에 포함되고, 다시 창을 열 때 그 engine 이 같은 슬롯을 이어쓴다. 재배정했다면 남의 슬롯 파일을 덮어썼을 것이다.
- 프로세스가 죽으면 점유는 전부 사라진다 — 크래시가 슬롯을 영구 점유로 남기지 않는다.

결정의 근거·대안·재검토 조건은 [ADR-0059](../adr/0059-id-targets-and-view-owned-selection.md), 배정 규칙과 창 닫힘 정책의 현재 동작은 [layout-persistence](../features/layout-persistence/index.md).

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

플러그인은 별도 프로세스로 실행한다. IPC 권한 검사는 호스트 API 호출을 제한하며 OS 자원 접근 전체를 격리하는 샌드박스는 아니다([플러그인 권한](../concepts/plugins.md#권한-permissions)).

## 관련

- [아키텍처 개요](index.md) — headless `gui` feature 분리
- [input-layer](input-layer.md) — 윈도우 내부 마우스 입력 계층
- [concepts/hierarchy](../concepts/hierarchy.md) — Workspace·Pane·Tab·Surface 도메인 계층
- [ADR-0059](../adr/0059-id-targets-and-view-owned-selection.md) — 레이아웃 슬롯 점유 모델의 근거·대안
- [features/layout-persistence](../features/layout-persistence/index.md) — 슬롯 배정·저장·복원의 현재 동작
