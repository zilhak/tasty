# Debug 전용 IPC

[identity.md](../identity.md) 원칙 1 ②(사용자 입력 재현은 release 에 없다)에 따라, **사용자의 키/마우스/단축키 입력을 그대로 재현하거나 내부 렌더 상태를 덤프하는 IPC** 는 release 빌드에 노출되지 않는다. `#[cfg(debug_assertions)]` 로 격리되어 debug 빌드에서만 컴파일·등록된다.

판단 기준(원칙 1):

- **에이전트 기능 → release IPC**: surface/tab/workspace 생성·조회·닫기, 클립보드, 알림, 메타데이터 등 — 에이전트가 *자기 작업* 을 하려고 필요한 동작. 예: **`surface.completion`**(surface highlight 발동) — 에이전트가 "이 surface 확인 필요" 라고 *자기 작업 결과를 보고* 하는 것이라 PushNotification 과 동류 → **release 정식**. 반면 그 highlight 를 *포커스를 주입해* 해제하는 경로는 사용자 입력 재현이라 debug 격리 대상(현 해제는 실 렌더 포커스 기준이라 애초에 IPC 아님).
- **디버그 기능 → debug 전용**: 사용자 단축키/마우스로 트리거되는 동작의 자동 재현(키/마우스 주입, 단축키로만 여는 popup/도구 메뉴의 IPC 트리거), 그리고 렌더러·파서 검증용 저수준 덤프.

## 라우팅

JSON-RPC 라우터(`src/adapters/ipc/handler.rs::handle_with_caller`)는 권한·텔레메트리 게이트를 통과시킨 뒤 핸들러를 탐색한다:

```rust
if let Some(resp) = route_engine_handler(core, state, engine, caller, request, id.clone()) {
    return resp;                       // release+debug 공통 핸들러 (~150개)
}
#[cfg(debug_assertions)]
if let Some(resp) = route_debug_handler(state, engine, request, id.clone()) {
    return resp;                       // debug 빌드 전용
}
JsonRpcResponse::method_not_found(id, &request.method)
```

`route_debug_handler` 함수 자체가 `#[cfg(debug_assertions)]` 라 release 바이너리엔 분기 한 줄과 함수가 모두 사라진다. release 에서 debug 메서드를 부르면 `method_not_found` 로 떨어진다.

### `AppState` 하나로는 부족한 debug 메서드는 App-level 에서 분기

`debug.event_bus.*` / `debug.extension.invoke_hook` / `debug.popup.*` 는 `route_debug_handler` 를 거치지 않는다 — `AppState` 가 `PluginManager` 를 들고 있지 않기 때문이다. 이들은 `App` 레벨의 `ipc_step_debug`(`src/app/ipc/debug_methods.rs`)에서 `plugin_manager` 를 직접 호출한다.

`debug.fullscreen.*` 도 같은 곳에서 처리되지만 이유는 또 다르다 — **창을 골라야 하기 때문**이다. `route_debug_handler` 는 `AppState`(=`MainView` 하나)만 받아 다른 창을 볼 수 없는데, 전체화면 무대는 창 단위 상태라([fullscreen-stage](../design/systems/fullscreen-stage.md)) `window_id` 로 대상을 지목하지 못하면 "창 2 개에 각각 무대를 띄운 뒤 한쪽만 닫는다" 같은 시나리오 자체가 구동 불가다. `self.view.views` 순회는 App 레벨에서만 가능하다. `window_id` 해석은 `ui.screenshot` 과 동형 — 지정하면 그 창, 미지정이고 창이 하나면 그 창, 여럿이면 `-32000` 에러다. 포커스된 창으로 조용히 폴백하지 않는다([focus](../design/policies/focus.md)).

`debug.settings.open` 도 같은 `ipc_step_debug` 에서 처리되지만 이유는 또 다르다 — 설정 모달은 `AppEvent::OpenSettings`(event-loop proxy → `open_settings_modal`) 로만 열리는 **별도 winit 윈도우**라, `AppState` 핸들러가 아니라 `App` 의 `self.view.proxy` 가 필요하다(`window.create` 와 동일 패턴). 사용자 단축키/버튼 클릭과 같은 진입점을 그대로 호출하므로, 정상 모달 동작과 100% 동일하다. `tab` 인자는 `App.pending_settings_tab`(debug 전용 필드)에 1회성으로 실려 `open_settings_modal` 이 소비한다.

