# 설치 자동화 계획

> 이 문서는 **앞으로 도입할 가능성이 있는** 설치 자동화 기능을 정리한 비전 문서다.
> 현재 실제로 제공되는 배포 형태와 설치 절차는 [`docs/installation.md`](../installation.md) 참조.
> 빌드/배포 인프라(러너, 도구) 측면은 [`docs/dev-guide/build.md`](../dev-guide/build.md),
> [`docs/dev-guide/release-runners.md`](../dev-guide/release-runners.md) 참조.

---

## `tasty setup` 명령

바이너리 설치 후 실행되는 환경 감지 및 설정 자동 생성 프로세스다. **현재 미구현.**

### 실행 시점 (안)

- **첫 실행 자동 감지**: `config.toml`이 없으면 자동으로 `tasty setup` 실행
- **수동 실행**: `tasty setup` 명령
- **재설치**: `tasty setup --force` (기존 설정 백업 후 재생성)

### 실행 흐름 (안)

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

`tasty setup`이 호출하는 자동 감지 로직들. **현재 미구현.**

### GPU 감지

wgpu 어댑터 열거로 GPU 정보를 수집한다.

```rust
let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
    backends: wgpu::Backends::all(),
    ..Default::default()
});

let adapters: Vec<_> = instance.enumerate_adapters(wgpu::Backends::all()).collect();
```

**감지 항목:**

| 항목 | 용도 |
|------|------|
| `info.name` | 로그, 품질 프리셋 자동 선택 |
| `info.vendor` | GPU 벤더 판단 (NVIDIA/AMD/Intel/Apple) |
| `info.backend` | 사용 백엔드 확인 (`Vulkan`, `DX12`, `Metal`) |
| `info.driver` | 드라이버 버전 (호환성 이슈 추적) |
| `info.device_type` | 소프트웨어 렌더러 감지 |
| `limits.max_texture_dimension_2d` | 아틀라스 페이지 크기 결정 |
| `features` | 셰이더 변형 선택 |

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

### 폰트 감지

시스템 모노스페이스/CJK 폰트를 열거하여 폴백 체인 구성:

```
사용자 설정 폰트 (있으면)
  → 감지된 모노스페이스 폰트
    → 감지된 CJK 폰트
      → 번들된 폴백 폰트 (Noto Sans Mono)
        → 시스템 기본 모노스페이스
```

CJK 폰트가 없으면 경고 출력.

### 셸 감지

Unix는 `$SHELL`, Windows는 `pwsh` → `powershell.exe` → `cmd.exe` 순 탐색.

**OSC 7 지원:**

| 셸 | OSC 7 지원 | 비고 |
|-----|-----------|------|
| zsh 5.8+ | 기본 지원 | `.zshrc` 설정 필요 |
| bash 5.1+ | 수동 설정 필요 | `PROMPT_COMMAND`에 추가 |
| fish 3.0+ | 기본 지원 | 자동 |
| PowerShell 7+ | 수동 설정 필요 | `prompt` 함수 수정 |
| cmd.exe | 미지원 | CWD 폴링으로 대체 |

### 하드웨어 프로파일링

| 항목 | 용도 | 설정 매핑 |
|------|------|----------|
| CPU 코어 수 | 스레드 풀 크기 | `performance.thread_pool_size = min(cores, 16)` |
| 총 RAM | 스크롤백 버퍼 한도 | RAM < 4GB → 5000줄, < 8GB → 10000줄, ≥ 8GB → 50000줄 |
| 디스크 속도 (선택) | 세션 저장 주기 추천값 | SSD → 8초 (기본), HDD → 30초 추천 |

---

## 자동 생성 결과물

### config.toml 자동 생성

감지된 환경 정보 기반 최적화 설정을 auto-detected 주석과 함께 생성:

```toml
# Auto-detected by tasty setup (YYYY-MM-DD)
# System: ...
# GPU: ...
# Monitor: ...

[gpu]
backend = "vulkan"            # auto-detected: vulkan available via NVIDIA driver
atlas_size = 4096             # auto-detected: max_texture 16384, using 4096
quality = "high"              # auto-detected: benchmark 2.1ms/frame
vsync = true                  # auto-detected: 144Hz monitor

[display]
dpi_scale = 1.5
refresh_rate = 144

[font]
family = "JetBrains Mono"
fallback = ["Noto Sans CJK KR", "Noto Color Emoji"]

[terminal]
shell = "pwsh"
scrollback_lines = 50000
osc7_support = true

[performance]
thread_pool_size = 8
```

