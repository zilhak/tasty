# 화면 없는 PTY (`tasty pty`)

## 원칙과 현재 동작

[자원·설정 원칙](../../principles/performance-and-settings.md)에 맞춰 강제 자원 통제와 선택 도구를 구분한다. 동시 개수는 제한하지 않는다. ID 공간이 소진됐을 때만 생성을 거절하며 이는 ID 무결성을 지키는 제한이다. 동시 실행 수를 맞추고 싶은 호출자는 선택형 조율 기능인 [semaphore](../agent-collaboration/index.md#semaphore)로 spawn 전에 permit을 얻고 kill 뒤 반납한다. 별도 설정은 두지 않는다. 실행 중인 PTY는 유휴 시간만으로 끝내지 않는다. 끝내는 경로는 명시적 `pty.kill`과 호출자가 spawn에서 고른 유휴 제한(`idle_timeout_secs`)뿐이다. 이미 끝난 항목의 정리는 Tasty 내부 저장 관리이며, Tasty가 스스로 항목을 지운 결과는 [결과 전달 원칙](../../principles/feedback-and-recovery.md)에 따라 알림과 `pty.*` 응답 문자열로 전달한다.

- **Status**: Implemented
- **주체**: AI Agent
- **설계**: [터미널 (Terminal)](../../surfaces/terminal/index.md) (신규 `pty.*` 네임스페이스 결정) · [포커스 정책 (운영 상세)](../../design/policies/focus.md) (surface와 PTY의 ID 범위 분리)
- **코드**: `src/runtime/terminal_store/standalone.rs` (단일 collection의 standalone 정책) · `crates/tasty-terminal/src/pty.rs` (PtyState·exit/reap) · `src/adapters/ipc/handler/pty.rs` (IPC) · `src/runtime/effect_runner.rs` 및 `src/runtime/effect_runner/installation.rs` (원 PTY 준비·승격) · `crates/tasty-cli` `pty` 서브커맨드 (CLI)
- **화면**: 없음 — headless 전용. 렌더되지 않고 포커스/닫은-항목 히스토리/선택에 닿지 않는다(identity.md 원칙 1). 승격(`pty.attach_surface`) 후에만 일반 terminal surface 로 렌더.

## 목적

에이전트가 화면에 탭을 만들지 않고 명령을 실행하고 실제 종료 코드를 받게 한다.
`tasty pty`는 Surface 트리를 바꾸지 않는다. 화면이 필요해지면 실행 중인 Terminal을
`pty.attach_surface`로 기존 Pane의 탭에 연결할 수 있다. Surface를 처음부터 만드는
[자식 터미널](../child-terminal/index.md)과 구분한다.

## 내부 동작 (headless-valid)

### 저장과 종료 코드

`EngineSession.runtime`의 `TerminalStore` 한 항목이 Terminal과 선택적인 Pty를 소유한다.
standalone metadata와 종료 관측은 해당 Pty의 PtyState에 있다. 같은 PTY를 별도 registry에 다시 등록하지 않는다.
Pty가 child handle을 끝까지 소유하며 `try_wait` 결과를 보관한다. 외부 watcher로 child를 넘기지 않는다.
`pty.wait`는 이 값을 즉시 조회하며 블로킹 대기가 아니다.
wait 실패는 내부 WaitFailed로 보존하고 기존 wire에서는 exit_code:null/success:false의 결과로 답한다.
kill/remove 응답은 신호 전달·실제 종료·reap 완료를 모두 확인했다는 뜻이 아니다.
owner_agent_id는 호출자의 텔레메트리 agent ID다. 세션 토큰을 쓰는 에이전트는 그 세션의 ID, 플러그인은 매니페스트 ID, 토큰 없는 로컬 호출은 호출자가 밝힌 `TASTY_AGENT_ID`(없으면 `_host`)다([텔레메트리 도출 규칙](../telemetry/index.md#도출-규칙)). 로컬 호출의 값은 자기 신고라 위조할 수 있으므로 강한 인증 정보로 취급하지 않는다.

### ID 범위

surface ID는 `[1, PTY_ID_BASE)`, PTY ID는 `0x8000_0000` 이상이다.
`is_surface_id_space`를 명령 인덱싱·IPC surface 인자·memory surface scope 검증에서 공통으로 사용한다.
큰 정수를 u32로 바꿀 때는 검사된 변환을 사용한다. headless PTY의 OSC 133은 surface 명령 인덱스에 넣지 않는다.

레이아웃 복원 부팅에서는 surface 카운터의 시작값을 정할 때 PTY 범위의 오래된 scope를 제외하고
오류 로그를 남긴 뒤 삭제한다. 복원을 하지 않는 headless나 restore_layout 비활성 실행에는 이 정리가 없다.
그 경우 카운터가 1부터 시작하므로 잘못된 scope에서 시작값을 물려받지는 않는다.

### 자원 회수

동시 개수 상한은 없다. 이미 끝난 항목도 새 PTY 생성을 막지 않는다. 회수 규칙은 항목 상태에 따라 다르다.

| 상태 | 기본 동작 | 회수 조건 | 알림 레벨 |
|---|---|---|---|
| 실행 중, 유휴 제한 없음 | 유지 | `pty.kill`뿐이다. 출력 여부나 API 호출 간격과 관계없다 | 없음 |
| 실행 중, `idle_timeout_secs` 지정 | 유지 | 마지막 `pty.read`·`pty.write`·`pty.wait` 뒤 지정한 초가 지나면 프로세스를 끝내고 항목을 지운다. 출력은 유휴 시각을 갱신하지 않는다 | 경고 |
| 이미 끝남 | 종료 결과 보존 | 종료를 처음 확인한 시각과 마지막 API 접근 중 늦은 쪽에서 5분(`EXITED_RETENTION`)이 지나면 항목을 지운다 | 정보 |

- 유휴 제한은 1초 이상이어야 하며 0이면 spawn을 `invalid params`로 거절한다. 지정한 PTY에만 적용한다.
- 유휴 제한 만료를 경고로 알리는 이유는 작업이 자연 종료 전에 중단됐지만 호출자가 고른 제한에 따른 결과라 크리티컬한 실패는 아니기 때문이다. 끝난 항목 정리는 실패가 아니므로 정보다.
- 알림은 한 번의 정리에서 같은 이유로 지운 PTY ID를 한 항목에 모은다. PTY는 workspace에 속하지 않으므로 그 창 engine의 첫 workspace를 출처로, surface 0으로 기록하고 `NotificationCreated` host event를 보낸다. workspace가 없으면 경고 로그만 남는다.
- 끝난 항목은 정리 전까지 `pty.wait`·`pty.list`로 종료 코드를 조회할 수 있다. 종료 확인 시각은 주기 정리나 접근 시 정리가 처음 본 시각이므로 실제 종료보다 최대 90초 늦을 수 있다.
- Tasty가 지운 ID는 최근 256개(`RECLAIM_RECORD_LIMIT`)까지 이유와 함께 기록한다. 이 ID로 `pty.write`·`pty.read`·`pty.wait`·`pty.kill`이 오면 `invalid params`로 회수 이유를 담은 문자열을 돌려준다. 유휴 제한 만료는 `... was reclaimed by Tasty: the idle timeout of <N>s set at spawn passed ...`, 끝난 항목 정리는 `... was removed by Tasty: it had exited (exit code <c>) and its result was kept for 300s`다. 요청 라우팅도 회수 기록을 가진 engine을 그 ID의 소유자로 고르므로 이 문자열이 그대로 전달된다. 기록에 없는 ID는 지금과 같은 대상 없음 응답(라우팅의 `no live headless pty <id> ...`, 핸들러의 `headless pty <id> not found`)이다. `pty.kill`로 지운 ID와 화면에 연결한 ID는 기록하지 않는다.
- spawn/list 접근에서도 주기 타이머를 기다리지 않고 같은 회수를 적용한다.
- Tick::PtySweep는 30초 주기, Lax slack 60초로 정리한다. 아무 호출이 없어도 기한 뒤 최대 90초 안에 회수한다.
- GUI main·parked engine과 headless 모두 같은 `EngineMut::reclaim_standalone_ptys`를 사용한다.
  Terminal/Pty 쌍과 waker 중복 방지 등록을 함께 정리하고 회수 기록과 알림까지 남긴다.
- 창을 닫아 engine을 해제할 때 그 engine의 standalone PTY가 함께 끝나는 경로는 이 정책과 별개이며 바뀌지 않았다.

자식은 PTY를 소유한 host 수명을 따른다. Windows의 Job Object와 Unix의 hangup 차이는
[터미널 수명](../../surfaces/terminal/index.md#프로세스-종료--절전-복귀)을 참고한다.

### 화면에 연결하기

`AdoptTerminal { pane_id, pty_id }`는 새 surface ID를 만들고 TerminalStore의 키를 옮긴다.
새 ID로 waker를 연결하고 기존 Pane에 background tab을 추가한다. 프로세스와 화면 내용은 유지한다.
사용자의 활성 탭은 바꾸지 않으며 tab.created·surface.created를 보낸다.
PtyState의 standalone 표시가 제거되어 이후에는 surface API로 다룬다. 물리 generation과 child handle은 유지된다. 옛 PTY ID의 waker 등록도 정리한다.
연결이 실패하면 프로세스를 종료하지 않고 원래 PTY ID로 돌려놓는다. 그 ID를 다른 owner가 차지해 돌려놓지 못하면 요청은 `-32068`(복구 필요)로 끝나고, 프로세스는 `pty.list`에 나타나지 않은 채 앱이 끝날 때까지 남는다([자원 회수](../../architecture/resource-recovery.md)).

## 인터페이스

- **AI Agent (IPC/CLI)** — 모든 대상은 pty id 로 직접 지정(포커스 독립, 원칙 3). `pty.list` 는 필터 없이 전 목록 반환.

| CLI | IPC method | 권한 | 목적 |
|-----|-----------|------|------|
| `tasty pty spawn [--cwd <dir>] [--idle-timeout <secs>] [-- <cmd>...]` | `pty.spawn` | `TerminalSpawn` | headless PTY 를 띄우고 pty id 반환. command(trailing var-arg) 생략 시 bare shell, 지정 시 즉시 실행(initial stdin 주입). 선택 `idle_timeout_secs`(1 이상)를 주면 그 PTY 에만 유휴 제한을 둔다. 동시 개수로 거절하지 않으며 ID 공간 소진 때만 `-32000` 에러. |
| `tasty pty write --id <n> "<text>"` | `pty.write` | `TerminalWrite` | 실행 중 PTY 에 stdin 을 as-is 전송(자동 제출 없음 — 개행은 호출자 포함). idle 리셋. |
| `tasty pty read --id <n> [--lines <k>] [--show-dim]` | `pty.read` | `TerminalRead` | 현재 화면 텍스트 읽기(옵션 `lines`=내용 기준 마지막 N줄 — 하단 공백 행 건너뜀, 모자라면 스크롤백에서 채움). `surface.screen_text` 와 동일 추출이며 진단 필드(`is_terminal`·`scrollback_len`·`alt_screen`)도 같이 낸다 — N 보다 적게 왔을 때 "그게 전부" 와 "잘렸다" 를 가른다. idle 리셋. dim(ghost-suggestion, 예: Claude Code 자동완성 제안) 셀은 기본 제외 — `--show-dim`/`show_dim:true` 로 포함. |
| `tasty pty state --id <n>` (예전 이름 `pty wait`) | `pty.wait` | `TerminalRead` | 즉시 반환 폴링(blocking 아님). exit cell 조회 → `{exited, exit_code, success}`. idle 리셋. |
| `tasty pty kill --id <n>` | `pty.kill` | `TerminalWrite` | 소유 프로세스 종료 요청 + collection 항목 회수(실제 reap 완료와 구분)(Surface 를 닫는 게 아님 — headless 라 없음). |
| `tasty pty list` | `pty.list` | `TerminalRead` | 등록된 headless PTY 전체 목록(`id`/`owner_agent_id`/`cwd`/`command`/`has_exited`/`exit_code`/`idle_timeout_secs`). 끝났지만 보존 중인 항목도 포함한다. 접근 시 같은 회수 정책을 적용한다(주기 타이머와 별개로 항상 돈다). |
| `tasty pty attach-surface --pty <n> --pane <p>` | `pty.attach_surface` | `SurfaceWrite, TerminalSpawn` | headless PTY 를 Pane `p` 의 실제 Tab 으로 승격(상태 보존). `{pane_id, tab_id, surface_id}` 반환. |

### headless → 승격 흐름

`pty.spawn` 으로 만든 PTY 는 완전히 숨겨져 있다(오직 `pty.*` 로만 조회/조작). 실제 화면이 필요해지면 같은 pty 를 `pty.attach_surface` 로 승격해 Tab 으로 만든다 — 프로세스·화면 상태가 그대로 옮겨지고, 이후로는 일반 terminal surface(`surface.*`)로 다룬다. 필요할 때만 화면에 연결하는 방식이다.

## 비-목표 (Out of scope)

- **GUI 상시 가시화** — headless PTY 실행 중임을 상태바/점유 계약(docs/dev-guide/attach-behavior.md)으로 노출하는 것은 이번 범위 밖(후속 선택). 승격 전까지는 `pty.list` 로만 보인다.
- **`agent.task` Run 의 pty backend 전환** — DAG 러너 subprocess(`runner_host.rs` 의 `shell_children`) 를 이 PTY 기능으로 옮기는 것은 범위 밖이다. `Run` 은 bare subprocess + `Stdio::piped()` 캡처로 대응한다(argv 의미·exit code 주체·재시작 수명을 그대로 유지하는 게 우선이라 tty 지원은 필요해질 때 재검토) — [dev-guide/agent-runner](../../dev-guide/agent-runner.md#run-출력-캡처).
- **blocking wait** — `pty.wait` 는 즉시 반환 폴링이다(다른 poll-based 모델과 동일). 호출자가 반복 폴링한다.

## Acceptance Criteria

- Given 끝난 항목을 포함해 이미 8개 넘게 등록됨 When `pty.spawn{command}` Then surface와 겹치지 않는 고범위 pty id 반환 + `pty.list` 에 등장, command 즉시 실행.
- Given 실행 중 pty When `pty.write` → 종료 유발 → `pty.wait` Then Pty가 관측한 실제 exit_code 반환.
- Given PTY ID 공간 소진 When `pty.spawn` Then `headless PTY ID space exhausted` 에러(자원 미생성) — panic 없음.
- Given 유휴 제한 없이 실행 중인 pty When 출력 여부와 관계없이 `pty.*` 를 390초 넘게 부르지 않음 Then 프로세스와 `pty.list` 항목이 유지된다.
- Given `idle_timeout_secs` 를 지정한 실행 중 pty When 그 시간 동안 read·write·wait 가 없음 Then 접근 시 정리나 최대 90초 뒤 주기 정리가 프로세스를 끝내고 Terminal/Pty를 함께 회수, 경고 알림을 남기고 같은 ID 의 `pty.*` 는 유휴 제한 만료를 밝히는 `invalid params` 를 받는다.
- Given 끝난 pty When 정리 전 `pty.wait` Then `exited: true` 와 종료 코드. When 보존 기간 경과 Then 정보 알림을 남기고 같은 ID 의 `pty.*` 는 종료 코드와 보존 기간을 밝히는 `invalid params` 를 받는다.
- Given 발급된 적 없는 고범위 ID When `pty.*` Then 지금과 같은 대상 없음 응답(`no live headless pty <id>` 또는 `headless pty <id> not found`).
- Given 회수 기한이 지난 항목이 있음 When `pty.spawn` Then 주기 타이머를 기다리지 않고 같은 회수를 적용한 뒤 spawn 이 성공.
- Given 살아있는 headless pty When `pty.attach_surface{pane}` Then Terminal 이 surface_id 로 re-key(상태 보존) + pane tab 등장 + `pty.list` 에서 제거 + `tab.created` cascade.
- kill/자동 회수/adopt 각각이 회수/재배선하는 pty_id 의 waker dedup 게이트를 정리(`forget_surface`) — 누수 없음.
