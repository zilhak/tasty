# ADR-0413: gui 에서 IPC wake 는 루프의 나머지에 차례를 넘기고, 예산에서 멈춘 회차는 루프를 다시 깨운다

- **Status**: Accepted
- **Date**: 2026-09-21
- **Tags**: ipc, dispatch, fairness, gui, winit, timers, measurement, adr-0410, adr-0313

## Context

[ADR-0410](0410-a-dispatch-round-also-stops-at-a-time-budget-and-callers-are-served-in-arrival-order.md)
은 회차에 시간 예산(16 ms)을 두면서 "회차가 끝나면 gui 는 그 iteration 의 렌더·타이머로 넘어간다" 를
전제로 삼았다. gui 에서 그 전제는 성립하지 않는다.

gui 에서 회차는 두 자리에서 돈다 — `about_to_wait`(iteration 마다 한 번)와 `IpcReady` 사용자 이벤트
처리기. winit(x11, 0.30)은 한 iteration 안에서 사용자 이벤트를 **큐가 빌 때까지** 꺼내 처리한 뒤에야
`AboutToWait` 을 보낸다(`single_iteration` 의 `while let Ok(event) = self.user_receiver.try_recv()`).
IPC 생산자는 명령마다 wake 를 한 번 부르고, 회차가 답을 주면 호출자는 곧바로 다음 명령과 wake 를
보낸다. 그래서 회차가 도는 동안 wake 가 다시 쌓이고, 지속 부하에서는 그 큐가 비는 순간이 없다 —
타이머(`about_to_wait` 앞머리의 `drain_due`)도 렌더도 영영 차례가 안 온다. 회차의 시간 예산은 한
회차를 자를 뿐, 다음 회차가 곧바로 이어지는 것은 막지 못한다.

실측(2026-09-21, debug gui 빌드 · Xvfb · 격리 홈). 부하는 연결 16 개가 탭 만들기·목록·닫기를 20 초
보내고, 드문 호출자가 250 ms 마다 `system.info` 를 부르고, 전역 훅 `interval:1` 이 초마다 시각을 적는
것이다. 표의 "타이머 최대 간격" 은 부하 구간에서 훅 기록의 이웃 간격 최댓값이다(부하가 없을 때도
2.0 s 다 — 훅은 1 Hz tick 에서 "1 초가 지났는가" 를 보므로 간격이 둘로 번갈아 선다).

| 바이너리 | 타이머 최대 간격 | 부하 명령 수 / 20 s | 드문 호출 p50 |
|---|---|---|---|
| 변경 전 main | 21.3 s · 21.4 s · 21.4 s (세 번) | 4,032 · 4,677 · 4,629 | 154 · 136 · 150 ms |
| 시간 예산만(ADR-0410) | 21.0 s | 4,974 | 132 ms |
| 시간 예산 + 이 결정 | 2.02 s | 3,831 | 155 ms |

- 같은 부하에서 변경 전 바이너리의 렌더 통계는 26.6 초 창에 present 1 회였고, 이 결정을 넣은
  바이너리는 부하 내내 초당 30~38 present 였다.
- 이 결정을 넣고 이벤트 수를 센 계측 빌드에서, 부하 중 `about_to_wait` 은 초당 14~34 회 돌았고
  사용자 이벤트가 회차를 연 것은 초당 0 회였다. 회차는 전부 `about_to_wait` 에서 돌았다.
- 원인이 IPC wake 인지 가르기 위해 PTY 를 안 만드는 조회 부하(조회 4 종, 연결 16, 15 초)도 쟀다 —
  변경 전 바이너리도 타이머 최대 간격이 2.03 s 였다. 조회는 한 회차가 수십 µs 라 wake 가 쌓이기
  전에 큐가 빈다. 변경 전 바이너리에서 탭 부하의 연결 수를 줄이면 간격도 준다(연결 1: 1.99 s,
  연결 4: 3.56 s, 연결 16: 21.3 s).
