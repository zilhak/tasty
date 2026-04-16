# Linux: New Window 접근성 개선 TODO

## 1. ~~마지막 윈도우 닫기 동작~~ (구현 완료)

`set_minimized(true)`로 태스크바에 유지하도록 구현됨. Windows와 동일.

## 2. .desktop 파일에 Actions 추가

앱 아이콘 우클릭 시 "New Window" 표시 (GNOME, KDE 모두 지원).

```ini
[Desktop Action new-window]
Name=New Window
Exec=tasty new window
```

- 코드 변경 없이 패키징 시 `.desktop` 파일에 추가하면 됨
- `tasty new window` CLI가 IPC로 `window.create`를 호출하여 기존 인스턴스에 새 윈도우 생성

## 3. System Tray 아이콘 (선택사항)

Windows와 동일. 백그라운드 실행 시 시스템 트레이에 아이콘 표시.

- DE마다 트레이 지원이 다름 (GNOME은 확장 필요, KDE는 네이티브)
- 우선순위 낮음

## 4. Explorer 파일 클립보드 — text/uri-list 구현

Explorer 다중 선택 기능은 구현 완료되었으나, Linux에서 OS 파일 탐색기(Nautilus, Dolphin, Thunar 등)와의 클립보드 호환은 아직 미구현.

**현재 상태**: `src/file_clipboard/linux.rs`에 stub만 존재. `set_file_clipboard()`와 `get_file_clipboard()` 모두 에러 반환.

**인터페이스** (macOS와 동일, 이미 정의됨):
```rust
pub fn set_file_clipboard(paths: &[&str], op: FileClipboardOp) -> Result<(), String>
pub fn get_file_clipboard() -> Result<Option<(Vec<String>, FileClipboardOp)>, String>
```

### 클립보드 포맷 (두 가지 동시 설정 필요)

Linux 파일 탐색기들은 X11/Wayland 클립보드에 **두 가지 MIME 타입**으로 파일 목록을 저장한다:

1. **`text/uri-list`** (표준):
   - RFC 2483 형식. 각 파일을 `file://` URI로 변환하여 `\r\n` 구분
   - 예: `file:///home/user/doc.txt\r\nfile:///home/user/img.png\r\n`
   - 공백/특수문자는 percent-encoding 적용 (예: `Hello World.txt` → `Hello%20World.txt`)

2. **`x-special/gnome-copied-files`** (GNOME/GTK 확장, Copy/Cut 구분):
   - 첫 줄: `copy` 또는 `cut` (복사/잘라내기 구분)
   - 이후 줄: `file://` URI (줄바꿈 `\n` 구분)
   - 예:
     ```
     copy
     file:///home/user/doc.txt
     file:///home/user/img.png
     ```
   - Nautilus, Thunar, Nemo, Caja 등 GTK 기반 파일 탐색기가 이 포맷을 사용
   - KDE Dolphin은 `application/x-kde-cutselection` 포맷을 추가로 사용하지만, `x-special/gnome-copied-files`도 읽을 수 있음

### 구현 접근법

**방법 A: `wl-clipboard` / `xclip` CLI 도구 사용 (간단)**:
```bash
# 쓰기 (Wayland)
echo -e "copy\nfile:///path/to/file" | wl-copy --type "x-special/gnome-copied-files"

# 읽기 (Wayland)
wl-paste --type "x-special/gnome-copied-files"

# 쓰기 (X11)
echo -e "copy\nfile:///path/to/file" | xclip -selection clipboard -t "x-special/gnome-copied-files"

# 읽기 (X11)
xclip -selection clipboard -t "x-special/gnome-copied-files" -o
```

- 장점: 구현이 간단, Wayland/X11 자동 대응
- 단점: `wl-copy`/`xclip` 외부 의존성 필요, 서브프로세스 실행 오버헤드
- `std::process::Command`로 호출 가능. Wayland면 `wl-copy`/`wl-paste`, X11이면 `xclip` 사용
- `$WAYLAND_DISPLAY` 환경변수로 Wayland/X11 판별

