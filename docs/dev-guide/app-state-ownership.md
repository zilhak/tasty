# AppState 필드 소유권

`AppState`(`src/state.rs`)는 창 하나의 상태다. 필드를 다음 세 종류로 나누고,
수명·값을 쓰고 읽는 주체·헤드리스 빌드 포함 여부를 기록한다.

- **도메인 사실** — 에이전트가 IPC 로 묻고 바꾸는 구조의 일부. 창이 없어도 뜻이 있다.
- **사용자 view 상태** — 로컬 사용자가 지금 무엇을 보고 어디를 누르는지. 포커스·선택·
  popup 입력·hover 가 여기다. 불가침 원칙 1·3(`docs/identity.md`)이 지키는 대상이다.
- **실행 자원** — 다른 소유자의 핸들 사본이나, 한 계층이 넣고 다른 계층이 비우는 큐.

소유권을 struct 두 개로 가르지 않고 **컴파일 경계와 모듈 경계**로 가른 결정과 그 근거는
[ADR-0002](../adr/0002-domain-execution-and-ports.md).
"어떤 정의를 gui 전용으로 가르는가" 의 판정 규칙 자체는 [헤드리스 정의 경계](headless-build-boundaries.md)
가 정본이고, 이 문서는 그 규칙을 `AppState` 에 적용한 결과표다.

## 열 읽는 법

- **수명**: `프레임`(매 egui 프레임 다시 채운다) · `열림`(popup·메뉴·드래그가 열려 있는 동안) ·
  `요청`(설정한 쪽 다음 소비 한 번에 비워진다) · `세션`(창이 유지되는 동안) · `영속`(디스크에서
  로드된다).
- **headless**: `없음` 은 `cfg(feature = "gui")` 로 그 빌드에서 필드가 사라진 것이다.
  `읽힘` 은 headless 라이브러리 안에 그 필드를 읽는 자리가 있는 것이다. `③` 은 필드가
  headless 에도 컴파일되고 그 빌드가 값을 쓰지만 읽는 자가 GUI 뿐이라 항목 단위
  `expect(dead_code)` 를 단 것이다. `②` 는 headless 라이브러리에는 없고 테스트 구성에만 있는 것이다.
  `debug 헤드리스만 읽힘` 은 게이트가 `cfg(any(feature = "gui", debug_assertions[, test]))` 라 debug
  헤드리스에서만 컴파일되고 읽히는(주로 debug `ui.state` 덤프) 것이다 — release 헤드리스에는 필드가 없다.

빌드별 포함·사용 여부는 아래 "재는 법"의 컴파일 진단으로 확인한다.

## `dialogs` — GUI 소유 한 덩어리

`dialogs: DialogState` 는 필드 하나지만 그 안에 popup·메뉴·드래그의 입력 상태가 스무 개 넘게
있다. 자료형은 전부 `src/state/dialogs.rs` 한 파일에 있고, 그 모듈과 `dialogs` 필드는
`cfg(feature = "gui")` 다. **headless 빌드에는 이 상태가 아예 없다.**

모두 다음 분류를 따른다: 사용자 view 상태 · 수명 `열림` 또는 `요청` · 설정하는 쪽과 비우는 쪽이
모두 GUI(popup `draw_fn`·`on_close`, 사이드바·탭바, 메인 루프). 에이전트가 세우는 것처럼 보이는
자리도 비우는 쪽은 GUI 다:

| 필드 | 에이전트 쪽 입구 | 실제 도메인 데이터의 저장 위치 |
|---|---|---|
| `pending_approval_ids` | `approval.request` · capability elevation 이 `enqueue_approval` 로 push | approval 레코드는 `Core` 의 approval 저장소가 갖는다. 이 큐는 그중 **popup 이 보여줄 순서**다 |
| `file_picker` | `file_picker.trigger`(그 핸들러 모듈이 gui 전용) | 결과는 plugin 에 이벤트로 나간다(ADR-0036) |
| `pending_open_preset_window` · `pending_preset_window_selection` | 사용자 origin 의 preset 저장만 세운다 | 저장된 preset 은 `PresetStore` 에 있다 |

그래서 headless 가 이 상태를 제외해도 승인 등 도메인 데이터는 유지된다. approval 을 묻는 IPC
(`approval.list` 류)는 저장소를 읽는다.

