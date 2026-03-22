# Rust 크레이트 생태계 관점

각 분리 후보의 외부 재사용 가치, 유사 기존 크레이트와의 차별점, 공개 API 설계를 분석한다.

---

## 1. tasty-hooks

### 유사 기존 크레이트

| 크레이트 | 설명 | 차이점 |
|----------|------|--------|
| `signal-hook` | OS 시그널 핸들링 | 터미널 이벤트가 아닌 OS 시그널 대상 |
| `notify` | 파일시스템 감시 | 완전히 다른 영역 |
| (없음) | 터미널 이벤트 훅 | **고유 영역** |

### 고유 가치 제안

터미널 에뮬레이터/AI 에이전트 생태계에서 **이벤트 기반 자동화 훅**은 신규 개념이다. 프로세스 종료, 출력 패턴 매칭, Bell, 알림, 유휴 타임아웃 등의 이벤트에 셸 명령을 바인딩하는 라이브러리는 crates.io에 존재하지 않는다.

### 잠재적 외부 사용자

- 다른 터미널 에뮬레이터 (Alacritty, WezTerm, Rio)
- CI/CD 파이프라인 자동화 도구
- AI 코딩 에이전트 프레임워크 (Claude Code, Cursor, Aider)
- 터미널 세션 모니터링 도구

### 공개 API 설계

```rust
// tasty-hooks/src/lib.rs

pub type HookId = u64;

/// 훅이 반응하는 이벤트 유형
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HookEvent {
    ProcessExit,
    OutputMatch(String),
    Bell,
    Notification,
    IdleTimeout(u64),
}

impl HookEvent {
    /// "process-exit", "output-match:pattern", "bell" 등의 문자열에서 파싱
    pub fn parse(s: &str) -> Option<Self>;
    /// 표시용 문자열로 직렬화
    pub fn to_display_string(&self) -> String;
}

/// 하나의 훅 등록 정보
#[derive(Clone, Debug)]
pub struct SurfaceHook {
    pub id: HookId,
    pub surface_id: u32,
    pub event: HookEvent,
    pub command: String,
    pub once: bool,
}

/// 훅 매니저. 등록/삭제/조회/실행을 관리.
pub struct HookManager {
    // ...
}

impl HookManager {
    pub fn new() -> Self;
    pub fn add_hook(&mut self, surface_id: u32, event: HookEvent, command: String, once: bool) -> HookId;
    pub fn remove_hook(&mut self, hook_id: HookId) -> bool;
    pub fn list_hooks(&self, surface_id: Option<u32>) -> Vec<&SurfaceHook>;
    pub fn check_and_fire(&mut self, surface_id: u32, events: &[HookEvent]) -> Vec<HookId>;
}
```

### 라이선스 호환성

MIT. 모든 의존 크레이트 (`regex`, `serde`)가 MIT/Apache-2.0 듀얼 라이선스이므로 문제 없음.

---

## 2. tasty-terminal

### 유사 기존 크레이트

| 크레이트 | 설명 | 차이점 |
|----------|------|--------|
| `termwiz` | VTE 파싱 + Surface | 파싱만, PTY 관리 없음 |
| `vte` | ANSI 이스케이프 파서 | 저수준 파서만 |
| `alacritty_terminal` | Alacritty 터미널 | Alacritty에 밀접 결합 |
| `portable-pty` | PTY 추상화 | PTY만, VTE 파싱 없음 |

### 고유 가치 제안

**PTY 생성 + VTE 파싱 + Surface 관리 + 이벤트 발생 + Read Mark API**를 하나의 패키지로 통합. 기존 크레이트는 이 중 하나만 담당하지만, tasty-terminal은 "터미널 엔진"을 완성된 형태로 제공한다.

특히 Read Mark API(`set_mark`/`read_since_mark`)는 AI 에이전트가 명령 결과만 추출하는 데 특화된 기능으로, 기존 크레이트에 없다.

### 잠재적 외부 사용자

- 헤드리스 터미널 테스트 프레임워크
- CI/CD에서 셸 실행 + 출력 캡처
- AI 에이전트의 셸 인터페이스 라이브러리
- 다른 터미널 에뮬레이터의 백엔드

### 공개 API 설계

