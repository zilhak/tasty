# Tasty API 레퍼런스 — IPC/CLI 공통

## 접속 방법

Tasty는 TCP 기반 JSON-RPC 2.0 서버를 내장하고 있다.

- **주소**: `127.0.0.1:<동적포트>`
- **포트 파일**: `~/.tasty/tasty.port` (Tasty 실행 시 생성, 종료 시 삭제)

### Python 접속 예시

```python
import socket, json, os

port = int(open(os.path.expanduser("~/.tasty/tasty.port")).read().strip())
s = socket.socket()
s.settimeout(5)
s.connect(('127.0.0.1', port))

def call(method, params=None):
    req = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params or {}}
    s.sendall((json.dumps(req) + '\n').encode())
    return json.loads(s.recv(65536).decode())
```

### JSON-RPC 포맷

```json
{"jsonrpc": "2.0", "id": 1, "method": "메서드명", "params": {}}
```

응답:
```json
{"jsonrpc": "2.0", "id": 1, "result": { ... }}
```

## CLI 커맨드

```bash
# GUI 실행
tasty

# 시스템
tasty list info               # 버전, 워크스페이스 수

# 워크스페이스
tasty list workspaces         # 워크스페이스 목록
tasty new workspace [--name NAME] [--cwd PATH] [--type terminal|markdown|explorer|html|image] [--file PATH] [--path DIR] [--url URL]
tasty set workspace --id ID [--name NAME] [--subtitle TEXT] [--description TEXT]

# 윈도우
tasty list windows            # 윈도우 목록
tasty new window              # 새 윈도우 생성

# 패인/탭
tasty list panes              # 패인 목록
tasty list tabs --pane ID     # 탭 목록 (id, name, type, surface_id)
tasty split --level pane|surface --target-surface ID|this|nickname [--direction vertical|horizontal] [--type terminal|markdown|explorer|html|image] [--cwd PATH] [--file PATH] [--path DIR] [--url URL] [--meta JSON]
tasty split --level pane --target-pane PANE_ID [--direction vertical|horizontal] [--type ...] [...]
tasty new tab --pane ID [--type terminal|markdown|explorer|html|image] [--cwd PATH] [--file PATH] [--path DIR] [--url URL]
tasty close pane --pane ID
tasty close tab --tab ID

# 서피스 (Surface)
tasty list surfaces           # 서피스 목록 (id, type, cols, rows, pty_ready — 비터미널 포함)
tasty send text "ls -la\r" [--surface ID]   # 텍스트 전송 (\r = Enter). deferred 자동 wake
tasty send key enter [--surface ID]         # 키 전송. deferred 자동 wake
tasty wake [--surface ID]                   # deferred 터미널의 PTY를 명시적으로 spawn (입력 없이)
tasty set mark [--surface ID]               # 출력 마크 설정
tasty read since-mark [--surface ID] [--strip-ansi]  # 마크 이후 출력 읽기
tasty read screen [--surface ID]            # 현재 화면 텍스트 읽기
tasty close surface --surface ID
tasty close self                            # 자기 자신 닫기 (TASTY_SURFACE_ID 사용)

# 추가 Surface 타입 (pane/surface 레벨 모두 지원)
tasty new tab --pane <ID> --type markdown --file /path/to/file.md
tasty new tab --pane <ID> --type explorer [--path /dir]
tasty split --level pane --target-surface this --type markdown --file /path/to/file.md
tasty split --level surface --target-surface this --type markdown --file /path/to/file.md
tasty split --level pane --target-pane <ID> --type explorer [--path /dir]

# IME 시뮬레이션 (debug 서브커맨드)
tasty debug ime-enable             # IME 활성화
tasty debug ime-preedit "ㅎ"        # 조합 중 텍스트 표시
tasty debug ime-commit "한"         # 확정 → PTY 전송
tasty debug ime-status             # 현재 IME 상태 확인
tasty debug ime-disable            # IME 비활성화 + preedit 클리어
tasty debug info                   # 디버그 정보 조회

# 이동/순서 변경 (인덱스는 0-based)
tasty move tab --pane PANE_ID --from FROM_IDX --to TO_IDX      # 같은 pane 안에서 탭 순서 변경
tasty move workspace --from FROM_IDX --to TO_IDX               # 워크스페이스 순서 변경

# 도구 (내장 tool, 번들 plugin이 등록)
tasty tool clipboard list                                      # 클립보드 히스토리 항목 목록
tasty tool clipboard get --index N                             # 특정 항목 조회
tasty tool clipboard paste --index N [--surface ID]            # 특정 항목 붙여넣기
tasty tool clipboard remove --index N                          # 항목 삭제
tasty tool clipboard clear                                     # 전체 비우기

# Plugin 관리
tasty plugin list                                              # 설치된 plugin 목록
tasty plugin show --id ID                                      # plugin 상세
tasty plugin install --path /path/to/plugin/dir                # 매니페스트가 있는 디렉터리 설치
tasty plugin remove --id ID
tasty plugin enable --id ID
tasty plugin disable --id ID
tasty plugin logs --id ID [--follow]
tasty plugin permissions --id ID                               # 매니페스트 권한 + granted
tasty plugin grant --id ID --permission TOKEN
tasty plugin revoke --id ID --permission TOKEN
tasty plugin extension list                                    # plugin이 등록한 extension 포인트

# 이미지 (com.tasty.image plugin)
tasty image open <path> [--surface ID]
tasty image save [--surface ID]
tasty image export --format png --out <path>
tasty image next [--surface ID]                                # 폴더 내 다음 이미지
tasty image prev [--surface ID]
tasty image paste                                              # 클립보드 이미지 붙여넣기
tasty image list

# 훅
tasty set hook --event process-exit --command "echo done"
tasty list hooks
tasty unset hook --hook HOOK_ID

# 알림
tasty notify "메시지" [--title "제목"]
tasty list notifications

# 트리 (전체 구조 출력)
tasty list tree

# 메시지 패싱
tasty send queue --to SURFACE_ID "내용"             # 큐에 메시지 전송
tasty read queue [--surface ID] [--from ID] [--peek]  # 큐에서 메시지 읽기 (기본: 소비)
tasty list queue [--surface ID]                     # 큐 상태 확인 (count + 미리보기)
tasty read queue --clear [--surface ID]             # 큐 비우기

# Claude
tasty claude install              # ~/.claude/settings.json에 tasty Stop 훅 등록 (wait/이벤트 훅 사용 전 필수)
tasty claude uninstall            # 등록된 tasty Stop 훅 제거
tasty claude launch [--workspace NAME] [--directory PATH] [--task "설명"]
tasty claude spawn [--surface ID] [--direction vertical|horizontal] [--cwd PATH] [--role ROLE] [--nickname NAME] [--prompt "TEXT"]
# --surface: pane 분할 위치 (기본: TASTY_SURFACE_ID). parent는 항상 명령을 실행한 surface.
tasty claude children             # 자식 Claude 목록
tasty claude parent               # 부모 Claude 조회
tasty claude kill --child INDEX      # 자식 Claude 종료 (child index 지정)
tasty claude respawn --child INDEX [--cwd PATH] [--role ROLE] [--nickname NAME] [--prompt "TEXT"]
tasty claude broadcast "텍스트\r" [--role ROLE]   # 모든 자식에 텍스트 전송
tasty claude wait --child INDEX [--timeout 60]       # idle/needs_input/exited 대기 (사전 요구: tasty claude install)

# Claude Hook 통합 (Claude Code의 훅 시스템에서 호출)
tasty claude install                # ~/.claude/settings.json에 Stop/Notification/SessionEnd/SubagentStop/SessionStart 5종 등록 (idempotent)
tasty claude uninstall              # ~/.claude/settings.json에서 위 5종 제거 (사용자 entry는 보존)
tasty claude hook stop              # Claude 작업 완료 → idle 상태 설정 + claude-idle 훅 실행
tasty claude hook session-end       # 세션 종료 → idle 상태 + claude-idle 훅 + 세션 메타 삭제
tasty claude hook subagent-stop     # Task subagent 종료 → idle 상태 + claude-idle 훅 실행
tasty claude hook notification      # Claude 입력 필요 → needs-input 상태 설정 + needs-input 훅 실행
tasty claude hook prompt-submit     # 사용자 입력 전송 → active 상태로 전환
tasty claude hook session-start --session <UUID>  # 세션 시작 → active 상태 + 세션 ID를 surface 메타에 저장 (레이아웃 복원 시 사용)
tasty claude hook stop --surface 5  # 특정 surface 지정 (또는 TASTY_SURFACE_ID 환경변수)
```

