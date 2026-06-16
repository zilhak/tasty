# clap 4

커맨드라인 인자 파싱 라이브러리. derive 매크로를 통해 구조체/열거형에서 CLI를 자동 생성한다.

## Cargo.toml

```toml
[dependencies]
clap = { version = "4", features = ["derive", "env"] }
```

`env` 피처는 환경변수 바인딩에 필요.

## 기본 구조

```rust
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "tasty")]
#[command(about = "GPU 가속 터미널 에뮬레이터")]
#[command(version)]  // Cargo.toml의 version 자동 사용
struct Cli {
    /// 실행할 셸 (기본값: 시스템 기본 셸)
    #[arg(short, long)]
    shell: Option<String>,
}

fn main() {
    let cli = Cli::parse();
    println!("{:?}", cli);
}
```

## #[command] 속성

```rust
#[derive(Parser)]
#[command(
    name = "tasty",                        // 프로그램 이름
    version = "0.1.0",                     // 버전 (또는 자동: version)
    about = "터미널 에뮬레이터",              // 짧은 설명
    long_about = "GPU 가속 크로스플랫폼...", // 긴 설명
    author = "zilhak",                     // 저자
    after_help = "예시: tasty --shell zsh", // 도움말 뒤에 붙는 텍스트
    propagate_version = true,              // 서브커맨드에 버전 전파
    disable_help_flag = false,             // -h/--help 비활성화 여부
)]
struct Cli { ... }
```

## #[arg] 속성

### short / long

```rust
#[derive(Parser)]
struct Cli {
    /// 디버그 모드 활성화
    #[arg(short, long)]          // -d, --debug
    debug: bool,

    /// 출력 파일 경로
    #[arg(short = 'o', long = "output")]  // -o, --output
    output: Option<String>,

    /// 터미널 폰트 크기
    #[arg(long)]                 // --font-size (long만)
    font_size: Option<f32>,
}
```

### default_value

```rust
#[derive(Parser)]
struct Cli {
    /// 폰트 크기
    #[arg(long, default_value = "14")]
    font_size: f32,

    /// 컬럼 수
    #[arg(long, default_value_t = 80)]
    cols: u32,

    /// 셸 경로
    #[arg(long, default_value = "/bin/bash")]
    shell: String,
}
```

### env (환경변수 바인딩)

```rust
#[derive(Parser)]
struct Cli {
    /// API 토큰 (TASTY_TOKEN 환경변수로도 설정 가능)
    #[arg(long, env = "TASTY_TOKEN")]
    token: Option<String>,

    /// 설정 파일 경로
    #[arg(long, env = "TASTY_CONFIG", default_value = "~/.tasty/config.toml")]
    config: String,
}
```

### conflicts_with

```rust
#[derive(Parser)]
struct Cli {
    /// JSON 출력
    #[arg(long, conflicts_with = "plain")]
    json: bool,

    /// 일반 텍스트 출력
    #[arg(long, conflicts_with = "json")]
    plain: bool,

    // 여러 인자와 충돌
    #[arg(long, conflicts_with_all = ["json", "plain"])]
    silent: bool,
}
```

### requires / required_unless_present

```rust
#[derive(Parser)]
struct Cli {
    #[arg(long)]
    username: Option<String>,

    // --username이 있을 때만 의미 있음
    #[arg(long, requires = "username")]
    password: Option<String>,

    // --output 또는 --dry-run 중 하나는 반드시 필요
    #[arg(long, required_unless_present = "dry_run")]
    output: Option<String>,

    #[arg(long)]
    dry_run: bool,
}
```

### 위치 인자 (positional)

```rust
#[derive(Parser)]
struct Cli {
    /// 실행할 커맨드
    command: Option<String>,

    /// 커맨드에 전달할 인자들
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
}
```

### num_args (복수 값)

```rust
#[derive(Parser)]
struct Cli {
    /// 여러 파일
    #[arg(short, long, num_args = 1..)]
    files: Vec<String>,

    /// 정확히 2개
    #[arg(long, num_args = 2)]
    range: Vec<i32>,
}
```

## 서브커맨드 (Subcommand)

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tasty")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 새 터미널 세션 시작
    New {
        /// 실행할 셸
        #[arg(short, long, default_value = "bash")]
        shell: String,

        /// 시작 디렉토리
        #[arg(short = 'D', long)]
        dir: Option<String>,
    },

    /// 기존 세션에 붙기
    Attach {
        /// 세션 ID
        session_id: String,
    },

    /// 세션 목록
    List,

    /// 설정 관리
    Config(ConfigArgs),
}

