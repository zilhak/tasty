# 웹훅 (Inbound webhook listener)

- **Status**: Implemented
- **주체**: 로컬 사용자 · AI Agent (`webhook.*` — `register` 만 plugin 허용(`Network` 권한), 나머지 local-only)
- **ADR**: [ADR-0032](../../adr/0032-webhook-admission.md)(신뢰 모델·불변식) · [ADR-0027](../../adr/0027-lua-and-hook-execution.md)(공유 핸들러 레지스트리)
- **코드**: `src/webhook/`(리스너·레지스트리·lifetime·인증·남용차단·영속화) · `src/adapters/ipc/handler/webhook.rs`(IPC) · `crates/tasty-cli/src/commands/webhook.rs`(CLI)
- **화면**: 전용 화면 없음. GUI와 headless에서 동작하며 경고는 기존 toast 또는 로그로 알린다

## 목적

GitHub Action 처럼 **외부 이벤트가 HTTP 로 들어오면 tasty 를 구동**하는 경량 인바운드 서버다. tasty 의 제어용 IPC 포트(loopback 전용, [ADR-0006](../../adr/0006-bounded-ipc-transport.md))와 별개의 HTTP 포트로 통지를 받는다. 기본은 이 컴퓨터(127.0.0.1)에서 오는 호출만 받고, 설정 `[webhook] allow_external = true`(Settings › General · `tasty webhook allow-external on`)를 켜면 모든 IPv4 인터페이스(0.0.0.0)에서 받는다. 실제 외부→내부 포워딩은 공유기/OS 몫이고, tasty 가 제공하는 건 "특정 포트에 특정 규칙으로 데이터가 들어오면 지정 핸들러를 구동하라" 이다.

대표 흐름: tasty 의 어떤 기능이 외부에 작업을 걸어둠 → 그 작업이 완료/오류나면 외부가 웹훅으로 통지 → tasty 가 내부적으로 반응(예: `notification.create`). 웹훅 응답은 "잘 전달됨" ACK 뿐이다.

## 내부 동작 (headless-valid)

### 싱글턴 라우터

프로세스당 리스너는 **단 하나**다. 다수의 웹훅 등록을 opaque path 로 멀티플렉싱하며, 개별 웹훅은 port/path 를 지정하지 못한다 — 리스너가 발급·은닉한다. 라우팅 키는 `(port, opaque path)` 로, 현재는 단일 포트만 실사용한다.

- **opaque id**: 등록 시 랜덤 8 바이트 → 16 hex 소문자(`gen_opaque_id`). **비순차**라 열거를 막고, keyspace 스캔은 남용차단으로 보완한다.
- **발급 URL**: `http://{host}:{port}/{id}`. host·port는 리스너가 실제로 bind한 주소(`RuntimePorts`의 웹훅 기록)에서 온다. `0.0.0.0` bind는 표기상 `127.0.0.1` 로 적는다. 리스너가 없으면(탐색 범위가 모두 막힘) 포트 없이 `http://127.0.0.1/{id}` 로 적는다. 저장한 Persistent 웹훅의 URL도 재시작 뒤 그 실행의 포트를 따른다.

### 요청 처리 흐름

`tiny_http`가 HTTP를 받고 요청별 worker가 다음 순서로 처리한다.

1. remote IP가 cooldown 중이면 429로 끝낸다. 선검사를 통과한 `Screened`만 application body를 읽는다.
2. 경로·query·헤더를 정리하고 body 크기를 검사한다. 기본 1MiB를 넘으면 413이며, 상한 이내의 잘못된 UTF-8·JSON은 null로 취급한다.
3. 등록 경로·lifetime·method·인증을 같은 registry 잠금 안에서 확인한다. 인증은 CountLimit 차감보다 먼저 한다.
4. 매칭·인증에 성공하면 등록의 남은 횟수를 차감한다. 이 횟수는 접수된 시퀀스 수이며 내부 step 성공 횟수가 아니다.
5. 401·404·405·413을 출처 실패에 집계한다. 200·410·429는 세지 않는다.
6. 고정 ACK를 보내고 별도 실행을 진행한다. 413과 선차단 429는 `respond_and_close`로 연결을 닫는다.

외부 값은 params의 문자열 leaf에 `${body.x}`·`${header.x}`·`${query.x}`로 치환한다.
메서드명·객체 key·실행 순서는 owner가 고정한다. HTTP 응답에 시퀀스 결과를 넣지 않는다.
직접 ShellCommand를 바인딩할 수 없지만 owner가 고른 IPC의 효과까지 자동으로 안전해지는 것은 아니다.

