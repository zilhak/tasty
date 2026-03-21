# 라이브러리 분리 분석

현재 단일 바이너리 크레이트인 tasty를 여러 라이브러리 크레이트로 분리할 후보 7개를 분석한다.

---

## 분리 후보 요약

| # | 후보 | 현재 파일 | 분리 크레이트명 | 분리 가능성 | 인터페이스 설계 | 이점 | 비용 |
|---|------|-----------|----------------|------------|----------------|------|------|
| 1 | 터미널 엔진 | terminal.rs | tasty-terminal | 높음 | trait Terminal | 독립 테스트, 재사용 | PTY 의존 격리 필요 |
| 2 | GPU 렌더러 | font.rs + renderer.rs | wgpu-terminal-renderer | 중간 | trait TerminalSurface | 다른 앱에서 재사용 | wgpu API 안정성 |
| 3 | IPC 프로토콜 | ipc/protocol.rs | jsonrpc-types | 높음 | 그대로 | 즉시 분리 가능 | 거의 없음 |
| 4 | IPC 서버 | ipc/server.rs | jsonrpc-tcp-server | 높음 | trait RequestHandler | 범용 JSON-RPC 서버 | 핸들러 trait 설계 |
| 5 | 알림 저장소 | notification.rs | notification-store | 높음 | 그대로 | 즉시 분리 가능 | notify-rust 선택적 |
| 6 | 이벤트 훅 | hooks.rs | event-hooks | 높음 | 그대로 | 즉시 분리 가능 | 거의 없음 |
| 7 | 데이터 모델 | model.rs | tasty-model | 중간 | Terminal trait 추출 | 핵심 분리 | terminal 의존 해제 필요 |

---

## 1. terminal.rs → tasty-terminal

### 현재 상태
- 파일: `terminal.rs` (646줄)
- 역할: PTY 생성/관리, VTE 파싱, Surface 갱신, 이벤트 생성, 읽기 마크 API.
- 의존: `portable-pty`, `termwiz`, `regex`, `std::sync::mpsc`.

### 분리 가능성: **높음**

`terminal.rs`는 `model.rs`에 의존하지 않는다. 반대로 `model.rs`가 `Terminal`과 `Waker` 타입을 import한다. 따라서 분리 후 `tasty-model`이 `tasty-terminal`에 의존하는 구조가 된다.

### 인터페이스 설계

```rust
// tasty-terminal/src/lib.rs
pub type Waker = Arc<dyn Fn() + Send + Sync>;

pub struct Terminal { ... }

impl Terminal {
    pub fn new(cols: usize, rows: usize, waker: Waker) -> Result<Self>;
    pub fn new_with_shell(cols: usize, rows: usize, shell: Option<&str>, waker: Waker) -> Result<Self>;
    pub fn process(&mut self) -> bool;
    pub fn send_key(&mut self, text: &str);
    pub fn send_bytes(&mut self, bytes: &[u8]);
    pub fn resize(&mut self, cols: usize, rows: usize);
    pub fn surface(&self) -> &termwiz::surface::Surface;
    pub fn take_events(&mut self) -> Vec<TerminalEvent>;
    pub fn set_mark(&mut self);
    pub fn read_since_mark(&self, strip_ansi: bool) -> String;
}

pub struct TerminalEvent { ... }
pub enum TerminalEventKind { ... }
```

### 분리 단계
1. `terminal.rs`를 `tasty-terminal` 크레이트로 이동.
2. `TerminalEvent`, `TerminalEventKind`, `Waker` 타입을 공개 API로 노출.
3. `model.rs`에서 `use tasty_terminal::{Terminal, Waker}`로 import 변경.
4. `state.rs`, `ipc/handler.rs` 등에서 import 경로 갱신.

### 이점
- 터미널 엔진의 독립 단위 테스트 가능.
- 다른 터미널 에뮬레이터 프로젝트에서 재사용 가능.
- GUI 없이 헤드리스 터미널 모드 구현 가능.

### 비용
- `termwiz::surface::Surface` 타입이 공개 API에 노출됨 → termwiz 버전 호환성.
- PTY 관련 테스트에 실제 셸 프로세스가 필요 (CI 환경 주의).

---

## 2. font.rs + renderer.rs → wgpu-terminal-renderer

### 현재 상태
- 파일: `font.rs` (358줄) + `renderer.rs` (699줄) = 1,057줄.
- 역할: cosmic-text 폰트 관리, 글리프 아틀라스, wgpu 셀 렌더 파이프라인.
- 의존: `cosmic-text`, `wgpu`, `bytemuck`, `termwiz::surface::Surface`, `model::Rect`.

### 분리 가능성: **중간**

`termwiz::surface::Surface`에 직접 의존한다 (`renderer.rs:514` — `prepare()` 메서드가 `Surface` 참조를 받음). `model::Rect`도 사용한다.

### 인터페이스 설계