## 메서드 목록

debug 메서드는 모두 `local_only()` — plugin caller 는 호출 불가, CLI/network IPC(로컬)만 가능.

| method | params | 설명 |
|--------|--------|------|
| `ui.state` | `{}` | 현재 UI 상태(settings_open, popup, 활성 workspace/pane/tab 수 등) 덤프 |
| `debug.info` | `{}` | 실행 중 인스턴스 debug 정보 |
| `debug.cell_info` | `surface_id, row, col` | 셀 단위 렌더 속성(텍스트, fg/bg, bold/italic/underline …) |
| `debug.screen_attrs` | `surface_id, row` | 한 행 전체 셀 속성 |
| `debug.glyph_color` | `surface_id, row, col, bg_mode?` | GPU 렌더러가 실제로 push 하는 글리프 bg/fg RGBA (renderer 색 해석 검증) |
| `debug.feed_bytes` | `surface_id, bytes(hex)` 또는 `text` | VTE 바이트를 PTY 우회로 터미널에 직접 주입(파서/렌더 테스트) |
| `debug.inject_mouse` | `surface_id, row, col, button?, event_type?` | SGR mouse(1006) 시퀀스로 마우스 이벤트 주입 † |
| `debug.inject_key` | `surface_id, bytes(hex)` 또는 `text` | 키 이벤트 주입 † |
| `debug.selection` | `{}` | focused window 의 로컬 텍스트 선택 상태 read-only 덤프(`present`·`surface_id`·`mode`·`dragging`·`empty`·`anchor/cursor/start/end{col,row}`). 마우스 라우팅 회귀 net 의 관찰면 — 순수 관찰(사용자 상태 불변) |
| `debug.pending_menu` | `{}` | 대기 중 컨텍스트 메뉴 read-only 덤프(`present`·`kind`·`surface_id?`). live pending 우선, 없으면 주입 포획본(`debug_captured_menu`). 우클릭 라우팅 회귀 관찰용 |
| `debug.focused_surface` | `{}` | 현재 포커스된 surface id read-only 덤프(`surface_id`, 없으면 null). `surface.list` 가 노출 않는 view-layer 포커스를 관찰 — click-to-activate 라우팅 회귀 net 용 |
| `debug.switch_workspace` | `index` (0-based) | 활성 워크스페이스 전환 — 사용자 포커스 조작 재현 † |
| `debug.close_workspace` | `index` (0-based) | 워크스페이스 컨텍스트 메뉴 "Close workspace" 재현 — 워크스페이스 안 **모든** surface + closed_item 스냅샷까지 한 번에 닫는다. release `surface.close` 는 cascade 특성상 마지막 한 surface 만 정리하므로 "탭 많은 워크스페이스 통째 close" 비용([close-sequence](../architecture/close-sequence.md) `path="gui"`)은 이 메서드로만 재현된다. 마지막 workspace 는 거절(0개가 되면 다음 redraw 가 패닉) |
| `debug.switch_tab` | `index` (0-based) | 포커스 pane 의 활성 탭 전환 — 사용자 탭 클릭 재현 (egui-mesh 탭 가시성 시나리오 검증 등) † |
| `debug.tool.list` | `{}` | 도구 메뉴 항목 전체를 표시 순서대로 |
| `debug.tool.invoke` | `key` (`<plugin_id>/<tool_id>`) | 도구 항목을 사용자 클릭과 동일하게 dispatch |
| `debug.popup.list` | `{}` | contribute 된 popup 정의 + 현재 열린 instance (각 instance 에 `z_seq` — host popup 과 공유하는 전역 시퀀스라 `debug.host_popup.list` 값과 직접 비교 가능) |
| `debug.popup.open` | `plugin_id, popup_id, context?` | popup 인스턴스 강제 open (응답에 `instance_id`) |
| `debug.popup.close` | `instance_id` | popup 인스턴스 강제 close. release 경로(plugin 의 `popup.close`)와 **같은 close 큐**로 합류한다([ADR-0084](../adr/0084-plugin-triggered-host-popup-ownership.md) Decision 3) — 여기서 매니저를 직접 치면 부모-자식 연쇄 정리를 건너뛰어, 이 표면으로 하는 재현 검증이 실제 동작과 어긋난다 |
| `debug.host_popup.list` | `{}` | 호스트 빌트인 popup(`PopupDef`) 전체 목록 (id + title_key + `close_on_outside_click`). 열려 있는 항목은 `open:true` 와 함께 `z_seq`·`rect`(논리 pt) 를 노출한다 — 겹친 popup 의 마우스 소유권 판정을 좌표 실측 없이 검증하기 위한 관찰면 |
| `debug.host_popup.open` | `popup_id, workspace_scope?` | 호스트 빌트인 popup 을 focused window 중앙에 강제 open (사용자 클릭 경로 우회, 시각 검증용). `workspace_scope:true` 면 활성 workspace 스코프로 연다 — 런타임 스코프 주입(`OpenPopupMode::WithScope`)을 쓰는 popup(`dag_list`)의 가시성 게이트를 재현하려면 이 플래그가 필요하다 |
| `debug.host_popup.close` | `popup_id` | 호스트 빌트인 popup 강제 close |
| `debug.modifier_hint.hold` | `ctrl?`, `alt?`, `option?`, `shift?`, `elapsed_ms?` | modifier-hint 오버레이의 홀드 조합을 직접 세팅(생략 축=false, 모두 false 면 홀드 해제). `elapsed_ms` 는 홀드 타이머를 그만큼 과거로 백데이트해 표시 지연(500/1200ms) 게이트를 즉시 통과. 실 modifier 홀드 우회 force-state(사용자 홀드 경로 우회). 응답은 `state` 와 동일한 렌더 상태 덤프 |
| `debug.modifier_hint.state` | `{}` | 오버레이 렌더 상태를 draw 경로와 동일 로직으로 재평가해 덤프: `held{ctrl,alt,option,shift}\|null` · `hold_elapsed_ms` · `dismissed` · `reveal_delay_ms`(Shift 단독 2000, 그 외 500) · `visible` · `alpha` · `header_combo`(전체 조합 키캡) · `sections[{combo,rows,roles,empty}]`(눌린 조합으로 좁혀진 섹션 — `combo`=섹션 헤더의 조합 전체 키캡, `rows`=각 행의 **leaf 키캡만**(`Ctrl+K` 가 아니라 `K` — modifier 는 섹션 헤더가 담당), `roles`=역할 설명 키, `empty`=바인딩·역할 모두 없는 조합(draw 가 "바인딩 없음" 플레이스홀더로 렌더, ADR-0038)). 스크린샷 없이 좁힘·즉시갱신·지연·빈-플레이스홀더 자동 단정용 |
| `debug.settings.open` | `tab?`, `subtab?` | 설정 모달 강제 open (사용자 클릭/단축키 우회, 시각 검증용). `tab` = L1 `general`/`terminal`/`appearance`/`keybindings`/`file_handler`/`misc`/`plugins` (생략 시 `general`). `subtab` = 선택한 L1 의 L2 섹션 키(아래 표), 생략·미지정 키면 해당 L1 의 기본 L2 유지. `AppEvent::OpenSettings` 발화 → 별도 모달 윈도우 생성 |
| `debug.settings.apply` | `settings` (object) | 부분(또는 전체) 설정 patch 를 **라이브 settings 직렬화 복사본** 위에 재귀 deep-merge 한 뒤 완성된 전체 `Settings` 로 `UpdateSettings` 를 dispatch — 설정 모달 저장과 **동일 경로**라 collapse·theme·`config.toml` save 까지 cascade 가 처리한다(모달/proxy 불요). 라이브를 pre-mutate 하지 않으므로 cascade 의 prev≠new 비교가 살아 collapse 분기가 정상 발화. **알 수 없는 키는 조용히 무시(no-op)** — `Settings` 가 `deny_unknown_fields` 가 아니라 `#[serde(default)]` 이므로 오타 키는 변화 없이 통과한다(검증자 혼동 주의). 타입 불일치/비-object 는 `-32602` 로 거부되고 라이브는 불변. gui 게이트 없이 headless 에서도 동작 |

