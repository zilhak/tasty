# 데이터 흐름

입력·출력·IPC·알림·설정이 모듈 사이를 이동하는 경로를 파일과 함수로 설명한다. Intent 큐의 처리 구조는 [action-dispatch](../design/flows/action-dispatch.md)를 참고한다.

`src/boot.rs::run()`은 winit 이벤트와 `AppEvent`의 TerminalOutput·IpcReady·StreamReady를 처리한다. 이벤트에 따른 큐 처리는 `dispatch_pending_*`에서, 시간에 따른 작업은 중앙 타이머 허브에서 관리한다. GUI의 `about_to_wait`와 헤드리스의 `recv_timeout` 루프는 실행 시각이 된 키만 처리한다([타이머 허브](../dev-guide/timer-hub.md)).

---

## 1. 키보드 입력 → 터미널 → 화면

```
winit KeyEvent / Ime
  → app/event_handler.rs (ApplicationHandler::window_event)
  → view/main/ (overlay/host-egui surface 면 egui 가 먼저 소비 — input-layer)
  → view/main/keyboard.rs (handle_keyboard_input) · view/main/ime.rs (handle_event)
      ├── 단축키·vi·escape 매칭 → origin과 원 대상을 담은 Intent 값
      └── 그 외 키 → 포커스 surface 로 분배:
          ├── Terminal → 원 surface/generation에 묶인 입력 요청 → App/Engine 실행 → PTY stdin → 셸
          └── egui-mesh(image, 그리고 markdown 의 확인 팝업 2개) → egui_mesh_push_key/text/ime
              → set_context.raw_input forward → plugin egui TextEdit (egui-mesh-channel)
```

셸 출력은 비동기로 돌아온다(흐름 2). View는 입력 값을 만들고 App/Engine adapter가 원 binding을 검사해 Terminal 또는 plugin에 전달한다. 단축키의 구조 변경과 고빈도 raw 입력은 같은 영속 사건으로 취급하지 않는다. (`markdown` 은 [ADR-0029](../adr/0029-webview-host-integration.md) 로 webview 전환됨 — 본문은 `set_context`/`paint` 를 아예 받지 않고, 네이티브 WebView 가 자체적으로 입력을 처리한다. 위 경로는 markdown 의 대용량/파일열기 확인 팝업 2개에만 해당.)

---

## 2. PTY 출력 → 파싱 → 렌더링

```
PTY stdout
  → Pty reader → 원 resource generation의 TerminalIngest
  → TerminalState 잠금 안에서 termwiz Parser → vte_handler/ (CSI/OSC/ESC 처리)
  → dirty/waker → AppEvent::TerminalOutput(id) → App의 도메인 관측
          └── osc.rs: OSC 7(cwd) · OSC 133(prompt boundary) · 알림 OSC → TerminalEvent
  → view/main/redraw.rs (handle_redraw)
  → gfx/gpu/render_pass.rs — accumulator 렌더 (단일 pass):
      begin_frame() → 각 surface append_terminal_viewport() → flush_buffers() → render_all()
      → wgpu submit + present
```

accumulator 모델(surface 별 submit 아님)·scissor·multi-page atlas 상세는 [gpu-rendering](../dev-guide/gpu-rendering.md).

---

## 3. IPC 요청 → 처리 → 응답

```
tasty-cli (또는 외부 프로그램)
  → TCP 연결 (~/.tasty/tasty.port)
  → hub IPC 서버 (수신 스레드) → JSON-RPC 파싱 → mpsc → AppEvent::IpcReady
  → boot.rs 메인 루프 → app/ipc.rs (process_ipc)
      → adapters/ipc/handler/ (도메인별 핸들러 + 권한 게이트 + audit)
      → RequestScope로 대상 해소; 구조 변경은 journal admission/commit/publication, 실행은 EngineMut adapter (origin=Agent)
      → JsonRpcResponse → TCP 회신
  → tasty-cli 결과 포맷 출력
```

