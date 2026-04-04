# 데이터 흐름

5가지 주요 데이터 흐름을 설명한다. 줄 번호 대신 파일명+함수명으로 참조한다.

---

## 1. 키보드 입력 → 터미널 → 화면 출력

```
winit KeyEvent
  → event_handler.rs (ApplicationHandler::window_event)
  → tasty_window/mod.rs (handle_window_event)
  → gpu/mod.rs (handle_egui_event) — egui가 소비하면 여기서 종료
  → tasty_window/keyboard.rs (handle_keyboard_input)
      ├── Escape → 설정/알림 패널 닫기
      ├── shortcuts.rs (handle_shortcut) → 단축키 매칭 시 종료
      └── tasty_window/keyboard.rs (send_key_to_terminal)
          → tasty-terminal crate (Terminal::send_key)
          → PTY stdin
              → 셸 프로세스 처리
              → PTY stdout
          → 리더 스레드 (Terminal 내부, 8KB 청크)
          → EventLoopProxy::send_event(TerminalOutput)
  → tasty_window/redraw.rs (handle_redraw)
      → state/layout.rs (process_all) — 모든 터미널 process()
      → gpu/mod.rs (render)
          → renderer/mod.rs (prepare_terminal_viewport)
          → gpu/render_pass.rs (render_clear_pass → render_terminals → render_egui_pass)
          → wgpu submit + present
```

---

## 2. PTY 출력 → 파싱 → 렌더링

```
PTY stdout
  → tasty-terminal 리더 스레드 (8KB 청크, mpsc 채널)
  → Terminal::process()
      → output_buffer에 축적 (Read Mark API용, 1MB 순환)
      → termwiz Parser::parse_as_vec()
      → vte_handler.rs (action_to_changes)
          ├── Action::Print → Change::Text
          ├── Action::CSI → map_csi (SGR/Cursor/Edit/Mode)
          ├── Action::Esc → map_esc
          └── Action::OSC → map_osc (알림 이벤트 생성)
      → termwiz Surface::add_change()
  → renderer/mod.rs (prepare_with_bg 또는 prepare_terminal_viewport)
      → 셀 순회 → BgInstance + GlyphInstance 벡터
      → font.rs (GlyphAtlas::get_or_insert) — 글리프 래스터라이즈
      → GPU 버퍼 업로드 (queue.write_buffer)
  → renderer/mod.rs (render_scissored)
      → Pass 1: 배경 파이프라인 (BgInstance)
      → Pass 2: 글리프 파이프라인 (GlyphInstance, 알파 블렌딩)
```

---

## 3. IPC 요청 → 처리 → 응답

```
CLI 클라이언트 (cli/mod.rs run_client)
  → cli/request.rs (command_to_request) — Commands → JSON-RPC
  → TCP 연결 (포트: ~/.tasty/tasty.port)
  → ipc/server.rs (수신 스레드)
      → JSON 라인 파싱 → JsonRpcRequest
      → IpcCommand { request, response_tx } → mpsc 채널
      → EventLoopProxy::send_event(IpcReady)
  → main.rs (process_ipc)
      ├── window.create/close/focus/list → App 레벨 처리
      ├── ui.screenshot → gpu/screenshot.rs
      └── 나머지 → ipc/handler/mod.rs (handle)
          → 도메인별 핸들러 (workspace/pane/tab/surface/claude/hooks/message/meta)
          → AppState 조작 → JsonRpcResponse
  → response_tx.send() → 연결 스레드 → TCP 전송
  → cli/format.rs (format_output) — 결과 출력
```

---

## 4. 알림 발생 → 저장 → UI 표시

```
알림 소스:
  ├── tasty-terminal vte_handler.rs (OSC 9/99/777) → TerminalEvent::Notification
  ├── tasty-terminal vte_handler.rs (BEL) → TerminalEvent::BellRing
  └── tasty-terminal lib.rs (프로세스 종료) → TerminalEvent::ProcessExited

수집 + 저장:
  → tasty_window/redraw.rs (handle_redraw)
      → state/mod.rs (collect_events) — 모든 워크스페이스 순회
      → notification.rs (NotificationStore::add)
          ├── 병합: 같은 소스에서 coalesce_ms 이내 → body 합치기
          └── FIFO: 100개 초과 시 pop_front
      → notify-rust (OS 네이티브 알림, 비활성 윈도우 + 초당 1회 제한)
      → tasty-hooks (HookManager::check_and_fire) — Notification 이벤트 훅 실행

UI 표시:
  → ui/sidebar.rs — 워크스페이스 카드에 "!" 배지
  → ui/notification.rs (draw_notification_panel) — 스크롤 목록, 워크스페이스 점프, 읽음 처리
```

---

## 5. 설정 로드 → 적용

```
시작 시:
  → settings/mod.rs (Settings::load)
      → ~/.tasty/config.toml 읽기 → toml::from_str
      → 파일 없음/파싱 실패 → Settings::default() 폴백
      → #[serde(default)]로 부분 TOML 지원
  → main.rs (init_app_state)
      → gpu/mod.rs (GpuState::new) — font_size, font_family, theme, opacity 반영
      → state/mod.rs (AppState::new) — shell, scrollback, sidebar_width 반영

런타임 변경:
  → shortcuts.rs (toggle_settings) → AppEvent::OpenSettings
  → main.rs (open_settings_modal) → modal_window.rs (별도 OS 윈도우)
      → settings_ui/mod.rs (draw_settings_panel) — 드래프트 편집
      → Save: settings = draft + settings.save() (TOML 직렬화 → 파일 쓰기)
  → main.rs (close_settings_modal)
      → 모든 윈도우에 새 설정 적용

런타임 즉시 반영되는 항목:
  - font_size/font_family: gpu/egui_bridge.rs (post_egui_update)에서 변경 감지 → 렌더러 재초기화
  - theme/opacity: gpu/egui_bridge.rs에서 테마 전환
  - notification 설정: 매 프레임 settings에서 직접 참조
  - keybindings: 매 단축키 이벤트 시 settings에서 참조

새 터미널 생성 시만 반영되는 항목:
  - general.shell, shell_mode, shell_args
  - general.scrollback_lines
```
