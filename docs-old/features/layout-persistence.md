# 레이아웃 영속화

- **Status**: Implemented

### 개요
- 설정 (`general.restore_layout`, 기본 off) 활성화 시 워크스페이스/페인/탭/서피스 구조를 `~/.tasty/layout.json`에 JSON으로 저장
- 앱 시작 시 저장된 레이아웃을 복원하여 이전 세션의 창 배치를 재현

### 저장 대상
- 워크스페이스 목록 (이름, 부제, 설명)
- 페인 트리 구조 (split direction, ratio)
- 탭 목록 (이름, explicit_name, active_tab)
- 서피스 레이아웃 트리 (split direction, ratio)
- 각 서피스의 타입별 최소 정보:
  - Terminal: cwd, `restore.command`(있을 때), `scrollback_ref`(설정 `restore_terminal_content` on 일 때)
  - Markdown: file path
  - Explorer: root path
  - Html: url
  - Image: file path
  - Empty: (없음)
- 활성 워크스페이스 인덱스, 포커스된 페인 인덱스

### 저장 타이밍
- 구조적 변경 시 dirty flag 설정 + 500ms 디바운스
- 대상 이벤트: 워크스페이스/페인/탭/서피스 추가·삭제·분할, 이름 변경, split ratio 드래그, 서피스 타입 변환, 닫힌 항목 복원
- 앱 종료 시 dirty 상태면 즉시 flush

### 복원 시점
- 앱 시작 시 1회만 (`EngineState::new()`)
- `layout.json` 파싱 실패 또는 파일 없음 시 기본 "Workspace 1"로 폴백
- 개별 서피스 복원 실패 시 해당 서피스만 스킵하고 나머지 계속 복원

### TUI 세션 복원

Claude Code 등 TUI 앱이 실행 중이던 터미널을 복원할 때, 해당 앱의 세션을 자동으로 재개한다.

- `tasty claude install`로 `SessionStart`/`SessionEnd` hook이 등록되면, plugin이 세션 시작 시
  `restore.command` (예: `claude -r <session-id>`) 및 `claude-session-id` 메타키를 surface 메타데이터에
  set/unset 한다. 호스트는 agent-agnostic하게 `restore.command` 값만 읽어 복원에 사용한다.
- Claude Code 의 hook 시스템은 hook 명령 실행 시 **stdin 으로 JSON payload**(`session_id`, `message` 등)를
  전달한다. `tasty claude hook` CLI 가 stdin JSON 을 자동으로 파싱해 `session_id` → `--session`,
  `message` → `--message` 로 채우므로, settings.json 에 등록되는 명령은 어떤 event 든 `tasty claude
  hook <event>` 형태로 단순하다. (옛 버전이 `--session ${CLAUDE_SESSION_ID}` 쉘 확장에 기대다 실패하던
  회귀가 있어, 재 `tasty claude install` 시 옛 entry 는 자동으로 갱신된다.)
- plugin이 비활성화되거나 `tasty claude install` 없이 사용해도 오류 없이 일반 셸로 복원됨
- 다른 agent plugin도 `restore.command` 메타키만 set하면 동일한 메커니즘으로 자체 세션 복원을 지원할 수 있음

복원이 발동하는 경로:
1. **앱 재시작 (레이아웃 복원)**: `restore_layout` 설정 활성화 시, 레이아웃 저장 시점에 세션 ID가 있는 터미널은 `restore_command`를 함께 저장. 복원 시 셸 초기화 후 자동 실행
2. **닫힌 항목 복원 (Ctrl+Shift+T)**: surface/tab/workspace 닫기 시 `ClosedSurface`에 `restore_command`를 포함하여 스냅샷. 복원 시 셸 시작 후 `restore_command` 자동 실행

명령 주입 타이밍은 PTY 가 spawn 되는 그 순간이다. `TerminalConfig.initial_input` 으로 `Terminal::new` 에 명령을 넘기면, writer thread 가 시작되기 전에 PTY master fd 에 동기적으로 write_all + flush 된다. 따라서 child shell 이 stdin 을 처음 read 하는 순간 무조건 이 바이트가 첫 입력으로 들어가, GUI redraw / BusyPoll / 워크스페이스 전환 등 추가 트리거 없이 spawn 과 동시에 실행된다. deferred 경로는 `DeferredSpawn.restore_command` 가 `ensure_initialized` 시 `initial_input` 으로 변환되어 동일하게 처리된다.

### 터미널 내용 복원 (scrollback)

설정 `general.restore_terminal_content` (기본 on) 가 활성화되어 있으면 레이아웃 저장 시 각 터미널의 scrollback (위로 스크롤 가능한 출력 히스토리) **과 현재 화면 (visible) 라인** 이 함께 보존되어, 앱 재시작 후에도 이전 출력을 그대로 볼 수 있다. 화면 라인은 scrollback 뒤에 이어 붙여 저장하므로 복원 후 위로 스크롤하면 [이전 scrollback → 이전 화면 → 새 prompt] 순으로 보인다 (trailing blank row 는 capture 시 trim).

- 저장 위치: `~/.tasty/scrollback/<persist_id>.bin` — `layout.json` 의 `Terminal.scrollback_ref` 가 이 파일을 가리킨다. 직렬화 포맷은 메모리 ↔ 디스크 swap 에 쓰이는 것과 동일 (magic `TSSB`, version 2, line-by-line records).
- `persist_id` 는 surface 가 처음 capture 될 때 발급되어 `surface-meta` (`scrollback.persist_id`) 에 보관된다. 다음 capture 가 같은 surface 면 같은 파일을 atomic 하게 덮어쓰므로 orphan 누적이 없다.
- 옵션 OFF: capture 단계에서 디스크 쓰기를 스킵하고, restore 단계에서도 `scrollback_ref` 가 있어도 무시한다.
- 옵션 ON → OFF 전환: Settings Save 시점에 `~/.tasty/scrollback/` 전체를 비운다 (사용자가 "더 이상 안 쓴다" 라고 명시한 상태).
- 비활성 워크스페이스 / 다른 탭의 deferred 터미널은 PTY 가 실제로 spawn 되는 시점 (`ensure_active_workspace_initialized` / `ensure_surface_initialized`) 에 큐에서 꺼내 inject 된다.
- 닫힌 항목 복원 (`Ctrl+Shift+T`) 의 scrollback 은 **옵션과 무관하게 항상** 복원된다 (메모리에 들고 있다가 즉시 재사용; 디스크 쓰기 없음).

#### Lifecycle / 정리

- Surface 닫힘: `surface-meta` 정리 직전에 `scrollback.persist_id` 를 회수해 `~/.tasty/scrollback/<id>.bin` 도 함께 삭제.
- 앱 시작: `layout.json` 의 모든 `scrollback_ref` 집합을 기준으로 디렉터리를 스캔해 알려지지 않은 `.bin` 파일을 일괄 삭제 (capture 도중 크래시 잔재 정리).
- 옵션 OFF 전환: 디렉터리 전체 삭제.
- Atomic rename (`<id>.bin.tmp` → `<id>.bin`) 로 부분 쓰기 잔재를 방지.

### 저장하지 않는 것
- 화면 내용 (visible cells) — scrollback 만 보존하며 현재 화면은 새 셸 프롬프트로 채워진다.
- PTY 상태, 환경변수, 실행 중인 명령
- 팝업 상태
