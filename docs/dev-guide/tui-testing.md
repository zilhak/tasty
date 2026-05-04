# TUI 테스트 가이드

터미널 에뮬레이션 버그를 재현하고 자동 검증하는 방법.

## 원칙

**사용자가 TUI에서 문제를 발견하면, 다음 순서로 처리한다:**

1. **재현 경로 특정** — 어떤 VTE 시퀀스/동작이 문제를 일으키는지 최소 재현 경로를 찾는다
2. **tasty-tui-sim에 시나리오 추가** — 해당 문제를 결정적으로 재현하는 시나리오를 만든다
3. **기대 동작을 테스트로 작성** — `debug.cell_info` / `debug.screen_attrs`로 정상 상태를 검증하는 E2E 테스트를 만든다
4. **테스트가 실패하는 것을 확인** — 버그가 있는 상태에서 테스트가 실패하는지 확인한다
5. **버그를 수정하고 테스트 통과 확인** — 수정 후 테스트가 통과하면 회귀 방지가 보장된다

이 순서를 반드시 따를 것. "수정 먼저, 테스트 나중"은 금지.

## 도구

### tasty-tui-sim (TUI 시뮬레이터)

`crates/tasty-tui-simulator/`에 있는 VTE 시퀀스 시뮬레이터. 고수준 명령("cursor 5 3", "bold", "print hello")을 raw VTE escape sequence로 변환하여 터미널에 출력한다. 터미널 입장에서는 실제 TUI 앱(vim, htop 등)이 동작하는 것과 동일한 바이트 스트림을 받는다.

테스트 전용이 아니라 **독립적인 TUI 도구**이며, 테스트가 이 도구를 활용하는 구조.

```bash
cargo build -p tasty-tui-simulator
```

#### 인터랙티브 모드 (핵심)

서브커맨드 없이 실행하면 stdin에서 명령을 읽는 REPL로 동작한다. 외부(E2E 테스트)에서 `surface.send`로 명령을 한 줄씩 보내서 터미널 상태를 단계적으로 구성할 수 있다.

```bash
tasty-tui-sim              # 인터랙티브 모드 진입
tasty-tui-sim interactive  # 명시적으로도 가능
```

시작 시 `READY`를 출력하고, 매 명령 실행 후 `OK`를 출력한다. 테스트에서는 `wait_for_output("OK")`로 동기화한다.

#### 인터랙티브 명령어 레퍼런스

**화면 제어:**

| 명령 | 설명 |
|------|------|
| `clear` | 화면 초기화 (ED 2 + CUP home) |
| `reset` | 전체 터미널 리셋 (RIS) |

**커서 이동:**

| 명령 | 설명 |
|------|------|
| `cursor <row> <col>` | 지정 위치로 이동 (0-indexed) |
| `cursor-up [N]` | N칸 위로 (기본 1) |
| `cursor-down [N]` | N칸 아래로 |
| `cursor-right [N]` | N칸 오른쪽으로 |
| `cursor-left [N]` | N칸 왼쪽으로 |
| `cursor-save` | 커서 위치 저장 (DECSC) |
| `cursor-restore` | 저장된 위치로 복원 (DECRC) |

**텍스트 출력:**

| 명령 | 설명 |
|------|------|
| `print <text>` | 현재 위치에 텍스트 출력 |
| `println <text>` | 텍스트 출력 + CR+LF |
| `newline` | LF 출력 |
| `cr` | CR 출력 |
| `tab` | TAB 출력 |
| `bell` | BEL(0x07) 출력 |

**SGR (텍스트 속성 & 색상):**

| 명령 | 설명 |
|------|------|
| `sgr <params>` | SGR 시퀀스 출력 (예: `sgr 1` = bold, `sgr 38;5;1` = red fg) |
| `sgr-reset` | SGR 리셋 |
| `bold` | 볼드 |
| `italic` | 이탤릭 |
| `underline` | 밑줄 |
| `strikethrough` | 취소선 |
| `inverse` | 반전 |
| `dim` | 흐리게 |
| `fg <N>` 또는 `fg <r;g;b>` | 전경색 (팔레트 인덱스 또는 TrueColor) |
| `bg <N>` 또는 `bg <r;g;b>` | 배경색 |

**지우기:**

| 명령 | 설명 |
|------|------|
| `erase-display [N]` | ED (0=커서~끝, 1=시작~커서, 2=전체, 3=스크롤백 포함) |
| `erase-line [N]` | EL (0=커서~끝, 1=시작~커서, 2=전체) |

