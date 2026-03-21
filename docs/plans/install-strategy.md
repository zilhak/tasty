# 설치 전략

tasty의 설치 방법, 환경 감지, 자동 설정 생성, 크로스 플랫폼 경로를 정의하는 횡단 전략 문서다.

---

## 설치 방법

### 1. 사전 빌드 바이너리 (GitHub Releases) -- 권장

GitHub Releases에서 플랫폼별 바이너리를 다운로드한다.

| 플랫폼 | 아티팩트 | 포맷 |
|--------|---------|------|
| Windows x64 | `tasty-x86_64-pc-windows-msvc.zip` | ZIP (exe + DLL) |
| Windows ARM64 | `tasty-aarch64-pc-windows-msvc.zip` | ZIP |
| macOS x64 | `tasty-x86_64-apple-darwin.tar.gz` | tar.gz |
| macOS ARM64 (Apple Silicon) | `tasty-aarch64-apple-darwin.tar.gz` | tar.gz |
| macOS Universal | `Tasty.dmg` | DMG (App Bundle) |
| Linux x64 | `tasty-x86_64-unknown-linux-gnu.tar.gz` | tar.gz |
| Linux ARM64 | `tasty-aarch64-unknown-linux-gnu.tar.gz` | tar.gz |
| Linux (AppImage) | `Tasty-x86_64.AppImage` | AppImage |

**원라인 설치 스크립트:**

```bash
# Unix (macOS / Linux)
curl -fsSL https://tasty.dev/install.sh | sh

# Windows (PowerShell)
irm https://tasty.dev/install.ps1 | iex
```

설치 스크립트는 아키텍처 감지 → 최신 릴리스 다운로드 → PATH 등록 → `tasty setup` 실행까지 자동 처리한다.

### 2. 패키지 매니저

| 매니저 | 명령 | 플랫폼 |
|--------|------|--------|
| Homebrew | `brew install tasty` | macOS, Linux |
| Scoop | `scoop install tasty` | Windows |
| WinGet | `winget install tasty` | Windows |
| AUR | `yay -S tasty` / `paru -S tasty` | Arch Linux |
| cargo install | `cargo install tasty-terminal` | 모든 플랫폼 |
| Nix | `nix profile install nixpkgs#tasty` | NixOS, Linux, macOS |

`cargo install`은 소스에서 빌드하므로 Rust 툴체인과 시스템 의존성(CMake, pkg-config, libfreetype 등)이 필요하다.

### 3. 소스 빌드

```bash
git clone https://github.com/user/tasty.git
cd tasty

# 의존성 확인
# Linux: sudo apt install cmake pkg-config libfreetype6-dev libfontconfig1-dev
# macOS: xcode-select --install
# Windows: Visual Studio Build Tools (MSVC)

cargo build --release

# 바이너리 위치: target/release/tasty (Unix) / target/release/tasty.exe (Windows)
```

**빌드 feature flags:**

| Flag | 기본값 | 설명 |
|------|--------|------|
| `wayland` | on (Linux) | Wayland 지원 |
| `x11` | on (Linux) | X11 지원 |
| `gpu-profiling` | off | GPU 타이밍 쿼리 활성화 |
| `bundled-fonts` | on | 폴백 폰트 번들 (Noto Sans Mono) |

```bash
# Wayland 전용 빌드 예시
cargo build --release --no-default-features --features wayland,bundled-fonts
```

---

## 설치 스크립트 (`tasty setup`)

바이너리 설치 후 실행되는 환경 감지 및 설정 생성 프로세스다.

### 실행 시점

- **첫 실행 자동 감지**: `config.toml`이 없으면 자동으로 `tasty setup` 실행
- **수동 실행**: `tasty setup` 명령
- **재설치**: `tasty setup --force` (기존 설정 백업 후 재생성)

### 실행 흐름

