# ADR-0050: agent-native headless PTY 는 `terminal.*` 확장이 아니라 신규 `pty.*` 네임스페이스로 제공한다

- **Status**: Accepted
- **Date**: 2026-07-14
- **Tags**: pty, headless, agent-native, terminal, surface-independence, ipc, cli, permission, adopt-terminal, exit-code, concurrency-limit, idle-ttl, adr-0040

## Context

에이전트가 Surface(Tab) 없이 백그라운드에서 1회성 명령/자동화를 돌리고 **진짜 exit-code** 를 회수해야 하는 요구가 있었다. 기존 자식 터미널 기계(`terminal.*`, `child_terminal.rs`)는 이 요구를 그대로 충족하지 못한다:

- **`terminal.*` 는 Surface 존재를 전제한다.** `terminal.spawn` 은 새 Tab(Surface)을 만들어 GUI 에 렌더하고, `terminal.kill` 은 Surface 를 닫는 연산이다. 그래서 권한 매핑에도 Surface 토큰이 섞인다 — `terminal.spawn → [SurfaceWrite, TerminalWrite, TerminalSpawn]`, `terminal.kill → [SurfaceWrite]`. Surface 는 곧 포커스/닫은-항목 히스토리/선택 같은 **사용자 상태에 닿는 지점**이다(identity.md 원칙 1).
- headless PTY 는 정의상 Surface 트리를 전혀 건드리지 않아야 한다 — 렌더되지 않고, 사용자 상태에 닿을 지점 자체가 없다. `TerminalStore`(`CoreState::terminals`)는 이미 Surface 트리와 분리된 flat `HashMap<SurfaceId, Terminal>` 이고, 출력 이벤트 게이트(`sync_output_event_gates`/`ObserverRouter::wants`)는 Surface 트리를 조회하지 않는 단순 값 비교라, Surface 노드가 없는 orphan id 를 넣어도 크래시 위험이 없음을 확인했다.
- `terminal.*` 에는 **동시 개수 상한도 idle TTL 도 없다** — 자식이 Tab 으로 보여 사용자가 눈으로 닫을 수 있는 GUI 안전망이 있기 때문이다. headless PTY 는 그 안전망이 없어 좀비 누적을 스스로 막아야 한다.

## Decision

**headless PTY 를 `terminal.*` 확장이 아니라 신규 `pty.*` 네임스페이스로 제공한다.** 구성:

- **별도 registry**(`src/core/pty_registry.rs`, `PtyRegistry`) — Surface 없는 headless PTY 의 메타데이터 + exit-code cell 을 호스트 SoT 로 보관. `child_terminal.rs` 와 병렬 구조지만 역할이 다르다(그쪽은 GUI 에 보이는 장수명 child-agent surface). 비영속(자식 프로세스는 호스트와 수명을 같이함).
- **disjoint id 공간** — pty id 는 `PTY_ID_BASE`(`0x8000_0000`) 이상에서 발급해 Surface id(1부터 증가)와 절대 충돌하지 않는다. 같은 pty id 를 `pty_registry`(메타/exit)와 `terminals`(`TerminalStore`, 실제 `Terminal`) 두 store 에 공유 등록한다.
- **좀비 방지 정책** — 동시 개수 상한(기본 8) + idle TTL(기본 5분). 상한 초과 시 spawn 을 실패시키고(panic 하지 않음), idle TTL 초과 항목은 접근 시점 lazy sweep 으로 두 store 에서 함께 회수한다. `rate_limit.rs` 철학대로 기본값은 코드에 박되 override 가능.
- **진짜 exit-code 캡처** — `runner_host.rs` 의 watcher-thread 패턴을 이식: 소유한 `portable_pty::Child` 를 close-over 한 detached 스레드가 `child.wait()` 로 실제 종료코드를 뽑아 entry 의 exit cell 에 채운다. `pty.wait` 는 Surface 라이브 여부가 아니라 이 cell 로 판정한다(즉시 반환 폴링).
- **6개 IO 메서드 + 승격 1개** — `pty.spawn`/`write`/`read`/`wait`/`kill`/`list` 는 Surface 를 전혀 건드리지 않으므로 권한에 `Surface*` 토큰이 섞이지 않고 기존 `Terminal*` 3종만 재사용한다(`spawn→[TerminalSpawn]`, `write/kill→[TerminalWrite]`, `read/wait/list→[TerminalRead]`). 새 `Pty*` 권한 토큰은 만들지 않는다.
- **승격 경로 `pty.attach_surface`** — headless PTY 를 실제 Surface(Tab)로 adopt 하는 탈출구. 이건 진짜 Surface 를 만들므로 `[SurfaceWrite, TerminalSpawn]` 을 쓴다. 구현상 `surface.respawn_terminal`(`RespawnTerminal`, 같은 surface_id 위에서 `TerminalStore::replace`)과 **반대 방향** 연산이다 — attach 는 (1) 새 surface_id 발급 (2) 기존 `Terminal` 을 pty_id 에서 remove (3) 새 surface_id 로 re-key insert + waker 재배선 하는 조합이라, 신규 `DomainIntent::AdoptTerminal { pane_id, pty_id }` 를 도입해 처리한다.

