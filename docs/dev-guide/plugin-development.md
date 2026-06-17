# 플러그인 제작 가이드

외부 Tasty 플러그인을 작성·빌드·설치하는 법. 개념(배포/통합 축·권한)은 [concepts/plugins](../concepts/plugins.md) 먼저. 권한 모델 상세는 [plugin-permissions](plugin-permissions.md), 민감 데이터는 [plugin-sensitive-data](plugin-sensitive-data.md).

**번들 플러그인이 곧 reference 예제다** — 각 기여 타입을 만들 때 아래 표의 해당 플러그인 코드를 시작점으로 복사·수정하는 게 가장 빠르다.

## 기여 타입 → 예제 플러그인

| 만들고 싶은 것 | 보면 되는 번들 플러그인 | 난이도 |
|---------------|------------------------|--------|
| **host-rendered surface** (host 가 그림) | [image](../plugins/image/index.md)(최소) · [markdown](../plugins/markdown/index.md)(+파일 핸들러·settings) | ★ |
| **webview surface** | [html](../plugins/html/index.md) | ★ |
| **plugin-rendered surface** (자가 렌더 + UI DSL) | [explorer](../plugins/explorer/index.md) | ★★★ |
| **도구 메뉴 항목 + popup** | [git-viewer](../plugins/git-viewer/index.md)(view/logic 분리) · [clipboard-history](../plugins/clipboard-history/index.md) | ★★ |
| **CLI + IPC namespace** | [codex](../plugins/codex/index.md) · [claude](../plugins/claude/index.md) | ★★★ |
| **이벤트 구독 / 훅 / 외부 설치** | [claude](../plugins/claude/index.md)(`surface.closed`·Claude 훅·install) | ★★★ |
| **wasm 플러그인** | [clipboard-history](../plugins/clipboard-history/index.md)(`--features wasm`) | ★★ |

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

- **`rendering = "host"`** (image/markdown): 플러그인은 `[[surface_kinds]]` 로 kind 를 선언만 한다. host 화이트리스트(`("markdown","com.tasty.markdown")` 등)에 매칭되면 host 가 직접 렌더 — 플러그인은 surface 콜백을 구현하지 않는다. host-rendered 코드는 host 소유(`src/engine/surface_registry/builtins.rs`).
- **`rendering = "webview"`** (html): host 의 네이티브 WebView 오버레이로 그림. surface 의 URL 을 host 가 동기화.
- **(기본)** (explorer): 플러그인이 직접 렌더. host 트리엔 `RemoteSurface` marker, 플러그인이 `create_surface`/`handle_event` 에서 **UI tree DSL**(§4)로 트리를 반환. `snapshot_surface`/`restore_surface` 로 세션 복원.

### 파일 핸들러 (detector + handler)

확장자 → surface 매핑. `[[contributes.detector]]`(확장자 규칙) + `[[contributes.handler]]`(`action = open_surface{surface_kind}`). 권한: `file_handler.define`(신규 detector) / `file_handler.extend:<id>` / `file_handler.handle:<id>`. handler `id` 는 short name — install 단계가 `<plugin_id>/<id>` 로 자동 prefix. priority 동순위면 owner tiebreak `user > plugin > host`. 예: [image](../plugins/image/index.md)·[markdown](../plugins/markdown/index.md).

### 도구 메뉴 항목 + popup

- `[[contributes.tool]]`(`ui.tool_item`) — [도구 메뉴](../features/tools-menu/index.md)에 항목. `action.kind`: `event`(Event Bus 발화) / `open_surface`(탭 추가) / `open_popup`(`popup_id = <plugin_id>/<id>`). `order_hint` 오름차순(빌트인 0..99).
- `[[contributes.popup]]`(`ui.popup`) — trigger `event`(자동 open) 또는 `ipc`(명시 호출). SDK 콜백 `open_popup`/`handle_popup_event`/`on_popup_closed`. 동일 `popup_id` 라도 `instance_id` 가 다르면 별개 인스턴스. 예: [git-viewer](../plugins/git-viewer/index.md)·[clipboard-history](../plugins/clipboard-history/index.md).

### CLI + IPC namespace

