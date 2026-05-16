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
tasty read parse-since-mark [--surface ID] [--parsers path,url,...]  # 마크 이후 출력을 파서로 분해
tasty read commands [--surface ID] [--limit N] [--since UNIX_MS]      # OSC 133 으로 인덱싱된 명령 목록
tasty read last-command [--surface ID]      # 가장 최근 명령 1건
tasty read command-at --index N [--surface ID]  # 0-based 인덱스 (음수면 끝에서부터)
tasty output observe start [--surface ID] [--parsers path,url] [--kinds path] \
    [--sink memory|file] [--path FILE] [--max-records N]   # 옵저버 등록
tasty output observe stop --observer ID         # 옵저버 종료
tasty output observe list                       # 활성 옵저버 목록
tasty output observe info --observer ID         # 옵저버 통계
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

# 에이전트 메모리 (~/.tasty/memory.db, scope: global/account/window/workspace/surface)
tasty memory put --workspace 7 --key task.plan --value "..."   # 공유 영역에 저장 (owner=_host)
tasty memory put --surface 3 --key buf --value @/tmp/buf.txt   # @파일은 UTF-8 텍스트로 로드
tasty memory get --workspace 7 --key task.plan
tasty memory delete --workspace 7 --key task.plan [--cas N]
tasty memory list --workspace 7 [--prefix task.] [--limit 50]
tasty memory exists --workspace 7 --key task.plan
tasty memory count --workspace 7 [--prefix task.]
tasty memory scopes                                            # 사용 중인 스코프 목록
tasty memory stats [--workspace 7]                             # entries + bytes
tasty memory secret put --global --key api.token --value "sk-..."  # secret 영역 (caller별 분리)
tasty memory secret get --global --key api.token
tasty memory secret list --global [--prefix api.]
tasty memory secret delete --global --key api.token
tasty memory secret stats

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
| `surface.parse_since_mark` | `surface_id, parsers?: string[] \| string` | 마크 이후 출력을 빌트인 파서로 분해. 응답: `{ surface_id, parsers, items: [{ kind, line, byte_start, byte_end, data }] }`. 알 수 없는 파서 id 면 `invalid_params`. 기본 파서 = `path,url,prompt_boundary,exit_code`. 옵션: `compile_error,stack_trace,test_result,progress,osc_link,osc_notification` (전체 카탈로그: [output-parsers.md](output-parsers.md)). |
| `surface.commands` | `surface_id, limit?: usize, since?: i64` | OSC 133 으로 인덱싱된 명령 목록. 각 record: `{ prompt_started_at, command_started_at, ended_at, exit_code, command }` (단위: unix-ms). 셸 통합이 미설치된 surface 는 빈 배열. |
| `surface.last_command` | `surface_id` | 가장 최근 record. 없으면 `null`. |
| `surface.command_at` | `surface_id, index: i64` | 0-based 인덱스 (음수면 끝에서부터). 범위 밖이면 `null`. |
| `output.observe_start` | `surface_id?, parsers?, kinds?, sink: { type, ... }` | 옵저버 등록. `sink.type = "memory"` 이면 `max_records` (기본 10000, 0=무한 ring buffer), `"file"` 이면 `path?` (생략 시 `~/.tasty/observers/<id>.jsonl`). 반환: `{ observer_id, info }`. 알 수 없는 파서 / 경로 오픈 실패 시 `invalid_params` / `internal_error`. |
| `output.observe_stop` | `observer_id` | 옵저버 종료. sink worker thread 정리. |
| `output.observe_list` | — | 활성 옵저버 전체 목록 (`{ observers: [info, ...] }`). |
| `output.observe_info` | `observer_id` | 단일 옵저버 상태: `{ id, surface_id, parsers, kinds, sink, total_in, total_out, dropped, last_event_ms }`. |
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

