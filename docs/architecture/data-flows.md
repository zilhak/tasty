# 데이터 흐름

5가지 주요 데이터 흐름을 단계별로 설명한다.

---

## 1. 키보드 입력 → 터미널 → 화면 출력

사용자 키 입력이 PTY를 거쳐 화면에 반영되기까지의 전체 흐름.

### 단계별 흐름 (14단계)

**1단계: winit 키 이벤트 수신**
- `main.rs:390` — `WindowEvent::KeyboardInput { event, .. }` 매칭.
- winit이 OS로부터 키 이벤트를 수신하고 `window_event()`로 전달한다.

**2단계: egui 이벤트 소비 확인**
- `main.rs:341-345` — `gpu.handle_egui_event(window, &event)` 호출.
- `gpu.rs:141` — `egui_state.on_window_event(window, event)` 실행.
- egui가 이벤트를 소비했으면 `egui_consumed = true`가 되어 터미널로 전달하지 않는다.

**3단계: Escape 오버레이 처리**
- `main.rs:396-410` — 설정 창이나 알림 패널이 열려 있으면 Escape 키로 닫는다.
- `state.settings_open = false` 또는 `state.notification_panel_open = false`.

**4단계: 앱 단축키 처리**
- `main.rs:414` — `self.handle_shortcut(&event.logical_key, self.modifiers)` 호출.
- `main.rs:102-241` — Ctrl+Shift 조합 (N/T/E/O/D/J/I), Ctrl+Tab, Alt+1~9, Alt+Arrow 등.
- 단축키에 매칭되면 `true` 반환, 터미널로 전달하지 않는다.

**5단계: 오버레이 차단 확인**
- `main.rs:420-425` — `settings_open || notification_panel_open`이면 터미널 전달 차단.

**6단계: 포커스 터미널 획득**
- `main.rs:429-430` — `state.focused_terminal_mut()` 호출.
- `state.rs:142-143` — `focused_pane_mut()` → `active_terminal_mut()` 체인.
- `state.rs:122-134` — 스테일 ID 처리: `focused_pane` ID가 유효하지 않으면 첫 패인으로 폴백.

**7단계: 텍스트 이벤트 전송**
- `main.rs:432-437` — `event.text`가 존재하면 `terminal.send_key(s)` 호출.
- `terminal.rs:554-557` — `pty_writer.write_all(text.as_bytes())` + `flush()`.
- winit의 `KeyEvent.text`는 수정자 키 반영된 텍스트 (예: Ctrl+C → `\x03`).

**8단계: 특수 키 이스케이프 시퀀스 전송**
- `main.rs:440-468` — Enter(`\r`), Backspace(`\x7f`), 방향키(`\x1b[A~D`), F키 등.
- `terminal.rs:560-562` — `pty_writer.write_all(bytes)` + `flush()`.

**9단계: PTY 처리 → 셸 프로세스**
- PTY를 통해 바이트가 셸 프로세스의 stdin으로 전달된다.
- 셸이 처리한 결과가 PTY의 stdout으로 출력된다.

**10단계: PTY 리더 스레드 수신**
- `terminal.rs:99-113` — 백그라운드 스레드가 `pty_reader.read(&mut buf)` 루프.
- 8KB 청크 단위로 읽어 `mpsc::sync_channel(32)`로 전송.
- `waker()` 콜백 호출 (108행) → `EventLoopProxy::send_event(AppEvent::TerminalOutput)`.

**11단계: 이벤트 루프 웨이크업**
- `main.rs:266-275` — `AppEvent::TerminalOutput` 수신 → `dirty = true` + `window.request_redraw()`.

**12단계: PTY 출력 파싱**
- `main.rs:620-624` — `state.process_all()` 호출.
- `state.rs:259-269` — 모든 워크스페이스의 터미널 순회 → `terminal.process()`.
- `terminal.rs:137-179` — `action_rx.try_recv()` → `parser.parse_as_vec` → `action_to_changes` → `surface.add_change`.
- 원시 바이트는 `output_buffer`에도 축적 (142행, 읽기 마크 API용).