## Consequences

- **얻은 것**: 에이전트가 사용자 상태에 전혀 닿지 않는 headless PTY 를 굴리고 진짜 exit-code 를 회수할 수 있다(identity.md 원칙 1 정합). 권한 표면이 최소(`Terminal*` 3종 재사용)라 기존 plugin 이 이미 grant 받은 토큰으로 커버되어 재승인 피로가 없다. Surface 가 필요해지면 `attach_surface` 로 상태 보존하며 승격 가능 — 완전 숨김과 가시화 사이의 탈출구.
- **잃은 것**: `terminal.*` 와 `pty.*` 두 계열이 공존해 표면이 늘었다(개념 중복이 아니라 Surface 유무로 갈리는 별개 축이지만, 학습 비용은 증가). 두 store(registry/TerminalStore)를 항상 함께 정리해야 하는 정합 부담이 생긴다 — 어느 한 쪽만 지우면 누수/좀비.
- **운영 비용 / 유지 부담**: `sync_output_event_gates` 가 매 tick headless PTY 도 순회하므로 개수에 비례한 O(N) 비용이 있으나 동시 개수 상한이 이를 bound 한다. idle TTL 은 SSH 원격 등 오래 걸리는 명령을 고려해 기본값을 정했고 override 가능. GUI 최소 가시화(상태바 카운트 등)는 이번 결정 범위 밖의 후속 선택으로 남긴다.

## Alternatives Considered

- **A. 기존 `terminal.*` 확장** (자식 터미널 기계에 headless 플래그/옵션을 추가) — 기각. `terminal.*` 은 Surface 생성·라이브 트리 판정·Surface 닫기에 묶여 있고 권한도 `Surface*` 를 포함한다. headless 를 여기 얹으면 "Surface 를 만들되 안 보이게" 같은 특수 분기가 핵심 경로에 번지고, headless 인데도 `SurfaceWrite`/`SurfaceRead` 권한을 요구하게 되어 사용자 상태 비접촉 원칙이 깨진다. Surface 유무는 옵션이 아니라 **다른 축**이라 네임스페이스를 가르는 게 더 깨끗하다.
- **B. `runner_host` 의 shell 서브프로세스 관리에 편입** — 기각. `runner_host` 의 `shell_children` 은 `agent.task` DAG 러너의 subprocess 추적에 특화돼 있고, PTY(터미널 emulation·화면 텍스트 읽기·stdin 주입) 가 아니라 stdout/stderr 캡처 모델이다. `pty.read`(화면 텍스트)·`pty.write`(as-is stdin) 요구와 맞지 않는다. exit-code watcher **패턴**만 이식하고 registry 는 분리했다.
- **C. `pty.*` 전용 새 권한 토큰(`PtySpawn`/`PtyWrite`/…) 신설** — 기각. 기존 `TerminalSpawn`/`TerminalWrite`/`TerminalRead` 3종이 "Surface 유무와 무관한 PTY 자체 권한"으로 이미 세분화돼 있어 그대로 재사용 가능하다. 새 토큰은 `plugin-permissions.md` 의 5단계 추가 절차를 요구하고, 기존 plugin 이 별도 재승인을 받아야 해서 승인 피로만 늘린다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

- headless PTY 실행을 GUI 에 상시 노출(상태바 카운트 이상의 occupancy 편입)해야 하는 요구가 생겨 Surface 없는 개념과 점유 계약(ADR-0040)의 경계가 모호해질 때.
- `agent.task` Run 을 pty backend 로 전환(TODO 19)하면서 `runner_host` shell 관리와 `pty_registry` 를 통합하는 게 이득이 될 때.
- 동시 개수 상한/idle TTL 기본값이 실사용(장수명 SSH 세션 등)에서 반복적으로 부적절하다고 드러나 정책 모델 자체(호스트 고정 vs agent-configurable)를 바꿔야 할 때.
- `TerminalSpawn`/`Write`/`Read` 재사용이 headless 와 Surface-기반 사용을 권한상 구분하지 못해 보안/감사 문제가 될 때.

## References

- [features/headless-pty](../features/headless-pty/index.md) — 기능 문서(메서드·권한·승격 흐름)
- [features/child-terminal](../features/child-terminal/index.md) — Surface 기반 자식 터미널(대비 대상)
- [ADR-0040](0040-occupancy-soft-hard-tiers-agent-occupant.md) — 점유 soft/hard 계층(headless 는 Surface 가 없어 이 계약 밖)
- [dev-guide/plugin-permissions](../dev-guide/plugin-permissions.md) — 권한 토큰 모델
- 코드: `src/core/pty_registry.rs` · `src/adapters/ipc/handler/pty.rs` · `src/core/mod.rs`(`apply_adopt_terminal`) · `crates/tasty-terminal/src/lib.rs`(waker 재배선)