서피스별 키-값 스토어. 어떤 프로세스(Claude Code 포함)든 서피스별 메타데이터를 읽고 쓸 수 있다. 내부적으로는 `memory.*` 의 `scope=surface:<id>` 영역에 `text/plain` 값으로 저장되는 thin facade 다. 즉 `surface.meta.set` 와 `memory.put { scope: "surface:N", content_type: "text/plain", ... }` 는 같은 row 를 갱신한다 (소유자는 host).

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `surface.meta.set` | `surface_id?, key: string, value: string` | 키-값 설정. 응답: `{ ok: true }` |
| `surface.meta.get` | `surface_id?, key: string` | 값 조회. 응답: `{ value: "..." }` 또는 `{ value: null }` |
| `surface.meta.unset` | `surface_id?, key: string` | 키 삭제. 응답: `{ ok: true }` |
| `surface.meta.list` | `surface_id?` | 전체 메타데이터 객체 반환 |

**키 형식**: `1..256` 자 `[a-z0-9._-]+` (memory 스토어 공통 규칙). 대문자/공백 키는 거부된다.

**값 형식**: UTF-8 문자열만. memory.put 으로 같은 surface scope 에 JSON/binary 를 직접 넣은 경우 `surface.meta.get` 은 JSON 값을 stringified 형태로 변환해 반환하고 (`tracing::warn`), binary 는 `null` 로 응답한다.

서피스가 닫히면 해당 scope 의 모든 키가 host 에 의해 즉시 purge 된다 (`memory.changed` `deleted` 이벤트로도 관측 가능).

> **Deprecated alias**: 옛 이름 `surface.meta_set` / `meta_get` / `meta_unset` / `meta_list`(underscore 합성)는 호스트가 자동 정규화하지만 `tracing::warn`이 출력된다. **1.0 tag 직전에 일괄 제거**되므로 새 호출자는 점 표기(`surface.meta.*`)를 사용한다.

**CLI 사용 예시:**

```bash
tasty surface-meta set --key role --value orchestrator
tasty surface-meta get --key role
tasty surface-meta unset --key role
tasty surface-meta list
tasty surface-meta list --surface 3   # 특정 서피스 지정
```

### 에이전트 메모리 (`memory.*`, `memory.secret.*`)

`~/.tasty/memory.db` (SQLite, WAL 모드)에 저장되는 영속 키-값 스토어. 재시작 후에도 보존된다. 두 개의 영역이 같은 파일에 공존한다:

- **Regular** (`memory.*`): 공유 네임스페이스. 모든 caller 가 모든 entry 를 **읽을** 수 있지만, **갱신·삭제는 해당 entry 를 만든 owner 본인** 또는 host (CLI / 사용자) 만 가능.
- **Secret** (`memory.secret.*`): caller 별 사전 분할. owner 가 PK 일부라 다른 plugin 의 secret 영역은 IPC 표면에 **개념 자체가 존재하지 않는다**. 디스크에는 평문 BLOB 으로 저장된다 — 보호 약속은 "다른 plugin 의 IPC 접근 차단" 한 가지. DB 파일을 직접 여는 경우/디스크 도난/백업 sync 는 보호 범위 밖이다. 진짜 민감 데이터는 `docs/dev-guide/plugin-sensitive-data.md` 가이드를 따라 별도 관리할 것.

**스코프 토큰**: `global` | `account:<userid>` | `window:<id>` | `workspace:<id>` | `surface:<id>`.

**키 규칙**: 1..256자 `[a-z0-9._-]+`. 점으로 계층(`task.123.plan`). 예약 prefix: `tasty.` (호스트 내부), `plugin.<plugin-id>.` (각 plugin namespace).

**값**: `text/plain` (문자열) | `application/json` (임의 JSON) | `application/octet-stream` (base64).

**Owner**: caller 가 host 면 `_host`, plugin 이면 그 plugin id. **plugin 이 owner 를 직접 전달할 수 없으며**, 호스트가 `CallerContext` 로부터 자동 도출한다.

**Config** (`~/.tasty/config.toml [memory]`):

| 키 | 기본값 | 의미 |
|----|--------|------|
| `entry_max_mb` | `1` | 단일 entry 의 user-visible 값 최대 (MiB) |
| `regular_quota_mb_total` | `1024` | Regular 영역 전체 합산 한도 (MiB) |
| `secret_quota_mb_per_plugin` | `10` | Secret 영역 owner (plugin) 별 한도 (MiB) |