## IPC 메서드 레퍼런스

### 시스템

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `system.info` | 없음 | 버전, 워크스페이스 수 |

### 디버그 전용 (debug 빌드에서만 사용 가능)

다음 메서드들은 릴리즈 빌드에 포함되지 않는다. `cargo build` (debug)에서만 사용 가능.
상세는 `dev-guide/debug-ipc.md` 참조.

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `system.shutdown` | 없음 | 테스트 종료 시 프로세스 정상 종료 |
| `ui.state` | 없음 | GUI 내부 상태 조회 (설정창/알림패널 열림 여부, 패인 수 등) |
| `ui.screenshot` | `path?: string` | 스크린샷을 PNG로 저장 (GUI 모드 전용, 비동기) |
| `debug.info` | 없음 | 디버그 정보 조회 (scale_factor, cell 크기, viewport 등) |
| `debug.cell_info` | `surface_id, col, row` | 특정 셀의 문자/속성/색상 조회 |
| `debug.screen_attrs` | `surface_id` | 화면 전체 SGR 속성 매트릭스 조회 |
| `debug.glyph_color` | `surface_id, x, y` | 픽셀 좌표의 글리프 RGBA 색 추출 |
| `debug.feed_bytes` | `surface_id, bytes` | PTY 출력 시뮬레이션 (VTE에 직접 주입) |
| `debug.inject_key` | `key, modifiers[], surface_id?` | 사용자 키 입력 재현 |
| `debug.inject_mouse` | `surface_id, x, y, button, kind` | 사용자 마우스 입력 재현 |
| `debug.tool.list` | 없음 | 등록된 tool entry 전체 목록 |
| `debug.tool.invoke` | `tool_id, params?` | tool 직접 호출 (권한 우회) |
| `debug.popup.list` | 없음 | 현재 열려있는 popup 목록 |
| `debug.popup.open` | `popup_id, params?` | popup 트리거 |
| `debug.popup.close` | `popup_id?` | popup 닫기 (생략 시 전체) |
| `debug.extension.invoke_hook` | `extension_id, event, params?` | plugin extension hook 직접 호출 |
| `debug.event_bus.list_subscribers` | 없음 | 이벤트 버스 구독자 dump |
| `debug.event_bus.publish` | `topic, payload` | 이벤트 버스에 강제 발행 |
| `debug.event_bus.trace` | `enable: bool` | 이벤트 흐름 trace on/off |

