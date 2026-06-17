# 출력 파서 카탈로그

`tasty-output` 크레이트가 PTY 출력 텍스트를 의미 단위(`ParsedItem`)로 분해하는 빌트인 파서들. 같은 집합이 두 진입점에서 공유된다 — `surface.parse_since_mark`(일회성 batch) / `output.observe_*`(스트리밍). 진입점 사용법·동작은 [features/terminal-output](../features/terminal-output/index.md).

## 응답 스키마

```jsonc
{ "kind": "path", "line": 2, "byte_start": 9, "byte_end": 24, "data": { /* 파서별 */ } }
```

## 파서 목록

| id | 기본 활성 | 범위 | 핵심 payload |
|----|:---:|------|--------------|
| `path` | ✓ | 한 줄 | `path, line, col` |
| `url` | ✓ | 한 줄 | `url, scheme(http/https/ftp/ssh/file)` |
| `prompt_boundary` | ✓ | 한 줄 | `phase(A/B/C/D), payload` |
| `exit_code` | ✓ | 한 줄 | `code: i32` |
| `compile_error` | ✗ | 여러 줄 | `tool(rustc/gcc/tsc), severity, code, message, path, line_number, column` |
| `stack_trace` | ✗ | 여러 줄 | `language(python/rust/node/java), frames, exception?, message?, thread?` |
| `test_result` | ✗ | 한 줄 | `framework(cargo/pytest/jest), status, counts, duration_seconds?` |
| `progress` | ✗ | 한 줄 | `style(bar/size/percent), percent, current/total/unit` |
| `osc_link` | ✗ | 한 줄 | `url, text, params`(OSC 8) |
| `osc_notification` | ✗ | 한 줄 | `osc(9/777), title?, message, action?` |

기본 활성 4종(`DEFAULT_PARSER_IDS` = `path`/`url`/`prompt_boundary`/`exit_code`)은 false-positive 위험이 낮다. 나머지 6종은 도메인 특수성/오탐 위험 때문에 **명시 opt-in**.

## 멀티라인 파서 한계

`compile_error`·`stack_trace` 는 여러 줄에 걸친 패턴을 본다.
- `parse_since_mark`(batch): 전체 block 을 `parse_block` 으로 정확히 분해.
- `output.observe_*`(streaming): 라인별 `parse_line` dispatch 라 **멀티라인 파서는 발화하지 않는다.** 컴파일 에러 수집은 `prompt_boundary` 옵저버로 종료 감지 후 `parse_since_mark` batch 로.

## 예시

```
error[E0308]: mismatched types        →  compile_error { tool:"rustc", code:"E0308", path:"src/main.rs", line_number:10, column:5 }
 --> src/main.rs:10:5
```
```
=== 5 passed, 1 failed, 2 skipped in 0.34s ===   →  test_result { framework:"pytest", status:"failed", counts:{passed:5,failed:1,skipped:2}, duration_seconds:0.34 }
```
```
[====>     ] 45%                       →  progress { style:"bar", percent:45.0 }
Downloading 50 MB / 200 MB            →  progress { style:"size", current:50, total:200, current_unit:"MB", percent:25.0 }
\e]8;;https://x\e\\here\e]8;;\e\\      →  osc_link { url:"https://x", text:"here" }
```

## 관련

- [features/terminal-output](../features/terminal-output/index.md) — 진입점·sink·필터·백압
- [reference/api](api.md) — `surface.parse_since_mark` / `output.observe_*`
