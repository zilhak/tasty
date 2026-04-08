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
tasty info                    # 버전, 워크스페이스 수

# 워크스페이스
tasty list                    # 워크스페이스 목록
tasty new workspace [--name NAME] [--cwd PATH]
tasty select-workspace INDEX  # 0-based
tasty update-workspace [--id ID] [--name NAME] [--subtitle TEXT] [--description TEXT]

# 윈도우
tasty windows                 # 윈도우 목록
tasty new window              # 새 윈도우 생성

# 패인/탭
tasty panes                   # 패인 목록
tasty new split --level pane-group|surface [--target ID] [--direction vertical|horizontal] [--cwd PATH]
tasty new tab [--cwd PATH]
tasty close pane
tasty close tab

# 터미널 (Surface)
tasty surfaces                # 서피스 목록 (id, cols, rows)
tasty send "ls -la\r"         # 텍스트 전송 (\r = Enter)
tasty send-key enter          # 키 전송
tasty set-mark                # 출력 마크 설정
tasty read-since-mark [--strip-ansi]  # 마크 이후 출력 읽기
tasty close surface

# 비터미널 패널
tasty new markdown /path/to/file.md     # 마크다운 뷰어 탭 열기
tasty new explorer [--path /dir]        # 파일 탐색기 탭 열기

# IME 시뮬레이션
tasty ime-enable                   # IME 활성화
tasty ime-preedit "ㅎ"              # 조합 중 텍스트 표시
tasty ime-commit "한"               # 확정 → PTY 전송
tasty ime-status                   # 현재 IME 상태 확인
tasty ime-disable                  # IME 비활성화 + preedit 클리어

# 포커스 이동
tasty focus-direction left    # 왼쪽 패인/서피스로 포커스 이동
tasty focus-direction right   # 오른쪽
tasty focus-direction up      # 위
tasty focus-direction down    # 아래

# 훅
tasty set-hook --event process-exit --command "echo done"
tasty list-hooks
tasty unset-hook --hook HOOK_ID

# 알림
tasty notify "메시지" [--title "제목"]
tasty notifications

# 트리 (전체 구조 출력)
tasty tree

# 메시지 패싱
tasty message-send --to SURFACE_ID "내용"           # 서피스에 메시지 전송
tasty message-read [--surface ID] [--from ID] [--peek]  # 메시지 읽기 (기본: 소비)
tasty message-count [--surface ID]                  # 대기 메시지 수 확인
tasty message-clear [--surface ID]                  # 메시지 큐 삭제

# Claude
tasty claude launch [--workspace NAME] [--directory PATH] [--task "설명"]
tasty claude spawn [--direction vertical|horizontal] [--cwd PATH] [--role ROLE] [--nickname NAME] [--prompt "TEXT"]
tasty claude children             # 자식 Claude 목록
tasty claude parent               # 부모 Claude 조회
tasty claude kill --child ID      # 자식 Claude 종료
tasty claude respawn --child ID [--cwd PATH] [--role ROLE] [--nickname NAME] [--prompt "TEXT"]
tasty claude broadcast "텍스트\r" [--role ROLE]   # 모든 자식에 텍스트 전송
tasty claude wait --child 42 [--timeout 60]       # idle/needs_input/exited 대기

