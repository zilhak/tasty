# 데이터 흐름

모듈 경계를 넘는 주요 흐름 5종을 파일+함수 기준으로 본다(줄 번호 대신). 호스트 *내부 동작* 이 Intent 큐로 통일된 디스패치 모델은 [action-dispatch](../design/flows/action-dispatch.md) — 본 문서는 입력→PTY→렌더 같은 *런타임 파이프라인* 이다.

메인 이벤트 루프는 `src/boot.rs` 의 `run()` 이 winit 이벤트와 `AppEvent`(`TerminalOutput` / `IpcReady` / `StreamReady`)를 drain 한다. 루프에는 축이 둘이다 — **프레임축**(이벤트가 있었으니 큐를 비운다: 위 drain + `dispatch_pending_*`)과 **시간축**(N ms 마다/뒤에 한 번). 시간축은 전부 중앙 타이머 허브에 키로 등록되고 `about_to_wait`(gui) / `recv_timeout` 루프(headless)가 due 한 키만 실행한다 — 등록·실행·대기 전략은 [timer-hub](../dev-guide/timer-hub.md).

---

## 1. 키보드 입력 → 터미널 → 화면

```
winit KeyEvent / Ime
  → app/event_handler.rs (ApplicationHandler::window_event)
  → view/main/ (overlay/host-egui surface 면 egui 가 먼저 소비 — input-layer)
  → view/main/keyboard.rs (handle_keyboard_input) · view/main/ime.rs (handle_event)
      ├── 단축키·vi·escape 매칭 → Intent 발화 (shortcuts → UiIntent/DomainIntent, action-dispatch)
      └── 그 외 키 → 포커스 surface 로 분배:
          ├── Terminal → forward_key_to_terminal → tasty-terminal(send_key) → PTY stdin → 셸
          └── egui-mesh(image, 그리고 markdown 의 확인 팝업 2개) → egui_mesh_push_key/text/ime
              → set_context.raw_input forward → plugin egui TextEdit (egui-mesh-channel)
```

셸 출력은 비동기로 돌아온다(흐름 2). 키 입력 중 *단축키* 만 Intent 큐를 타고, 터미널 키스트로크는 PTY 로 직접, egui-mesh surface 키/IME 는 plugin 으로 forward 된다. (`markdown` 은 [ADR-0029](../adr/0029-webview-host-integration.md) 로 webview 전환됨 — 본문은 `set_context`/`paint` 를 아예 받지 않고, 네이티브 WebView 가 자체적으로 입력을 처리한다. 위 경로는 markdown 의 대용량/파일열기 확인 팝업 2개에만 해당.)

---

## 2. PTY 출력 → 파싱 → 렌더링

```
PTY stdout
  → tasty-terminal 리더 스레드 (mpsc) → waker → AppEvent::TerminalOutput(id)
  → boot.rs 메인 루프 → core (PTY drain)
  → tasty-terminal Terminal::process()
      → termwiz Parser → vte_handler/ (CSI/OSC/ESC → Surface 변경)
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
      → 동작은 Intent 큐 또는 Core 직접 조작 (action-dispatch: origin=Agent)
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

호출자가 봉투에 응답 대기 상한을 실었으면 명령은 **기한**(큐 진입 + 상한)을 든다. 회차는 명령을
꺼낸 직후, 게이트보다 앞에서 기한을 보고 지났으면 실행하지 않고 `-32067` 로 답한다. 응답을
기다리는 연결 스레드와 "시작했는가" 를 상태 칸 하나로 정하므로, 시작 뒤의 만료만 `-32061`(결과
불명)이다([ADR-0007](../adr/0007-ipc-scheduling-and-deadlines.md)).

큐에서 **꺼낸** 쪽의 누계 — 회차가 멈춘 이유 · 실행 전 만료 수 · 지금 실행 중인(in-flight) 요청
수 — 는 `tasty_ipc::dispatch::DispatchStats`(`Core::dispatch`)에 있고, 큐에 **든** 쪽(입장 장부)과
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

알림은 *시스템 조건* 발이라 popup 을 자동으로 띄우지 않는다 — 데이터(Store)만 바꾸고 UI 가 수동 표시([toast/popup 발화 정책](../design/systems/popup.md)).

---

## 5. 설정 로드 → 적용

```
시작 시:
  → tasty-settings Settings::load()
      → ~/.tasty/config.toml → toml::from_str, 없거나 실패 시 default 폴백
      → #[serde(default)] 부분 TOML 지원
  → boot/core 초기화 시 GpuState/AppState 에 반영 (font·theme·opacity·shell·scrollback)

런타임 변경:
  → 설정 모달(SettingsView)에서 draft 편집 → Save → Settings::save() (TOML write)
  → 닫힐 때 모든 MainView 에 적용

즉시 반영: font·theme·opacity(렌더러 재초기화/테마 전환), notification·keybindings(매 프레임/이벤트 참조).
새 터미널부터 반영: shell·shell_mode·scrollback (effective_shell_args — tasty 모드 --rcfile 주입은 플랫폼별).
```

저장소 전반(state.db / config.toml / presets / themes)은 [storage](../design/systems/storage.md), 설정 창 IA 는 [features/settings](../features/settings/index.md).

## 관련

- [action-dispatch](../design/flows/action-dispatch.md) — Intent 큐 디스패치 모델
- [아키텍처 개요](index.md) — Core/Hub/View 분리, 모듈 배치