### 셸 통합 자동 설치

OSC 7 (CWD 보고) 훅을 셸 rc 파일에 추가하고, 백업을 남긴다.

- zsh → `~/.zshrc`에 `precmd_functions+=(__tasty_osc7)`
- bash → `~/.bashrc`의 `PROMPT_COMMAND`에 추가
- fish → `~/.config/fish/conf.d/tasty.fish`
- PowerShell → `$PROFILE`의 `prompt` 함수 수정

기존 셸 rc 파일은 `~/.zshrc.tasty-backup` 식으로 보존.

### 셸 완성 파일

clap의 `generate` 기능으로 zsh/bash/fish/PowerShell 완성 파일을 자동 생성.

| 셸 | 완성 파일 경로 |
|-----|---------------|
| zsh | `~/.local/share/tasty/completions/_tasty` + `fpath` 등록 |
| bash | `~/.local/share/bash-completion/completions/tasty` |
| fish | `~/.config/fish/completions/tasty.fish` |
| PowerShell | `~/.local/share/tasty/completions/tasty.ps1` + `$PROFILE`에 source |

---

## `tasty doctor` (런타임 진단)

설정과 환경의 정합성을 진단하는 명령. **현재 미구현.**

```
$ tasty doctor

✓ GPU: NVIDIA RTX 4070 (Vulkan) — 정상
✓ 디스플레이: 2560x1440 @ 144Hz, scale 1.5 — 정상
✓ 폰트: JetBrains Mono — 설치됨
✓ CJK 폰트: Noto Sans CJK KR — 설치됨
✓ 셸: pwsh 7.4.1 — OSC 7 지원
✗ 셸 완성: zsh 완성 파일 누락
  → 수정: tasty setup --shell-completions

요약: 7/8 통과, 1 문제
```

검사 항목: GPU 접근, 렌더 테스트, PTY 테스트, 폰트 렌더 테스트, IPC 테스트, 셸 통합 상태,
PATH, 셸 완성, 설정 유효성.

---

## 런타임 재감지

앱 시작 시 현재 하드웨어와 저장된 `config.toml`의 auto-detected 값을 비교, 변경 시 대응.

| 변경 유형 | 동작 |
|----------|------|
| GPU 변경 | 프롬프트: "GPU가 변경되었다. 재설정을 실행할까?" |
| DPI 변경 | 자동 조정 (아틀라스 재빌드, 폰트 재래스터라이즈) |
| 모니터 주사율 변경 | VSync 설정 자동 업데이트 |
| RAM 변경 | 로그 경고만 (스크롤백은 수동 조정) |

---

## 설치 검증 루틴 (`tasty setup --verify`)

`tasty setup` 완료 후 자동 실행되는 검증 루틴.

### 검증 항목

1. **GPU 렌더 테스트** — 오프스크린 텍스처에 테스트 프레임 렌더 후 readback
2. **PTY 테스트** — 셸 spawn + echo 왕복
3. **폰트 렌더 테스트** — ASCII / CJK / 이모지 글리프 래스터라이즈
4. **IPC 테스트** — Unix socket / Windows named pipe 생성

실패 시 구체적 원인과 수정 방법 제안.

---

## 명시적으로 진행 안 할 항목

다음은 비전 문서 초기에 적혔지만 현재 진행 계획에서 제외한다:

- **원라인 설치 스크립트** (`curl ... | sh`, `irm ... | iex`) — 별도 도메인 호스팅 필요
- **패키지 매니저 통합** — Homebrew tap, Scoop, WinGet, AUR `PKGBUILD`, Nix, `cargo install`
  레지스트리 등록 — 외부 인프라/리뷰 부담 대비 효과 불충분
- **Flatpak / Flathub** — 샌드박스 권한 모델·리뷰 비용 큼

배포 채널은 GitHub Releases에 올라가는 산출물(`.tar.gz`/`.deb`/`.rpm`/`.AppImage`/`.dmg`/`.zip`/`.msi`)로
한정한다. 자세한 현재 상태는 [`docs/installation.md`](../installation.md) 참조.
