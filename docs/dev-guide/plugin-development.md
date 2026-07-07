# 플러그인 제작 가이드

외부 Tasty 플러그인을 작성·빌드·설치하는 법. 개념(배포/통합 축·권한)은 [concepts/plugins](../concepts/plugins.md) 먼저. 권한 모델 상세는 [plugin-permissions](plugin-permissions.md), 민감 데이터는 [plugin-sensitive-data](plugin-sensitive-data.md).

**번들 플러그인이 곧 reference 예제다** — 각 기여 타입을 만들 때 아래 표의 해당 플러그인 코드를 시작점으로 복사·수정하는 게 가장 빠르다.

## 기여 타입 → 예제 플러그인

| 만들고 싶은 것 | 보면 되는 번들 플러그인 | 난이도 |
|---------------|------------------------|--------|
| **egui-mesh surface** (자가 렌더 mesh 합성) | [image](../plugins/image/index.md) · [markdown](../plugins/markdown/index.md)(+파일 핸들러·settings) · [mesh-demo](egui-mesh-channel.md)(최소 PoC) | ★★ |
| **webview surface** | [html](../plugins/html/index.md) | ★ |
| **도구 메뉴 항목 + popup** | [git-viewer](../plugins/git-viewer/index.md)(view/logic 분리) · [clipboard-viewer](../plugins/clipboard-viewer/index.md)(master-detail) | ★★ |
| **CLI + IPC namespace** | [codex](../plugins/codex/index.md) · [claude](../plugins/claude/index.md) | ★★★ |
| **이벤트 구독 / 훅 / 외부 설치** | [claude](../plugins/claude/index.md)(`surface.closed`·Claude 훅·install) | ★★★ |
| **wasm 플러그인** (frozen POC) | `crates/tasty-plugin-sdk-wasm`(workspace-exclude harness) — [ADR-0009](../adr/0009-plugin-sandbox-deferred.md) | ★★ |

전부 `crates/tasty-plugin-<name>/` 에 있다.

## 개요

플러그인은 **별도 OS 프로세스**로 실행되어 호스트와 TCP+NDJSON 으로 통신한다. 호스트는 `~/.tasty/plugins/<id>/` 에서 매니페스트를 발견하면 자동 spawn 한다. 작성자는 SDK(`tasty-plugin-sdk`)의 `Plugin` trait 을 구현하고 `run()` 을 호출하면 — SDK 가 핸드셰이크(토큰 인증, AuthAck 5초 대기)·NDJSON 직렬화·dispatch loop·ping/shutdown 을 가린다.

