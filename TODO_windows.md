# Windows: New Window 접근성 개선 TODO

## 1. ~~마지막 윈도우 닫기 동작~~ (구현 완료)

`set_minimized(true)`로 태스크바에 유지하도록 구현됨. Windows/Linux에서는 윈도우를 파괴하지 않고 최소화.

## 2. 태스크바 Jump List에 "New Window" 추가

Windows 태스크바 아이콘 우클릭 시 "New Window" 항목 표시.

- `ICustomDestinationList` COM API 사용
- 클릭 시 `tasty new window` CLI 명령 실행 (이미 IPC로 `window.create` 지원됨)
- 별도 COM 코드 필요, 작업량 있음

## 3. System Tray 아이콘 (선택사항)

백그라운드 실행 시 시스템 트레이에 아이콘 표시.

- "Show Window", "New Window", "Quit" 메뉴 제공
- `tray-icon` 또는 유사 크레이트 필요
- Jump List만으로 충분할 수 있으므로 우선순위 낮음

## 4. Explorer 파일 클립보드 — CF_HDROP 구현

Explorer 다중 선택 기능은 구현 완료되었으나, Windows에서 OS 파일 탐색기(Windows Explorer)와의 클립보드 호환은 아직 미구현.

**현재 상태**: `src/file_clipboard/windows.rs`에 stub만 존재. `set_file_clipboard()`와 `get_file_clipboard()` 모두 에러 반환.

**인터페이스** (macOS와 동일, 이미 정의됨):
```rust
pub fn set_file_clipboard(paths: &[&str], op: FileClipboardOp) -> Result<(), String>
pub fn get_file_clipboard() -> Result<Option<(Vec<String>, FileClipboardOp)>, String>
```

### set_file_clipboard 구현 방법

Windows Explorer가 인식하는 파일 복사/잘라내기를 위해 **두 가지 클립보드 포맷**을 동시에 설정해야 한다:

1. **CF_HDROP** (파일 목록):
   - `OpenClipboard()`, `EmptyClipboard()`, `SetClipboardData()`, `CloseClipboard()` Win32 API 사용
   - `DROPFILES` 구조체 + 널 종단 UTF-16 파일 경로 배열을 `GlobalAlloc`으로 할당
   - `DROPFILES.pFiles`는 구조체 끝에서 파일 경로 데이터까지의 오프셋
   - `DROPFILES.fWide = TRUE` (UTF-16 사용)
   - 파일 경로는 각각 널(\0) 종단, 마지막에 이중 널(\0\0)로 끝남

2. **Preferred DropEffect** (복사/잘라내기 구분):
   - `RegisterClipboardFormatW(L"Preferred DropEffect")`로 커스텀 포맷 등록
   - `DROPEFFECT_COPY = 1` (복사) 또는 `DROPEFFECT_MOVE = 2` (잘라내기)
   - 4바이트 DWORD 값을 `GlobalAlloc`으로 할당하여 `SetClipboardData()`에 전달

**참고 구현 순서**:
```
1. RegisterClipboardFormatW("Preferred DropEffect")로 포맷 ID 확보
2. OpenClipboard(NULL)
3. EmptyClipboard()
4. DROPFILES + UTF-16 파일 경로 배열을 GlobalAlloc으로 할당 → SetClipboardData(CF_HDROP, ...)
5. DWORD DropEffect 값을 GlobalAlloc으로 할당 → SetClipboardData(preferred_drop_effect_format, ...)
6. CloseClipboard()
```

**사용할 크레이트**: `windows` 크레이트 (이미 Cargo.toml에 의존성 있음) 또는 raw Win32 FFI.

### get_file_clipboard 구현 방법

1. `OpenClipboard(NULL)`
2. `IsClipboardFormatAvailable(CF_HDROP)`로 파일 데이터 존재 여부 확인
3. `GetClipboardData(CF_HDROP)`로 `HDROP` 핸들 획득
4. `DragQueryFileW(hdrop, 0xFFFFFFFF, NULL, 0)`으로 파일 수 조회
5. 각 인덱스에 대해 `DragQueryFileW(hdrop, i, buffer, buffer_len)`으로 파일 경로 추출
6. `GetClipboardData(preferred_drop_effect_format)`으로 Copy/Cut 구분
7. `CloseClipboard()`

### 호출 경로 (이미 구현됨)

Explorer UI(`src/explorer_ui.rs`)에서 이미 다중 선택을 지원하며, 클립보드 호출 코드도 완성되어 있다:

```rust
// 복사 (Ctrl+C) / 잘라내기 (Ctrl+X)
let paths: Vec<&str> = panel.selected_files.iter().map(|s| s.as_str()).collect();
let _ = crate::file_clipboard::set_file_clipboard(&paths, op);

// 붙여넣기 (Ctrl+V)
if let Ok(Some((sources, op))) = crate::file_clipboard::get_file_clipboard() { ... }
```

`windows.rs`의 두 함수만 구현하면 Explorer 다중 선택 → 복사/잘라내기 → 붙여넣기가 Windows Explorer와 양방향으로 호환된다.