### 윈도우

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `window.list` | 없음 | 전체 윈도우 목록 (id, focused, title) |
| `window.create` | 없음 | 새 독립 윈도우 생성 |
| `window.close` | 없음 | 포커스된 윈도우 닫기 |
| `window.focus` | `id: string` | 특정 윈도우에 포커스 |

### 워크스페이스

> `cwd` 파라미터를 생략하면, `general.inherit_cwd` 설정이 켜져 있을 때 호출 시점의 source surface(분할 대상 또는 포커스된 surface)에서 cwd를 상속한다. 자세한 매트릭스는 `docs/design/split-command.md` "cwd 결정 정책" 참조.

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `workspace.list` | 없음 | 전체 워크스페이스 목록 (id, name, subtitle, description, active, pane_count, busy_count) |
| `workspace.create` | `name?, subtitle?, description?, cwd?` | 새 워크스페이스 생성 후 활성화 |
| `workspace.update` | `index?\|id?, name?, subtitle?, description?` | 워크스페이스 정보 수정 (생략 시 활성 워크스페이스) |
| `workspace.move` | `from: number, to: number` | 워크스페이스 순서 변경 (0-based 인덱스) |
| `tree` | 없음 | 전체 계층 구조 (워크스페이스 → 패인 → 탭). 모든 노드에 `busy_count`, surface 리프에 `busy` 부여. 터미널 리프에는 `pty_ready` 플래그가 붙는다 (deferred 복원 상태이면 `false`). |

### 패인

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `pane.list` | 없음 | 전체 워크스페이스의 패인 목록 (workspace_id, workspace_name 포함) |
| `split` | `level`, `target_surface?\|target_pane?`, `direction?`, `type?`, `cwd?`, `file?`, `path?`, `url?`, `meta?` | 분할. level: pane/surface. target_surface: surface ID/nickname, target_pane: pane ID (둘 중 하나 필수). type: terminal(기본)/markdown/explorer/html/image. surface 레벨에서도 비터미널 타입 지원 |
| `pane.close` | `pane_id` | 패인 닫기 |