```
tasty setup
    ├─ 1. GPU 감지
    ├─ 2. 디스플레이 감지
    ├─ 3. OS 감지
    ├─ 4. 폰트 감지
    ├─ 5. 셸 감지
    ├─ 6. 하드웨어 프로파일링
    ├─ 7. 벤치마크 (선택적)
    ├─ 8. config.toml 생성
    ├─ 9. 셸 통합 설치
    └─ 10. 설치 검증
```

각 단계는 실패해도 다음 단계로 진행하며, 실패한 항목은 기본값으로 대체하고 경고를 출력한다.

---

## 환경 감지 항목

### GPU 감지

wgpu 어댑터 열거로 GPU 정보를 수집한다.

```rust
let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
    backends: wgpu::Backends::all(),
    ..Default::default()
});

let adapters: Vec<_> = instance.enumerate_adapters(wgpu::Backends::all()).collect();

for adapter in &adapters {
    let info = adapter.get_info();
    println!("GPU: {} ({})", info.name, info.vendor);
    println!("Backend: {:?}", info.backend);
    println!("Driver: {}", info.driver);
    println!("Device Type: {:?}", info.device_type);

    let limits = adapter.limits();
    println!("Max Texture: {}x{}", limits.max_texture_dimension_2d, limits.max_texture_dimension_2d);
    println!("Max Buffer Size: {} bytes", limits.max_buffer_size);
}
```

**감지 항목:**

| 항목 | 용도 | 예시 |
|------|------|------|
| `info.name` | 로그, 품질 프리셋 자동 선택 | "NVIDIA GeForce RTX 4070" |
| `info.vendor` | GPU 벤더 판단 (NVIDIA/AMD/Intel/Apple) | `0x10DE` (NVIDIA) |
| `info.backend` | 사용 백엔드 확인 | `Vulkan`, `DX12`, `Metal` |
| `info.driver` | 드라이버 버전 (호환성 이슈 추적) | "535.129.03" |
| `info.device_type` | 소프트웨어 렌더러 감지 | `DiscreteGpu`, `IntegratedGpu`, `Cpu` |
| `limits.max_texture_dimension_2d` | 아틀라스 페이지 크기 결정 | 16384 → 4096 사용 |
| `features` | 셰이더 변형 선택 | 컴퓨트 셰이더 지원 여부 |

**아틀라스 페이지 크기 결정 로직:**

```
max_texture >= 8192  → atlas_size = 4096
max_texture >= 4096  → atlas_size = 2048
max_texture >= 2048  → atlas_size = 1024
else                 → atlas_size = 512 (극단적 레거시)
```

**벤치마크 (선택적):**

간단한 렌더 테스트를 실행하여 실제 GPU 성능을 측정한다.

1. 80x24 그리드의 셀을 렌더링하는 테스트 프레임 100회 실행
2. 평균 프레임 시간 측정
3. 품질 프리셋 자동 선택:

```
frame_time < 2ms   → Ultra
frame_time < 5ms   → High
frame_time < 10ms  → Medium
frame_time < 20ms  → Low
else               → Software
```

### 디스플레이 감지

```rust
let event_loop = EventLoop::new().unwrap();
let monitors: Vec<_> = event_loop.available_monitors().collect();

for monitor in &monitors {
    println!("Name: {:?}", monitor.name());
    println!("Scale Factor: {}", monitor.scale_factor());
    println!("Size: {:?}", monitor.size());  // 물리 픽셀
    println!("Refresh Rate: {:?}Hz", monitor.refresh_rate_millihertz().map(|r| r / 1000));
    println!("Position: {:?}", monitor.position());
}
```

| 항목 | 용도 | 설정 매핑 |
|------|------|----------|
| `scale_factor` | DPI 스케일, 폰트 래스터라이즈 크기 | `display.dpi_scale` |
| `refresh_rate_millihertz` | VSync 프레임 레이트 | `display.refresh_rate` |
| 모니터 수 | 멀티 모니터 기본값 | 복수 모니터 시 윈도우 위치 기억 활성화 |

### OS 감지

```rust
let os = std::env::consts::OS;       // "windows", "macos", "linux"
let arch = std::env::consts::ARCH;   // "x86_64", "aarch64"
```

**플랫폼별 추가 감지:**