**13단계: 인스턴스 데이터 빌드**
- `gpu.rs:238-264` — 각 서피스에 대해 `renderer.prepare_viewport(terminal.surface(), ...)` 호출.
- `renderer.rs:632-654` — 유니폼 갱신 + `prepare()` 호출.
- `renderer.rs:514-589` — Surface 셀 순회 → BgInstance/GlyphInstance 벡터 생성 → GPU 버퍼 업로드.

**14단계: GPU 렌더링**
- `gpu.rs:248-264` — 터미널 패스: `render_scissored()` 호출.
- `renderer.rs:657-688` — 시저 렉트 설정 → 배경 파이프라인 → 글리프 파이프라인.
- `gpu.rs:267-321` — egui 패스 → `queue.submit()` → `output.present()`.

---

## 2. PTY 출력 → 파싱 → 렌더링

셸 프로세스의 출력이 화면에 나타나기까지의 파싱 흐름.

### 단계별 흐름 (8단계)

**1단계: 원시 바이트 수신**
- `terminal.rs:140` — `action_rx.try_recv()` 으로 8KB 청크 수신.
- `terminal.rs:142-155` — `output_buffer`에 축적, 1MB 초과 시 앞부분 트림, 읽기 마크 조정.

**2단계: termwiz 파서 실행**
- `terminal.rs:157` — `self.parser.parse_as_vec(&data)`.
- termwiz Parser가 VTE 이스케이프 시퀀스를 `Action` enum으로 변환.

**3단계: 액션 디스패치**
- `terminal.rs:182-195` — `action_to_changes(action)` 분기:
  - `Action::Print(c)` → `Change::Text`
  - `Action::Control(code)` → `map_control()`
  - `Action::CSI(csi)` → `map_csi()`
  - `Action::Esc(esc)` → `map_esc()`
  - `Action::OperatingSystemCommand(osc)` → `map_osc()` (이벤트 생성, 변경 없음)

**4단계: CSI 처리**
- `terminal.rs:219-234` — `map_csi()`:
  - `CSI::Sgr` → `map_sgr()` (236행): 색상, 굵기, 밑줄, 이탤릭 등 속성 변경.
  - `CSI::Cursor` → `map_cursor()` (269행): 커서 이동 (Up/Down/Left/Right/Position/Save/Restore).
  - `CSI::Edit` → `map_edit()` (360행): 화면 지우기 (EraseInDisplay, EraseInLine, Scroll).
  - `CSI::Mode` → 미구현 (224행, TODO: DECSET/DECRST).

**5단계: Surface 갱신**
- `terminal.rs:161-163` — `surface.add_change(change)` 루프.
- termwiz Surface가 셀 그리드 상태를 업데이트한다 (커서 위치, 셀 속성, 텍스트 내용).

**6단계: 프로세스 종료 감지**
- `terminal.rs:170-176` — `check_process_alive()` 실패 시 `ProcessExited` 이벤트 생성 (1회만).

**7단계: Surface → 인스턴스 데이터**
- `renderer.rs:514-589` — `prepare()`:
  - `surface.screen_lines()` 순회.
  - 각 셀의 `attrs()` → `color_attr_to_rgba()` (배경/전경).
  - BgInstance 생성 (모든 셀).
  - GlyphInstance 생성 (비어있지 않은 셀): `atlas.get_or_insert()` → 아틀라스 UV 좌표 조회/래스터라이즈.
  - 인스턴스 버퍼 GPU 업로드 (`queue.write_buffer`).

**8단계: 2-pass 렌더**
- `renderer.rs:592-608` 또는 `renderer.rs:657-688` (scissored):
  - Pass 1: 배경 파이프라인 — bg_bind_group, bg_instance_buffer, `draw(0..6, 0..bg_instance_count)`.
  - Pass 2: 글리프 파이프라인 — glyph_bind_group (아틀라스 텍스처 + 샘플러), glyph_instance_buffer, 알파 블렌딩.

---

## 3. IPC 요청 → 처리 → 응답

CLI 또는 외부 프로그램의 JSON-RPC 요청이 처리되는 흐름.

### 단계별 흐름 (10단계)

