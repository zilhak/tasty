# headless-pty (Surface 없는 agent-native PTY primitive · `tasty pty`)

- **Status**: Implemented
- **주체**: AI Agent
- **ADR**: [ADR-0050](../../adr/0050-headless-pty-primitive.md) (신규 `pty.*` 네임스페이스 결정)
- **코드**: `src/core/pty_registry.rs` (registry+exit-code) · `src/adapters/ipc/handler/pty.rs` (IPC) · `src/core/mod.rs` `apply_adopt_terminal` (승격) · `crates/tasty-cli` `pty` 서브커맨드 (CLI)
- **화면**: 없음 — headless 전용. 렌더되지 않고 포커스/닫은-항목 히스토리/선택에 닿지 않는다(identity.md 원칙 1). 승격(`pty.attach_surface`) 후에만 일반 terminal surface 로 렌더.

## 목적

에이전트가 **Surface(Tab) 없이** 백그라운드에서 1회성 명령/자동화를 돌리고 **진짜 exit-code** 를 회수하는 primitive 를 호스트 1급으로 제공한다. 자식 터미널([child-terminal](../child-terminal/index.md), `terminal.*`)이 GUI 에 보이는 장수명 child-agent *surface* 를 만드는 것과 달리, `pty.*` 는 Surface 트리를 전혀 건드리지 않는다. Surface 유무는 옵션이 아니라 별개 축이라 네임스페이스를 갈랐다(ADR-0050). 필요해지면 `pty.attach_surface` 로 상태 보존하며 실제 Tab 으로 승격할 수 있다.

## 내부 동작 (headless-valid)

- **두 store 정합**: 메타데이터·exit-code cell 은 `engine.pty_registry`(`PtyRegistry`), 실제 headless `Terminal` 은 `engine.terminals`(`TerminalStore`)에 **같은 pty id** 로 보관한다. pty id 는 `PTY_ID_BASE`(`0x8000_0000`) 이상 disjoint 범위에서 발급해 Surface id(1부터 증가)와 절대 충돌하지 않는다. 어느 한 쪽만 지우면 누수/좀비가 되므로 kill/sweep 은 **항상 두 store 를 함께** 정리한다.
- **좀비 방지 (동시 개수 상한 + idle TTL)**: GUI 안전망(닫기 버튼)이 없으므로 호스트가 스스로 회수한다. 동시 개수 기본 상한 8 — 초과 시 spawn 을 실패시킨다(panic 하지 않음). idle(무 IO 활동) TTL 기본 5분 — `pty.spawn`/`pty.list` 접근 시점에 lazy sweep 으로 만료 항목을 두 store 에서 함께 회수한다(`Terminal` drop → PTY master close → 자식 SIGHUP). read/write/wait 은 idle 타이머를 리셋하므로 활발히 폴링 중인 PTY 는 회수되지 않는다. 상한/TTL 은 기본값을 코드에 박되 override 가능(`rate_limit.rs` 철학).
- **진짜 exit-code 캡처**: spawn 시 `Terminal` 에서 넘겨받은 `portable_pty::Child` 를 close-over 한 detached watcher 스레드가 `child.wait()` 로 실제 종료코드를 뽑아 entry 의 exit cell 에 채운다(`runner_host.rs` 패턴 이식). `pty.wait` 는 Surface 라이브 여부가 아니라 이 cell 로 판정한다.
- **owner 귀속**: 각 PTY 는 spawn 한 caller 의 `owner_agent_id` 를 기록한다(cap/telemetry 귀속용, `TASTY_AGENT_ID` 기반 — 위조 가능한 잠정 모델).
- **승격 (adopt)**: `pty.attach_surface` 는 `AdoptTerminal { pane_id, pty_id }` intent 로 실행한다. 새 surface_id 를 발급하고, headless `Terminal` 을 pty_id → surface_id 로 **re-key**(`TerminalStore` remove→insert)하며 새 surface_id 로 waker 를 재배선한 뒤, pane 트리에 background tab 으로 등록한다(포커스 독립 — active_tab 을 바꾸지 않음). `tab.create` 와 동형 cascade(`tab.created`/`surface.created` host event)를 발화해 GUI 가 렌더하게 한다. 승격 후 그 pty id 는 registry 에서 빠져 `pty.list` 에 더 이상 나타나지 않는다(같은 `Terminal` 인스턴스라 화면 상태 보존).

## 인터페이스

- **AI Agent (IPC/CLI)** — 모든 대상은 pty id 로 직접 지정(포커스 독립, 원칙 3). `pty.list` 는 필터 없이 전 목록 반환.

