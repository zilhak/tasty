# child-terminal (호스트 내재화 자식 터미널 관리 · `tasty terminal`)

- **Status**: Implemented
- **주체**: AI Agent
- **ADR**: [ADR-0040](../../adr/0040-occupancy-soft-hard-tiers-agent-occupant.md) (soft 점유 소비자)
- **코드**: `src/core/child_terminal.rs` (registry) · `src/adapters/ipc/handler/terminal.rs` (IPC) · `crates/tasty-cli/src/commands/terminal.rs` (CLI)
- **화면**: 없음 — headless 전용 (자식은 일반 terminal surface 로 렌더; soft 점유 테두리는 [surface-highlight](../surface-highlight/index.md)/점유 계약)

## 목적

에이전트가 자식 터미널 surface 를 spawn/tell/wait/kill 하는 **범용 기계**를 호스트 1급으로 제공한다. 같은 기계가 codex/claude 플러그인에 각각 중복돼 있었는데, 특정 에이전트 바이너리에 묶이지 않는 부분(자식 registry·spawn 조합·self-heal)을 호스트로 끌어올려 단일 SoT 로 수렴한다 (CLAUDE.md 원칙 2: 에이전트 기능은 IPC+CLI 양면·호스트 1급).

## 내부 동작 (headless-valid)

- **registry**(`ChildTerminalRegistry`): `parent_surface → 자식 목록`, `child_surface → parent`, parent별 다음 index, child별 idle/needs_input 상태를 보관한다. `~/.tasty/child-terminals.json` 에 영속(등록/제거마다 즉시 save). session token 추적(`session.rs`)·agent shell 서브프로세스 추적(`runner_host` `shell_children`)과는 **다른 서브시스템**이다.
- **spawn**: 대상 workspace 의 pane 에 `terminal` 탭을 만들고(호출자 지정 `--command` 를 그대로 전송) child 를 registry 에 등록한 뒤, 그 child 를 **soft 점유**로 표시한다(주체 = spawn 을 발동한 parent surface). command 는 임의 문자열 — 에이전트 특화 command 빌더는 플러그인에 잔류.
- **soft 점유 연결**: spawn 성공 시 `occupy_soft(child, parent)`, kill 시 `release_occupancy(child)` 를 **in-process core 함수**로 호출한다 (`occupancy.*` IPC method 는 없다 — ADR-0040 경계). soft 점유는 표시만 하고 입력을 차단하지 않으며(`attached=false`), parent 가 죽으면 focus 지연 청소로 풀린다.
- **wait**: 1-tick snapshot. child 가 `active` 인데 surface 가 라이브 트리에 없으면 `exited` 로 강등한다. idle/needs_input 은 registry 상태값을 그대로 반환. 상태값을 넣는 신호원(`set_state`)은 에이전트 hook 이 채운다.
- **self-heal**: 호스트가 라이브 surface 트리를 직접 소유하므로, 접근 시점마다 라이브 집합과 대조해(reconcile) 죽은 자식을 registry 에서 정리한다(이벤트 구독 없이 동기). 부팅 후 첫 접근이 이전 세션 잔재를 회수한다.

## 인터페이스

- **AI Agent (IPC/CLI)** — 모든 대상은 ID 로 직접 지정(포커스 독립, 원칙 3):
  - `tasty terminal spawn --workspace <ws> --command "<cmd>" [--surface <parent>] [--pane] [--cwd] [--role] [--nickname] [--wait] [--timeout]` ↔ `terminal.spawn`
  - `tasty terminal tell "<text>" [--surface] [--wait]` ↔ `terminal.tell`
  - `tasty terminal wait [--surface] [--child] [--timeout]` ↔ `terminal.wait`
  - `tasty terminal children [--surface]` ↔ `terminal.children`
  - `tasty terminal parent --surface <child>` ↔ `terminal.parent`
  - `tasty terminal kill [--surface] --child <n>` ↔ `terminal.kill`
  - `tasty terminal respawn [--surface] --child <n> [--cwd] [--command] [--role] [--nickname]` ↔ `terminal.respawn`
  - `tasty terminal broadcast "<text>" [--surface] [--role]` ↔ `terminal.broadcast`
  - `tasty terminal set-state --surface <child> --state <idle|needs_input|active>` ↔ `terminal.set_state` (에이전트 hook 진입점)
- **wait/auto-wait**: `terminal wait` 는 terminal state(`idle`/`needs_input`/`exited`) 도달까지 polling. `spawn`/`tell` 은 `--wait` 지정 시에만 응답 직후 `terminal.wait` 를 chain 한다 — 호스트엔 아직 idle 신호원이 없어 opt-in(에이전트 hook 이 `set_state` 로 신호를 넣는 배선 이후 의미).

## 비-목표 (Out of scope)

- **에이전트 특화 로직** — codex/claude 바이너리 command 빌더, hook/trust, telemetry 는 플러그인에 잔류한다. 호스트는 임의 command 만 붙인다.
- **`terminal launch`(새 workspace 생성)** — 에이전트 편의 명령이라 범위 밖.
- **soft 입력 독립성(잔여 입력 비오염)** — 현재는 단순 `surface.send` 로 붙인다(ADR 방향성만).

## Acceptance Criteria

- [x] Given workspace `<ws>` When `terminal.spawn{parent=P, command}` Then 자식 터미널이 생성·registry 등록되고 `occupancy_of(child)==Soft`·`holder.parent==P`·`attached=false`.
- [x] Given 점유된 child C When `terminal.kill` Then `occupancy_of(C)==None` + surface 닫힘.
- [x] Given child surface 가 라이브 트리에 없음 When `terminal.wait{surface=child}` Then `state=="exited"`.
- [x] Given 죽은 자식이 남은 registry When `terminal.children` Then reconcile 로 목록에서 제거.