### 탭

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `tab.list` | `pane_id` | 지정 패인의 탭 목록 (id, name, type, surface_id, active, busy_count) |
| `tab.create` | `pane_id`, `type?`, `cwd?`, `file?`, `path?`, `url?` | 새 탭 생성. type: terminal(기본)/markdown/explorer/html/image |
| `tab.close` | `tab_id` | 탭 닫기 |
| `tab.move` | `pane_id, from: number, to: number` | 같은 pane 내에서 탭 순서 변경 (0-based) |

### Surface (터미널 상호작용)

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `surface.list` | 없음 | 모든 서피스 목록 (id, pane_id, workspace_id, tab_index, type, cols, rows, busy, foreground_process?, foreground_pid?, **pty_ready**?). 터미널 서피스에는 `pty_ready: true`, 레이아웃 복원으로 만들어졌지만 아직 PTY가 spawn되지 않은 deferred 터미널에는 `pty_ready: false`가 표시된다. |
| `surface.send` | `text, surface_id` | 텍스트 전송. `\r`로 Enter. 대상이 deferred 상태면 **자동으로 PTY를 spawn**한 뒤 전송한다. |
| `surface.send_to` | `text, surface_id` | 특정 서피스에 텍스트 전송. deferred 자동 wake. |
| `surface.send_key` | `key, surface_id` | 키 이름 전송 (enter, tab, escape, up, down 등). deferred 자동 wake. |
| `surface.send_combo` | `key, modifiers[], surface_id` | 수정자 키 조합 전송 (Ctrl+C 등). deferred 자동 wake. |
| `surface.wake` | `surface_id` | deferred 터미널의 PTY를 명시적으로 spawn. 반환: `{ woke, pty_ready, surface_id }`. 이미 살아있거나 deferred가 아니면 `woke: false`. send 계열은 자동 wake되므로 이 메서드는 "PTY는 띄우되 아직 아무 명령도 보내고 싶지 않을 때"만 필요하다. |
| `surface.close` | `surface_id` | 서피스 닫기 |
| `surface.close_self` | `surface_id` | 호출한 서피스 자신을 닫기 |
| `surface.screen_text` | `surface_id` | 현재 화면의 텍스트 반환 |
| `surface.set_mark` | `surface_id` | 현재 출력 위치에 마크 설정 |
| `surface.read_since_mark` | `surface_id, strip_ansi?: bool` | 마크 이후 새 출력 반환 |
| `surface.cursor_position` | `surface_id` | 커서 위치 (x, y) 반환 |
| `surface.is_typing` | `surface_id` | 최근 5초 내 키 입력 여부. 반환: `{ typing, idle_seconds }` |
| `surface.send_wait_idle` | `surface_id, text` | 유휴 시에만 텍스트 전송. 타이핑 중이면 `{ sent: false }`. deferred 자동 wake. |
| `surface.foreground_process` | `surface_id` | 현재 PTY foreground 프로세스 이름/pid. 플랫폼별 감지(macOS: `process_pid_path`, Linux: `/proc/.../stat`, Windows: WMI) |
| `surface.locate` | `surface_id` | surface가 속한 window_id, workspace_id, pane_id, tab_id, tab_index를 한 번에 반환 |
| `surface.respawn_terminal` | `surface_id, cwd?: string, command?: string[]` | 죽거나 살아있는 PTY를 종료 후 새 PTY로 교체 (같은 surface 유지) |
| `surface.switch_input_source` | `surface_id, source: string` | OS 입력 소스 전환 (macOS 전용). source는 입력 소스 식별자 |
| `surface.raw_key` | `surface_id, key, modifiers[], press: bool` | winit KeyEvent를 직접 dispatch (macOS 전용 — 사용자 입력 재현 용도) |

### IME 시뮬레이션

AI 에이전트가 한글/CJK IME 입력을 프로그래밍 방식으로 시뮬레이션할 수 있다. 실제 winit IME 이벤트와 동일한 코드 경로를 타므로 preedit 렌더링, 커서 위치 보정 등을 완전히 테스트할 수 있다.

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `surface.ime_enable` | 없음 | IME 활성화. 이후 KeyboardInput의 비ASCII 텍스트가 무시됨 |
| `surface.ime_disable` | 없음 | IME 비활성화 + preedit 클리어 |
| `surface.ime_preedit` | `text: string`, `cursor?: number` | 조합 중 텍스트 표시. 빈 문자열이면 preedit 클리어 |
| `surface.ime_commit` | `text: string` | 조합 확정. 텍스트를 PTY로 전송하고 preedit 클리어 |
| `surface.ime_status` | 없음 | 현재 IME 상태 조회. `{ active, preedit_text, has_preedit }` |

