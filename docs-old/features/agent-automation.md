# 에이전트 자동화

- **Status**: Implemented

tasty의 핵심 차별점으로, "에이전트가 에이전트를 제어하는 자동화"를 위한 세 가지 기능을 제공한다.

### Surface Hook 시스템 (crates/tasty-hooks)

Surface별 이벤트 훅을 등록하여 특정 이벤트 발생 시 셸 명령을 자동 실행한다.

- **HookManager**: 훅 등록/삭제/조회/실행을 관리하는 중앙 매니저
- **HookEvent 타입**:
  - `ProcessExit`: 셸 프로세스 종료 시
  - `OutputMatch(pattern)`: PTY 출력이 정규식 패턴에 매칭될 때
  - `Bell`: BEL 문자 수신 시
  - `Notification`: OSC 알림 수신 시
  - `IdleTimeout(secs)`: N초간 PTY 출력 없을 때
  - `ClaudeIdle`: Claude Code 작업 완료 시
  - `NeedsInput`: Claude Code 사용자 입력 필요 시
  - `ClaudeError`: Claude child PTY가 알려진 비정상 패턴(API Error, content filter, rate limit 등)을 출력했을 때 자동 fire
- **ProcessExit 구현**: 터미널 프로세스 종료 시 ProcessExited 이벤트 자동 발생 및 훅 실행. 프로세스 종료 후 해당 서피스를 자동으로 닫음. 서피스 → 탭 → 패인 → 워크스페이스 순으로 계층을 올라가며 적절한 레벨에서 정리. 마지막 워크스페이스의 마지막 서피스인 경우 새 셸을 스폰
- **정규식 캐싱**: OutputMatch 훅 등록 시 정규식을 사전 컴파일하여 매칭 시 재컴파일 방지
- **once 옵션**: true로 설정하면 한 번 실행 후 자동 삭제
- **비동기 실행**: 훅 명령은 백그라운드 스레드에서 실행 (메인 루프 블로킹 없음)
- **이벤트 루프 통합**: main.rs에서 TerminalEvent 수집 후 Bell/Notification/ProcessExit 이벤트에 대해 자동으로 훅 체크 및 실행
- **Surface ID 추적**: 각 이벤트가 발생한 Surface ID를 추적하여 훅이 올바른 Surface에서 실행
- CLI: `tasty set hook --event bell --command "notify-send 'bell'" --once`
- IPC: `hook.set`, `hook.list`, `hook.unset` 메서드

### Read Mark API (crates/tasty-terminal)

터미널 출력에 마크를 설정하고, 마크 이후의 새 출력만 효율적으로 읽는 델타 트래킹 API.

- **output_buffer**: PTY에서 수신한 원시 바이트를 최대 1MB까지 순환 버퍼에 저장
- **read_mark**: 바이트 오프셋 기반 마크 위치 추적
- **버퍼 관리**: 1MB 초과 시 오래된 데이터 자동 삭제, 마크가 잘린 영역에 있으면 무효화(None), 아니면 오프셋 조정
- **set_mark()**: 현재 버퍼 끝 위치에 마크 설정
- **read_since_mark(strip_ansi)**: 마크 이후 출력 텍스트 반환. `strip_ansi=true`이면 ANSI 이스케이프 시퀀스 제거
- **strip_ansi_escapes()**: `LazyLock<Regex>`으로 초기화 시점 한 번만 컴파일하는 정규식으로 ANSI CSI, OSC BEL, OSC ST 시퀀스 제거 (반복 호출 시 regex 재컴파일 없음)
- **Surface ID로 조회**: AppState에서 전체 워크스페이스/패인/탭/서피스 트리를 재귀 탐색하여 특정 Surface의 마크 설정/읽기 지원
- CLI: `tasty set mark`, `tasty read since-mark --strip-ansi`
- IPC: `surface.set_mark`, `surface.read_since_mark` 메서드

### Claude Code 통합 (com.tasty.claude plugin)

Claude Code 워크스페이스 런처, parent-child 자식 관리, hook 통합 전체가 번들 plugin
`com.tasty.claude`로 이전되었다. 호스트는 plugin 등록만 처리하고, 모든 claude.*
IPC 메서드와 `tasty claude *` CLI 서브커맨드는 plugin이 자체적으로 노출한다.

#### Claude 워크스페이스 런처 (claude.launch)
- 새 워크스페이스 자동 생성 및 이름 설정
- 지정된 디렉토리로 이동 후 `claude` 명령 실행 (shell-escape로 인젝션 방지)
- `--task` 옵션으로 작업 설명 전달 가능 (shell-escape 적용)
- CLI: `tasty claude launch --workspace "my-project" --directory "/path/to/project" --task "Fix the bug"`
- IPC: `claude.launch` 메서드 (workspace, directory, task 파라미터)