```rust
// tasty-terminal/src/lib.rs

pub type Waker = Arc<dyn Fn() + Send + Sync>;

pub struct Terminal { /* ... */ }

impl Terminal {
    /// PTY + 셸 프로세스 생성
    pub fn new(cols: usize, rows: usize, waker: Waker) -> Result<Self>;
    pub fn new_with_shell(cols: usize, rows: usize, shell: Option<&str>, waker: Waker) -> Result<Self>;

    /// PTY에서 데이터를 읽고 VTE 파싱. 변경이 있으면 true 반환.
    pub fn process(&mut self) -> bool;

    /// 입력 전송
    pub fn send_key(&mut self, text: &str);
    pub fn send_bytes(&mut self, bytes: &[u8]);

    /// 리사이즈
    pub fn resize(&mut self, cols: usize, rows: usize);

    /// 현재 화면 Surface 참조
    pub fn surface(&self) -> &termwiz::surface::Surface;

    /// 누적 이벤트 소비
    pub fn take_events(&mut self) -> Vec<TerminalEvent>;

    /// Read Mark API
    pub fn set_mark(&mut self);
    pub fn read_since_mark(&self, strip_ansi: bool) -> String;

    /// 마우스 트래킹 상태
    pub fn mouse_tracking(&self) -> MouseTrackingMode;
    pub fn bracketed_paste(&self) -> bool;
    pub fn cursor_visible(&self) -> bool;
}

pub struct TerminalEvent {
    pub surface_id: u32,
    pub kind: TerminalEventKind,
}

pub enum TerminalEventKind {
    Notification { title: String, body: String },
    BellRing,
    TitleChanged(String),
    CwdChanged(String),
    ProcessExited,
    ClipboardSet(String),
}

pub enum MouseTrackingMode {
    None, Click, CellMotion, AllMotion,
}
```

### 라이선스 호환성

MIT. `portable-pty` (MIT), `termwiz` (MIT), `regex` (MIT/Apache-2.0) 모두 호환.

---

## 3. tasty-ipc-protocol

### 유사 기존 크레이트

| 크레이트 | 다운로드/월 | 설명 |
|----------|------------|------|
| `jsonrpc-core` | ~150K | JSON-RPC 타입 + 디스패치 |
| `lsp-types` | ~500K | LSP 프로토콜 타입 |
| `json-rpc-types` | ~10K | 순수 JSON-RPC 2.0 타입 |

### 고유 가치 제안

**없음.** 131줄의 기본적인 JSON-RPC 2.0 타입 정의로, `jsonrpc-core`나 `json-rpc-types`의 하위 호환. crates.io에 공개할 경우 기존 크레이트와 경쟁할 이점이 없다.

### 잠재적 외부 사용자

사실상 없음.

---

## 4. tasty-ipc-server

### 유사 기존 크레이트

| 크레이트 | 설명 |
|----------|------|
| `jsonrpc-tcp-server` | 범용 JSON-RPC TCP 서버 |
| `jsonrpc-http-server` | HTTP 기반 JSON-RPC 서버 |
| `tower` + `hyper` | 범용 서버 프레임워크 |

### 고유 가치 제안

제한적. 동기(sync) 채널 기반 단순 TCP JSON-RPC 서버로, tokio 없이 동작하는 것이 특징이지만, 196줄 수준으로 라이브러리보다는 예제 코드에 가깝다.

---

## 5. tasty-notification

### 유사 기존 크레이트

| 크레이트 | 설명 |
|----------|------|
| `notify-rust` | OS 네이티브 알림 전송 |
| (없음) | 인앱 알림 저장소 |

### 고유 가치 제안

FIFO 알림 저장소 + 병합(coalesce) + 워크스페이스별 카운트라는 조합은 tasty 고유 요구사항. 범용 알림 저장소로 사용하기에는 API가 너무 특화되어 있다.

---

## 6. tasty-settings

### 고유 가치 제안

**없음.** TOML 기반 설정 로드/저장은 `config` 크레이트 등으로 대체 가능하고, `Settings` 구조체 자체가 tasty 고유 필드(폰트 크기, 사이드바 너비 등)만 담는다.

---

## 7. tasty-model

### 고유 가치 제안

