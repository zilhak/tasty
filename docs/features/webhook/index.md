# 웹훅 (Inbound webhook listener)

- **Status**: Implemented
- **주체**: 로컬 사용자 · AI Agent (`webhook.*`, local-only)
- **ADR**: [ADR-0046](../../adr/0046-webhook-owner-trust-one-way-ack.md)(신뢰 모델·불변식) · [ADR-0047](../../adr/0047-shared-hook-handler-registry-source-gate.md)(공유 핸들러 레지스트리)
- **코드**: `src/webhook/`(리스너·레지스트리·lifetime·인증·남용차단·영속화) · `src/adapters/ipc/handler/webhook.rs`(IPC) · `crates/tasty-cli/src/commands/webhook.rs`(CLI)
- **화면**: 없음 — headless 전용(경고는 기존 toast/`tracing::warn!` 재사용)

## 목적

GitHub Action 처럼 **외부 이벤트가 HTTP 로 들어오면 tasty 를 구동**하는 경량 인바운드 서버다. tasty 의 제어용 IPC 포트(loopback 전용, [ADR-0004](../../adr/0004-ipc-transport-tcp.md))와 별개로, `0.0.0.0` 의 설정 포트를 열어 외부 발신자의 통지를 받는다. 실제 외부→내부 포워딩은 공유기/OS 몫이고, tasty 가 제공하는 건 "특정 포트에 특정 규칙으로 데이터가 들어오면 지정 핸들러를 구동하라" 이다.

대표 흐름: tasty 의 어떤 기능이 외부에 작업을 걸어둠 → 그 작업이 완료/오류나면 외부가 웹훅으로 통지 → tasty 가 내부적으로 반응(예: `notification.create`). 웹훅 응답은 "잘 전달됨" ACK 뿐이다.

## 내부 동작 (headless-valid)

### 싱글턴 라우터

프로세스당 리스너는 **단 하나**다. 다수의 웹훅 등록을 opaque path 로 멀티플렉싱하며, 개별 웹훅은 port/path 를 지정하지 못한다 — 리스너가 발급·은닉한다. 라우팅 키는 `(port, opaque path)` 로, 현재는 단일 포트만 실사용한다.

- **opaque id**: 등록 시 랜덤 8바이트 → 16 hex 소문자(`gen_opaque_id`). **비순차**라 열거를 막고, keyspace 스캔은 남용차단으로 보완한다.
- **발급 URL**: `http://{host}:{port}/{id}`. `0.0.0.0`/빈 호스트는 표기상 `127.0.0.1` 로 치환.

### 요청 처리 흐름

`tiny_http`([ADR-0048](../../adr/0048-webhook-http-tiny-http-blocking.md))가 bind + accept 하고, 요청마다 worker 스레드에서:

1. **남용차단 선검사** — 출처 IP 가 쿨다운 중이면 즉시 `429`.
2. path/query 분리, 헤더 소문자 정규화, 바디 JSON 파싱(실패 시 `null`).
3. **매칭**(`match_request`) — path 없음 `404`, lifetime 만료 `410`(lazy 삭제), 메서드 불일치 `405`(카운트 미차감), 성공 시 카운트 1 차감(소진되면 삭제).
4. 매칭 성공이면 **인증 검증** — 실패 시 `401`.
5. `404`/`405` 는 출처 실패로 집계(`record_failure`).
6. **ACK 즉시 응답**(`build_ack`) — 여기까지 핸들러 실행과 무관.
7. **fire-and-forget** — `execute_sequence` 로 핸들러(IpcSequence)를 메인 루프에 전달. 결과는 응답으로 되돌리지 않는다.

### 단방향 ACK (불변식)

HTTP 응답은 **고정 상태코드 + 고정 문자열 바디**뿐이다. `build_ack(status)` 는 IpcSequence 실행 결과를 **인자로 받지 않아** 내부 데이터가 응답에 실릴 코드 경로 자체가 없다([ADR-0046](../../adr/0046-webhook-owner-trust-one-way-ack.md)).

| 상태 | 코드/바디 | 트리거 |
|------|-----------|--------|
| `Received` | 200 `received` | 매칭 성공 |
| `Unauthorized` | 401 `unauthorized` | 인증 설정됐으나 토큰 미제시/불일치 |
| `NotFound` | 404 `not found` | path 없음 |
| `MethodNotAllowed` | 405 `method not allowed` | 메서드 불일치 |
| `Gone` | 410 `gone` | lifetime 만료(lazy 삭제) |
| `TooManyRequests` | 429 `too many requests` | 남용차단 쿨다운 출처 |

