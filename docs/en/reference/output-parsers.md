<!-- source-hash: 438a27d3668f -->
# Output parser catalogue

The built-in parsers with which the `tasty-output` crate decomposes PTY output text into semantic units (`ParsedItem`). The same set is shared by two entry points — `surface.parse_since_mark` (one-shot batch) / `output.observe_*` (streaming). Entry-point usage and behaviour: [features/terminal-output](../features/terminal-output/index.md).

## Response schema

```jsonc
{ "kind": "path", "line": 2, "byte_start": 9, "byte_end": 24, "data": { /* per parser */ } }
```

## Parsers

| id | On by default | Span | Key payload |
|----|:---:|------|--------------|
| `path` | ✓ | single line | `path, line, col` |
| `url` | ✓ | single line | `url, scheme(http/https/ftp/ssh/file)` |
| `prompt_boundary` | ✓ | single line | `phase(A/B/C/D), payload` |
| `exit_code` | ✓ | single line | `code: i32` |
| `compile_error` | ✗ | multi-line | `tool(rustc/gcc/tsc), severity, code, message, path, line_number, column` |
| `stack_trace` | ✗ | multi-line | `language(python/rust/node/java), frames, exception?, message?, thread?` |
| `test_result` | ✗ | single line | `framework(cargo/pytest/jest), status, counts, duration_seconds?` |
| `progress` | ✗ | single line | `style(bar/size/percent), percent, current/total/unit` |
| `osc_link` | ✗ | single line | `url, text, params` (OSC 8) |
| `osc_notification` | ✗ | single line | `osc(9/777), title?, message, action?` |

The four on by default (`DEFAULT_PARSER_IDS` = `path` / `url` / `prompt_boundary` / `exit_code`) carry a low false-positive risk. The other six are **explicit opt-in** because of domain specificity and false-positive risk.

## Multi-line parser limitation

`compile_error` and `stack_trace` look at patterns spanning several lines.
- `parse_since_mark` (batch): the whole block is decomposed exactly with `parse_block`.
- `output.observe_*` (streaming): per-line `parse_line` dispatch, so **multi-line parsers never fire.** To collect compile errors, detect the end with a `prompt_boundary` observer and then run a `parse_since_mark` batch.

## Examples

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

## Related

- [features/terminal-output](../features/terminal-output/index.md) — entry points, sinks, filters, back-pressure
- [reference/api](api.md) — `surface.parse_since_mark` / `output.observe_*`
