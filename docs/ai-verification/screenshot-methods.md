# 스크린샷 방법

> **원칙: OS 화면 녹화를 먼저 쓰지 말 것.** tasty 자체 캡처(`ui.screenshot` IPC)가 실제 렌더한 프레임을 PNG 로 떨군다 — OS 화면 녹화 권한 불요, 다른 창 가림·포커스·최대화 상태에 영향받지 않음. OS 화면 캡처는 IPC 를 못 쓰는 상황(셸 설정 모드 등)에서만 폴백.

| 상태 | 방법 |
|------|------|
| 정상 모드 (IPC/CLI 가능) | **`tasty screenshot` CLI / `ui.screenshot` IPC** (권장·기본) |
| **`tasty-gallery` 검증** | **`TASTY_GALLERY_SHOT` env GPU 캡처** (갤러리는 IPC 없음 — 아래) |
| IPC 불가 (셸 설정 모드 등) | OS 화면 캡처 (최후 폴백) |

시각 판단 휴리스틱·체크리스트는 [visual-verification](visual-verification.md).

## `tasty screenshot` CLI / `ui.screenshot` IPC (권장)

정식 release 기능이다(더 이상 debug 전용 아님). **focus 독립** — 대상 창/surface 를 ID 로 직접
지정하며 focused 창에 의존하지 않는다(불가침 원칙 3). 에이전트가 *자기 작업을 관찰*하는
캡처라 사용자 상태(focus/가시 탭/선택)를 건드리지 않는다(원칙 1·2). 임의 경로 파일 쓰기
표면이라 `local_only`(plugin 미노출) — CLI/로컬 client 만 호출.

- GUI 모드 전용(headless 불가).
- 두 가지 대상:
  - **surface 캡처** `--surface <id>` — 그 **터미널** surface 를 **오프스크린 렌더**로 그 자체
    grid 크기(cols×rows)에 캡처한다. 배경 탭·다른 workspace·비-focus 창의 surface 도 찍히며
    swapchain/present/가시 프레임/focus 를 전혀 건드리지 않는다(`pending_surface_screenshot`
    → `GpuState::capture_surface_to_png`). 소유 창은 surface_id 로 자동 해소(창별 CoreState 순회).
    **터미널만 지원**(v1). egui 패널(explorer/markdown/image/html)·plugin·webview surface 는
    범위 밖 — 명확한 에러 반환.
  - **window 캡처** `--window <id>`(없고 main 창이 1개면 그 창; 그 외는 에러 — focus 기본값
    금지) — 그 창의 전체 프레임(chrome 포함)을 swapchain readback 으로 캡처
    (`pending_screenshot`). 명시한 id 는 **설정·플러그인·종료 확인 모달과 preset 창**도
    가리킬 수 있다(아래 "무엇을 캡처할 수 있는가").
- 응답 `{ "path": .., ("surface_id"|"window_id"): .., "scheduled": true }` — **다음 프레임에
  캡처 예약**(비동기). 호출 직후 잠깐 기다렸다 파일을 읽는다. 대상 창은 자동으로 redraw 를
  요청받아(비-focus 창도) 캡처가 발화한다.

```bash
# 터미널 surface 를 ID 로 (focus 무관, 배경 탭도 가능)
tasty screenshot --path /abs/out.png --surface 5
# 창 전체 프레임 (창 여러 개면 --window 필수; list windows 로 ID 조회)
tasty screenshot --path /abs/win.png --window 2
```

### 무엇을 캡처할 수 있는가 (경계)

**명시한 `--window <id>` 는 모든 창을 가리킬 수 있다** — main 창뿐 아니라 설정 · 플러그인 ·
종료 확인 모달과 preset 창까지. 별도 winit 창으로 뜨는 UI 를 자동 시각 검증할 수 있어야
디자인 정합 확인이 사람 눈에 의존하지 않는다. X11 화면 캡처는 GPU 창에서 검게 나오므로
대안이 되지 못한다.

경계는 **창의 종류가 아니라 두 가지 다른 축**에 있다 (근거·기각 대안·재검토 조건:
[ADR-0118](../adr/0118-screenshot-reads-any-window-explicit-id-only.md)).

1. **명시 지정만 넓어진다.** `--window` 를 생략했을 때의 자동 선택은 종전대로 **main 창이
   정확히 하나일 때뿐**이고, 모달로 폴백하지도 포커스를 보지도 않는다(불가침 원칙 3).
   자동 선택이 "사용자가 무엇을 열어두었는가" 에 의존하기 시작하면 같은 명령이 실행할
   때마다 다른 것을 찍는다.
2. **캡처는 읽기이지 행동이 아니다.** `window.list` 와 `window.close` 는 종전대로 main 창만
   다룬다 — 모달 · preset 은 사용자 조작 영역이라 에이전트 **행동** 대상이 아니다. 캡처가
   가능해져도 그 집합은 넓어지지 않는다.