**대체 화면:**

| 명령 | 설명 |
|------|------|
| `altscreen-enter` | DECSET 1049 |
| `altscreen-exit` | DECRST 1049 |

**스크롤 리전:**

| 명령 | 설명 |
|------|------|
| `scroll-region <top> <bottom>` | DECSTBM 설정 (0-indexed) |
| `scroll-region-reset` | DECSTBM 리셋 |
| `scroll-up [N]` | SU — N줄 위로 스크롤 |
| `scroll-down [N]` | SD — N줄 아래로 스크롤 |

**터미널 크기:**

| 명령 | 설명 |
|------|------|
| `size` | 현재 터미널 크기를 `SIZE:{cols}x{rows}` 형식으로 출력 |

**마우스 트래킹:**

| 명령 | 설명 |
|------|------|
| `mouse-track` | X10 클릭 트래킹 + SGR 인코딩 활성화 (1000+1006) |
| `mouse-track-off` | 마우스 트래킹 비활성화 |
| `mouse-track-motion` | 셀 모션 트래킹 + SGR (1002+1006) |
| `mouse-track-all` | 전체 모션 트래킹 + SGR (1003+1006) |

**DECSET/DECRST:**

| 명령 | 설명 |
|------|------|
| `decset <mode>` | DECSET (예: `decset 1049`) |
| `decrst <mode>` | DECRST |

**Raw 시퀀스:**

