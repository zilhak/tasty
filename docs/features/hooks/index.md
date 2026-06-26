# 훅 (Surface / Global hooks)

- **Status**: Implemented
- **주체**: 로컬 사용자 · AI Agent (`hook.*` / `global_hook.*`)
- **ADR**: 없음
- **코드**: `tasty-hooks` 크레이트(`HookManager`/`HookEvent`), `hook.*`·`global_hook.*` 핸들러
- **화면**: 없음

## 목적

특정 이벤트 발생 시 셸 명령을 자동 실행하는 훅. surface 별 이벤트 훅(`hook.*`)과 surface 무관 글로벌 훅(`global_hook.*`)이 있다. "에이전트가 에이전트를 제어하는 자동화" 의 토대(conductor 가 polling 없이 자식 완료를 감지).

## 내부 동작

### Surface hook (`HookEvent`)

`HookManager` 가 등록/삭제/조회/실행 관리. 이벤트 타입:

| 이벤트 | 발화 |
|--------|------|
| `ProcessExit` | 셸 프로세스 종료 |
| `OutputMatch(pattern)` | PTY 출력이 정규식 매칭(등록 시 사전 컴파일) |
| `Bell` | BEL 수신 |
| `Notification` | OSC 알림 수신 |
| `IdleTimeout(secs)` | N초간 PTY 출력 없음 |
| `Custom(string)` | 코어가 모르는 임의 이벤트 식별자. 정확 문자열 일치로 매칭. 플러그인 소유 이벤트(예: claude plugin 이 fire 하는 `claude-idle` / `needs-input` / `claude-error` / `claude-child-idle` / `claude-child-needs-input`)는 모두 이 변형으로 처리된다 — 코어에 에이전트 고유 이벤트명을 박지 않는다. |

- **once** 옵션: true 면 한 번 실행 후 자동 삭제. 기본은 persistent.
- **비동기 실행**: 훅 명령은 백그라운드 스레드에서(메인 루프 블로킹 없음). 각 이벤트의 발생 surface ID 를 추적해 올바른 surface 에서 실행.
- ProcessExit 은 surface 자동 닫기까지(surface→tab→pane→workspace 계층 정리, 마지막이면 새 셸 spawn).

### Global hook (조건)

surface 무관 — `condition` 으로 트리거:

- `interval:SECS` — 매 N초 반복
- `once:SECS` — N초 후 1회 실행 후 자동 삭제
- `file:/path` — 파일 수정 감지 시

## 인터페이스

- **사용자/AI Agent/CLI**:
  - `hook.set`/`hook.list`/`hook.unset` — `tasty set hook --event bell --command "..." [--once]`
  - `global_hook.set`/`list`/`unset` — `tasty set global-hook --condition interval:60 --command "..." [--label ...]`
  - 표 → [reference/api](../../reference/api.md#기타-호스트)

## 관련

- [agent-collaboration](../agent-collaboration/index.md) · [notifications](../notifications/index.md) · [claude plugin](../../plugins/claude/index.md)(Claude hook 발화)
