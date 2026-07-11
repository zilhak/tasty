# 훅 (Surface / Global hooks)

- **Status**: Implemented
- **주체**: 로컬 사용자 · AI Agent (`hook.*` / `global_hook.*`)
- **ADR**: 없음
- **코드**: `tasty-hooks` 크레이트(`HookManager`/`HookEvent`/`HookBinding`), `hook.*`·`global_hook.*` 핸들러, 실행 배선 `src/hook_handler/trigger.rs`
- **화면**: 없음

## 목적

특정 이벤트 발생 시 동작을 자동 실행하는 훅. surface 별 이벤트 훅(`hook.*`)과 surface 무관 글로벌 훅(`global_hook.*`)이 있다. "에이전트가 에이전트를 제어하는 자동화" 의 토대(conductor 가 polling 없이 자식 완료를 감지).

surface hook 은 더 이상 셸 명령 문자열을 직접 들지 않고, **공유 훅 핸들러 레지스트리**(`src/hook_handler/`)의 핸들러를 참조한다 — 웹훅과 훅이 같은 핸들러 정의를 공유하는 구조다. 기존 `--command` 인라인 셸은 하위호환을 위해 **익명 hook 핸들러**로 감싸 그대로 실행된다.

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

#### 이벤트 키 검증 (내장 + 플러그인 선언)

`HookEvent::parse` 는 미인식 문자열을 `Custom(String)` 으로 무조건 수용하므로(파싱·검증 책임 분리), `hook.set` / `surface.fire_hook` 핸들러 단계에서 키를 **(내장 ∪ 활성 플러그인 선언)** 집합으로 검증한다.

- **내장 이벤트**(`process-exit` / `bell` / `notification` / `output-match:` / `idle-timeout:`)는 플러그인 무관하게 항상 허용.
- **플러그인 선언 이벤트**는 플러그인이 manifest `[[contributes.hook_events]]` 로 자기가 발사하는 키를 선언해야 한다. 코어는 이름을 하드코딩하지 않고 이 카탈로그를 활성 플러그인 hello 시 집계한다(언로드/제거 시 제거).
- 내장도 아니고 활성 플러그인이 선언하지도 않은 키(오타·미존재 이벤트)는 **등록 거부**(`invalid_params`, 에러 메시지에 내장 + 활성 선언 목록 안내). 죽은 hook 등록을 막는다.
- 따라서 **플러그인이 비활성이면 그 플러그인의 이벤트 hook 등록도 거부**된다(예: claude plugin 비활성 시 `claude-idle` hook 등록 불가 — 의도된 dead-setting 방지). claude plugin 은 위 5개 키를 manifest 로 선언한다.

- **once** 옵션: true 면 한 번 실행 후 자동 삭제. 기본은 persistent.
- **비동기 실행**: 훅 동작은 백그라운드에서(메인 루프 블로킹 없음). 각 이벤트의 발생 surface ID 를 추적해 올바른 surface 에서 실행.
- ProcessExit 은 surface 자동 닫기까지(surface→tab→pane→workspace 계층 정리, 마지막이면 새 셸 spawn).

#### 바인딩 (핸들러 참조 vs 인라인 셸)

surface hook 은 `HookBinding` 으로 무엇을 실행할지 표현한다:

- **`Handler(id)`** — 공유 훅 핸들러 레지스트리 핸들러 id 참조(`tasty set hook --handler <id>`). 등록 시 핸들러가 존재하고 `source` 가 hook 트리거를 수용(`hook` 또는 `any`)하는지 검증한다 — `webhook` 전용 핸들러는 거부된다.
- **`InlineShell(cmd)`** — 하위호환 익명 셸(`tasty set hook --command "..."`). 레지스트리에 등록되지 않는 인라인 핸들러라 export/영속화 대상이 아니다.

`tasty-hooks` 는 leaf 크레이트라 레지스트리를 볼 수 없어 `(surface, event)` 매칭만 하고 바인딩을 돌려준다. 실제 실행(레지스트리 조회 + `source` 게이트 + `ShellCommand`→셸 / `IpcSequence`→IPC 순차 실행)은 본체 `hook_handler::trigger::execute_binding` 이 담당한다. `IpcSequence` 실행에는 IPC injector 가 필요하다(없으면 건너뛰고 warn).

### Global hook (조건)

surface 무관 — `condition` 으로 트리거:

- `interval:SECS` — 매 N초 반복
- `once:SECS` — N초 후 1회 실행 후 자동 삭제
- `file:/path` — 파일 수정 감지 시

## 인터페이스

- **사용자/AI Agent/CLI**:
  - `hook.set`/`hook.list`/`hook.unset` — `tasty set hook --event bell --command "..." [--once]` 또는 핸들러 참조 `tasty set hook --event bell --handler <id>` (`--command`/`--handler` 택1)
  - `global_hook.set`/`list`/`unset` — `tasty set global-hook --condition interval:60 --command "..." [--label ...]`
  - 표 → [reference/api](../../reference/api.md#기타-호스트)

## 관련

- **트리거 출처 대칭**: 훅(내부 이벤트)은 웹훅([webhook](../webhook/index.md), 외부 HTTP 트리거)과 대칭인 trigger 출처다. 두 출처가 공유하도록 [공유 훅 핸들러 레지스트리(ADR-0047)](../../adr/0047-shared-hook-handler-registry-source-gate.md)가 신설됐다 — `source: hook|webhook|any` 게이트로 셸 action 은 `hook` 출처 전용이다. 이 훅(내부 이벤트)은 위 셸 명령 인라인(`SurfaceHook.command`) 모델로 동작하며 그 레지스트리를 소비하지 않는다(현재 소비처는 웹훅 등록 경로).
- [agent-collaboration](../agent-collaboration/index.md) · [notifications](../notifications/index.md) · [claude plugin](../../plugins/claude/index.md)(Claude hook 발화)
