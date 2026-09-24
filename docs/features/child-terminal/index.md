# child-terminal (호스트 내재화 자식 터미널 관리 · `tasty terminal`)

- **Status**: Implemented
- **주체**: AI Agent
- **ADR**: [ADR-0021](../../adr/0021-occupancy-and-attach-admission.md) (soft 점유 소비자)
- **코드**: `src/core/child_terminal.rs` (registry) · `src/adapters/ipc/handler/terminal.rs` (IPC) · `crates/tasty-cli/src/commands/terminal.rs` (CLI)
- **화면**: 없음 — headless 전용 (자식은 일반 terminal surface 로 렌더; soft 점유 테두리는 [surface-highlight](../surface-highlight/index.md)/점유 계약)

## 목적

에이전트가 자식 터미널을 만들고 메시지를 보내고 종료할 수 있도록 호스트가 공통 기능을
제공한다. 자식 목록과 점유 관리는 호스트가 맡고, Codex·Claude 명령 구성은 각 플러그인이 맡는다.

## 내부 동작 (headless-valid)

### 자식 목록과 상태 기록

`ChildTerminalRegistry`는 부모별 자식 목록, 자식의 부모, 부모별 다음 index, 자식의
idle/needs_input 상태를 보관한다. 마지막 상태 보고 시각 `last_state_report_at`도 저장해
훅이 얼마나 오래 오지 않았는지 판단한다. 등록할 때 초기화하고 `terminal.set_state`마다
갱신하며, 재시작 후에도 해석하도록 Unix epoch 밀리초를 쓴다.

목록은 `~/.tasty/child-terminals.json`에 영속한다. 등록·제거 시 즉시 저장하며, 이전 파일에
보고 시각이 없으면 `serde(default)`로 빈 맵을 사용한다. 이 목록은 session token 추적
(`session.rs`)이나 셸 서브프로세스 추적(`runner_host`의 `shell_children`)과 별개다.
호스트는 접근할 때마다 실제 surface 목록과 대조해 사라진 자식을 제거한다. 재시작 후 첫
접근에서도 이전 세션의 잔재를 정리한다.

### 상태 보고와 화면 알림

`terminal.set_state --state needs_input`는 자식 상태만 갱신한다. 화면 표시를 담당하는
[AttentionStore](../surface-highlight/index.md)의 `AttentionKind::NeedsInput`와는 별개다.
Claude 플러그인은 같은 훅에서 `terminal.set_state`와
`surface.completion { kind: "needs_input" }`를 함께 호출한다
(`crates/tasty-plugin-claude/src/hook.rs::apply_hook`). 사용자가 탭을 보면 해제되는 것은
화면 알림이며, `tasty terminal state`의 상태 보고는 바뀌지 않는다.

### spawn과 점유

spawn은 지정한 workspace의 pane에 `terminal` 탭을 만들고 registry에 등록한다.
새 자식에는 부모 surface를 소유자로 하는 soft 점유를 설정한다. soft 점유는 표시만 하고
입력을 막지 않는다(`attached=false`). 부모가 사라지면 포커스 시점의 지연 정리로 해제한다.

`command`는 선택 사항이다. 생략하면 탭 생성·등록·soft 점유·`child_surface_id` 반환까지만
진행한다. Codex·Claude 플러그인은 이 ID를 받은 뒤 session token과 surface ID를 포함한
명령을 구성해 `surface.send`로 보낸다. 호스트는 지정된 명령 문자열을 그대로 전송한다.

점유 처리는 내부 함수 `occupy_soft(child, parent)`, `release_occupancy(child)`,
`release_soft_occupancy(child, parent)`를 사용한다. 외부 `occupancy.*` IPC는 없다
([ADR-0021](../../adr/0021-occupancy-and-attach-admission.md)).

### adopt와 release

adopt는 새 PTY를 만들지 않고 기존 surface를 자식으로 등록한다. 대상 존재, 자기 자신인지,
이미 등록됐는지, hard 점유 중인지를 순서대로 검사하고 soft 점유를 시도한다. 다른 부모의
soft 점유 때문에 실패하면 registry는 변경하지 않는다. spawn과 달리 점유를 먼저 확보해
자식 목록과 점유 목록이 어긋나지 않게 한다.

release는 자식 관계와 soft 점유만 제거하고 탭은 남긴다. registry에서 제거한 뒤 저장하며,
점유는 부모까지 확인하는 `release_soft_occupancy(child, parent)`로 해제한다. 따라서 hard
점유를 해제하지 않는다. 점유가 이미 풀려 있어 해제에 실패하더라도 경고만 기록하고
관계 제거는 성공한다. kill은 관계·점유를 제거한 뒤 surface도 닫는다.