### 단방향 ACK (불변식)

HTTP 응답은 **고정 상태코드 + 고정 문자열 바디**뿐이다. `build_ack(status)` 는 IpcSequence 실행 결과를 **인자로 받지 않아** 내부 데이터가 응답에 실릴 코드 경로 자체가 없다([ADR-0032](../../adr/0032-webhook-admission.md)).

그래서 **`200` 은 매칭돼 넘겼다는 뜻이지 실행이 됐다는 뜻이 아니다.** 응답은 실행 전에 확정되므로 IpcSequence 가 통째로 실패해도 발신자는 `200` 을 받고, 한 스텝이 실패해도 다음 스텝이 계속 가서 **부분 적용이 정상 종료 상태로 남을 수 있다**(`execute_sequence` — 현재는 조건 분기가 없다). 실패의 유일한 관측점은 `tracing::error!` 로그다. 외부 발신자는 상태코드로 재시도를 정하므로 이 성질이 곧 계약이다.

| 상태 | 코드/바디 | 트리거 |
|------|-----------|--------|
| `Received` | 200 `received` | 매칭 성공 |
| `Unauthorized` | 401 `unauthorized` | 인증 설정됐으나 토큰 미제시/불일치 |
| `NotFound` | 404 `not found` | path 없음 |
| `MethodNotAllowed` | 405 `method not allowed` | 메서드 불일치 |
| `Gone` | 410 `gone` | lifetime 만료(lazy 삭제) |
| `PayloadTooLarge` | 413 `payload too large` | body 가 요청당 상한 초과 |
| `TooManyRequests` | 429 `too many requests` | 남용차단 쿨다운 출처 |

### lifetime 6종

`Lifetime { persistence, limit }` = **{영속성 2} × {제한 3}**.

- **persistence**: `Persistent`(재시작 후 config 복원) / `Temporary`(재시작 시 소멸).
- **limit**: `Unlimited` / `TimeLimit { deadline_unix }`(절대 시각) / `CountLimit { remaining }`. (기존 훅의 `once` = `CountLimit{remaining:1}` 에 해당.)

**만료 집행은 타이머 없이 3시점**에서만 확정된다 — ① 호출 시 lazy 판정(`match_request` 가 만료면 삭제 + `410`), ② 재시작 복원 시 만료 엔트리 필터, ③ 명시적 `webhook.sweep`. 한 번도 안 불린 시간제한 웹훅은 그때까지 등록 상태로 남되, 호출되면 즉시 만료 응답한다.

### 영속화 (`~/.tasty/webhooks.toml`)

`Persistent` 웹훅만 저장한다(`Temporary` 는 저장 안 함). 저장 항목: `id`, `methods`, `handler`(또는 인라인 `sequence`), `limit`(kind + `deadline_unix`/`remaining`), `auth`. `TimeLimit` deadline 은 절대 Unix 시각이라 재시작 후에도 정확히 만료한다. 재시작 복원(`restore_into_registry`)은 이미 만료된 엔트리를 등록하지 않고 파일에서 정리한다. 최상위 `port`(명시 지정 포트)·`format`(파일 형식 표시)과 `[[webhook]]` 배열이 같은 파일을 공유하며, 각각의 writer 가 상대 섹션을 보존한다. 파일을 읽거나 파싱하지 못하면 어느 writer도 쓰지 않는다(`tasty_settings::webhook_port_file::read_table`). Persistent 등록·해제는 메모리 변경을 되돌리고 `-32603`(등록: "failed to save the persistent webhook; nothing was registered", 해제: "... is still registered")으로 답한다. 응답할 호출자가 없는 저장(만료 정리, 호출 때 남은 횟수 차감, 복원 뒤 정리)은 쓰지 않고 경고만 남긴다. 저장 포트 읽기는 경고 후 없는 것으로 다루고, 포트 저장·삭제는 오류로 끝난다.

### 선택적 인증 (가벼운 발신자 확인)

인증은 등록별 선택 사항이다. 없으면 URL에 도달한 누구나 작업을 촉발할 수 있다.
고정 공유 토큰을 확인하며 HMAC·서명 검증은 지원하지 않는다. TLS도 리스너가 직접 제공하지 않는다.