#### Parent-Child 관계 관리
부모 Claude 인스턴스가 자식 Claude 인스턴스를 생성·관리하는 시스템. AI 에이전트가
멀티 에이전트 워크플로우를 구성할 때 사용한다.

- **자식 추적**: plugin 내부 state에서 surface ID, 인덱스, cwd, role, nickname을 보존
- **자동 정리**: 부모 또는 자식 surface가 닫힐 때 관계를 자동으로 정리. 부모가 먼저
  닫혀도 자식이 살아있는 동안 관계 유지 (ghost cleanup)
- **claude.spawn**: 두 가지 모드 지원:
  - **`--surface` 모드**: 대상 surface의 pane을 분할하여 새 터미널 생성 후 `claude` 명령 자동 실행
  - **`--workspace` 모드**: workspace를 지정하면 plugin이 spawn pane을 자동 관리. parent surface마다
    지정된 workspace 내에 전용 spawn pane을 갖고, 2×2 그리드 알고리즘으로 최적 배치
    (1→좌우분할→좌측상하분할→우측상하분할→새탭). 4개 초과 시 탭 확장
  - `--workspace`와 `--surface`는 동시 사용 불가
  - 부모(parent)는 항상 spawn 명령을 실행한 surface(`TASTY_SURFACE_ID`). cwd, role, nickname, prompt 파라미터 지원
- **claude.children / claude.parent / claude.kill / claude.respawn / claude.broadcast / claude.wait**:
  자식 목록 조회, 부모 역참조, 자식 종료, 자식 재시작(레이아웃 유지), 일괄 전송,
  상태 폴링(idle|needs_input|active|exited). CLI 동일.
- **claude.wait_by_surface**: child surface id 단독 lookup 으로 자식 state 를 반환.
  `claude.wait` 와 동일 semantics 이되 (parent, child_index) 가 아닌 surface id 만 받는다.
  `tell` 의 자동 wait chain 이 사용 (tell 응답에 child_index 가 없으므로 surface id 기준
  wait 가 필요).
- **claude.wait_any**: 여러 자식 중 *먼저* idle / needs_input / exited 가 되는 것을 즉시
  깨운다. 응답 JSON 에 `child_index` 키가 포함되어 어느 자식이 깨어났는지 알 수 있다.
  우선순위는 입력 children 순서 (동시 다수 terminal 시 결정적). timeout 도달 또는
  iteration 중 전원 active 인 tick 의 응답은 `{"state":"pending"}` (child_index 키 없음).
- **자동 wait chain** (`spawn` / `tell`): `tasty claude spawn` 과 `tasty claude tell` 은
  1 차 IPC 응답 직후 자동으로 `claude.wait` / `claude.wait_by_surface` 를 chain 호출하여
  child 가 `idle` / `needs_input` / `exited` terminal state 에 도달할 때까지 block 한다.
  응답은 line-delimited 두 JSON — 첫 줄이 spawn/tell 응답, 둘째 줄이 wait 응답.
  - `--no-wait` (bool): chain 을 skip — 기존 fire-and-forget 동작 (1 차 응답 한 줄만 출력).
  - `--timeout SECS` (u32): wait polling deadline. 생략하면 무한 대기.
  - 다른 plugin 도 manifest `[[contributes.cli.subcommand]].auto_wait` 필드로 동일 패턴을
    선언할 수 있다 — 상세는 [`docs/dev-guide/cli-naming.md`](dev-guide/cli-naming.md) 의
    "auto-wait chain" 섹션 참조.
- CLI: `tasty claude spawn --direction vertical --cwd /path --role worker --nickname "agent-1" --prompt "Fix bugs"`
- CLI: `tasty claude children`, `tasty claude parent`, `tasty claude kill --child 1`, `tasty claude respawn --child 1`
- CLI: `tasty claude broadcast "text\r" [--role ROLE]`, `tasty claude wait --child 1 [--timeout SECS]`
- CLI: `tasty claude wait-any --children "1,2,3" [--timeout SECS]` — 첫 깨어난 자식의 응답을
  즉시 출력. `--timeout` 누락 시 무한 polling (`wait` 와 동일 정책).
- `--child`는 child index를 받는다 (spawn 시 반환되는 `child_index` 값)