Claude의 PTY 오류 스캐너는 `terminal.parent`로 관계가 남아 있는지 확인한다. release 후에도
surface는 살아 있으므로 `surface.locate`로 대신 판단하면 감시를 끝낼 수 없다.
release는 해당 자식의 `claude-error` 알림도 중단한다
([Claude 플러그인](../../plugins/claude/index.md)의 PTY 오류 스캔 범위 참고).

<a id="상태-판정-hook--관측-융합--adr-0072"></a>

## 훅과 출력 관측을 함께 사용하는 상태 판정

설계 근거는 [ADR-0041](../../adr/0041-agent-state-and-completion.md)이다.

`terminal.children`과 `terminal.state`는 훅으로 보고받은 상태와 호스트 관측을 함께 사용한다.
두 경로는 `CoreState::child_liveness{,_with_live}`를 공유한다.
registry의 `active`는 idle이나 입력 대기 보고가 없다는 뜻이므로 실제 실행의 증거가 부족할 수 있다.
훅이 유실됐거나 프로그램이 멈춘 경우를 보완하기 위해 PTY·전경 프로세스·출력 시각을 확인한다.
마지막 훅 보고 시각은 등록 시 초기화하고 각 보고 때 갱신하며, 재시작 후에도 해석할 수 있도록
Unix epoch 밀리초로 저장한다. 관측 조합은 registry 밖의 순수 함수에서 판단한다.

### 판정 우선순위

위에서 아래로 먼저 맞는 규칙이 이긴다.

| # | 조건 | `state` | `confidence` | `evidence` |
|---|---|---|---|---|
| 1 | surface 가 라이브 트리에 없음 | `exited` | `confirmed` | `surface_gone` |
| 2 | hook 이 `needs_input` 보고 | `needs_input` | `reported` | `hook_needs_input` |
| 3 | hook 이 `idle` 보고 | `idle` | `reported` | `hook_idle` |
| 4 | PTY busy | `active` | `confirmed` | `pty_busy` |
| 5 | PTY 미기동(deferred) | `active` | `unobserved` | `pty_not_started` |
| 6 | 전경 프로그램이 셸로 복귀 | `stale` | `confirmed` | `foreground_is_shell` |
| 7 | 무출력 경과시간 관측 불가(mirror 등) | `active` | `unobserved` | `observation_unavailable` |
| 8 | 무출력 < 임계값 | `active` | `heuristic` | `recent_output` |
| 9 | 무출력 ≥ 임계값 && hook 침묵 < 임계값 | `active` | `heuristic` | `recent_hook_report` |
| 10 | 무출력 ≥ 임계값 && hook 침묵 ≥ 임계값 | `stale` | `heuristic` | `output_and_hook_silent` |

- **2·3 이 관측보다 위**인 것은 의도다 — 명시적으로 받은 보고를 무출력 추정으로
  덮어쓰지 않기 때문이다.
- **5 가 6~10 보다 위**인 것도 의도다 — deferred terminal 은 출력을 낸 적이
  없어, 게이트하지 않으면 spawn 직후 전부 `stale` 로 오판정된다.
- 임계값: 무출력 `CHILD_OUTPUT_SILENCE` = 120s, hook 침묵 `CHILD_HOOK_SILENCE` = 300s.
  `BUSY_OUTPUT_WINDOW`(2s)를 그대로 쓸 수 없다 — 그 창은 "지금 화면이 갱신되는 중인가"
  용도라 사람이 프롬프트를 읽는 몇 초만으로도 넘어간다.
- hook 침묵 기준점이 없으면(이 기능 도입 전에 영속된 항목) 침묵으로 간주한다 —
  무출력 시간도 이미 임계값을 넘겼고 최근 훅 보고도 확인할 수 없기 때문이다.

### 미등록 surface

`terminal.state` 는 registry 에 없는 surface 도 조회를 거부하지 않는다(`state_of` 의
미등록 fallback 계약 유지). 다만 응답은 registry 원값이 아니라 파생 판정이므로,
**PTY 가 떠 있고 셸 프롬프트에 머무는** 임의의 live surface 는 `active` 가 아니라
`stale`(`foreground_is_shell`)로 나온다 — "이 surface 에서 도는 프로그램이 없다" 는
관측 사실 그대로다. PTY 미기동(deferred) surface 는 게이트에 걸려 `active`
(`pty_not_started`)로 남는다.

### `stale` 의 의미와 한계

