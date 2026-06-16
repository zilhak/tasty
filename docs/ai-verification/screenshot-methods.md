# GUI 테스트 시 스크린샷 방법

> **원칙: OS 화면 녹화(`screencapture` / PowerShell `CopyFromScreen`)를 먼저 쓰지 말 것.**
> tasty 에는 자체 캡처 기능(`ui.screenshot` IPC)이 있다. 이게 tasty 가 실제로
> 렌더한 프레임을 PNG 로 떨어뜨리므로 OS 권한(화면 녹화 권한) 없이 동작하고,
> 다른 창에 가려지거나 포커스/최대화 상태에 영향받지 않는다.
> OS 화면 캡처는 **IPC 를 쓸 수 없는 상황(셸 설정 모드 등)에서만** 폴백으로 쓴다.

| 상태 | 방법 |
|------|------|
| 정상 모드 (IPC 사용 가능) | **`ui.screenshot` IPC** — tasty 자체 렌더링을 PNG 로 캡처 (권장·기본) |
| 셸 설정 모드 등 IPC 불가 | OS 화면 캡처 (macOS `screencapture` / Windows PowerShell `CopyFromScreen`) — 최후의 폴백 |

## 방법 1: `ui.screenshot` IPC (정상 모드, 권장)

- **debug 빌드 전용** (`#[cfg(debug_assertions)]`). release 에는 노출되지 않는다.
- GUI 모드 전용. headless 모드에서는 사용 불가.
- params: `{ "path": "<절대경로>.png" }`. 응답은 `{ "path": .., "scheduled": true }` —
  다음 프레임에 캡처가 예약된다(비동기). 호출 직후 잠깐 기다렸다 파일을 읽는다.

`tasty screenshot` 같은 **CLI 서브커맨드는 없다.** JSON-RPC(개행 구분) 를 IPC 포트로
직접 보낸다. 포트 번호는 포트 파일에서 읽는다 — **debug: `~/.tasty/tasty-debug.port`**,
release: `~/.tasty/tasty.port` (`crates/tasty-ipc/src/port_file.rs`).

```python
import socket, json
port = int(open("/Users/<you>/.tasty/tasty-debug.port").read().strip())
req = {"jsonrpc": "2.0", "id": 1, "method": "ui.screenshot",
       "params": {"path": "/abs/out.png"}}
s = socket.socket(); s.settimeout(8); s.connect(("127.0.0.1", port))
s.sendall((json.dumps(req) + "\n").encode())
print(s.recv(8192).decode().strip())  # {"result":{"path":..,"scheduled":true},..}
s.close()
```

### 사용자 세션을 건드리지 않고 격리 실행 (검증용)

`tasty_home()` 은 `$HOME/.tasty` 고정이라 env override 가 없다. 사용자가 이미 tasty 를
띄워둔 상태에서 검증용 인스턴스를 따로 돌리려면 **별도 `HOME` 으로 실행**한다.
debug 빌드는 포트 파일명이 `tasty-debug.port` 라 release 사용자 세션과 충돌하지 않는다.

```bash
TH=$(mktemp -d); mkdir -p "$TH/.tasty"; cp ~/.tasty/config.toml "$TH/.tasty/"
# tasty 터미널 안에서 실행하면 GUI 부팅을 skip 하므로 --launch 강제 필요
HOME="$TH" ./target/debug/tasty --launch &
# 뜬 뒤 위 python 으로 "$TH/.tasty/tasty-debug.port" 포트에 ui.screenshot 호출
# 끝나면: pkill -f "target/debug/tasty --launch"; rm -rf "$TH"
```

테스트 격리용으로 `--port-file <PATH>` CLI 옵션도 있다(클라이언트가 읽을 포트 파일 지정).

## 방법 2: OS 화면 캡처 (IPC 불가 시 폴백만)

IPC 를 쓸 수 없을 때(셸 설정 모드 등)만 사용한다.

- **macOS**: `screencapture` 는 해당 프로세스에 화면 녹화 권한이 있어야 한다
  (권한 없으면 `could not create image from display` 로 실패). 권한이 없으면 방법 1 을 쓴다.
- **Windows**: PowerShell `CopyFromScreen`.

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

- 프로세스 종료는 bash `taskkill /F` 대신 PowerShell 사용:
  `powershell -Command "Get-Process tasty -ErrorAction SilentlyContinue | Stop-Process -Force"`
- tasty.exe 실행 중이면 `cargo build` 가 exe 를 못 덮어쓴다(access denied) — 빌드 전 종료.
- 윈도우가 가려져 있으면 `Win32::ShowWindow` + `SetForegroundWindow` 로 최대화 후 캡처.