| CLI | IPC method | 권한 | 목적 |
|-----|-----------|------|------|
| `tasty pty spawn [--cwd <dir>] [-- <cmd>...]` | `pty.spawn` | `TerminalSpawn` | headless PTY 를 띄우고 pty id 반환. command(trailing var-arg) 생략 시 bare shell, 지정 시 즉시 실행(initial stdin 주입). 상한 초과 시 에러. |
| `tasty pty write --id <n> "<text>"` | `pty.write` | `TerminalWrite` | 실행 중 PTY 에 stdin 을 as-is 전송(자동 제출 없음 — 개행은 호출자 포함). idle 리셋. |
| `tasty pty read --id <n> [--lines <k>] [--show-dim]` | `pty.read` | `TerminalRead` | 현재 화면 텍스트 읽기(옵션 `lines`=하단 N줄). `surface.screen_text` 와 동일 추출. idle 리셋. dim(ghost-suggestion, 예: Claude Code 자동완성 제안) 셀은 기본 제외 — `--show-dim`/`show_dim:true` 로 포함. |
| `tasty pty wait --id <n>` | `pty.wait` | `TerminalRead` | 즉시 반환 폴링(blocking 아님). exit cell 조회 → `{exited, exit_code, success}`. idle 리셋. |
| `tasty pty kill --id <n>` | `pty.kill` | `TerminalWrite` | 프로세스 종료 + 두 store 회수(Surface 를 닫는 게 아님 — headless 라 없음). |
| `tasty pty list` | `pty.list` | `TerminalRead` | 살아있는 headless PTY 전체 목록(`id`/`owner_agent_id`/`cwd`/`command`/`has_exited`/`exit_code`). 접근 시 idle sweep. |
| `tasty pty attach-surface --pty-id <n> --pane-id <p>` | `pty.attach_surface` | `SurfaceWrite, TerminalSpawn` | headless PTY 를 Pane `p` 의 실제 Tab 으로 승격(상태 보존). `{pane_id, tab_id, surface_id}` 반환. |

### headless → 승격 흐름

`pty.spawn` 으로 만든 PTY 는 완전히 숨겨져 있다(오직 `pty.*` 로만 조회/조작). 실제 화면이 필요해지면 같은 pty 를 `pty.attach_surface` 로 승격해 Tab 으로 만든다 — 프로세스·화면 상태가 그대로 옮겨지고, 이후로는 일반 terminal surface(`surface.*`)로 다룬다. 완전 숨김과 가시화 사이의 탈출구다.

## 비-목표 (Out of scope)

- **GUI 상시 가시화** — headless PTY 실행 중임을 상태바/점유 계약(ADR-0040)으로 노출하는 것은 이번 범위 밖(후속 선택). 승격 전까지는 `pty.list` 로만 보인다.
- **`agent.task` Run 의 pty backend 전환** — DAG 러너 subprocess(`runner_host.rs` 의 `shell_children`) 를 이 primitive 위로 옮기는 것은 범위 밖이다. `Run` 은 bare subprocess + `Stdio::piped()` 캡처로 대응한다(argv 의미·exit code 주체·재시작 수명을 그대로 유지하는 게 우선이라 tty 지원은 필요해질 때 재검토) — [dev-guide/agent-runner](../../dev-guide/agent-runner.md#run-출력-캡처).
- **blocking wait** — `pty.wait` 는 즉시 반환 폴링이다(다른 poll-based 모델과 동일). 호출자가 반복 폴링한다.

## Acceptance Criteria

- [x] Given 상한 미만 When `pty.spawn{command}` Then disjoint 고범위 pty id 반환 + `pty.list` 에 등장, command 즉시 실행.
- [x] Given 실행 중 pty When `pty.write` → 종료 유발 → `pty.wait` Then watcher 가 잡은 실제 exit_code 반환.
- [x] Given 상한 도달 When `pty.spawn` Then `LimitReached` 에러(자원 미생성) — panic 없음.
- [x] Given idle 이 TTL 초과 When `pty.spawn`/`pty.list` 접근 Then 두 store 에서 함께 회수.
- [x] Given 살아있는 headless pty When `pty.attach_surface{pane}` Then Terminal 이 surface_id 로 re-key(상태 보존) + pane tab 등장 + `pty.list` 에서 제거 + `tab.created` cascade.
- [x] kill/idle-sweep/adopt 각각이 회수/재배선하는 pty_id 의 waker dedup 게이트를 정리(`forget_surface`) — 누수 없음.