플러그인 namespace 메서드(`claude.*` 등)는 `plugin_bridge/` 를 거쳐 plugin 프로세스로 위임된다. attach 스트리밍은 별도 `StreamReady` 경로(`tasty_ipc::stream_hub::StreamHub`).

**한 회차에는 두 예산이 있다.** gui 의 `process_ipc` 와 headless 의 `pump_ipc` 가 같은 규칙
(`app::ipc_round::IpcRound`)을 쓴다 — 큐에서 명령을 **하나 꺼내 끝까지 처리하고 다음 것을
꺼내며**, 명령 수가 `DRAIN_BUDGET_PER_ROUND`(동시 연결 상한에서 파생,
[ADR-0007](../adr/0007-ipc-scheduling-and-deadlines.md))에 닿거나 경과 시간이
`ROUND_TIME_BUDGET`(16 ms, [ADR-0007](../adr/0007-ipc-scheduling-and-deadlines.md))에
닿으면 멈춘다. 첫 명령은 시간과 무관하게 늘 처리한다. 남은 것은 **큐에 그대로** 있다가 다음
회차가 집는다. headless 는 이벤트 채널에 `IpcReady` 를 하나만 둔다(생산자 쪽 게이트) — 루프가 회차를
열기 전에 게이트를 풀고, 회차가 끝났을 때 큐에 명령이 남았으면 루프를 한 번 더 깨운다. 명령마다 wake 를
두면 지속 부하에서 채널에 적체가 쌓여 같은 채널의 plugin·PTY wake 가 그 뒤에서 굶는다
([ADR-0007](../adr/0007-ipc-scheduling-and-deadlines.md)). 예산은 명령 사이에서만 보므로 **이미 실행 중인 handler 는 끊지 못한다.**

gui 에서는 회차가 `about_to_wait`(iteration 마다 한 번)와 `IpcReady` 사용자 이벤트 두 자리에서 돈다.
winit 은 사용자 이벤트를 큐가 빌 때까지 처리한 뒤에야 `about_to_wait` 로 넘어가므로, 사용자 이벤트가
늘 회차를 열면 지속 부하에서 타이머·입력·렌더가 굶는다. 그래서 사용자 이벤트는 직전 회차가 끝난 뒤
한 회차 예산이 지났을 때만 회차를 열고(`app::ipc::IpcPacer`), 예산에서 멈춘 회차는 루프를 스스로 한
번 더 깨운다 — 건너뛴 wake 가 남은 명령의 몫이었을 수 있어서다. 그 재깨움은 양보 규칙을
`about_to_wait` 한 번 사이에 한 번 건너뛴다([ADR-0007](../adr/0007-ipc-scheduling-and-deadlines.md)).

순서는 도착 순이다. 연결 하나는 응답을 받을 때까지 다음 요청을 안 보내므로 큐에 한 번에 하나만
올리고, 그래서 어떤 요청 앞에 설 수 있는 명령 수는 연결 상한과 주입 깊이 상한으로 유한하다.
호출자별 스케줄링은 없다(ADR-0007).

호출자가 요청에 응답 대기 상한을 지정하면 명령은 **기한**(큐 진입 + 상한)을 든다. 회차는 명령을
꺼낸 직후, 게이트보다 앞에서 기한을 보고 지났으면 실행하지 않고 `-32067` 로 답한다. 응답을
기다리는 연결 스레드와 "시작했는가" 를 상태 칸 하나로 정하므로, 시작 뒤의 만료만 `-32061`(결과
불명)이다([ADR-0007](../adr/0007-ipc-scheduling-and-deadlines.md)).

큐에서 **꺼낸** 쪽의 누계 — 회차가 멈춘 이유 · 실행 전 만료 수 · 지금 실행 중인(in-flight) 요청
수 — 는 `tasty_ipc::dispatch::DispatchStats`(`AppServices`의 dispatch 계측)에 있고, 큐에 **든** 쪽(입장 장부)과
함께 `CommandQueueSnapshot::read` 한 자리에서 읽는다. in-flight는 실행을 시작했고
명령 처리 또는 응답 대기가 끝나지 않은 요청이다([ADR-0008](../adr/0008-ipc-pressure-observability.md)). 응답 대기가 먼저 끝나도 명령을 처리 중이면 집계에 남는다.
두 값은 `system.pressure`(CLI `tasty list pressure`)의 `queue_admission` · `queue_dispatch` 덩어리로
나간다([ADR-0008](../adr/0008-ipc-pressure-observability.md)).

