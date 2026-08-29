# 중앙 타이머 허브 (TimerHub)

메인 루프의 **시간축** 주기 작업은 전부 `tasty-timer` 의 `TimerHub` 에 키로 등록된다.
전용 ticker 스레드를 새로 만들거나 매 프레임 `Instant` elapsed 게이트를 두는 방식은
쓰지 않는다.

- 크레이트: [`crates/tasty-timer`](../../crates/tasty-timer/src/lib.rs)
- 호스트측 키/등록: [`src/app/timers.rs`](../../src/app/timers.rs)
- gui 실행부: `src/app/event_handler.rs` `about_to_wait`
- headless 실행부: `src/boot.rs` `run_headless` 의 `recv_timeout` 루프

## 두 축을 섞지 않는다

메인 루프에는 성격이 다른 두 축이 있다.

| 축 | 트리거 | 예 | 허브 대상 |
|---|---|---|---|
| 프레임축 | "이벤트가 있었으니 큐를 비운다" | `dispatch_pending_*`, `process_ipc` | ✘ |
| 시간축 | "N ms 마다 / N ms 뒤에 한 번" | busy 재평가, attach 뷰 갱신, 메뉴 트래킹 | ✔ |

프레임축에는 주기 개념이 없다. 허브는 시간축만 다룬다.

## API

```rust
hub.every(key, interval, precision, now);      // 반복
hub.once_after(key, delay, precision, now);    // 일회성(발화 후 자동 제거)
hub.cancel(key);
hub.is_registered(key);

let due: Vec<K> = hub.drain_due(now);          // due 한 키만. 실행은 호출자 몫
let at: Option<Instant> = hub.next_deadline();  // None = 깨울 이유 없음
hub.snapshot();                                 // 관측용
```

**허브는 콜백을 담지 않는다.** `Box<dyn FnMut(&mut App)>` 를 App 필드에 두면 drain 중
`&mut self` 를 다시 빌릴 수 없어 take-run-restore 우회가 강요되고 재진입에 취약해진다.
키만 반환하므로 실행부는 평범한 `match` 로 남는다.

**모든 API 가 `now` 를 인자로 받는다.** 내부에서 `Instant::now()` 를 부르지 않으므로
단위 테스트가 가짜 기준시각으로 완전히 결정론적이다. 호출부는 프레임 앞머리에서 읽은
`now` 를 재사용하되, 프레임 말미에 데드라인을 새로 잡을 때는 시각을 다시 읽는다
(긴 프레임 뒤에 이미 지난 데드라인을 등록하면 다음 프레임이 즉시 깨어나 spin 한다).

### 반복 타이머의 드리프트

다음 발화 시각은 `now` 가 아니라 **직전 데드라인**에 `interval` 배수를 더해 정한다.
1초 타이머를 2.5초 시점에 drain 하면 다음은 3.0초다(2.0 도 3.5 도 아니다) — 위상이
유지되므로 프레임 지연이 누적되지 않는다. 절전 복귀처럼 `u32` 배수로 표현할 수 없을
만큼 밀리면 `now` 기준으로 재정렬한다(밀린 만큼을 몰아서 발화시키지 않는다 — 허브는
발화를 큐잉하지 않는다).

## Strict vs Lax

| | `next_deadline()` 기여 | 쓰는 곳 |
|---|---|---|
| `Strict` | `next_due` — 그 시각에 반드시 깨운다 | 사용자가 지연을 체감하는 작업 |
| `Lax { slack }` | `next_due + slack` — slack 전까지 기여하지 않는다 | 늦어도 되는 정리/유지보수 |

`Lax` 는 due 해도 그 자체로 wakeup 을 만들지 않고, **다른 이유로 깨어난 프레임에
편승**해 실행된다(coalescing). `deadline + slack` 을 넘기면 hard deadline 으로 승격돼
반드시 깨운다 — 완전 idle 상태에서 영원히 안 도는 starvation 을 막는다.