**CLI:**

```bash
tasty debug ime-enable             # IME 활성화
tasty debug ime-preedit "ㅎ"        # preedit 표시
tasty debug ime-preedit "하"        # 모음 조합
tasty debug ime-preedit "한"        # 받침 조합
tasty debug ime-commit "한"         # 확정 → PTY 전송
tasty debug ime-status             # 상태 확인
tasty debug ime-disable            # IME 비활성화
```

**한글 입력 시뮬레이션 전체 흐름:**

```bash
tasty debug ime-enable
# "한글" 입력
tasty debug ime-preedit "ㅎ"
tasty debug ime-preedit "하"
tasty debug ime-preedit "한"
tasty debug ime-commit "한"
tasty debug ime-preedit "ㄱ"
tasty debug ime-preedit "그"
tasty debug ime-preedit "글"
tasty debug ime-commit "글"
tasty debug ime-disable
```

### Surface 메타데이터

파일 기반 키-값 스토어. 어떤 프로세스(Claude Code 포함)든 서피스별 메타데이터를 읽고 쓸 수 있다.

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `surface.meta.set` | `surface_id?, key: string, value: string` | 키-값 설정. 응답: `{ ok: true }` |
| `surface.meta.get` | `surface_id?, key: string` | 값 조회. 응답: `{ value: "..." }` 또는 `{ value: null }` |
| `surface.meta.unset` | `surface_id?, key: string` | 키 삭제. 응답: `{ ok: true }` |
| `surface.meta.list` | `surface_id?` | 전체 메타데이터 객체 반환 |

> **Deprecated alias**: 옛 이름 `surface.meta_set` / `meta_get` / `meta_unset` / `meta_list`(underscore 합성)는 호스트가 자동 정규화하지만 `tracing::warn`이 출력된다. **1.0 tag 직전에 일괄 제거**되므로 새 호출자는 점 표기(`surface.meta.*`)를 사용한다.

**CLI 사용 예시:**

```bash
tasty surface-meta set --key role --value orchestrator
tasty surface-meta get --key role
tasty surface-meta unset --key role
tasty surface-meta list
tasty surface-meta list --surface 3   # 특정 서피스 지정
```

### 훅

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `hook.set` | `event, command, surface_id?, once?` | 이벤트 훅 등록 |
| `hook.list` | `surface_id?` | 등록된 훅 목록 |
| `hook.unset` | `hook_id` | 훅 제거 |

**훅 이벤트 타입**:

| 이벤트 | 발동 조건 |
|--------|----------|
| `process-exit` | 자식 프로세스 종료 |
| `bell` | BEL 문자(`\x07`) 수신 |
| `notification` | OSC 알림 시퀀스 수신 |
| `output-match:PATTERN` | 출력이 정규식에 매칭 |
| `idle-timeout:SECS` | N초간 출력 없음 |
| `claude-idle` | Claude Code 작업 완료 (idle 상태 전환) |
| `needs-input` | Claude Code 사용자 입력 필요 |
| `claude-error` | Claude child PTY가 알려진 비정상 패턴(API Error, content filter, rate limit, network error 등)을 출력. `claude.spawn`/`claude.launch` 자식 surface에서 자동 감시되며, 사용자/에이전트도 추가 hook을 걸 수 있다. |

### 글로벌 훅 (타이머 / 파일 감시)

서피스에 종속되지 않는 전역 훅. 타이머나 파일 변경을 조건으로 셸 명령을 실행한다.

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `global_hook.set` | `condition, command, label?` | 글로벌 훅 등록. 반환: `{ hook_id }` |
| `global_hook.list` | 없음 | 등록된 글로벌 훅 목록 |
| `global_hook.unset` | `hook_id` | 글로벌 훅 제거. 반환: `{ removed }` |

**condition 포맷**:

| 형식 | 설명 |
|------|------|
| `interval:SECS` | 매 N초마다 반복 실행 |
| `once:SECS` | N초 후 1회 실행 후 자동 삭제 |
| `file:/path/to/watch` | 파일 수정 시 실행 |

**CLI**:

```bash
tasty set global-hook --condition interval:30 --command "echo tick" --label "heartbeat"
tasty set global-hook --condition once:5 --command "notify-send done"
tasty set global-hook --condition "file:/tmp/trigger" --command "bash /tmp/trigger"
tasty list global-hooks
tasty unset global-hook --hook HOOK_ID
```

### 알림

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `notification.list` | 없음 | 최근 50개 알림 |
| `notification.create` | `workspace_id` 또는 `surface_id`, `title?, body?` | 알림 생성. **포커스 독립**: 알림이 붙을 워크스페이스를 명시해야 한다. `surface_id`가 주어지면 그 surface 소속 워크스페이스로 자동 라우팅. 워크스페이스가 1개뿐일 때만 둘 다 생략 가능 (호환성 폴백, 향후 제거 예정) |

### 메시지 패싱 (Surface 간 통신)

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `message.send` | `to_surface_id: number, content: string, from_surface_id?: number` | 다른 서피스에 메시지 전송. 응답: `{ id: N }` |
| `message.read` | `surface_id?: number, from_surface_id?: number, peek?: bool` | 메시지 읽기 (기본: 소비). `peek: true`이면 큐에서 제거하지 않음. `from_surface_id`로 발신자 필터 가능 |
| `message.count` | `surface_id?: number` | 대기 중인 메시지 수. 응답: `{ count: N }` |
| `message.clear` | `surface_id?: number` | 메시지 큐 전체 삭제. 응답: `{ cleared: true }` |

### Plugin 관리

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `plugin.list` | 없음 | 설치된 plugin 목록 (id, name, version, enabled, running, surface_kinds, log_path) |
| `plugin.install` | `path: string` | path는 `tasty-plugin.toml`이 있는 디렉터리. 매니페스트 검증 후 `~/.tasty/plugins/<id>/`로 재귀 복사하고 활성화되어 있으면 즉시 spawn |
| `plugin.remove` | `id: string` | 살아있는 process를 graceful shutdown 후 plugin 디렉터리 삭제 |
| `plugin.enable` | `id: string` | plugin 활성화 + 즉시 spawn (이전에 비활성화되어 있던 plugin) |
| `plugin.disable` | `id: string` | plugin 비활성화 + 살아있으면 graceful shutdown |
| `plugin.show` | `id: string` | 단일 plugin의 매니페스트 + 런타임 상태 dump |
| `plugin.permissions` | `id: string` | `{id, manifest:[...], granted:[...]}` 매니페스트 권한 + 현재 grant된 권한 |
| `plugin.grant` | `id: string, permission: string` | 매니페스트에 선언된 권한 토큰을 granted에 추가. `ipc.invoke:<prefix>` 형식의 토큰은 다른 plugin의 namespace 호출 허용 |
| `plugin.revoke` | `id: string, permission: string` | granted에서 권한 제거 |
| `plugin.extension.list` | 없음 | plugin이 등록한 extension 포인트 전체 목록 |

상세는 `plugins.md` (권한 토큰 매핑 표 포함) 참조.

#### Plugin contributed CLI / IPC

설치된 plugin이 매니페스트에 `[[contributes.cli]]`를 선언했다면
`tasty <plugin-name> <subcommand>` 형태로 사용 가능. 사용 가능한 명령은
`tasty --help`에서 확인한다 (정적 호스트 명령에 이어 plugin 명령이 표시된다).

마찬가지로 plugin이 `[[contributes.ipc_namespace]]`를 선언했다면 `<prefix>.<method>`
IPC 메서드를 직접 호출할 수 있다 — 호스트는 메서드 이름과 params를 그대로
plugin으로 forward하므로, 실제 시그니처/응답은 각 plugin의 문서를 참조해야 한다.

### 클립보드 히스토리 (com.tasty.clipboard-history)

번들 plugin이 등록하는 도구. `tool.clipboard.*` namespace.

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `tool.clipboard.list` | 없음 | 히스토리 항목 목록 (`{ index, preview, kind, ts }`) |
| `tool.clipboard.get` | `index: number` | 항목 raw 데이터 조회 |
| `tool.clipboard.paste` | `index: number, surface_id?` | 대상 surface에 항목 붙여넣기 |
| `tool.clipboard.remove` | `index: number` | 특정 항목 삭제 |
| `tool.clipboard.clear` | 없음 | 히스토리 전체 비우기 |

### 이미지 (com.tasty.image)

