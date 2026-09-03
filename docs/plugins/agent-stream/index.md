# Agent Stream (`com.tasty.agent-stream`)

- **Status**: Implemented (수집 파이프라인 + SSE 방출) — 인바운드 웹훅 배선은 미구현
- **주체**: AI Agent (CLI/IPC). 로컬 사용자 UI 없음 — headless
- **배포/통합**: workspace 번들(`BUILTINS` 등록) · CLI + IPC namespace — [plugins 개념](../../concepts/plugins.md)
  - `bundle = false` — 배포 패키징(DMG / AppImage / MSIX / deb)에서는 제외한다. 워크스페이스 빌드의 dev 번들 sync 는 그대로 동작한다.
- **코드**: `crates/tasty-plugin-agent-stream/`
- **권한**: `surface.read`(세션 id meta 조회 · 대상 생존 확인) · `fs.read`(transcript 읽기) · `fs.write`(data_dir 의 watch 스냅샷 쓰기) · `network`(SSE 엔드포인트 bind)
- **화면**: 없음
- **근거**: [ADR-0093](../../adr/0093-agent-response-relay-reads-transcript-jsonl.md)(수집) · [ADR-0100](../../adr/0100-agent-stream-sse-endpoint-exposure.md)(SSE 노출)

> **예제로서**: **상주 background 스레드 + plugin 자체 HTTP 서버 + CLI/IPC namespace** 예제 — SDK 가 async 를 지원하지 않는 조건에서 파일 tail 루프를 얹는 최소 형태 → [plugin-development](../../dev-guide/plugin-development.md#cli--ipc-namespace) · [§10 한계](../../dev-guide/plugin-development.md#10-한계-현재-sdk).

## 목적

surface 에서 도는 AI 코딩 에이전트의 응답을 **구조화 이벤트로 수집**한다. 화면 스크레이핑(`tasty read screen` · `output-match` 훅)과 달리 ANSI·박스 문자·줄바꿈이 섞이지 않고, 사고 블록과 응답 텍스트가 소스에서부터 분리돼 있다.

이름이 claude 전용이 아닌 이유는 codex 등 다른 에이전트도 transcript 위치만 다르고 tail·정규화·전송이 같기 때문이다. **현재 해석되는 소스는 Claude Code 하나다.**

## 내부 동작

### 대상 해석 — surface_id → 세션 id → transcript

1. claude plugin 의 `SessionStart` 훅이 세션 id 를 surface meta `claude-session-id` 로 기록한다.
2. 이 plugin 이 `surface.meta.get` 으로 그 값을 읽는다. **meta 가 없으면 watch 를 거부한다** — 어떤 파일을 볼지 결정할 수 없는데 등록을 받아주면 호출자는 스트림이 붙었다고 믿은 채 아무 것도 받지 못한다.
3. 세션 id 가 그대로 파일명이다. transcript 루트(`$CLAUDE_CONFIG_DIR/projects`, 미설정 시 `~/.claude/projects`) **한 겹 아래를 훑어** `<session-id>.jsonl` 을 찾는다 — project-slug 규칙을 계산하지 않는다(ADR-0093).

claude plugin 에 대한 **코드 의존은 없다**. 접점은 host IPC 로 읽는 surface meta 키 하나뿐이다.

### tail 루프

호스트 healthcheck(15s ping / 60s 무응답 시 강제 재시작)를 깨지 않기 위해 **전용 스레드**에서 돈다. 두 가지 주기를 가진다.

| 주기 | 하는 일 |
|------|---------|
| 300ms | 파일을 offset 부터 읽어 완성된 라인을 이벤트로 변환. host IPC 호출 없음 |
| 3s (10 tick) | `surface.locate` 로 대상 생존 확인, `surface.meta.get` 으로 세션 교체 확인 |

파일 상태 이상은 전부 다룬다.

| 상태 | 판정 | 대응 |
|------|------|------|
| 아직 생성 안 됨(세션 시작 직후 race) | `metadata` 가 `NotFound` | `awaiting_transcript` 로 대기, 매 tick 경로 재해석 |
| 읽는 중 삭제 | 위와 동일 | 재생성되면 처음부터 다시 읽는다 |
| 중간 truncate | `len < offset` | 0 부터 재동기화 |
| rotate / 파일 교체 | inode(Unix) · file index(Windows) 변화 | 0 부터 재동기화 |
| 개행 없이 끝난 부분 라인 | 버퍼 잔여 | 다음 read 로 완성될 때까지 보류 — 완성 시 **정확히 1 회** 방출 |

재동기화가 같은 레코드를 다시 읽어도 레코드 `uuid` 중복 제거(최근 4096 개 기억)가 흡수한다. `uuid` 가 없는 레코드는 같음을 주장할 근거가 없어 그대로 통과시킨다.

### 이벤트 모델

`assistant` 레코드의 `message.content[]` 블록과 턴 종료만 이벤트가 된다.

| kind | 출처 | 실린 필드 |
|------|------|-----------|
| `text` | `content[].type == "text"` | `text` |
| `thinking` | `content[].type == "thinking"` (본문 키는 `thinking`) | `text` |
| `tool_use` | `content[].type == "tool_use"` | `tool_name` · `tool_input` |
| `turn_end` | 아래 표 | `reason` |

모든 이벤트에 `seq`(전역 단조 증가) · `surface_id` · `session_id` 가 붙고, 파일에서 온 이벤트는 `record_uuid` · `timestamp` 도 함께 싣는다.

> **`thinking` 의 본문은 비어 있을 수 있다.** Claude Code 버전/설정에 따라 transcript 의 `thinking` 블록이 `signature` 만 남기고 본문을 비운 채 기록된다(실측). 이 경우 `thinking` 이벤트의 `text` 가 빈 문자열이다 — 소스에 없는 것을 지어내지 않고 그대로 중계한다. kind 분리 자체는 유효하므로 소비자는 여전히 사고 블록을 골라 버릴 수 있다.

**턴 종료 사유** — 정상 완료만 다루면 소비자가 영원히 대기하는 상태가 생기므로 비정상 경로를 모두 포함한다.

`reason` 은 **출처를 접두로 구분한다.** `stop:` 은 transcript 의 `stop_reason` 원문을 그대로 옮긴 값이고, `stream:` 은 이 파이프라인이 판정해 만든 예약 사유다. 접두가 없으면 외부 스펙이 언젠가 `session_ended` 같은 문자열을 `stop_reason` 으로 쓰기 시작했을 때 소비자가 둘을 구분할 수 없다. 소비자는 `reason.starts_with("stream:")` 으로 예약 사유를 가른다.

| `reason` | 언제 |
|----------|------|
| `stop:end_turn` / `stop:max_tokens` / … | `assistant.message.stop_reason` 에 `stop:` 을 붙인 것 (단 `tool_use` 는 턴 종료가 아니다 — 툴 결과를 받아 계속된다) |
| `stream:api_error` | `isApiErrorMessage: true` — API 오류 응답은 `stop_reason` 이 평범한 값으로 오므로 이쪽을 먼저 본다 |
| `stream:cancelled` | `user` 레코드에 `[Request interrupted by user…]` 마커 |
| `stream:session_ended` | 대상 surface 가 사라졌거나, 같은 surface 에서 새 세션이 시작돼 이전 세션이 닫혔다 |
| `stream:unwatched` | `agent_stream.unwatch` 호출 |
| `stream:rewatched` | 같은 surface 를 다시 `watch` 해 이전 등록이 교체됐다 |

사용자 프롬프트 본문 · 툴 결과 · 첨부 · 모드 전환 등 나머지 레코드는 **중계 대상이 아니다**(비-목표).

### 세션 전환

verify tick 에서 surface meta 의 세션 id 가 바뀐 것을 발견하면, 이전 세션에 `turn_end{reason=stream:session_ended}` 를 남기고 tail 대상을 새 파일로 교체한다. 새 세션 파일은 **처음부터** 읽는다(그 세션의 전부가 대상이다). 새 파일이 아직 없으면 경로 미해결 상태로 두고 매 tick 재해석한다.

### 재시작 복구 — at-least-once

watch 대상과 byte offset 을 `TASTY_PLUGIN_DATA_DIR/watches.json` 에 남기고, plugin 재시작 시 **저장된 offset 그대로** 재개한다. 쓰기는 같은 디렉토리의 `.json.tmp` 에 쓴 뒤 rename 하는 원자적 교체다 — 저장 도중 죽어도 반쯤 쓰인 스냅샷이 남지 않는다. 마지막 flush 이후의 레코드가 다시 읽힐 수 있다 — **누락보다 중복을 택한 결정**이다(ADR-0093). 소비자는 `record_uuid` 로 중복을 접는다.

`seq` 커서도 같은 스냅샷에 남긴다. 재시작마다 1 부터 다시 세면 `after_seq` 를 들고 있던 소비자가 재시작 후 처음 N 개 이벤트를 조용히 못 받는다 — 중복은 허용해도 침묵하는 누락은 허용하지 않는다. 다만 **버퍼 내용 자체는 메모리에만 있어 재시작으로 사라진다**(보존되는 것은 커서의 의미뿐).

`TASTY_PLUGIN_DATA_DIR` 이 주입되지 않은 비정상 기동에서는 영속화를 **건너뛴다**(조용히 다른 경로에 쓰지 않는다).

## 인터페이스

- **AI Agent**: `tasty agent-stream …` CLI 와 `agent_stream.*` IPC 양면. GUI 전용 경로 없음.
- **로컬 사용자**: 없음(headless).

### CLI / IPC

| CLI | IPC 메서드 | 설명 |
|-----|-----------|------|
| `tasty agent-stream watch [--surface N] [--from-start]` | `agent_stream.watch` | tail 시작. `--surface` 미지정 시 `TASTY_SURFACE_ID`. 기본은 현재 파일 끝부터 — `--from-start` 면 처음부터 |
| `tasty agent-stream unwatch [--surface N]` | `agent_stream.unwatch` | tail 중지 + `turn_end{reason=stream:unwatched}` |
| `tasty agent-stream list` | `agent_stream.list` | 전 대상 조회(포커스 무관). `status` 는 `tailing` / `awaiting_transcript` |
| `tasty agent-stream poll [--surface N] [--after-seq S] [--limit L]` | `agent_stream.poll` | seq 커서 기반 **비파괴** 읽기. 여러 소비자가 각자 커서로 같은 버퍼를 읽는다 |
| `tasty agent-stream serve --port N [--bind ADDR] [--token T]` | `agent_stream.serve` | SSE 엔드포인트를 연다. 이미 떠 있으면 끄고 새 설정으로 다시 연다(`replaced: true`) |
| `tasty agent-stream serve-stop` | `agent_stream.serve_stop` | 엔드포인트를 닫고 열린 구독을 전부 끊는다 |
| `tasty agent-stream serve-info` | `agent_stream.serve_info` | 엔드포인트 상태 + 구독자별 카운터. **토큰은 싣지 않는다** |

`watch` 는 **surface_id 를 명시적으로 지정**하는 것만 지원한다 — "전부 watch" 와일드카드가 없다(ADR-0093 결정 3). 같은 surface 를 다시 `watch` 하면 이전 등록을 교체하고(`replaced: true`) 그 등록에 `turn_end{reason=stream:rewatched}` 를 남긴다.

**`--from-start` 의 예외** — `--from-start` 없이 등록했더라도, 등록 시점에 transcript 파일이 아직 없었다면(`awaiting_transcript`) 나중에 파일을 찾은 순간 **처음부터** 읽는다. "현재 파일 끝부터"의 기준이 될 파일이 애초에 없었으므로 그 세션의 전부가 대상이다. 세션 전환으로 파일이 바뀌는 경우도 같다.

`poll` 의 `--surface` 는 파라미터 이름이 `filter_surface` 다. CLI 계층이 `surface` 라는 이름의 u32 인자를 `TASTY_SURFACE_ID` 로 자동 채우기 때문에, 그대로 두면 지정하지 않았는데도 호출자 자신의 surface 로 조용히 좁혀진다.

수집 버퍼는 4096 개 상한의 링이다. 넘치면 오래된 것부터 버리고 `poll` 응답의 `dropped` 로 알린다.

## SSE 엔드포인트

수집한 이벤트를 외부 소비자(FE 서버 등)가 구독해 받아가는 채널이다. plugin 이 **자기 프로세스에서** HTTP 서버를 연다 — 본체 웹훅 리스너는 인바운드 전용이라 재사용할 수 없다(응답에 내부 데이터가 실릴 코드 경로가 없다). 근거·대안·재검토 조건은 [ADR-0100](../../adr/0100-agent-stream-sse-endpoint-exposure.md).

```bash
tasty agent-stream serve --port 8787                 # loopback, 무인증
tasty agent-stream serve --port 8787 --bind 0.0.0.0 --token s3cret
curl -N http://127.0.0.1:8787/events
```

### 계약

| 항목 | 값 |
|------|-----|
| 경로 | `GET /events` (그 밖의 경로는 404, 다른 메서드는 405) |
| 응답 | `200` · `Content-Type: text/event-stream` · `Cache-Control: no-cache` · `X-Accel-Buffering: no` · `Content-Length` 없음 |
| 프레임 | `id: <seq>` / `event: <kind>` / `data: <JSON>` + 빈 줄. `data` 는 개행마다 `data:` 를 다시 붙인다 |
| 첫 바디 | `retry: 3000` — 끊김이 정상 경로이므로 재접속 간격을 명시한다 |
| keep-alive | 15s 유휴마다 `: keep-alive` 주석 줄 |
| 종료 | plugin 이 죽거나 `serve-stop` 하면 연결이 닫힌다. 소비자는 **재구독 전제**로 만든다 |

`event` 는 `text` / `thinking` / `tool_use` / `turn_end` 네 kind 와, 재개 시에만 나가는 `gap` 하나다. 네 kind 의 `data` JSON 은 **`poll` 응답의 이벤트 객체와 완전히 같은 스키마**다(`kind` · `seq` · `surface_id` · `session_id` · `timestamp` · `record_uuid` + kind 별 필드). 두 채널이 같은 직렬화 함수를 쓰므로 소비자는 파서를 하나만 들면 된다. `gap` 은 수집 이벤트가 아니라 **재전송 불가 구간 통지**이고 `data` 는 `{"kind":"gap","from":<seq>,"to":<seq>}` 다(아래 재개 절).

### 구독 파라미터

| 파라미터 | 기본 | 의미 |
|----------|------|------|
| `?surface=<id>` | 전체 | 그 surface 의 이벤트만 받는다 |
| `?thinking=1` | **꺼짐** | 사고 블록(`thinking`) 포함. 응답 텍스트와 민감도가 달라 기본은 제외한다 |
| `?after_seq=<n>` | 없음 | 재개 커서(아래) |
| `?token=<t>` | — | 구독 토큰(헤더 대신 쓸 때) |

### 인증

토큰이 설정돼 있으면 `Authorization: Bearer <t>` 또는 `?token=<t>` 로 제시한다(**상수시간 비교**). 미제시·불일치는 `401` 이고 **바디는 비어 있다** — 거부 응답에 내부 상태를 싣지 않는다. 쿼리 경로를 함께 두는 이유는 브라우저 `EventSource` 가 커스텀 헤더를 붙이지 못하기 때문이다.

**bind 정책**: 기본은 `127.0.0.1`. loopback 이 아닌 주소는 `--token` 없이는 **거부한다** — 이 엔드포인트로 나가는 것은 대화 전문이라 "실수로 `0.0.0.0` 무인증" 조합을 설정 검증에서 구조적으로 막는다. HTTP 레이어(`tiny_http`)에는 헤더 크기 상한도 읽기 타임아웃도 없어 **광역 bind 에서는 연결당 메모리·스레드 상한이 없다** — 루프백 밖으로 열 때는 앞단에 리버스 프록시를 두고 상한·타임아웃·TLS 를 거기서 거는 것을 권장한다.

**포트 정책**: `--port` 는 필수이고 **자동 폴백이 없다**(본체 웹훅 리스너와 같은 정책). bind 실패는 그대로 에러로 올라온다 — 임의 포트로 옮겨 뜨면 소비자가 붙을 고정 주소가 없어진다.

### 재개 (`Last-Event-ID`)

SSE 의 `id` 는 수집 파이프라인의 전역 단조 증가 `seq` 이고, 그 값이 곧 `poll` 의 `after_seq` 커서다. 재접속 시 `Last-Event-ID: <seq>` 헤더(또는 `?after_seq=<seq>`)를 주면 **그 뒤의 이벤트부터** 재전송한다.

- 재전송 원본은 **수집 버퍼 그대로**(4096 개 상한)다. 별도 재개 버퍼를 두지 않는다 — 두 버퍼의 상한이 다르면 "`poll` 로는 보이는데 SSE 로는 안 보이는" 불일치가 생긴다.
- 커서를 주지 않은 구독은 **재전송 없이 지금부터** 흘린다.
- 커서가 버퍼에서 이미 밀려난 경우 그 구간은 복구되지 않는다. `poll` 응답의 `dropped` 와 같은 한계다. 다만 **조용히 건너뛰지는 않는다** — 재전송에 앞서 `gap` 이벤트로 잃어버린 구간을 먼저 알린다.

  ```
  id: 0
  event: gap
  data: {"kind":"gap","from":1,"to":5}
  ```

  `id` 는 소비자가 보낸 커서 그대로다 — 갭 통지가 커서를 전진시키면 그 뒤 재연결에서 남은 이벤트까지 건너뛴다.
- **plugin 재시작을 건너뛴 재개는 되지 않는다.** `seq` 커서 자체는 스냅샷으로 보존되지만 버퍼 내용은 메모리에만 있다. 재시작 후의 재개 요청은 그 시점 이후의 이벤트만 받는다 — 이 구간의 누락은 tail 쪽 at-least-once 재개([ADR-0093](../../adr/0093-agent-response-relay-reads-transcript-jsonl.md))가 파일에서 다시 읽어 메운다. 이 경우도 조용하지 않다 — 커서가 **재시작 시점의 마지막 `seq` 보다 뒤에 있으면** 그 사이 구간을 알리는 **`gap` 프레임이 먼저 나가고**, 이어서 tail 이 파일에서 다시 읽은 내용을 **새 `seq`** 로 흘린다. 같은 내용이 두 번 오는 구간은 소비자가 `record_uuid` 로 접는다. 재시작 직전까지 다 받은 소비자(커서 = 마지막 `seq`)에게는 놓친 구간이 없으므로 `gap` 이 나가지 않는다.

### 백압 · 느린 구독자

구독자마다 **256 슬롯 bounded 큐**를 둔다. 생산자(tail 스레드)는 `try_send` 만 쓰고 **절대 블로킹하지 않는다** — 느린 구독자가 수집 자체를 멈추면 안 되기 때문이다. 가득 차면 버리고 `serve-info` 의 `dropped` 카운터를 올린다.

**연속 64 회 버려진 구독은 허브에서 끊는다.** 버리기만 하고 연결을 유지하면 소비자는 "연결은 살아 있는데 구멍 난 스트림" 을 받아 무엇을 놓쳤는지 알 수 없다. 끊으면 소비자가 재접속하고 `Last-Event-ID` 로 이어붙이므로 누락이 복구 가능한 형태로 드러난다.

끊기는 것은 **허브의 구독 등록**이고, 소켓은 진행 중인 write 가 끝나거나 실패할 때 닫힌다. 즉 TCP 송신 버퍼까지 채운 소비자에게는 커널이 그 write 를 포기할 때까지 연결 스레드 1 개가 남을 수 있다.

### 재시작 후 자동 복구

`serve` 설정(bind/port/token)은 watch 스냅샷(`<data_dir>/watches.json`)에 함께 남고, 강제 재시작 후 자동으로 다시 bind 한다. 되살아나지 않으면 watch 는 복구됐는데 붙을 곳이 없어 "재구독으로 복구" 전제가 무너진다. 재기동 bind 가 실패하면 경고만 남기고 수집은 계속된다(`poll` 로는 계속 읽을 수 있다).

**`serve` 재호출이 bind 단계에서 실패하면 엔드포인트는 닫힘으로 확정된다** — 옛 리스너는 이미 내려간 상태이고 영속된 `serve` 설정도 함께 지워지므로, 재시작해도 옛 주소로 되살아나지 않는다. 다시 열려면 `serve` 를 명시적으로 다시 호출한다. 실패한 요청이 "닫힌 줄 알았던 엔드포인트" 를 나중에 되살리는 쪽이 대화 전문 채널에서는 더 위험하기 때문이다([ADR-0100](../../adr/0100-agent-stream-sse-endpoint-exposure.md) 결정 1). **인자 검증에서 거부된 재호출은 여기 해당하지 않는다** — `--port` 누락·범위 밖·IP 로 해석되지 않는 bind·광역 bind 무토큰은 옛 서버를 건드리기 전에 반환하므로, 떠 있던 엔드포인트와 스냅샷이 그대로 유지된다. 잘못된 인자 하나가 멀쩡히 돌던 스트림을 내리지는 않는다.

> **토큰은 그 스냅샷 파일에 평문으로 남는다.** 본체 웹훅 토큰과 같은 신뢰 수준·같은 저장 방식이다(설정 파일 평문). unix 에서는 스냅샷 파일을 `0600` 으로 만들어 같은 머신의 다른 사용자에게 열리지 않게 한다(Windows 는 파일 ACL 기본값을 따른다).

## 비-목표

- **턴 correlation** — 이벤트를 턴 단위로 묶어주는 상위 구조는 만들지 않는다. 소비자가 `turn_end` 로 경계를 잡는다.
- **인바운드 웹훅 배선** — 외부 요청으로 에이전트를 실행시키는 경로는 별개다. 그 방향의 host 리스너는 [webhook](../../features/webhook/index.md).
- **사용자 프롬프트 · 툴 결과 중계** — 에이전트가 낸 것만 이벤트로 만든다.
- **codex transcript** — 이름은 담고 있으나 현재 해석되는 소스는 Claude Code 하나다.
- **transcript 쓰기/변경** — 읽기 전용이다.

## Acceptance Criteria

- Given 에이전트가 도는 surface Then `agent-stream watch` 가 세션 id 를 해석하고 transcript 경로를 돌려준다.
- Given watch 중인 surface 에서 에이전트가 응답 Then 수 초 내에 그 텍스트가 `text` 이벤트로 `poll` 에 나타난다.
- Given 사고 블록이 있는 응답 Then `thinking` 과 `text` 가 서로 다른 kind 로 나온다.
- Given `claude-session-id` meta 가 없는 surface Then `watch` 가 명확한 에러로 거부된다(조용한 무동작 없음).
- Given 턴이 오류/취소/세션 종료로 끝남 Then 그에 맞는 `reason` 의 `turn_end` 가 나온다.
- Given plugin `disable && enable` Then 저장된 offset 에서 tail 이 재개된다(중복 허용).
- Given `serve` 로 연 엔드포인트 Then `curl -N` 이 연결을 유지한 채 응답 이벤트를 순차 출력한다.
- Given 토큰을 설정한 엔드포인트 Then 토큰 없는/틀린 구독이 401 로 거부되고 바디가 비어 있다.
- Given `?thinking=1` 유무 Then 사고 블록이 구독별로 포함/제외된다.
- Given 수집 버퍼 밖으로 밀려난 재개 커서 Then 재전송보다 **먼저** `gap` 이벤트가 나오고 그 `from`/`to` 가 잃어버린 구간을 가리킨다.
- Given 느린 구독자 Then tail 파이프라인이 멈추지 않고 `serve-info` 의 `dropped` 만 오른다.
- Given 구독 중 plugin `disable && enable` Then 연결이 끊기고 엔드포인트가 자동으로 다시 열려 재구독이 성공한다.

## 관련

- [ADR-0093](../../adr/0093-agent-response-relay-reads-transcript-jsonl.md) — 소스·전달 보장·대상 지정 방식의 근거
- [ADR-0100](../../adr/0100-agent-stream-sse-endpoint-exposure.md) — SSE 엔드포인트 노출 정책의 근거
- [ADR-0048](../../adr/0048-webhook-http-tiny-http-blocking.md) — 같은 HTTP 레이어(`tiny_http`)를 고른 근거
- [claude](../claude/index.md) — `claude-session-id` surface meta 를 기록하는 쪽
- [dev-guide/plugin-development](../../dev-guide/plugin-development.md) — §9.1 반영 절차 · §10 한계
- [features/terminal-output](../../features/terminal-output/index.md) — 화면 기반 출력 구조화(다른 소스)
