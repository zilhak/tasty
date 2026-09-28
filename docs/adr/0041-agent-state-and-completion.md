# ADR-0041: 에이전트 상태 보고와 완료 전달을 분리한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: agents, hooks, completion, observation
- **Group**: agents

## Context

에이전트가 보고한 마지막 상태만 저장하면 훅이 유실됐을 때 부모가 계속 기다릴 수 있다.
반대로 출력이 없다는 이유만으로 작업이 끝났다고 판단하면 오래 실행되는 정상 작업을 중단시킨다.
상태를 판단하는 근거와 부모에게 결과를 전달하는 방법을 구분해야 한다.

## Decision

자식의 상태는 훅 보고와 호스트의 관측을 함께 사용한다. 목록과 단건 조회는 같은 판단 함수를
쓰고 `state`, `evidence`, `confidence`를 따로 반환한다. 관측으로 만든 `stale`과 `exited`는
호스트의 관측 결과이며 훅 입력으로 저장하지 않는다. 대상 터미널에 입력을 보내 반응을 시험하지 않는다.

Claude 플러그인은 출력 정지를 감시하고 `active` 또는 `stale`인 자식을 부모에게 알린다.
추정에 의한 `stale`도 알리지만 작업 완료나 재시작 허가로 해석하지 않는다.
재사용 후보를 세는 기능은 확정된 `stale`만 사용한다.

Codex의 도구 실행 승인 대기는 `PermissionRequest` 훅으로 관측한다.
`PostToolUse`는 active로, `Interrupt`는 idle로 돌린다. 입력 대기는 작업 성공이 아니므로
Codex 작업의 성공 종료 상태에 넣지 않는다. idle 전이는 `terminal.state`의 `needs_input` 상태를 해제한다.
화면의 attention 알림은 별도 상태이며 [사용자 확인·clear 규칙](0024-attention-ownership-and-clear.md)을 따른다.

Claude의 승인 모드는 호출 인자, 플러그인 기본 설정, 사용자 Claude 설정 순으로 정한다.
호출과 기본 설정에 값이 없으면 플래그를 붙이지 않는다. 프로필이 같은 모드를 정하면서
별도 모드도 주어지면 조용히 하나를 선택하지 않고 거부한다. 호출별 값은 복원 명령에 저장하지 않는다.
Codex의 승인·샌드박스 옵션과 기본값은 그 플러그인의 실제 계약대로 유지한다.

API 오류 뒤 Claude 자동 재개는 사용자가 켜야 동작한다. 지정한 일시적 오류만 대상으로 하며
보내기 직전에 설정, 상태, 프로세스, 턴, 사용자 입력, 연속 시도 수를 다시 확인한다.
서브에이전트 실패를 메인 턴 종료로 취급하지 않는다.

Claude의 메인 턴이 이어지는 동안에는 idle을 보고하지 않는다. `SubagentStop`은 서브에이전트의
종료일 뿐이므로 상태·완료 알림·화면 알림·경과 시간 기록을 만들지 않는다. `Stop` payload가
끝나지 않은 백그라운드 작업을 보고하면 그 Stop은 대기이며 턴 종료가 아니다. 판정에는
`waiting_on_background_work`가 있으면 그 값을, 없으면 `background_tasks`에 끝나지 않은 항목이
있는지를 쓰고, 항목 종류는 가리지 않는다. 끝난 항목은 `status`가 `completed`·`failed`·`cancelled`·`canceled`·`killed`·`stopped`·`error`·`done`
중 하나(대소문자 무관)인 항목이다. 공식 hooks 문서(2026-09-28 확인)는 항목을 진행 중인 작업이라
하고 `status` 값 목록을 주지 않으므로, 목록에 없는 값과 `status`가 없는 항목은 대기로 센다.
`waiting_on_background_work` 필드와 `pending` 같은 `status` 값은 공식 문서와 Claude Code 2.1.283
실측에 없지만, 오면 방어적으로 읽는다. 대기 Stop은 `active`만 보고하며 완료 알림·경과 시간·
자동 재개 성공 처리를 하지 않는다. 두 필드가 없으면 이전처럼 Stop을 턴 종료로 본다.

부모에게 전달하는 완료·입력 대기·정지 알림은 부모 종류와 무관하게 완료 로그에 기록한다.
부모 PTY에 사용자 메시지처럼 넣지 않으며 Codex App Server의 별도 전달 경로도 두지 않는다.
부모가 그 로그를 읽을 수단은 따로 준비해야 한다. 로그 기록이 부모의 읽기나 다음 턴 시작을
보장하지는 않는다. 상태 알림을 작업 성공 판정과도 구분한다.