`stale` 은 **`exited` 가 아니다.** surface 는 살아 있고, "이 surface 에서 에이전트
프로세스가 돌고 있지 않거나, 돌고 있다는 증거가 없다" 는 뜻이다. `terminal.adopt` 로
들어온 자식은 애초에 에이전트가 아닌 일반 셸일 수 있으므로 종료로 단정해선 안 된다.

무출력 기반 정지 판정은 **원리적으로 휴리스틱**이다 — SIGSTOP 으로 멈춘 프로세스,
긴 추론 중인 에이전트, 출력이 없는 긴 명령은 관측상 구별되지 않는다. surface 부재와 전경 셸 복귀는 각각 확정 관측이며, 훅 보고·PTY busy·관측 불가도
위 표처럼 별도 confidence를 갖는다. 특히 추정에 의한 `stale`만으로 작업 성공이나
재시작 가능 여부를 결정하지 않는다.

### 출력 전용

`stale`/`exited` 는 호스트가 관측으로만 만들어내는 값이다. `terminal.set_state` 는
여전히 `idle`/`needs_input`/`active` 세 값만 받는다 — hook 이 파생 상태를 registry 에
밀어넣을 수 있으면 호스트 관측과 훅 보고를 구분할 수 없게 된다.

### 조회만이 소비처가 아니다 — push 축