두 판정은 dialog 가 없어도 debug 빌드의 두 조합에 남는다(release 헤드리스에는 없다) —
`has_input_dialog_open` 은 debug 헤드리스에서 `false` 를 답하고, 그 값을 쓰는 `keyboard_overlay_open`
도 그대로다. `ui.state` debug 덤프가 두 값을 두 조합에서 같은 키로 찍기 때문이다.

## 나머지 필드

| 필드 | 분류 | 수명 | 설정하는 쪽 → 비우는 쪽 | headless |
|---|---|---|---|---|
| `active_workspace` | 사용자 view 상태 | 세션 | 사용자 전환 → — | 읽힘 (대상 생략 시 기본값) |
| `category_last_active` | 사용자 view 상태 | 세션 | 사용자 전환 → — | debug 헤드리스만 읽힘(release 에는 필드 없음) |
| `settings_open_requested` · `plugins_open` | 사용자 view 상태 | 요청 | 사이드바 버튼 → 다음 프레임 `dispatch_pending_modal_opens` | 앞은 debug 헤드리스만 읽힘(`ui.state`, release 에는 필드 없음), 뒤는 없음 |
| `active_modal_id` · `active_modal_kind` | 사용자 view 상태 | 열림 | `App::open_modal` → 모달 닫힘 | debug 헤드리스만 읽힘(release 에는 필드 없음) |
| `sidebar_width` · `sidebar_visible` · `sidebar_collapsed` | 사용자 view 상태 | 세션 | 설정·사용자 토글 → — | 없음 |
| `pending_resize_cursor` · `switch_overlay` · `modifier_hint` · `tutorial` | 사용자 view 상태 | 프레임·열림 | GUI 입력 → GUI | 없음 |
| `dialogs` | 사용자 view 상태 | 열림·요청 | 위 절 | 없음 |
| `tab_bar_height` | 사용자 view 상태 | 프레임 | 탭바 실측 → — | 없음 (테스트 구성에는 있다 — ②) |
| `popups` · `toasts` · `banners` | 사용자 view 상태 | 열림 | GUI → GUI | 없음 |
| `fullscreen_stage` · `stage_closed_queue` · `stage_deferred_grid_resync` | 사용자 view 상태 | 열림 | GUI → GUI | 없음 |
| `search` | 사용자 view 상태 | 열림 | 검색 바 → 검색 바 | 없음 |
| `port_scan` · `port_favorites_scan` | 사용자 view 상태 | 열림 | port scanner popup → popup | 없음 |
| `command_palette` | 사용자 view 상태 | 열림 | 팔레트 → 팔레트 | 없음 |
| `recent_files` | 도메인 사실 | 영속 | 디스크 로드·파일 열기 → — | 읽힘 |
| `popup_hovered` · `banner_hovered` · `modifier_hint_hovered` · `resize_edge_widget_hovered` | 사용자 view 상태 | 프레임 | egui 패스 → 입력 라우팅 | 없음 |
| `plugin_popup_open` | 사용자 view 상태 | 프레임 | plugin popup 그리기 → 입력 라우팅 | debug 헤드리스만 읽힘(`ui.state`, release 에는 필드 없음) |
| `popup_layers` · `plugin_popup_layers` · `host_popup_hittest` · `popup_escape_owner` · `plugin_popup_hittest` · `banner_layer` · `modifier_hint_layer` | 사용자 view 상태 | 프레임 | egui 패스 → 입력 라우팅 | 없음 |
| `preset_store` · `memory` | 실행 자원 (Core 소유 Arc 의 사본) | 세션 | Core → — | `memory` 는 읽힘, `preset_store` 는 ③(사본을 받지만 읽는 자가 GUI 뿐 — `expect`) |
| `pending_lifecycle_events` | 실행 자원 (큐) | 요청 | close cascade → 메인 루프가 plugin 에 통지 | 읽힘 |
| `pending_host_events` | 실행 자원 (큐) | 요청 | `enqueue_host_event` → Event Bus 이벤트 발행 | 읽힘 (headless drain) |
| `last_focused_surface_id` · `last_active_workspace_id` · `last_focused_tab` · `last_tab_locations` | 실행 자원 (변화 감지 기준값) | 세션 | GUI tick 의 감지 → 같은 자리 | 없음 |
| `explorer_views` · `dag_graph_views` | 사용자 view 상태 | 열림 (surface 수명) | surface 그리기 → surface 닫힘 | 없음 |
| `tool_registry` · `palette_plugin_commands` | 도메인 사실의 사본 (plugin 기여 목록) | 세션 | plugin 활성 → 재계산 | 없음 |
| `pending_plugin_command_invokes` · `pending_tool_events` · `pending_popup_opens` | 실행 자원 (큐) | 요청 | 도구 메뉴·팔레트 → 메인 루프 | 없음 |
| `pending_handler_ipc` | 실행 자원 (큐) | 요청 | 파일 핸들러 dispatch → 메인 루프 | 없음 (넣는 자리 `file::dispatch` 의 핸들러 실행도 GUI 다) |
| `drop_hover` · `pending_file_drops` | 사용자 view 상태 | 열림·요청 | OS drag&drop → frame end | 없음 |
| `plugin_popup_closes` · `plugin_popup_focus_bumps` · `plugin_banner_closes` | 실행 자원 (큐) | 요청 | egui 패스 → 메인 루프가 plugin 에 통지 | 없음 |
| `plugin_mesh_popup_regions` · `plugin_popup_ime_cursor_area` | 사용자 view 상태 | 프레임 | egui 패스 → 합성·IME | 없음 |
| `plugin_mesh_popup_forward` · `plugin_mesh_banner_forward` | 사용자 view 상태 | 열림 | egui 패스 → 합성 | 없음 |
| `plugin_mesh_banner_regions` | 사용자 view 상태 | 프레임 | egui 패스 → 합성 | 없음 |
| `plugin_mesh_popup_pending_repaint` · `plugin_mesh_banner_pending_repaint` | 사용자 view 상태 | 요청 | plugin repaint 요청 → 합성 | 없음 |
| `plugin_popup_user_activated` | 실행 자원 (사용자 행동 근거) | 열림 | `draw_plugin_popups` 입력 forward → popup 닫힘 | 없음 |
| `webview_user_navigations` | 실행 자원 (사용자 행동 근거) | 요청 | `sync_webviews` → 한 번 쓰이거나 webview 소멸 | 없음 |
| `pending_intents` | 실행 자원 (큐) | 요청 | GUI 의 `dispatch_intent` · IPC 진입점이 옮기는 요청 출구 → `dispatch_pending_intents` / headless drain | 읽힘 |