완료 로그는 기존 경로와 한 줄 메시지 형식을 유지한다. append할 때 메시지와 개행을 먼저
한 버퍼로 합친다. 크기에 따른 전량 비우기와 쓰기는 분리하고, 버린 바이트 누계는 옆 메타
파일로 알린다. 누계를 확정할 수 없는 경우에는 정확한 손실량을 만들어내지 않는다.
일반 부팅은 포트 파일을 공개하기 전에 완료 로그를 정리한다. 포트 파일의 디렉터리가
데이터 루트와 다르면 청소를 생략하지만, 이것만으로 여러 호스트의 로그가 격리되지는 않는다.

## Consequences

부모는 상태의 근거와 확실성을 보고 다음 행동을 정할 수 있다.
출력 정지 알림은 긴 추론에도 발생할 수 있어 부모의 확인이 필요하다.
아무 알림 없이 기다리는 위험을 줄이는 대신, 일부 알림의 오탐을 감수한다.
`terminal.state`의 `needs_input` 해제는 도구 종료까지 늦어질 수 있고, 자동 재개는 사용자 입력 흔적만 있어도
보수적으로 취소한다. 끝나지 않는 백그라운드 명령을 남긴 채 턴을 끝낸 Claude 자식은 idle이
되지 않아 spawn·tell 대기 노드와 완료 알림이 오지 않는다. 대기 노드에는 제한 시간을 두어야 한다.
Claude Code가 끝난 항목에 위 목록에 없는 `status`를 붙여 남기면 그 자식도 idle이 되지 않는다.
Stop 게이트가 턴을 이어 가게 한 Stop은 여전히 idle로 기록된다. 완료 로그는 제한된 기록이며 재시작·비우기·실패로 미독 내용이 사라질 수 있다.

## Alternatives Considered

- 훅만 믿으면 유실된 상태를 복구할 수 없고, 관측만 믿으면 명시적인 완료 보고를 버리게 된다.
- 전경이 셸이라는 사실만으로 서피스가 종료됐다고 표시하면 일반 셸을 등록한 자식도 잘못 처리한다.
- 호스트에 상태 전이 구독을 새로 만들기보다 현재 Claude 플러그인의 감시 루프를 사용한다.
- 정지 이벤트를 개명하면 기존 구독이 끊어지므로 이름은 유지하고 알림 내용에서 원인을 구분한다.

- PTY가 출력 중이면 훅의 idle 보고보다 관측을 우선하는 방법은 Claude 화면 재그리기와 잠금 경합으로도
  busy가 되어 완료가 늦어지거나 오지 않을 수 있다. 러너가 idle을 여러 번 연속 확인한 뒤 끝내는 방법은
  백그라운드 대기처럼 긴 구간을 막지 못한다. 두 방법 대신 훅 보고 자체를 바로잡는다.
- 백그라운드 서브에이전트만 대기로 보고 셸은 제외하면 `run_in_background` 셸을 기다리는 턴이 일찍
  끝난다. 끝나지 않는 셸 때문에 idle이 오지 않는 위험을 감수하고 종류를 가리지 않는다.
- `running`·`pending`처럼 진행을 뜻하는 값만 대기로 세면 문서에 없는 진행 값이 오는 순간 턴 도중에
  idle이 된다. 조기 종결은 후속 작업을 미완성 산출물로 실행하게 하므로, 끝을 뜻하는 값만 목록으로 두고
  모르는 값 때문에 idle이 늦어지거나 오지 않는 위험을 택한다.

- 승인 화면 문구나 무출력으로 승인 대기를 추측하면 버전 변화와 장기 실행을 오판할 수 있다.
  구조화된 훅이 있는 경우 이를 사용한다.
- 권한 우회를 기본값으로 정하거나 복원 명령에 남기면 사용자의 설정과 호출 범위를 넘을 수 있다.
  모드를 고를 인터페이스는 제공하되 사용자의 선택을 보존한다.
- 자동 재개를 기본으로 켜거나 예약할 때만 검사하면 뒤늦은 사용자 입력과 설정 변경을 놓친다.
  rate limit은 해제 시각을 모르는 짧은 재시도로 해결되지 않아 자동 재개에서 제외한다.

