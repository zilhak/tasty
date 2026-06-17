# Invariants

*깨지면 안 되는 시스템 약속* — 코드 변경 시 가장 먼저 점검할 리스트. 각 invariant 는 가능하면 컴파일/CI 로 강제하고, 그게 불가능하면 review 로 지킨다.

| Invariant | 적용 시점 | 강제 기제 |
|-----------|----------|----------|
| [surface-cwd](surface-cwd.md) | surface 생성/변환 | `Surface::source_cwd()` default 없음 — compile-time |
| 포커스 독립성 | 모든 CLI/IPC 명령 | review (전 워크스페이스 순회·ID 직접 지정) — [focus 정책](../../design/policies/focus.md) |
| 사용자/에이전트 행동 분리 | release 빌드 IPC 노출 | review + `#[cfg(debug_assertions)]` 격리 — [debug-ipc](../../dev-guide/debug-ipc.md) |
| Intent 디스패치 규율 | 호스트 내부 동작 | `check-intent-discipline.sh` grep CI — [action-dispatch](../../design/flows/action-dispatch.md) |

> 새 invariant 는 *위반이 조용히 통과하면 큰 회귀* 인 약속만 등재한다. 일반 코딩 규칙은 [CLAUDE.md](../../../CLAUDE.md)/dev-guide 로.
