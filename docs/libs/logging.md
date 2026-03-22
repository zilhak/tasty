# logging

`tracing`은 구조화된 로그와 스팬(span) 기반 진단, `tracing-subscriber`는 출력 포맷 설정을 담당한다.

## Cargo.toml

```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

## tracing 0.1 — 기본 매크로

### 로그 레벨 매크로

```rust
use tracing::{debug, error, info, trace, warn};

fn process_input(data: &[u8]) {
    trace!("입력 처리 시작: {} 바이트", data.len());
    debug!("입력 데이터: {:?}", &data[..data.len().min(16)]);
    info!("입력 처리 완료");
    warn!("입력 크기가 큼: {} 바이트", data.len());
    error!("입력 처리 실패");
}
```

레벨별 용도:

| 레벨 | 용도 | 기본 출력 |
|------|------|-----------|
| `trace` | 매우 상세한 내부 상태 | 비활성 |
| `debug` | 개발 중 진단 정보 | 비활성 |
| `info` | 정상 운영 이벤트 | 활성 |
| `warn` | 주의 필요한 상황 | 활성 |
| `error` | 오류 발생 | 활성 |

### 구조화된 필드

```rust
use tracing::info;

// 키-값 필드
info!(
    user_id = 42,
    session = "abc123",
    command = "ls -la",
    "커맨드 실행"
);

// Debug 포맷
let rect = wgpu::Extent3d { width: 800, height: 600, depth_or_array_layers: 1 };
debug!(size = ?rect, "렌더 타겟 생성");

// Display 포맷 (기본)
let path = std::path::Path::new("/tmp/tasty.sock");
info!(socket = %path, "IPC 소켓 열기");

// 조건부 메시지
warn!(
    cols = cols,
    rows = rows,
    max = 32768,
    cols > 32768 || rows > 32768,  // 조건이 true일 때만 출력
    "터미널 크기 비정상"
);
```

### span — 실행 컨텍스트 추적

```rust
use tracing::{info, span, Level};

fn render_frame(frame_id: u64) {
    let span = span!(Level::DEBUG, "render_frame", frame_id = frame_id);
    let _guard = span.enter();  // 스코프 진입 (Drop 시 자동 종료)

    info!("프레임 렌더링 시작");
    // 이 함수 안의 모든 로그에 frame_id 컨텍스트가 붙음
    draw_background();
    draw_cells();
    info!("프레임 렌더링 완료");
}

fn draw_background() {
    // 부모 스팬(render_frame)의 컨텍스트가 자동으로 전파됨
    tracing::debug!("배경 그리기");
}
```

### instrument — 함수 자동 계측

```rust
use tracing::instrument;

// 함수 전체를 스팬으로 감싸기
#[instrument]
fn process_pty_output(data: &[u8]) {
    // 자동으로 span!(Level::INFO, "process_pty_output", data = ?data) 생성
    info!("PTY 출력 처리 중");
}

// 커스텀 레벨과 이름
#[instrument(level = "debug", name = "pty_write")]
fn write_to_pty(pty: &mut Pty, data: &[u8]) -> std::io::Result<usize> {
    debug!(bytes = data.len(), "PTY에 쓰기");
    pty.write(data)
}

// 필드 제어: 포함/제외
#[instrument(fields(frame_id, skip(renderer)))]
fn render(renderer: &Renderer, frame_id: u64) {
    // renderer는 스팬에서 제외 (큰 타입이라 출력하면 잡음)
    // frame_id는 포함
}

// 에러 로깅 자동화
#[instrument(err)]
fn parse_config(path: &str) -> Result<Config, ConfigError> {
    // 에러 발생 시 자동으로 error!() 로그
    let s = std::fs::read_to_string(path)?;
    toml::from_str(&s).map_err(Into::into)
}

// 반환값 로깅
#[instrument(ret)]
fn calculate_cell_size(font_size: f32) -> f32 {
    // 반환값을 debug! 로 자동 출력
    font_size * 0.6
}
```

### span! 매크로 직접 사용

```rust
use tracing::{span, Level};

// 이름 없는 필드와 함께
let span = span!(
    Level::INFO,
    "connection",
    peer_addr = %"127.0.0.1:8080",
    protocol = "IPC",
);

// 비동기 코드에서 (enter() 대신 in_scope 또는 .instrument())
async fn handle_client(id: u32) {
    let span = span!(Level::INFO, "client", id = id);

    async move {
        info!("클라이언트 연결");
        // ...
    }
    .instrument(span)
    .await;
}
```

## tracing-subscriber 0.3

### fmt::init — 빠른 초기화

```rust
fn main() {
    // 기본 초기화 (RUST_LOG 환경변수 사용)
    tracing_subscriber::fmt::init();

    tracing::info!("애플리케이션 시작");
}
```

`RUST_LOG=debug cargo run` 으로 레벨 제어.

### EnvFilter — 세밀한 필터링

```rust
use tracing_subscriber::{fmt, EnvFilter};