#### Regular API (`memory.*`)

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `memory.put` | `scope, key, value 또는 value_b64, content_type?, expires_at?, cas?` | 저장(upsert). 응답: `{ ok: true, version: N }` |
| `memory.get` | `scope, key` | 단건 조회. 응답: entry 객체 또는 `null` |
| `memory.delete` | `scope, key, cas?` | 삭제. 응답: `{ ok: true }` |
| `memory.list` | `scope, prefix?, limit?, since?, until?, offset?` | 엔트리 목록. `since`/`until` 은 `updated_at` 의 unix ms 범위, `offset` 으로 `--limit` 와 조합한 페이지네이션. 응답: `{ entries: [...], count: N }` |
| `memory.exists` | `scope, key` | 존재 여부. 응답: `{ exists: bool }` |
| `memory.count` | `scope, prefix?` | 갯수. 응답: `{ count: N }` |
| `memory.scopes` | _없음_ | 사용 중인 스코프 목록. 응답: `{ scopes: [...] }` |
| `memory.stats` | `scope?` | 엔트리 갯수 + byte 합계. 응답: `{ scope, entries, bytes }` |
| `memory.query` | `scope, path, equals, prefix?, limit?, since?, until?, offset?` | `application/json` entry 만 대상으로 dot-path 매칭 (`"task.status"` 형식, 배열 인덱스 미지원). `equals` 값과 deep-equality 비교. 응답: `{ entries: [...], count: N }` |
| `memory.export` | `scope?` | regular 영역을 dump. `scope` 가 있으면 그 scope 만, 없으면 전체. **secret 영역은 절대 export 되지 않는다.** 응답: `{ entries: [...], count: N }` (각 entry 는 `memory.get` 응답과 같은 형태) |
| `memory.import` | `entries: [...], replace?` | regular 영역으로 entry 입력. CAS 는 무시되며 `replace=false` (기본) 면 충돌 시 skip, `true` 면 덮어쓰기. caller 가 새 owner 가 된다. 응답: `{ applied: N, skipped: M }` |

#### Secret API (`memory.secret.*`)

같은 (scope, key) 라도 caller 별로 완전히 분리된 값. 다른 plugin 의 secret 은 조회·열거 자체가 불가능하다.

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `memory.secret.put` | `scope, key, value 또는 value_b64, content_type?, expires_at?, cas?` | 저장 (현재 caller 의 secret 영역). 응답: `{ ok: true, version: N }` |
| `memory.secret.get` | `scope, key` | 단건 조회. 응답: entry 객체 또는 `null` |
| `memory.secret.delete` | `scope, key, cas?` | 삭제. 응답: `{ ok: true }` |
| `memory.secret.list` | `scope, prefix?, limit?` | 엔트리 목록. 응답: `{ entries: [...], count: N }` |
| `memory.secret.exists` | `scope, key` | 존재 여부. 응답: `{ exists: bool }` |
| `memory.secret.count` | `scope, prefix?` | 갯수. 응답: `{ count: N }` |
| `memory.secret.scopes` | _없음_ | 사용 중인 스코프 목록. 응답: `{ scopes: [...] }` |
| `memory.secret.stats` | `scope?` | 엔트리 갯수 + 저장된 byte 합계. 응답: `{ scope, entries, bytes }` |

#### 유지 보수 (`memory.gc`)

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `memory.gc` | _없음_ | 만료된 entry (regular + secret) 일괄 DELETE. **local-only** (plugin 불가). 응답: `{ regular: N, secret: M }`. read 경로는 항상 만료 필터를 거치므로 사용자에게 보이는 동작은 변하지 않고 디스크 + quota 만 회복된다. CLI: `tasty memory gc`. 호스트는 surface/workspace 가 닫힐 때 해당 scope 의 entry 를 자동 정리하므로, 보통은 명시 호출이 불필요하다 — 만료 위주 정리 또는 진단용. |

#### 변경 이벤트 (`memory.changed`)

별도 watch IPC 가 아니라 **Event Bus 1.0 의 host event** 로 노출된다. plugin 은 매니페스트의 `event_subscribe = ["memory.changed"]` + `permissions = ["memory.read"]` 로 구독한다.

