# 화면 없는 PTY (`tasty pty`)

- **Status**: Implemented
- **주체**: AI Agent
- **ADR**: [ADR-0013](../../adr/0013-terminal-io-and-process-lifetime.md) (신규 `pty.*` 네임스페이스 결정) · [ADR-0017](../../adr/0017-workspace-identity-and-focus.md) (surface/PTY id 공간 disjoint 집행)
- **코드**: `src/core/pty_registry.rs` (registry+exit-code) · `src/adapters/ipc/handler/pty.rs` (IPC) · `src/core/impl_attach.rs` `apply_adopt_terminal` (승격) · `crates/tasty-cli` `pty` 서브커맨드 (CLI)
- **화면**: 없음 — headless 전용. 렌더되지 않고 포커스/닫은-항목 히스토리/선택에 닿지 않는다(identity.md 원칙 1). 승격(`pty.attach_surface`) 후에만 일반 terminal surface 로 렌더.

## 목적

에이전트가 화면에 탭을 만들지 않고 명령을 실행하고 실제 종료 코드를 받게 한다.
`tasty pty`는 Surface 트리를 바꾸지 않는다. 화면이 필요해지면 실행 중인 Terminal을
`pty.attach_surface`로 기존 Pane의 탭에 연결할 수 있다. Surface를 처음부터 만드는
[자식 터미널](../child-terminal/index.md)과 구분한다.

## 내부 동작 (headless-valid)

### 저장과 종료 코드

`PtyRegistry`는 메타데이터와 종료 코드를, `TerminalStore`는 실제 Terminal을 같은 PTY ID로 보관한다.
`portable_pty::Child`를 받은 watcher가 wait로 실제 종료 코드를 기록한다.
`pty.wait`는 이 값을 즉시 조회하며 블로킹 대기가 아니다.
owner_agent_id는 호출자의 TASTY_AGENT_ID에서 오며 위조할 수 있어 강한 인증 정보로 취급하지 않는다.

### ID 범위

surface ID는 `[1, PTY_ID_BASE)`, PTY ID는 `0x8000_0000` 이상이다.
`is_surface_id_space`를 명령 인덱싱·IPC surface 인자·memory surface scope 검증에서 공통으로 사용한다.
큰 정수를 u32로 바꿀 때는 검사된 변환을 사용한다. headless PTY의 OSC 133은 surface 명령 인덱스에 넣지 않는다.

레이아웃 복원 부팅에서는 surface 카운터의 시작값을 정할 때 PTY 범위의 오래된 scope를 제외하고
오류 로그를 남긴 뒤 삭제한다. 복원을 하지 않는 headless나 restore_layout 비활성 실행에는 이 정리가 없다.
그 경우 카운터가 1부터 시작하므로 잘못된 scope에서 시작값을 물려받지는 않는다.

### 자원 회수

동시 개수 기본 상한은 8, idle TTL은 5분이다. 상한 초과는 오류로 반환한다.
read·write·wait는 idle 시각을 갱신하며 기본값은 override할 수 있다.

- spawn/list 접근에서 만료 항목을 먼저 정리한다. 특히 spawn의 상한 판단보다 먼저 수행한다.
- Tick::PtySweep는 30초 주기, Lax slack 60초로 정리한다. 아무 호출이 없어도 TTL 뒤 최대 90초 안에 회수한다.
- GUI main·parked engine과 headless 모두 같은 CoreState::sweep_idle_ptys를 사용한다.
  registry, TerminalStore, waker 중복 방지 등록을 함께 정리한다.