**1단계: 포트 파일 읽기**
- `ipc/server.rs:172-184` — `IpcServer::read_port_file()`.
- CLI 클라이언트가 `~/.config/tasty/tasty.port` 파일에서 포트 번호를 읽는다.

**2단계: TCP 연결**
- `cli.rs:127-131` — `TcpStream::connect(format!("127.0.0.1:{}", port))`.
- `ipc/server.rs:49-54` — 서버의 수신 스레드가 `listener.accept()` → 연결 스레드 spawn.

**3단계: JSON-RPC 요청 전송**
- `cli.rs:134-136` — `command_to_request(&command)` → `serde_json::to_string` → `writeln!(stream, ...)`.

**4단계: 서버 파싱**
- `ipc/server.rs:97-121` — 연결 스레드가 라인 단위로 읽어 `serde_json::from_str::<JsonRpcRequest>` 파싱.
- 파싱 실패 시 `-32700` 에러 응답 즉시 전송.

**5단계: 채널 전달**
- `ipc/server.rs:123-134` — `IpcCommand { request, response_tx }` 생성 → `cmd_tx.send(cmd)`.
- 응답 채널 `mpsc::sync_channel(1)` 생성, 연결 스레드는 `resp_rx.recv()` 대기.

**6단계: 메인 스레드 수신**
- `main.rs:244-262` — `App::process_ipc()`.
- `ipc_server.try_recv()` 루프로 비차단 수신.
- `RedrawRequested` 이벤트 처리 시 호출 (617행).

**7단계: 핸들러 디스패치**
- `main.rs:256` — `ipc::handler::handle(state, &cmd.request)`.
- `ipc/handler.rs:10-36` — `request.method` 문자열 매치 → 20개 핸들러 중 하나 호출.

**8단계: AppState 조작**
- 핸들러 함수 내에서 `AppState`의 메서드 호출.
- 예: `handle_workspace_create` (66행) → `state.add_workspace()`.
- 예: `handle_surface_send` (261행) → `state.focused_terminal_mut()?.send_key(text)`.

**9단계: 응답 전송**
- `main.rs:257` — `cmd.response_tx.send(response)`.
- `ipc/server.rs:138-148` — 연결 스레드가 응답을 `serde_json::to_string` → `writeln!(writer, ...)`.

**10단계: CLI 출력**
- `cli.rs:138-157` — BufReader로 응답 라인 읽기 → `JsonRpcResponse` 파싱.
- 에러면 stderr + exit(1), 성공이면 `format_output()` 호출.
- `cli.rs:247-258` — 명령별 포매팅 (tree, list, panes 등).

---

## 4. 알림 발생 → 저장 → UI 표시

터미널 이벤트에서 알림이 생성되어 UI에 표시되기까지의 흐름.

### 단계별 흐름 (7단계)

**1단계: 터미널 이벤트 생성**
- 알림 소스 3가지:
  - OSC 9/99/777: `terminal.rs:498-547` — `map_osc()` 내에서 `TerminalEvent { kind: Notification { title, body } }` 생성.
  - BEL 문자: `terminal.rs:208-214` — `map_control(ControlCode::Bell)` → `TerminalEvent { kind: BellRing }`.
  - 프로세스 종료: `terminal.rs:170-176` — `TerminalEvent { kind: ProcessExited }`.

**2단계: 이벤트 수집**
- `main.rs:631-632` — `state.collect_events()` 호출.
- `state.rs:334-387` — 모든 워크스페이스의 모든 PaneNode → Tab → Panel → Terminal/SurfaceGroupLayout 재귀 순회.
- 각 `terminal.take_events()`로 이벤트를 drain하고, `surface_id`를 주입.

**3단계: 알림 이벤트 처리**
- `main.rs:636-656` (Notification):
  - 조건: `settings.notification.enabled`.
  - OS 알림 조건: `system_notification && !window_focused && should_send_system_notification()`.
  - `notification::send_system_notification(title, body)` (159행) → `notify_rust::Notification`.
  - `state.notifications.add(ws_id, surface_id, title, body)`.

