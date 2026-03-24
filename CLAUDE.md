# Tasty - 크로스 플랫폼 GPU 가속 터미널 에뮬레이터

## 프로젝트 개요

cmux(macOS 전용)에서 영감을 받은 크로스 플랫폼 GPU 가속 터미널 에뮬레이터.
Rust 기반 네이티브 GUI 앱으로 Windows, macOS, Linux를 모두 지원한다.
WezTerm/Alacritty와 유사한 접근이지만 AI 코딩 에이전트에 특화된 기능을 제공한다.

- 레포: git@github.com:zilhak/tasty.git
- 라이선스: MIT

## 기술 스택

| 역할 | 라이브러리 |
|------|-----------|
| 윈도우/입력 | winit |
| GPU 렌더링 | wgpu |
| UI 위젯 | egui (UI) + 커스텀 셰이더 (터미널) |
| VTE 파싱 | termwiz |
| PTY | portable-pty (Windows: ConPTY) |
| IPC | Unix socket (Linux/macOS), Named pipe (Windows) |
| CLI | clap |

## 작업 규칙

### 문서 갱신 (필수)

**모든 작업 완료 시 docs를 반드시 갱신할 것.**

- 새 기능이 구현되면 `docs/features.md`에 해당 기능을 추가하고 설명을 붙인다.
- 기존 기능이 변경되면 해당 문서의 설명을 업데이트한다.
- `docs/index.md`의 목차도 함께 갱신한다.
- 구현 히스토리는 남기지 않는다. 현재 상태만 기술한다.

### 커밋 규칙

Conventional Commits 형식을 따른다.

```
<type>: <description>

[optional body]
```

| 타입 | 용도 |
|------|------|
| feat | 새 기능 |
| fix | 버그 수정 |
| docs | 문서 변경 |
| refactor | 리팩토링 |
| test | 테스트 추가/수정 |
| chore | 빌드, 설정 등 기타 |

### 코드 컨벤션

- 언어: Rust
- 빌드: cargo
- 포맷: rustfmt
- 린트: clippy

## 개발 환경 주의사항

### Python 실행

이 Windows 환경에서 `python3` 명령은 정상 동작하지 않는다 (exit code 49, `-c` 인자 무시). Python이 필요할 때는 반드시 `python`을 사용할 것.

```bash
# 잘못됨 - 동작하지 않음
python3 -c "print('hello')"

# 올바름
python -c "print('hello')"
```

### TCP 통신 도구

`ncat`, `nc`, `netcat`이 설치되어 있지 않다. IPC 테스트 등에서 TCP 통신이 필요하면 Python socket을 사용할 것.

```python
import socket, json
s = socket.socket()
s.settimeout(5)
s.connect(('127.0.0.1', PORT))
s.sendall((json.dumps(request) + '\n').encode())
data = s.recv(4096)
s.close()
```

### Windows 프로세스 정리

`process.kill()`은 Windows에서 자식 프로세스를 종료하지 않는다. 프로세스 트리 전체를 종료하려면 `taskkill /F /T /PID`를 사용해야 한다. 테스트 harness의 Drop에서 이미 적용되어 있다.

### GUI 수동 테스트 (스크린샷 캡처)

GUI를 직접 확인하려면 앱을 백그라운드 실행 후 PowerShell로 스크린샷을 찍는다.

1. **프로세스 종료**: bash의 `taskkill`은 `/F` 플래그가 경로와 충돌한다. PowerShell을 사용할 것.
   ```bash
   powershell -Command "Get-Process tasty -ErrorAction SilentlyContinue | Stop-Process -Force"
   ```

2. **빌드 → 실행**: tasty.exe가 실행 중이면 cargo build가 exe를 덮어쓸 수 없다 (access denied). 반드시 프로세스를 먼저 종료한 후 빌드할 것.

3. **스크린샷 캡처**: PowerShell 스크립트 파일을 만들어 실행한다. bash에서 `$` 변수가 먹히므로 인라인 PowerShell은 동작하지 않는다.
   ```powershell
   # take_screenshot.ps1
   Add-Type -AssemblyName System.Windows.Forms, System.Drawing
   $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
   $bmp = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
   $g = [System.Drawing.Graphics]::FromImage($bmp)
   $g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
   $bmp.Save("E:\workspace\tasty\screenshot.png")
   $g.Dispose(); $bmp.Dispose()
   ```
   ```bash
   powershell -NoProfile -ExecutionPolicy Bypass -File take_screenshot.ps1
   ```

4. **윈도우 포커스/최대화**: `Win32::ShowWindow` + `SetForegroundWindow`로 Tasty 창을 최대화한 후 찍어야 전체가 보인다.

### egui 레이아웃 주의사항

- `CentralPanel` 안에서 `add_space`로 수동 중앙 배치하면 자식 Frame이 부모 너비를 무시하고 넘칠 수 있다.
- 정중앙 배치가 필요하면 `egui::Window`에 `.anchor(Align2::CENTER_CENTER, vec2(0,0))`을 쓰는 것이 안정적이다.
- wgpu 24에서 egui render pass에 `forget_lifetime()`이 필요하다 (`'static` 라이프타임 요구).