`debug.settings.open` 의 `subtab` 키(활성 `tab` 종속):

| `tab` | 유효 `subtab` 키 |
|-------|------------------|
| `general` | `general` · `notifications` · `accessibility` · `overlay` · `remote_transfer` · `display`(macOS 전용 UI — 키 자체는 크로스플랫폼으로 강제 선택 가능) |
| `terminal` | `general` · `mouse_capture` · `tui` · `performance` |
| `appearance` | `theme` · `colors` · `general` · `display` · `tasty` · `terminal` |
| `keybindings` | `general` · `workspace` · `pane` · `tab` · `surface` · `clipboard` · `zoom` · `image` · `preset` · `plugins` |
| `file_handler` | `extension_mapping` · `detectors` · `handlers` |
| `misc` | `tastyrc` (Windows 전용) |
| `plugins` | — (L2 가 plugin contribute page 라 정적 키 없음; 무시) |
| `debug.banner.list` | `{}` | 빌트인 배너 정의 + 현재 표시 중/큐 배너(스코프 token·남은초·`total_queued`) |
| `debug.banner.show` | `banner_id, scope` | 배너 강제 발화 (def 의 ttl 따라 ttl/persistent, 응답에 push `outcome`) — 사용자 조작 우회, 시각 검증용 |
| `debug.banner.close` | `banner_id` | 표시 중/큐 배너 강제 close (표시 중이면 큐 head 승격) |
| `debug.banner.set_countdown` | `scope, seconds` | 표시 중 TTL 배너의 카운트다운 조절 |
| `debug.event_bus.list_subscribers` | `key` | 해당 키 구독 plugin 목록 |
| `debug.event_bus.publish` | `key, payload, scope` | 임의 키로 host envelope 발화 |
| `debug.event_bus.trace` | `trace_id` | 같은 trace_id envelope 들을 발화 순서로 |
| `debug.extension.invoke_hook` | `extension_id, kind, phase, mode, target, payload` | 매니페스트 매칭 우회로 extension hook 직접 호출 (fail-open/backoff 우회) |
| `debug.fullscreen.list` | `{}` | 등록된 전체화면 무대 정의 전체 — `{"stages":[{id,title_key}]}`. 제목은 i18n **키** 그대로라 로케일에 무관하게 단정 가능 |
| `debug.fullscreen.open` | `stage_id`, `window_id?` | 무대를 창에 강제로 올린다(popup 타이틀바 전체화면 버튼 우회, 시각 검증용). 창당 하나 계약 그대로 — 다른 무대가 올라와 있으면 **교체**(닫힘 훅 발화), 같은 id 면 no-op. 정의 테이블에 없는 `stage_id` 는 창을 고르기 전에 `-32602` 로 **거부**(조용한 no-op 아님). 응답: `window_id`·`stage_id`·`previous_stage_id`·`replaced` |
| `debug.fullscreen.close` | `window_id?` | 그 창의 활성 무대를 내린다. 응답 `closed` 는 실제로 내린 무대가 있었는지(없었으면 `false`), `stage_id` 는 내려간 무대 id |
| `debug.fullscreen.state` | `window_id?` | 활성 무대 id(없으면 `null`) + 창 상태 덤프: `stage_active`·`os_fullscreen`·`maximized`·`inner_size{width,height}`·`monitor{name,position,size,scale_factor}`. **무대 상태와 OS 창 전환은 별개** — `open` 직후 `stage_id` 는 즉시 서지만 `os_fullscreen` 은 다음 프레임의 `sync_window_fullscreen` 이 반영한다 |
| `window.focus` / `view.focus` | — | 프로그래밍적 포커스 전환(사용자 단축키/마우스 영역이라 debug 전용) |