| 항목 | 감지 방법 | 중요성 |
|------|----------|--------|
| Windows 버전 | `winver` 레지스트리 / `RtlGetVersion` | ConPTY 사용 가능 여부 (Win10 1809+) |
| Linux 세션 타입 | `$XDG_SESSION_TYPE` | "wayland" vs "x11" 판별 |
| Linux 컴포지터 | `$XDG_CURRENT_DESKTOP`, `$WAYLAND_DISPLAY` | 투명도/블러 지원 여부 |
| macOS 버전 | `sw_vers -productVersion` | API 가용성 |

**Windows 버전 검증:**

```rust
#[cfg(target_os = "windows")]
fn check_conpty_support() -> bool {
    // Windows 10 version 1809 (Build 17763) 이상 필요
    let version = os_info::get();
    match version.version() {
        os_info::Version::Semantic(major, minor, patch) => {
            *major >= 10 && *patch >= 17763
        }
        _ => false,
    }
}
```

ConPTY 미지원 시 경고를 출력하고, 레거시 WinPTY 폴백 또는 최소 기능 모드로 동작한다.

### 폰트 감지

시스템에 설치된 폰트를 열거하여 최적의 폰트 체인을 구성한다.

```rust
use font_kit::source::SystemSource;

let source = SystemSource::new();
let families = source.all_families().unwrap();

// 모노스페이스 폰트 탐색
let monospace_candidates = [
    "JetBrains Mono", "Fira Code", "Cascadia Code", "Hack",
    "Source Code Pro", "Inconsolata", "Menlo", "Consolas",
    "SF Mono", "DejaVu Sans Mono", "Liberation Mono",
];

let detected_mono = monospace_candidates.iter()
    .find(|name| families.contains(&name.to_string()));

// CJK 폰트 탐색
let cjk_candidates = [
    // 한국어 우선
    "Noto Sans CJK KR", "Source Han Sans KR", "Apple SD Gothic Neo",
    "Malgun Gothic", "NanumGothic",
    // 일본어
    "Noto Sans CJK JP", "Source Han Sans JP", "Hiragino Sans",
    "MS Gothic", "Yu Gothic",
    // 중국어
    "Noto Sans CJK SC", "Source Han Sans SC", "PingFang SC",
    "Microsoft YaHei",
    // 범용
    "Noto Sans CJK", "Source Han Sans",
];

let detected_cjk: Vec<&str> = cjk_candidates.iter()
    .filter(|name| families.contains(&name.to_string()))
    .copied()
    .collect();
```

**폰트 폴백 체인 구성:**

```
사용자 설정 폰트 (있으면)
  → 감지된 모노스페이스 폰트
    → 감지된 CJK 폰트
      → 번들된 폴백 폰트 (Noto Sans Mono)
        → 시스템 기본 모노스페이스
```

CJK 폰트가 하나도 없으면 경고를 출력한다:
```
⚠ CJK 폰트를 찾을 수 없음. 한중일 문자가 □로 표시될 수 있음.
  권장: Noto Sans CJK 설치 (https://fonts.google.com/noto)
```

### 셸 감지

```rust
#[cfg(unix)]
fn detect_shell() -> ShellInfo {
    let shell_path = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let shell_name = Path::new(&shell_path).file_name().unwrap().to_str().unwrap();

    // 셸 버전 감지
    let version_output = Command::new(&shell_path).arg("--version").output();

    ShellInfo {
        path: shell_path,
        name: shell_name.to_string(),
        version: parse_version(version_output),
        supports_osc7: check_osc7_support(shell_name),
    }
}

#[cfg(target_os = "windows")]
fn detect_shell() -> ShellInfo {
    // PowerShell 7 (pwsh) → PowerShell 5 (powershell.exe) → cmd.exe 순으로 탐색
    if which::which("pwsh").is_ok() {
        ShellInfo { path: "pwsh".into(), name: "pwsh".into(), .. }
    } else {
        ShellInfo { path: "powershell.exe".into(), name: "powershell".into(), .. }
    }
}
```

