# 12. 클립보드 통합

## cmux 구현 방식

- 클립보드 이미지 붙여넣기
- X11 스타일 미들클릭 선택 붙여넣기
- URL/파일 드롭 시 셸 이스케이프
- HTML/RTF 붙여넣기 변환

## 크로스 플랫폼 구현 방안

### 네이티브 GUI의 이점

네이티브 GUI 앱이므로 OS 클립보드에 직접 접근할 수 있다.

### OS별 클립보드 접근

| OS | 텍스트 | 이미지 | Rust 크레이트 |
|----|--------|--------|--------------|
| **Windows** | Win32 Clipboard API | ✅ BMP/PNG | `clipboard-win` 또는 `arboard` |
| **macOS** | NSPasteboard | ✅ PNG/TIFF | `arboard` |
| **Linux (X11)** | XCB selection | ✅ PNG | `arboard` (x11) |
| **Linux (Wayland)** | wl-clipboard 프로토콜 | ✅ PNG | `arboard` (wayland) |

`arboard` 크레이트가 텍스트와 이미지 클립보드를 세 OS에서 통합 지원한다.

### 복사/붙여넣기 키 방식

세 가지 방식을 제공하며, 설정에서 독립적으로 ON/OFF 할 수 있다.

| 방식 | 복사 | 붙여넣기 | 기본 ON |
|------|------|----------|---------|
| macOS 방식 | Alt+C | Alt+V | macOS만 |
| Linux 방식 | Ctrl+Shift+C | Ctrl+Shift+V | Linux만 |
| Windows 방식 | Ctrl+C (선택 있을 때) | Ctrl+V | Windows만 |

- 세 가지를 모두 활성화하면 모든 키 조합이 복사/붙여넣기로 동작한다
- Windows 방식이 ON이고 선택이 없을 때: Ctrl+C → PTY 전달 (SIGINT)
- 상세 키 정책은 [10-keyboard-shortcuts.md](10-keyboard-shortcuts.md) 참조

### 구현 기능

| 기능 | 구현 방법 |
|------|----------|
| 텍스트 복사 | 선택 영역 텍스트 → `arboard` 클립보드 쓰기 |
| 텍스트 붙여넣기 | `arboard` 클립보드 읽기 → PTY 입력 전송 |
| 이미지 붙여넣기 | 클립보드 이미지 → base64 인코딩 후 PTY 전송 (앱별 처리) |
| URL/파일 드롭 | winit의 `WindowEvent::DroppedFile` → 셸 이스케이프 후 PTY 전송 |
| 선택 즉시 복사 | 마우스 드래그 선택 완료 시 자동으로 클립보드에 복사 (설정 가능) |
| 미들클릭 붙여넣기 | Linux X11 primary selection 지원 |
| 멀티라인 경고 | 여러 줄 붙여넣기 시 확인 대화상자 (보안) |
| 브래킷 붙여넣기 | Bracketed Paste Mode (CSI ? 2004 h/l) 지원 |

### OSC 52 지원

터미널 에뮬레이터로서 자식 프로세스의 OSC 52 시퀀스도 처리한다. termwiz가 OSC 52를 파싱한다.

```
\x1b]52;c;{base64_encoded_text}\x07
```

자식 프로세스(셸, vim, tmux 등)가 OSC 52를 보내면 tasty가 OS 클립보드에 복사한다.

**보안 정책:**

| 동작 | 기본 설정 | 설명 |
|------|----------|------|
| 쓰기 (클립보드 설정) | 허용 | 자식 프로세스가 클립보드에 텍스트를 설정 |
| 읽기 (클립보드 조회) | 차단 | 자식 프로세스가 클립보드 내용을 읽는 것을 차단 (보안) |
| 크기 제한 | 1MB | base64 디코딩 전 최대 크기 |

설정에서 `[clipboard] osc52_write = true`, `osc52_read = false`로 제어한다.

### 브래킷 붙여넣기 (Bracketed Paste)

셸이 Bracketed Paste Mode를 활성화하면 (`CSI ? 2004 h`), 붙여넣기 시 텍스트를 `\x1b[200~` ... `\x1b[201~`로 감싸서 PTY에 전송한다. 이를 통해 셸이 붙여넣기 텍스트를 일반 입력과 구분할 수 있다.

## 최적화 전략

- **대용량 붙여넣기 쓰로틀링**: 대량 텍스트 붙여넣기 시 PTY에 청크 단위로 전송하여 셸 버퍼 오버플로를 방지한다. 청크 간에 적절한 딜레이를 둔다.
- **이미지 변환 캐싱**: 클립보드 이미지 포맷 변환 결과를 캐싱한다. 동일 이미지에 대한 반복 변환을 방지한다.
- **비동기 클립보드 접근**: 일부 OS에서 클립보드 접근이 느릴 수 있으므로 별도 스레드에서 처리한다. 특히 Windows에서 클립보드 잠금 대기가 발생할 수 있다.

## 구현 가능 여부

| OS | 텍스트 | 이미지 | 드래그앤드롭 | 비고 |
|----|--------|--------|------------|------|
| Windows | ✅ | ✅ | ✅ | 완전 지원 |
| macOS | ✅ | ✅ | ✅ | 완전 지원 |
| Linux | ✅ | ✅ | ✅ | X11/Wayland 모두 지원 |

네이티브 GUI이므로 cmux와 동등한 클립보드 통합이 가능하다.