| 명령 | 설명 |
|------|------|
| `raw <hex>` | 16진수 바이트를 직접 출력 (예: `raw 1b5b48` = ESC[H) |
| `esc <seq>` | ESC + 문자열 출력 (예: `esc [H` = ESC[H) |

**프리셋 시나리오 (인라인 실행):**

| 명령 | 설명 |
|------|------|
| `scenario cursor` | 커서 이동 시나리오 |
| `scenario colors` | ANSI/TrueColor 시나리오 |
| `scenario attrs` | 텍스트 속성 시나리오 |
| `scenario unicode` | CJK 전각 시나리오 |
| `scenario scroll-region` | 스크롤 리전 시나리오 |

**종료:**

| 명령 | 설명 |
|------|------|
| `quit` / `exit` | 정상 종료 (exit code 0). `BYE` 출력 후 종료 |
| `exit-code <N>` | 지정 코드로 종료 |
| `crash` | SIGABRT로 비정상 종료 |
| `panic` | Rust panic으로 비정상 종료 |

#### 원샷 서브커맨드 (수동 확인용)

미리 정의된 시나리오를 한번에 실행하고 끝내는 모드. 수동으로 눈으로 확인할 때 사용.

```bash
tasty-tui-sim cursor --row 5 --col 10 --exit
tasty-tui-sim colors --exit
tasty-tui-sim attrs --exit
tasty-tui-sim altscreen --exit
tasty-tui-sim unicode --exit
tasty-tui-sim scroll-region --exit
```

- `--exit`: 출력 후 즉시 종료. 없으면 키 입력 대기.

### 디버그 IPC (debug 빌드 전용)

셀 속성을 프로그래밍적으로 조회하는 IPC 메서드. `#[cfg(debug_assertions)]`로 릴리즈 빌드에서 제외.

```bash
# 특정 셀 조회
tasty debug cell-info --row 0 --col 0

# 행 전체 셀 속성 조회
tasty debug screen-attrs --row 2
```

**`debug.cell_info` 응답 필드:**

termwiz가 파싱한 `CellAttributes` 단계의 정보를 그대로 노출한다. **렌더러가 실제로 그 속성을 GPU 출력에 반영했는지는 별도로 `debug.glyph_color`로 확인해야 한다.**

| 필드 | 타입 | 설명 |
|------|------|------|
| `text` | string | 셀의 문자 ("X", "한", " " 등) |
| `fg` | string | 전경색: `"default"`, `"palette:N"`, `"#rrggbb"` |
| `bg` | string | 배경색: 위와 동일 형식 |
| `bold` | bool | (호환용) `intensity == "bold"`와 동치 |
| `italic` | bool | 이탤릭 여부 |
| `underline` | bool | (호환용) `underline_style != "none"`과 동치 |
| `strikethrough` | bool | 취소선 여부 |
| `inverse` | bool | SGR 7 반전 여부 |
| `width` | int | 셀 너비 (1 또는 2, 전각 문자는 2) |
| `intensity` | string | `"normal"` \| `"bold"` \| `"half"` (faint/dim, SGR 2) |
| `underline_style` | string | `"none"` \| `"single"` \| `"double"` \| `"curly"` \| `"dotted"` \| `"dashed"` |
| `underline_color` | string | 밑줄 색 (fg/bg와 동일 포맷) |
| `blink` | string | `"none"` \| `"slow"` \| `"rapid"` |
| `invisible` | bool | SGR 8 |
| `overline` | bool | SGR 53 |
| `vertical_align` | string | `"baseline"` \| `"super"` \| `"sub"` |

> **주의**: `overline`, `underline_color`, `vertical_align`은 termwiz `CellAttributes`에는 존재하지만 termwiz의 `AttributeChange` enum에는 해당 variant가 없어서 현재 SGR 파이프라인이 이 속성들을 셀에 전달하지 못한다 (`crates/tasty-terminal/src/vte_handler.rs`의 `Sgr::Overline | UnderlineColor | VerticalAlign` 분기 참조). 따라서 SGR로 입력해도 항상 기본값이 반환된다. 검증 인프라만 먼저 준비된 상태이며, 실제 전달은 termwiz API 확장 또는 우회 구현이 필요하다.

### debug.glyph_color (debug 빌드 전용)

특정 셀에 대해 **렌더러가 GPU에 push하는 (bg, fg) RGBA**를 반환한다. `debug.cell_info`가 termwiz 단계의 속성을 보여준다면, 이쪽은 그 속성이 실제 색상 결정에 반영되었는지를 검증한다.

```bash
tasty debug glyph-color --row 0 --col 0
tasty debug glyph-color --row 0 --col 0 --bg-mode unfocused
tasty debug glyph-color --row 0 --col 0 --surface 3
```

**응답 필드:**

| 필드 | 타입 | 설명 |
|------|------|------|
| `row`, `col` | int | 요청한 좌표 |
| `in_bounds` | bool | 좌표에 셀이 존재하는지 |
| `bg_mode` | string | `"focused"` \| `"unfocused"` (어떤 default bg를 적용했는지) |
| `default_bg` | object | `{ r, g, b, a, hex }` — 결정에 쓰인 기본 배경 |
| `bg` | object | 같은 형식. 셀에 실제 적용된 배경 |
| `fg` | object | 같은 형식. 셀에 실제 적용된 전경 |

**검증 예시 (faint/dim 회귀 방지):**

```bash
# 1. tasty-tui-sim에 SGR 2 (dim) 후 텍스트 출력
echo -e "dim\nprint Hint\nquit" | tasty-tui-sim ...
# 2. termwiz 단계 검증: intensity 가 "half"로 들어왔는지
tasty debug cell-info --row 0 --col 0
# → { ..., "intensity": "half", ... }
# 3. 렌더러 단계 검증: fg가 어둡게 적용되었는지
tasty debug glyph-color --row 0 --col 0
# → bg 위에서 fg가 dim 처리되었는지 hex로 비교
```

`compute_cell_colors`(`src/renderer/palette.rs`)가 GPU 인스턴스 버퍼에 들어가는 색을 계산하는 단일 진실 원천이다. 렌더러 코드와 `debug.glyph_color`가 모두 이 함수를 사용하므로, 함수 내 처리가 누락된 SGR 속성은 `debug.glyph_color` 결과에서 즉시 드러난다.

### 입력 시뮬레이션 IPC (debug + `--enable-input-simulation` 필요)

마우스/키보드 이벤트를 PTY에 주입하는 디버그 IPC. **2단계 게이트**로 보호된다:

1. `#[cfg(debug_assertions)]` — 릴리즈 빌드에서 코드 자체가 없음
2. `--enable-input-simulation` 플래그 — debug 빌드라도 이 플래그 없이 실행하면 거부

```bash
# 플래그 없이 실행하면 거부됨
tasty
# → debug.inject_mouse 호출 시: "input simulation not enabled"

# 플래그로 명시적 허락
tasty --enable-input-simulation
# → debug.inject_mouse 동작
```

**`debug.inject_mouse`** — SGR 마우스 이벤트를 PTY에 주입:

```json
{"method": "debug.inject_mouse", "params": {
    "surface_id": 1,
    "col": 5, "row": 3,
    "button": 0,
    "event_type": "press"
}}
```

- `button`: 0=left, 1=middle, 2=right (기본: 0)
- `event_type`: `"press"`, `"release"`, `"move"` (기본: "press")

**`debug.inject_key`** — 임의 바이트/텍스트를 PTY에 주입:

```json
{"method": "debug.inject_key", "params": {
    "surface_id": 1,
    "text": "hello"
}}
```
또는 hex 바이트로:
```json
{"method": "debug.inject_key", "params": {
    "surface_id": 1,
    "bytes": "1b5b41"
}}
```

## 시나리오 추가 방법

### 1. 문제 재현 경로 찾기

사용자가 보고한 문제를 최소한의 VTE 시퀀스로 재현한다.

예: "한글 입력 후 커서가 1칸만 이동한다" → 전각 문자 출력 후 커서 위치를 확인하면 재현 가능.

### 2. tasty-tui-sim에 시나리오 추가

`crates/tasty-tui-simulator/src/main.rs`에 새 서브커맨드를 추가한다.

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

`tests/e2e_tests.rs`에 테스트를 추가한다. 인터랙티브 모드를 사용하면 하나의 TUI 프로세스 안에서 명령을 단계별로 보내며 검증할 수 있다.

```rust
// 1. TUI 앱을 인터랙티브 모드로 실행
tasty.set_mark(sid);
tasty.send_text(sid, "tasty-tui-sim\n");
tasty.wait_for_output(sid, "READY", Duration::from_secs(5));

// 2. 명령을 보내고 OK를 기다려 동기화
tasty.send_text(sid, "clear\n");
tasty.wait_for_output(sid, "OK", Duration::from_secs(2));

// 3. 재현하려는 동작을 단계별로 수행
tasty.send_text(sid, "cursor 0 0\n");
tasty.wait_for_output(sid, "OK", Duration::from_secs(2));

tasty.send_text(sid, "print 한글\n");
tasty.wait_for_output(sid, "OK", Duration::from_secs(2));

// 4. 기대 상태 검증
let cell = tasty.call("debug.cell_info", json!({
    "surface_id": sid, "row": 0, "col": 0
}));
assert_eq!(cell["text"], "한");
assert_eq!(cell["width"], 2);

let next = tasty.call("debug.cell_info", json!({
    "surface_id": sid, "row": 0, "col": 2  // 한(2칸) 뒤
}));
assert_eq!(next["text"], "글");

// 5. 추가 동작 (같은 프로세스에서 계속)
tasty.send_text(sid, "cursor 1 0\n");
tasty.wait_for_output(sid, "OK", Duration::from_secs(2));
tasty.send_text(sid, "bold\n");
tasty.wait_for_output(sid, "OK", Duration::from_secs(2));
tasty.send_text(sid, "print HELLO\n");
tasty.wait_for_output(sid, "OK", Duration::from_secs(2));

let bold_cell = tasty.call("debug.cell_info", json!({
    "surface_id": sid, "row": 1, "col": 0
}));
assert_eq!(bold_cell["text"], "H");
assert_eq!(bold_cell["bold"], true);

// 6. 종료
tasty.send_text(sid, "quit\n");
tasty.wait_for_output(sid, "BYE", Duration::from_secs(2));
```

**테스트 패턴:**
1. `send_text`로 `tasty-tui-sim\n` 실행, `READY` 대기
2. 명령을 한 줄씩 보내고, 매번 `OK` 대기로 동기화
3. `debug.cell_info` / `debug.screen_attrs`로 셀 상태 검증
4. 같은 프로세스에서 여러 단계를 연속 수행 가능
5. `quit`으로 정상 종료, `crash`로 비정상 종료 테스트

## 시나리오별 기대 출력 레퍼런스

각 시나리오가 어떤 행/열에 무엇을 출력하는지 정리. E2E 테스트 작성 시 검증값으로 사용한다.

### cursor

커서를 지정 위치로 이동한 뒤 마커 문자를 출력한다.

```
옵션: --row R --col C --marker M (기본: row=5, col=10, marker=X)
```

| row | col | 기대 | 비고 |
|-----|-----|------|------|
| R | C | `M` | 마커 문자 |
| 마지막 행 | 0~ | `CURSOR_TEST_DONE` | 완료 마커 |

### colors

ANSI 16색과 TrueColor 출력.

**Row 0**: ANSI 16색 전경 — 각 문자가 해당 팔레트 색상의 fg로 출력

| col | text | fg |
|-----|------|----|
| 0 | `0` | `palette:0` (black) |
| 1 | `1` | `palette:1` (red) |
| ... | ... | ... |
| 9 | `9` | `palette:9` |
| 10 | `A` | `palette:10` |
| 15 | `F` | `palette:15` (bright white) |

**Row 1**: ANSI 16색 배경 — 각 셀이 해당 팔레트 색상의 bg, 문자는 공백

| col | text | bg |
|-----|------|----|
| 0 | ` ` | `palette:0` |
| 1 | ` ` | `palette:1` |
| ... | ... | ... |
| 15 | ` ` | `palette:15` |

**Row 2**: TrueColor 전경

| col | text | fg |
|-----|------|----|
| 0 | `R` | `#ff0000` |
| 1 | `G` | `#00ff00` |
| 2 | `B` | `#0000ff` |

### attrs

각 행에 하나의 텍스트 속성을 적용하여 출력.

| row | text | bold | italic | underline | strikethrough | inverse |
|-----|------|------|--------|-----------|---------------|---------|
| 0 | `BOLD` | true | false | false | false | false |
| 1 | `ITALIC` | false | true | false | false | false |
| 2 | `UNDERLINE` | false | false | true | false | false |
| 3 | `STRIKE` | false | false | false | true | false |
| 4 | `INVERSE` | false | false | false | false | true |
| 5 | `COMBO` | true | true | true | false | false |

### altscreen

대체 화면 진입/퇴장 시퀀스 검증.

1. 일반 화면에 `NORMAL_SCREEN` 출력 (row 0)
2. DECSET 1049로 대체 화면 진입
3. 대체 화면에 `ALT_SCREEN_CONTENT` 출력 (row 0)
4. 마지막 행에 `ALTSCREEN_TEST_DONE` 출력
5. (대기 후) DECRST 1049로 대체 화면 퇴장

**대체 화면 진입 직후:**

| row | col 0~ | 비고 |
|-----|--------|------|
| 0 | `ALT_SCREEN_CONTENT` | 대체 화면 콘텐츠 |
| 마지막 행 | `ALTSCREEN_TEST_DONE` | 완료 마커 |

**퇴장 후:**

| row | col 0~ | 비고 |
|-----|--------|------|
| 0 | `NORMAL_SCREEN` | 원래 일반 화면으로 복원되어야 함 |

### unicode

CJK 전각 문자 렌더링 검증. 전각 문자는 2셀을 차지한다.

**Row 0**: 한글 `한글`

| col | text | width |
|-----|------|-------|
| 0 | `한` | 2 |
| 2 | `글` | 2 |

**Row 1**: 한자 `漢字`

| col | text | width |
|-----|------|-------|
| 0 | `漢` | 2 |
| 2 | `字` | 2 |

**Row 2**: ASCII + 전각 혼합 `AB한CD`

| col | text | width |
|-----|------|-------|
| 0 | `A` | 1 |
| 1 | `B` | 1 |
| 2 | `한` | 2 |
| 4 | `C` | 1 |
| 5 | `D` | 1 |

**Row 3**: 히라가나 `あいう`

| col | text | width |
|-----|------|-------|
| 0 | `あ` | 2 |
| 2 | `い` | 2 |
| 4 | `う` | 2 |

### scroll-region

DECSTBM으로 스크롤 리전 설정 후 리전 내 스크롤 검증.

**초기 출력**: 모든 행에 `LINE0`~`LINE7`

**스크롤 리전**: row 2~5 (1-indexed 3~6)

**리전 바닥(row 5)에서 개행 후 기대 상태:**

| row | 기대 text | 비고 |
|-----|-----------|------|
| 0 | `LINE0` | 리전 밖 — 영향 없음 |
| 1 | `LINE1` | 리전 밖 — 영향 없음 |
| 2 | `LINE3` | 리전 내부 — LINE2가 스크롤 아웃, LINE3이 올라옴 |
| 3 | `LINE4` | 리전 내부 |
| 4 | `LINE5` | 리전 내부 |
| 5 | `SCROLLED` | 리전 바닥에 새 텍스트 |
| 6 | `LINE6` | 리전 밖 — 영향 없음 |
| 7 | `LINE7` | 리전 밖 — 영향 없음 |

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