제한적. Workspace → PaneNode → Pane → Tab → Panel → SurfaceNode 계층 구조는 터미널 에뮬레이터에 특화되어 있지만, 각 프로젝트마다 고유한 데이터 모델을 갖기 때문에 범용 재사용이 어렵다.

---

## 8. tasty-renderer

### 유사 기존 크레이트

| 크레이트 | 설명 | 차이점 |
|----------|------|--------|
| `wgpu` | GPU 추상화 | 저수준 API만 |
| `iced` | UI 프레임워크 | 터미널 렌더링 아님 |
| `cosmic-text` | 텍스트 렌더링 | 범용, 터미널 특화 아님 |
| (없음) | wgpu 터미널 셀 렌더러 | **고유 영역** |

### 고유 가치 제안

**wgpu 기반 터미널 셀 렌더러**는 crates.io에 존재하지 않는다. 2-pass 인스턴스 렌더링(배경 + 글리프), 글리프 아틀라스, xterm-256 팔레트 지원을 포함하는 완성된 패키지. 다른 터미널 프로젝트에서 GPU 렌더링을 도입할 때 즉시 사용 가능.

### 잠재적 외부 사용자

- wgpu 기반 터미널 에뮬레이터를 만드려는 프로젝트
- 게임 엔진 내 터미널 임베딩
- IDE의 내장 터미널 렌더러

### 공개 API 설계 (목표)

```rust
// tasty-renderer/src/lib.rs

/// 렌더러가 읽을 터미널 화면 추상화
pub trait TerminalSurface {
    fn dimensions(&self) -> (usize, usize);
    fn cells(&self) -> CellIterator<'_>;
    fn cursor_position(&self) -> Option<(usize, usize)>;
    fn cursor_visible(&self) -> bool;
}

pub struct CellInfo {
    pub col: usize,
    pub row: usize,
    pub text: String,
    pub foreground: CellColor,
    pub background: CellColor,
    pub bold: bool,
    pub italic: bool,
}

pub enum CellColor {
    Default,
    Indexed(u8),
    Rgb(f32, f32, f32, f32),
}

pub struct Rect {
    pub x: f32, pub y: f32, pub width: f32, pub height: f32,
}

pub struct CellRenderer { /* ... */ }

impl CellRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, font_size: f32, font_family: &str) -> Self;
    pub fn prepare(&mut self, surface: &dyn TerminalSurface, queue: &wgpu::Queue);
    pub fn prepare_viewport(&mut self, surface: &dyn TerminalSurface, queue: &wgpu::Queue, viewport: &Rect, screen_width: u32, screen_height: u32);
    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>);
    pub fn render_scissored<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>, viewport: &Rect, surface_width: u32, surface_height: u32);
    pub fn resize(&self, queue: &wgpu::Queue, width: u32, height: u32);
    pub fn grid_size(&self, width: u32, height: u32) -> (usize, usize);
    pub fn cell_width(&self) -> f32;
    pub fn cell_height(&self) -> f32;
}

/// termwiz 호환 adapter (feature = "termwiz")
#[cfg(feature = "termwiz")]
impl TerminalSurface for termwiz::surface::Surface { /* ... */ }
```

### 라이선스 호환성

MIT. `wgpu` (MIT/Apache-2.0), `cosmic-text` (MIT/Apache-2.0), `bytemuck` (MIT/Apache-2.0 OR Zlib) 모두 호환.

---

## 생태계 가치 종합

| 후보 | 고유 가치 | 외부 수요 | crates.io 공개 가치 |
|------|----------|----------|---------------------|
| `tasty-hooks` | **높음** — 터미널 이벤트 훅 최초 | 높음 | **높음** |
| `tasty-terminal` | **높음** — 통합 터미널 엔진 | 중간 | **높음** |
| `tasty-renderer` | **높음** — wgpu 터미널 렌더러 최초 | 중간 | **중간** (분리 난이도 고려) |
| `tasty-ipc-protocol` | 없음 — 기존 대체재 다수 | 없음 | 없음 |
| `tasty-ipc-server` | 낮음 — 단순 TCP 서버 | 없음 | 없음 |
| `tasty-notification` | 없음 — tasty 고유 | 없음 | 없음 |
| `tasty-model` | 없음 — tasty 고유 | 없음 | 없음 |
| `tasty-settings` | 없음 — tasty 고유 | 없음 | 없음 |