fn init_logging() {
    let filter = EnvFilter::try_from_default_env()
        // RUST_LOG가 없으면 기본값 사용
        .unwrap_or_else(|_| EnvFilter::new("tasty=info,warn"));

    fmt()
        .with_env_filter(filter)
        .init();
}

// RUST_LOG 문법:
// RUST_LOG=debug                    → 전체 debug
// RUST_LOG=tasty=debug              → tasty 크레이트만 debug
// RUST_LOG=tasty=debug,wgpu=warn    → tasty는 debug, wgpu는 warn
// RUST_LOG=tasty::render=trace      → 특정 모듈만 trace
```

### 포맷 커스터마이징

```rust
use tracing_subscriber::{fmt, EnvFilter};

fn init_logging() {
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(true)           // 모듈 경로 표시 (tasty::render)
        .with_thread_ids(true)       // 스레드 ID 표시
        .with_thread_names(true)     // 스레드 이름 표시
        .with_file(true)             // 파일명 표시
        .with_line_number(true)      // 줄 번호 표시
        .with_level(true)            // 레벨 표시 (기본 true)
        .with_ansi(true)             // 색상 ANSI 코드
        .compact()                   // 컴팩트 형식
        .init();
}

// JSON 포맷 (로그 수집기용)
fn init_json_logging() {
    fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
}

// 개발용: 보기 좋은 출력
fn init_pretty_logging() {
    fmt()
        .pretty()
        .with_env_filter("tasty=debug")
        .init();
}
```

### 레이어 시스템 (고급)

```rust
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

fn init_layered_logging() {
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_ansi(cfg!(not(windows)));  // Windows는 ANSI 비활성화

    let filter_layer = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .init();
}
```

### 파일 출력

```rust
use std::fs::File;
use tracing_subscriber::{fmt, EnvFilter};

fn init_file_logging(log_path: &str) {
    let file = File::create(log_path).expect("로그 파일 생성 실패");

    fmt()
        .with_writer(file)
        .with_ansi(false)  // 파일에는 ANSI 코드 불필요
        .with_env_filter(EnvFilter::new("debug"))
        .init();
}

// 콘솔과 파일 동시 출력
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn init_dual_logging(log_path: &str) {
    let file = File::create(log_path).unwrap();

    let console_layer = tracing_subscriber::fmt::layer()
        .with_ansi(true);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file)
        .with_ansi(false)
        .json();

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(console_layer)
        .with(file_layer)
        .init();
}
```

## 터미널 에뮬레이터 실전 패턴

```rust
use tracing::{debug, error, info, instrument, warn};

pub struct PtyManager {
    id: u32,
}

impl PtyManager {
    #[instrument(skip(self), fields(pty_id = self.id))]
    pub fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        debug!(bytes = data.len(), "PTY 쓰기");
        // ...
        Ok(())
    }

    #[instrument(skip(self), fields(pty_id = self.id), err)]
    pub fn resize(&mut self, cols: u16, rows: u16) -> std::io::Result<()> {
        info!(cols, rows, "PTY 크기 조정");
        if cols == 0 || rows == 0 {
            warn!(cols, rows, "비정상적인 크기 요청");
        }
        // ...
        Ok(())
    }
}

// 성능 측정
fn render_with_timing() {
    let start = std::time::Instant::now();

    // ... 렌더링 ...

    let elapsed = start.elapsed();
    if elapsed.as_millis() > 16 {
        warn!(
            ms = elapsed.as_millis(),
            "프레임 시간 초과 (목표: 16ms)"
        );
    } else {
        debug!(ms = elapsed.as_millis(), "프레임 완료");
    }
}
```

## EnvFilter 문법 요약

```
RUST_LOG=level                           # 전체 크레이트
RUST_LOG=crate=level                     # 특정 크레이트
RUST_LOG=crate::module=level             # 특정 모듈
RUST_LOG=crate=level,other_crate=level   # 여러 규칙 (쉼표 구분)
RUST_LOG=[span_name]=level               # 특정 스팬
RUST_LOG=crate[span{field=value}]=level  # 필드 필터링
```

예시:
```bash
RUST_LOG=tasty=debug cargo run
RUST_LOG=tasty::render=trace,tasty=info,wgpu=warn cargo run
RUST_LOG=debug,hyper=off cargo run
```
