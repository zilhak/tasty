# termwiz 0.22 사용 가이드

termwiz 0.22 기준. VTE(Virtual Terminal Emulator) 파싱 및 터미널 상태 관리 라이브러리.

---

## 목차

1. [Parser](#1-parser)
2. [Action 열거형](#2-action-열거형)
3. [ControlCode](#3-controlcode)
4. [CSI](#4-csi)
5. [DecPrivateModeCode](#5-decprivatemodecode)
6. [Esc / EscCode](#6-esc--esccode)
7. [Surface](#7-surface)
8. [Change / Position / CellAttributes](#8-change--position--cellattributes)
9. [전체 파싱 루프 예시](#9-전체-파싱-루프-예시)

---

## 1. Parser

`termwiz::escape::parser::Parser`는 바이트 스트림을 `Action` 시퀀스로 변환한다. 내부에 상태를 유지하므로 인스턴스를 재사용해야 한다.

### 생성

```rust
use termwiz::escape::parser::Parser;

let mut parser = Parser::new();
```

### `parse(bytes, callback)`

바이트 슬라이스를 파싱하고 각 `Action`에 대해 콜백을 호출한다.

```rust
parser.parse(bytes, |action| {
    // action: Action
    handle_action(action);
});
```

**상태 유지 주의:** 이스케이프 시퀀스가 여러 바이트에 걸쳐 있을 수 있다. `parse()`를 여러 번 호출해도 내부 상태가 누적되므로, PTY에서 읽은 청크를 그대로 넘기면 된다.

```rust
// 올바른 사용: 청크 단위로 반복 호출
let mut buf = [0u8; 4096];
loop {
    let n = pty_reader.read(&mut buf)?;
    parser.parse(&buf[..n], |action| handle_action(action));
}
```

```rust
// 잘못된 사용: 매번 새 Parser 생성
// → 시퀀스 경계에서 파싱이 깨진다
for chunk in chunks {
    let mut parser = Parser::new(); // 상태 리셋됨 — 버그
    parser.parse(chunk, |a| { … });
}
```

### `parse_as_vec(bytes) -> Vec<Action>`

콜백 대신 `Vec<Action>`을 반환하는 편의 메서드.

```rust
let actions = parser.parse_as_vec(bytes);
for action in actions {
    handle_action(action);
}
```

내부적으로는 `parse()`와 동일하다. 상태 유지 동작도 같다.

---

## 2. Action 열거형

`termwiz::escape::Action`은 파서가 생성하는 최상위 열거형이다.

```rust
use termwiz::escape::Action;
```

| 변형 | 설명 |
|------|------|
| `Action::Print(char)` | 화면에 출력할 문자 |
| `Action::PrintString(String)` | 연속된 출력 문자열 (최적화) |
| `Action::Control(ControlCode)` | C0 제어 문자 (`\n`, `\r`, `\x07` 등) |
| `Action::DeviceControl(Box<DeviceControlMode>)` | DCS 시퀀스 |
| `Action::OperatingSystemCommand(Box<OperatingSystemCommand>)` | OSC 시퀀스 (제목 변경 등) |
| `Action::CSI(CSI)` | CSI 시퀀스 (커서, SGR, 편집 등) |
| `Action::Esc(Esc)` | ESC 시퀀스 |
| `Action::Sixel(Box<Sixel>)` | Sixel 그래픽 데이터 |
| `Action::XtGetTcap(Vec<String>)` | XTGETTCAP 응답 |
| `Action::KittyImage(Box<KittyImage>)` | Kitty 그래픽 프로토콜 |

### 가장 빈번한 변형

터미널 에뮬레이터 구현에서 가장 자주 만나는 변형:

```rust
match action {
    Action::Print(c) => {
        self.grid.print_char(c);
    }
    Action::PrintString(s) => {
        for c in s.chars() {
            self.grid.print_char(c);
        }
    }
    Action::Control(ctrl) => {
        self.handle_control(ctrl);
    }
    Action::CSI(csi) => {
        self.handle_csi(csi);
    }
    Action::Esc(esc) => {
        self.handle_esc(esc);
    }
    Action::OperatingSystemCommand(osc) => {
        self.handle_osc(*osc);
    }
    _ => {}
}
```

---

## 3. ControlCode

`termwiz::escape::ControlCode`는 C0 제어 문자를 나타낸다.

```rust
use termwiz::escape::ControlCode;
```

| 변형 | 값 | 설명 |
|------|-----|------|
| `ControlCode::Null` | `\x00` | NUL |
| `ControlCode::StartOfHeading` | `\x01` | SOH |
| `ControlCode::StartOfText` | `\x02` | STX |
| `ControlCode::EndOfText` | `\x03` | ETX (Ctrl+C) |
| `ControlCode::EndOfTransmission` | `\x04` | EOT (Ctrl+D) |
| `ControlCode::Enquiry` | `\x05` | ENQ |
| `ControlCode::Acknowledge` | `\x06` | ACK |
| `ControlCode::Bell` | `\x07` | BEL — 벨 소리 |
| `ControlCode::Backspace` | `\x08` | BS — 백스페이스 |
| `ControlCode::HorizontalTab` | `\x09` | HT — 탭 |
| `ControlCode::LineFeed` | `\x0A` | LF — 개행 |
| `ControlCode::VerticalTab` | `\x0B` | VT |
| `ControlCode::FormFeed` | `\x0C` | FF |
| `ControlCode::CarriageReturn` | `\x0D` | CR — 캐리지 리턴 |
| `ControlCode::ShiftOut` | `\x0E` | SO — G1 문자 집합 |
| `ControlCode::ShiftIn` | `\x0F` | SI — G0 문자 집합 |
| `ControlCode::Escape` | `\x1B` | ESC |

```rust
match ctrl {
    ControlCode::LineFeed => self.cursor_down(1),
    ControlCode::CarriageReturn => self.cursor_col = 0,
    ControlCode::Backspace => self.cursor_left(1),
    ControlCode::HorizontalTab => self.advance_tab(),
    ControlCode::Bell => self.ring_bell(),
    _ => {}
}
```

---

## 4. CSI

`termwiz::escape::csi::CSI`는 Control Sequence Introducer 시퀀스다. 가장 복잡하고 중요한 변형이다.

```rust
use termwiz::escape::csi::{CSI, Sgr, Cursor, Edit, Mode};
```

### CSI 최상위 변형

| 변형 | 설명 |
|------|------|
| `CSI::Sgr(Sgr)` | Select Graphic Rendition — 색상, 볼드 등 |
| `CSI::Cursor(Cursor)` | 커서 이동 및 조작 |
| `CSI::Edit(Edit)` | 문자/줄 삽입, 삭제, 지우기 |
| `CSI::Mode(Mode)` | 터미널 모드 설정/해제 |
| `CSI::Device(Box<Device>)` | 장치 속성 쿼리 |
| `CSI::Mouse(MouseReport)` | 마우스 이벤트 보고 |
| `CSI::Window(Box<Window>)` | 창 조작 |
| `CSI::SelectCharacterPath(_, _)` | 문자 경로 |
| `CSI::Unspecified(Box<Unspecified>)` | 알 수 없는 시퀀스 |

### Sgr — 텍스트 속성

```rust
use termwiz::escape::csi::Sgr;
use termwiz::color::{ColorSpec, AnsiColor, SrgbaTuple};
```

| 변형 | 설명 |
|------|------|
| `Sgr::Reset` | 모든 속성 초기화 |
| `Sgr::Intensity(Intensity)` | `Bold`, `Half`, `Normal` |
| `Sgr::Underline(Underline)` | `Single`, `Double`, `Curly`, `Dashed`, `Dotted`, `None` |
| `Sgr::Blink(Blink)` | `Slow`, `Rapid`, `None` |
| `Sgr::Italic(bool)` | 이탤릭 |
| `Sgr::Inverse(bool)` | 전경/배경 반전 |
| `Sgr::Invisible(bool)` | 보이지 않음 |
| `Sgr::StrikeThrough(bool)` | 취소선 |
| `Sgr::Overline(bool)` | 윗줄 |
| `Sgr::Font(Font)` | 대체 폰트 |
| `Sgr::Foreground(ColorSpec)` | 전경색 |
| `Sgr::Background(ColorSpec)` | 배경색 |
| `Sgr::UnderlineColor(ColorSpec)` | 밑줄 색상 |

`ColorSpec` 변형:
```rust
ColorSpec::Default         // 기본 색상
ColorSpec::PaletteIndex(u8) // 0-15: ANSI, 16-231: 6x6x6 큐브, 232-255: 그레이스케일
ColorSpec::TrueColor(SrgbaTuple) // 24비트 색상
```

```rust
match sgr {
    Sgr::Reset => self.attrs = CellAttributes::default(),
    Sgr::Intensity(i) => self.attrs.set_intensity(i),
    Sgr::Foreground(color) => self.attrs.set_foreground(color),
    Sgr::Background(color) => self.attrs.set_background(color),
    Sgr::Underline(u) => self.attrs.set_underline(u),
    Sgr::Italic(b) => self.attrs.set_italic(b),
    Sgr::Inverse(b) => self.attrs.set_reverse(b),
    _ => {}
}
```

### Cursor — 커서 조작

```rust
use termwiz::escape::csi::Cursor;
```

| 변형 | ESC 시퀀스 | 설명 |
|------|-----------|------|
| `Cursor::Up(u32)` | `CSI n A` | 위로 n행 |
| `Cursor::Down(u32)` | `CSI n B` | 아래로 n행 |
| `Cursor::Right(u32)` | `CSI n C` | 오른쪽으로 n열 |
| `Cursor::Left(u32)` | `CSI n D` | 왼쪽으로 n열 |
| `Cursor::NextLine(u32)` | `CSI n E` | 다음 줄의 시작으로 |
| `Cursor::PrecedingLine(u32)` | `CSI n F` | 이전 줄의 시작으로 |
| `Cursor::CharacterAbsoluteColumn(u32)` | `CSI n G` | 절대 열 위치 |
| `Cursor::Position { line, col }` | `CSI r;c H` | 절대 위치 (1-기반) |
| `Cursor::PositionX(u32)` | `CSI n \`' | 수평 절대 위치 |
| `Cursor::PositionY(u32)` | `CSI n d` | 수직 절대 위치 |
| `Cursor::LineTabulation(u32)` | `CSI n I` | 탭 정지 n번 |
| `Cursor::BackwardTabulation(u32)` | `CSI n Z` | 역방향 탭 |
| `Cursor::TabulationClear(TabulationClear)` | `CSI n g` | 탭 정지 삭제 |
| `Cursor::ActivePositionReport { line, col }` | `CSI r;c R` | 현재 위치 보고 |
| `Cursor::RequestActivePositionReport` | `CSI 6 n` | 위치 쿼리 |
| `Cursor::SaveCursor` | `CSI s` | 커서 저장 |
| `Cursor::RestoreCursor` | `CSI u` | 커서 복원 |
| `Cursor::CursorStyle(CursorStyle)` | `CSI n SP q` | 커서 모양 변경 |

`CursorStyle`: `Default`, `BlinkingBlock`, `SteadyBlock`, `BlinkingUnderline`, `SteadyUnderline`, `BlinkingBar`, `SteadyBar`

```rust
match cursor {
    Cursor::Position { line, col } => {
        // line, col은 OneBased<u32>
        self.cursor_row = line.as_zero_based() as usize;
        self.cursor_col = col.as_zero_based() as usize;
    }
    Cursor::Up(n) => self.cursor_row = self.cursor_row.saturating_sub(*n as usize),
    Cursor::Down(n) => self.cursor_row += *n as usize,
    Cursor::Right(n) => self.cursor_col += *n as usize,
    Cursor::Left(n) => self.cursor_col = self.cursor_col.saturating_sub(*n as usize),
    _ => {}
}
```

### Edit — 내용 편집

```rust
use termwiz::escape::csi::Edit;
```

| 변형 | ESC 시퀀스 | 설명 |
|------|-----------|------|
| `Edit::EraseInLine(EraseInLine)` | `CSI n K` | 줄 지우기 |
| `Edit::EraseInDisplay(EraseInDisplay)` | `CSI n J` | 화면 지우기 |
| `Edit::InsertCharacter(u32)` | `CSI n @` | 커서 위치에 공백 n개 삽입 |
| `Edit::DeleteCharacter(u32)` | `CSI n P` | 커서 위치에서 n자 삭제 |
| `Edit::InsertLine(u32)` | `CSI n L` | 줄 n개 삽입 |
| `Edit::DeleteLine(u32)` | `CSI n M` | 줄 n개 삭제 |
| `Edit::ScrollUp(u32)` | `CSI n S` | 위로 n행 스크롤 |
| `Edit::ScrollDown(u32)` | `CSI n T` | 아래로 n행 스크롤 |
| `Edit::EraseCharacter(u32)` | `CSI n X` | 커서 위치에서 n자 지우기 |
| `Edit::Repeat(u32)` | `CSI n b` | 마지막 문자 n번 반복 |

`EraseInLine` 변형:
- `EraseInLine::EraseToEndOfLine` — 커서~줄 끝
- `EraseInLine::EraseToStartOfLine` — 줄 시작~커서
- `EraseInLine::EraseLine` — 전체 줄

`EraseInDisplay` 변형:
- `EraseInDisplay::EraseToEndOfDisplay` — 커서~화면 끝
- `EraseInDisplay::EraseToStartOfDisplay` — 화면 시작~커서
- `EraseInDisplay::EraseDisplay` — 전체 화면
- `EraseInDisplay::EraseScrollback` — 스크롤백 삭제

### Mode — 터미널 모드

```rust
use termwiz::escape::csi::{Mode, DecPrivateMode, DecPrivateModeCode};
```

| 변형 | 설명 |
|------|------|
| `Mode::SetDecPrivateMode(DecPrivateMode)` | `CSI ? n h` — DEC 모드 설정 |
| `Mode::ResetDecPrivateMode(DecPrivateMode)` | `CSI ? n l` — DEC 모드 해제 |
| `Mode::SaveDecPrivateMode(DecPrivateMode)` | `CSI ? n s` — DEC 모드 저장 |
| `Mode::RestoreDecPrivateMode(DecPrivateMode)` | `CSI ? n r` — DEC 모드 복원 |
| `Mode::SetMode(TerminalMode)` | `CSI n h` — ANSI 모드 설정 |
| `Mode::ResetMode(TerminalMode)` | `CSI n l` — ANSI 모드 해제 |
| `Mode::QueryDecPrivateMode(DecPrivateMode)` | `CSI ? n $p` — 모드 쿼리 |

---

## 5. DecPrivateModeCode

`termwiz::escape::csi::DecPrivateModeCode`는 DEC 전용 터미널 모드다.

```rust
use termwiz::escape::csi::DecPrivateModeCode;
```

| 변형 | 번호 | 설명 |
|------|------|------|
| `DecPrivateModeCode::ApplicationCursorKeys` | 1 | 커서 키를 앱 모드로 (`\x1b[A` → `\x1bOA`) |
| `DecPrivateModeCode::UsNationalCharacterSet` | 2 | ANSI/VT52 모드 선택 |
| `DecPrivateModeCode::SelectVt52Mode` | 2 | VT52 에뮬레이션 |
| `DecPrivateModeCode::ColumnMode` | 3 | 80/132 열 모드 |
| `DecPrivateModeCode::SmoothScroll` | 4 | 부드러운 스크롤 |
| `DecPrivateModeCode::ReverseVideo` | 5 | 전체 화면 반전 |
| `DecPrivateModeCode::OriginMode` | 6 | 스크롤 영역 기준 커서 위치 |
| `DecPrivateModeCode::AutoWrap` | 7 | 줄 끝에서 자동 줄바꿈 |
| `DecPrivateModeCode::AutoRepeat` | 8 | 키 자동 반복 |
| `DecPrivateModeCode::ShowCursor` | 25 | 커서 표시/숨김 |
| `DecPrivateModeCode::ReverseWraparound` | 45 | 역방향 줄바꿈 |
| `DecPrivateModeCode::Logging` | 46 | 로깅 |
| `DecPrivateModeCode::UseAlternateScreen` | 47 | 대체 화면 전환 |
| `DecPrivateModeCode::BracketedPaste` | 2004 | 괄호 붙이기 붙여넣기 |
| `DecPrivateModeCode::ClearAndEnableAlternateScreen` | 1049 | 대체 화면 활성화 + 커서 저장 (권장) |
| `DecPrivateModeCode::EnableAlternateScreen` | 1047 | 대체 화면만 활성화 |
| `DecPrivateModeCode::SaveCursorPosition` | 1048 | 커서 위치 저장 |
| `DecPrivateModeCode::OptEnableAlternateScreen` | 1049 | 1047 + 1048 조합 |

#### 마우스 트래킹 모드

| 변형 | 번호 | 설명 |
|------|------|------|
| `DecPrivateModeCode::X10Mouse` | 9 | X10 마우스 클릭만 |
| `DecPrivateModeCode::NormalMouse` | 1000 | 버튼 이벤트 |
| `DecPrivateModeCode::ButtonEventMouse` | 1002 | 버튼 누른 상태로 이동 |
| `DecPrivateModeCode::AnyEventMouse` | 1003 | 모든 마우스 이동 |
| `DecPrivateModeCode::Utf8Mouse` | 1005 | UTF-8 인코딩 마우스 |
| `DecPrivateModeCode::SGRMouse` | 1006 | SGR 형식 마우스 (권장) |
| `DecPrivateModeCode::UrxvtMouse` | 1015 | URXVT 형식 마우스 |
| `DecPrivateModeCode::SgrPixelMouse` | 1016 | SGR 픽셀 좌표 마우스 |

#### 포커스 트래킹

| 변형 | 번호 | 설명 |
|------|------|------|
| `DecPrivateModeCode::FocusTracking` | 1004 | 포커스 획득/상실 이벤트 (`\x1b[I` / `\x1b[O`) |

```rust
match mode {
    Mode::SetDecPrivateMode(DecPrivateMode::Code(code)) => {
        match code {
            DecPrivateModeCode::ShowCursor => self.show_cursor = true,
            DecPrivateModeCode::BracketedPaste => self.bracketed_paste = true,
            DecPrivateModeCode::ClearAndEnableAlternateScreen => {
                self.save_cursor();
                self.switch_to_alt_screen();
                self.clear_screen();
            }
            DecPrivateModeCode::FocusTracking => self.focus_tracking = true,
            DecPrivateModeCode::SGRMouse => self.mouse_encoding = MouseEncoding::SGR,
            DecPrivateModeCode::AnyEventMouse => self.mouse_tracking = MouseTracking::Any,
            _ => {}
        }
    }
    Mode::ResetDecPrivateMode(DecPrivateMode::Code(code)) => {
        match code {
            DecPrivateModeCode::ShowCursor => self.show_cursor = false,
            DecPrivateModeCode::ClearAndEnableAlternateScreen => {
                self.switch_to_primary_screen();
                self.restore_cursor();
            }
            _ => {}
        }
    }
    _ => {}
}
```

---

## 6. Esc / EscCode

`termwiz::escape::Esc`와 `EscCode`는 2바이트 ESC 시퀀스를 나타낸다.

```rust
use termwiz::escape::{Esc, EscCode};
```

### Esc 변형

| 변형 | 설명 |
|------|------|
| `Esc::Code(EscCode)` | 표준 EscCode |
| `Esc::Unspecified { intermediate, control }` | 알 수 없는 시퀀스 |

### EscCode 주요 변형

| 변형 | 시퀀스 | 설명 |
|------|--------|------|
| `EscCode::FullReset` | `ESC c` | 터미널 완전 초기화 (RIS) |
| `EscCode::DecSaveCursorPosition` | `ESC 7` | 커서 위치 및 속성 저장 (DECSC) |
| `EscCode::DecRestoreCursorPosition` | `ESC 8` | 커서 위치 및 속성 복원 (DECRC) |
| `EscCode::ReverseIndex` | `ESC M` | 커서를 위로 이동, 스크롤 영역 상단에서는 아래로 스크롤 (RI) |
| `EscCode::Index` | `ESC D` | 커서를 아래로 이동 (IND) |
| `EscCode::NextLine` | `ESC E` | 다음 줄 시작으로 이동 (NEL) |
| `EscCode::HorizontalTabSet` | `ESC H` | 탭 정지 설정 (HTS) |
| `EscCode::DecLineDrawing` | `ESC (0` | 줄 그리기 문자 집합 G0 |
| `EscCode::AsciiCharacterSet` | `ESC (B` | ASCII 문자 집합 G0 |
| `EscCode::DecDoubleHeightTopHalf` | `ESC # 3` | 이중 높이 위 절반 |
| `EscCode::DecDoubleHeightBottomHalf` | `ESC # 4` | 이중 높이 아래 절반 |
| `EscCode::DecSingleWidthLine` | `ESC # 5` | 단일 너비 줄 |
| `EscCode::DecDoubleWidthLine` | `ESC # 6` | 이중 너비 줄 |
| `EscCode::DecScreenAlignmentDisplay` | `ESC # 8` | 화면 정렬 테스트 |
| `EscCode::ApplicationKeypad` | `ESC =` | 애플리케이션 키패드 모드 |
| `EscCode::NormalKeypad` | `ESC >` | 일반 키패드 모드 |

```rust
match esc {
    Esc::Code(EscCode::FullReset) => {
        self.reset_to_initial_state();
    }
    Esc::Code(EscCode::ReverseIndex) => {
        if self.cursor_row == self.scroll_top {
            self.scroll_down(1); // 스크롤 영역 내용을 아래로
        } else {
            self.cursor_row = self.cursor_row.saturating_sub(1);
        }
    }
    Esc::Code(EscCode::DecSaveCursorPosition) => {
        self.saved_cursor = Some((self.cursor_row, self.cursor_col, self.attrs.clone()));
    }
    Esc::Code(EscCode::DecRestoreCursorPosition) => {
        if let Some((row, col, attrs)) = self.saved_cursor.take() {
            self.cursor_row = row;
            self.cursor_col = col;
            self.attrs = attrs;
        }
    }
    _ => {}
}
```

---

## 7. Surface

`termwiz::surface::Surface`는 터미널 화면 내용을 나타내는 셀 버퍼다.

### 생성

```rust
use termwiz::surface::Surface;

let mut surface = Surface::new(80, 24); // (cols, rows)
```

### `add_change(change)`

`Change` 명령을 버퍼에 추가한다.

```rust
use termwiz::surface::Change;
use termwiz::cell::AttributeChange;
use termwiz::color::{ColorAttribute, AnsiColor};

surface.add_change(Change::Text("Hello, World!".to_string()));
surface.add_change(Change::Attribute(AttributeChange::Foreground(
    ColorAttribute::TrueColorWithDefaultFallback(
        termwiz::color::SrgbaTuple::from((255, 0, 0, 255))
    ),
)));
```

### `screen_lines() -> &[Line]`

현재 화면의 모든 줄을 반환한다.

```rust
for (row_idx, line) in surface.screen_lines().iter().enumerate() {
    for (col_idx, cell) in line.cells().iter().enumerate() {
        let ch = cell.str();
        let attrs = cell.attrs();
        // 렌더링
    }
}
```

### `cursor_position() -> (usize, usize)`

현재 커서 위치를 `(col, row)` 순서로 반환한다.

```rust
let (col, row) = surface.cursor_position();
```

### `resize(cols, rows)`

표면 크기를 변경한다. 내용은 유지되며 범위를 벗어난 부분은 잘린다.

```rust
surface.resize(120, 40);
```

### `diff(other) -> Vec<Change>`

두 Surface의 차이를 Change 목록으로 반환한다. 효율적인 화면 갱신에 사용한다.

```rust
let changes = old_surface.diff(&new_surface);
for change in changes {
    apply_change_to_renderer(&change);
}
```

---

## 8. Change / Position / CellAttributes

### Change

`termwiz::surface::Change`는 Surface에 적용 가능한 변경 명령이다.

```rust
use termwiz::surface::Change;
```

| 변형 | 설명 |
|------|------|
| `Change::Text(String)` | 현재 커서 위치에 텍스트 출력 |
| `Change::Attribute(AttributeChange)` | 속성 변경 (색상, 볼드 등) |
| `Change::AllAttributes(CellAttributes)` | 모든 속성을 한 번에 설정 |
| `Change::CursorPosition { x, y }` | 커서 이동 |
| `Change::CursorColor(ColorAttribute)` | 커서 색상 |
| `Change::CursorShape(CursorShape)` | 커서 모양 |
| `Change::CursorVisibility(CursorVisibility)` | 커서 표시 여부 |
| `Change::ClearScreen(ColorAttribute)` | 화면 지우기 |
| `Change::ClearToEndOfLine(ColorAttribute)` | 줄 끝까지 지우기 |
| `Change::ClearToEndOfScreen(ColorAttribute)` | 화면 끝까지 지우기 |
| `Change::ScrollRegionUp { first_row, last_row, count }` | 스크롤 영역 위로 |
| `Change::ScrollRegionDown { first_row, last_row, count }` | 스크롤 영역 아래로 |
| `Change::Title(String)` | 터미널 창 제목 |
| `Change::Image(ImageCell)` | 인라인 이미지 |

### Position

`termwiz::surface::Position`은 커서 위치 지정에 사용한다.

```rust
use termwiz::surface::Position;

Position::Absolute(0)    // 절대 위치 (0-기반)
Position::Relative(-1)   // 상대 위치
NoChange                 // 변경 없음
```

```rust
surface.add_change(Change::CursorPosition {
    x: Position::Absolute(10),
    y: Position::Absolute(5),
});
```

### CellAttributes

`termwiz::cell::CellAttributes`는 셀의 텍스트 속성을 보유한다.

```rust
use termwiz::cell::{CellAttributes, Intensity, Underline, Blink};
use termwiz::color::{ColorAttribute, AnsiColor};

let mut attrs = CellAttributes::default();
attrs.set_intensity(Intensity::Bold);
attrs.set_foreground(ColorAttribute::PaletteIndex(1)); // 빨간색
attrs.set_background(ColorAttribute::Default);
attrs.set_underline(Underline::Single);
attrs.set_italic(true);
attrs.set_reverse(false);
attrs.set_strikethrough(false);
```

접근자:
```rust
let intensity = attrs.intensity();       // Intensity
let fg = attrs.foreground();             // ColorAttribute
let bg = attrs.background();             // ColorAttribute
let underline = attrs.underline();       // Underline
let is_italic = attrs.italic();          // bool
let is_reverse = attrs.reverse();        // bool
```

---

## 9. 전체 파싱 루프 예시

PTY 출력을 읽어 터미널 상태에 반영하는 완전한 루프.

```rust
use termwiz::escape::parser::Parser;
use termwiz::escape::{Action, ControlCode};
use termwiz::escape::csi::{CSI, Cursor, Edit, Mode, Sgr};
use termwiz::escape::csi::{DecPrivateMode, DecPrivateModeCode};
use termwiz::escape::{Esc, EscCode};

pub struct TerminalState {
    parser: Parser,
    cols: usize,
    rows: usize,
    cursor_col: usize,
    cursor_row: usize,
    scroll_top: usize,
    scroll_bottom: usize,
    // ... 기타 상태
}

impl TerminalState {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            parser: Parser::new(),
            cols,
            rows,
            cursor_col: 0,
            cursor_row: 0,
            scroll_top: 0,
            scroll_bottom: rows - 1,
        }
    }

    /// PTY에서 읽은 바이트를 처리한다. 상태 유지를 위해 Parser를 재사용한다.
    pub fn process_bytes(&mut self, bytes: &[u8]) {
        // parse()에 &mut self를 넘길 수 없으므로 actions로 수집 후 처리
        let actions: Vec<Action> = self.parser.parse_as_vec(bytes);
        for action in actions {
            self.handle_action(action);
        }
    }

    fn handle_action(&mut self, action: Action) {
        match action {
            Action::Print(c) => {
                self.print_char(c);
            }
            Action::PrintString(s) => {
                for c in s.chars() {
                    self.print_char(c);
                }
            }
            Action::Control(ctrl) => {
                self.handle_control(ctrl);
            }
            Action::CSI(csi) => {
                self.handle_csi(csi);
            }
            Action::Esc(esc) => {
                self.handle_esc(esc);
            }
            Action::OperatingSystemCommand(osc) => {
                self.handle_osc(*osc);
            }
            _ => {}
        }
    }

    fn handle_control(&mut self, ctrl: ControlCode) {
        match ctrl {
            ControlCode::LineFeed
            | ControlCode::VerticalTab
            | ControlCode::FormFeed => {
                self.linefeed();
            }
            ControlCode::CarriageReturn => {
                self.cursor_col = 0;
            }
            ControlCode::Backspace => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
            }
            ControlCode::HorizontalTab => {
                // 다음 탭 정지로 이동
                self.cursor_col = (self.cursor_col / 8 + 1) * 8;
                self.cursor_col = self.cursor_col.min(self.cols - 1);
            }
            ControlCode::Bell => {
                // 벨 처리 (시스템 알림 또는 무시)
            }
            _ => {}
        }
    }

    fn handle_csi(&mut self, csi: CSI) {
        match csi {
            CSI::Sgr(sgr) => self.handle_sgr(sgr),
            CSI::Cursor(cursor) => self.handle_cursor(cursor),
            CSI::Edit(edit) => self.handle_edit(edit),
            CSI::Mode(mode) => self.handle_mode(mode),
            _ => {}
        }
    }

    fn handle_cursor(&mut self, cursor: Cursor) {
        use termwiz::escape::csi::OneBased;
        match cursor {
            Cursor::Up(n) => {
                self.cursor_row = self.cursor_row.saturating_sub(n as usize);
            }
            Cursor::Down(n) => {
                self.cursor_row = (self.cursor_row + n as usize).min(self.rows - 1);
            }
            Cursor::Right(n) => {
                self.cursor_col = (self.cursor_col + n as usize).min(self.cols - 1);
            }
            Cursor::Left(n) => {
                self.cursor_col = self.cursor_col.saturating_sub(n as usize);
            }
            Cursor::Position { line, col } => {
                self.cursor_row = (line.as_zero_based() as usize).min(self.rows - 1);
                self.cursor_col = (col.as_zero_based() as usize).min(self.cols - 1);
            }
            _ => {}
        }
    }

    fn handle_mode(&mut self, mode: Mode) {
        match mode {
            Mode::SetDecPrivateMode(DecPrivateMode::Code(code)) => {
                match code {
                    DecPrivateModeCode::ShowCursor => { /* 커서 표시 */ }
                    DecPrivateModeCode::ClearAndEnableAlternateScreen => {
                        // 대체 화면 전환
                    }
                    DecPrivateModeCode::BracketedPaste => {
                        // 괄호 붙이기 활성화
                    }
                    _ => {}
                }
            }
            Mode::ResetDecPrivateMode(DecPrivateMode::Code(code)) => {
                match code {
                    DecPrivateModeCode::ShowCursor => { /* 커서 숨김 */ }
                    DecPrivateModeCode::ClearAndEnableAlternateScreen => {
                        // 기본 화면으로 복귀
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn handle_esc(&mut self, esc: Esc) {
        match esc {
            Esc::Code(EscCode::FullReset) => {
                self.full_reset();
            }
            Esc::Code(EscCode::ReverseIndex) => {
                if self.cursor_row == self.scroll_top {
                    // 스크롤 영역 아래로 스크롤
                } else {
                    self.cursor_row -= 1;
                }
            }
            Esc::Code(EscCode::DecSaveCursorPosition) => {
                // 커서 저장
            }
            Esc::Code(EscCode::DecRestoreCursorPosition) => {
                // 커서 복원
            }
            _ => {}
        }
    }

    fn handle_sgr(&mut self, _sgr: Sgr) {
        // 색상/속성 처리
    }

    fn handle_edit(&mut self, _edit: Edit) {
        // 편집 처리
    }

    fn handle_osc(
        &mut self,
        _osc: termwiz::escape::OperatingSystemCommand,
    ) {
        // OSC 처리 (창 제목, 색상 쿼리 등)
    }

    fn print_char(&mut self, _c: char) {
        // 문자 출력 후 커서 전진
    }

    fn linefeed(&mut self) {
        if self.cursor_row == self.scroll_bottom {
            // 스크롤
        } else {
            self.cursor_row += 1;
        }
    }

    fn full_reset(&mut self) {
        self.cursor_col = 0;
        self.cursor_row = 0;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
    }
}
```

### PTY 읽기 스레드와의 통합

```rust
use std::sync::{Arc, Mutex};

let state = Arc::new(Mutex::new(TerminalState::new(80, 24)));
let state_clone = Arc::clone(&state);

std::thread::spawn(move || {
    let mut buf = [0u8; 4096];
    loop {
        match pty_reader.read(&mut buf) {
            Ok(0) => break, // PTY 종료
            Ok(n) => {
                let mut s = state_clone.lock().unwrap();
                s.process_bytes(&buf[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::yield_now();
            }
            Err(_) => break,
        }
    }
});
```