플러그인이 contribute 할 수 있는 것은 [concepts/plugins 통합 축](../concepts/plugins.md#통합-축--host-에-무엇을-기여하나) 참고. **contribute 0 개여도 valid** (예: 다른 surface 닫힘만 관찰).

## 1. 크레이트 골격 + 매니페스트

```
my-plugin/
  Cargo.toml          # [[bin]] + tasty-plugin-sdk 의존
  tasty-plugin.toml   # 매니페스트
  src/main.rs
```

매니페스트 필수 필드: `manifest_version` · `id`(reverse-DNS, 전역 유일) · `name` · `version`(semver) · `api_version` · `[entry]`. 실제 contribute 하는 항목만 추가 선언한다.

```toml
manifest_version = 1
id = "com.example.myplugin"
name = "My Plugin"
version = "0.1.0"
api_version = "1"
permissions = ["fs.read", "surface.write"]
lang_dir = "lang"

[entry]
type = "process"
command = "my-plugin"

[[surface_kinds]]
kind = "myplugin_main"               # 소문자 + '_' + 숫자
display_name_i18n_key = "surface.kind.myplugin"
```

## 2. Plugin trait

```rust
use tasty_plugin_sdk::{Plugin, SurfaceCreateCtx, SurfaceResult, ui::{label, vbox, button}};

struct MyPlugin { counter: u32 }

impl Plugin for MyPlugin {
    fn id(&self) -> &str { "com.example.myplugin" }
    fn version(&self) -> &str { "0.1.0" }
    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        SurfaceResult { tree: Some(self.build_tree()), display_name: Some("My Plugin".into()) }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(/* RUST_LOG */ "info").init();
    tasty_plugin_sdk::run(MyPlugin { counter: 0 })
}
```

contribute 한 항목에 대응하는 콜백만 채우면 된다 — surface 가 없으면 `create_surface` 는 호출되지 않는다.

## 3. 기여 타입별 작성

각 타입은 매니페스트 선언 + (필요 시) trait 콜백. 자세한 매니페스트 스니펫은 해당 예제 플러그인의 `tasty-plugin.toml` 을 본다.

### Surface kind — `rendering` 3 종

- **`rendering = "egui-mesh"`** (markdown/image/mesh_demo): 플러그인이 **자기 프로세스에서 egui 를 tessellate** 한 mesh 를 host 가 전용 `egui_wgpu::Renderer` 로 합성. SDK 를 `features=["egui-mesh"]` 로 받아 `paint_surface` 에서 `EguiMeshSurface::paint(...)` 호출. bundled 화이트리스트 + api_version gate. plugin-content 를 그리는 **유일한 렌더 채널**(ADR-0028). 채널 상세는 [egui-mesh-channel](egui-mesh-channel.md).
- **`rendering = "webview"`** (html): host 의 네이티브 WebView 오버레이로 그림. surface 의 URL 을 host 가 동기화.
- **`rendering = "remote"` (기본)**: webview 와 같은 `RemoteSurface` stand-in 등록만 하는 marker — host 는 이 kind 의 콘텐츠를 그리지 않는다. `snapshot_surface`/`restore_surface` 로 세션 복원.

surface kind 선언에는 host 가 kind-agnostic 하게 소비하는 메타가 함께 실린다 — host 본체에 `if kind == "..."` 를 박지 않기 위한 것들이다:

- **`icon`** — 탭/프리셋 leading 아이콘의 **이름**. host 가 자기 아이콘 세트에서 `icons::from_name` 으로 glyph 에 매핑한다(현재 이름: `markdown`/`folder`/`image`/`html`/`terminal`/`file`; 미지의 이름은 `file` 로 fallback). 예: markdown 의 `icon = "markdown"`.
- **`preset_fields`** — 프리셋 편집기가 이 kind 를 편집할 때 노출할 입력 필드 스키마. `required = true` 인 `param_key` 는 surface 생성 IPC(`pane.split`/`workspace.new`)의 **필수 파라미터**로도 쓰인다(단일 진실원). 예: markdown 은 `file` 필드 하나(required).
- **`param_aliases`** — 옛 caller 가 넘기는 alias 키 → canonical 키 매핑. host 가 convert 경로에서 정규화한다. 예: markdown 의 `{ file_path = "file" }`.
- **`default_params`** — surface 생성 시 params 에 없으면 host 가 주입하는 기본값(키 → 리터럴 또는 정책 토큰). 정책 토큰: `@settings.explorer_view_mode`(Settings 의 마지막 explorer view mode), `@home`(홈 디렉토리 — **새 탭 생성 fresh-context 에서만** 해석; split/preset/workspace 처럼 cwd 를 상속·carry 하는 경로에선 건너뛴다). 예: explorer(builtin)의 `{ view_mode = "@settings.explorer_view_mode", path = "@home" }`.
- **capability flags**(모두 기본 false) — host 의 입력/줌/복사/붙여넣기 게이트를 kind 하드코딩 없이 판정한다:
  - **`consumes_egui_input`** — host 가 이 kind 를 host egui 위젯으로 렌더해 winit 키/IME 를 host egui 로 흘린다(예: explorer). egui-mesh 렌더 kind 는 false(중앙 키 디스패처가 forward).
  - **`zoomable`** — 줌 in/out/reset 단축키로 폰트 크기 override 조절(예: markdown/explorer).
  - **`egui_copy`** — copy 단축키가 egui `Event::Copy` 를 주입(선택 텍스트를 plugin egui 가 복사, 예: markdown).
  - **`copy_path`** — select-all / copy-path 단축키(선택 항목 경로 복사) 소비(예: explorer).
  - **`egui_paste`** — paste 를 이 kind 가 자체 소비(host 가 terminal paste 로 흘리지 않음, 예: image).
- **`name_from_param`** — 자동 탭 명명 시 basename 을 파생할 params 키. 선언하면 그 키 값의 basename 을 탭 표시명으로 쓴다(예: markdown/image 는 `"file"`, explorer(builtin)는 `"path"` → `README.md`). 미선언이면 kind 표시명(`display_name_i18n_key`)으로 fallback. host 의 `kind == "markdown"` basename 명명 하드코딩을 대체.
- **`size_confirm_limit`** — 파일 핸들러로 이 kind 를 열 때 확인 팝업을 띄우는 크기 임계값(bytes). 선언하면 파일이 그 값을 *초과* 할 때 즉시 열지 않고 확인 팝업을 띄운다(예: markdown 은 `1048576` = 1MB). 미선언이면 게이트 없음(항상 즉시 열기). host 의 `kind == "markdown"` 1MB 게이트 하드코딩을 대체.

### 파일 핸들러 (detector + handler)

확장자 → surface 매핑. `[[contributes.detector]]`(확장자 규칙) + `[[contributes.handler]]`(`action = open_surface{surface_kind}`). 권한: `file_handler.define`(신규 detector) / `file_handler.extend:<id>` / `file_handler.handle:<id>`. handler `id` 는 short name — install 단계가 `<plugin_id>/<id>` 로 자동 prefix. priority 동순위면 owner tiebreak `user > plugin > host`. 예: [image](../plugins/image/index.md)·[markdown](../plugins/markdown/index.md).

### 도구 메뉴 항목 + popup

- `[[contributes.tool]]`(`ui.tool_item`) — [도구 메뉴](../features/tools-menu/index.md)에 항목. `action.kind`: `event`(Event Bus 발화) / `open_surface`(탭 추가) / `open_popup`(`popup_id = <plugin_id>/<id>`). `order_hint` 오름차순(빌트인 0..99).
- `[[contributes.popup]]`(`ui.popup`) — trigger `event`(자동 open) 또는 `ipc`(명시 호출). SDK 콜백 `open_popup`/`paint_popup`/`on_popup_closed`(egui-mesh). 동일 `popup_id` 라도 `instance_id` 가 다르면 별개 인스턴스. 예: [git-viewer](../plugins/git-viewer/index.md)·[clipboard-viewer](../plugins/clipboard-viewer/index.md).

### CLI + IPC namespace

`[[contributes.ipc_namespace]]`(prefix) + `[[contributes.cli]]`(`tasty <name> …`). 플러그인은 `handle_ipc_method` 로 `<prefix>.*` 메서드를 받는다. prefix 는 소문자+숫자+`_`, 호스트 예약어 금지(`system`/`surface`/`tab`/`pane`/`workspace`/`window`/… ). CLI 서브커맨드의 `ipc_method` 는 자기 prefix 와 매칭돼야 한다. 예: [codex](../plugins/codex/index.md)·[claude](../plugins/claude/index.md).

### 단축키 (commands)

`[[contributes.commands]]` — `id` · `default_keybinding` · `binding_mode`(`independent` 또는 `inherit:<host_action>`). 호스트가 키 매칭 시 `command.invoke` → SDK `handle_command`. 플러그인 키는 **focus surface 가 그 플러그인 소유일 때만** 호스트 키보다 우선.

### 설정 페이지

`[[contributes.settings_pages]]`(`ui.settings_page`) — [설정 창](../features/settings/index.md)에 sub-tab 동적 등록. `category`(appearance/general/keybindings/plugin/…). 플러그인 비활성 시 sub-tab 자동 소멸. 예: [markdown](../plugins/markdown/index.md).

`[[contributes.settings_pages.items]]` 의 `kind` (공통 필드: `id` · `label_key` · `storage_key`):

- `font_override` — surface 폰트 override. host 가 `plugin_font_overrides.<storage_key>` 슬롯에 read/write (아래 generic 컨트롤과 **별개 전역 네임스페이스**).
- `toggle` — on/off. `default`(bool). host 는 Switch 로 렌더, bool 저장.
- `select` — 드롭다운. `options = [{ value, label_key }]` + `default`(반드시 options.value 중 하나). Select 로 렌더, 선택 value(문자열) 저장.
- `number` — 수치. `default`(f64) · `min`/`max`(선택; 주어지면 min≤default≤max) · `suffix_key`(선택, 단위 i18n 키). DragValue 로 렌더, f64 저장.

`toggle`/`select`/`number` 값은 `plugin_settings.<plugin_id>.<storage_key>` 슬롯(`PluginSettingValue` = Bool/Text/Number)에 저장·영속된다 — `font_override` 의 전역 슬롯과 충돌하지 않는 plugin-scoped 네임스페이스. 예: [html](../plugins/html/index.md) 이 HTML viewer 설정(zoom/color scheme/allow remote content/sandbox scripts)을 이 방식으로 노출.

### 이벤트 구독 / 윈도우 / 확장

- **event_subscribe** — `event_subscribe = ["surface.closed"]` + `on_start` 에서 `bus.subscribe(...)`. `on_event` 로 envelope 수신(`reason`: user/ipc/crash). 예: [claude](../plugins/claude/index.md)/[codex](../plugins/codex/index.md).
- **window** — `[[contributes.window]]`(`window.spawn`). 현재는 schema + 등록 stub 까지(실 spawn 은 별도 영역).
- **extension** — 다른 플러그인의 IPC/event 흐름을 가로채기. `[extends]` + `ext:<target>` 권한 + `handle_extension_hook`. mode: `transform`/`filter`/`observe`. target 당 활성 1개(나머지 `Conflict`). fail-open(timeout/에러 시 원래 값 사용).

## 4. Plugin UI 렌더 (egui-mesh 채널)

plugin 이 그리는 모든 UI(surface/popup/banner)는 egui-mesh 채널 하나로 통한다 —
plugin 이 자기 프로세스에서 egui 를 구동해 tessellate 한 `(ClippedPrimitive,
TexturesDelta, ppp)` 를 SharedBuffer 로 host 에 보내고 host 가 합성한다. 위젯 어휘
제한이 없고(egui 전부 사용 가능) 색·간격은 host 가 forward 한 `Theme` 토큰에서
가져온다. 상세·SDK 헬퍼(`EguiMeshSurface`/`EguiMeshPopup`/`EguiMeshBanner`)는
[egui-mesh-channel](egui-mesh-channel.md).

**chrome 아이콘**(툴바·주소창 등)은 raw 유니코드 글리프로 그리지 말고 `tasty-icons`
canonical 아이콘을 쓴다. plugin `build.rs` 가 `[build-dependencies] tasty-icons`(egui off)
+ usvg 로 `Icon.svg` 를 평탄화해 점배열을 `OUT_DIR` 에 베이크하고, 런타임엔
`tasty_plugin_sdk::baked_icon::draw(painter, icon, center, size, color)` 로 텍스처 없이
DPI 독립·theme tint 벡터 stroke 로 그린다. 새 아이콘 = `tasty-icons` 에 const 추가 +
plugin `build.rs` 의 `ICONS` 목록에 한 줄. 근거·대안은 [ADR-0036](../adr/0036-plugin-icon-buildtime-bake-tasty-icons-single-source.md).

## 5. 호스트 IPC 호출

`HostHandle::call("surface.list", json!({}))` — 매니페스트에 해당 권한 선언 + grant 필요. `?` 한 번으로 `PluginError → IpcMethodError` 변환. 주요 에러 variant: `HostCall{message}`(permission_denied 등) · `HostCallTimeout` · `HandshakeRejected/Timeout` · `HostClosed`. 권한↔메서드 매핑은 [plugin-permissions](plugin-permissions.md).

## 6. 데이터 저장 위치

| 데이터 | 위치 | 비고 |
|--------|------|------|
| 정적 자산(아이콘/lang/README) | `TASTY_PLUGIN_DIR` | **읽기 전용** — 업그레이드 시 통째 교체 |
| 사용자 편집 설정 | `TASTY_PLUGIN_CONFIG_PATH` | 업그레이드 보존 |
| DB·캐시·로그 | `TASTY_PLUGIN_DATA_DIR` | **쓰기 OK**, 업그레이드 보존 |
| 작업 메타/진행 상태(≤1 MiB) | `memory.*` / `memory.secret.*` | host SQLite. cap 초과는 `ValueTooLarge` |
| **진짜 민감 데이터**(토큰/키/자격증명) | **OS keyring** | secret 영역 금지 — [plugin-sensitive-data](plugin-sensitive-data.md) |

`memory.secret` 의 유일한 보장은 **플러그인 간 IPC 격리**다 — 디스크엔 평문. regular vs secret vs keyring 선택은 [plugin-sensitive-data](plugin-sensitive-data.md).

## 7. 호스트 런타임 계약 (env · 생명주기 · 핸드셰이크)

플러그인 *작성* 과 별개로, 호스트가 플러그인 프로세스를 어떻게 띄우고 살려두는지 — SDK 가 의존하는 런타임 계약(`crates/tasty-host-plugin/`).

### spawn 시 주입 환경변수

호스트가 자식 프로세스에 넘기는 env (`process.rs`). SDK 가 이걸로 자기 위치·로그·호스트 접속을 안다.

| 환경변수 | 값 |
|----------|-----|
| `TASTY_PLUGIN_ID` | plugin id |
| `TASTY_PLUGIN_DIR` | 본체 디렉터리(읽기 전용) |
| `TASTY_PLUGIN_DATA_DIR` / `TASTY_PLUGIN_CONFIG_PATH` / `TASTY_PLUGIN_LOG_PATH` | 데이터·설정·로그 경로 |
| `TASTY_HOST_IPC_PORT` | 호스트 listener 포트 |
| `TASTY_PLUGIN_TOKEN` | 핸드셰이크 토큰(1회용) |
| `TASTY_HOST_API_VERSION` | 호스트 protocol 메이저 |
| `TASTY_PLUGIN_HANDLE_ENDPOINT` | handle 채널 엔드포인트(있을 때) |
| `TASTY_LOCALE` | 활성 로케일(i18n Translator) |
| `TASTY_HOST_PID` | 호스트 프로세스 PID (**macOS 만** — SDK watchdog 가 부모 사망 감지에 사용) |

### 생명주기 (healthcheck / 자동 재시작·비활성화)

`manager.rs` 상수 기준:

- **부팅**: `~/.tasty/plugins/` 스캔 → enabled 전부 spawn.
- **헬스체크**: `PING_INTERVAL`(15s)마다 ping, `HEALTHCHECK_TIMEOUT`(60s) 무응답이면 강제 재시작.
- **자동 비활성화**: `RESTART_FAILURE_WINDOW`(10s) 내 `RESTART_FAILURE_LIMIT`(3)회 spawn 실패 → 정지(사용자가 `tasty plugin enable` 로 수동 재개까지).
- **종료**: shutdown 메서드 송신 후 timeout, 초과 시 kill.

### 프로세스 수명 결박 (3 OS — 크래시·강제종료 포함)

위 "종료" 경로는 `PluginProcess::shutdown` / `Drop` 의 `child.kill()` 에 의존하므로 **정상 종료만** 커버한다. 하드 크래시·`taskkill /f`·디버거 강제종료 등 Drop 이 돌지 않는 경로에서는 플러그인이 고아로 잔존할 수 있다. 이를 OS 커널 레벨에서 막기 위해, 호스트가 어떤 식으로 죽든 플러그인이 함께 종료되도록 결박한다 (`crate::reaper::PluginReaper`, spawn 시 `prepare`/`adopt` 배선). OS 별 메커니즘이 비대칭이라 단일 추상화 뒤에 숨긴다:

| OS | 메커니즘 | 통합 지점 | 손자(node/chrome) |
|----|----------|-----------|--------------------|
| **Windows** | Job Object (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`). 호스트가 Job 핸들을 `PluginManager` 수명 동안 소유 → 호스트 사망 시 핸들 닫히며 OS 가 Job 내 전 프로세스 강제 종료. | 호스트 `adopt`(각 자식 assign) | **자동 커버**(Job 멤버십 자식 상속) |
| **Linux** | `prctl(PR_SET_PDEATHSIG, SIGKILL)` (자식 `pre_exec`). 부모 사망 시 커널이 직속 플러그인에 SIGKILL. | 호스트 `prepare`(pre_exec) | 고아 허용(범위 밖) |
| **macOS** | PDEATHSIG 등가물 부재 → SDK 런타임 watchdog 이 `getppid` 폴링(500ms)해 부모 PID 변화 감지 시 self-exit. 호스트는 `TASTY_HOST_PID` env 만 주입. | 플러그인 SDK(`runtime.rs`) | 고아 허용(범위 밖) |

모든 결박 실패(Job 생성/assign 실패 등)는 `tracing::warn!` 으로 흡수하고 기존 kill 기반 정리로 degrade — 결박 실패가 기능이나 호스트를 죽이지 않는다.

**결박 대상은 플러그인에 국한되지 않는다.** PTY pane 의 사용자 셸과 그 안에서 돌던 모든 것(AI 에이전트·빌드·MCP 서버 등 자식 트리)도 호스트 수명에 묶인다 — **tasty 가 죽으면(정상 종료·크래시·`taskkill /f`·디버거 강제 stop 무관) tasty 안에서 돌던 프로세스는 함께 종료된다.** 메커니즘은 OS 별로 다르지만 결과는 같다:

- **Windows**: 터미널 셸을 전역 호스트 Job Object 에 결박한다 (공용 primitive `tasty-reaper`, `Terminal::new` 이 spawn 직후 `adopt_pid`, 부팅 시 `boot.rs` 에서 `init_host_reaper` 1회). ConPTY 는 "pseudoconsole 종료 ⇒ 자식 종료" 를 보장하지 않아, 결박이 없으면 tasty 비정상 종료 시 화면 없는 좀비 셸 트리가 누적된다(개발 중 디버거 stop 마다 수십 개씩). 플러그인 job(위 표, `PluginManager` 소유)과 터미널 job(전역)은 별개 인스턴스지만 둘 다 `KILL_ON_JOB_CLOSE` 라 프로세스 사망 시 동일하게 정리된다.
- **Unix**: tasty 종료 시 커널이 PTY master fd 를 닫으며 발생하는 SIGHUP 이 셸 foreground 프로세스 그룹을 정리하므로 별도 결박 없이 같은 결과가 난다(portable-pty `CommandBuilder` 가 `pre_exec` 를 노출하지 않아 셸에는 PDEATHSIG 설치 불가 — 대신 SIGHUP 이 그 역할을 한다).

정상 종료 경로(surface 닫기/quit)에서는 `PtyBackend::Drop` 이 셸을 명시적으로 kill 해 PTY master HUP 에만 의존하지 않는다. 결정 배경·대안·재검토 조건은 [ADR-0034](../adr/0034-terminal-shell-host-lifetime-binding.md).

### 토큰 핸드셰이크 (보안)

호스트가 `127.0.0.1:0`(랜덤 포트) listen → spawn 시 `TASTY_HOST_IPC_PORT` + `TASTY_PLUGIN_TOKEN` 전달 → 플러그인이 그 포트로 connect 후 **첫 줄에 `AuthMessage{plugin_id, token}`** 전송 → 토큰 일치해야 인증 통과(`HANDSHAKE_TIMEOUT` 내), mismatch 면 즉시 끊음. SDK transport 가 이 핸드셰이크를 자동 수행하므로 작성자는 보통 신경 쓸 필요 없다.

## 8. 규약

- **이름**: crate `tasty-plugin-<name>` = binary 이름, id `com.x.<name>`(다어절 hyphen), IPC prefix = id 마지막 segment 의 `_` 변환, i18n key root = prefix.
- **i18n**: 매니페스트 `*_i18n_key` 는 host 가 lookup. 플러그인이 직접 그리는 텍스트는 `tasty_plugin_sdk::i18n::Translator`(`TASTY_LOCALE` 주입). 키는 자기 prefix 안에만(`surface.kind.<own>` 만 예외).
- **권한 표기**: 실제 필요한 것만. 자기 namespace `ipc.invoke:<self>` 금지(self-loop 차단으로 무용).
- **모듈 분리**: main.rs 가 ~300줄 넘으면 `state.rs`/`handlers.rs`/`install.rs` 로 분리(claude/codex 가 reference). 단순 플러그인(image 61줄)은 단일 main.rs.
- **Cargo**: `tasty-plugin-protocol` 직접 의존 금지 — SDK 가 re-export. `[lints] workspace = true`.

## 9. 빌드 & 설치

```bash
cargo build --release -p my-plugin
tasty plugin install ./          # 매니페스트 권한 자동 grant + spawn
```

**워크스페이스 내 번들 플러그인 개발**: `BUILTINS`(`crates/tasty-host-plugin/src/builtin.rs`) 등록 플러그인은 워크스페이스 빌드 시 호스트가 부팅에 자동 sync(`ensure_dev_bundle` → `install_builtins_if_needed`). 단 루트 `cargo build` 는 본 바이너리만 빌드하므로 플러그인 변경은 `cargo build -p <crate>` 또는 `--workspace` 필요. 매니페스트 변경은 호스트 재시작으로 `tasty <plugin> --help` 에 반영.

디버깅: `tasty plugin logs <id> --follow` / `~/.tasty/plugins-logs/<id>.log` / `RUST_LOG=debug`.

## 10. 한계 (현재 SDK)

- async 미지원 — 모든 콜백 동기(무거운 I/O 는 플러그인 내부 thread).
- HotReload 미지원 — 코드 변경은 `disable && enable`.
- 권한 게이트는 **호스트 IPC 호출만** 막는다 — 플러그인 프로세스의 직접 `std::fs` 는 OS 샌드박스가 없는 한 강제 안 됨([plugin-permissions 한계](plugin-permissions.md#한계)).

## 관련

- [concepts/plugins](../concepts/plugins.md) — 분류 축·권한 개요
- [plugin-permissions](plugin-permissions.md) · [plugin-sensitive-data](plugin-sensitive-data.md)
- [plugins/](../plugins/index.md) — 번들 플러그인(= 예제) 카탈로그
- [features/plugin-system](../features/plugin-system/index.md) — 설치/관리 UI
</content>
