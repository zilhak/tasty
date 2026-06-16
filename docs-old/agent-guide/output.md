# 터미널 출력 구조화

`tasty-output` 크레이트와 `output.*` namespace 는 터미널 출력을 **의미 단위**로 분해해 에이전트가 다루기 쉬운 JSON 으로 제공한다. 세 가지 진입점이 있다:

| 진입점 | 호출 패턴 | 용도 |
|--------|-----------|------|
| `surface.parse_since_mark` | 일회성 batch | 마크 이후 출력을 **한 번에** 분해해 응답으로 받기 |
| `surface.commands` (+ `last_command`, `command_at`) | 일회성 batch | OSC 133 으로 인덱싱된 **명령 단위** 메타데이터 조회 |
| `output.observe_start` | 스트리밍 | PTY 라인이 들어올 때마다 파서 → sink fan-out (`memory`/`file`) |

세 경로 모두 같은 파서 카탈로그를 공유한다 (`docs/agent-guide/output-parsers.md`).

## 한 번에 분해 — `parse_since_mark`

마크를 찍고 명령을 실행한 뒤, 누적된 출력을 파서로 분해한다.

```
tasty set mark --surface S
tasty send text "cargo test\n" --surface S
# ... 종료 대기 ...
tasty read parse-since-mark --surface S --parsers path,url,compile_error,test_result
```

응답:
```json
{
  "surface_id": "S",
  "parsers": ["path", "url", "compile_error", "test_result"],
  "items": [
    { "kind": "compile_error", "line": 12, "data": { "tool": "rustc", "path": "src/main.rs", "line_number": 42, ... } },
    { "kind": "test_result",   "line": 88, "data": { "framework": "cargo", "status": "failed", "passed": 4, "failed": 1 } }
  ]
}
```

`--parsers` 를 생략하면 기본 4종 (`path,url,prompt_boundary,exit_code`) 만 활성화된다. 고급 6종은 명시적으로 지정해야 한다.

## 명령 단위 인덱싱 — `surface.commands`

셸 통합이 OSC 133 시퀀스를 보내면 호스트가 각 명령의 **prompt 시작 / 명령 시작 / 종료 / exit code / 명령 문자열** 을 `tasty-memory` 위에 기록한다 (`scope=surface:<id>` / `tasty.commands.<unix-ms>`).

```
tasty read commands --surface S --limit 10
tasty read last-command --surface S
tasty read command-at --surface S --index -1   # 음수 인덱스 = 끝에서
```

응답 record:
```json
{
  "prompt_started_at": 1734517200000,
  "command_started_at": 1734517200500,
  "ended_at": 1734517203800,
  "exit_code": 0,
  "command": "cargo test --workspace"
}
```

OSC 133 미지원 셸은 빈 배열을 돌려준다. 셸 통합 설치 방법은 사용 중인 셸의 문서 참조.

## 스트리밍 옵저버 — `output.observe_*`

PTY 라인이 들어올 때마다 파서를 돌리고 결과를 sink 로 fan-out 한다. 옵저버는 **휘발성** — 호스트 재시작 시 사라진다 (필요하면 시작 hook 으로 재등록).

### 시작

```bash
tasty output observe start \
  --surface S \
  --parsers path,url,exit_code \
  --kinds path,exit_code \
  --sink memory --max-records 1000
```

JSON RPC:
```json
{
  "method": "output.observe_start",
  "params": {
    "surface_id": "S",
    "parsers": ["path", "url", "exit_code"],
    "kinds": ["path", "exit_code"],
    "sink": { "type": "memory", "max_records": 1000 }
  }
}
```

응답:
```json
{
  "observer_id": "obs_7f3a",
  "info": { "id": "obs_7f3a", "surface_id": "S", "parsers": [...], "kinds": [...], "sink": {...}, "total_in": 0, "total_out": 0, "dropped": 0 }
}
```

### sink 종류

| `sink.type` | 추가 필드 | 효과 |
|-------------|----------|------|
| `memory` | `max_records?: usize` (기본 10000, 0=무한) | `tasty.observer.<id>.<ms>` 키로 `tasty-memory` 에 누적. ring buffer. `memory.list` / `memory.query` 로 회수 |
| `file` | `path?: string` (기본 `~/.tasty/observers/<id>.jsonl`) | JSONL append. 외부 도구 (`jq`, `tail -F`) 와 직접 결합 |

socket / fifo sink 는 Phase 4 (observability) 에서 추가 예정.

### 필터

- `parsers`: 어떤 파서를 활성화할지. 생략 시 기본 4종.
- `kinds`: **출력 후 필터**. 파서가 만든 item 중 이 kind 만 sink 로 보낸다. `parsers` 와 별개. 예: `parsers=[path,url]` + `kinds=[url]` → path 매치는 무시.
- `surface_id`: 생략하면 **전체 surface** 출력을 본다 (wildcard observer).

### 백압

각 옵저버는 별개의 bounded channel (capacity 256). 채워지면 새 item 은 **drop** 되고 `info.dropped` 카운터가 증가한다. PTY 스레드를 절대 block 하지 않는다 (터미널이 멈추는 일 방지). drop 이 누적되면 sink 가 느리거나 파서 출력이 너무 많다는 신호.

### 조회 / 정리

```bash
tasty output observe list                 # 모든 옵저버
tasty output observe info --id obs_7f3a   # total_in/out/dropped/last_event_ms
tasty output observe stop --id obs_7f3a
```

Surface 가 닫히면 그 surface 에 매인 옵저버는 자동 정리된다. wildcard (`surface_id` 없음) 옵저버는 유지된다.

## 멀티라인 파서의 동작 차이

`compile_error` 와 `stack_trace` 는 **여러 줄에 걸친** 패턴을 본다 (rustc 의 `--> path:line:col`, Python traceback 의 frame 시퀀스 등).

- `parse_since_mark` (batch): 전체 텍스트를 block 으로 받아 정확하게 분해.
- `output.observe_*` (streaming): 라인 단위 dispatch 이므로 **멀티라인 파서는 발화하지 않는다**.

옵저버로 컴파일 에러를 모으고 싶다면 다음 패턴을 쓴다:

1. `prompt_boundary` 옵저버로 명령 시작/종료를 감지.
2. 종료 후 `parse_since_mark` 또는 `surface.commands` + `parse_since_mark` 조합으로 batch 파싱.

## 권한

`surface.parse_since_mark`, `surface.commands*`, `output.observe_*` 모두 **`TerminalRead`** 권한이 필요하다. plugin 매니페스트에서 명시적으로 grant 받아야 한다 (`docs/dev-guide/plugin-permissions.md`).

## 관련 문서

- 파서 카탈로그: [output-parsers.md](output-parsers.md)
- IPC 레퍼런스: [api-reference.md](api-reference.md)
- Event 카탈로그 (옵저버는 별개): [event-catalog.md](event-catalog.md)
