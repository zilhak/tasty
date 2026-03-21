# 04. 알림 시스템

## cmux 구현 방식

- 터미널 패인에 파란색 알림 링
- 사이드바 탭 하이라이트
- macOS UNUserNotificationCenter 연동
- OSC 9/99/777 터미널 시퀀스 감지
- `cmux notify` CLI
- 커스텀 사운드, 커스텀 명령 실행
- Claude Code 훅 연동

## 크로스 플랫폼 구현 방안

### 인앱 알림 (GPU 렌더링)

네이티브 GUI이므로 풍부한 시각적 알림이 가능:

- **패인 글로우 효과**: 셰이더로 패인 테두리에 부드러운 발광 효과 (cmux의 파란 링과 동등)
- **사이드바 뱃지**: 알림 카운트 뱃지, 탭 배경색 변경
- **토스트 알림**: 앱 내부에 일시적 토스트 메시지 오버레이
- **알림 패널**: 전체 알림 히스토리를 보여주는 슬라이드 패널

### 시스템 트레이

| OS | 시스템 트레이 | Rust 크레이트 |
|----|-------------|--------------|
| **Windows** | 시스템 트레이 아이콘 + 풍선 알림 | `tray-icon` |
| **macOS** | 메뉴바 아이콘 + 배너 알림 | `tray-icon` |
| **Linux** | 시스템 트레이 (StatusNotifierItem) | `tray-icon` |

### OS 네이티브 알림

| OS | API | Rust 크레이트 |
|----|-----|--------------|
| **Windows** | Toast Notification API (WinRT) | `winrt-notification` |
| **macOS** | `UNUserNotificationCenter` | `mac-notification-sys` 또는 `notify-rust` |
| **Linux** | D-Bus `org.freedesktop.Notifications` | `notify-rust` |

`notify-rust` 크레이트가 세 OS를 모두 지원한다.

### OSC 시퀀스 감지

터미널 에뮬레이터로서 termwiz가 모든 이스케이프 시퀀스를 직접 처리한다. termwiz는 OSC 시퀀스를 기본 지원한다.

| 시퀀스 | 용도 | 파싱 난이도 |
|--------|------|-----------|
| OSC 9 | iTerm2 알림 | 단순 |
| OSC 99 | Kitty 알림 (key=value) | 중간 |
| OSC 777 | rxvt 알림 | 단순 |

### 사운드

| OS | 방법 |
|----|------|
| **Windows** | `PlaySound` Win32 API 또는 `rodio` 크레이트 |
| **macOS** | `NSSound` 또는 `rodio` 크레이트 |
| **Linux** | PulseAudio/PipeWire 또는 `rodio` 크레이트 |

`rodio` 크레이트로 통일 가능. BEL 문자(`\x07`) 처리도 포함.

## 최적화 전략

- **알림 합치기 (coalescing)**: 짧은 시간 내 동일 소스의 알림을 하나로 합친다. 예: 500ms 이내에 같은 PTY에서 온 BEL 알림은 하나만 표시한다.
- **알림 저장소 크기 제한**: 오래된 알림을 자동 정리한다. FIFO 방식으로 최대 N개만 유지하여 메모리를 제한한다.
- **글로우 애니메이션 최적화**: 워크스페이스 탭의 글로우 애니메이션 프레임을 GPU 셰이더로 처리하여 CPU 부담을 최소화한다.
- **시스템 알림 빈도 제한**: OS 알림 API 호출 빈도를 제한하여 시스템 부하를 방지한다. 짧은 시간 내 다수의 OS 알림을 하나로 묶는다.

## 구현 가능 여부

| OS | 인앱 알림 | 시스템 트레이 | OS 알림 | 사운드 |
|----|----------|-------------|--------|--------|
| Windows | ✅ | ✅ | ✅ | ✅ |
| macOS | ✅ | ✅ | ✅ | ✅ |
| Linux | ✅ | ✅ | ✅ | ✅ |

GUI 앱이므로 cmux와 동등한 수준의 알림 시스템을 구현할 수 있다.
