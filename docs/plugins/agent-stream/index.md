# Agent Stream (`com.tasty.agent-stream`)

- **Status**: Implemented (수집 파이프라인) — 외부 방출(SSE)·인바운드 웹훅 배선은 미구현
- **주체**: AI Agent (CLI/IPC). 로컬 사용자 UI 없음 — headless
- **배포/통합**: workspace 번들(`BUILTINS` 등록) · CLI + IPC namespace — [plugins 개념](../../concepts/plugins.md)
  - `bundle = false` — 배포 패키징(DMG / AppImage / MSIX / deb)에서는 제외한다. 워크스페이스 빌드의 dev 번들 sync 는 그대로 동작한다.
- **코드**: `crates/tasty-plugin-agent-stream/`
- **권한**: `surface.read`(세션 id meta 조회 · 대상 생존 확인) · `fs.read`(transcript 읽기) · `fs.write`(data_dir 의 watch 스냅샷 쓰기)
- **화면**: 없음
- **근거**: [ADR-0093](../../adr/0093-agent-response-relay-reads-transcript-jsonl.md)

> **예제로서**: **상주 background 스레드 + CLI/IPC namespace** 예제 — SDK 가 async 를 지원하지 않는 조건에서 파일 tail 루프를 얹는 최소 형태 → [plugin-development](../../dev-guide/plugin-development.md#cli--ipc-namespace) · [§10 한계](../../dev-guide/plugin-development.md#10-한계-현재-sdk).

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

`watch` 는 **surface_id 를 명시적으로 지정**하는 것만 지원한다 — "전부 watch" 와일드카드가 없다(ADR-0093 결정 3). 같은 surface 를 다시 `watch` 하면 이전 등록을 교체하고(`replaced: true`) 그 등록에 `turn_end{reason=stream:rewatched}` 를 남긴다.

**`--from-start` 의 예외** — `--from-start` 없이 등록했더라도, 등록 시점에 transcript 파일이 아직 없었다면(`awaiting_transcript`) 나중에 파일을 찾은 순간 **처음부터** 읽는다. "현재 파일 끝부터"의 기준이 될 파일이 애초에 없었으므로 그 세션의 전부가 대상이다. 세션 전환으로 파일이 바뀌는 경우도 같다.

`poll` 의 `--surface` 는 파라미터 이름이 `filter_surface` 다. CLI 계층이 `surface` 라는 이름의 u32 인자를 `TASTY_SURFACE_ID` 로 자동 채우기 때문에, 그대로 두면 지정하지 않았는데도 호출자 자신의 surface 로 조용히 좁혀진다.

수집 버퍼는 4096 개 상한의 링이다. 넘치면 오래된 것부터 버리고 `poll` 응답의 `dropped` 로 알린다.

## 비-목표

- **외부 방출(SSE)** — 이 plugin 은 수집까지만 한다. 방출 채널은 별개다.
- **인바운드 웹훅 배선** — 외부 요청으로 에이전트를 실행시키는 경로는 별개다. 그 방향의 host 리스너는 [webhook](../../features/webhook/index.md).
- **사용자 프롬프트 · 툴 결과 중계** — 에이전트가 낸 것만 이벤트로 만든다.
- **codex transcript** — 이름은 담고 있으나 현재 해석되는 소스는 Claude Code 하나다.
- **transcript 쓰기/변경** — 읽기 전용이다.

## Acceptance Criteria

- [ ] Given 에이전트가 도는 surface Then `agent-stream watch` 가 세션 id 를 해석하고 transcript 경로를 돌려준다.
- [ ] Given watch 중인 surface 에서 에이전트가 응답 Then 수 초 내에 그 텍스트가 `text` 이벤트로 `poll` 에 나타난다.
- [ ] Given 사고 블록이 있는 응답 Then `thinking` 과 `text` 가 서로 다른 kind 로 나온다.
- [ ] Given `claude-session-id` meta 가 없는 surface Then `watch` 가 명확한 에러로 거부된다(조용한 무동작 없음).
- [ ] Given 턴이 오류/취소/세션 종료로 끝남 Then 그에 맞는 `reason` 의 `turn_end` 가 나온다.
- [ ] Given plugin `disable && enable` Then 저장된 offset 에서 tail 이 재개된다(중복 허용).

## 관련

- [ADR-0093](../../adr/0093-agent-response-relay-reads-transcript-jsonl.md) — 소스·전달 보장·대상 지정 방식의 근거
- [claude](../claude/index.md) — `claude-session-id` surface meta 를 기록하는 쪽
- [dev-guide/plugin-development](../../dev-guide/plugin-development.md) — §9.1 반영 절차 · §10 한계
- [features/terminal-output](../../features/terminal-output/index.md) — 화면 기반 출력 구조화(다른 소스)