판단 기준: *못 돌면 사용자가 즉시 알아채는가*. busy indicator(1s)·attach mirror(3s)·
메뉴 트래킹(8ms)은 `Strict`. "언젠가 치우면 되는" 캐시 정리류는 `Lax`.

## 대기 전략 — 왜 waker 스레드인가 (대안 B 채택)

두 안이 있었다.

- **대안 A (순수 `WaitUntil`)** — ticker 스레드를 전부 없애고 `ControlFlow::WaitUntil`
  로만 깨운다.
- **대안 B (단일 waker 스레드)** — 고정 주기 ticker 3개를 **데드라인 기반 waker 1개**로
  합치고 `EventLoopProxy::send_event` 로 깨운다. `WaitUntil` 은 보조로만 쓴다.

**대안 B 를 채택했다.** 근거: tasty 에는 창이 없거나 사실상 없는 상태가 실재한다 —
macOS 최소화는 window 를 파괴하고 `CoreState` 만 `parked_states` 에 남기며
(`src/app/window_lifecycle.rs`), tray 상주는 모든 창을 닫는다
([system-tray](../design/policies/system-tray.md)). 그 상태에서도 busy forward(원격
attach client 로의 `StreamControl::Activity`)와 글로벌 훅은 계속 돌아야 한다 —
`poll_busy_states` 가 `parked_states` 를 순회하는 이유가 정확히 그것이다.

그런데 **창 없이도 이벤트 루프가 계속 깨어난다는 보장은 플랫폼마다 다르다.**
`src/app/shutdown_machine.rs` 의 창 없는 종료 경로가 같은 이유로 이미 구동을 이벤트
루프에 맡기지 않는다. 대안 A 는 그 보장을 플랫폼 구현에 넘기므로, 세 OS 에서 최소화·
tray 상주 상태의 1Hz busy forward 를 실증하기 전에는 채택할 수 없다.

대안 B 는 절전 이득을 그대로 얻으면서 그 위험이 없다:

- 스레드 수 3 → 1 (`busy_tick` 1s + `attach_tick` 3s + headless `spawn_busy_ticker` 1s
  → `tasty-timer-waker` 1개)
- 깨우는 시점이 **고정 주기가 아니라 허브가 정한 데드라인**이다. 등록이 없으면 무기한
  park(idle wakeup 0), 등록되면 `Condvar` 로 즉시 재무장한다.
- 정확성은 waker 스레드가 책임지고, `WaitUntil` 은 창이 있을 때 wake 지연을 줄이는
  보조다. 둘 다 같은 데드라인(`sync_timer_control_flow`)을 받는다.

**headless 는 waker 스레드조차 없다.** 메인 루프가 `rx.recv_timeout(deadline - now)` 로
직접 데드라인을 지키므로 wake 신호가 필요 없다(`AppEvent::TimerTick` 은 gui 전용).

## 실행부

### gui — `about_to_wait`

```rust
for key in self.timers.drain_due(Instant::now()) {
    match key {
        Tick::Busy => { poll_busy_states(); poll_global_hooks(); poll_idle_timeout_hooks(); }
        Tick::AttachView => poll_attach_views(),
        Tick::NativeMenu => {}   // 깨어나는 것 자체가 목적
    }
}
// ... 파이프라인 ...
self.sync_timer_control_flow(event_loop);   // waker + ControlFlow 를 한 번에
```

`set_control_flow` 는 **말미 1회**만 호출한다. 기본값을 앞에서 깔고 뒤에서 덮어쓰면
합성 순서에 따라 데드라인이 유실된다.

### headless — `run_headless`

```rust
let ev = match app.timers.next_deadline() {
    Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
        Ok(ev) => Some(ev),
        Err(Timeout) => None,          // 타이머만 돌린다
        Err(Disconnected) => break,
    },
    None => match rx.recv() { Ok(ev) => Some(ev), Err(_) => break },
};
for key in app.timers.drain_due(Instant::now()) { /* headless 실행부 */ }
let Some(event) = ev else { continue };
```