#### Hook 통합
- **상태 추적**: plugin이 surface별 idle/needs_input 상태를 자체 관리
- **claude.hook**: stop, notification, session-end, subagent-stop, prompt-submit, session-start 이벤트를
  받아 상태 갱신 + host의 `surface.fire_hook` / `surface.meta.set` / `surface.meta.unset` 호출
- **상태 priority**: needs_input > idle > active. claude.children 응답의 `state` 필드에 반영
- **자동 정리**: surface가 닫힐 때 plugin state에서 자식·상태·error scan plate 함께 정리
- **HookEvent**: 호스트 측 `ClaudeIdle`/`NeedsInput`/`ClaudeChildIdle`/`ClaudeChildNeedsInput`/
  `ClaudeError` 이벤트 타입 유지 (`claude-idle`, `needs-input`, `claude-child-idle`,
  `claude-child-needs-input`, `claude-error` 키). plugin이 host의 fire_hook IPC로 발화
- **parent fan-out**: child Claude 에서 `stop` / `subagent-stop` / `session-end` 가 들어오면
  child surface 의 `claude-idle` 외에 *parent surface* 의 `claude-child-idle` 도 함께 fire 한다.
  `notification` 도 동일하게 parent surface 의 `claude-child-needs-input` 을 fire. parent 매핑이
  없는 (top-level) 호출에는 영향 없음. conductor 가 polling 없이 자식 완료를 감지하기 위한 채널.
- **ClaudeError 자동 감시**: `claude.spawn` / `claude.launch`로 만들어진 child surface는 PTY 출력에
  대한 패턴 스캐너가 plugin 안에서 자동 활성화된다. plugin이 `surface.read_since_mark`로 새
  출력 슬라이스를 받아 ANSI strip 후 catalog 정규식과 매칭하고, 매칭되면 `surface.fire_hook`으로
  `claude-error` 훅을 fire한다. 카탈로그(plugin 내장): `API Error`, `Output blocked by content
  filtering policy`, `overloaded_error`, `rate_limit_error`, `Bad Request`, `Internal Server Error`,
  `network error` (대소문자 무시). 일반 셸 surface는 영향 없음.
- **`tasty claude install` / `uninstall`**: `~/.claude/settings.json`의 `hooks` 객체에
  Stop/Notification/SessionEnd/SubagentStop 4종 entry를 idempotent하게 추가·제거한다. 각 entry의
  command는 `[ -n "$TASTY_SURFACE_ID" ] && tasty claude hook <token> || true` 형태로 tasty 외부에서
  claude를 실행할 때는 무해하게 통과한다.
- **wait의 사전 요구사항 점검**: `tasty claude wait`는 시작 시 Stop 훅 등록 여부를 확인하고
  미설치면 안내 메시지를 stderr에 출력하고 exit code 1로 종료한다.
- CLI: `tasty claude install`, `tasty claude uninstall`, `tasty claude hook stop|notification|session-end|subagent-stop|prompt-submit|session-start [--surface ID]`
- IPC: `claude.hook`, `claude.launch`, `claude.spawn`, `claude.children`, `claude.parent`, `claude.kill`,
  `claude.respawn`, `claude.broadcast`, `claude.wait`, `claude.wait_any`, `claude.wait_by_surface`
  메서드. 권한 토큰: `ipc.invoke:claude`

### Surface Metadata Store (surface_meta.rs)

파일 기반 키-값 스토어로, 어떤 프로세스(Claude Code 포함)든 서피스별 임의 메타데이터를 읽고 쓸 수 있다.

- **저장 위치**: OS 임시 디렉토리 — `{tmp}/tasty-surfaces/<surface_id>/meta.json` (Windows: `%TEMP%`, macOS: `/var/folders/...`, Linux: `/tmp`)
- **SurfaceMetaStore**: 정적 메서드만 가지는 유틸리티 구조체 (상태 없음)
- **ensure_created(surface_id)**: 서피스 생성 시 메타 디렉토리와 빈 JSON 파일 생성. `send_fast_init()` 내부에서 자동 호출됨
- **remove(surface_id)**: 서피스 닫힐 때 메타 디렉토리 전체 삭제. 탭/패인/서피스 닫기 시 자동 호출됨
- **set/get/unset/list**: 파일을 읽어 HashMap으로 역직렬화, 수정 후 pretty JSON으로 재직렬화
- **범용 키-값**: 역할(role), 닉네임, 상태 등 에이전트가 필요한 임의 데이터를 저장 가능
- CLI: `tasty surface-meta set|get|unset|list --key KEY [--value VALUE] [--surface ID]`
- IPC: `surface.meta_set`, `surface.meta_get`, `surface.meta_unset`, `surface.meta_list` 메서드