**OSC 7 지원 감지:**

| 셸 | OSC 7 지원 | 비고 |
|-----|-----------|------|
| zsh 5.8+ | 기본 지원 (precmd hook) | `.zshrc` 설정 필요 |
| bash 5.1+ | 수동 설정 필요 | `PROMPT_COMMAND`에 추가 |
| fish 3.0+ | 기본 지원 | 자동 |
| PowerShell 7+ | 수동 설정 필요 | `prompt` 함수 수정 |
| cmd.exe | 미지원 | OSC 7 불가, CWD 폴링으로 대체 |

### 하드웨어 프로파일링

```rust
fn profile_hardware() -> HardwareProfile {
    HardwareProfile {
        cpu_cores: num_cpus::get(),                    // 논리 코어 수
        cpu_physical: num_cpus::get_physical(),        // 물리 코어 수
        total_ram_mb: sysinfo::System::new_all().total_memory() / 1024 / 1024,
    }
}
```

| 항목 | 용도 | 설정 매핑 |
|------|------|----------|
| CPU 코어 수 | 스레드 풀 크기 | `performance.thread_pool_size = min(cores, 16)` |
| 총 RAM | 스크롤백 버퍼 한도 | RAM < 4GB → 5000줄, < 8GB → 10000줄, ≥ 8GB → 50000줄 |
| 디스크 속도 (선택) | 세션 저장 주기 추천값 | SSD → 8초 (기본값), HDD → 30초 추천 |

---

## 설치 결과물

### config.toml 생성

감지된 환경 정보를 기반으로 최적화된 설정 파일을 생성한다.

```toml
# Auto-detected by tasty setup (2026-03-21)
# System: Windows 11 (Build 26200), x86_64
# GPU: NVIDIA GeForce RTX 4070 (Vulkan)
# Monitor: 2560x1440 @ 144Hz, scale 1.5

[gpu]
backend = "vulkan"            # auto-detected: vulkan available via NVIDIA driver
atlas_size = 4096             # auto-detected: max_texture 16384, using 4096
quality = "high"              # auto-detected: RTX 4070, benchmark 2.1ms/frame
vsync = true                  # auto-detected: 144Hz monitor
software_fallback = false     # auto-detected: discrete GPU available

[display]
dpi_scale = 1.5               # auto-detected: 2560x1440 @ 27"
refresh_rate = 144             # auto-detected: primary monitor 144Hz

[font]
family = "JetBrains Mono"     # auto-detected: installed on system
size = 14                     # default
fallback = [
    "Noto Sans CJK KR",       # auto-detected: CJK support
    "Noto Color Emoji",        # auto-detected: emoji support
]
subpixel_rendering = false    # auto-detected: dpi_scale 1.5 → subpixel 비활성화

[terminal]
shell = "pwsh"                # auto-detected: PowerShell 7 found
scrollback_lines = 50000      # auto-detected: 32GB RAM
osc7_support = true           # auto-detected: pwsh supports OSC 7

[performance]
thread_pool_size = 8           # auto-detected: 8 physical cores
session_save_interval = 8      # default: 8초 (cmux와 동일), HDD 감지 시 30 추천

[window]
opacity = 1.0                 # default (1.0 = 불투명)
blur = false                  # default (배경 블러 비활성화)
```

각 값에 주석으로 auto-detected 근거를 기록하여, 사용자가 수동 조정할 때 참고할 수 있게 한다.

**생성 위치:** 크로스 플랫폼 설정 경로 (아래 "크로스 플랫폼 설치 경로" 참조)

### 셸 통합 설치

#### OSC 7 (CWD 보고) 설정

셸에 현재 작업 디렉토리를 터미널에 보고하는 훅을 설치한다.

**zsh** (`~/.zshrc`에 추가):
```zsh
# tasty: OSC 7 CWD reporting
__tasty_osc7() {
    printf '\e]7;file://%s%s\e\\' "${HOST}" "${PWD}"
}
precmd_functions+=(__tasty_osc7)
```