원칙 1 은 에이전트 행동의 부수효과가 **사용자 상태**(포커스 / 닫은 항목 히스토리 / 선택 ·
스크롤 · 커서)에 닿는 것을 금지한다. 캡처는 이미 그려진 프레임의 readback + 리페인트
요청이고 리페인트는 멱등이라 그중 무엇도 바꾸지 않는다. 그리고 `ui.screenshot` 은
`local_only`(plugin 미노출)라 호출자는 이미 사용자 권한으로 `config.toml`(설정 창이 그리는
내용 전부)과 PTY 를 읽을 수 있다 — 모달 캡처를 막아도 새로 감춰지는 정보가 없고, 자동
검증만 잃는다.

### 모달 창의 ID 를 얻는 법

`list windows`(`window.list`)는 위 2 번 때문에 **main 창만** 열거한다. 모달 id 는 OS 창
목록에서 얻는다 — X11 에서 winit `WindowId` 는 **X11 window id 그 자체**라 그대로 넘길 수
있다(다른 플랫폼은 대응이 다르므로 이 방법은 X11 한정).

```bash
# 모달을 띄우고(debug 빌드 전용) X11 창 목록에서 id 를 고른다
tasty debug settings open --tab general
for w in $(xdotool search --pid "$TASTY_PID"); do
  echo "$w $(xdotool getwindowname "$w" 2>/dev/null)"
done
# xdotool 은 입력 전용 더미 창까지 뱉으므로 **창 이름으로 골라야 한다**(빈 이름 = 더미).
tasty screenshot --path /abs/settings.png --window <위에서 고른 id>
```

존재하지 않는 id 는 `Window id <id> not found` 로 거절된다.

CLI 없이 raw JSON-RPC(개행 구분)를 포트로 직접 보낼 수도 있다. 포트 파일은 debug 빌드면
**debug 루트** `~/.tasty-debug/tasty.port`, release 면 `~/.tasty/tasty.port` 다(루트 분리 —
격리 표 [independent-verification](../dev-guide/independent-verification.md), 구현
`crates/tasty-ipc/src/port_file.rs`).

```python
import socket, json
port = int(open("/Users/<you>/.tasty-debug/tasty.port").read().strip())  # debug 빌드 루트
req = {"jsonrpc":"2.0","id":1,"method":"ui.screenshot","params":{"path":"/abs/out.png","surface_id":5}}
s = socket.socket(); s.settimeout(8); s.connect(("127.0.0.1", port))
s.sendall((json.dumps(req)+"\n").encode())
print(s.recv(8192).decode().strip())   # {"result":{"path":..,"surface_id":5,"scheduled":true},..}
```

### 사용자 세션을 건드리지 않고 격리 실행

debug 빌드는 이미 `~/.tasty-debug/` 루트로 release(`~/.tasty/`)와 자동 분리되므로, 보통은 그냥 `./target/debug/tasty --launch` 로 띄우고 `~/.tasty-debug/tasty.port` 로 접속하면 사용자 release 세션과 충돌하지 않는다(격리 표·상세: [independent-verification](../dev-guide/independent-verification.md)).

루트를 명시적으로 분리하고 싶으면(병렬 debug 인스턴스 등) `TASTY_HOME` env 로 루트를 강제한다 — `tasty_home()` 이 `TASTY_HOME` 을 debug/release 자동 분기보다 우선한다(`crates/tasty-utils/src/path.rs`).

```bash
TH=$(mktemp -d); cp ~/.tasty/config.toml "$TH/"    # config 는 루트 바로 아래
TASTY_HOME="$TH" ./target/debug/tasty --launch &   # tasty 터미널 안에서면 GUI 부팅 skip 되므로 --launch 강제
MY_APP=$!                                          # 띄운 즉시 PID 를 잡는다
kill -0 "$MY_APP" || echo "기동 실패 — 로그를 본다"  # 실패를 확인 안 하면 이후가 전부 헛돈다
# "$TH/tasty.port" 로 ui.screenshot 호출 (TASTY_HOME 루트라 -debug 접미사 없음)
kill "$MY_APP"; rm -rf "${TH:?}"                   # 정리 — 저장한 PID 로만
```

**정리는 반드시 자기가 띄운 PID 로 한다.** 이름이나 명령줄 패턴으로 찾아서 죽이면
**자기 것이 아닌 인스턴스까지 죽인다** — 이 레포는 사용자 release · 다른 검증 세션 ·
병렬 lane 의 debug 인스턴스가 동시에 떠 있는 것이 일상이고, 실제로 그 형태가 다른
세션의 프로세스를 죽인 사고가 두 번 났다. 레포의 PreToolUse 훅도 같은 이유로 패턴
기반 프로세스 종료를 차단한다. `rm -rf` 의 대상에도 같은 원칙이 적용된다 —
`${TH:?}` 로 빈 변수가 경로가 되는 경우를 막는다.