- headless 는 이 형태가 없다. 메인 루프가 이벤트 **하나마다** due 한 타이머를 돌린다
  (`src/boot.rs` 의 루프). 같은 부하에서 headless 의 타이머 최대 간격은 2.01 s 였다.

## Decision

**gui 의 `IpcReady` 처리기는 직전 회차가 끝난 뒤 한 회차 예산(`ROUND_TIME_BUDGET`)이 지났을 때만
회차를 연다.** 그 전에 온 wake 는 루프를 깨우는 일만 하고, 명령은 곧 올 `about_to_wait` 의 회차가
집는다. 회차가 안 돌면 답이 안 나가 새 명령도 안 오므로 사용자 이벤트 큐는 곧 비고, 루프는
`about_to_wait`(타이머 · 회차 · 렌더)으로 넘어간다. 규칙은 `src/app/ipc.rs` 의 `IpcPacer` 가 들고,
회차가 끝날 때마다(어느 자리에서 돌았든) 끝난 시각을 적는다.

**회차가 예산에서 멈췄으면 루프를 한 번 더 깨운다**(`IpcReady` 를 스스로 보낸다). 남은 명령의 wake
는 이미 건너뛴 이벤트로 소비됐을 수 있어, 안 깨우면 다른 입력이나 다음 타이머가 올 때까지 남는다.
큐가 빈 회차는 깨우지 않으므로 busy-spin 은 없다.

**사용자 이벤트 쪽 회차를 없애지는 않는다.** 플랫폼 모달 루프(창 크기 조절 · 메뉴 추적 등)가
`AboutToWait` 없이 사용자 이벤트만 전하는 구간이 있다면, 그 구간에서 IPC 를 살리는 것이 이 경로다.
그런 구간이 macOS · Windows 에 실제로 있는지는 이 머신(x11)에서 잴 수 없다 — 그래서 호환을 지키는
쪽을 골랐다. 한 회차 예산이 지난 wake 는 여전히 회차를 연다.

**headless 는 바꾸지 않는다.** 이벤트마다 타이머를 돌리므로 부하가 타이머를 밀어내지 않고, 남은
명령은 그 명령들이 부른 wake 가 다음 회차를 부른다(ADR-0313 의 근거 그대로).

## Consequences

- **얻은 것**: gui 에서 지속 IPC 부하 중에도 타이머(전역 훅 · busy · idle-timeout · plugin pump
  안전망)와 렌더가 돈다. ADR-0410 의 "회차가 한 프레임보다 오래 루프를 쥐지 않는다" 가 gui 에서
  비로소 참이 된다.
- **잃은 것**: 무거운 부하의 처리량이 준다 — 실측 20 초 명령 수 4,974(시간 예산만) → 3,831(이 결정).
  그 몫이 렌더·타이머로 간 시간이다.
- **잃은 것**: 한 iteration 에 회차가 둘까지 돈다(사용자 이벤트 하나 + `about_to_wait` 하나). 사용자
  이벤트 쪽 회차는 예산 하나가 지났을 때만 열리므로 둘째가 연달아 붙지는 않는다.
- **미측정**: 스스로 깨우기가 **유일한** wake 인 구성을 만들지 못했다. 탭 만들기로 쌓은 64 건 버스트는
  스스로 깨우기를 끈 빌드에서도 추가 입력 없이 끝났다(1.23 · 1.40 s, 켠 빌드 1.21 · 1.68 · 2.35 s) —
  새 셸의 PTY 출력과 렌더가 다음 iteration 을 부르기 때문이다. 그런 부수 wake 가 없는 느린 명령이
  생기면 차이가 드러난다.

## Alternatives Considered