```rust
// wgpu-terminal-renderer/src/lib.rs

/// 렌더러가 읽을 터미널 서피스 추상화
pub trait TerminalSurface {
    fn dimensions(&self) -> (usize, usize);
    fn screen_lines(&self) -> Vec<ScreenLine>;
    fn cursor_position(&self) -> (usize, usize);
}

pub struct Rect { pub x: f32, pub y: f32, pub width: f32, pub height: f32 }

pub struct CellRenderer { ... }
pub struct FontConfig { ... }
pub struct GlyphAtlas { ... }
```

### 분리 단계
1. `Rect`를 렌더러 크레이트에 자체 정의하거나, 별도 `tasty-types` 크레이트로 추출.
2. `TerminalSurface` trait 정의 → `termwiz::surface::Surface`에 대한 impl은 feature flag 또는 adapter.
3. `CellRenderer::prepare()` 파라미터를 `&dyn TerminalSurface`로 변경.
4. `font.rs`와 `renderer.rs`를 합쳐서 하나의 크레이트로 구성.

### 이점
- 터미널 렌더링 엔진을 독립 라이브러리로 공개 가능.
- termwiz 외의 VTE 백엔드 (예: vte, alacritty_terminal)로 교체 가능.
- 렌더러 단독 벤치마크/테스트 가능.

### 비용
- `TerminalSurface` trait 설계에 시간 필요 (termwiz Screen API 추상화).
- wgpu 버전 업그레이드 시 공개 API 변경 필요.
- WGSL 셰이더를 크레이트에 포함해야 함.

---

## 3. ipc/protocol.rs → jsonrpc-types

### 현재 상태
- 파일: `ipc/protocol.rs` (131줄).
- 역할: `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcError` 타입 + 팩토리 메서드.
- 의존: `serde`, `serde_json` 만.

### 분리 가능성: **높음 (즉시)**

외부 의존이 serde/serde_json뿐이고, tasty 내부 타입을 참조하지 않는다.

### 인터페이스 설계
현재 API를 그대로 노출.

```rust
// jsonrpc-types/src/lib.rs
pub struct JsonRpcRequest { ... }
pub struct JsonRpcResponse { ... }
pub struct JsonRpcError { ... }
```

### 분리 단계
1. 파일을 `jsonrpc-types` 크레이트로 이동.
2. `tasty`에서 `jsonrpc-types = { path = "crates/jsonrpc-types" }` 추가.
3. import 경로 갱신 (`cli.rs`, `ipc/server.rs`, `ipc/handler.rs`).

### 이점
- 5분 내 분리 완료 가능.
- 다른 프로젝트에서 경량 JSON-RPC 타입으로 재사용.

### 비용
- 거의 없음. 크레이트 관리 오버헤드 정도.

---

## 4. ipc/server.rs → jsonrpc-tcp-server

### 현재 상태
- 파일: `ipc/server.rs` (196줄).
- 역할: TCP 리스너, 연결 관리, JSON-RPC 파싱, 메인 스레드 채널 통신.
- 의존: `ipc/protocol.rs`, `directories` (포트 파일 경로), 표준 라이브러리.

### 분리 가능성: **높음**

`IpcServer`는 `JsonRpcRequest`/`JsonRpcResponse`만 사용하고 tasty 상태를 직접 참조하지 않는다. 핸들러 로직은 `ipc/handler.rs`에 있다.

### 인터페이스 설계

```rust
// jsonrpc-tcp-server/src/lib.rs
pub struct IpcCommand {
    pub request: JsonRpcRequest,
    pub response_tx: mpsc::SyncSender<JsonRpcResponse>,
}

pub struct IpcServer { ... }

impl IpcServer {
    pub fn start(port_file: Option<PathBuf>) -> Result<Self>;
    pub fn try_recv(&self) -> Result<IpcCommand, TryRecvError>;
    pub fn port(&self) -> u16;
}
```

### 분리 단계
1. 포트 파일 경로를 매개변수로 받도록 리팩토링 (`port_file_path()`의 하드코딩 제거).
2. `jsonrpc-types` 크레이트에 의존.
3. 파일을 `jsonrpc-tcp-server` 크레이트로 이동.

### 이점
- 범용 JSON-RPC TCP 서버로 재사용 가능.
- 서버 로직 독립 테스트.

### 비용
- `directories` 의존을 선택적으로 만들거나 호출자에게 위임해야 함.
- `read_port_file()`은 tasty 고유 로직이므로 tasty 쪽에 남겨야 할 수 있음.

---

## 5. notification.rs → notification-store

### 현재 상태
- 파일: `notification.rs` (239줄).
- 역할: FIFO 알림 저장, 병합, 읽음 관리, OS 네이티브 알림.
- 의존: `model::SurfaceId`, `model::WorkspaceId` (타입 별칭만), `notify-rust`.

### 분리 가능성: **높음**

`SurfaceId`와 `WorkspaceId`는 `u32` 타입 별칭이므로 분리 시 자체 정의하거나 제네릭으로 대체 가능.