# Claude Hook 통합 (Claude Code의 훅 시스템에서 호출)
tasty claude hook stop              # Claude 작업 완료 → idle 상태 설정 + claude-idle 훅 실행
tasty claude hook notification      # Claude 입력 필요 → needs-input 상태 설정 + needs-input 훅 실행
tasty claude hook prompt-submit     # 사용자 입력 전송 → active 상태로 전환
tasty claude hook session-start     # 세션 시작 → active 상태로 전환
tasty claude hook stop --surface 5  # 특정 surface 지정 (또는 TASTY_SURFACE_ID 환경변수)
```

## IPC 메서드 레퍼런스

### 시스템

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `system.info` | 없음 | 버전, 워크스페이스 수 |
| `ui.state` | 없음 | 현재 UI 상태 (설정창/알림패널 열림 여부, 패인 수 등) |
| `ui.screenshot` | `path?: string` | 스크린샷을 PNG로 저장 (GUI 모드 전용, 비동기) |

### 윈도우

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `window.list` | 없음 | 전체 윈도우 목록 (id, focused, title) |
| `window.create` | 없음 | 새 독립 윈도우 생성 |
| `window.close` | 없음 | 포커스된 윈도우 닫기 |
| `window.focus` | `id: string` | 특정 윈도우에 포커스 |

### 워크스페이스

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `workspace.list` | 없음 | 전체 워크스페이스 목록 (id, name, subtitle, description, active) |
| `workspace.create` | `name?, subtitle?, description?, cwd?` | 새 워크스페이스 생성 후 활성화 |
| `workspace.update` | `index?\|id?, name?, subtitle?, description?` | 워크스페이스 정보 수정 (생략 시 활성 워크스페이스) |
| `workspace.select` | `index: number` | 워크스페이스 전환 (0-based) |
| `tree` | 없음 | 전체 계층 구조 (워크스페이스 → 패인 → 탭) |

### 패인

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `pane.list` | 없음 | 활성 워크스페이스의 패인 목록 |
| `split` | `level: "pane-group"\|"surface"`, `target_id?: number`, `direction?: "vertical"\|"horizontal"`, `cwd?: string` | 분할. level로 상위/하위 레이아웃 선택, target_id로 대상 지정 (생략 시 focused), 포커스 이동 없음. cwd로 작업 디렉토리 지정 가능 |
| `pane.close` | 없음 | 포커스된 패인 닫기 |
| `pane.focus` | `pane_id: number` | 특정 패인에 포커스 |
| `focus.direction` | `direction: "left"\|"right"\|"up"\|"down"` | 방향성 포커스 이동 (SurfaceGroup 내부 우선, 이후 패인 간) |

### 탭

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `tab.list` | 없음 | 포커스된 패인의 탭 목록 |
| `tab.create` | `cwd?: string` | 새 탭 생성. cwd로 작업 디렉토리 지정 가능 |
| `tab.close` | 없음 | 활성 탭 닫기 |
| `tab.open_markdown` | `file_path`, `pane_id?` | 마크다운 뷰어 탭 열기 |
| `tab.open_explorer` | `path?`, `pane_id?` | 파일 탐색기 탭 열기 (path 생략 시 홈 디렉토리) |

### Surface (터미널 상호작용)

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `surface.list` | 없음 | 모든 서피스 목록 (id, pane_id, cols, rows) |
| `surface.send` | `text, surface_id?` | 텍스트 전송. `\r`로 Enter |
| `surface.send_to` | `text, surface_id` | 특정 서피스에 텍스트 전송 (surface_id 필수) |
| `surface.send_key` | `key, surface_id?` | 키 이름 전송 (enter, tab, escape, up, down 등) |
| `surface.send_combo` | `key, modifiers[], surface_id?` | 수정자 키 조합 전송 (Ctrl+C 등) |
| `surface.focus` | `surface_id` | 특정 서피스에 포커스 |
| `surface.close` | 없음 | 포커스된 서피스 닫기 |
| `surface.screen_text` | `surface_id?` | 현재 화면의 텍스트 반환 |
| `surface.set_mark` | `surface_id?` | 현재 출력 위치에 마크 설정 |
| `surface.read_since_mark` | `surface_id?, strip_ansi?: bool` | 마크 이후 새 출력 반환 |
| `surface.cursor_position` | `surface_id?` | 커서 위치 (x, y) 반환 |

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
tasty ime-enable                   # IME 활성화
tasty ime-preedit "ㅎ"              # preedit 표시
tasty ime-preedit "하"              # 모음 조합
tasty ime-preedit "한"              # 받침 조합
tasty ime-commit "한"               # 확정 → PTY 전송
tasty ime-status                   # 상태 확인
tasty ime-disable                  # IME 비활성화
```