자식은 PTY를 소유한 host 수명을 따른다. Windows의 Job Object와 Unix의 hangup 차이는
[터미널 수명](../terminal/index.md#프로세스-종료--절전-복귀)을 참고한다.

### 화면에 연결하기

`AdoptTerminal { pane_id, pty_id }`는 새 surface ID를 만들고 TerminalStore의 키를 옮긴다.
새 ID로 waker를 연결하고 기존 Pane에 background tab을 추가한다. 프로세스와 화면 내용은 유지한다.
사용자의 활성 탭은 바꾸지 않으며 tab.created·surface.created를 보낸다.
PTY registry에서 제거되어 이후에는 surface API로 다룬다. 옛 PTY ID의 waker 등록도 정리한다.

## 인터페이스

- **AI Agent (IPC/CLI)** — 모든 대상은 pty id 로 직접 지정(포커스 독립, 원칙 3). `pty.list` 는 필터 없이 전 목록 반환.

| CLI | IPC method | 권한 | 목적 |
|-----|-----------|------|------|
| `tasty pty spawn [--cwd <dir>] [-- <cmd>...]` | `pty.spawn` | `TerminalSpawn` | headless PTY 를 띄우고 pty id 반환. command(trailing var-arg) 생략 시 bare shell, 지정 시 즉시 실행(initial stdin 주입). 상한 초과 시 에러. |
| `tasty pty write --id <n> "<text>"` | `pty.write` | `TerminalWrite` | 실행 중 PTY 에 stdin 을 as-is 전송(자동 제출 없음 — 개행은 호출자 포함). idle 리셋. |
| `tasty pty read --id <n> [--lines <k>] [--show-dim]` | `pty.read` | `TerminalRead` | 현재 화면 텍스트 읽기(옵션 `lines`=내용 기준 마지막 N줄 — 하단 공백 행 건너뜀, 모자라면 스크롤백에서 채움). `surface.screen_text` 와 동일 추출이며 진단 필드(`is_terminal`·`scrollback_len`·`alt_screen`)도 같이 낸다 — N 보다 적게 왔을 때 "그게 전부" 와 "잘렸다" 를 가른다. idle 리셋. dim(ghost-suggestion, 예: Claude Code 자동완성 제안) 셀은 기본 제외 — `--show-dim`/`show_dim:true` 로 포함. |
| `tasty pty wait --id <n>` | `pty.wait` | `TerminalRead` | 즉시 반환 폴링(blocking 아님). exit cell 조회 → `{exited, exit_code, success}`. idle 리셋. |
| `tasty pty kill --id <n>` | `pty.kill` | `TerminalWrite` | 프로세스 종료 + 두 store 회수(Surface 를 닫는 게 아님 — headless 라 없음). |
| `tasty pty list` | `pty.list` | `TerminalRead` | 살아있는 headless PTY 전체 목록(`id`/`owner_agent_id`/`cwd`/`command`/`has_exited`/`exit_code`). 접근 시 idle sweep(주기 타이머와 별개로 항상 돈다). |
| `tasty pty attach-surface --pty-id <n> --pane-id <p>` | `pty.attach_surface` | `SurfaceWrite, TerminalSpawn` | headless PTY 를 Pane `p` 의 실제 Tab 으로 승격(상태 보존). `{pane_id, tab_id, surface_id}` 반환. |

### headless → 승격 흐름

`pty.spawn` 으로 만든 PTY 는 완전히 숨겨져 있다(오직 `pty.*` 로만 조회/조작). 실제 화면이 필요해지면 같은 pty 를 `pty.attach_surface` 로 승격해 Tab 으로 만든다 — 프로세스·화면 상태가 그대로 옮겨지고, 이후로는 일반 terminal surface(`surface.*`)로 다룬다. 완전 숨김과 가시화 사이의 탈출구다.

## 비-목표 (Out of scope)

- **GUI 상시 가시화** — headless PTY 실행 중임을 상태바/점유 계약(ADR-0021)으로 노출하는 것은 이번 범위 밖(후속 선택). 승격 전까지는 `pty.list` 로만 보인다.
- **`agent.task` Run 의 pty backend 전환** — DAG 러너 subprocess(`runner_host.rs` 의 `shell_children`) 를 이 primitive 위로 옮기는 것은 범위 밖이다. `Run` 은 bare subprocess + `Stdio::piped()` 캡처로 대응한다(argv 의미·exit code 주체·재시작 수명을 그대로 유지하는 게 우선이라 tty 지원은 필요해질 때 재검토) — [dev-guide/agent-runner](../../dev-guide/agent-runner.md#run-출력-캡처).
- **blocking wait** — `pty.wait` 는 즉시 반환 폴링이다(다른 poll-based 모델과 동일). 호출자가 반복 폴링한다.

## Acceptance Criteria

- Given 상한 미만 When `pty.spawn{command}` Then disjoint 고범위 pty id 반환 + `pty.list` 에 등장, command 즉시 실행.
- Given 실행 중 pty When `pty.write` → 종료 유발 → `pty.wait` Then watcher 가 잡은 실제 exit_code 반환.
- Given 상한 도달 When `pty.spawn` Then `LimitReached` 에러(자원 미생성) — panic 없음.
- Given idle 이 TTL 초과 When `pty.spawn`/`pty.list` 접근 Then 두 store 에서 함께 회수.
- Given idle 이 TTL 초과 When `pty.*` 를 **한 번도 부르지 않음** Then 주기 타이머가 최대 90초 안에 두 store 에서 함께 회수.
- Given 상한이 꽉 찼고 그 항목들이 idle 만료 When `pty.spawn` Then lazy sweep 이 먼저 돌아 spawn 이 성공(주기 타이머를 기다리지 않는다).
- Given 살아있는 headless pty When `pty.attach_surface{pane}` Then Terminal 이 surface_id 로 re-key(상태 보존) + pane tab 등장 + `pty.list` 에서 제거 + `tab.created` cascade.
- kill/idle-sweep/adopt 각각이 회수/재배선하는 pty_id 의 waker dedup 게이트를 정리(`forget_surface`) — 누수 없음.