gui/headless 는 **같은 키 집합을 같은 주기로** 굴린다. `Tick::Busy` 가 headless 에서
하는 일은 넷이다 — busy 재평가 + attach forward / 글로벌 훅 / idle-timeout 훅(바인딩
실행 + `HookFired` enqueue 포함) / plugin pump 안전망. 하나라도 빠지면 headless 원격
attach 가 조용히 멈춘다. `Tick::AttachView` 는 렌더가 없는 headless 에선 등록하지
않는다(키 자체가 `#[cfg(feature = "gui")]`).

## 가드 중 타이머는 정지한다 (계약)

`about_to_wait` 의 shutdown / boot 가드는 조기 return 한다. **그 동안 `drain_due` 에
닿지 않으므로 steady-state 타이머는 통째로 멈춘다.** 의도된 계약이다 — 종료·부팅 중에
주기 작업이 정리 중이거나 아직 구성되지 않은 상태를 건드리지 않는다. 두 가드는 각자
자기 `ControlFlow`(프레임 워치독)를 직접 설정하고 빠져나가므로 말미의
`sync_timer_control_flow` 와 합성되지 않는다.

가드가 풀리면 밀린 타이머는 **한 번만** 발화하고 위상이 재정렬된다(허브는 발화를
큐잉하지 않는다).

## 새 주기 작업을 추가할 때

1. `src/app/timers.rs` 의 `Tick` 에 키를 추가한다. gui 전용이면
   `#[cfg(feature = "gui")]` 로 게이트한다(워크스페이스 `dead_code = "deny"`).
2. 주기 상수를 같은 파일에 두고 `register_steady_state` 에 등록한다. 조건부로만
   도는 작업이면 등록 대신 발생 시점에 `once_after` / 해제 시점에 `cancel` 한다
   (`reschedule_pending_menu_poll` 이 그 형태다).
3. gui/headless 실행부 `match` 에 arm 을 추가한다. 한쪽만 도는 키면 다른 쪽 `match`
   에서 `cfg` 로 사라지므로 컴파일러가 누락을 잡아준다.
4. `Strict` / `Lax` 를 위 기준으로 고르고, 이유를 키의 doc-comment 에 남긴다.

## 허브 대상이 아닌 것

시간을 다루지만 주기 작업이 아니라 허브에 넣지 않는 코드:

| 위치 | 성격 |
|---|---|
| `src/app/window_lifecycle.rs` 부팅 deadline 대기 | 1회성 동기 대기 |
| `src/app/shutdown_machine.rs` `HEADLESS_POLL_INTERVAL` | 창 없는 종료의 블로킹 스텝 — 의도적으로 이벤트 루프에 안 맡긴 코드 |
| `src/adapters/production/tcp_ipc_server.rs` accept 재시도 | IO 재시도 |
| `src/webhook/abuse.rs` | 시각 비교(순수 함수). 실행 스케줄 없음 |
| `src/adapters/ui/` toast/banner TTL | 렌더 프레임 종속 애니메이션(프레임축) |
| `crates/tasty-terminal` `ALIVE_CHECK_INTERVAL` | syscall 레이트 리밋 |
| `src/core/agent/runner_thread.rs` `TICK_INTERVAL` | 워커 스레드 소유. 메인 루프와 실행·소유권 경계가 다르다 |
| `crates/tasty-plugin-*/` 자체 폴링 | 별도 **프로세스** — 본체 허브가 닿을 수 없다 |

## 참고

- [data-flows](../architecture/data-flows.md) — 메인 루프의 두 축
- [shutdown-sequence](../architecture/shutdown-sequence.md) — 가드 중 정지 계약의 배경
- [multi-window](../architecture/multi-window.md) — 단일 프로세스·메인 스레드 단일 루프 전제