| 위치 | 입력 |
|------|------|
| QueryKey | 지정 query key |
| BearerHeader | Authorization: Bearer 토큰 |
| BodyField | JSON 점 구분 경로의 문자열 값 |
| HeaderKey | 지정 HTTP header |

비교는 `ct_eq`를 사용한다. 조회 응답은 위치·key 이름만 보여주고 토큰 값은 반환하지 않는다.
Persistent 토큰은 `webhooks.toml`에 평문으로 저장된다. Unix 파일 모드는 현재 atomic_write의 tempfile 생성 방식을 따른다.
저장 경로를 바꾸면 권한도 함께 확인해야 하며 이 응답 은닉을 저장 암호화로 해석하지 않는다.

### body 상한 (요청당)

JSON 입력은 기본 1MiB이며 `TASTY_WEBHOOK_MAX_BODY_BYTES`의 양수로 조정한다. 0·파싱 실패는 기본값이다.
유효 Content-Length가 상한을 넘으면 application body 읽기 전에 거절한다.
chunked·길이 미상은 상한+1 바이트를 읽어 초과를 구별하며 UTF-8 변환 전에 판정한다.
초과 요청은 시퀀스를 실행하지 않고 CountLimit도 차감하지 않는다.

413과 선차단 429는 vendor/tiny_http의 `Request::respond_and_close`를 쓴다.
Connection:close를 보내고 공유 종료 flag로 EqualReader Drop의 잔여 읽기와 길이 비례 allocation을 막는다.
큰 미완료 body에서는 상대의 body·FIN 없이 worker와 소켓 양방향을 정리한다.
거절 연결은 재사용하지 않는다. body가 전송 중이면 RST 때문에 client가 ACK를 받지 못할 수도 있다.

이 값은 application 입력 버퍼 상한이며 전체 메모리·동시 요청·연결·JSON 표현·IPC 치환 후 크기의 상한은 아니다.
Content-Length≤1024는 라이브러리가 요청 생성 전에 먼저 읽는다.
이 작은 body에서 client가 close 헤더를 무시하면 이미 다음 헤더를 기다리던 연결은 기존 idle keep-alive처럼 남을 수 있다.

vendor 업데이트는 응답 수신과 worker·수신 방향 종료를 별도로 확인한다.
`listener_body_tests.rs`의 raw_oversize·raw_blocked_source 검사와 정상 keep-alive·chunked·SSE를 함께 본다.
사본의 상류 보안 수정은 수동으로 추적하며 자동 advisory 검사가 모두 대조한다고 가정하지 않는다.
패치 범위는 [PATCHES.md](../../../vendor/tiny_http/PATCHES.md)에 있다.

### 남용차단 (일시 거부)

키는 소켓의 remote IP다. 포트는 제외하며 X-Forwarded-For를 출처로 그대로 사용하지 않는다.
같은 NAT·proxy 뒤 발신자는 실패 수와 cooldown을 공유할 수 있다.

- `counts_as_failure`는 401·404·405·413만 센다. 인증 실패는 이 제한에 포함되지만 등록의 CountLimit를 줄이지 않는다.
- 기본 10 초 고정 창에서 실패 20 회가 되면 60 초 cooldown으로 들어가 이후 같은 IP 요청을 429로 거절한다.
- `TASTY_WEBHOOK_ABUSE_THRESHOLD`, `TASTY_WEBHOOK_ABUSE_WINDOW_SECS`, `TASTY_WEBHOOK_ABUSE_COOLDOWN_SECS`로 양수를 지정한다.
  미설정·0·파싱 실패는 기본값이며 threshold는 u32 최댓값 이내로 제한한다.
- 차단 중 재시도는 종료 시각을 연장하지 않는다. 만료 확인 때 카운터와 창 시작을 초기화한다.
- 고정 창 경계에는 짧은 구간에 `2×threshold−1`회 실패가 처리될 수 있다.
  창마다 threshold 미만으로 반복하면 cooldown에 들어가지 않으므로 모든 패턴의 일정 처리율 상한이나 짧은 토큰의 안전을 보장하지 않는다.
- `PRUNE_TRIGGER_SOURCES=4096`은 표 크기 상한이 아니다. 기준을 넘으면 최대 창마다 한 번만 훑고
  cooldown 중이거나 창 안의 항목을 보존한다. 많은 출처로 표가 커질 수 있다.
  timer로 비우지 않고 다음 실패에서 정리 조건을 확인한다.