종료 중의 drain 은 이 정책을 따르지 않는다 — 남은 요청을 거절하며 비워야 한다
([shutdown-sequence](shutdown-sequence.md)).

---

## 4. 알림 발생 → 저장 → 표시

```
알림 소스 (tasty-terminal):
  ├── vte_handler OSC 9/99/777 → TerminalEvent::Notification
  ├── BEL → BellRing
  └── 프로세스 종료 → ProcessExited

수집/저장:
  → core 가 이벤트 수집 → store/notification.rs (NotificationStore::add)
      ├── 병합: 같은 소스 coalesce 윈도우 내 → body 합치기
      └── FIFO 상한 초과 시 pop_front
  → tasty-hooks (Notification 이벤트 훅)

표시:
  → adapters/ui/ 사이드바 워크스페이스 배지 + 알림 패널(adapters/ui/notification.rs, popup 시스템에
    등록된 headless popup — 렌더 루프 자체는 adapters/ui/popup/frame.rs)
```

알림이 발생하면 Store를 갱신한다. popup은 자동으로 열지 않고 사용자가 요청했을 때 표시한다([팝업 정책](../design/systems/popup.md)).

---

## 5. 설정 로드 → 적용

```
시작 시:
  → tasty-settings Settings::load()
      → ~/.tasty/config.toml → toml::from_str, 없거나 실패 시 default 폴백
      → #[serde(default)] 부분 TOML 지원
  → boot/core 초기화 시 GpuState/MainViewState 에 반영 (font·theme·opacity·shell·scrollback)

런타임 변경:
  → SettingsView에서 draft 편집 → footer Save로 SettingsEdits 회수
  → App이 원 SettingsEditOwner와 함께 적용·저장하고 MainView에 설정 변경 반영
  → Cancel/일반 close는 Save 실행 초안을 만들지 않음

즉시 반영: font·theme·opacity(렌더러 재초기화/테마 전환), notification·keybindings(매 프레임/이벤트 참조).
새 터미널부터 반영: shell·shell_mode·scrollback (`ShellConfig::from_settings` 가 `runtime_shell` 로 실행 셸을 정하고 그 셸 기준 `shell_args_for` — tasty 모드 --rcfile 주입은 플랫폼별).
```

### 에이전트·plugin 설정 변경 요청의 완료 의미

`settings.set_remote_transfer`, `settings.set_input_rule`·`remove_input_rule`·`initialize_input_rule`, `webhook.config`의 `allow_external`, debug 전용 `debug.settings.apply`는 핸들러 시점 설정의 전체 사본을 고쳐 `DomainIntent::UpdateSettings` 하나를 요청 큐에 넣고 그 자리에서 성공 응답을 만든다. 입력 규칙 3종은 응답이 `changed: true`일 때만 큐에 넣는다. `initialize_input_rule`이 이미 채운 호출자/앱에 대해 `changed: false`를 응답할 때는 성공 응답이지만 큐에 넣는 변경이 없다. 메모리 적용과 `config.toml` 저장은 이 큐를 처리할 때 일어난다. 응답을 보내는 시점은 진입 경로마다 다르다.

| 진입 경로 | 순서 | 근거 |
|---|---|---|
| GUI 외부 IPC | 응답 → 큐 처리 | `App::ipc_step_routing`이 응답을 보낸 뒤 `dispatch_pending_intents`를 부른다 |
| GUI plugin 호출 | 큐 처리 → 응답 | `App::dispatch_checked`가 `dispatch_pending_intents` 뒤에 응답을 돌려주고 plugin 결과로 회신한다 |
| headless 외부 IPC | 큐 처리 → 응답 | `boot/headless_dispatch.rs`가 `drain_pending_intents_in_app` 뒤에 응답한다 |
| headless plugin 호출 | 큐 처리 → 응답 | `boot/headless_plugins.rs`가 같은 drain 뒤에 plugin 결과를 회신한다 |

