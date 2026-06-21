# 스크린샷 방법

> **원칙: OS 화면 녹화를 먼저 쓰지 말 것.** tasty 자체 캡처(`ui.screenshot` IPC)가 실제 렌더한 프레임을 PNG 로 떨군다 — OS 화면 녹화 권한 불요, 다른 창 가림·포커스·최대화 상태에 영향받지 않음. OS 화면 캡처는 IPC 를 못 쓰는 상황(셸 설정 모드 등)에서만 폴백.

| 상태 | 방법 |
|------|------|
| 정상 모드 (IPC 가능) | **`ui.screenshot` IPC** (권장·기본) |
| **`tasty-gallery` 검증** | **`TASTY_GALLERY_SHOT` env GPU 캡처** (갤러리는 IPC 없음 — 아래) |
| IPC 불가 (셸 설정 모드 등) | OS 화면 캡처 (최후 폴백) |

시각 판단 휴리스틱·체크리스트는 [visual-verification](visual-verification.md).

## `ui.screenshot` IPC (권장)

- **debug 빌드 전용**(`src/app/ipc/window_required.rs` 가 게이트). release 미노출.
- GUI 모드 전용(headless 불가).
- params `{ "path": "<절대경로>.png" }`. 응답 `{ "path": .., "scheduled": true }` — **다음 프레임에 캡처 예약**(비동기, `pending_screenshot`). 호출 직후 잠깐 기다렸다 파일을 읽는다.
- `tasty screenshot` 같은 CLI 는 없다. JSON-RPC(개행 구분)를 포트로 직접 보낸다. 포트 파일은 **debug `~/.tasty/tasty-debug.port`** / release `~/.tasty/tasty.port`(`crates/tasty-ipc/src/port_file.rs`).

```python
import socket, json
port = int(open("/Users/<you>/.tasty/tasty-debug.port").read().strip())
req = {"jsonrpc":"2.0","id":1,"method":"ui.screenshot","params":{"path":"/abs/out.png"}}
s = socket.socket(); s.settimeout(8); s.connect(("127.0.0.1", port))
s.sendall((json.dumps(req)+"\n").encode())
print(s.recv(8192).decode().strip())   # {"result":{"path":..,"scheduled":true},..}
```

### 사용자 세션을 건드리지 않고 격리 실행

`tasty_home()` 은 `$HOME/.tasty` 고정(env override 없음). 사용자가 띄워둔 tasty 와 충돌 없이 검증 인스턴스를 돌리려면 **별도 `HOME`** 으로 실행한다. debug 빌드는 포트 파일명이 `tasty-debug.port` 라 release 세션과 자연 분리된다.

```bash
TH=$(mktemp -d); mkdir -p "$TH/.tasty"; cp ~/.tasty/config.toml "$TH/.tasty/"
HOME="$TH" ./target/debug/tasty --launch &   # tasty 터미널 안에서면 GUI 부팅 skip 되므로 --launch 강제
# "$TH/.tasty/tasty-debug.port" 로 ui.screenshot 호출
# 정리: pkill -f "target/debug/tasty --launch"; rm -rf "$TH"
```

테스트 격리용 `--port-file <PATH>` 옵션도 있다(클라이언트가 읽을 포트 파일 지정).

## `tasty-gallery` 캡처 (`TASTY_GALLERY_SHOT`)

갤러리(`tasty-gallery`)는 **별도 바이너리라 `ui.screenshot` IPC 가 없다.** 그렇다고 OS 캡처로 가면 권한 벽에 막힌다(아래). 대신 갤러리에 내장된 **env 트리거 일회성 GPU readback 캡처**를 쓴다 — 본체 `ui.screenshot` 과 동일한 swapchain readback(BGRA→RGB, 256B row 정렬)이라 권한 불요·결정적.

- 형식: `TASTY_GALLERY_SHOT=<item_index>:<png_절대경로>`. 지정 카탈로그 항목을 선택해 4 프레임 settle 후 캡처하고 **자체 종료**한다(`crates/tasty-gallery/src/main.rs`).
- `item_index` 는 `catalog::all()` 순서(0-base). 목록: `grep -n 'name: "' crates/tasty-gallery/src/catalog.rs`.
- 갤러리는 캡처 후 스스로 종료하므로 `timeout` 불필요(macOS 엔 `timeout` 명령도 없다).

```bash
out="$PWD/.claude-workspace/temp/gallery-button.png"
TASTY_GALLERY_SHOT="3:$out" ./target/debug/tasty-gallery   # idx 3 = "Button"
# 윈도우 1100x720, 1:1(논-레티나) → 좌측 사이드바 ~240px, 우측이 specimen 패널
```

## OS 화면 캡처 (폴백만)

- **macOS** `screencapture` — 해당 프로세스에 화면 녹화 권한 필요(없으면 `could not create image from display` 실패 → `ui.screenshot` 또는 갤러리는 `TASTY_GALLERY_SHOT` 사용).
- **Windows** PowerShell `CopyFromScreen`. 윈도우가 가려져 있으면 `ShowWindow`+`SetForegroundWindow` 로 최대화 후 캡처. tasty.exe 실행 중이면 `cargo build` 가 exe 를 못 덮어쓰니 빌드 전 종료(`Stop-Process -Force`).
