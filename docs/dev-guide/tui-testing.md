# TUI 테스트 가이드

터미널 에뮬레이션 버그를 재현하고 자동 검증하는 방법.

## 원칙

**사용자가 TUI에서 문제를 발견하면, 다음 순서로 처리한다:**

1. **재현 경로 특정** — 어떤 VTE 시퀀스/동작이 문제를 일으키는지 최소 재현 경로를 찾는다
2. **tasty-test-tui에 시나리오 추가** — 해당 문제를 결정적으로 재현하는 시나리오를 만든다
3. **기대 동작을 테스트로 작성** — `debug.cell_info` / `debug.screen_attrs`로 정상 상태를 검증하는 E2E 테스트를 만든다
4. **테스트가 실패하는 것을 확인** — 버그가 있는 상태에서 테스트가 실패하는지 확인한다
5. **버그를 수정하고 테스트 통과 확인** — 수정 후 테스트가 통과하면 회귀 방지가 보장된다

이 순서를 반드시 따를 것. "수정 먼저, 테스트 나중"은 금지.

## 도구

### tasty-test-tui

`crates/tasty-test-tui/`에 있는 crossterm 기반 테스트 앱. 저수준 VTE 시퀀스를 직접 출력하여 터미널 에뮬레이터가 올바르게 해석하는지 검증한다.

```bash
# 빌드
cargo build -p tasty-test-tui

# 실행 (Tasty 터미널 안에서)
tasty-test-tui cursor --row 5 --col 10 --exit
tasty-test-tui colors --exit
tasty-test-tui attrs --exit
tasty-test-tui altscreen --exit
tasty-test-tui unicode --exit
tasty-test-tui scroll-region --exit
```

**시나리오 규칙:**
- 각 시나리오는 **결정적(deterministic)** — 같은 입력이면 항상 같은 화면 상태
- 출력 완료 시 마지막 행에 `*_TEST_DONE` 마커를 출력한다
- `--exit` 플래그: 출력 후 즉시 종료 (E2E 테스트용). 없으면 키 입력 대기 (수동 확인용)

### 디버그 IPC (debug 빌드 전용)

셀 속성을 프로그래밍적으로 조회하는 IPC 메서드. `#[cfg(debug_assertions)]`로 릴리즈 빌드에서 제외.

```bash
# 특정 셀 조회
tasty debug cell-info --row 0 --col 0

# 행 전체 셀 속성 조회
tasty debug screen-attrs --row 2
```

**`debug.cell_info` 응답 필드:**

| 필드 | 타입 | 설명 |
|------|------|------|
| `text` | string | 셀의 문자 ("X", "한", " " 등) |
| `fg` | string | 전경색: `"default"`, `"palette:N"`, `"#rrggbb"` |
| `bg` | string | 배경색: 위와 동일 형식 |
| `bold` | bool | 볼드 여부 |
| `italic` | bool | 이탤릭 여부 |
| `underline` | bool | 밑줄 여부 |
| `strikethrough` | bool | 취소선 여부 |
| `inverse` | bool | 반전 여부 |
| `width` | int | 셀 너비 (1 또는 2, 전각 문자는 2) |

## 시나리오 추가 방법

### 1. 문제 재현 경로 찾기

사용자가 보고한 문제를 최소한의 VTE 시퀀스로 재현한다.

예: "한글 입력 후 커서가 1칸만 이동한다" → 전각 문자 출력 후 커서 위치를 확인하면 재현 가능.

### 2. tasty-test-tui에 시나리오 추가

`crates/tasty-test-tui/src/main.rs`에 새 서브커맨드를 추가한다.

```rust
// Commands enum에 추가
/// 문제를 재현하는 시나리오 설명
NewScenario {
    #[arg(long)]
    exit: bool,
},

// main()에서 매칭
Commands::NewScenario { exit } => scenario_new(exit),

// 시나리오 함수 구현
fn scenario_new(exit: bool) {
    let mut out = io::stdout();
    clear_and_setup(&mut out);

    // 문제를 재현하는 VTE 시퀀스 출력
    write!(out, "\x1b[H한글").unwrap();  // 전각 문자 출력
    out.flush().unwrap();

    finish(&mut out, "NEW_TEST_DONE", exit);
}
```

**시나리오 작성 규칙:**
- `clear_and_setup()`으로 시작 — 화면 초기화
- VTE 시퀀스는 crossterm이 아닌 **raw escape sequence로 직접 출력** — 목적이 터미널 에뮬레이터의 시퀀스 해석 검증이므로
- `finish()`로 종료 — 마커 출력 + 대기/종료 처리
- 마커 이름은 `{SCENARIO}_TEST_DONE` 형식

### 3. E2E 테스트 작성

`tests/e2e_tests.rs`에 테스트를 추가한다.

```rust
// TUI 앱 실행
tasty.set_mark(sid);
tasty.send_text(sid, "tasty-test-tui new-scenario --exit\n");
tasty.wait_for_output(sid, "NEW_TEST_DONE", Duration::from_secs(5));

// 기대 상태 검증
let cell = tasty.call("debug.cell_info", json!({
    "surface_id": sid, "row": 0, "col": 0
}));
assert_eq!(cell["text"], "한");
assert_eq!(cell["width"], 2);

// 전각 문자 뒤의 셀 위치 검증
let next = tasty.call("debug.cell_info", json!({
    "surface_id": sid, "row": 0, "col": 2  // 한(2칸) 뒤는 col 2
}));
assert_eq!(next["text"], "글");
```

**테스트 패턴:**
1. `send_text`로 TUI 앱 실행 (반드시 `--exit` 플래그)
2. `wait_for_output`로 완료 마커 대기
3. `debug.cell_info` 또는 `debug.screen_attrs`로 셀 상태 검증

## 검증 가능한 항목

| 항목 | 검증 방법 |
|------|-----------|
| 커서 위치 | `cursor` 시나리오 → `cell_info`로 마커 위치 확인 |
| SGR 색상 | `colors` 시나리오 → `cell_info`의 `fg`/`bg` 필드 |
| 텍스트 속성 | `attrs` 시나리오 → `cell_info`의 `bold`/`italic` 등 |
| 대체 화면 | `altscreen` 시나리오 → alt screen 내용 확인 → 퇴장 후 원래 내용 확인 |
| CJK 전각 문자 | `unicode` 시나리오 → `width` 필드가 2인지, 다음 셀 위치가 올바른지 |
| 스크롤 리전 | `scroll-region` 시나리오 → 리전 밖 행이 영향 안 받았는지 |
| IME 컴포징 위치 | 커서 시나리오 + `debug ime-preedit` → `cursor_position`으로 위치 검증 |

## 주의사항

- **ratatui 사용 금지** — TUI 앱은 crossterm의 raw escape sequence를 직접 출력해야 한다. 고수준 프레임워크가 시퀀스를 추상화하면 "터미널이 시퀀스를 올바르게 해석하는가"를 검증할 수 없다.
- **디버그 IPC는 릴리즈 빌드에 없다** — E2E 테스트는 debug 빌드에서만 실행된다.
- **TUI 앱 자체의 버그 주의** — TUI 앱이 잘못된 시퀀스를 보내면 테스트가 오염된다. 시나리오는 가능한 한 단순하게 유지할 것.
- **`--exit` 없이 실행하면 키 입력 대기** — 수동 확인용. E2E 테스트에서는 반드시 `--exit`를 사용한다.
