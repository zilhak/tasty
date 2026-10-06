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

이 선택은 2026-09-28에 바뀌었다. 처음(2026-09-28, 대기 Stop 판정 도입)에는 `status`가 `running`·`pending`인
항목만 대기로 셌다. 당시 이유는 2.1.283 실측 값이 `running`이었고, 공식 hooks 문서의 예시가
`status` `pending`과 `waiting_on_background_work`를 쓴다는 조사 기록이었다. 그 기록은 웹 요약에 의존했고,
같은 날 원문을 직접 확인하니 두 값 모두 문서에 없어 틀린 근거로 확인됐다. 원문은 항목을 진행 중인 작업이라
하고 값 목록을 주지 않아, 진행 값만 세면 문서에 없는 진행 값에서 조기 종결된다. 그래서 끝을 뜻하는 값만
목록으로 두는 지금의 판정으로 바꿨다.

Claude `Notification`은 `notification_type`으로 이 세션이 입력을 기다리는지 판단한다. 공식 hooks
문서의 유형 가운데 도구 승인(`permission_prompt`), MCP 입력 폼과 URL 열기(`elicitation_dialog`·
`elicitation_url_dialog`), 사용량 한도가 풀린 뒤의 `Enter` 대기(`quota_auto_resume_stale`),
`agent_needs_input`은 `needs_input`이다. 한도로 멈췄던 작업의 재개(`quota_auto_resume_fired`)는
`active`이고, 나머지 유형은 상태를 바꾸지 않는다. 필드가 없으면 `needs_input`이다. 목록에 없는 값은
상태를 바꾸지 않고 경고만 남긴다. 유형별 표는 [Claude 통합](../plugins/claude/index.md#notification-유형별-상태)에 있다.

Stop 게이트가 `block`으로 턴을 이어 가게 한 Stop은 턴 종료가 아니다. 게이트가 붙은 세션의 Stop은 idle 처리를
보류하고 `active`를 보낸 뒤, 같은 `session_id`·`prompt_id`의 게이트 판정을 플러그인 메모리에서 짝짓는다.
게이트 수는 Stop마다 지금 실행 중인 Claude의 settings(`claude-settings-file` meta)에서 다시 센다. 이 meta는 플러그인이 Claude를 실행하거나 `--resume`으로 복원할 때 기록하고, 기록 뒤 처음 시작한 세션(`claude-settings-session`)의 프로세스가 끝나는 SessionEnd에서 지운다. 이전 세션의 늦은 SessionEnd가 respawn·reboot가 새로 기록한 경로를 지우지 않게 하기 위해서다. 판정이 모두 통과하면 보류한 idle 처리를 실행하고, 하나라도
block이면 `active`를 유지한다. 판정이 5초 안에 다 오지 않으면 온 판정만으로 확정하고 오지 않은 판정은 통과로 본다.
Claude Code의 연속 block 상한(기본 8, `CLAUDE_CODE_STOP_HOOK_BLOCK_CAP`)에 닿은 block은 통과로 본다.
보류 중에 새 턴이 오면 보류한 Stop을 idle로 확정한 뒤 새 턴을 처리한다. 세부는
[Claude 통합](../plugins/claude/index.md#stop-게이트와-idle)에 있다.

백그라운드 작업을 기다리는 Claude 자식의 정지 알림은 일반 정지와 구분한다. 대기 Stop이 대기를 플러그인
메모리와 surface meta `claude-background-wait`에 기록한다. 기다리는 작업의 출력 파일(Claude Code 임시 폴더의
`<session_id>/tasks/<id>.output`, 서브에이전트는 transcript를 가리키는 링크이며 대상이 없는 링크는 대상이 생길 때까지 빼고)을 찾으면 그 크기·수정 시각을 활동으로 보고,
모든 파일이 일반 기준(120초) 동안 그대로면 작업 이름과 조용한 분을 적어 대기 한 번에 한 번 알린다. 파일을 찾지 못하면
warn을 한 번 남기고 누적 화면 출력이 10분 동안 같을 때 대기 한 번에 한 번만 알린다. 이때 문구는 기다리는 작업의 종류와
경과한 분만 적는다. 어느 문구도 멈췄을 가능성이나 유실된 훅을 추정하지 않는다.
대기가 아닌 Stop, 백그라운드 작업이 남지 않은 StopFailure, 새 턴, 세션 종료가 대기 기록을 지운다.

API 오류로 끝난 턴(`StopFailure`)은 payload에 `background_tasks`가 없다. 그래서 플러그인이 메인 턴의 백그라운드
작업을 따로 기록한다. 시작은 `PostToolUse`(Bash·Agent)의 `tool_response`, 끝은 `<task-notification>` prompt의
`<task-id>`와 `Stop`의 `background_tasks`로 갱신한다. 기록이 남은 채 `StopFailure`가 오면 대기 Stop과 같이 `active`로
보고하고 idle·완료 알림·자동 재개를 만들지 않는다. 오류는 `claude-last-stop-failure` meta와 `claude-stop-failure` 이벤트로 남기고,
spawn·tell이 이 이벤트에 등록한 알림 훅이 부모 완료 로그에 "API 오류로 턴이 끝났지만 백그라운드 작업이 남아 계속 대기한다"는 줄을 한 번 쓴다.
완료 줄의 "입력을 기다린다" 오류 힌트는 자식이 `idle`일 때만 붙인다.

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
백그라운드 작업 기록은 메모리에만 있어, 플러그인이 다시 시작된 뒤의 `StopFailure`는 작업이 남아 있어도 idle과 완료 알림을 보낸다. `tasty claude install`을 다시 실행하기 전의 설치본도 시작 훅이 없어 같다. 작업이 끝났다는 `<task-notification>` 턴이 오지 않는 작업이 기록에 남으면 그 뒤의 `StopFailure`는 다음 `Stop`이 기록을 바꿀 때까지 idle이 되지 않는다. 작업이 바쁜 턴 중에 끝나 알림 턴 없이 소비되는 경우가 이에 해당하는지는 미측정이다. 작업이 남은 `StopFailure`의 오류는 부모 완료 로그의 별도 줄 하나로만 전달된다. 작업이 끝나 열린 턴이 오류 meta를 지우므로 그 턴의 완료 줄과 자동 재개에는 오류가 반영되지 않는다. Bash·Agent 도구 호출마다(포그라운드 호출 포함) `PostToolUse` 훅 명령이 실행되고, 훅이 끝날 때까지 Claude Code는 다음 API 요청을 보내지 않는다. 훅 명령은 셸에서 payload의 백그라운드 표지(`"backgroundTaskId":"`, `"isAsync":true`)를 먼저 보고 포그라운드 호출이면 tasty CLI를 실행하지 않는다. 2026-10-06 실측(Claude Code 2.1.291, debug 호스트, loadavg 48~68·20코어, 각 10회)에서 포그라운드 Bash 1회의 도구 종료부터 다음 API 요청까지 중앙값은 훅 없음 58ms, 셸 판정 훅 94ms였다(판정 없이 CLI를 실행하던 명령은 320ms). 백그라운드 호출은 CLI를 실행하므로 `sh -c` 기준 중앙값 208ms(30회)가 더해진다. payload 형식이 바뀌어 표지가 맞지 않으면 백그라운드 작업 기록이 빠지고, 그 작업이 남은 `StopFailure`는 idle이 된다.
게이트가 붙은 세션은 턴이 끝나도 판정이 모일 때까지 idle이 늦어진다. 판정이 5초 안에 오지 않으면 그만큼 늦고, 그 사이
block된 판정이 늦게 오면 턴이 이어지는데도 idle로 기록된다. 늦게 온 판정은 다음 Stop이 오기 전까지만 버리고, 다음 Stop이 오면
앞 Stop의 빈자리를 지운다. 다만 그 사이 다음 Stop의 게이트 판정이 상태 훅보다 먼저 오면 앞 Stop의 늦은 판정으로 보고 버리므로,
그 Stop은 5초 뒤 통과로 확정되어 block이어도 idle로 기록될 수 있다. 시간 초과로 확정한 idle은 그 사이 같은 세션에 새 턴이
시작됐으면 보내지 않는다. Claude Code의 연속 block 상한은 tasty 게이트가 아닌 Stop 훅의 continuation도 세지만 플러그인은 tasty
게이트의 block만 센다. 다른 Stop 훅이 함께 턴을 이어 가게 하면 상한으로 끝난 턴의 idle이 기록되지 않을 수 있다. 사용자가 직접 `--settings`로 게이트를 붙인 Claude는 게이트 수를
알 수 없어 block된 Stop도 idle로 기록된다. SessionEnd 없이 끝난 Claude의 settings meta는 남아, 같은 surface에서 다음에 실행한 Claude의 Stop이 최대 5초 늦게 idle이 될 수 있다. 프로필이 붙은 surface에서 `--settings` 없이 `claude -r`로 다시 연 세션도 같다. 보류 중에 플러그인이 다시 시작되면 그 Stop의 idle은 기록되지 않는다. Claude Code가 새 입력 대기 유형을 추가하면 목록에 넣기 전까지 그 대기를 `needs_input`으로 보고하지 않는다. agent view를 연 Claude 자식은 다른 세션의 입력 대기에도, auto mode의 classifier 요금 안내에도 `needs_input`이 된다. 출력 파일 경로는 공식 문서에 없는 Claude Code 내부 규칙(2.1.291 확인)이라 버전이 바뀌면 찾지 못할 수 있다. 플러그인은 자식의 환경 변수를 읽지 못해 자식에게만 `CLAUDE_CODE_TMPDIR`을 다르게 주면 찾지 못한다. transcript 저장이 꺼진 자식의 서브에이전트는 링크 대상이 없어 화면 기준(10분)으로 돌아간다. 셸 파일과 함께 기다리면 셸 파일만 보므로, 셸 작업이 조용하고 서브에이전트만 일하는 동안에도 120초 알림이 갈 수 있다. 찾지 못한 대기의 자식이 실제로 멈춰도 부모는 10분 뒤에야, 대기 한 번에 한 번만 알림을 받는다. 출력 없이 오래 계산하는 작업(예: 출력 없는 빌드 단계, `sleep`)은 120초 뒤 알림을 받는다. 출력이 계속 늘지만 진행하지 않는 작업은 알리지 않는다. Linux에서만 실측했고 macOS·Windows의 경로와 서브에이전트 링크는 미측정이다. 완료 로그는 제한된 기록이며 재시작·비우기·실패로 미독 내용이 사라질 수 있다.

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

- 목록에 없는 `notification_type`을 `needs_input`으로 보면 새 입력 대기 유형을 놓치지 않지만, 입력 대기가
  아닌 새 유형이 턴 도중에 오면 spawn·tell 대기 노드와 후속 작업이 조기에 진행된다. 입력 대기 유형은
  `*_prompt`·`*_dialog`·`*_needs_input`처럼 이름으로 드러날 가능성이 높고 조기 종결의 피해가 더 커서
  상태를 바꾸지 않는 쪽을 택한다.
- `elicitation_complete`·`elicitation_response`로 `active`를 되돌리면 대기가 풀린 것을 반영할 수 있지만,
  `needs_input`의 원인을 기록하지 않으므로 겹친 승인 대기까지 지운다. 원인별 해제 구조가 없어 상태를 바꾸지 않는다.
- `agent_needs_input`을 무시하면 agent view의 다른 세션 대기나 auto mode의 classifier 요금 안내(약 6초 동안 입력이 없을 때)로
  조기 종결되지 않지만, 이 세션이 teammate 설정 질문을 하는 경우의 대기를 놓친다. payload로 세 경우를 구분할 수 없어
  `needs_input`을 유지한다.
- 게이트 CLI가 판정과 함께 상태를 직접 보고하면 짝짓기가 필요 없지만, 게이트가 여럿이면 한 게이트는 다른 게이트의 판정을
  모른다. 한 게이트의 통과로 idle을 보고하면 다른 게이트의 block을 놓친다.
- 상태 훅이 게이트 판정을 기다리게 하면 순서 문제가 사라지지만, 플러그인은 요청을 한 워커에서 차례로 처리하므로 기다리는
  동안 그 판정 요청도 처리되지 않는다. 보류 표에 넣고 곧바로 반환한다.
- idle을 그대로 보고하고 block이 오면 `active`로 되돌리면 구현이 단순하지만, 그 사이 `claude-idle`·완료 알림·spawn·tell 대기
  노드의 종결이 이미 일어난다. 되돌릴 수 없는 부수 효과라 보류한다.
- 게이트 수를 세지 않고 판정 한 건만 기다리면 게이트가 여럿일 때 첫 통과로 확정해 뒤의 block을 놓친다.
- 백그라운드 대기 중에 정지 알림을 끄면 부모 로그에 오해를 부르는 줄이 생기지 않는다. 하지만 끝나지 않는
  백그라운드 명령을 남긴 자식은 idle이 되지 않으므로, 이 알림이 부모가 받는 유일한 신호다. 그래서 알림을 끄지 않고 기준과 문구를 바꾼다.
- 호스트 상태에 대기 값을 추가하면 부모와 러너가 대기를 직접 조회할 수 있다. 하지만 상태 판정 규칙과 Codex
  플러그인까지 바뀌는 큰 변경이라, 대기 노드의 제한 시간 정책과 함께 다시 검토한다.
- 기존 "looks stuck" 문구를 유지하면 Claude Code가 알려 준 정상 대기를 훅 유실로 설명해 부모가 정상 대기에 개입하게 만든다.
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
- Claude Code가 `StopFailure`에 `background_tasks`를 싣거나, `PostToolUse`의 `backgroundTaskId`·`agentId`·`async_launched`
  또는 `<task-notification>` prompt의 `<task-id>` 형식을 바꿀 때. 자동 검사는 없다. 백그라운드 Bash를 띄운 자식의 플러그인 로그에
  `background task <id> started` 줄이 나오는지, 작업이 끝난 뒤 `background task(s) reported finished` 줄이 나오는지 본다.
- 끝나지 않는 백그라운드 명령 때문에 idle이 오지 않는 사례가 보고될 때.
  자동으로 감지하지 않는다. 사례를 확인하려면 해당 자식의 플러그인 로그에서 마지막
  `waiting on background work` 줄 뒤에 idle을 만드는 Stop이 없는지 보고, `tasty claude children`이
  그 자식을 `idle`·`needs_input`으로 바꾸지 않고 `active` 또는 `stale`로 보고하는지(`evidence`를
  함께 본다. 출력·훅이 오래 조용하면 `output_and_hook_silent`, 전경이 셸이면 `foreground_is_shell`의
  `stale`이 된다), 대기 노드가 `tasty agent task-list`에서 `running`에 머무는지를 함께 본다. 자식 화면에 실행 중인 백그라운드 명령이 남아 있는지도 확인한다.
- 끝난 백그라운드 항목이 끝 값 목록에 없는 `status`로 `background_tasks`에 남는 사례가 보일 때.
  자동 검사는 없다. 위 추적 훅의 Stop payload에서 항목의 `status`를 보고, 그 작업이 끝났는데도
  플러그인 로그에 `waiting on background work` 줄이 이어지는지 확인한다.
- Claude Code가 Stop 훅의 실행 방식(병렬 실행·연속 block 상한·`prompt_id`)을 바꾸거나 block된 Stop을 따로 알리는 이벤트를 제공할 때.
  자동 검사는 없다. 게이트를 붙인 자식의 플러그인 로그에서 `a gate blocked the stop — turn continues` 줄과 그 뒤의 다음 Stop이
  짝을 이루는지, 턴 종료 때 `every gate let the turn end — reporting idle` 줄이 나오는지, `gate decision(s) did not arrive` 경고가
  반복되는지 확인한다.
- Claude Code가 `Notification` 유형을 추가·변경하거나 payload가 `agent_needs_input`의 세 경우를 구분하게 될 때.
  코드 목록과 [Claude 통합](../plugins/claude/index.md#notification-유형별-상태) 표의 일치는 claude plugin
  테스트 `the_docs_notification_table_matches_the_effect_list`가 검사한다. 공식 문서와의 일치는 자동으로
  검사하지 않는다. 공식 hooks 문서의 Notification matcher 값 목록을 표와 대조하고, 플러그인 로그의
  `unknown notification_type` 경고를 확인한다.
- 대기 노드의 제한 시간 정책을 정하거나, 백그라운드 대기 중 알림의 기준(출력 파일 120초, 찾지 못하면 화면 10분)이 너무 늦거나 이르다는 사례가 보고될 때. 플러그인 로그에 `no output file found for background task` 경고가 늘어 Claude Code의 작업 출력 경로가 바뀐 것으로 보일 때.
  자동 검사는 없다. 부모 완료 로그에서 `waiting on background work (…) for <분> min` 줄의 시각을 해당 자식 플러그인
  로그의 `waiting on background work` 줄 시각과 비교한다.
- 초안의 존재나 재시도 가능 시각을 직접 조회할 수 있거나 원치 않는 자동 재개가 보고될 때.

- 완료 로그의 미독 손실·파일 수·부모 자동 수신 요구가 현재 보존 방식으로 감당되지 않을 때.
- 한 데이터 루트를 여러 호스트가 공유하거나 세대를 넘는 reader가 필요할 때.
- 잠금 실패나 상한 초과가 반복돼 손실량을 알 수 없는 경우가 늘어날 때.

## References

- [자식 터미널](../features/child-terminal/index.md)
- [Claude 통합](../plugins/claude/index.md)
- [Codex 통합](../plugins/codex/index.md)
- [외부 상호작용](../dev-guide/external-interaction.md)