**한글 입력 시뮬레이션 전체 흐름:**

```bash
tasty ime-enable
# "한글" 입력
tasty ime-preedit "ㅎ"
tasty ime-preedit "하"
tasty ime-preedit "한"
tasty ime-commit "한"
tasty ime-preedit "ㄱ"
tasty ime-preedit "그"
tasty ime-preedit "글"
tasty ime-commit "글"
tasty ime-disable
```

### Surface 메타데이터

파일 기반 키-값 스토어. 어떤 프로세스(Claude Code 포함)든 서피스별 메타데이터를 읽고 쓸 수 있다.

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `surface.meta_set` | `surface_id?, key: string, value: string` | 키-값 설정. 응답: `{ ok: true }` |
| `surface.meta_get` | `surface_id?, key: string` | 값 조회. 응답: `{ value: "..." }` 또는 `{ value: null }` |
| `surface.meta_unset` | `surface_id?, key: string` | 키 삭제. 응답: `{ ok: true }` |
| `surface.meta_list` | `surface_id?` | 전체 메타데이터 객체 반환 |

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
tasty global-hook-set --condition interval:30 --command "echo tick" --label "heartbeat"
tasty global-hook-set --condition once:5 --command "notify-send done"
tasty global-hook-set --condition "file:/tmp/trigger" --command "bash /tmp/trigger"
tasty global-hook-list
tasty global-hook-unset --hook HOOK_ID
```

### 알림

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `notification.list` | 없음 | 최근 50개 알림 |
| `notification.create` | `title?, body?` | 알림 생성 |

### 메시지 패싱 (Surface 간 통신)

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `message.send` | `to_surface_id: number, content: string, from_surface_id?: number` | 다른 서피스에 메시지 전송. 응답: `{ id: N }` |
| `message.read` | `surface_id?: number, from_surface_id?: number, peek?: bool` | 메시지 읽기 (기본: 소비). `peek: true`이면 큐에서 제거하지 않음. `from_surface_id`로 발신자 필터 가능 |
| `message.count` | `surface_id?: number` | 대기 중인 메시지 수. 응답: `{ count: N }` |
| `message.clear` | `surface_id?: number` | 메시지 큐 전체 삭제. 응답: `{ cleared: true }` |

### Claude 전용

| 메서드 | 파라미터 | 설명 |
|--------|---------|------|
| `claude.launch` | `workspace?, directory?, task?` | 새 워크스페이스에서 Claude 실행 |
| `claude.spawn` | `surface_id?, direction?, cwd?, role?, nickname?, prompt?` | 부모 pane을 분할하여 자식 Claude 인스턴스 생성 |
| `claude.children` | `surface_id?` | 부모 surface의 자식 목록 조회 |
| `claude.parent` | `surface_id?` | 자식 surface의 부모 조회 |
| `claude.kill` | `child_surface_id` | 자식 Claude 인스턴스 종료 |
| `claude.respawn` | `child_surface_id, cwd?, role?, nickname?, prompt?` | 자식 Claude 인스턴스 재시작 |
| `claude.broadcast` | `surface_id?, text: string, role?: string` | 부모의 모든 자식에 텍스트 동시 전송. role 필터로 특정 역할만 대상 지정 가능. 반환: `{ sent_count, children }` |
| `claude.wait` | `child_surface_id: number` | 자식의 현재 상태 조회. 반환: `{ state: "idle"\|"needs_input"\|"active"\|"exited" }`. CLI에서 폴링하여 대기 구현 가능 |
| `claude.set_idle_state` | `surface_id?, idle: bool` | Claude idle 상태 설정 (idle=false 시 needs_input도 해제) |
| `claude.set_needs_input` | `surface_id?, needs_input: bool` | Claude needs-input 상태 설정 |
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