#[derive(Parser)]
struct ConfigArgs {
    #[command(subcommand)]
    action: ConfigAction,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// 설정값 읽기
    Get {
        key: String,
    },
    /// 설정값 쓰기
    Set {
        key: String,
        value: String,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::New { shell, dir } => {
            println!("새 세션: shell={shell}, dir={:?}", dir);
        }
        Commands::Attach { session_id } => {
            println!("세션 붙기: {session_id}");
        }
        Commands::List => {
            println!("세션 목록");
        }
        Commands::Config(args) => match args.action {
            ConfigAction::Get { key } => println!("get: {key}"),
            ConfigAction::Set { key, value } => println!("set: {key}={value}"),
        },
    }
}
```

## ValueEnum

열거형 값을 CLI 인자로 사용한다.

```rust
use clap::{Parser, ValueEnum};

#[derive(ValueEnum, Clone, Debug)]
enum ColorScheme {
    Dark,
    Light,
    #[value(name = "solarized-dark")]
    SolarizedDark,
    #[value(name = "solarized-light")]
    SolarizedLight,
}

#[derive(Parser)]
struct Cli {
    /// 색상 테마
    #[arg(long, value_enum, default_value_t = ColorScheme::Dark)]
    color_scheme: ColorScheme,

    /// GPU 백엔드
    #[arg(long, value_enum)]
    backend: Option<GpuBackend>,
}

#[derive(ValueEnum, Clone, Debug)]
enum GpuBackend {
    Vulkan,
    Metal,
    Dx12,
    #[value(name = "opengl")]
    OpenGl,
    Auto,
}
```

## Args (서브 구조체 재사용)

```rust
use clap::{Args, Parser};

/// 폰트 관련 공통 인자
#[derive(Args, Debug)]
struct FontArgs {
    /// 폰트 패밀리
    #[arg(long, default_value = "JetBrains Mono")]
    font_family: String,

    /// 폰트 크기 (pt)
    #[arg(long, default_value_t = 14.0)]
    font_size: f32,

    /// 줄 높이 배율
    #[arg(long, default_value_t = 1.4)]
    line_height: f64,
}

#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    font: FontArgs,

    #[arg(long)]
    fullscreen: bool,
}
```

## 전체 예시: tasty CLI

```rust
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "tasty",
    about = "GPU 가속 터미널 에뮬레이터",
    version,
    propagate_version = true,
)]
pub struct Cli {
    /// 상세 로그 출력
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// 설정 파일 경로
    #[arg(long, env = "TASTY_CONFIG")]
    pub config: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 새 터미널 창 열기
    Open {
        #[arg(short, long, default_value = "bash")]
        shell: String,

        #[arg(short = 'D', long)]
        working_dir: Option<String>,

        #[arg(long, value_enum, default_value_t = ColorScheme::Dark)]
        color_scheme: ColorScheme,

        /// 실행 후 종료
        #[arg(long, conflicts_with = "stay")]
        close_on_exit: bool,

        #[arg(long, conflicts_with = "close_on_exit")]
        stay: bool,
    },

    /// IPC 서버 시작
    Server {
        #[arg(long, default_value = "/tmp/tasty.sock")]
        socket: String,
    },
}

#[derive(ValueEnum, Clone, Debug)]
pub enum ColorScheme {
    Dark,
    Light,
}

fn main() {
    let cli = Cli::parse();

    let log_level = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    println!("로그 레벨: {log_level}");

    match cli.command {
        Some(Commands::Open { shell, working_dir, color_scheme, .. }) => {
            println!("터미널 열기: {shell} in {:?} with {:?}", working_dir, color_scheme);
        }
        Some(Commands::Server { socket }) => {
            println!("서버 시작: {socket}");
        }
        None => {
            // 서브커맨드 없으면 기본 동작 (터미널 열기)
            println!("기본 터미널 실행");
        }
    }
}
```

## 유용한 ArgAction

```rust
#[derive(Parser)]
struct Cli {
    // 플래그 카운트 (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    // 기본 bool 플래그
    #[arg(long, action = clap::ArgAction::SetTrue)]
    debug: bool,

    // --no-color 패턴
    #[arg(long = "no-color", action = clap::ArgAction::SetFalse)]
    color: bool,
}
```

## 에러 처리

```rust
use clap::Parser;

fn main() {
    // parse()는 에러 시 process::exit 호출
    let cli = Cli::parse();

    // 수동 처리 (에러를 직접 제어할 때)
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
}
```
