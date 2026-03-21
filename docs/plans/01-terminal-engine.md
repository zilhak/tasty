# 01. 터미널 엔진

## cmux 구현 방식

- libghostty (Zig 기반) 임베딩
- Metal GPU 가속 렌더링
- Ghostty 설정 파일 호환 (`~/.config/ghostty/config`)
- 폰트 줌, CJK 폴백, background-opacity, 리사이즈 깜빡임 최소화

## 크로스 플랫폼 구현 방안

tasty는 WezTerm/Alacritty와 같은 **네이티브 GPU 가속 터미널 에뮬레이터**다.
VTE 파서 + PTY 관리 + GPU 렌더링을 직접 구현한다.

### 아키텍처 개요

```
┌─────────────────────────────────────────────────┐
│                  winit (윈도우/입력)               │
├─────────────────────────────────────────────────┤
│              wgpu (GPU 렌더링 백엔드)              │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐ │
│  │ 글리프    │  │ 셀 그리드 │  │ UI 위젯       │ │
│  │ 래스터    │  │ 렌더링    │  │ (사이드바 등)  │ │
│  └──────────┘  └──────────┘  └───────────────┘ │
├─────────────────────────────────────────────────┤
│     termwiz (VTE 파서 + Surface 셀 그리드)          │
├─────────────────────────────────────────────────┤
│         PTY 관리 (portable-pty)                  │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐     │
│  │ Shell 1  │  │ Shell 2  │  │ Shell N  │     │
│  └──────────┘  └──────────┘  └──────────┘     │
└─────────────────────────────────────────────────┘
```

### 핵심 컴포넌트

| 컴포넌트 | 라이브러리 | 역할 |
|---------|----------|------|
| 윈도우 관리 | `winit` | OS 윈도우 생성, 이벤트 루프, 입력 처리 |
| GPU 렌더링 | `wgpu` | Vulkan/Metal/DX12/OpenGL 추상화 |
| VTE 파서 | `termwiz` | ANSI/DEC 이스케이프 시퀀스 파싱, 셀 그리드(Surface), 스크롤백 |
| 폰트 렌더링 | `cosmic-text` + `swash` | 폰트 셰이핑, 래스터라이징, CJK 폴백 |
| 글리프 캐시 | 자체 구현 (텍스처 아틀라스) | GPU 텍스처에 글리프 캐싱 |
| PTY | `portable-pty` | 크로스 플랫폼 PTY 추상화 |

### VTE 파서: termwiz

`termwiz`(WezTerm 프로젝트)를 VTE 파서로 사용한다.

**선택 근거:**

- **Surface 개념**: 셀 그리드(Surface)를 내장하여 별도 구현 없이 셀 관리, 스크롤백, SGR 속성 처리가 가능하다
- **라이브러리 설계**: WezTerm 내부에서 분리된 독립 크레이트로, 외부 프로젝트에서 사용하기 적합하다
- **OSC 지원**: OSC 9/52/99/777 등 알림 및 클립보드 관련 시퀀스를 기본 지원한다
- **ConPTY 호환**: Windows ConPTY 환경에서 검증된 구현이다

**추상화 레이어:**

향후 유연성을 위해 `TerminalBackend` 트레이트로 감싼다.

```rust
trait TerminalBackend {
    fn process_input(&mut self, bytes: &[u8]);
    fn get_surface(&self) -> &Surface;
    fn get_scrollback(&self) -> &ScrollbackBuffer;
    fn resize(&mut self, cols: usize, rows: usize);
}

struct TermwizBackend {
    terminal: Terminal,
    // termwiz::terminal::Terminal 래핑
}

impl TerminalBackend for TermwizBackend {
    // ...
}
```

### GPU 렌더링 파이프라인

1. **셀 그리드 → 정점 버퍼**: 각 셀의 글리프/배경색을 정점 데이터로 변환
2. **글리프 아틀라스**: `swash`로 래스터라이즈한 글리프를 GPU 텍스처에 캐싱
3. **배경 패스**: 셀 배경색을 사각형으로 렌더링
4. **글리프 패스**: 텍스처 아틀라스에서 글리프를 샘플링하여 렌더링
5. **UI 패스**: 사이드바, 명령 팔레트 등 UI 요소 렌더링

### OS별 GPU 백엔드

| OS | 기본 백엔드 | 폴백 |
|----|-----------|------|
| **Windows** | Vulkan 또는 DX12 | DX11, OpenGL |
| **macOS** | Metal | — |
| **Linux** | Vulkan | OpenGL |

