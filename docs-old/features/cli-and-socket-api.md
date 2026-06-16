# CLI 도구 & 소켓 API

- **Status**: Implemented

### JSON-RPC IPC 서버 (ipc/)
- GUI 모드 시작 시 `127.0.0.1`의 랜덤 포트에 TCP 서버 자동 기동
- 포트 번호를 `~/.tasty/tasty.port` 파일에 기록하여 CLI 클라이언트가 접속 가능
- `--port-file` 옵션으로 커스텀 포트 파일 경로 지정 가능 (테스트 격리용)
- 앱 종료 시 포트 파일 자동 삭제 (Drop trait)
- JSON-RPC 2.0 프로토콜: 줄 단위 JSON 요청/응답
- 멀티클라이언트: 각 TCP 연결을 별도 스레드에서 처리
- 메인 스레드 채널 통신: IPC 스레드 -> mpsc 채널 -> 이벤트 루프에서 처리 -> oneshot 응답

### 스트리밍 채널 (server→client push)

요청-응답 JSON-RPC 위에 **서버가 클라이언트로 연속 push** 할 수 있는 스트리밍 채널을 같은 TCP listener 에 얹는다. attach/detach 의 실시간 PTY 출력 전송 토대이며, [attach/detach](#attachdetach-surfaceworkspace-mirror) 가 이 채널 위에서 동작한다.

- **연결 승격**(`src/adapters/production/tcp_ipc_server.rs`): 한 연결의 첫 줄이 `{"method":"stream.open",...}` 면 그 연결을 JSON-RPC 직렬 루프에서 빼내 **length-prefixed 바이너리 프레임** 양방향 파이프로 승격한다. 일반 요청-응답 경로는 무손상.
- **프레임 포맷**(`crates/tasty-ipc/src/stream.rs`): `[tag u8][len u32 BE][payload]`. tag = `Data`/`Control`/`Ping`/`Detach`. 프레임 길이 상한 1 MiB(OOM 방어). 바이너리 1:1 (base64 없음).
- **push 레지스트리**(`src/adapters/production/stream_hub.rs::StreamHub`): 연결마다 bounded sink 를 보유해 **메인 루프가 특정 클라로 non-blocking push**(느린 클라는 프레임 drop → 임계 초과 시 disconnect). 메인 루프(`AppEvent::StreamReady`)가 클라 inbound 프레임을 drain.
- **보안**: 스트림 채널은 **자체 토큰을 강제하지 않는다** — 신뢰를 SSH + 127.0.0.1 loopback 에 위임(SSH 가 뚫리면 tasty 보안은 무의미). 인증 레이어 없음.
- **클라 transport**(`crates/tasty-cli/src/stream.rs::StreamConnection`): 핸드셰이크 + 프레임 read/write. debug 빌드의 `tasty debug stream-echo` 가 push 경로를 end-to-end 검증(보낸 프레임이 메인 루프를 거쳐 echo 로 회신).

### 헤드리스 데몬 모드

GUI 없이 동작하는 PTY 호스트 데몬. `--no-default-features` 빌드는 항상 헤드리스이며, GUI 빌드도 `--headless` 플래그로 진입을 시도한다(단, GUI 빌드는 헤드리스 런타임을 내장하지 않아 경고 후 GUI 로 fallback — 실제 헤드리스는 `--no-default-features` 빌드 전용).

- **데몬 본체** (`src/boot.rs::run_headless`): winit/wgpu/egui 없이 `mpsc::channel<AppEvent>` 기반 blocking 루프(`rx.recv()`)로 동작. waker(`HeadlessWaker`/`HeadlessWakerFactory`, `src/adapters/production/headless_waker.rs`)가 PTY 출력 도착 시 `TerminalOutput`, IPC 명령 도착 시 `IpcReady` 를 push 하면 루프가 깨어 처리한다.
- **부팅 시 engine 부트스트랩**: GUI 는 첫 윈도우 생성 시 `CoreState`/`AppState` 를 만들지만 헤드리스는 창이 없으므로 `run_headless` 가 직접 1 회 생성한다. `CoreState::new_with_ids` 가 default workspace + 터미널 1 개(80×24)를 spawn 하므로 **client 0 명에도 PTY 가 살아 있다.**
- **PTY 펌프**: `TerminalOutput` 수신 시 `Core::process_all_pty_output` 으로 reader 채널을 drain(채널 포화로 reader 가 블록되는 것 방지) → termwiz 파싱 → 화면/scrollback 갱신 + observer/command_index/OSC52 부수효과 적용. 따라서 화면을 그리지 않아도 출력이 계속 반영된다.
- **IPC dispatch** (`src/boot/headless_dispatch.rs::pump_ipc`): `IpcReady` 수신 시 큐를 drain 해 각 명령을 단일 engine 으로 직결 dispatch(caller 해석 → `handle_with_caller`). GUI 의 view/parked/plugin 의존 5-step 라우터를 engine 1 개 환경에 맞게 간소화한 것.
- **스트리밍 채널 공존**: `StreamReady` 수신 시 `StreamHub::pump_inbound` 로 스트림 inbound 를 drain(PTY 펌프/IPC dispatch 와 독립 이벤트). push 는 non-blocking 이라 PTY drain 을 방해하지 않는다. ([스트리밍 채널](#스트리밍-채널-serverclient-push) 참조.)
- **제약(현재 단계)**: layout 복원은 헤드리스에선 미적용(항상 default workspace). 데몬 종료는 프로세스 kill 로 한다(`system.shutdown` IPC 헤드리스 dispatch 미포함). busy indicator 미구현.

### attach/detach (surface/workspace mirror)

한 인스턴스(server)의 터미널 surface 또는 workspace 를 다른 인스턴스/CLI client 가 **배타 점유해 attach** 한다. client 는 원격 grid 를 mirror 로 재구성해 실시간 입출력하고, server 는 점유된 대상을 readonly 로 보인다(내용 보임, 조작은 차단·force-detach 로 회수). **server 는 transport 를 모르고 항상 `127.0.0.1` 로만 client 를 받는다 — 로컬/원격 구분은 전적으로 client 측 개념**이다(로컬=포트파일 직결, 원격=`ssh -L` 터널 후 터널 localport 직결).

정확한 동작 명세(서버/클라이언트 계층, 점유 모델, 초기 스냅샷+delta, 모드, workspace 다중화, GUI mirror, 자동 매핑, force-detach, SSH 터널, IPC 표면)는 **[dev-guide/attach-behavior.md](dev-guide/attach-behavior.md)** 단일 출처를 본다. 여기서는 사용자/에이전트가 보는 CLI 표면만 요약한다.

- **원격 attach (release)**: `tasty remote attach [SURFACE] --ssh user@host` 또는 `--profile <name>`. surface 대신 `--workspace <id>` 로 워크스페이스 전체(트리 mirror). 모드 옵션 — `--dump-after <ms>`(출력 수집 후 mirror 화면 stdout, GUI 없이 검증), `--send <str>`(attach 직후 1회 입력, workspace 는 `--send-to <remote_sid>`), `--raw`(stdin/stdout passthrough, detach `Ctrl+\`, workspace 불가), `--no-reconnect`(SSH 끊김 시 자동 재연결 끄기), `--force-detach`(점유 강제 해제 — `--ssh` 와 상호배타). GUI mirror 트리거 — `--into-gui --target-port <원격포트> --workspace <원격ws>`.
- **원격 생존 확인 (release)**: `tasty remote check --ssh user@host` / `--profile <name>`. 포트 발견만으론 stale 포트(이미 죽은 인스턴스의 포트 파일)를 alive 로 오판할 수 있어, 포트 발견 + `ssh -L` 터널 + `system.info` IPC 1회까지 거쳐 응답이 와야 alive(stdout + exit 0), 아니면 dead(stderr + exit≠0).
- **로컬 loopback attach (debug 전용)**: `tasty debug attach [SURFACE] [--workspace <id>] [--dump-after|--send|--send-to|--raw|--force-detach]`. 같은 머신 self-attach 는 *사용자 mirror 조작의 자동 재현* 성격이라 release 표면에서 제거하고 debug 빌드로 격리한다(원칙 1 ②, [dev-guide/debug-ipc.md](dev-guide/debug-ipc.md)). 서버 수신 경로는 로컬·원격 공용으로 보존되므로 "로컬 attach 제거"는 **클라이언트 로컬 진입점만** 제거한 것이다.
- **화면 스크래핑(정식)**: 단발 화면 읽기는 attach 세션이 아니라 `tasty read screen`(현재 화면) / `tasty read since-mark`(마크 이후 출력)를 쓴다. attach 의 `--dump-after` 는 mirror 검증용이다.
- **자동 매핑**: `tasty set workspace --id <id> --ssh-profile <name> --remote-workspace N`(또는 `--ssh <user@host>`)으로 로컬 워크스페이스 ↔ 원격 컴퓨터를 매핑하면, 그 워크스페이스를 활성화할 때 호스트가 자동으로 SSH 터널을 세워 원격 워크스페이스를 GUI mirror 로 띄운다(`src/app/auto_attach.rs`). SSH 연결 프로필(`~/.tasty/ssh-profiles.toml`)은 `tasty tool ssh add/list/show/edit/remove/detect` 로 관리한다(비밀번호 미저장 — identity_file/ssh-agent 위임, decision 5).
- **IPC**: `attach.acquire`/`release`(`stream.open{target}` 핸드셰이크) + `attach.force_detach`/`force_detach_workspace`/`into_gui`/`list`(JSON-RPC). `remote`/`debug attach` 는 IPC 네임스페이스가 아니라 이 `attach.*`(+`system.info`) 위의 CLI 디스패치 계층이다. 보안은 SSH + 127.0.0.1 loopback 위임(자체 토큰 없음).

### 지원 메서드

모든 서피스 관련 메서드는 optional `surface_id` 파라미터를 지원한다. 지정하면 해당 서피스에 직접 접근하고, 생략하면 현재 포커스된 서피스에 작용한다.

#### 시스템
- `system.info`: 버전, 워크스페이스 수, 활성 워크스페이스 인덱스

#### 디버그 전용 (debug 빌드에서만 사용 가능)
다음 메서드들은 `cfg(debug_assertions)` 게이트로 릴리즈 빌드에서 제외된다. 개발 및 테스트 용도로만 존재한다. release 빌드에서는 `src/ipc/method_meta.rs::DEBUG_METHODS`가 빈 슬라이스로 컴파일되어 `method_meta(name)` lookup이 `None`을 반환한다 (자세한 동작은 [dev-guide/debug-ipc.md](dev-guide/debug-ipc.md) 참조).

- `system.shutdown`: 테스트 종료 시 프로세스 정상 종료
- `ui.state`: GUI 오버레이 상태 조회 (settings_open, notification_panel_open, active_workspace, workspace_count, pane_count, tab_count)
- `ui.screenshot`: 현재 화면을 PNG로 저장
- `debug.info`: 개발용 디버그 정보 조회 (scale_factor, cell 크기, viewport 등). `src/debug_info.rs`를 수정하여 커스텀 정보 추가 가능
- `debug.cell_info`: 특정 셀(row, col)의 텍스트, 색상(fg/bg), 속성을 termwiz `CellAttributes` 단계에서 조회. 노출 필드: `text`, `fg`, `bg`, `bold`, `italic`, `underline`, `strikethrough`, `inverse`, `width`, `intensity` (`normal`/`bold`/`half`), `underline_style` (`none`/`single`/`double`/`curly`/`dotted`/`dashed`), `underline_color`, `blink` (`none`/`slow`/`rapid`), `invisible`, `overline`, `vertical_align` (`baseline`/`super`/`sub`)
- `debug.screen_attrs`: 특정 행 전체의 셀 속성을 일괄 조회 (필드 구성은 `cell_info`와 동일, `col` 추가)
- `debug.glyph_color`: 특정 셀에 대해 **렌더러가 GPU에 push하는 (bg, fg) RGBA**를 반환. `cell_info`가 termwiz 단계의 속성을 보여준다면 이쪽은 그 속성이 실제 색상 결정에 반영되었는지를 검증한다 (`bg_mode: "focused" | "unfocused"` 옵션)
- `debug.inject_mouse`: SGR 마우스 이벤트를 PTY에 주입 (`--enable-input-simulation` 필요)
- `debug.inject_key`: 임의 바이트/텍스트를 PTY에 주입 (`--enable-input-simulation` 필요)
- `--enable-input-simulation` CLI 플래그: debug 빌드에서 입력 시뮬레이션 IPC를 활성화. 이 플래그 없이는 inject_mouse/inject_key가 거부됨 (2단계 게이트: 컴파일 + 런타임)
- `debug` CLI 서브커맨드: `tasty debug info`, `tasty debug ime-*`, `tasty debug cell-info`, `tasty debug screen-attrs`, `tasty debug glyph-color`, `tasty debug attach`(로컬 loopback attach — 원격은 `tasty remote attach`) 등 디버그 관련 CLI 명령

#### 워크스페이스
- `workspace.list`: 전체 워크스페이스 목록 (이름, 활성 여부, 패인 수)
- `workspace.create`: 새 워크스페이스 생성 (선택적 이름, 타입 지정). `--type markdown --file <path>` 등으로 비터미널 워크스페이스 생성 가능
- `workspace.update`: 워크스페이스 이름, 부제, 설명 수정
- `workspace.move`: 워크스페이스 순서 이동 (`from_index`, `to_index`)

#### 윈도우
- `window.list`: 전체 윈도우 목록 (id, focused, title)
- `window.create`: 새 독립 윈도우 생성
- `window.close`: 포커스된 윈도우 닫기
- `window.focus`: 특정 윈도우에 포커스

#### 패인
- `pane.list`: 전체 워크스페이스의 패인 목록 (포커스 여부, 탭 수)
- `split`: 통합 분할 명령. `level`(pane/surface), `target_surface`(surface ID/nickname) 또는 `target_pane`(pane ID)으로 대상 지정, `direction`(vertical/horizontal), `type`(terminal/markdown/explorer + plugin contribute) 파라미터. pane/surface 레벨 모두 비터미널 타입 지원. 포커스 이동 없음
- `pane.close`: 패인 닫기 (unsplit)

#### 탭
- `tab.list`: 지정 패인의 탭 목록 (id, name, type, surface_id, active)
- `tab.create`: 지정 패인에 새 탭 추가
- `tab.close`: 탭 닫기
- `tab.move`: 탭 순서 이동 (`pane_id`, `from_index`, `to_index`)

#### 서피스
- `surface.list`: 전체 워크스페이스의 서피스 목록 (id, type, pane_id, tab_index, cols/rows). 비터미널 서피스(Markdown, Explorer, Html)도 포함
- `surface.close`: 서피스 닫기. cascade(surface → tab → pane → workspace)로 마지막 워크스페이스의 마지막 서피스까지 닫혀도 윈도우는 종료되지 않으며, 빈 워크스페이스가 새로 생성되어 invariant("열린 윈도우는 ≥1 workspace")가 유지된다 (에이전트가 사용자의 윈도우를 끄는 부작용 방지).
- `surface.close_self`: 호출한 서피스 자신을 닫기 (TASTY_SURFACE_ID 기반). `surface.close`와 동일한 cascade·자동 재생성 규칙 적용.

#### 입력
- `surface.send`: 텍스트 전송 (optional surface_id)
- `surface.send_key`: 특수키 전송 — enter, tab, escape, backspace, 방향키, home/end, pageup/pagedown, delete/insert, f1~f12 (optional surface_id)
- `surface.send_combo`: **키 조합 전송** — Ctrl+C (0x03), Ctrl+Z (0x1A), Ctrl+D (0x04), Alt+키 (ESC prefix) 등. 파라미터: `{key, modifiers: ["ctrl"|"shift"|"alt"], surface_id?}`
- `surface.send_to`: 특정 surface_id에 텍스트 직접 전송 (포커스 변경 없이)

#### 출력 읽기
- `surface.screen_text`: 화면 텍스트 조회 (optional surface_id)
- `surface.cursor_position`: 커서 위치 (x, y) 조회 (optional surface_id)
- `surface.set_mark`: 출력 읽기 마크 설정 (optional surface_id)
- `surface.read_since_mark`: 마크 이후 출력 텍스트 조회, ANSI 제거 옵션 (optional surface_id)

#### 타이핑 감지
- `surface.is_typing`: 서피스가 최근 5초 내 키 입력을 받았는지 조회. 반환: `{ typing: bool, idle_seconds: f64 }` (idle_seconds가 -1이면 입력 기록 없음). optional surface_id
- `surface.send_wait_idle`: 서피스가 유휴 상태일 때만 텍스트 전송. 타이핑 중이면 `{ sent: false, reason: "typing" }` 반환, 유휴면 전송 후 `{ sent: true }` 반환. CLI에서 폴링하여 대기 구현 가능. optional surface_id, 필수 text

#### 알림
- `notification.list`: 최근 50개 알림 목록
- `notification.create`: 알림 생성

#### 트리
- `tree`: 전체 워크스페이스/패인/탭 트리 구조 조회

#### 훅
- `hook.set`: 서피스 훅 등록 (event, command, once)
- `hook.list`: 등록된 훅 목록 조회 (서피스별 필터 가능)
- `hook.unset`: 훅 삭제

#### 글로벌 훅
- `global_hook.set`: 글로벌 훅 등록. 파라미터: `condition` (타입별 포맷), `command`, `label?`. 반환: `{ hook_id: N }`
  - `interval:SECS` — 매 N초마다 반복 실행
  - `once:SECS` — N초 후 1회 실행 후 자동 삭제
  - `file:/path` — 파일 수정 감지 시 실행
- `global_hook.list`: 등록된 글로벌 훅 전체 목록. 각 항목: `{ id, condition, command, label }`
- `global_hook.unset`: `hook_id`로 글로벌 훅 삭제. 반환: `{ removed: bool }`

#### 메시지 패싱
- `message.send`: `to_surface_id`, `content`, `from_surface_id?` — 다른 서피스의 메시지 큐에 메시지 추가. 응답: `{ id: N }`
- `message.read`: `surface_id?`, `from_surface_id?`, `peek?` — 메시지 큐 읽기. 기본적으로 소비(consume), `peek: true`이면 읽기만 하고 큐에서 제거하지 않음. `from_surface_id`로 발신자 필터 가능
- `message.count`: `surface_id?` — 대기 중인 메시지 수. 응답: `{ count: N }`
- `message.clear`: `surface_id?` — 메시지 큐 전체 삭제. 응답: `{ cleared: true }`

#### Surface 메타데이터
- `surface.meta_set`: `surface_id?`, `key`, `value` — 서피스별 메타데이터 키-값 설정. 응답: `{ ok: true }`
- `surface.meta_get`: `surface_id?`, `key` — 메타데이터 값 조회. 응답: `{ value: "..." }` 또는 `{ value: null }`
- `surface.meta_unset`: `surface_id?`, `key` — 메타데이터 키 삭제. 응답: `{ ok: true }`
- `surface.meta_list`: `surface_id?` — 전체 메타데이터 객체 반환

#### 에이전트 전용 (번들 plugin이 제공)
- `claude.launch`: Claude Code 전용 워크스페이스 생성 및 실행 — `com.tasty.claude` plugin이 등록
- `codex.launch`: Codex CLI 전용 워크스페이스 생성 및 실행 — `com.tasty.codex` plugin이 등록

### 멀티 윈도우
- `window.create` IPC 또는 `tasty new window` CLI로 새 독립 윈도우 생성
- 키바인딩: `new_window` (기본: Alt+Shift+N, macOS에서 Cmd+Shift+N)
- 각 윈도우는 자체 GPU 서피스, egui 컨텍스트, 터미널 세트를 보유
- `window.list`: 전체 윈도우 목록 (id, focused, title)
- `window.close`: 포커스된 윈도우 닫기
- `window.focus`: 특정 윈도우에 포커스
- 윈도우 닫기 시 HashMap에서 제거
- 마지막 윈도우 닫기: `close_behavior` 설정에 따라 동작 (ask/minimize/quit)
- Minimize 동작 플랫폼 분기:
  - macOS: 윈도우 파괴 + 모든 state를 parked_states에 보존 → dock 클릭으로 복원
  - Windows/Linux: `set_minimized(true)`로 태스크바에 유지 → 클릭으로 복원
- 멀티윈도우 minimize 시 모든 윈도우의 state를 보존 (Vec 기반)
- 모달 활성 시 다른 윈도우 입력 차단
- macOS: 커스텀 NSApplicationDelegate로 dock/메뉴 통합
  - dock 아이콘 클릭 시 윈도우가 없으면 자동 복원 (applicationShouldHandleReopen)
  - dock 우클릭 메뉴에 "New Window" 항목 (applicationDockMenu)
  - 앱 상단 메뉴바 File → New Window (Cmd+Shift+N)

### 앱 아이콘
- 커스텀 앱 아이콘 (수박 디자인) 적용
- **런타임 윈도우 아이콘**: 모든 윈도우(메인, 설정, 종료 확인)에 256x256 PNG를 winit `with_window_icon`으로 설정. Windows 태스크바, Linux WM 등에서 표시됨
- **Windows exe 아이콘**: `build.rs` + `winresource`로 `.ico`를 exe에 임베드. 탐색기, 작업 관리자에서 표시됨
- **Windows 트레이 아이콘**: 32x32 PNG에서 디코딩한 실제 아이콘 사용 (기존 하드코딩 사각형 대체)
- **macOS 번들**: `assets/macos/Info.plist` + `assets/icons/icon.icns` 제공. `.app` 번들 생성 시 사용
- **Linux 데스크탑**: `assets/linux/tasty.desktop` 엔트리 파일 + 다양한 크기의 PNG 아이콘 제공
- 아이콘 에셋: `assets/icons/` (icon.icns, icon.ico, icon_16~1024.png)

### GUI 통합 테스트 프레임워크
- `tests/gui_common/mod.rs`의 `GuiTestInstance` 헬퍼: 실제 GUI 모드로 프로세스 스폰
- `enigo` 크레이트로 키보드/마우스 입력 시뮬레이션 (Windows SendInput API)
- `windows` 크레이트로 창 탐색(FindWindowW) 및 포커스 전환(SetForegroundWindow)
- IPC `ui.state` 메서드로 GUI 오버레이 상태 검증
- `wait_for_ui()`: 조건 기반 UI 상태 폴링 (타임아웃 포함)
- `measure_ui_latency()`: UI 동작별 응답 속도 측정
- IPC Waker: IPC 명령 도착 시 `EventLoopProxy`를 통해 이벤트 루프 즉시 깨움
- 24개 GUI 테스트:
  - 설정창 열기/닫기 (Ctrl+,, Escape)
  - 알림 패널 토글 (Ctrl+Shift+I, Escape)
  - 워크스페이스 생성/전환 (Ctrl+Shift+N, Alt+1~9)
  - 탭 생성/닫기 (Ctrl+Shift+T, Ctrl+W)
  - 패인 분할/닫기 (Ctrl+Shift+E/O/W)
  - 키보드 라우팅: 오버레이 열림 시 터미널 입력 차단 검증
  - 키보드 라우팅: 오버레이 없을 때 터미널 입력 전달 검증
  - 전체 워크플로우: 워크스페이스→패인→탭 CRUD 통합 시나리오
  - 속도 테스트: 설정 토글, 워크스페이스 전환, 탭 전환 반복 측정 (1초 이내 응답 보장)

### CLI 클라이언트 (cli.rs)
- `tasty` 명령에 서브커맨드가 있으면 CLI 모드, 없으면 GUI 모드로 동작
- clap 기반 그룹형 서브커맨드: `new`, `close`, `list`, `set`, `send`, `read`, `unset`, `notify`, `surface-meta`, `is-typing`, `debug` (`claude`, `codex` 등은 번들 plugin이 자체적으로 등록)
- 포트 파일에서 포트 번호를 읽어 TCP 연결 후 JSON-RPC 요청/응답
- `list tree` 커맨드: 워크스페이스/패인/탭 계층을 트리 형태로 표시 (ID, surface type 포함)
- `list tabs --pane ID` 커맨드: 지정 패인의 탭 목록 조회 (id, name, type, surface_id)
- 에러 시 종료 코드 1 반환

#### 포커스 독립 원칙
- 모든 CLI/IPC 명령은 focus에 의존하지 않고 명시적 ID로 대상을 지정
- 생성/삭제 명령: `--pane`, `--surface`, `--target` 등이 필수. 미지정 시 에러 + 사용법 안내
- 동작 명령 (`send`, `read`, `set` 등): `--surface` 미지정 시 `TASTY_SURFACE_ID` 환경변수에서 자동 채움
- 모든 응답에 실제 적용된 파라미터 값(surface_id, pane_id 등)을 포함하여 묵시적 기본값도 확인 가능
