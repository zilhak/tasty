# utilities

터미널 에뮬레이터에서 공통으로 사용하는 유틸리티 라이브러리 모음.

## Cargo.toml

```toml
[dependencies]
pollster = "0.4"
bytemuck = { version = "1", features = ["derive"] }
regex = "1"
shell-escape = "0.1"
directories = "6"
```

## pollster 0.4 — 비동기 블로킹

비동기 함수를 동기 컨텍스트에서 실행한다. wgpu 초기화 등 한 번만 실행하는 비동기 작업에 적합하다.

### block_on

```rust
use pollster::FutureExt;  // block_on 메서드를 Future에 추가

// 방법 1: 메서드 체인
let adapter = wgpu::Instance::new(wgpu::InstanceDescriptor::default())
    .request_adapter(&wgpu::RequestAdapterOptions::default())
    .block_on()
    .expect("GPU 어댑터 없음");

// 방법 2: 함수 스타일
let result = pollster::block_on(async {
    perform_async_init().await
});
```

### wgpu 초기화 패턴

```rust
use pollster::FutureExt;

pub struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl GpuContext {
    pub fn new(surface: &wgpu::Surface<'_>) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(surface),
                ..Default::default()
            })
            .block_on()
            .expect("적합한 GPU 어댑터를 찾을 수 없음");

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Tasty GPU"),
                    ..Default::default()
                },
                None,
            )
            .block_on()
            .expect("GPU 디바이스 생성 실패");

        Self { device, queue }
    }
}
```

주의: `pollster`는 단일 스레드 실행자로, 멀티스레드 런타임(tokio)에서 호출하면 데드락이 발생할 수 있다. 애플리케이션 초기화 시에만 사용한다.

## bytemuck 1 — 메모리 안전 타입 변환

GPU 버퍼에 데이터를 전달할 때 Rust 타입을 바이트 슬라이스로 변환한다.

### Pod, Zeroable

```rust
use bytemuck::{Pod, Zeroable};

// Pod: Plain Old Data — 임의 비트 패턴이 유효한 타입
// Zeroable: 모든 비트가 0인 상태가 유효한 타입
#[repr(C)]  // C 레이아웃 보장 (필수)
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],   // x, y
    pub uv: [f32; 2],         // 텍스처 좌표
    pub color: [f32; 4],      // RGBA
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct CellUniform {
    pub cell_size: [f32; 2],
    pub viewport_size: [f32; 2],
    pub scroll_offset: f32,
    pub _padding: [f32; 3],  // 16바이트 정렬
}
```

### cast_slice — 슬라이스 변환

```rust
use bytemuck::cast_slice;

let vertices: Vec<Vertex> = vec![
    Vertex { position: [0.0, 0.0], uv: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
    Vertex { position: [1.0, 0.0], uv: [1.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
    Vertex { position: [0.5, 1.0], uv: [0.5, 1.0], color: [1.0, 1.0, 1.0, 1.0] },
];

// Vec<Vertex> → &[u8] (wgpu 버퍼에 직접 전달)
let byte_slice: &[u8] = cast_slice(&vertices);

queue.write_buffer(&vertex_buffer, 0, byte_slice);
```

### bytes_of — 단일 값 변환

```rust
use bytemuck::bytes_of;

let uniform = CellUniform {
    cell_size: [8.0, 16.0],
    viewport_size: [1920.0, 1080.0],
    scroll_offset: 0.0,
    _padding: [0.0; 3],
};

// 단일 구조체 → &[u8]
let bytes = bytes_of(&uniform);
queue.write_buffer(&uniform_buffer, 0, bytes);
```

### wgpu 버퍼 생성 패턴

```rust
use bytemuck::{cast_slice, Pod};

fn create_vertex_buffer<T: Pod>(
    device: &wgpu::Device,
    data: &[T],
    label: &str,
) -> wgpu::Buffer {
    use wgpu::util::DeviceExt;
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: cast_slice(data),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    })
}
```

## regex 1 — 정규표현식

### Regex::new

```rust
use regex::Regex;

// 컴파일 (실패 시 에러)
let re = Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap();

// 에러 처리
let re = Regex::new(r"[invalid")
    .map_err(|e| format!("잘못된 정규식: {e}"))?;
```

### is_match

