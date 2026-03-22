# error-handling

`anyhow`는 애플리케이션 레벨 에러 처리, `thiserror`는 라이브러리 레벨 커스텀 에러 타입 정의에 사용한다.

## Cargo.toml

```toml
[dependencies]
anyhow = "1"
thiserror = "2"
```

## anyhow 1

### Result 타입 별칭

```rust
use anyhow::Result;

// anyhow::Result<T> = std::result::Result<T, anyhow::Error>
fn load_config(path: &str) -> Result<String> {
    let content = std::fs::read_to_string(path)?;
    Ok(content)
}

// 표준 에러를 자동으로 anyhow::Error로 변환
fn parse_port(s: &str) -> Result<u16> {
    let port: u16 = s.parse()?;  // ParseIntError → anyhow::Error
    Ok(port)
}
```

### Context — 에러 메시지 추가

```rust
use anyhow::{Context, Result};

fn load_config(path: &str) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("설정 파일을 읽을 수 없음: {path}"))?;

    let config: Config = toml::from_str(&content)
        .with_context(|| format!("TOML 파싱 실패: {path}"))?;

    Ok(config)
}

// context() vs with_context()
// context(): 정적 문자열 (항상 평가됨)
// with_context(): 클로저 (실패 시에만 평가됨 — 권장)
fn connect(host: &str) -> Result<()> {
    std::net::TcpStream::connect(host)
        .context("TCP 연결 실패")?;          // 정적
    Ok(())
}

fn read_file(path: &str) -> Result<Vec<u8>> {
    std::fs::read(path)
        .with_context(|| format!("파일 읽기 실패: {path}"))?;  // 동적
    Ok(vec![])
}
```

### anyhow! 매크로 — 즉석 에러 생성

```rust
use anyhow::{anyhow, Result};

fn validate_port(port: u16) -> Result<()> {
    if port < 1024 {
        return Err(anyhow!("포트 번호는 1024 이상이어야 합니다. 입력값: {}", port));
    }
    Ok(())
}

fn get_env(key: &str) -> Result<String> {
    std::env::var(key).map_err(|_| anyhow!("환경변수 {} 가 설정되지 않았습니다", key))
}
```

### bail! 매크로 — 즉시 반환

```rust
use anyhow::{bail, Result};

fn process_command(cmd: &str) -> Result<()> {
    if cmd.is_empty() {
        bail!("커맨드가 비어있습니다");
    }
    if cmd.len() > 4096 {
        bail!("커맨드가 너무 깁니다: {} 바이트 (최대 4096)", cmd.len());
    }
    // 처리 계속...
    Ok(())
}
```

### ensure! 매크로 — 조건 검사

```rust
use anyhow::{ensure, Result};

fn open_pty(cols: u16, rows: u16) -> Result<()> {
    ensure!(cols > 0, "cols는 0보다 커야 합니다");
    ensure!(rows > 0, "rows는 0보다 커야 합니다");
    ensure!(cols <= 32768, "cols가 너무 큼: {}", cols);
    ensure!(rows <= 32768, "rows가 너무 큼: {}", rows);
    // PTY 생성...
    Ok(())
}
```

### 다운캐스트 — 원본 에러 타입 복원

```rust
use anyhow::{Context, Result};
use std::io;

fn may_fail() -> Result<()> {
    std::fs::read_to_string("/nonexistent")
        .context("파일 없음")?;
    Ok(())
}

fn handle_error() {
    if let Err(e) = may_fail() {
        // 원본 에러 타입으로 다운캐스트
        if let Some(io_err) = e.downcast_ref::<io::Error>() {
            match io_err.kind() {
                io::ErrorKind::NotFound => eprintln!("파일을 찾을 수 없음"),
                io::ErrorKind::PermissionDenied => eprintln!("권한 없음"),
                _ => eprintln!("IO 오류: {io_err}"),
            }
        } else {
            eprintln!("기타 오류: {e}");
        }

        // 에러 체인 출력
        eprintln!("원인: {:?}", e);  // Debug: 전체 체인
        eprintln!("메시지: {}", e);  // Display: 최상위 메시지만

        // 체인 순회
        for cause in e.chain() {
            eprintln!("  인한: {cause}");
        }
    }
}
```

### main() 에러 처리

```rust
use anyhow::Result;

fn main() -> Result<()> {
    // ? 연산자를 main에서 직접 사용 가능
    let config = load_config("config.toml")?;
    run(config)?;
    Ok(())
}

// 더 나은 에러 포맷 (색상 포함)
fn main() {
    if let Err(e) = run_app() {
        eprintln!("오류: {e}");
        // 원인 체인 출력
        for (i, cause) in e.chain().skip(1).enumerate() {
            eprintln!("  {}: {cause}", i + 1);
        }
        std::process::exit(1);
    }
}

fn run_app() -> anyhow::Result<()> {
    // ...
    Ok(())
}
```

## thiserror 2