† **`debug.inject_mouse` / `debug.inject_key` 는 런타임 추가 게이트가 있다** — `--enable-input-simulation` 으로 띄운 인스턴스에서만 동작한다(`engine.input_simulation_enabled`). 안 켜져 있으면 `-32001` 로 거부.

### egui 프레임이 세우는 컨텍스트 메뉴 관찰 (`TASTY_DEBUG_SUPPRESS_NATIVE_MENU`)

`debug.inject_egui_mouse`(winit 우회, egui 입력 큐에 직접 주입 — `event_type` ∈ move/press/release, `button` 0/1/2; `surface_id` 지정 시 `(fx,fy)` 를 그 surface rect 안 정규화 좌표로 해석해 창 크기 무관하게 조준)는 explorer 그리드/컨텍스트 메뉴처럼 egui 위젯 `secondary_clicked` 로 생산되는 메뉴를 탄다. 이 메뉴는 `MainView::process_pending_native_menu` 가 실제 OS native 팝업으로 소비한다(macOS/Windows 는 **블로킹** 모달, Linux 는 비블로킹이지만 팝업이 실제로 뜨는 건 같다) — 어느 쪽이든 headless 관찰이 막힌다. `TASTY_DEBUG_SUPPRESS_NATIVE_MENU=1` 로 띄우면 그 지점에서 메뉴를 표시하지 않고 `debug_captured_menu` 로 포획만 해, `debug.pending_menu` 로 종류를 단언할 수 있다(winit 경로 `debug.inject_window_mouse` 는 핸들러가 즉시 세워 이미 포획됨 — 이 env 는 egui 경로용). GUI 테스트 하네스(`tests/gui_common`)가 이 env 를 켠다. debug 격리, release 미노출.