- Codex App Server에 별도 outbox와 sender를 두면 외부 버전·세션 바인딩·수락 여부·재송신
  상태를 계속 관리해야 한다. 그 비용을 줄이기 위해 모든 부모에 로그를 사용한다.
- 로그 줄이나 경로에 세대·순번을 추가하면 기존 reader가 바뀐다. 현재는 별도 메타 파일을 쓴다.
- 로그 파일 자체 잠금은 Windows append를 막을 수 있어 메타 파일을 잠근다.
  메타를 rename하면 잠금 대상이 나뉘므로 고정 폭으로 제자리 갱신한다.
- 비우기 잠금을 무한 대기하면 느린 reader가 완료 전달을 막는다. 대기 상한 뒤에는
  손실량의 정확성을 포기하고 기록을 진행한다.

## Reconsideration Triggers

- 정상 작업을 정지로 보고하는 사례가 반복되거나 출력 패턴이 달라질 때.
- PID 기반 확인이나 재전송 가능한 훅 전달로 더 정확한 상태를 알 수 있을 때.
- 호스트가 상태 전이를 저장하거나 Codex에도 같은 출력 감시 기능이 생길 때.

- 승인 완료 이벤트나 도구별 승인 식별자가 추가돼 현재 해제 지연을 줄일 수 있을 때.
- 외부 CLI가 허용하는 승인 모드나 훅 payload를 바꿀 때.
- Claude Code가 `background_tasks`의 모양이나 의미를 바꾸거나 `waiting_on_background_work`를 새로 보낼 때.
  이 필드를 검사하는 자동 검사는 없다. 확인하려면 `claude.spawn`의 `profile_file`로 Stop 훅
  stdin을 파일에 덧붙이는 추적 훅을 붙인 자식에게 백그라운드 서브에이전트나 `run_in_background`
  셸을 쓰게 하고, 기록된 Stop payload의 필드 이름과 항목의 `status` 값을 대조한다.
  같은 턴에서 플러그인 로그(`tasty plugin logs com.tasty.claude`)에
  `claude hook stop s<N>: waiting on background work (<판정 근거>) — main turn continues, stays active`
  줄이 없거나(괄호 값은 `waiting_on_background_work`가 true면 `waiting_on_background_work`,
  `background_tasks`로 판정했으면 `N background task(s): <type 목록>`이다) `claude hook: background_tasks is not JSON`·
  `claude hook: background_tasks is not an array`·`claude hook: unreadable waiting_on_background_work` 경고가 나오면 판정 입력이 바뀐 것이다.
- 끝나지 않는 백그라운드 명령 때문에 idle이 오지 않는 사례가 보고될 때.
  자동으로 감지하지 않는다. 사례를 확인하려면 해당 자식의 플러그인 로그에서 마지막
  `waiting on background work` 줄 뒤에 idle을 만드는 Stop이 없는지 보고, `tasty claude children`이
  그 자식을 `idle`·`needs_input`으로 바꾸지 않고 `active` 또는 `stale`로 보고하는지(`evidence`를
  함께 본다. 출력·훅이 오래 조용하면 `output_and_hook_silent`, 전경이 셸이면 `foreground_is_shell`의
  `stale`이 된다), 대기 노드가 `tasty agent task-list`에서 `running`에 머무는지를 함께 본다. 자식 화면에 실행 중인 백그라운드 명령이 남아 있는지도 확인한다.
- 끝난 백그라운드 항목이 끝 값 목록에 없는 `status`로 `background_tasks`에 남는 사례가 보일 때.
  자동 검사는 없다. 위 추적 훅의 Stop payload에서 항목의 `status`를 보고, 그 작업이 끝났는데도
  플러그인 로그에 `waiting on background work` 줄이 이어지는지 확인한다.
- Stop 게이트의 차단 결과를 상태 보고와 연결할 수단이 생길 때.
- 초안의 존재나 재시도 가능 시각을 직접 조회할 수 있거나 원치 않는 자동 재개가 보고될 때.

- 완료 로그의 미독 손실·파일 수·부모 자동 수신 요구가 현재 보존 방식으로 감당되지 않을 때.
- 한 데이터 루트를 여러 호스트가 공유하거나 세대를 넘는 reader가 필요할 때.
- 잠금 실패나 상한 초과가 반복돼 손실량을 알 수 없는 경우가 늘어날 때.

## References

- [자식 터미널](../features/child-terminal/index.md)
- [Claude 통합](../plugins/claude/index.md)
- [Codex 통합](../plugins/codex/index.md)
- [외부 상호작용](../dev-guide/external-interaction.md)