```json
// memory.changed payload (scope=System)
{ "scope": "surface:42", "key": "task.plan", "kind": "created",  "version": 1 }
{ "scope": "surface:42", "key": "task.plan", "kind": "updated",  "version": 2 }
{ "scope": "surface:42", "key": "task.plan", "kind": "deleted" }
{ "scope": "workspace:1", "key": "tmp",      "kind": "expired" }
```

- `kind` 는 `created` / `updated` / `deleted` / `expired`.
- `scope` 는 token 문자열 (`surface:42`, `workspace:1`, `window:3`, `global`, `account:default`).
- **Secret 영역 변경은 발화하지 않는다.** owner/key 정보 누설을 막기 위함. 자기 plugin 의 secret 상태는 직접 호출한 IPC 응답으로만 관찰한다.
- 호스트가 1 변경 = 1 envelope 으로 발화한다. surface/workspace 닫힘으로 다수 key 가 한번에 사라지면 각 key 마다 별도 `deleted` 이벤트가 도착한다.
- 자세한 카탈로그 등록 정보는 `event-catalog.md` 의 "Memory" 섹션 참조.

#### Entry 객체

```json
{
  "scope": "surface:3",
  "key": "task.plan",
  "kind": "text",                              // "text" | "json" | "binary"
  "content_type": "text/plain",
  "value": "...",                              // text/json일 때
  "value_b64": "...", "size": 1234,            // binary일 때
  "version": 2,
  "created_at": 1715800000000,
  "updated_at": 1715800001234,
  "expires_at": null,
  "owner": "com.example.todo"                  // regular 응답에만 포함 (secret 은 자기 영역이므로 생략)
}
```

#### 동작 의미

- **CAS**: `put`/`delete`에 `cas: <expected_version>` 지정. 일치하지 않으면 `cas_conflict` 에러.
- **owner enforcement** (regular): 다른 owner 의 entry 를 갱신·삭제하려 하면 `owned_by_other` 에러. `_host` (CLI) 는 모든 entry 를 수정할 수 있는 root.
- **읽기는 공유** (regular): 모든 caller 는 모든 owner 의 entry 를 읽을 수 있고, 응답에 `owner` 필드로 누가 만든 entry 인지 노출된다.
- **저장 형태** (secret): 디스크에는 평문 BLOB 으로 저장된다. secret 영역의 보호는 owner 격리(IPC 표면에서 다른 plugin 의 entry 가 노출되지 않음) 한 가지에 한정된다. `memory.db` 파일을 직접 여는 경우, 백업/cloud sync, 디스크 도난 시나리오는 보호 범위 밖.

#### 에러 코드

| code | 의미 |
|------|------|
| `-32602` | invalid params (key/scope/content-type 검증 실패) |
| `-32603` | DB 에러 등 internal |
| `-32004` | `not_found` |
| `-32005` | `cas_conflict` |
| `-32006` | `owned_by_other` (regular 영역에서 다른 plugin 의 entry 를 수정·삭제 시도) |
| `-32007` | `quota_exceeded` (`area`/`used`/`limit` 포함) |

#### Plugin 권한

| 토큰 | 허용 메서드 |
|------|------------|
| `memory.read` | `memory.get` `memory.list` `memory.exists` `memory.count` `memory.scopes` `memory.stats` |
| `memory.write` | `memory.put` `memory.delete` |
| `memory.secret` | `memory.secret.*` 전체 (자기 영역만 접근 가능하므로 read/write 분리 불필요) |

#### CLI 사용 예시