## CLI 노출

CLI 도 동일하게 debug 빌드에서만 등록된다 — `DebugCommands`(`crates/tasty-cli/src/commands/debug/mod.rs`)가 모듈째 `#![cfg(debug_assertions)]`. 서브커맨드: `info` · `cell-info` · `screen-attrs` · `glyph-color` · `ime-*` · `switch-input-source` · `raw-key`(주입에 macOS 손쉬운 사용 권한 필요 — 미승인이면 `surface.raw_key` 가 `permission_denied` 에러를 돌려준다. [macOS 권한](../features/macos-permissions/index.md)) · `event-bus` · `extension` · `tool` · `popup` · `host-popup` · `modifier-hint` · `banner` · `settings` · `stream-echo` · `attach`. (`settings open [--tab <name>] [--subtab <key>]` → `debug.settings.open`; `settings apply --json '<obj>'` 또는 `settings apply --file <path>` → `debug.settings.apply`. 예: `tasty debug settings apply --json '{"general":{"workspace_categories_enabled":false}}'`. JSON 파싱/파일 읽기 에러는 CLI 단에서 1차로 잡아 종료하고, 서버는 `params.get("settings")` 가 object 임을 기대한다.)

### `tasty debug attach` (JSON-RPC 메서드 아님)

로컬 loopback self-attach 의 CLI 진입점 — **debug 전용**(`commands/debug/attach.rs`). 로컬 self-attach 는 *사용자가 직접 하는 mirror 조작* 의 자동 재현 성격이라 release 표면에 없다. 원격 attach 는 release `tasty remote attach`. 둘 다 attach 세션 머신은 공용으로 보존되고, 로컬 진입점만 debug 로 격리된다. 결정 근거는 [ADR-0007](../adr/0007-attach-targets-remote.md), 메커니즘은 [attach-behavior.md](attach-behavior.md).

- 표면: `tasty debug attach [SURFACE] [--workspace <id>] [--dump-after <ms>] [--send <str>] [--send-to <sid>] [--raw] [--force-detach]`. `--ssh`/`--profile`/`--into-gui` 같은 원격 옵션은 없다(loopback 전용).
- stream 핸드셰이크 + framed 교환이라 JSON-RPC "메서드 목록" 표/`DEBUG_METHODS` 에 없다. force-detach 자체는 release JSON-RPC(`attach.force_detach`).

### `tasty debug stream-echo` (JSON-RPC 메서드 아님)

스트리밍 채널(`stream.open` 승격)의 server→client push 경로를 end-to-end 검증하는 CLI 전용 명령. raw framed 교환이라 메서드 표에 없다. *입력 재현이 아니라 transport 인프라 검증* 이지만, 사용자 검증 보조라 debug 표면에 둔다.