**bash** (`~/.bashrc`에 추가):
```bash
# tasty: OSC 7 CWD reporting
__tasty_osc7() {
    printf '\e]7;file://%s%s\e\\' "${HOSTNAME}" "${PWD}"
}
PROMPT_COMMAND="__tasty_osc7${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
```

**fish** (`~/.config/fish/conf.d/tasty.fish`):
```fish
# tasty: OSC 7 CWD reporting
function __tasty_osc7 --on-variable PWD
    printf '\e]7;file://%s%s\e\\' (hostname) "$PWD"
end
```

**PowerShell** (`$PROFILE`에 추가):
```powershell
# tasty: OSC 7 CWD reporting
function prompt {
    $esc = [char]0x1b
    "$esc]7;file://$env:COMPUTERNAME/$($PWD.Path -replace '\\','/')$esc\"
    "PS $($executionContext.SessionState.Path.CurrentLocation)$('>' * ($nestedPromptLevel + 1)) "
}
```

설치 시 기존 셸 설정 파일을 백업한다 (`~/.zshrc.tasty-backup` 등).

#### PATH 등록

- **Unix**: `~/.local/bin/`에 심볼릭 링크 또는 바이너리 배치, `PATH`에 추가 확인
- **Windows**: `%LOCALAPPDATA%\tasty\`를 사용자 PATH에 추가 (레지스트리)
- **Homebrew/Scoop**: 패키지 매니저가 자동 처리

#### 셸 완성 파일

clap의 `generate` 기능으로 각 셸의 완성 파일을 생성한다.

```rust
use clap_complete::{generate, Shell};

// 셸 감지 결과에 따라 적절한 완성 파일 생성
match detected_shell {
    "zsh"  => generate(Shell::Zsh, &mut app, "tasty", &mut File::create(zsh_completions_path)?),
    "bash" => generate(Shell::Bash, &mut app, "tasty", &mut File::create(bash_completions_path)?),
    "fish" => generate(Shell::Fish, &mut app, "tasty", &mut File::create(fish_completions_path)?),
    "pwsh" | "powershell" => generate(Shell::PowerShell, &mut app, "tasty", &mut File::create(ps_completions_path)?),
    _ => {}
}
```

| 셸 | 완성 파일 경로 |
|-----|---------------|
| zsh | `~/.local/share/tasty/completions/_tasty` + `fpath` 등록 |
| bash | `~/.local/share/bash-completion/completions/tasty` |
| fish | `~/.config/fish/completions/tasty.fish` |
| PowerShell | `~/.local/share/tasty/completions/tasty.ps1` + `$PROFILE`에 source |

---

## 런타임 재감지

### 하드웨어 변경 감지

앱 시작 시 현재 하드웨어 정보와 저장된 `config.toml`의 auto-detected 값을 비교한다.

```rust
fn check_hardware_changes(config: &Config) -> Vec<HardwareChange> {
    let mut changes = Vec::new();

    let current_gpu = detect_gpu();
    if current_gpu.name != config.gpu.detected_device_name {
        changes.push(HardwareChange::GpuChanged {
            old: config.gpu.detected_device_name.clone(),
            new: current_gpu.name,
        });
    }

    let current_monitor = detect_primary_monitor();
    if current_monitor.scale_factor != config.display.dpi_scale {
        changes.push(HardwareChange::DpiChanged {
            old: config.display.dpi_scale,
            new: current_monitor.scale_factor,
        });
    }

    // ... 모니터, RAM 등 추가 비교
    changes
}
```

**변경 감지 시 동작:**

| 변경 유형 | 동작 |
|----------|------|
| GPU 변경 | 프롬프트: "GPU가 변경되었다. 재설정을 실행할까?" |
| DPI 변경 | 자동 조정 (아틀라스 재빌드, 폰트 재래스터라이즈) |
| 모니터 주사율 변경 | VSync 설정 자동 업데이트 |
| RAM 변경 | 로그 경고만 (스크롤백은 수동 조정) |

### `tasty doctor`

설정과 환경의 정합성을 진단하는 명령이다.

```
$ tasty doctor

