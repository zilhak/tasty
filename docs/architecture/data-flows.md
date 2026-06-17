# 데이터 흐름

모듈 경계를 넘는 주요 흐름 5종을 파일+함수 기준으로 본다(줄 번호 대신). 호스트 *내부 동작* 이 Intent 큐로 통일된 디스패치 모델은 [action-dispatch](../design/flows/action-dispatch.md) — 본 문서는 입력→PTY→렌더 같은 *런타임 파이프라인* 이다.

메인 이벤트 루프는 `src/boot.rs` 의 `run()` 이 winit 이벤트와 `AppEvent`(`TerminalOutput` / `IpcReady` / `StreamReady`)를 drain 한다.

---

## 1. 키보드 입력 → 터미널 → 화면

```
winit KeyEvent
  → app/event_handler.rs (ApplicationHandler::window_event)
  → view/main/ (egui 가 먼저 소비하면 종료 — input-layer)
  → view/main/keyboard.rs (handle_keyboard_input)
      ├── 단축키 매칭 → Intent 발화 (shortcuts → UiIntent/DomainIntent, action-dispatch)
      └── 그 외 키 → send_key_to_terminal
          → tasty-terminal (Terminal::send_key) → PTY stdin → 셸
```

셸 출력은 비동기로 돌아온다(흐름 2). 키 입력 중 *단축키* 만 Intent 큐를 타고, 일반 키스트로크는 PTY 로 직접 간다.

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

플러그인 namespace 메서드(`claude.*` 등)는 `plugin_bridge/` 를 거쳐 plugin 프로세스로 위임된다. attach 스트리밍은 별도 `StreamReady` 경로(stream_hub).

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
  → adapters/ui/notification.rs (OS 네이티브 알림, 비활성 view 한정)
  → tasty-hooks (Notification 이벤트 훅)

표시:
  → adapters/ui/ 사이드바 워크스페이스 배지 + 알림 패널
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