## 메서드 메타 등록

debug 메서드의 메타(`local_only()`)는 `crates/tasty-ipc/src/method_meta.rs::DEBUG_METHODS` 에 등록한다 (`METHOD_TABLE` 아님). 이 상수는 `#[cfg(debug_assertions)]` 일 때만 항목을 갖고, release 에선 **빈 슬라이스**로 컴파일된다.

새 debug IPC 추가 절차:

1. 핸들러를 `#[cfg(debug_assertions)]` (필요시 `+ feature = "gui"`) 로 감싼다.
2. `route_debug_handler` match 에 분기 추가 (PluginManager 필요하면 `ipc_step_debug`).
3. `DEBUG_METHODS` 표에 `(method, local_only())` 등록. 빠뜨리면 `tests/ipc_router_table_parity.rs` 가 잡는다 — 미등재도 plugin 호출자에겐 거부지만(`UnknownMethod`), 표만 봐서는 정책인지 누락인지 구분이 안 되므로 등재는 선택이 아니다([api-conventions](api-conventions.md) "권한 표 등재").
4. CLI 진입이 필요하면 `DebugCommands` variant 추가.
5. 본 문서 표에 추가. **`docs/` 의 에이전트용 release 문서에는 쓰지 않는다.**

## 디버그 코드 격리 정책 (필수)

기준 한 줄: *"이 코드를 통째로 지우고 컴파일 에러 몇 줄만 정리하면 디버그 기능이 깨끗이 사라지는가?"* 그게 되면 격리 OK.

- **debug 핸들러는 별도 파일에 모은다** — `src/adapters/ipc/handler/` 의 `debug.rs`(cell/screen/glyph/feed/inject) · `debug_plugin.rs`(event_bus/extension) · `tool.rs` · `popup.rs` 가 각각 `#[cfg(...)]` 로 모듈 선언된다. 일반 핸들러 파일(`pane.rs`, `surface.rs` 등) 중간에 `#[cfg(debug_assertions)] fn debug_xxx()` 를 끼우지 않는다.
  - **예외 — gui 게이트 없는 debug 핸들러**: `debug.rs` 모듈은 `#[cfg(all(debug_assertions, feature = "gui"))]` 로 선언돼 headless 빌드에서 통째로 사라진다. 따라서 **gui 무관하게 headless 에서도 동작해야 하는 비-gui debug 핸들러**(`ui.state` 의 `handle_ui_state`, `debug.settings.apply` 의 `handle_debug_settings_apply`)는 `debug.rs` 가 아니라 `handler.rs` 안에 `#[cfg(debug_assertions)]` 로 직접 둔다. 삭제 가능성(핸들러 fn + route 한 줄 + `DEBUG_METHODS` 한 줄 + CLI variant)은 그대로 유지된다.
- **외부 표면에 남는 cfg 가드는 router 분기 한 줄** (위 라우팅 코드의 `#[cfg(debug_assertions)] route_debug_handler(...)`).
- **삭제 가능성 테스트**: debug 파일을 지웠을 때 cfg-guard 호출처 몇 줄 제거 외에 다른 변경이 필요하면 격리가 깨진 것이다.

### 예외: 데이터 구조의 dev-only 필드

매니페스트나 빌트인 spec 처럼 **데이터 구조의 필드 하나만 dev 전용**인 경우는 분리 대상이 아니다(예: `BuiltinSpec` 의 `#[cfg(debug_assertions)] crate_dir`). *디버그 동작* 이 아니라 *dev 빌드 데이터 차이* 라 같은 룰을 적용하면 구조가 찢어진다.

## 관련

- [identity.md](../identity.md) — 원칙 1 ②(사용자 입력 재현 격리), 포커스 독립성
- [ADR-0007](../adr/0007-attach-targets-remote.md) — 로컬 attach 를 debug 로 격리한 결정
- [attach-behavior.md](attach-behavior.md) — attach 메커니즘
- [independent-verification.md](independent-verification.md) — debug IPC 를 쓴 자체 검증
