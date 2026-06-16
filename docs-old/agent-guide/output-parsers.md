# Output Parsers

`tasty-output` 크레이트는 PTY 출력 텍스트를 의미 단위(`ParsedItem`)로 분해하는 빌트인 파서를 제공한다. 같은 파서 집합이 다음 진입점에서 공유된다:

- `surface.parse_since_mark` IPC / `tasty read parse-since-mark` CLI — 마크 이후 출력을 일회성 분해.
- `output.observe_start` IPC / `tasty output observe start` CLI — 출력 스트림을 실시간 파서 → sink 로 fan-out.

## 응답 스키마

```jsonc
{
  "kind": "path",              // 파서 id (= 항목 종류)
  "line": 2,                   // 0-based 라인 번호
  "byte_start": 9,             // 매치된 byte range (라인 내 offset)
  "byte_end": 24,
  "data": { /* 파서별 페이로드 (아래 표) */ }
}
```

## 파서 목록

| id | 기본 활성 | 범위 | 핵심 페이로드 |
|----|----------|------|---------------|
| `path` | ✓ | 한 줄 | `path`, `line`, `col` |
| `url` | ✓ | 한 줄 | `url`, `scheme` (`http`/`https`/`ftp`/`ssh`/`file`) |
| `prompt_boundary` | ✓ | 한 줄 | `phase` (`A`/`B`/`C`/`D`), `payload` |
| `exit_code` | ✓ | 한 줄 | `code: i32` |
| `compile_error` | ✗ | 여러 줄 | `tool` (`rustc`/`gcc`/`tsc`), `severity`, `code`, `message`, `path`, `line_number`, `column` |
| `stack_trace` | ✗ | 여러 줄 | `language` (`python`/`rust`/`node`/`java`), `frames`, `exception?`, `message?`, `thread?` |
| `test_result` | ✗ | 한 줄 | `framework` (`cargo`/`pytest`/`jest`), `status` (`passed`/`failed`), counts, `duration_seconds?` |
| `progress` | ✗ | 한 줄 | `style` (`bar`/`size`/`percent`), `percent`, `current/total/unit` |
| `osc_link` | ✗ | 한 줄 | `url`, `text`, `params` (OSC 8) |
| `osc_notification` | ✗ | 한 줄 | `osc` (9 / 777), `title?`, `message`, `action?` |

기본 활성 4종은 false-positive 위험이 낮고 모든 surface 에서 유용하므로 `DEFAULT_PARSER_IDS` 에 포함된다. 나머지 6종은 도메인 특수성 또는 false-positive 위험 때문에 **명시적으로 opt-in** 해야 한다.

## 옵저버 스트리밍 한계

옵저버 경로는 PTY 라인 → `parse_line` (라인별 dispatch) → sink 로 흐른다. **멀티라인 파서**(`compile_error`, `stack_trace`)는 라인 컨텍스트로는 동작할 수 없으므로 옵저버 스트림에서는 발화하지 않는다. 멀티라인 분해가 필요하면 `surface.parse_since_mark` 를 사용하라 — 이 경로는 전체 block 을 받아 `parse_block` 으로 호출한다.

## 예시

### compile_error (rustc)

입력:
```
error[E0308]: mismatched types
 --> src/main.rs:10:5
```
출력:
```json
{
  "kind": "compile_error",
  "line": 0,
  "data": {
    "tool": "rustc",
    "severity": "error",
    "code": "E0308",
    "message": "mismatched types",
    "path": "src/main.rs",
    "line_number": 10,
    "column": 5
  }
}
```

### stack_trace (python)

입력:
```
Traceback (most recent call last):
  File "app.py", line 10, in main
    raise ValueError('bad')
ValueError: bad
```
출력:
```json
{
  "kind": "stack_trace",
  "data": {
    "language": "python",
    "exception": "ValueError",
    "message": "bad",
    "frames": [{ "path": "app.py", "line": 10, "func": "main" }]
  }
}
```

### test_result (pytest)

입력: `============== 5 passed, 1 failed, 2 skipped in 0.34s ==============`

출력:
```json
{
  "kind": "test_result",
  "data": {
    "framework": "pytest",
    "status": "failed",
    "counts": { "passed": 5, "failed": 1, "skipped": 2 },
    "duration_seconds": 0.34
  }
}
```

### progress

| 입력 | `style` | 페이로드 |
|------|---------|----------|
| `[====>     ] 45%` | `bar` | `percent: 45.0`, `bar: "====>     "` |
| `Downloading 50 MB / 200 MB` | `size` | `current: 50`, `total: 200`, `current_unit: "MB"`, `percent: 25.0` |
| `Progress: 78%` | `percent` | `percent: 78.0` |

### osc_link / osc_notification

| 입력 | 파서 | 페이로드 핵심 |
|------|------|--------------|
| `\e]8;;https://x\e\\here\e]8;;\e\\` | `osc_link` | `url: "https://x"`, `text: "here"` |
| `\e]9;Build finished\x07` | `osc_notification` | `osc: 9`, `message: "Build finished"` |
| `\e]777;notify;Compile;done\x07` | `osc_notification` | `osc: 777`, `action: "notify"`, `title: "Compile"`, `message: "done"` |