```rust
use regex::Regex;

let url_re = Regex::new(r"https?://[^\s]+").unwrap();

let line = "문서: https://example.com/docs 참조";
if url_re.is_match(line) {
    println!("URL 발견");
}

// 터미널 출력에서 ANSI 이스케이프 감지
let ansi_re = Regex::new(r"\x1b\[[0-9;]*[mGKHF]").unwrap();
let has_ansi = ansi_re.is_match("\x1b[31mHello\x1b[0m");
```

### captures

```rust
use regex::Regex;

let re = Regex::new(r"(\w+)@(\w+)\.(\w+)").unwrap();
let text = "user@example.com";

if let Some(caps) = re.captures(text) {
    let full = caps.get(0).unwrap().as_str();  // "user@example.com"
    let user = caps.get(1).unwrap().as_str();  // "user"
    let domain = caps.get(2).unwrap().as_str(); // "example"
    let tld = caps.get(3).unwrap().as_str();    // "com"
    println!("{user} at {domain}.{tld}");
}

// 명명 캡처 그룹
let re = Regex::new(r"(?P<year>\d{4})-(?P<month>\d{2})-(?P<day>\d{2})").unwrap();
if let Some(caps) = re.captures("2026-03-22") {
    let year = &caps["year"];    // "2026"
    let month = &caps["month"];  // "03"
    let day = &caps["day"];      // "22"
    println!("{year}/{month}/{day}");
}

// 모든 매치 순회
let re = Regex::new(r"\b\w+\b").unwrap();
for cap in re.captures_iter("hello world foo") {
    println!("{}", &cap[0]);
}
```

### replace_all

```rust
use regex::Regex;

// 단순 치환
let re = Regex::new(r"\s+").unwrap();
let result = re.replace_all("hello   world   foo", " ");
println!("{result}");  // "hello world foo"

// 캡처 그룹 참조
let re = Regex::new(r"(\w+)\s(\w+)").unwrap();
let result = re.replace_all("hello world foo bar", "$2 $1");
println!("{result}");  // "world hello bar foo"

// 클로저로 동적 치환 (ANSI 코드 제거)
let ansi_re = Regex::new(r"\x1b\[[0-9;]*[mGKHF]").unwrap();
let clean = ansi_re.replace_all("\x1b[31mHello\x1b[0m World", "");
println!("{clean}");  // "Hello World"

// URL 마스킹
let url_re = Regex::new(r"https?://[^\s]+").unwrap();
let masked = url_re.replace_all("Visit https://secret.com/token=abc", |caps: &regex::Captures| {
    let url = caps.get(0).unwrap().as_str();
    let host: String = url.chars().take(20).collect();
    format!("{host}...")
});
```

### OnceLock 패턴 — 정적 정규식

```rust
use regex::Regex;
use std::sync::OnceLock;

// 정규식을 한 번만 컴파일하고 재사용 (스레드 안전)
fn ansi_escape_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\x1b(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])").unwrap()
    })
}

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"https?://[^\s<>\"{}|\\^\[\]`]+").unwrap()
    })
}

// 사용
fn strip_ansi(s: &str) -> std::borrow::Cow<str> {
    ansi_escape_re().replace_all(s, "")
}

fn find_urls(s: &str) -> Vec<&str> {
    url_re().find_iter(s).map(|m| m.as_str()).collect()
}
```

## shell-escape 0.1 — 셸 인자 이스케이핑

### escape

```rust
use shell_escape::escape;
use std::borrow::Cow;

// 셸에서 안전하게 사용할 수 있도록 이스케이프
let path = Cow::Borrowed("/home/user/my file.txt");
let escaped = escape(path);
println!("{escaped}");  // '/home/user/my file.txt'

// 특수 문자 처리
let cmd = Cow::Borrowed("echo $HOME; rm -rf /");
let safe = escape(cmd);
println!("{safe}");  // 'echo $HOME; rm -rf /'

// String에서 사용
fn escape_arg(s: &str) -> String {
    escape(Cow::Borrowed(s)).into_owned()
}
```

### 터미널 명령 구성 패턴

```rust
use shell_escape::escape;
use std::borrow::Cow;

/// 안전한 셸 커맨드 구성
fn build_command(program: &str, args: &[&str]) -> String {
    let mut parts = vec![escape(Cow::Borrowed(program)).into_owned()];
    for arg in args {
        parts.push(escape(Cow::Borrowed(arg)).into_owned());
    }
    parts.join(" ")
}