### 인터페이스 설계

```rust
// notification-store/src/lib.rs
pub struct Notification {
    pub id: u64,
    pub source_workspace: u32,
    pub source_surface: u32,
    pub title: String,
    pub body: String,
    pub timestamp: Instant,
    pub read: bool,
}

pub struct NotificationStore { ... }

// OS 알림은 feature flag로 제어
#[cfg(feature = "system-notification")]
pub fn send_system_notification(title: &str, body: &str);
```

### 분리 단계
1. `WorkspaceId`/`SurfaceId` 대신 `u32` 직접 사용.
2. `notify-rust` 의존을 `system-notification` feature로 분리.
3. 크레이트로 이동.

### 이점
- 알림 로직 독립 테스트 (이미 8개 테스트 보유).
- `notify-rust` 없이도 사용 가능 (임베디드/헤드리스 환경).

### 비용
- 거의 없음.

---

## 6. hooks.rs → event-hooks

### 현재 상태
- 파일: `hooks.rs` (290줄).
- 역할: 이벤트 기반 셸 명령 훅 시스템.
- 의존: `regex` 만.

### 분리 가능성: **높음 (즉시)**

tasty 내부 타입을 일절 참조하지 않는다. `surface_id`는 단순 `u32`.

### 인터페이스 설계
현재 API를 그대로 노출.

```rust
// event-hooks/src/lib.rs
pub type HookId = u64;
pub struct SurfaceHook { ... }
pub enum HookEvent { ... }
pub struct HookManager { ... }
```

### 분리 단계
1. 파일을 `event-hooks` 크레이트로 이동.
2. import 경로 갱신 (`state.rs`, `main.rs`, `ipc/handler.rs`).

### 이점
- 5분 내 분리 완료 가능.
- 다른 이벤트 기반 앱에서 재사용 가능.
- 이미 16개 테스트 보유.

### 비용
- 거의 없음.

---

## 7. model.rs → tasty-model

### 현재 상태
- 파일: `model.rs` (1,370줄).
- 역할: 전체 데이터 모델 계층 (Workspace, PaneNode, Pane, Tab, Panel, SurfaceNode, SurfaceGroupNode, SurfaceGroupLayout, Rect, DividerInfo, SplitDirection).
- 의존: `terminal::Terminal`, `terminal::Waker`.

### 분리 가능성: **중간**

`Terminal`과 `Waker` 타입에 직접 의존한다. `SurfaceNode`가 `terminal: Terminal` 필드를 소유하고, 여러 메서드가 `Terminal`의 `process()`, `resize()`, `surface()` 등을 호출한다.

### 인터페이스 설계

`Terminal`을 trait으로 추상화하면 분리 가능하다.

```rust
// tasty-model/src/lib.rs
pub trait TerminalBackend {
    fn process(&mut self) -> bool;
    fn resize(&mut self, cols: usize, rows: usize);
    fn surface(&self) -> &termwiz::surface::Surface;
    fn take_events(&mut self) -> Vec<TerminalEvent>;
    fn set_mark(&mut self);
    fn read_since_mark(&self, strip_ansi: bool) -> String;
}

pub struct SurfaceNode<T: TerminalBackend> {
    pub id: SurfaceId,
    pub terminal: T,
}
```

### 분리 단계
1. `TerminalBackend` trait 정의.
2. `Terminal`에 trait impl.
3. `model.rs`의 모든 구조체를 제네릭 `T: TerminalBackend`로 파라미터화.
4. 또는 더 실용적으로: `Terminal`을 trait object (`Box<dyn TerminalBackend>`)로 사용.

### 이점
- 모델 레이어의 독립 테스트 (mock Terminal 주입).
- VTE 백엔드 교체 가능성.
- God Object 분해의 첫 단계.

### 비용
- 제네릭 파라미터가 모든 구조체에 전파되어 코드 복잡도 증가.
- trait object 사용 시 dynamic dispatch 오버헤드 (무시할 수준).
- 이미 13개 테스트가 빈 Pane으로 테스트하므로, trait 도입 시 mock이 더 쉬워짐.

---

## 분리 우선순위 권장

| 우선순위 | 크레이트 | 이유 |
|----------|---------|------|
| 1 | `jsonrpc-types` | 즉시 분리 가능, 비용 0 |
| 2 | `event-hooks` | 즉시 분리 가능, 비용 0 |
| 3 | `notification-store` | 즉시 분리 가능, feature flag으로 OS 알림 선택적 |
| 4 | `tasty-terminal` | 높은 재사용 가치, 적은 변경 |
| 5 | `jsonrpc-tcp-server` | 범용 서버, 포트 파일 로직만 리팩토링 |
| 6 | `tasty-model` | Terminal trait 추출 필요, 중간 비용 |
| 7 | `wgpu-terminal-renderer` | TerminalSurface trait 설계 필요, 가장 높은 비용 |