- 상태는 메모리에만 있으며 재시작하면 없어진다. Persistent webhook 등록과 별개다.

정상 200은 실패 수를 올리지 않지만 이미 같은 IP가 차단돼 있으면 정상 요청도 429를 받는다.
IPv6·proxy 출처 처리나 실제 메모리 제한을 추가할 때는 차단 항목을 밀어내 우회하지 못하도록 함께 설계한다.

### 리스너 주소와 포트

결정의 근거와 대안은 [ADR-0032](../../adr/0032-webhook-admission.md#리스너-주소와-포트-2026-10-07-개정)에 있다.

**주소**: 기본 `127.0.0.1`. config.toml의 `[webhook] allow_external = true`면 `0.0.0.0`(`webhook::bind_ip`). 다음 실행부터 적용된다.

**포트 결정 순서** (`webhook::bind`):

1. 실행 인자 `--webhook-port <N>`(GUI·headless 공통). 있으면 명시 지정이다.
2. 데이터 폴더 `webhooks.toml`의 `port`(`tasty webhook port <N>`으로 저장). 인자가 없을 때만 쓰며 명시 지정이다.
3. 둘 다 없으면 탐색: `28429`부터 하나씩 올리며 bind를 시도해 처음 성공한 포트를 쓴다. 상한은 64개(`28429..=28492`, `PROBE_COUNT`).

- 점유 여부는 미리 조회하지 않고 실제 bind 결과로 판단한다.
- **명시 지정 포트를 bind하지 못하면 실행을 막는다.** 이벤트 루프·메모리 저장소·저널을 만들기 전에 판단한다. GUI는 부팅 오류 화면(제목 "Webhook port in use", Quit)을 띄우고 종료 코드 1로 끝나며, headless는 같은 문구를 오류 로그로 남기고 종료 코드 1로 끝난다. 이때 데이터 폴더에는 저널 writer 잠금 파일만 남는다(측정).
- 탐색 범위가 모두 막히면 리스너 없이 실행하고 경고한다(`WebhookInitReport::Unavailable`).
- 탐색으로 `28429`가 아닌 포트를 얻었고 복원한 Persistent 웹훅이 있으면 경고한다(`MovedWithPersistent`). 새 URL은 `tasty webhook list`로 확인한다. 저장한 웹훅이 없으면 경고하지 않는다.
- 경고는 GUI toast(Warning, Window 범위)와 headless `tracing::warn!`이다.
- bind한 포트는 설정 파일에 써 넣지 않는다. 써 넣으면 다음 실행부터 명시 지정이 되기 때문이다.
- **Tasty 없이 바꾸기**: `tasty webhook port <N>`·`tasty webhook port --unset`(이전 이름 `tasty webhook config --port <N>`)은 실행 중인 인스턴스가 있으면 IPC `webhook.config`로 저장한다. 포트 파일(`tasty.port`)이 없거나 그 포트로의 연결이 거절되면(인스턴스 없음) CLI가 webhooks.toml을 직접 고치고 `{saved_port, applies_from: "next_start", instance_running: false, file, message}`를 출력한다. 연결 시간 초과 같은 다른 실패는 고치지 않고 오류로 끝난다. 연결은 됐지만 상대가 Tasty로 응답하지 않으면(JSON-RPC 오류가 아닌 실패: 연결 재설정, JSON이 아닌 응답) 다른 프로그램이 그 포트를 쥐었을 수 있으므로 파일을 고치지 않고, 원래 오류 아래에 그 가능성과 webhooks.toml의 `port` 줄을 직접 고치거나 남은 포트 파일을 지우는 방법을 안내한 뒤 종료 코드 1로 끝난다(`cli.webhook.port_not_tasty`). 파일 규칙(형식 표시, 이전 자동 기록 포트 정리, 다른 키 보존)은 `tasty_settings::webhook_port_file` 한 곳에 있고 인스턴스와 CLI가 같이 쓴다. 저장한 포트가 막혀 Tasty가 뜨지 않을 때 오류 안내가 이 명령을 알려 준다. 인자 없는 조회는 실행 중인 인스턴스가 있어야 한다.
- **이전 형식 파일**: 이전 버전은 파일이 없을 때 `port = 28429`를 자동으로 써 넣었다. `format = 2` 표시가 없는 파일의 `port = 28429`는 그 자동 값으로 보고 처음 읽을 때 지운다(탐색으로 바뀐다). 다른 값은 사용자가 정한 명시 지정으로 남긴다. 이 모듈이 파일을 쓸 때마다 `format = 2`를 함께 쓴다.
- 같은 컴퓨터에서 release·debug·격리 홈 인스턴스를 함께 띄우면 각 인스턴스가 비어 있는 다음 포트를 쓴다(측정: 다른 프로세스가 28429를 쥔 상태에서 격리 인스턴스가 28430).

**단일 실행 넘김**: release에서 같은 데이터 폴더의 Tasty가 이미 실행 중이고 두 번째 실행에 `--webhook-port`가 있으면, 실행 중인 쪽에 `webhook.config`로 포트를 묻는다. 다르면(리스너가 없는 경우 포함) 넘기지 않고 메시지 상자로 알린 뒤 종료 코드 1로 끝낸다. 같으면 평소처럼 넘긴다. 조회하지 못하면 막지 않고 평소처럼 넘기며, 확인하지 못했다는 경고를 `tracing::warn!`과 실행 기록(launch log)에 남긴다.

**값이 저장되는 파일**:

| 값 | 파일 | 바꾸는 방법 | 적용 |
|---|---|---|---|
| 외부 수신 허용 `allow_external` | config.toml `[webhook]` | Settings › General(Save/Cancel), `tasty webhook allow-external on\|off` | 다음 실행 |
| 명시 지정 포트 `port` | webhooks.toml 최상위 | `tasty webhook port <N>`·`--unset`(Tasty 없이도 가능) | 다음 실행 |
| 파일 형식 표시 `format` | webhooks.toml 최상위 | 포트를 쓸 때 자동 | — |
| Persistent 등록 `[[webhook]]` | webhooks.toml | `tasty webhook register --persistent`·`unregister` | 즉시 |
| 이번 실행만의 포트 | 저장 안 함 | `--webhook-port <N>` | 그 실행 |

외부 수신 허용은 설정 윈도우의 저장·취소 흐름을 따르도록 config.toml에, 포트는 인스턴스 없이도 고칠 수 있도록 웹훅 파일에 둔다.

### 포트 기록 (`RuntimePorts`)

이 인스턴스가 bind에 성공한 IPC 포트와 웹훅 포트를 `src/runtime_ports.rs`의 `RuntimePorts`가 한 번씩 기록한다. 포트가 필요한 곳(웹훅 URL, `webhook.config` 조회, 자기 자신 attach 거절, 인스턴스 파일)은 여기서 읽는다.

- `AppServices`가 GUI·headless 공통 앱 초기화에서 하나 만들어 소유한다. 프로세스 전역 static으로 두지 않아, 한 시험 프로세스가 여러 `AppServices`를 병렬로 만들어도 서로의 기록을 보지 않는다. 웹훅 레지스트리는 리스너 시작 때 같은 기록을 넘겨받는다.
- 기록은 bind 성공 뒤에만 한다. 각 항목은 주소와 정한 방법(`argument`·`config`·`probe`·`dynamic`)을 가진다.
- 데이터 폴더의 `tasty.port` 파일은 매니저가 아니라 IPC 서버가 bind 시점에 직접 쓴다(`src/adapters/production/tcp_ipc_server.rs`). 이전 완료 알림 로그(notify)를 지운 뒤에 주소를 공개하는 순서를 서버 안에서 지키기 위해서다. 값은 매니저에 기록한 것과 같은 bind 결과(`listener.local_addr()`)다.

### 부팅 초기화

GUI(`boot::run_gui`)와 headless(`boot::run_headless`)는 설정을 읽은 직후 `webhook::prepare(--webhook-port, &settings.webhook)`로 소켓을 선점하고 `Hub::webhook`에 둔다. IPC와 injector가 준비되면 `webhook::start`가 그 소켓으로 accept 스레드를 시작하고 포트를 기록한 뒤 `Persistent` 웹훅을 복원한다(GUI `src/app/boot_machine.rs`, headless `boot::start_ipc_and_seed`).

## 인터페이스

`webhook.register` 는 plugin 도 호출할 수 있고(`Network` 권한), 나머지는 **`local_only`** — CLI/로컬 클라이언트만 가능하다. 포커스 독립(대상은 `id` 로 지정, `list` 는 전 범위 순회).

| IPC | CLI | 동작 |
|-----|-----|------|
| `webhook.register` | `tasty webhook register` | 필요 메서드 + (`--handler <id>` xor `--sequence <json>`) + lifetime + 선택 인증 → `{id, url, ...}` 반환. `--method` 를 생략하면 CLI 는 `methods` 를 `null` 로 보내 서버 기본값 `POST`를 사용한다(빈 배열은 서버가 거절한다). `--sequence` 가 JSON 으로 안 읽히면 CLI 가 요청을 보내지 않고 인자 이름과 원인을 찍은 뒤 종료 코드 1 로 끝난다 |
| `webhook.list` | `tasty webhook list` | 전체 목록(각 항목 URL·메서드·steps·lifetime·인증여부) |
| `webhook.info` | `tasty webhook info --id <id>` | 단일 상세 |
| `webhook.unregister` | `tasty webhook unregister --id <id>` | 등록 해제(path 회수) |
| `webhook.sweep` | `tasty webhook sweep` | 만료 웹훅 일괄 정리 → 제거된 id 목록 |
| `webhook.config` | `tasty webhook port [<N> \| --unset]` · `tasty webhook allow-external [on\|off]` · `tasty webhook config [--port <N>]`(deprecated, 동작은 `webhook port`와 같고 출력의 `warnings`에 `deprecated: tasty webhook config, use tasty webhook port`를 싣는다) | 인자 없으면 실행 중인 리스너를 조회한다: `port`·`address`·`bound`·`source`(`argument`/`config`/`probe`)·`explicit`·`saved_port`(다음 실행 값)·`allow_external`(다음 실행 값). `port`로 저장, `unset_port: true`로 저장 값 삭제, `allow_external`로 외부 수신 설정 변경. 변경은 모두 다음 실행부터 적용된다(`applies_from: "next_start"`, `restart_required`) |

- **register 게이트**: `methods` 빈 배열 거부, `handler`/`sequence` 정확히 하나. `handler` 는 `validate_binding(handler, Webhook)` 로 검증 — 셸/hook-전용 핸들러는 거부([ADR-0027](../../adr/0027-lua-and-hook-execution.md)). 인라인 `sequence` 는 익명 핸들러(`user/wh-<slug>`)로 레지스트리에 등록된다.
- **lifetime 파라미터**: `--persistent`(bool), `--ttl-secs` xor `--count`(둘 다 없으면 `Unlimited`).
- **auth 파라미터**: `--auth-location <query|bearer|body|header>` + `--auth-token`(상호 requires), bearer 외에는 `--auth-key`.
- **핸들러**가 소비하는 페이로드→params 치환·source 게이트는 [공유 훅 핸들러 레지스트리(ADR-0027)](../../adr/0027-lua-and-hook-execution.md) 참조.
- **리스너 설정 GUI**: Settings › General 의 "Accept webhook calls from other computers" 스위치와 경고 callout이 `webhook.allow_external`을 바꾼다. 포트는 CLI 전용이다.
- **핸들러 레지스트리 GUI**: [Settings › Handler › Hook Handlers](../settings/screens/settings.md) 서브탭에서 레지스트리(host 기본 + plugin 기여 + user 매핑)를 조회·편집한다(토글/셸 명령 인라인 편집/user 행 추가·제거, `~/.tasty/hook-handlers.toml` 영속). **제거는 user 행만** — host/plugin 행은 그 자리에 자물쇠 글리프가 오고, 지워도 finalize 가 되살린다. host/plugin 행을 고치면(시퀀스·명령·Switch) 같은 id 의 user patch 로 저장되고, 그 행은 출처 Tag 뒤에 "edited" Tag 와 Revert 를 단다. Revert 는 Save 때 patch 를 지워(`tasty hook-handler remove --id` 와 같은 효과) 기본값으로 돌아가며, Save 전에는 Undo 와 "reverts on save" Tag 로 대기한다. 대기 중에는 Switch 가 기본값을 보이며 잠기고(툴팁 "Reverts on save. Undo to change it.") `Edit` 도 Undo 나 Save 전까지 비활성이다. `IpcSequence` 행은 mono 한 줄 요약과 `Edit`(그 자리 문자열 편집기)이고, 한 줄 형식으로 쓸 수 없는 시퀀스는 [`tasty hook-handler get`/`upsert`](../hooks/index.md#핸들러-레지스트리-hook_handler) 로 고친다(TOML 손편집 + `reload` 도 그대로 된다). **고쳐도 이미 등록된 웹훅은 안 바뀐다** — 엔트리가 등록 시점 스냅샷을 소유하므로 다시 등록해야 한다. **리스너(bind/port/secret) 설정은 이 서브탭에 없다** — 외부 수신 허용은 Settings › General, 포트는 CLI(`webhook.config`).

## 비-목표 (Out of scope)

- **HTTPS/TLS 종단** — 리버스 프록시/공유기에 위임(사용자 요구가 "포워딩은 OS/공유기 몫").
- **외부 발신자의 조회/응답 채널** — 응답은 ACK 전용. 내부 상태 조회는 로컬 소유자 채널(`list`/`info`)로만.
- **웹훅에서의 OS 셸 실행** — 셸(`ShellCommand`)은 기존 훅(source `hook`) 전용, 웹훅 바인딩 불가. 셸 핸들러가 훅 트리거/수동으로 실행될 때 받는 `TASTY_HOOK_*` env 목록은 [hooks 문서의 셸 핸들러 환경변수 절](../hooks/index.md#셸-핸들러-환경변수-tasty_hook_) 참조.
- **plugin 의 웹훅 관리** — 현재 plugin 은 `webhook.register` 만 호출할 수 있다(나머지는 local-only).
- **plugin 프로세스의 직접 소켓 소유** — 코어가 소켓을 소유한다.
- **웹훅 외 프로토콜(raw TCP 등)** — HTTP 웹훅으로 확정.

## Acceptance Criteria

- Given 포트 설정됨 When `webhook.register --method POST --sequence '<ipc-seq>'` Then 발급 URL 반환, `curl -XPOST` 시 IpcSequence 가 실행되고 응답은 고정 ACK 바디만.
- Given 등록된 웹훅 When `webhook.unregister` 후 그 path 호출 Then `404`.
- Given `CountLimit{remaining:N}` When 접수되는 호출을 N+1 회 Then N 회 후 소멸(다음 호출 `410`/`404`).
- Given `CountLimit` + 인증 설정된 웹훅 When 토큰 불일치 호출 Then `401` 이고 **remaining 은 그대로**.
- Given `TimeLimit` deadline 경과 When 호출 또는 `webhook.sweep` Then `410` + 삭제.
- Given 인증 설정된 웹훅 When 토큰 불일치 Then `401`; 미설정 웹훅은 무인증 통과.
- Given 없는 path 를 임계치 초과 반복 When 같은 출처 재요청 Then 쿨다운 동안 `429`(다른 출처의 정상 웹훅은 계속 처리).
- Given 인증 설정된 웹훅 When 같은 출처가 틀린 토큰을 임계치 초과 반복 Then 쿨다운 동안 `429`.
- Given `ShellCommand` 핸들러 When 웹훅 바인딩 시도 Then source 게이트로 거부.
- Given 다른 프로세스가 28429를 점유 When 인자 없이 실행 Then 28430에 bind하고 `tasty webhook port`가 `port: 28430`, `source: "probe"`를 돌려준다.
- Given 28429 점유 When `--webhook-port 28429`로 실행 Then 오류 메시지와 함께 종료 코드 1로 끝난다(GUI는 오류 화면의 Quit 뒤).
- Given 기본 설정 Then 리스너는 `127.0.0.1`에만 있다. `allow_external = true`로 다시 실행하면 `0.0.0.0`.
- Given 두 인스턴스(다른 데이터 폴더)를 함께 실행 Then 둘 다 서로 다른 포트에 리스너를 띄운다.
- Given 포트가 탐색으로 밀렸고 Persistent 웹훅이 있음 When 실행 Then 경고가 뜨고 `tasty webhook list`의 URL이 새 포트를 쓴다.
- Given release 인스턴스가 웹훅 포트 P로 실행 중 When 같은 데이터 폴더에서 `--webhook-port Q`(Q≠P)로 다시 실행 Then 메시지 상자 뒤 종료 코드 1, Q=P면 평소처럼 넘긴다.

## 관련

- [hooks](../hooks/index.md) — 내부 이벤트 트리거(웹훅과 대칭인 trigger 출처) · [notifications](../notifications/index.md) · [file-handler](../file-handler/index.md)(레지스트리 정본 템플릿)
- [API](../../reference/api.md#기타-호스트) · [웹훅 요청 처리와 제한](../../adr/0032-webhook-admission.md) · [Lua와 훅 실행](../../adr/0027-lua-and-hook-execution.md)