✓ GPU: NVIDIA RTX 4070 (Vulkan) — 정상
✓ 디스플레이: 2560x1440 @ 144Hz, scale 1.5 — 정상
✓ 폰트: JetBrains Mono — 설치됨
✓ CJK 폰트: Noto Sans CJK KR — 설치됨
✓ 셸: pwsh 7.4.1 — OSC 7 지원
✓ 셸 통합: OSC 7 훅 설치됨
✓ PATH: tasty 명령 사용 가능
✗ 셸 완성: zsh 완성 파일 누락
  → 수정: tasty setup --shell-completions

요약: 7/8 통과, 1 문제
```

검사 항목:

| 검사 | 설명 |
|------|------|
| GPU 접근 | wgpu 어댑터 열거 가능 여부 |
| 렌더 테스트 | 테스트 프레임 렌더링 성공 여부 |
| PTY 테스트 | 셸 프로세스 spawn + echo 테스트 |
| 폰트 렌더 테스트 | ASCII + CJK 샘플 글리프 래스터라이즈 |
| IPC 테스트 | 소켓/파이프 생성 가능 여부 |
| 셸 통합 | OSC 7 훅 존재 여부 |
| PATH | `tasty` 명령 경로 확인 |
| 셸 완성 | 완성 파일 존재 여부 |
| 설정 유효성 | `config.toml` 파싱 + 값 범위 검증 |

### `tasty setup --force`

전체 감지를 재실행하고 설정을 재생성한다.

1. 기존 `config.toml`을 `config.toml.backup.{timestamp}`로 백업
2. 전체 환경 감지 재실행
3. 새 `config.toml` 생성
4. 셸 통합 재설치 (기존 백업 후)
5. 설치 검증 실행

---

## 크로스 플랫폼 설치 경로

| OS | 바이너리 | 설정 | 데이터 | 로그 |
|----|---------|------|--------|------|
| Linux | `~/.local/bin/tasty` | `~/.config/tasty/` | `~/.local/share/tasty/` | `~/.local/share/tasty/logs/` |
| macOS | `/usr/local/bin/tasty` 또는 `/Applications/Tasty.app` | `~/.config/tasty/` | `~/Library/Application Support/tasty/` | `~/Library/Logs/tasty/` |
| Windows | `%LOCALAPPDATA%\tasty\tasty.exe` | `~/.config/tasty/` | `%LOCALAPPDATA%\tasty\` | `%LOCALAPPDATA%\tasty\logs\` |

### 디렉토리 구조

```
~/.config/tasty/               # 설정 (Linux/macOS)
├── config.toml                # 메인 설정
├── keybindings.toml           # 키바인딩 오버라이드
├── themes/                    # 사용자 테마
│   └── custom.toml
└── shell-integration/         # 셸 통합 스크립트 원본
    ├── osc7.zsh
    ├── osc7.bash
    ├── osc7.fish
    └── osc7.ps1

~/.local/share/tasty/          # 데이터 (Linux)
├── session.json               # 단일 세션 파일 (모든 워크스페이스 포함)
├── scrollback/                # 스크롤백 데이터
│   └── scrollback-{uuid}.bin
├── cache/                     # 캐시 (삭제 가능)
│   ├── glyph-atlas-cache/     # 글리프 아틀라스 캐시
│   └── font-metrics/          # 폰트 메트릭 캐시
└── logs/                      # 로그
    ├── tasty.log
    └── gpu-debug.log
```

### 경로 결정 로직

```rust
use directories::ProjectDirs;

