# 터미널 출력 구조화 (Terminal output)

- **Status**: Implemented
- **주체**: AI Agent
- **ADR**: 없음
- **코드**: `tasty-output` 크레이트, `surface.parse_since_mark`/`surface.commands`/`output.observe_*` 핸들러
- **화면**: 없음
- **메서드/파서**: [reference/api](../../reference/api.md#surface-상호작용) · [reference/output-parsers](../../reference/output-parsers.md)

## 목적

터미널 출력을 **의미 단위**로 분해해 에이전트가 다루기 쉬운 JSON 으로 제공한다. 모두 `terminal.read` 권한.

## 내부 동작 — 세 진입점

| 진입점 | 패턴 | 용도 |
|--------|------|------|
| `surface.parse_since_mark` | 일회성 batch | 마크 이후 출력을 한 번에 분해 |
| `surface.commands` (+`last_command`,`command_at`) | 일회성 batch | OSC 133 인덱싱된 **명령 단위** 메타데이터 |
| `output.observe_start` | 스트리밍 | PTY 라인마다 파서 → sink fan-out |

세 경로 모두 같은 [파서 카탈로그](../../reference/output-parsers.md)를 공유.

### parse_since_mark

`set mark` → 명령 실행 → `parse-since-mark --parsers path,url,compile_error,test_result`. `--parsers` 생략 시 기본 4종(`path,url,prompt_boundary,exit_code`). 고급 6종은 명시 opt-in. 전체 block 을 받아 멀티라인 파서(`compile_error`/`stack_trace`)도 정확히 분해.

### 명령 인덱싱 (OSC 133)

셸 통합이 OSC 133 을 보내면 각 명령의 prompt 시작/명령 시작/종료/exit code/명령 문자열을 `tasty-memory`(`surface:<id>` scope, `tasty.commands.<ms>`)에 기록. OSC 133 미지원 셸은 빈 배열.

### 스트리밍 옵저버

PTY 라인마다 파서를 돌려 sink 로 fan-out(**휘발성** — 호스트 재시작 시 소멸). sink: `memory`(ring buffer, `memory.list`/`query` 로 회수) / `file`(JSONL append). 필터: `parsers`(활성 파서) + `kinds`(출력 후 kind 필터) + `surface_id`(생략 시 전체 surface wildcard). **백압**: 옵저버별 bounded channel(256), 채워지면 drop + `info.dropped` 증가(PTY 스레드 절대 block 안 함). surface 닫히면 매인 옵저버 자동 정리, wildcard 는 유지.

> **멀티라인 파서는 옵저버에서 발화하지 않는다**(라인별 dispatch). 컴파일 에러 수집은 `prompt_boundary` 옵저버로 종료 감지 후 `parse_since_mark` batch.

## 인터페이스

- **AI Agent / CLI**: `tasty read parse-since-mark` · `tasty read commands/last-command/command-at` · `tasty output observe {start,list,info,stop}`. [reference/api](../../reference/api.md#surface-상호작용).

## 관련

- [reference/output-parsers](../../reference/output-parsers.md) — 파서 카탈로그 · [work-area](../work-area/index.md) — surface