wgpu가 이 모든 백엔드를 추상화한다.

### 폰트 렌더링

| 항목 | 구현 |
|------|------|
| 폰트 디스커버리 | `font-kit` 또는 `cosmic-text`의 시스템 폰트 탐색 |
| 셰이핑 | `cosmic-text` (HarfBuzz 기반 rustybuzz 사용) |
| 래스터라이징 | `swash` |
| CJK 폴백 | 폰트 폴백 체인 설정 (Noto Sans CJK 등) |
| 리가처 | HarfBuzz 셰이핑으로 자동 지원 |
| 서브픽셀 렌더링 | OS별 LCD 필터 적용 |

### PTY 관리

| OS | PTY API | 비고 |
|----|---------|------|
| **Linux/macOS** | `forkpty()`, `/dev/ptmx` | POSIX 표준 |
| **Windows** | ConPTY (`CreatePseudoConsole`) | Windows 10 1809+ |

`portable-pty` 크레이트가 세 OS를 모두 추상화한다.

## 최적화 전략

- **Dirty region tracking**: 변경된 셀만 재렌더링하고 전체 화면 리드로우를 회피한다. 셀 그리드에 damage flag를 두어 변경된 행/영역만 GPU에 업로드한다.
- **프레임 배칭**: PTY 출력이 연속으로 올 때 매번 렌더링하지 않고 프레임 단위로 합친다. `cat bigfile.txt` 같은 대량 출력 시 중간 프레임을 버리고 최종 상태만 렌더링한다.
- **VSync 동기화**: 모니터 주사율에 맞춘 렌더링을 수행하고, 유휴 시 렌더링을 완전 중단하여 GPU 사용률 0%를 달성한다. `wgpu`의 `PresentMode::Fifo`를 활용한다.
- **글리프 아틀라스 관리**: LRU 퇴출 정책으로 사용하지 않는 글리프를 제거하고, 아틀라스 포화 시 재구축한다. 멀티 아틀라스 페이지로 대규모 유니코드 범위를 커버한다.
- **PTY I/O 버퍼링**: 대량 출력 시 쓰로틀링을 적용하고(`cat bigfile.txt`), 읽기 버퍼 크기를 최적화한다. 64KB~256KB 범위에서 OS별로 튜닝한다.
- **멀티스레딩 파이프라인**: PTY 읽기(스레드) → VTE 파싱(스레드) → 렌더링(메인) 으로 파이프라인을 분리한다. 채널 기반 통신으로 스레드 간 데이터를 전달한다.
- **콜드 스타트 최적화**: 셰이더를 사전 컴파일하고 캐싱하여 첫 프레임까지의 시간을 단축한다. 폰트 인덱스도 디스크에 캐싱한다.
- **스크롤백 메모리**: 링 버퍼로 고정 메모리를 사용하여 스크롤백이 아무리 길어도 메모리 상한을 유지한다. 오래된 행은 압축 저장 옵션을 제공한다.
- **서브픽셀 렌더링 캐시**: LCD 필터 결과를 캐싱하여 동일 글리프에 대한 반복 래스터화를 방지한다.
- **리사이즈 최적화**: 리사이즈 중 렌더링을 디바운스하고, 최종 크기에서만 레이아웃을 재계산한다. 드래그 중에는 저해상도 미리보기만 표시한다.

## 구현 가능 여부

| OS | 가능 여부 | GPU 백엔드 | 비고 |
|----|-----------|----------|------|
| Windows | ✅ 가능 | Vulkan/DX12/DX11 | ConPTY 필요 (Win10 1809+) |
| macOS | ✅ 가능 | Metal | 제한 없음 |
| Linux | ✅ 가능 | Vulkan/OpenGL | Wayland/X11 모두 지원 |

## 참고 프로젝트

- [WezTerm](https://github.com/wez/wezterm) — Rust GPU 가속 터미널 에뮬레이터, 크로스 플랫폼, 가장 유사한 참고 대상
- [Alacritty](https://github.com/alacritty/alacritty) — Rust GPU 가속 터미널 에뮬레이터, OpenGL 기반
- [Rio](https://github.com/nicely/rio) — Rust + wgpu 터미널 에뮬레이터
- [Ghostty](https://ghostty.org/) — Zig GPU 가속 터미널 에뮬레이터 (cmux가 사용)