큐 처리 안의 순서는 다음과 같다.

- 검사: `AppServices::apply_live`가 `task_pipeline.validate()`를 실행한다. 실패하면 설정을 바꾸지 않는다. GUI는 `report_apply_error`가, headless는 `apply_one`이 `warn` 로그만 남긴다. `report_apply_error`는 surface 종류 철회 오류만 출처별로 토스트를 검토하므로 설정 검사·reset 오류는 토스트로 표시하지 않는다. 이 절의 요청은 모두 에이전트 출처(`from_agent_ipc`)다. 핸들러가 만든 성공 응답은 바뀌지 않는다. `debug.settings.apply`만 같은 검사를 핸들러에서 먼저 실행해 실패를 `-32602`로 응답한다.
- GUI 적용: `cascade_settings_updated`가 설정 세대를 하나 올린 뒤 `apply_settings_after_structure`를 실행한다. task report 반영 → 창·parked engine 전체의 메모리 설정 교체 → `Settings::save` → 전역 Theme 설치와 창·터미널 색 갱신, plugin `theme.changed`·`language.changed` 발행 순서다. 저장 실패는 `error` 로그만 남기고 메모리 적용과 뒤 단계는 그대로 진행한다.
- GUI category reset: 현재 설정에서 켜져 있던 `workspace_categories_enabled`를 끄는 변경은 모든 engine의 `ResetCategories`를 한 journal batch로 admission하고 그 자리에서 끝난다. 위 적용 단계는 batch commit 뒤 `finish_settings_results`에서 실행된다. reset이 실패하면 오류를 보고하고 설정은 적용하지 않는다.
  완료가 도착했을 때 그 사이에 다른 `UpdateSettings`가 세대를 올렸다면 오래된 완료의 설정 사본은 버린다. 이미 commit된 reset 결과는 유지한다. 대기 중 들어온 최신 설정 의도가 이긴다는 이 동작은 `app::journal::commands::tests::stale_settings_reset_completion_cannot_overwrite_a_newer_settings_intent`가 고정한다.
- headless 적용: `intent::headless::apply_settings`가 task report 반영 → engine 메모리 설정 교체 → `Settings::save` 순서로 실행한다. category reset journal 경로는 없다. 저장 실패는 `warn` 로그만 남긴다.

따라서 `applied: true`(`settings.set_remote_transfer`·`debug.settings.apply`), `changed: true`, `allow_external` 응답은 핸들러가 요청을 검사해 설정 변경을 큐에 넣었다는 뜻이다. 어느 경로에서도 메모리 적용이나 `config.toml` 저장 완료를 보장하지 않는다. 응답 내용은 큐 처리 전에 핸들러가 정하므로, 큐 처리를 마친 뒤 응답하는 경로에서도 큐 처리의 검사 실패, category reset 실패, 저장 실패는 성공 응답의 필드에 반영되지 않는다. 검사 실패와 GUI의 reset 실패는 `warn` 로그로, 저장 실패는 `error`(GUI)·`warn`(headless) 로그로만 남는다. category reset이 걸린 GUI 변경은 응답 뒤 journal commit에서 적용된다. 적용 결과는 `settings.get_remote_transfer`·`settings.get_input_rules` 같은 조회로 확인한다.

저장소 전반(state.db / config.toml / presets / themes)은 [storage](../design/systems/storage.md), 설정 창 IA 는 [features/settings](../features/settings/index.md).

## 관련

- [action-dispatch](../design/flows/action-dispatch.md) — Intent 큐 디스패치 모델
- [아키텍처 개요](index.md) — App/process · EngineSession/domain/runtime · View 소유와 모듈 배치