```bash
# 텍스트 저장 (scope alias: --global / --surface 3 / --workspace 7 / --window 42 / --account foo)
tasty memory put --workspace 7 --key task.plan --value "step 1: ..."

# JSON 저장 — 파싱되면 자동으로 application/json
tasty memory put --workspace 7 --key task.steps --value '{"n":3,"done":1}'

# 파일에서 읽기 (UTF-8)
tasty memory put --surface 3 --key buffer --value @/tmp/buf.txt

# 조회/삭제
tasty memory get --workspace 7 --key task.plan
tasty memory delete --workspace 7 --key task.plan --cas 1

# 리스트 + 카운트 (페이지네이션은 --limit + --offset, 시간 범위는 --since/--until)
tasty memory list --workspace 7 --prefix task. --limit 50
tasty memory list --workspace 7 --since 1715800000000 --limit 20 --offset 20
tasty memory count --workspace 7 --prefix task.

# JSON path 매칭 (application/json entry 만)
tasty memory query --workspace 7 --path task.status --equals '"open"'

# Export / Import (regular 만 — secret 은 export 안 됨)
tasty memory export --workspace 7 > workspace.json
tasty memory import --file workspace.json            # 기존 key 는 skip
tasty memory import --file workspace.json --replace  # 기존 key 덮어쓰기

# 메타
tasty memory scopes
tasty memory stats --workspace 7

# Secret 영역 (CLI는 owner=_host)
tasty memory secret put --global --key api.token --value "sk-..."
tasty memory secret get --global --key api.token
tasty memory secret list --global --prefix api.
tasty memory secret delete --global --key api.token
tasty memory secret stats

# TTL + GC
tasty memory put --workspace 7 --key cache --value "..." --ttl 3600   # 1시간 후 만료
tasty memory put --workspace 7 --key cache --value "..." --expires-at 1715900000000
tasty memory gc                                                       # 만료 entry 일괄 DELETE
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

### 휴먼 핸드오프 (Approval)

`notification.create` 와 달리 **요청-응답** 워크플로우. 상세 가이드: [`approval.md`](approval.md).

| 메서드 | 권한 | 설명 |
|--------|------|------|
| `approval.request` | `Approval` | 새 결정 요청 생성. `severity` ∈ {`info`, `warn`, `danger`}. 응답: `{ id, record }` |
| `approval.respond` | `Approval` | `{ id, choice, comment? }`. self-response 금지 (`-32011`) |
| `approval.await` | local-only | `{ id, timeout_ms? }` blocking. `{ outcome, choice?, by?, default_choice? }` |
| `approval.cancel` | `Approval` | `{ id }` — 종료되지 않은 요청 취소 |
| `approval.get` | `Approval` | `{ id }` — 단일 record |
| `approval.list` | `Approval` | `{ state?, workspace_id? }` — in-memory 조회 (현 세션) |
| `approval.history` | `Approval` | `{ since?, until?, workspace_id?, requester_id?, decision?, state?, limit? }` — 영속 기록 조회 (재시작 후에도 유지) |
| `approval.summary.set` | `Approval` + `MemoryWrite` | `{ workspace_id, content }` — workspace 별 markdown 요약 저장 |
| `approval.summary.get` | `Approval` + `MemoryRead` | `{ workspace_id }` — 요약 조회 |

### 에이전트 텔레메트리 (`telemetry.*`)

비용·관측·이상 탐지의 기반. raw event 기록 + 즉시 집계 조회 + dispatcher 자동 카운트(`ipc_calls` 메트릭, `method` 태그) + Cost Cap CRUD. 이상 탐지와 자동 요약은 후속 단계에서 켜진다.

- 식별자 검증: `metric` 은 `[a-z][a-z0-9_]*` (최대 64), `agent` 는 `[a-zA-Z0-9_-]+` (최대 64). 위반 시 `-32602 invalid_params`
- `agent` 미지정 시 caller agent 가 자동 적용 — Plugin → manifest plugin_id, Local → env `TASTY_AGENT_ID` (없으면 `_host`)
- `workspace_id` 미지정 시 활성 워크스페이스 id 가 적용됨
- 영속화: `workspace_id` 가 붙은 이벤트는 `scope=workspace:<id>`, 없으면 `global`

| 메서드 | Permission | 파라미터 / 응답 |
|--------|------------|------------------|
| `telemetry.record` | `Telemetry` | `{ metric, value, op?∈{set,inc,dec}=inc, agent?, workspace_id?, tags? }` → `{ key, ts, agent, metric }` |
| `telemetry.record_batch` | `Telemetry` | `{ events: [...] }` 동일 ts 로 일괄 기록 → `{ recorded, keys[] }` |
| `telemetry.summary` | `Telemetry` | `{ metric?, agent?, workspace_id?, since?, until? }` → `{ entries: [{ metric, agent, workspace_id?, count, sum, min, max, last }], count, total_events }` |
| `telemetry.timeseries` | `Telemetry` | `{ metric, agent?, workspace_id?, window?∈{1m,1h,1d}=1m, since?, until? }` → `{ window, buckets: [{ metric, agent, window_start, window_size_ms, count, sum, min, max, last }], count }` |
| `telemetry.top` | `Telemetry` | `{ by?∈{agent,workspace}=agent, limit?=10, metric?, agent?, workspace_id?, since?, until? }` → `{ by, entries: [{ key, sum, count }], count }` |
| `telemetry.cap.set` | `Telemetry` | `{ agent, metric, threshold>0, window?∈{total,1h,1d}=total, action?∈{stop,pause,require_approval,notify}=notify }` → `CostCap` (생성된 `id` 포함) |
| `telemetry.cap.list` | `Telemetry` | `{ agent? }` → `{ entries: [CostCap], count }` (`created_at` 오름차순) |
| `telemetry.cap.remove` | `Telemetry` | `{ id }` → `{ removed: true, id }` (없으면 `-32004 not_found`) |
| `telemetry.cap.status` | `Telemetry` | `{ agent? }` → `{ entries: [CostCap + current_value + ratio], count }` |
| `telemetry.cap.reset` | `Telemetry` | `{ id? } 또는 { agent? }` (둘 중 최소 하나) → `{ reset_ids: [], count }` — 매칭된 cap 들의 `triggered` 비움 |
| `telemetry.anomaly.list` | `Telemetry` | `{ agent?, kind?∈{call_burst,slow_loop,rss_surge}, since?, until? }` → `{ entries: [Anomaly], count }` — `detected_at` 오름차순 |

**Cost Cap 동작 (Phase 4.3d 까지)** — `record` / `record_batch` / dispatcher 자동 카운트 직후 inline 으로 cap 평가. agent+metric 가 일치하는 미발화 cap 의 `current_value` (윈도우 내 raw event sum) 가 `threshold` 이상이면 `triggered: { at, value }` 마크.

| 액션 | 평가 시점 동작 | 후속 IPC 처리 |
|------|---------------|--------------|
| `notify` | 활성 워크스페이스에 알림 추가 | 변화 없음 |
| `stop` | 알림 + `triggered` 기록 | plugin agent 모든 IPC `-32007 cap_blocked` 거부 (OS 프로세스 kill 트리거는 별도 `claude.kill` IPC 도입 후 결합 — 현재 Pause 와 실효 동등) |
| `pause` | 알림 + `triggered` 기록 | plugin agent 모든 IPC `-32007 cap_blocked` 거부 |
| `require_approval` | `approval.request` 자동 발행 (severity=warn) + `triggered` 기록 | plugin agent 모든 IPC `-32007 cap_blocked` 거부 — 사용자가 popup 응답 후 `cap.reset` 으로 재개 |

`cap_blocked` 해제는 Local caller (CLI) 의 `telemetry.cap.reset` 만 가능 — 차단된 plugin 본인은 reset 도 막힌다.

`CostCap` 스키마: `{ id, agent, metric, threshold, window, action, created_at, triggered?: { at, value } }`. `triggered` 가 있으면 이미 액션이 발화된 상태로 간주된다 (재발화는 `cap.reset` 필요).

**이상 탐지 (Phase 4.4)** — `CallBurst` 휴리스틱이 활성. dispatcher 가 plugin IPC 를 카운트할 때마다 `AnomalyDetector` 가 (agent, method) 의 sliding window 를 갱신하고, 1분 내 1000 회 임계에 도달하면 발화한다. 발화 시 호스트는 `tasty.telemetry.anomaly.{ts:013}.{id}` 키로 Global scope 에 영속 + 활성 워크스페이스 알림. 같은 (agent, method) 의 연쇄 발화는 1분 쿨다운으로 dedup.

`Anomaly` 스키마: `{ id, kind∈{call_burst,slow_loop,rss_surge}, agent, subject, detected_at, detail: { window_ms, threshold, count, ... } }`. `SlowLoop` / `RssSurge` 는 타입만 정의돼 있고 검출은 후속 sub-phase 에서 도입된다. (Surface 간 통신)

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