// 사용
let cmd = build_command("ssh", &["-p", "22", "user@host.example.com"]);
println!("{cmd}");  // ssh -p 22 user@host.example.com

let cmd = build_command("open", &["/path/with spaces/file.txt"]);
println!("{cmd}");  // open '/path/with spaces/file.txt'
```

## directories 6 — 플랫폼별 디렉토리

### BaseDirs

```rust
use directories::BaseDirs;

if let Some(base_dirs) = BaseDirs::new() {
    // 홈 디렉토리
    let home = base_dirs.home_dir();          // /home/user (Linux), C:\Users\user (Windows)

    // 설정 디렉토리
    let config = base_dirs.config_dir();      // ~/.config (Linux), %APPDATA% (Windows)
    let config_local = base_dirs.config_local_dir(); // ~/.config (Linux), %LOCALAPPDATA% (Windows)

    // 데이터 디렉토리
    let data = base_dirs.data_dir();          // ~/.local/share (Linux), %APPDATA% (Windows)
    let data_local = base_dirs.data_local_dir();

    // 캐시 디렉토리
    let cache = base_dirs.cache_dir();        // ~/.cache (Linux), %LOCALAPPDATA% (Windows)

    println!("설정: {}", config.display());
}
```

### ProjectDirs

```rust
use directories::ProjectDirs;

// ProjectDirs::from(qualifier, organization, application)
if let Some(proj_dirs) = ProjectDirs::from("com", "zilhak", "tasty") {
    // 앱 전용 설정 디렉토리
    let config_dir = proj_dirs.config_dir();
    // Linux:   ~/.config/tasty/
    // macOS:   ~/Library/Application Support/com.zilhak.tasty/
    // Windows: C:\Users\user\AppData\Roaming\zilhak\tasty\config\

    // 앱 전용 데이터 디렉토리
    let data_dir = proj_dirs.data_dir();

    // 앱 전용 캐시 디렉토리
    let cache_dir = proj_dirs.cache_dir();

    // 앱 전용 런타임 파일 (소켓 등)
    if let Some(runtime_dir) = proj_dirs.runtime_dir() {
        let socket_path = runtime_dir.join("tasty.sock");
        println!("IPC 소켓: {}", socket_path.display());
    }

    println!("설정 경로: {}", config_dir.display());
}
```

### 설정 파일 경로 유틸리티

```rust
use directories::ProjectDirs;
use std::path::PathBuf;

pub struct TastyDirs {
    config_dir: PathBuf,
    data_dir: PathBuf,
    cache_dir: PathBuf,
}

impl TastyDirs {
    pub fn new() -> Option<Self> {
        let proj = ProjectDirs::from("com", "zilhak", "tasty")?;

        Some(Self {
            config_dir: proj.config_dir().to_path_buf(),
            data_dir: proj.data_dir().to_path_buf(),
            cache_dir: proj.cache_dir().to_path_buf(),
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn theme_dir(&self) -> PathBuf {
        self.config_dir.join("themes")
    }

    pub fn font_cache(&self) -> PathBuf {
        self.cache_dir.join("fonts")
    }

    /// 모든 필요 디렉토리 생성
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.cache_dir)?;
        std::fs::create_dir_all(self.theme_dir())?;
        Ok(())
    }
}

// 사용
fn main() {
    let dirs = TastyDirs::new().expect("홈 디렉토리를 찾을 수 없음");
    dirs.ensure_dirs().expect("디렉토리 생성 실패");

    let config_path = dirs.config_file();
    if config_path.exists() {
        println!("설정 파일: {}", config_path.display());
    } else {
        println!("기본 설정 사용 (파일 없음: {})", config_path.display());
    }
}
```

## 플랫폼별 경로 요약

| 디렉토리 | Linux | macOS | Windows |
|---------|-------|-------|---------|
| `config_dir` | `~/.config/tasty` | `~/Library/Application Support/…` | `%APPDATA%\zilhak\tasty\config` |
| `data_dir` | `~/.local/share/tasty` | `~/Library/Application Support/…` | `%APPDATA%\zilhak\tasty\data` |
| `cache_dir` | `~/.cache/tasty` | `~/Library/Caches/…` | `%LOCALAPPDATA%\zilhak\tasty\cache` |
| `runtime_dir` | `/run/user/1000/tasty` | (없음) | (없음) |
