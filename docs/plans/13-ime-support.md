# 13. IME 지원

## cmux 구현 방식

- NSTextInputClient 프로토콜로 CJK IME 완전 지원
- Shift+Space 토글 처리
- 브라우저 주소창 IME
- macOS 받아쓰기 지원

## 크로스 플랫폼 구현 방안

### 네이티브 GUI에서의 IME

**네이티브 GUI 앱이므로 IME를 직접 처리한다.**
winit의 IME 이벤트를 통해 조합 문자 표시, 확정 문자 입력을 구현한다.

### winit IME 이벤트

```rust
WindowEvent::Ime(ime_event) => {
    match ime_event {
        Ime::Enabled => { /* IME 활성화 */ },
        Ime::Preedit(text, cursor) => {
            // 조합 중인 텍스트 표시 (예: "ㅎㅏㄴ")
            // cursor: 조합 커서 위치
        },
        Ime::Commit(text) => {
            // 완성된 텍스트 → PTY에 전달 (예: "한")
        },
        Ime::Disabled => { /* IME 비활성화 */ },
    }
}
```

### 구현 요소

| 요소 | 설명 |
|------|------|
| 조합 문자 인라인 표시 | Preedit 텍스트를 커서 위치에 밑줄/하이라이트로 표시 |
| IME 윈도우 위치 | `window.set_ime_cursor_area()`로 후보 윈도우 위치 지정 |
| 커밋 텍스트 전달 | 완성된 텍스트를 PTY에 UTF-8 바이트로 전송 |
| 와이드 문자 폭 | CJK 문자 2셀 폭 — `unicode-width` 크레이트 |
| 자체 입력 필드 | 명령 팔레트, 검색 바 등 GUI 입력 필드에서도 IME 지원 |

### OS별 IME 백엔드

| OS | IME 시스템 | winit 지원 |
|----|----------|-----------|
| **Windows** | TSF (Text Services Framework) / IMM32 | ✅ winit가 처리 |
| **macOS** | NSTextInputClient | ✅ winit가 처리 |
| **Linux (X11)** | XIM / IBus / Fcitx | ✅ winit가 처리 |
| **Linux (Wayland)** | text-input-v3 프로토콜 | ⚠️ winit 지원 진행 중 |

### 주의사항

- **Wayland IME**: winit의 Wayland IME 지원은 아직 완벽하지 않을 수 있다. `text-input-v3` 프로토콜 구현 상태를 추적해야 한다.
- **커서 위치 정확도**: 조합 문자 표시를 위해 셀 좌표 → 픽셀 좌표 변환이 정확해야 한다.
- **폰트 폴백**: CJK 글리프가 없는 폰트에서 자동으로 CJK 폰트로 폴백해야 한다.

### 참고 구현

WezTerm이 winit 기반 IME를 가장 잘 구현한 참고 사례이다.
Alacritty도 winit IME를 사용하지만 한글 조합 표시에 이슈가 있었던 이력이 있다.

## 최적화 전략

- **Preedit 렌더링 최적화**: 조합 중인 문자 영역만 재렌더링한다. 전체 셀 그리드를 다시 그리지 않고 preedit 오버레이만 업데이트한다.
- **IME 윈도우 위치 캐싱**: 커서 위치가 변경될 때만 IME 후보창 위치를 업데이트한다. 동일 셀에서 조합이 계속될 때 `set_ime_cursor_area` 호출을 스킵한다.
- **이벤트 필터링**: IME 비활성 시 관련 이벤트 처리를 완전 스킵한다. `Ime::Disabled` 상태에서는 preedit 관련 코드 경로를 타지 않는다.

## 구현 가능 여부

| OS | 가능 여부 | 비고 |
|----|-----------|------|
| Windows | ✅ 가능 | TSF/IMM32, winit 완전 지원 |
| macOS | ✅ 가능 | NSTextInputClient, winit 완전 지원 |
| Linux (X11) | ✅ 가능 | XIM/IBus/Fcitx, winit 지원 |
| Linux (Wayland) | ⚠️ 제한적 | text-input-v3 지원 진행 중 |

네이티브 GUI이므로 cmux와 동등한 수준의 IME 지원이 가능하지만, 직접 구현해야 하는 복잡도가 있다.