### lifetime 6종

`Lifetime { persistence, limit }` = **{영속성 2} × {제한 3}**.

- **persistence**: `Persistent`(재시작 후 config 복원) / `Temporary`(재시작 시 소멸).
- **limit**: `Unlimited` / `TimeLimit { deadline_unix }`(절대 시각) / `CountLimit { remaining }`. (기존 훅의 `once` = `CountLimit{remaining:1}` 에 해당.)

**만료 집행은 타이머 없이 3시점**에서만 확정된다 — ① 호출 시 lazy 판정(`match_request` 가 만료면 삭제 + `410`), ② 재시작 복원 시 만료 엔트리 필터, ③ 명시적 `webhook.sweep`. 한 번도 안 불린 시간제한 웹훅은 그때까지 등록 상태로 남되, 호출되면 즉시 만료 응답한다.

### 영속화 (`~/.tasty/webhooks.toml`)

`Persistent` 웹훅만 저장한다(`Temporary` 는 저장 안 함). 저장 항목: `id`, `methods`, `handler`(또는 인라인 `sequence`), `limit`(kind + `deadline_unix`/`remaining`), `auth`. `TimeLimit` deadline 은 절대 Unix 시각이라 재시작 후에도 정확히 만료한다. 재시작 복원(`restore_into_registry`)은 이미 만료된 엔트리를 등록하지 않고 파일에서 정리한다. `[listener] port` 섹션(포트 설정)과 `[[webhook]]` 배열이 같은 파일을 공유하며, 각각의 writer 가 상대 섹션을 보존한다.

### 선택적 인증 (가벼운 발신자 확인)

웹훅별 옵션이다 — 걸면 지정 위치의 고정 토큰이 일치해야 통과하고, **미설정이면 무인증 통과**한다. tasty 는 인증을 강제하지 않는다. 핸들러가 OS 를 못 건드리고 tasty IPC 만 조작하므로 HMAC 등 하드 보안은 불필요하다([ADR-0046](../../adr/0046-webhook-owner-trust-one-way-ack.md)).

위치 4종(`AuthLocation`): `QueryKey`(`?key=<token>`) · `BearerHeader`(`Authorization: Bearer <token>`) · `BodyField`(바디 JSON 점구분 경로의 문자열 leaf) · `HeaderKey`(임의 헤더). 토큰 비교는 **상수시간**(`ct_eq`)이고, 조회 응답은 위치·키 이름만 노출하고 **토큰 값은 절대 반환하지 않는다**(`auth_summary`).

### 남용차단 (일시 거부)

없는 path/메서드(404/405)로 반복 요청하는 출처를 일시 거부해, 짧은해시 keyspace 스캔·스팸을 막는다. 순수 코어 로직(`AbuseTracker`, 시각 주입)이라 결정론적으로 테스트된다.

- 한 출처가 `window`(기본 10초) 내 `threshold`(기본 20)회 이상 실패하면 `cooldown`(기본 60초) 동안 즉시 `429`. 정상 200 응답은 집계하지 않는다.
- 임계치/윈도우/쿨다운은 환경변수 오버라이드(`TASTY_WEBHOOK_ABUSE_THRESHOLD` / `_WINDOW_SECS` / `_COOLDOWN_SECS`).

### 포트 설정 (설정값 only)

리스너 포트는 **오로지 설정값**에서 온다 — tasty 가 임의 포트로 몰래 대체 bind 하지 않는다(자동 폴백 없음).

- 설정 파일이 처음 없으면 시드 포트 `28429`(User Ports 범위 임의값, 알려진 서비스 포트 아님)를 기록한다.
- 포트가 비면 리스너를 띄우지 않고 경고한다(`PortNotConfigured`). bind 실패(충돌/권한)도 경고하고 사용자가 설정을 고치게 위임한다(`BindFailed`, 자동 회피 없음).
- 경고는 **기존 인프라 재사용** — GUI 는 toast(`ToastManager`), headless 는 `tracing::warn!`. 신규 디자인 컴포넌트가 없어 화면이 없다.

### 부팅 초기화

공용 헬퍼 `webhook::init_from_config(injector)` 를 두 진입점에서 호출한다 — GUI 는 `window_lifecycle` 의 `start_ipc`/injector 확보 직후, headless 는 `boot` 의 IPC 시작 이후. 두 전제(core config 로드 + 메인 루프 IPC 처리 가능)를 만족한 시점이다. 중복 호출은 리스너 내부 bind 가드로 무해하다. init 후 `Persistent` 웹훅을 복원한다.

## 인터페이스