fn get_paths() -> TastyPaths {
    let proj = ProjectDirs::from("dev", "tasty", "tasty")
        .expect("홈 디렉토리를 찾을 수 없음");

    TastyPaths {
        config_dir: proj.config_dir().to_path_buf(),   // 설정
        data_dir: proj.data_dir().to_path_buf(),        // 데이터
        cache_dir: proj.cache_dir().to_path_buf(),      // 캐시
    }
}
```

`directories` 크레이트가 XDG Base Directory (Linux), Known Folders (Windows), Standard Directories (macOS)를 자동 처리한다.

---

## 설치 검증

`tasty setup` 완료 후 자동으로 실행되는 검증 루틴이다.

### 검증 항목

#### 1. GPU 렌더 테스트

```rust
fn verify_gpu_render() -> Result<(), VerifyError> {
    // 오프스크린 텍스처에 테스트 프레임 렌더링
    let test_surface = create_offscreen_texture(100, 100);
    render_test_frame(&test_surface)?;  // 배경 + 글리프 몇 개
    // 결과 텍스처 readback하여 예상 색상 확인
    let pixels = read_back_texture(&test_surface);
    assert_pixel_color(pixels[0], expected_bg_color)?;
    Ok(())
}
```

#### 2. PTY 테스트

```rust
fn verify_pty() -> Result<(), VerifyError> {
    let mut pty = PtyPair::spawn(detected_shell)?;
    pty.write(b"echo TASTY_TEST_OK\n")?;
    let output = pty.read_timeout(Duration::from_secs(3))?;
    assert!(output.contains("TASTY_TEST_OK"));
    Ok(())
}
```

#### 3. 폰트 렌더 테스트

```rust
fn verify_font_render() -> Result<(), VerifyError> {
    let test_strings = ["Hello, World!", "가나다라마바사", "你好世界", "🎉🚀"];
    for s in &test_strings {
        let glyphs = rasterize_string(s, &font_config)?;
        assert!(!glyphs.is_empty(), "글리프 래스터라이즈 실패: {}", s);
        // 모든 글리프가 .notdef가 아닌지 확인
        for g in &glyphs {
            if g.is_notdef {
                warn!("누락 글리프 감지: '{}' in '{}'", g.character, s);
            }
        }
    }
    Ok(())
}
```

#### 4. IPC 테스트

```rust
fn verify_ipc() -> Result<(), VerifyError> {
    #[cfg(unix)]
    {
        let socket_path = get_ipc_socket_path();
        let listener = UnixListener::bind(&socket_path)?;
        std::fs::remove_file(&socket_path)?;
    }
    #[cfg(windows)]
    {
        let pipe_name = get_ipc_pipe_name();
        // Named pipe 생성 테스트
        let pipe = NamedPipeServer::create(&pipe_name)?;
        drop(pipe);
    }
    Ok(())
}
```

### 검증 결과 출력

```
$ tasty setup

[1/10] GPU 감지...       NVIDIA RTX 4070 (Vulkan)
[2/10] 디스플레이 감지... 2560x1440 @ 144Hz, scale 1.5
[3/10] OS 감지...        Windows 11 (Build 26200)
[4/10] 폰트 감지...      JetBrains Mono + Noto Sans CJK KR
[5/10] 셸 감지...        pwsh 7.4.1 (OSC 7 지원)
[6/10] 하드웨어 감지...   8 cores, 32GB RAM, NVMe SSD
[7/10] 벤치마크...       2.1ms/frame → High preset
[8/10] config.toml 생성... ~/.config/tasty/config.toml
[9/10] 셸 통합 설치...   OSC 7 + 완성 파일
[10/10] 검증...

  ✓ GPU 렌더 테스트      통과 (3 프레임, 0 에러)
  ✓ PTY 테스트          통과 (pwsh spawn + echo)
  ✓ 폰트 렌더 테스트     통과 (ASCII + CJK + 이모지)
  ✓ IPC 테스트          통과 (named pipe 생성)

설치 완료. tasty를 실행하여 시작할 수 있다.
```

실패 시 구체적인 원인과 수정 방법을 제안한다:

```
  ✗ GPU 렌더 테스트      실패: wgpu adapter를 찾을 수 없음
    → 그래픽 드라이버를 업데이트하거나, tasty --software 로 소프트웨어 모드 실행

  ✗ 폰트 렌더 테스트     경고: CJK 글리프 누락 (가나다 → .notdef)
    → Noto Sans CJK 설치 권장: https://fonts.google.com/noto
```