### Error derive 기본

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TastyError {
    #[error("PTY 생성 실패: {0}")]
    PtyCreation(String),

    #[error("렌더링 오류")]
    Render,

    #[error("설정 파일 오류: {path}")]
    Config {
        path: String,
    },
}
```

### #[from] — 자동 From 구현

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TastyError {
    // From<io::Error> 자동 구현
    #[error("IO 오류: {0}")]
    Io(#[from] std::io::Error),

    // From<toml::de::Error> 자동 구현
    #[error("설정 파싱 오류: {0}")]
    Config(#[from] toml::de::Error),

    // From<serde_json::Error> 자동 구현
    #[error("JSON 오류: {0}")]
    Json(#[from] serde_json::Error),
}

// 사용: ? 연산자가 자동으로 변환
fn load_config(path: &str) -> Result<Config, TastyError> {
    let s = std::fs::read_to_string(path)?;  // io::Error → TastyError::Io
    let c: Config = toml::from_str(&s)?;      // toml::Error → TastyError::Config
    Ok(c)
}
```

### #[source] — 에러 원인 지정

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TastyError {
    // source는 Display에 나타나지 않지만 Error::source()로 접근 가능
    #[error("PTY 초기화 실패")]
    PtyInit {
        #[source]
        cause: std::io::Error,
        cols: u16,
        rows: u16,
    },
}

// 차이점:
// #[from] → From<T> 구현 + source() 구현
// #[source] → source()만 구현 (From은 직접 구현해야 함)
```

### #[error] 포맷 문법

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RenderError {
    // 튜플 필드: {0}, {1}, ...
    #[error("셰이더 컴파일 실패: {0}")]
    ShaderCompile(String),

    // 명명 필드: {필드명}
    #[error("텍스처 크기 초과: {width}x{height} (최대 {max_size})")]
    TextureTooBig {
        width: u32,
        height: u32,
        max_size: u32,
    },

    // source 필드 참조
    #[error("GPU 초기화 실패: {backend}")]
    GpuInit {
        backend: String,
        #[source]
        source: wgpu::RequestDeviceError,
    },
}
```

### transparent — 래퍼 에러

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TastyError {
    // Display와 source를 내부 에러에 위임
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),  // anyhow와 혼용 가능
}
```

### 실전: 터미널 에뮬레이터 에러 계층

```rust
use thiserror::Error;

/// PTY 관련 에러
#[derive(Error, Debug)]
pub enum PtyError {
    #[error("PTY 생성 실패: {0}")]
    Creation(#[from] std::io::Error),

    #[error("PTY 크기 설정 실패 (cols={cols}, rows={rows})")]
    Resize {
        cols: u16,
        rows: u16,
        #[source]
        source: std::io::Error,
    },

    #[error("PTY 쓰기 실패")]
    Write(#[from] std::io::Error),  // 주의: From 중복 불가, 이름 다르게 해야 함
}

// From 중복 해결: 별도 타입으로 분리
#[derive(Error, Debug)]
pub enum PtyError {
    #[error("PTY 생성 실패: {0}")]
    Creation(std::io::Error),

    #[error("PTY 크기 조정 실패")]
    Resize(std::io::Error),

    #[error("PTY 쓰기 실패")]
    Write(std::io::Error),
}

/// 렌더링 에러
#[derive(Error, Debug)]
pub enum RenderError {
    #[error("wgpu 표면 오류: {0}")]
    Surface(#[from] wgpu::SurfaceError),

    #[error("셰이더 컴파일 오류: {name}")]
    Shader { name: String },
}

/// 최상위 애플리케이션 에러
#[derive(Error, Debug)]
pub enum AppError {
    #[error("PTY 오류: {0}")]
    Pty(#[from] PtyError),

    #[error("렌더링 오류: {0}")]
    Render(#[from] RenderError),

    #[error("설정 오류: {0}")]
    Config(String),

    #[error("IO 오류: {0}")]
    Io(#[from] std::io::Error),
}
```

## anyhow + thiserror 혼용 패턴

```rust
// 라이브러리 코드: thiserror로 구체적인 에러 타입
pub mod pty {
    use thiserror::Error;

    #[derive(Error, Debug)]
    pub enum PtyError {
        #[error("생성 실패: {0}")]
        Create(std::io::Error),
    }

    pub fn open() -> Result<Pty, PtyError> { ... }
}

// 바이너리 코드: anyhow로 간편하게 처리
use anyhow::{Context, Result};

fn main() -> Result<()> {
    let pty = pty::open()
        .context("PTY 초기화 실패")?;  // PtyError → anyhow::Error
    Ok(())
}
```

## 비교 표

| 상황 | 사용 |
|------|------|
| 애플리케이션 `main`, 최상위 에러 전파 | `anyhow::Result` |
| 즉석 에러 생성 | `anyhow!`, `bail!`, `ensure!` |
| 에러에 문맥 추가 | `.context()`, `.with_context()` |
| 공개 라이브러리 API 에러 타입 | `thiserror::Error` |
| 여러 에러를 하나로 묶기 | `thiserror` + `#[from]` |
| 에러 원인 체인 | `thiserror` + `#[source]` |
| 에러 위임 (래퍼) | `#[error(transparent)]` |