번들 plugin이 등록하는 surface kind. `image.*` namespace는 plugin이 자체 forward한다.

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `image.open` | `path: string, surface_id?` | 이미지 파일 로드 |
| `image.save` | `surface_id?` | 현재 편집 내용을 원본 위치에 저장 |
| `image.export_png` | `surface_id?, out: string` | PNG로 export |
| `image.next` | `surface_id?` | 같은 폴더의 다음 이미지로 이동 |
| `image.prev` | `surface_id?` | 이전 이미지 |
| `image.paste` | `surface_id?` | 클립보드 이미지 붙여넣기 |
| `image.list` | 없음 | 열려있는 image surface 목록 |

### Claude 전용

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `claude.launch` | `workspace?, directory?, task?` | 새 워크스페이스에서 Claude 실행 |
| `claude.spawn` | `surface_id?, caller_surface_id?, direction?, cwd?, role?, nickname?, prompt?` | 대상 surface의 pane을 분할하여 자식 Claude 인스턴스 생성. `surface_id`는 분할 위치, `caller_surface_id`는 parent (CLI에서 자동 설정) |
| `claude.children` | `surface_id?` | 부모 surface의 자식 목록 조회 |
| `claude.parent` | `surface_id?` | 자식 surface의 부모 조회 |
| `claude.kill` | `surface_id?, child_index: number` | 자식 Claude 인스턴스 종료. child_index는 spawn 시 반환된 인덱스 |
| `claude.respawn` | `surface_id?, child_index: number, cwd?, role?, nickname?, prompt?` | 자식 Claude 인스턴스를 같은 surface에서 재시작 (레이아웃 변경 없음). child_index로 대상 지정 |
| `claude.broadcast` | `surface_id?, text: string, role?: string` | 부모의 모든 자식에 텍스트 동시 전송. role 필터로 특정 역할만 대상 지정 가능. 반환: `{ sent_count, children }` |
| `claude.wait` | `surface_id?, child_index: number` | 자식의 현재 상태 조회. 반환: `{ state: "idle"\|"needs_input"\|"active"\|"exited" }`. CLI에서 폴링하여 대기 구현 가능. CLI(`tasty claude wait`)는 시작 시 `~/.claude/settings.json`의 tasty Stop 훅 설치 여부를 점검하며, 미설치 시 안내 메시지를 출력하고 즉시 종료한다 (먼저 `tasty claude install` 실행 필요) |
| `claude.set_idle_state` | `surface_id?, idle: bool` | Claude idle 상태 설정 (idle=false 시 needs_input도 해제) |
| `claude.set_needs_input` | `surface_id?, needs_input: bool` | Claude needs-input 상태 설정 |
| `claude.tell` | `surface_id?, child_index?, text: string` | 특정 자식(또는 본인)에 텍스트 전송. `broadcast`와 달리 단일 대상 |
| `claude.install` | 없음 | `~/.claude/settings.json`에 Stop/Notification/SessionEnd/SubagentStop/SessionStart 훅 등록 (idempotent) |
| `claude.uninstall` | 없음 | 등록한 훅 제거 (사용자 entry는 보존) |
| `claude.hook` | `kind: string, surface_id?, session?` | Claude Code 훅 시스템에서 호출되는 진입점 (stop/notification/session-end/subagent-stop/prompt-submit/session-start) |
| `surface.fire_hook` | `surface_id?, event: string` | 특정 이벤트의 등록된 훅 수동 실행 |

## 일반적인 사용 패턴

### 명령 실행 후 결과 읽기

```python
call("surface.set_mark")               # 마크 설정
call("surface.send", {"text": "ls\r"}) # 명령 실행
import time; time.sleep(1)             # 출력 대기
result = call("surface.read_since_mark", {"strip_ansi": True})
print(result["result"]["text"])
```

### 다른 패인에서 명령 실행

```python
surfaces = call("surface.list")["result"]
target_id = surfaces[1]["id"]  # 두 번째 서피스
call("surface.send_to", {"text": "npm start\r", "surface_id": target_id})
```

### Ctrl+C 보내기

```python
call("surface.send_combo", {"key": "c", "modifiers": ["ctrl"]})
```

### 워크스페이스에 설명 달기

```python
call("workspace.update", {
    "name": "Backend",
    "subtitle": "API Server",
    "description": "Express.js REST API 개발 중"
})
```

### 프로세스 종료 감지

```python
call("hook.set", {
    "event": "process-exit",
    "command": "tasty notify 'Process finished'",
    "once": True
})
```