## 모듈 단위 예외 없이 가른다

AppState를 별도 도메인·GUI struct로 복제하지 않는다. DialogState처럼 생산자와 소비자가 GUI뿐인 상태는 모듈과 필드 모두 gui 조건으로 제외한다. 실제 승인 레코드는 Core에 있고 popup의 pending ID 목록은 화면 상태다. 창 상태가 필요한 진입점은 AppState를 소유할 수 있지만 도메인 실행과 IPC engine 핸들러에는 좁은 port만 전달한다. CoreState에도 사용자 포커스·선택·히스토리가 있으므로 AppState 제거만으로 사용자 상태 보호가 완성되지는 않는다. intent origin 검사를 함께 유지한다.

## `state` 가 아니라 `core` 에 두는 것

구조 변경은 도메인 결과로 반환하며 전송하지 않을 JSON-RPC 응답을 만들었다 다시 해석하지 않는다. 닫힌 surface 정리는 공용 `reclaim_closed_surfaces`가, MoveSurface 결과의 close 변환은 공용 생성자가 맡는다. 자원 정리를 Core::apply에 넣어 AppState 의존을 추가하지 않는다. must_use만으로 이벤트를 분해한 뒤 정리를 빠뜨리는 문제를 막을 수는 없다.

`core::structural_exec`가 split·tab 생성/이동/닫기·pane/surface 닫기의 검증과 적용을 맡는다.
IPC와 원격 forward는 같은 실행 함수를 쓰며 `Rejected`, `MissingEvent`, `Apply` 실패를 각 전송 형식으로 변환한다.
정수 범위 같은 공용 파라미터 판정은 core::param_bag에 둔다.
권한·점유·자기 대상 제한은 진입점에 남고 anchor 해석·snapshot·즉시 tap 억제·delta 계산은 forward에 남는다.
공용 cascade의 GUI 효과만 조건부로 실행한다.
변환한 surface의 mesh 정리는 매니저를 소유한 호출자에게 결과값으로 알린다.
두 경로의 실패 문구 일치와 기존 외부 문구 보존은 서로 다른 검증이다.