**4단계: NotificationStore 저장**
- `notification.rs:52-104` — `add()`:
  - 병합 확인: 같은 소스(workspace_id + surface_id)에서 `coalesce_ms` 이내 → 기존 알림에 body 합치기.
  - FIFO: 100개 초과 시 `pop_front()`.
  - 새 알림 생성 → `push_back()`.

**5단계: Hook 실행**
- `main.rs:654-655` — Notification 이벤트 시 `hook_manager.check_and_fire(surface_id, &[HookEvent::Notification])`.
- `hooks.rs:140-168` — 매칭 훅의 `command`를 `std::thread::spawn`으로 셸 실행.

**6단계: 사이드바 배지 렌더**
- `ui.rs:24-39` — `state.notifications.unread_count()` → 0보다 크면 헤더에 배지 표시.
- `ui.rs:53-74` — 각 워크스페이스별 `unread_count_for_workspace(ws_id)` → 하이라이트.

**7단계: 알림 패널 렌더**
- `ui.rs:242-396` — `draw_notification_panel()`:
  - `Ctrl+Shift+I` 토글 → `state.notification_panel_open` (main.rs:169-177).
  - 열릴 때 `mark_all_read()` 호출.
  - 최신 순 스크롤 목록, 워크스페이스 점프 버튼, 개별 읽음 처리.

---

## 5. 설정 로드 → 적용

TOML 설정 파일이 로드되어 런타임에 반영되기까지의 흐름.

### 단계별 흐름 (6단계)

**1단계: 경로 결정**
- `settings.rs:177-178` — `Settings::config_path()`.
- `BaseDirs::new()?.config_dir().join("tasty").join("config.toml")`.
- Windows: `%APPDATA%/tasty/config.toml`, macOS: `~/Library/Application Support/tasty/config.toml`, Linux: `~/.config/tasty/config.toml`.

**2단계: TOML 파싱**
- `settings.rs:192-217` — `Settings::load()`:
  - `fs::read_to_string(&path)` → `toml::from_str::<Settings>(&contents)`.
  - 파일 없음/파싱 실패 → `Settings::default()` 폴백.
  - 부분 TOML: `#[serde(default)]` 어노테이션으로 누락 필드는 기본값.

**3단계: 초기 적용 — GPU 초기화**
- `main.rs:297-300` — `resumed()`에서 `Settings::load()` 호출.
  - `sidebar_logical_width` → 터미널 영역 계산에 사용.
  - `font_size` → `CellRenderer::new()` (gpu.rs:88)에서 14.0 하드코딩 (미반영, 321행 주석).

**4단계: 초기 적용 — AppState 생성**
- `state.rs:75-100` — `AppState::new()`:
  - `Settings::load()` 재호출 (76행).
  - `settings.general.shell` → 첫 터미널 셸 결정 (82행).
  - `settings.notification.coalesce_ms` → NotificationStore 초기화 (91행).
  - `settings.appearance.sidebar_width` → `sidebar_width` 캐싱 (84행).

**5단계: 런타임 변경 — 설정 윈도우**
- `gpu.rs:171-182` — `settings_ui::draw_settings_window()` 호출.
- `settings_ui.rs:29-119` — 드래프트 편집 → Save/Cancel:
  - Save: `*settings = draft.clone()` + `settings.save()`.
  - `settings.rs:220-229` — `toml::to_string_pretty` → `fs::write`.

**6단계: 런타임 반영 한계**
- 대부분의 설정은 초기화 시에만 적용된다. 런타임 변경 후 즉시 반영되는 항목:
  - `notification.enabled`, `notification.system_notification`, `notification.sound`, `notification.coalesce_ms`: 매 프레임 `state.settings`에서 참조 (main.rs:637-669).
  - `sidebar_width`: `state.sidebar_width`가 초기화 시 캐싱되므로 런타임 변경은 미반영.
  - `general.shell`: 새 터미널 생성 시만 적용 (state.rs:177-178).
  - `appearance.font_size`, `font_family`, `theme`, `background_opacity`: 런타임 미반영 (GPU 리소스 재생성 필요).
  - `keybindings.*`: 런타임 미반영 (main.rs의 하드코딩 단축키 사용).
  - `clipboard.*`: 클립보드 미구현.