**방법 B: X11 직접 접근 (`x11-dl` 또는 `x11rb` 크레이트)**:
- `XSetSelectionOwner`, `XConvertSelection`, `SelectionRequest` 이벤트 처리
- MIME 타입을 X11 Atom으로 등록: `XInternAtom(display, "text/uri-list", ...)`, `XInternAtom(display, "x-special/gnome-copied-files", ...)`
- `CLIPBOARD` selection에 데이터 설정
- 장점: 외부 의존성 없음
- 단점: X11 전용, Wayland 미지원, 이벤트 루프 통합 복잡

**방법 C: `arboard` 크레이트 확장** (현재 텍스트 클립보드에 사용 중):
- `arboard`는 현재 텍스트/이미지만 지원하며 커스텀 MIME 타입을 지원하지 않음
- 직접 사용은 불가능

**권장**: 방법 A (`wl-clipboard`/`xclip` CLI). 가장 적은 코드로 Wayland/X11 모두 지원 가능.

### set_file_clipboard 구현 순서

```
1. 파일 경로를 file:// URI로 변환 (특수문자 percent-encoding)
2. x-special/gnome-copied-files 포맷 문자열 생성:
   "{copy|cut}\nfile:///path1\nfile:///path2\n..."
3. text/uri-list 포맷 문자열 생성:
   "file:///path1\r\nfile:///path2\r\n"
4. $WAYLAND_DISPLAY 확인하여 Wayland/X11 판별
5. Wayland:
   - wl-copy --type "x-special/gnome-copied-files" 로 stdin에 전달
   - wl-copy --type "text/uri-list" 로 stdin에 전달 (두 번째 호출)
   X11:
   - xclip -selection clipboard -t "x-special/gnome-copied-files" 로 stdin에 전달
```

**주의**: `wl-copy`는 한 번에 하나의 MIME 타입만 설정할 수 있다. 여러 MIME 타입을 동시에 제공하려면 `wl-copy --type "x-special/gnome-copied-files" --type "text/uri-list"` 형태가 필요하지만 이 문법은 지원되지 않을 수 있다. 이 경우 `x-special/gnome-copied-files`만 설정해도 Nautilus/Thunar/Dolphin 모두 동작한다.

### get_file_clipboard 구현 순서

```
1. $WAYLAND_DISPLAY 확인하여 Wayland/X11 판별
2. Wayland:
   - wl-paste --type "x-special/gnome-copied-files" 실행
   X11:
   - xclip -selection clipboard -t "x-special/gnome-copied-files" -o 실행
3. 출력 파싱:
   - 첫 줄: "copy" → FileClipboardOp::Copy, "cut" → FileClipboardOp::Cut
   - 나머지 줄: file:// URI → 로컬 경로로 변환 (percent-decoding)
4. x-special/gnome-copied-files 없으면 text/uri-list로 폴백
   - 이 경우 Copy/Cut 구분 불가 → 기본 Copy로 처리
```

### 호출 경로 (이미 구현됨)

Explorer UI(`src/explorer_ui.rs`)에서 이미 다중 선택을 지원하며, 클립보드 호출 코드도 완성되어 있다:

```rust
// 복사 (Ctrl+C) / 잘라내기 (Ctrl+X)
let paths: Vec<&str> = panel.selected_files.iter().map(|s| s.as_str()).collect();
let _ = crate::file_clipboard::set_file_clipboard(&paths, op);

// 붙여넣기 (Ctrl+V)
if let Ok(Some((sources, op))) = crate::file_clipboard::get_file_clipboard() { ... }
```

`linux.rs`의 두 함수만 구현하면 Explorer 다중 선택 → 복사/잘라내기 → 붙여넣기가 Nautilus/Dolphin 등과 양방향으로 호환된다.

### percent-encoding 참고

macOS 구현(`src/file_clipboard/macos.rs`)에 이미 `percent_decode()` 함수가 있으므로, 공통 유틸로 추출하거나 linux.rs에서 동일 로직을 사용할 수 있다. 인코딩은 `%XX` 형태로 비ASCII/공백/특수문자를 변환하면 된다.
