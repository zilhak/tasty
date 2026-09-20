# 텔레메트리 (Telemetry)

- **Status**: Implemented
- **주체**: AI Agent — 측정 대상이자 **통제 주체**다. 에이전트가 다른 에이전트에게 cap 을 걸고 지우고 푼다(`agent` 는 파라미터로 지목한다). 로컬 사용자도 CLI 로 같은 것을 할 수 있으나 1급 대상이 아니다 — headless 인스턴스에는 로컬 사용자가 아예 없다([identity](../../identity.md) §2.2).
- **ADR**: [0246](../../adr/0246-telemetry-has-no-opt-out-and-its-token-is-a-declaration.md) — 옵트아웃 축을 두지 않는다, 권한 토큰은 선언이다
- **코드**: `telemetry.*` 핸들러, `tasty-telemetry`, 영속 `tasty-memory`
- **화면**: 없음 (cap 발화 시 알림)
- **메서드 목록**: [reference/api](../../reference/api.md#텔레메트리-telemetry)

## 목적

AI 에이전트 활동을 도메인 메트릭으로 **기록·집계·차단**하는 관측 계층. 비용(토큰)·호출량을 추적하고 임계 초과 시 액션을 발화한다.

## 내부 동작

### 모델

Metric(`input_tokens`/`ipc_calls`/…) × Agent(`tasty.<plugin_id>`/`cli.<exe>`/`_host`, 미명시 시 CallerContext 자동) × Workspace(없으면 `global`) × Op(`Set`/`Inc`, 시계열은 둘 다 sum) × Window(`1m/5m/1h/1d`) × Tags. 이벤트는 `tasty.telemetry.event.{ts}.{seq}` 로 영속, 조회는 prefix scan + 순수 집계(재시작 후 누적 보존).

**집계본은 영속되지 않는다.** bucket 은 조회할 때마다 raw event 로부터 새로 만들어지고 버려진다 — 주기 rollup task 도, `tasty.telemetry.bucket.*` 키도 없다. 따라서 **raw event 보존량이 곧 조회 가능 범위**이며, 그 상한은 관측 로그 3종 공통 정책(`store::log_retention`)이 정하는 **최근 20,000 이벤트**다. 조용한 인스턴스에서는 수일치, 폴링이 도는 인스턴스에서는 수십 분치가 되므로 조회 범위가 데이터 양에 종속된다. 롤업을 신설하지 않기로 한 근거와 재검토 조건은 [ADR-0085](../../adr/0085-ipc-log-retention-bounded.md).

`ipc_calls` 는 dispatcher 가 plugin IPC 호출마다 자동 1회 기록(`tags.method`). `_host` 또는 `telemetry.*` 는 자기측정/재귀 방지로 skip.

### IPC 진입 관측

GUI·headless의 외부 소켓과 plugin host-call은 라우팅 전에 권한·cap·rate-limit을
검사한다. 통과한 호출만 `ipc_calls`로 한 번 기록하며 App 인터셉트, namespace forward,
전 창 합산, 일반 handler 사이에 예외를 두지 않는다. 일반 handler로 내려가도 다시
소비하거나 기록하지 않는다. 별칭은 canonical 메서드 태그로 남는다.
권한/한도 거부는 Allow 관측을 남기지 않고 기존 Deny audit·권한 격상 흐름을 유지한다.
`telemetry.*`와 호스트 caller의 관측 제외, Local의 rate 면제는 그대로다.
[ADR-0277](../../adr/0277-ipc-admission-and-observation-run-once.md).


### Cost Cap

(agent, metric, window) raw sum 이 `threshold` 이상이면 `triggered` + 액션:

| 액션 | 후속 IPC |
|------|----------|
| `notify` | 변화 없음 (알림만) |
| `pause` | 그 plugin agent 의 모든 IPC `-32007 cap_blocked`(과거 `stop` 값으로 저장된 cap 은 `pause` 로 자동 마이그레이션됨) |
| `require_approval` | `approval.request`(severity=warn) 자동 + IPC 차단 → 사용자 응답 후 `cap.reset` 으로 재개 |

차단된 plugin 본인은 `cap.reset` 도 막힌다 — **Local(CLI)만 reset**. 누적 임계인 cap 과 *시간당 비율*인 [agent rate-limit](../agent-collaboration/index.md) 은 다른 시스템.

### 이상 탐지

`AnomalyDetector` 가 세 휴리스틱을 유지한다 — **진짜 정체/누수 탐지가 아니라 값싼 신호 기반 후보 알림**이라는 전제로 소비해야 한다.

| 휴리스틱 | 판정 | 기본값 |
|---|---|---|
| `CallBurst` | (agent, method) sliding window 호출 카운트 | 1분 1000회↑ |
| `SlowLoop` | (agent, method, params-hash) sliding window 반복 카운트(동일 파라미터 반복) — 이 params-hash 는 **dedup 단위이기도 하다**(아래) | 5분 20회↑ |
| `RssSurge` | agent 당 최근 5개 RSS 샘플의 **엄격한 단조 증가**(스파이크 1회는 발화 안 함, 추세만) | 5 샘플 |

RSS 값 소스는 caller 타입별로 다르다: **Plugin** 은 host(`tasty-host-plugin::PluginManager`)가 `PluginProcess.child` 의 PID 를 sysinfo 로 30초 간격 직접 sampling(agent 자가 보고는 신뢰 불가 — 정확한 자기 RSS 를 보고할 유인이 없음). **Agent** 는 PID 기반이 구조적으로 불가능(원격/별도 프로세스)해 `telemetry.record` 자가 보고(`metric == "rss_bytes"`)로 받는다. 발화는 셋 다 동일하게 `tasty.telemetry.anomaly.*` 영속 + 알림이고, 1분 쿨다운 dedup 도 공유한다. 다만 **dedup 키가 셋 다 `subject` 인 것은 아니다**:

| 휴리스틱 | dedup 키 | subject |
|---|---|---|
| `CallBurst` | `(agent, kind, method)` | `method` — 키와 같다 |
| `SlowLoop` | `(agent, kind, "{method}#{params_hash:016x}")` | `method` — **키와 다르다** |
| `RssSurge` | `(agent, kind, "rss_bytes")` | `rss_bytes` — 키와 같다 |

`SlowLoop` 만 dedup 키에 `params_hash` 를 덧붙여, 같은 method 라도 파라미터 조합이 다르면 **독립된 loop 로 취급해 각자 쿨다운을 갖는다**(`params_hash` 는 detail 에도 실린다). 그래서 같은 method 의 발화가 수백 ms 간격으로 연달아 보이는 것은 정상이며 — 조합 수만큼 각자 분당 1건씩 나온다 — 쿨다운 버그가 아니다. surface 마다 파라미터가 다른 폴링에서는 이 배증이 커서, 18시간에 21,102건(≈ 조합 20종 × 1,080분)이 쌓인 실측이 있다. 보존 상한은 [ADR-0085](../../adr/0085-ipc-log-retention-bounded.md) 의 공통 정책(50시간 · 5,000건)을 따른다.

### 세션 요약

결정론적 순수 집계(LLM 없음): tokens(ipc_calls 제외 metric sum) / ipc_calls(method 별 top-N) / approvals 분포 / anomalies. `workspace_id` 미지정 시 전 워크스페이스 합산(포커스 독립).

### 요청 압력 게이지 (프로세스 축)

위 `ipc_calls` 와 **다른 축**이다. caller 로 나누지 않고, 저장소를 거치지 않으며, 프로세스
수명 동안 자라지 않는 고정 크기 원자값이다(근거·대안은
[ADR-0305](../../adr/0305-request-pressure-is-a-process-gauge-not-a-per-caller-observation.md),
노출 표면은 [ADR-0333](../../adr/0333-the-pressure-gauge-is-read-by-one-local-only-method-and-split-by-population.md)).
재는 것은 큐 깊이 · 큐 대기 · handler 실행 시간 · plugin 왕복 · DB 지연의 count·sum·max 이고,
평균은 파생이라 메서드로 낸다. 시간 축 셋(`db` 제외)에는 **고정 경계 분포**가 하나씩 더
붙는다([ADR-0340](../../adr/0340-the-pressure-answer-counts-seats-and-carries-a-fixed-bound-distribution.md)).
여기에 **시간이 아닌 축**이 하나 더 있다 — 동시 IPC 연결 자리다.

`system.pressure`(local-only) 가 그 누계를 읽는다. 응답은 **모수마다 한 덩어리**다.

| 덩어리 | 재는 자리 | 모수 |
|---|---|---|
| `queue_before_gate` | 명령이 큐에서 나온 직후 (`App::process_ipc` · `pump_ipc`) | 큐에 앉았던 **전부** — 뒤에 거부될 요청도 센다 |
| `handler_after_gate` | 게이트 통과 뒤 (`handle_checked_request`) | 실제로 **실행된 것만** |
| `plugin_round_trip` | plugin 응답 매칭부 (`PluginManager::handle_plugin_response`) | 응답이 **매칭된 요청만** — 취소·만료된 것은 끝점이 없다 |
| `db` | `MemoryStore` 의 `tx.commit()` 뒤 · `checkpoint_truncate` | **성공한** commit 과 시도된 checkpoint — 거부된 쓰기는 롤백이라 안 센다 |
| `connections` | accept 루프의 자리 획득·반납 (`TcpIpcServer`) | 이 포트에 붙은 **TCP 연결 전부** — 요청을 하나도 안 보내는 attach·mesh 스트림도 센다 |

셋째는 앞의 둘과 축이 다르다. 앞의 둘은 호스트가 **자기 큐와 자기 handler** 에서 보낸
시간이고, 셋째는 호스트가 **남의 프로세스를 기다린** 시간이다(`PluginWaitStats`). 큐도
handler 도 빠른데 응답이 느리면 그 시간은 plugin 안에 있었던 것이다. 게이지 하나를
프로세스가 소유하고 plugin manager 에 **핸들만** 넘긴다 — 창마다 매니저를 다시 만들어도
같은 핸들을 받으므로 축이 프로세스로 유지된다. 주입이 없는 구성(단위 시험)에서는
아무것도 세지 않아 `matched` 가 0 으로 남는다.

넷째는 **디스크가 받아준** 시간이다(`DbLatencyStats`). commit 은 handler 실행 시간 *안에*
들어 있으므로, 그 둘을 나란히 두면 "handler 가 느리다" 와 "handler 안의 쓰기가 느리다" 가
갈린다. 게이지는 `MemoryStore` 가 열릴 때 그 안에서 태어나고 부팅 wiring 이 핸들을 `Core`
에 복제해 둔다 — 진단이 값을 읽을 때 **메모리 뮤텍스를 안 잡는다**(적체를 재려고 적체하는
자물쇠를 잡으면 진단이 같이 막힌다). checkpoint 는 부팅 때의 WAL 되감기이고, 다른 커넥션이
읽는 중이면 못 끝내고 돌아오므로 그 횟수를 `checkpoints_busy` 로 따로 센다 — 시간만 봐서는
느린 것과 경합한 것이 안 갈린다. 스토어를 못 여는 조립(단위 시험)에서는 아무도 안 올려
`commits` 가 0 으로 남는다.

다섯째는 **시간이 아니라 자리**다(`ConnectionStats`). 앞의 넷이 전부 "얼마나 걸렸나" 인
반면 이것은 "자리가 남았나" 이고, 그 구분이 없으면 자원 포화가 밖에서 안 보인다 — 요청이
하나도 안 느려도 동시 연결 상한(`MAX_CONCURRENT_CONNECTIONS`, 256)이 차면 새 client 는
**붙지도 못한다.** 그 거절은 요청이 되기 전에 일어나므로 앞의 네 덩어리 어디에도 안 남고
`ipc_calls` 에도 안 남는다(JSON-RPC 요청이 아니라 TCP 연결이다). 값은 다섯이다 —
`live`(지금 붙어 있는 수, **이 응답에서 유일하게 내려가는 값**) · `live_max`(관측된 최댓값) ·
`limit`(서버가 집행하는 상한) · `accepted`(자리를 받아 간 누계) ·
`refused_saturated`(상한에 걸려 거절된 누계). `limit` 이 같이 나가야 `live` 가 상한에 얼마나
가까운지가 한 응답 안에서 읽힌다. 거절 로그는 포화로 들어가는 순간만 `warn` 이고 그 뒤로는
`debug` 로 접히지만, **접히는 것은 로그뿐이고 `refused_saturated` 는 전부 센다.** 게이지는
`Core` 가 낳아 부팅이 IPC 서버에 핸들을 건넨다 — `db` 와 방향이 반대인 이유는 서버가 `Core`
보다 **뒤에** 뜨기 때문이다. 서버가 안 뜬 조립(단위 시험)에서는 `accepted` 가 0 으로 남아
"연결을 받은 적이 없다" 로 읽힌다.

#### 분포 — 평균·최대가 못 답하는 것

`queue_before_gate.wait_us_hist` · `handler_after_gate.us_hist` ·
`plugin_round_trip.us_hist` 가 같은 관측의 **분포**를 든다. 평균과 최대만으로는 "전부
조금씩 느린가, 대부분 빠른데 꼬리가 몇 건인가" 가 안 갈린다 — 넷 중 한 건이 1 s 이고 셋이
0 인 분포와 넷이 전부 250 ms 인 분포는 count·sum·평균이 **전부 같다.** 처방은 반대다
(뒤는 용량, 앞은 그 한 건의 원인).

경계는 10 µs 부터 1 s 까지 반-십진(√10 ≈ 3.16 배) **열한 칸 + 넘침 한 칸**이고
`LATENCY_BUCKET_BOUNDS_US` 가 정본이다. 파생되지 않은 고른 값이며, 근거는 ADR-0333 의
실측(큐 대기 max 25.4 ms · handler max 0.48 ms)과 plugin 왕복이 초 단위까지 간다는 것이다.
응답은 덩어리마다 `bounds_us` 와 `counts` 를 **함께** 싣는다 — 값과 경계가 떨어지면
소비자가 경계를 복제하고 그 복제본이 갈린다. `counts` 는 `bounds_us` 보다 한 칸 길고
**누적이 아니다**: 칸끼리 겹치지 않아 합이 관측 수이고(Prometheus 의 `le` 누적 버킷과
다르다), 마지막 칸은 상한이 없어 "그 상한을 넘었다" 까지만 말한다 — 얼마나 넘었는지는
같은 덩어리의 `us_max` 가 답한다. **분위수는 호스트가 계산하지 않는다**: 버킷 해상도
안에서만 답할 수 있는 값이라 한 수로 내놓으면 없는 정밀도를 말하게 된다.

`db` 에는 분포가 없다 — 그 게이지는 `tasty-memory` 에 살고 histogram 타입은
`tasty-telemetry` 에 있어 의존 방향이 반대다. `connections` 에도 없다 — 시간이 아니라
자리라 분포를 잴 축이 아니다.

#### 덩어리를 가리지 않고 걸리는 것

아래 셋은 위 `분포` 소절에만 해당하는 서술이 **아니다** — 응답 전체의 성질이다.

두 수의 차는 "거부된 수" 가 아니다 — 게이트를 통과하고도 `handle_checked_request` 를 안
지나는 갈래가 있다(gui 의 app 층 메서드는 그 자리에서 답하고 돌아간다). 그래서 응답은
두 모수를 나란히 두고 뺄셈을 하지 않는다.

관측이 없는 평균은 `null` 이다 — 0 이면 "기다림이 없었다" 와 "잰 적이 없다" 가 같은 값이 된다.

창이 없다는 한계는 그대로다: 프로세스 수명 누계라 "지금 밀리는 중" 과 "부팅 직후 한 번
밀렸다" 가 같은 max 로 보인다. 분포는 그 한계를 **반만** 푼다 — 부팅 직후의 한 건은 꼬리
칸의 `1` 로 남아 그것이 소수임은 보이지만, 그 한 건이 언제였는지는 여전히 안 보인다.

## 인터페이스

- **AI Agent / CLI**: `telemetry.record(_batch)`(`telemetry` 권한) · `summary/timeseries/top` · `cap.{set,list,remove,status,reset}` · `anomaly.list` · `session_summary`. [reference/api](../../reference/api.md#텔레메트리-telemetry).
- **로컬 운영자 / CLI**: `system.pressure` (local-only) · `tasty list pressure` — 위 "요청 압력 게이지" 절.
- **Claude Code 통합**: `tasty claude install` hook 이 `session-start`→`stop` 의 `wall_time_ms`, notification 의 `input_tokens`(`tokens: N` 패턴)를 `tasty.com.tasty.claude` agent 로 자동 적재. [claude plugin](../../plugins/claude/index.md).

## 관련

- [agent-collaboration](../agent-collaboration/index.md) — rate-limit 과의 구분 · [human-handoff](../human-handoff/index.md) — require_approval