- **사용자 이벤트 처리기에서 회차를 없애고 `about_to_wait` 에서만 돈다** — 가장 단순하다. 안 고른
  이유: 모달 루프 구간에서 `AboutToWait` 가 안 오는 플랫폼이 있으면 그동안 IPC 가 멈춘다. 이 머신에서
  그 구간을 잴 수 없어, 경로를 남기고 양보만 시키는 쪽이 동작을 덜 바꾼다.
- **wake 를 합친다(대기 중인 `IpcReady` 가 있으면 새로 안 보낸다)** — 이벤트 수가 준다. 안 고른 이유:
  회차가 도는 동안 새 명령 하나가 새 wake 하나를 보내는 것은 그대로라, 사용자 이벤트 루프가 끝나지
  않는 형태가 남는다. 그리고 "대기 중" 표시를 언제 내리는가에 따라 wake 를 잃는 경합이 생긴다.
- **사용자 이벤트 처리기에서 due 한 타이머도 함께 돌린다** — 타이머만은 산다. 안 고른 이유: 창
  입력(x11 이벤트는 같은 iteration 의 앞에서 비운다)과 렌더는 여전히 못 돈다.
- **회차 사이의 양보 간격을 예산과 다른 값으로 둔다** — 파생할 값이 없다. 회차 하나가 쥘 수 있는
  시간과 같은 값을 쓰면 "IPC 가 루프를 쥐는 시간 ≤ 나머지에 돌려주는 기회" 로 읽힌다.

## Reconsideration Triggers

**채널이 붙는 것** — 판정 시점에 레포가 읽을 수 있는 사실이다.

- 양보 규칙이 빠지거나 방향이 뒤집히면 `a_wake_right_after_a_round_yields_to_the_rest_of_the_loop` ·
  `a_wake_a_budget_after_the_last_round_runs_one` 이 빨개진다.
- 회차가 멈춘 이유를 호출자에게 안 돌려주게 되면 `only_rounds_that_took_something_are_counted` 가
  빨개진다.

**원리적으로 안 붙는 것** — 사람이 관측해야 한다. 재는 법을 함께 적는다.

- **gui 에서 지속 부하 중 타이머가 도는가.** 재는 법: 격리 홈 gui 에 `set global-hook --condition
  interval:1 --command "date +%s.%N >> <파일>"` 을 걸고, 연결 16 개로 탭 만들기·목록·닫기를 20 초
  보내는 동안 기록의 최대 간격을 본다. 2 초 남짓이면 돈다.
- **macOS · Windows 에서 모달 루프 중 IPC 가 도는가.** 재는 법: 창 크기를 조절하거나 메뉴를 연 채
  `tasty list info` 가 답하는지 본다. 이 경로가 불필요하다고 확인되면 첫째 대안(사용자 이벤트 쪽
  회차 제거)을 다시 본다.
- **winit 이 사용자 이벤트 루프를 한 iteration 안에서 자르게 바뀌는가.** 재는 법: 판올림 때
  `platform_impl/linux/x11/mod.rs` 의 `single_iteration` 을 읽는다. 자르게 되면 이 결정의 양보는
  불필요해진다.

## References

- 전제를 고친 대상: [ADR-0410](0410-a-dispatch-round-also-stops-at-a-time-budget-and-callers-are-served-in-arrival-order.md)
  (회차가 끝나면 gui 는 렌더·타이머로 넘어간다는 문장)
- 관련: [ADR-0313](0313-the-dispatch-round-budget-is-the-connection-bound.md) (명령마다 wake 한 번이
  이월을 보장한다는 근거 — headless 에서는 그대로다)
- 결정이 실현된 현재 위치: `src/app/ipc.rs` 의 `IpcPacer` · `App::process_ipc`,
  `src/app/event_handler.rs` 의 `IpcReady` 처리, `src/app/ipc_round.rs` 의 `IpcRound::finish`
- 정본 문서: [data-flows](../architecture/data-flows.md) §3 · [timer-hub](../dev-guide/timer-hub.md)