핸들러는 사용하는 상태만 인자로 받는다. 쓰지 않는 `_state: AppState` 인자를 공통 호출 모양에 맞추려고 남기지 않는다. memory와 대상 nickname 해석은 Core의 공유 핸들에서 읽고, 창과 무관한 출력 조회는 CoreState에서 수행한다. GUI·debug에서 창 상태 자체를 조작하는 핸들러는 창 전용 라우터에 둔다.

IPC engine 핸들러는 `IpcWindow`와 요청별 `IntentOutbox`로 필요한 창 연산과 intent 생성을 수행한다.
진입점이 요청 완료 시 outbox를 창 큐 끝에 옮기며 게이트가 만든 intent가 핸들러 intent보다 앞선다.
`EntryWindow`는 입구 본문에 창 전체를 꺼내는 접근자를 제공하지 않는다.
창·debug 라우터는 별도 경로이며 각 창 메서드의 caller 정책을 선언·검증한다.
포트가 상속한 활성 워크스페이스 변경까지 막는 것은 아니므로 origin과 대상 정책을 별도로 유지한다.
헤드리스 pump는 응답 전 intent 적용이 AppState를 요구하는 동안 이를 소유한다.
drain이 좁은 port로 실행 가능해질 때만 pump 인자를 줄인다.

### 아직 남아 있는 동작 차이

헤드리스 close cascade는 workspace 전체 제거 때 memory scope 정리를 하지 않는다. GUI 통지와 같은 함수에 묶인 현재 한계이며, 공용 cascade를 만들었다고 이 단계까지 두 빌드에서 같아진 것은 아니다. [닫기 순서](../architecture/close-sequence.md#gui-와-headless-의-차이)를 따른다.

원격 pane split에서 전달된 params에 `target_pane`이 있고 서버가 `target_surface`도 채우면 두 대상 동시 지정 오류가 날 수 있다. 실행 함수 통합은 이 기존 동작을 바꾸지 않았다. 실패 문구의 경로 간 일치와 문구 자체의 호환은 별도로 검사한다.

## 재는 법

`없음`·`②`·`③` 칸은 headless 두 가지 검사로 확인한다. 아래는 debug 프로필이고, release 두 조합
(`--release` 를 더한 것)도 함께 돌린다 — debug 핸들러만 읽는 필드는 release headless 에서만
dead 가 된다. 여덟 조합 전체는 [헤드리스 정의 경계](headless-build-boundaries.md) "재는 법":

```bash
cargo check -p tasty --no-default-features --lib
cargo check -p tasty --no-default-features --all-targets
```

`dead_code` 는 이 크레이트에서 error 라, 새 필드가 headless 에 컴파일되고 아무도 안 읽으면 앞
검사가 그 필드를 이름으로 찍고 실패한다. `③` 의 `expect` 는 거꾸로 — headless 에 읽는 자가 생겨
진단이 사라지면 `unfulfilled_lint_expectations` 가 해당 항목을 **경고로** 알린다. 빌드·CI 를 막지는
않는다 — 그 lint 는 warn 이고 `Cargo.toml` 의 `[workspace.lints.rust]` deny 목록 밖이며, CI 는 `-D warnings` 를
안 쓴다. 그래서 이 칸의 변화는 경고 수로만 보인다. `②` 를 `cfg(feature =
"gui")` 로 좁히면 뒤 검사가 그 정의를 부르는 시험에서 실패한다.

`없음` 칸은 필드 선언 앞의 `#[cfg(feature = "gui")]` 로 읽는다.

## 관련

- [헤드리스 정의 경계](headless-build-boundaries.md) — gui 전용 판정 규칙 세 갈래와 여덟 칸
- [model-view-split](model-view-split.md) — Model 과 Host View 를 가르는 패턴
- [focus 정책](../design/policies/focus.md) — 사용자 view 상태 중 포커스의 운영 규칙