PID 를 놓쳤다면 **패턴으로 찾아 죽이지 말고 소유자부터 확인한다.** 격리 실행은
`TASTY_HOME` 이 인스턴스마다 다르므로 그것이 신원이 된다(Linux):

```bash
for pid in $(pgrep -x tasty); do
  home=$(tr '\0' '\n' < "/proc/$pid/environ" | grep '^TASTY_HOME=' | cut -d= -f2-)
  echo "$pid  TASTY_HOME=${home:-<없음>}"
done
# 위 목록에서 "$TH" 와 일치하는 PID 하나만 골라 kill <PID>
```

macOS 는 `/proc` 이 없으므로 `ps -E -p <pid>` 로 같은 env 를 본다. 어느 쪽이든 내
것이라고 확정할 수 없으면 죽이지 않는다. 남의 인스턴스를 죽였다면 **무엇을 언제
죽였고 그래서 어떤 검증이 무효가 됐는지**를 보고에 적는다 — 무효가 된 검증을 유효한
것처럼 보고하는 쪽이 사고 자체보다 나쁘다.

테스트 격리용 `--port-file <PATH>` 옵션도 있다(클라이언트가 읽을 포트 파일 지정).

## `tasty-gallery` 캡처 (`TASTY_GALLERY_SHOT`)

갤러리(`tasty-gallery`)는 **별도 바이너리라 `ui.screenshot` IPC 가 없다.** 그렇다고 OS 캡처로 가면 권한 벽에 막힌다(아래). 대신 갤러리에 내장된 **env 트리거 일회성 GPU readback 캡처**를 쓴다 — 본체 `ui.screenshot` 과 동일한 swapchain readback(BGRA→RGB, 256B row 정렬)이라 권한 불요·결정적.

- 형식: `TASTY_GALLERY_SHOT=<idx>[@<y>]:<png>[,...]` — **배치**. 콤마로 여러 항목을 주면 **한 인스턴스에서** 순차로 선택→4프레임 settle→캡처하고 마지막에 **자체 종료**한다(콜드스타트 1회. `crates/tasty-gallery/src/main.rs`).
- `idx` 는 **페이지(Category) index**(0-base, `catalog::pages()` 순서 = Foundations 0 · Components 1 · Icons 2 · Overlays 3 · Layouts 4 · Plugins 5 · Chrome 6).
- `@<y>` 는 본문 **스크롤 오프셋(px)** 이다. 한 페이지에 섹션이 여러 개 쌓이면 상단 뷰포트만으로는 아래쪽 specimen 을 찍을 수 없으므로 그 자리로 강제 스크롤한다 — 임시 섹션을 꽂았다 되돌리는 우회가 필요 없다. 정확한 y 를 모르면 여러 오프셋을 한 배치로 훑고 맞는 컷을 고른다.
- 창 크기는 `TASTY_GALLERY_SIZE=<w>x<h>` 로 덮어쓴다(기본 1100×720). 문서 컬럼이 최대 1080 이라 기본 창에서는 우측이 잘린다 — specimen 전폭을 담으려면 넓혀서 찍는다.
- 갤러리는 캡처 후 스스로 종료하므로 `timeout` 불필요(macOS 엔 `timeout` 명령도 없다).

```bash
B="${TMPDIR:-/tmp}/tasty-shots"; mkdir -p "$B"
# 여러 specimen 한 방에 (init 1회): idx 3=Button, 6=Badge·Tag·Kbd, 9=MenuItem·TreeRow
TASTY_GALLERY_SHOT="3:$B/button.png,6:$B/chips.png,9:$B/nav.png" ./target/debug/tasty-gallery

# 페이지 중간 섹션(Layouts 페이지의 Task DAG)을 전폭으로
TASTY_GALLERY_SIZE=1360x1000 \
  TASTY_GALLERY_SHOT="4@5200:$B/dag-canvas.png,4@11000:$B/dag-surface.png" ./target/debug/tasty-gallery
# 윈도우 1100x720, 1:1(논-레티나) → 좌측 사이드바 ~240px, 우측이 specimen 패널
```

## OS 화면 캡처 (폴백만)

- **macOS** `screencapture` — 해당 프로세스에 화면 녹화 권한 필요(없으면 `could not create image from display` 실패 → `ui.screenshot` 또는 갤러리는 `TASTY_GALLERY_SHOT` 사용).
- **Windows** PowerShell `CopyFromScreen`. 윈도우가 가려져 있으면 `ShowWindow`+`SetForegroundWindow` 로 최대화 후 캡처. tasty.exe 실행 중이면 `cargo build` 가 exe 를 못 덮어쓰니 빌드 전 종료(`Stop-Process -Force`).