Claude 플러그인의 `error_scan`은 출력 정지가 일정 시간 이어지면 `terminal.state`를 조회한다.
상태가 `active` 또는 `stale`이면 `claude-error-stalled` → `notify-error` 경로로 부모의
[완료 알림 로그](../../dev-guide/external-interaction.md#child-완료-알림--completion-log)에 남긴다.
이 이벤트 이름은 기존 구독을 유지하기 위한 것이며 오류가 검출되지 않은 정지도 포함한다.
알림 문구는 오류 뒤 정지와 오류 없이 정지한 경우를 구분한다.

오류 뒤 정지는 `STALL_QUIET`, 오류 없는 정지는 더 긴 `CHILD_OUTPUT_SILENCE`에 맞춰 확인한다.
정지 시간·중복 여부·쿨다운을 먼저 확인하므로 매 tick마다 상태 조회를 보내지는 않는다.
한 정지 구간에서는 한 번만 알리고 출력이 다시 시작되면 재알림을 허용한다.
나중에 실제 `needs_input` 훅이 와도 그 알림은 별도로 전달한다.

추정 `stale`도 알림 대상이다. 긴 추론과 실제 정지는 구별되지 않을 수 있으므로 부모가
상태를 확인해야 하며 이 알림만으로 작업을 재시작하지 않는다. `stale` 자체도 훅 유실을
증명하지 않는다. 재사용 후보를 세는 `spawn_census`는 확정 `stale`만 포함한다.
이 감시는 `terminal.set_state`를 호출하지 않고 관측 결과를 읽기만 한다.
Codex에는 이 출력 스캐너가 없으므로 같은 감시가 있다고 설명하지 않는다.

### 능동 프로빙 배제

대상 surface 에 입력을 주입해 반응을 보는 능동 프로빙은 사용자 입력 재현이라 release
금지 대상이고([`docs/identity.md`](../../identity.md) 원칙 1), 자식 에이전트의 프롬프트
상태도 오염시킨다. 판정은 **수동 관측만** 쓴다.

### 비용

추가 프로세스 스냅샷은 없다. 전경 프로그램 이름은 1Hz 일괄 폴링이 이미 채우는
`foreground_names` 캐시에서만 읽는다 — 자식마다 `Terminal::foreground_process_info()`
를 개별 호출하면 O(surfaces × processes) 를 되살리는 회귀다
(`src/core/state/busy.rs` 폴링 주석).

## 인터페이스

- **AI Agent (IPC/CLI)** — 모든 대상은 ID 로 직접 지정(포커스 독립, 원칙 3):
  - `tasty terminal spawn --workspace <ws> --command "<cmd>" [--surface <parent>] [--pane] [--cwd] [--role] [--nickname]` ↔ `terminal.spawn`
  - `tasty terminal tell "<text>" [--surface]` ↔ `terminal.tell`
  - `tasty terminal children [--surface]` ↔ `terminal.children`
  - `tasty terminal parent --surface <child>` ↔ `terminal.parent`
  - `tasty terminal state --surface <child>` ↔ `terminal.state` — 자식 단건 상태(`idle`/`needs_input`/`active`/`stale`/`exited`) 조회. `terminal.children` 의 항목별 `state` 와 **같은 판정 헬퍼**(`CoreState::child_liveness`)를 쓰므로 목록과 단건의 답이 갈리지 않는다. 이미 registry 에서 정리된(reconcile 로 사라진) surface 도 라이브 트리와 직접 대조해 `"exited"` 로 판별한다 — `ChildTerminalRegistry::state_of` 자체의 미등록 surface `"active"` fallback 계약은 그대로 둔 채, 상위 판정 계층이 그 위에서 죽은 surface 를 걸러낸다
  - `tasty terminal kill [--surface] --child <n>` ↔ `terminal.kill`
  - `tasty terminal respawn [--surface] --child <n> [--cwd] [--command] [--role] [--nickname]` ↔ `terminal.respawn`
  - `tasty terminal broadcast "<text>" [--surface] [--role]` ↔ `terminal.broadcast`
  - `tasty terminal set-state --surface <child> --state <idle|needs_input|active>` ↔ `terminal.set_state` (에이전트 hook 진입점). **파생 상태(`stale`/`exited`)는 입력으로 받지 않는다** — 출력 전용이다(아래 "상태 판정")
  - `tasty terminal adopt --target <surface> [--surface <parent>] [--cwd] [--role] [--nickname]` ↔ `terminal.adopt` — 새 탭을 만들지 않고, 이미 존재하는 임의의 surface 를 지금 시점에 명시적으로 child 로 등록(soft 점유)한다
  - `tasty terminal release [--surface <parent>] --child <n>` ↔ `terminal.release` — child 관계와 soft 점유만 해제한다. surface(탭) 자체는 닫지 않는다(`terminal.kill`과 달리)

### 판정 응답 필드

`terminal.children` 의 각 항목과 `terminal.state` 단건 응답은 판정에 사용한 세 필드를 모두
포함한다. 두 경로가 같은 직렬화 지점(`liveness_fields`,
`src/adapters/ipc/handler/terminal.rs`)을 거치므로 키 집합과 값이 구조적으로 일치한다.

| 필드 | 값 |
|---|---|
| `state` | `exited` \| `needs_input` \| `idle` \| `active` \| `stale` |
| `evidence` | `surface_gone` \| `hook_needs_input` \| `hook_idle` \| `pty_busy` \| `pty_not_started` \| `foreground_is_shell` \| `observation_unavailable` \| `recent_output` \| `recent_hook_report` \| `output_and_hook_silent` |
| `confidence` | `confirmed` \| `reported` \| `heuristic` \| `unobserved` |

나오는 조합은 위 "판정 우선순위" 표의 행 그대로다 — 임의 조합은 생기지 않는다.

`state` 하나만 보면 **같은 값의 근거가 갈리는 것을 구분할 수 없다.** 예를 들어
`active` 는 "PTY 가 지금 출력 중"(`pty_busy`/`confirmed`)일 수도 있고 "판정할 관측
정보가 없어서 그대로 둔 것"(`observation_unavailable`/`unobserved`)일 수도 있다.
소비자는 `confidence` 를 보고 **확정 판정만 종결로 다룰 수 있다** — `heuristic` 인
`stale` 은 SIGSTOP·긴 추론·무출력 명령과 구별되지 않으므로(위 "`stale` 의 의미와
한계") 그것만으로 종료 처리하면 실행 중인 자식을 잘못 종료할 수 있다.

원시 관측값(`busy`, 무출력 경과시간 등)은 싣지 않는다 — `evidence` 가 "어떤 관측으로
판정했는가" 를 이미 알려주므로 목적이 달성되고, 원시값을 계약으로 굳히면
임계값 조정이 소비자 계약 변경이 된다.

**소비자 정합**: claude plugin 의 `claude.children` remap 은 화이트리스트라 세 필드를
명시적으로 옮긴다(`crates/tasty-plugin-claude/src/handlers.rs`). codex plugin 의
`codex.children` 은 passthrough 라 자동 반영된다.

### `--child <n>` 은 index 지 surface_id 가 아니다

`--child` 는 부모별로 0 부터 발급되는 **child index**(`ChildTerminalRegistry::next_index_for`)를
받는다. `terminal.children` 이 반환하는 `surface_id` 와는 다른 번호 공간이며, 둘 다 정수라
혼동하기 쉽다. 두 공간은 **실제로 겹칠 수 있다** — 새 인스턴스는 surface id 도 1, 2, 3… 이라
`--child 2` 가 index 2 인지 surface 2 인지 구조적으로 구분되지 않는다. 그래서 넘어온 값을
surface_id 로 자동 해석하지 않고, **인자 의미는 index 로 고정한 채 에러 메시지가 안내**한다:

| 넘긴 값 | 응답 |
|---|---|
| 같은 부모의 `child_surface_id` | `… 4 is a child_surface_id, not a child index — use \`--child 2\`` |
| 다른 부모의 `child_surface_id` | `… under a different parent — use \`--surface 9000 --child 4\`` |
| 그 외(오타·범위 밖·이미 정리됨) | `… (valid child indices: 0, 2; 2 children)` |
| 그 외 + 등록된 child 가 0 | `… (no children registered under surface 9000)` |

kill/release/respawn 세 경로가 같은 메시지를 쓴다. 실패는 `exit=1` + stderr 이므로, 일괄
처리 스크립트는 **호출당 종료코드를 확인해야 한다** — 버리면 전건 실패를 성공으로 오인한다.

### `--surface` 생략과 다중 윈도우

`kill`/`release`/`respawn`/`broadcast` 는 `--surface`(parent) 를 생략할 수 있다 — host 가
현재 engine 의 `child_terminals.single_parent()` 로 폴백한다(parent 가 정확히 1개일 때만
성공, 0 개·2 개 이상이면 에러). 이 폴백은 **그 engine(= 하나의 main window) 안에서만**
유일성을 본다 — main window 가 2 개 이상 열린 세션에서는 애초에 어느 window 를 봐야
하는지가 정해지지 않는다. 그래서 이 4 개 메서드가 `--surface` 없이(그리고 라우팅 가능한
다른 리소스 id 도 없이) 호출됐는데 main window 가 2 개 이상이면, focused window 로 조용히
새지 않고 라우팅 단계(`App::find_request_owner`)에서 명시적 에러로 거부한다(단일 윈도우
세션은 기존처럼 생략 가능 — 하위 호환). 구현: `src/app/request_owner.rs`
`ambiguous_parent_fallback_requires_surface`.

## 비-목표 (Out of scope)

- **에이전트 특화 로직** — codex/claude 바이너리 command 빌더, hook/trust, telemetry 는 플러그인에 잔류한다. 호스트는 임의 command 만 붙인다.
- **`terminal launch`(새 workspace 생성)** — 에이전트 편의 명령이라 범위 밖.
- **soft 입력 독립성(잔여 입력 비오염)** — 현재는 단순 `surface.send` 로 붙인다(ADR 방향성만).

## Acceptance Criteria

- Given workspace `<ws>` When `terminal.spawn{parent=P, command}` Then 자식 터미널이 생성·registry 등록되고 `occupancy_of(child)==Soft`·`holder.parent==P`·`attached=false`.
- Given 점유된 child C When `terminal.kill` Then `occupancy_of(C)==None` + surface 닫힘.
- Given 죽은 자식이 남은 registry When `terminal.children` Then reconcile 로 목록에서 제거.
- Given 이미 존재하는 임의의 surface(spawn 으로 만들지 않은 일반 터미널 탭 포함) When `terminal.adopt{surface=P, target}` Then `occupancy_of(target)==Soft`·`holder.parent==P`·`terminal.children` 목록에 나타남.
- Given 이미 등록된 child 또는 hard 점유 중인 대상 When `terminal.adopt` Then 에러 반환 + registry 불변.
- Given 점유된 child C When `terminal.release` Then `occupancy_of(C)==None` + `terminal.children` 목록에서 사라짐 + surface(탭)는 여전히 열려있음(닫히지 않음).
- Given 등록되지 않은 child index When `terminal.release` Then 에러 반환.
- Given C 와 무관한 다른 surface 가 hard 점유 중 When `terminal.release{child=C}` Then 그 hard 점유는 영향받지 않음.
- Given 실행 중인 child C When `terminal.state{surface=C}` Then 응답이 `state`·`evidence`·`confidence`·`surface_id:C` 를 싣고 `state` 는 `exited` 가 아니다(값은 위 판정 우선순위 표).
- Given `terminal.kill`로 종료된 child C When `terminal.state{surface=C}` Then `state` 가 `"exited"` 다 (`"active"`가 아님).

## spawn 준비와 실제 PTY 종료

spawn은 registry 등록·soft 점유 준비 후 command를 전송한다. 준비나 동기 전송 단계에서 오류가 나면 이번 호출이 만든 surface만 표준 agent close 경로로 정리하고 자신의 registry/점유도 회수한다. adopt의 실패는 기존 surface나 같은 부모가 이미 갖고 있던 soft 점유를 변경하지 않는다.

headless의 실제 PTY 종료도 GUI와 같은 host process-exit 경로를 사용한다. SessionEnd 없이 종료해도 process-exit 훅을 발생시키고 soft 점유를 정리한다. headless도 HookFired의 task waiter를 처리하며 view 전용 ProcessExited broadcast는 GUI에 남는다. 종료 원인만으로 작업 성공을 추론하지 않는다.