전부 **`local_only`** — plugin 은 호출할 수 없고 CLI/로컬 클라이언트만 가능하다. 포커스 독립(대상은 `id` 로 지정, `list` 는 전 범위 순회).

| IPC | CLI | 동작 |
|-----|-----|------|
| `webhook.register` | `tasty webhook register` | 필요 메서드 + (`--handler <id>` xor `--sequence <json>`) + lifetime + 선택 인증 → `{id, url, ...}` 반환 |
| `webhook.list` | `tasty webhook list` | 전체 목록(각 항목 URL·메서드·steps·lifetime·인증여부) |
| `webhook.info` | `tasty webhook info --id <id>` | 단일 상세 |
| `webhook.unregister` | `tasty webhook unregister --id <id>` | 등록 해제(path 회수) |
| `webhook.sweep` | `tasty webhook sweep` | 만료 웹훅 일괄 정리 → 제거된 id 목록 |
| `webhook.config` | `tasty webhook config [--port <N>]` | 포트 조회/설정(설정은 재시작 후 반영) |

- **register 게이트**: `methods` 빈 배열 거부, `handler`/`sequence` 정확히 하나. `handler` 는 `validate_binding(handler, Webhook)` 로 검증 — 셸/hook-전용 핸들러는 거부([ADR-0047](../../adr/0047-shared-hook-handler-registry-source-gate.md)). 인라인 `sequence` 는 익명 핸들러(`user/wh-<slug>`)로 레지스트리에 등록된다.
- **lifetime 파라미터**: `--persistent`(bool), `--ttl-secs` xor `--count`(둘 다 없으면 `Unlimited`).
- **auth 파라미터**: `--auth-location <query|bearer|body|header>` + `--auth-token`(상호 requires), bearer 외에는 `--auth-key`.
- **핸들러**가 소비하는 페이로드→params 치환·source 게이트는 [공유 훅 핸들러 레지스트리(ADR-0047)](../../adr/0047-shared-hook-handler-registry-source-gate.md) 참조. 현재 런타임 레지스트리는 이 인라인 `sequence` 등록(익명 핸들러)이 유일한 소비처다.

## 비-목표 (Out of scope)

- **HTTPS/TLS 종단** — 리버스 프록시/공유기에 위임(사용자 요구가 "포워딩은 OS/공유기 몫").
- **외부 발신자의 조회/응답 채널** — 응답은 ACK 전용. 내부 상태 조회는 로컬 소유자 채널(`list`/`info`)로만.
- **웹훅에서의 OS 셸 실행** — 셸(`ShellCommand`)은 기존 훅(source `hook`) 전용, 웹훅 바인딩 불가.
- **plugin 의 웹훅 등록** — 현재 `webhook.*` 는 local-only(plugin 미노출).
- **plugin 프로세스의 직접 소켓 소유** — 코어가 소켓을 소유한다.
- **웹훅 외 프로토콜(raw TCP 등)** — HTTP 웹훅으로 확정.

## Acceptance Criteria

- [ ] Given 포트 설정됨 When `webhook.register --method POST --sequence '<ipc-seq>'` Then 발급 URL 반환, `curl -XPOST` 시 IpcSequence 가 실행되고 응답은 고정 ACK 바디만.
- [ ] Given 등록된 웹훅 When `webhook.unregister` 후 그 path 호출 Then `404`.
- [ ] Given `CountLimit{remaining:N}` When N+1 회 호출 Then N 회 후 소멸(다음 호출 `410`/`404`).
- [ ] Given `TimeLimit` deadline 경과 When 호출 또는 `webhook.sweep` Then `410` + 삭제.
- [ ] Given 인증 설정된 웹훅 When 토큰 불일치 Then `401`; 미설정 웹훅은 무인증 통과.
- [ ] Given 없는 path 를 임계치 초과 반복 When 같은 출처 재요청 Then 쿨다운 동안 `429`(정상 웹훅 무영향).
- [ ] Given `ShellCommand` 핸들러 When 웹훅 바인딩 시도 Then source 게이트로 거부.

## 관련

- [hooks](../hooks/index.md) — 내부 이벤트 트리거(웹훅과 대칭인 trigger 출처) · [notifications](../notifications/index.md) · [file-handler](../file-handler/index.md)(레지스트리 정본 템플릿)
- [api](../../reference/api.md#기타-호스트) · [ADR-0046](../../adr/0046-webhook-owner-trust-one-way-ack.md) · [ADR-0047](../../adr/0047-shared-hook-handler-registry-source-gate.md) · [ADR-0048](../../adr/0048-webhook-http-tiny-http-blocking.md)