`[[contributes.ipc_namespace]]`(prefix) + `[[contributes.cli]]`(`tasty <name> …`). 플러그인은 `handle_ipc_method` 로 `<prefix>.*` 메서드를 받는다. prefix 는 소문자+숫자+`_`, 호스트 예약어 금지(`system`/`surface`/`tab`/`pane`/`workspace`/`window`/… ). CLI 서브커맨드의 `ipc_method` 는 자기 prefix 와 매칭돼야 한다. 예: [codex](../plugins/codex/index.md)·[claude](../plugins/claude/index.md).

### 단축키 (commands)

`[[contributes.commands]]` — `id` · `default_keybinding` · `binding_mode`(`independent` 또는 `inherit:<host_action>`). 호스트가 키 매칭 시 `command.invoke` → SDK `handle_command`. 플러그인 키는 **focus surface 가 그 플러그인 소유일 때만** 호스트 키보다 우선. 예: [explorer](../plugins/explorer/index.md)(refresh/go_up).

### 설정 페이지

`[[contributes.settings_pages]]`(`ui.settings_page`) — [설정 창](../features/settings/index.md)에 sub-tab 동적 등록. `category`(appearance/general/keybindings/plugin/…). 1차 schema 는 `font_override` 항목만 지원(`storage_key` 가 `plugin_font_overrides` 의 key). 플러그인 비활성 시 sub-tab 자동 소멸. 예: [markdown](../plugins/markdown/index.md)·[explorer](../plugins/explorer/index.md).

### 이벤트 구독 / 윈도우 / 확장

- **event_subscribe** — `event_subscribe = ["surface.closed"]` + `on_start` 에서 `bus.subscribe(...)`. `on_event` 로 envelope 수신(`reason`: user/ipc/crash). 예: [claude](../plugins/claude/index.md)/[codex](../plugins/codex/index.md).
- **window** — `[[contributes.window]]`(`window.spawn`). 현재는 schema + 등록 stub 까지(실 spawn 은 별도 영역).
- **extension** — 다른 플러그인의 IPC/event 흐름을 가로채기. `[extends]` + `ext:<target>` 권한 + `handle_extension_hook`. mode: `transform`/`filter`/`observe`. target 당 활성 1개(나머지 `Conflict`). fail-open(timeout/에러 시 원래 값 사용).

## 4. UI tree DSL (plugin-rendered surface)

전체 위젯은 `crates/tasty-plugin-protocol/src/ui_tree.rs` `UiNode`. 주요 빌더(`tasty_plugin_sdk::ui::*`):

- 컨테이너: `vbox`/`hbox`(+`_spacing`) · `scroll_v` · `splitter(dir, ratio, a, b)`
- 표시: `label`(+`_styled`/`_color`) · `icon` · `text_preview`(+`_lang`) · `spacer`. 색 토큰 `text`/`subtext0`/`blue`/… 또는 `#aabbcc`
- 상호작용: `button`(+`_primary`) · `addressbar` · `tree_view`. `id` 가 이벤트에 echo
- 캔버스: `canvas`/`canvas_with_id`/`canvas_full` — `host.shared_buffer.create` 로 RGBA8 버퍼 확보 후 `commit(rect)`. 입력은 `UiEvent::CanvasPointer`

`handle_event` 가 `UiEvent`(Click/Key/TreeSelect/TreeExpand/Addressbar*/ContextMenu/Scroll/FocusChanged/Resize)를 받아 새 tree 반환. 호스트는 응답을 기다리지 않고 이전 tree 로 다음 프레임을 그린다(도착하면 교체). 예제: [explorer](../plugins/explorer/index.md)(`splitter` + `tree_view` + `addressbar`, lazy-load).

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

### 생명주기 (healthcheck / 자동 재시작·비활성화)

`manager.rs` 상수 기준:

- **부팅**: `~/.tasty/plugins/` 스캔 → enabled 전부 spawn.
- **헬스체크**: `PING_INTERVAL`(15s)마다 ping, `HEALTHCHECK_TIMEOUT`(60s) 무응답이면 강제 재시작.
- **자동 비활성화**: `RESTART_FAILURE_WINDOW`(10s) 내 `RESTART_FAILURE_LIMIT`(3)회 spawn 실패 → 정지(사용자가 `tasty plugin enable` 로 수동 재개까지).
- **종료**: shutdown 메서드 송신 후 timeout, 초과 시 kill.

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
