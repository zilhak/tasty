# Plugin 제작 가이드

외부 Tasty plugin을 작성·빌드·설치하는 방법.

이 문서는 본 저장소의 `crates/tasty-plugin-explorer/`를 reference로 인용한다 — SDK API의 구체적인 사용 예시가 필요하면 그 코드를 참조하라.

## 개요

Plugin은 별도 OS 프로세스로 실행되어 호스트와 TCP+JSON으로 통신한다. 호스트는 `~/.tasty/plugins/<id>/` 디렉터리에서 매니페스트를 발견하면 자동으로 spawn한다.

작성자가 다뤄야 할 것:

- **매니페스트** (`tasty-plugin.toml`) — id, surface kind, 권한 선언
- **`Plugin` trait 구현** — `create_surface` / `handle_event` / `restore_surface` / `snapshot_surface`
- **UI tree 빌드** — `ui::*` 헬퍼로 `UiNode`를 조립

호스트가 알아서 처리하는 것 (SDK가 감춤):

- TCP connect + 토큰 핸드셰이크
- NDJSON 직렬화/역직렬화
- 메시지 dispatch loop
- ping 응답
- shutdown 처리

## 1. 크레이트 골격

새 plugin 크레이트 디렉터리:

```
my-plugin/
  Cargo.toml
  tasty-plugin.toml
  src/
    main.rs
```

### Cargo.toml

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "my-plugin"
path = "src/main.rs"

[dependencies]
tasty-plugin-sdk = { path = "../tasty/crates/tasty-plugin-sdk" }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1"
```

> 현재 SDK는 crates.io에 publish되지 않았으므로 path 또는 git 의존성으로 가져온다. publish 후에는 `tasty-plugin-sdk = "0.1"`만으로 충분.

### tasty-plugin.toml

```toml
manifest_version = 1
id = "com.example.myplugin"           # 역도메인, 전역 유일
name = "My Plugin"
version = "0.1.0"
api_version = "1"                      # 호스트 protocol 메이저 버전
permissions = ["fs.read", "surface.write"]

[entry]
type = "process"
command = "my-plugin"                  # 매니페스트 디렉터리 기준 상대 또는 PATH

[[surface_kinds]]
kind = "myplugin_main"                 # 소문자 + '_' + 숫자만
display_name_i18n_key = "surface.kind.myplugin"
icon = "🔌"
```

매니페스트 검증 규칙은 `docs/agent-guide/plugins.md` 참조.

## 2. Plugin trait 구현

```rust
// src/main.rs
use tasty_plugin_sdk::{
    Plugin, SurfaceCreateCtx, SurfaceEventCtx, SurfaceResult, UiEvent, UiNode,
    ui::{label, vbox},
};

struct MyPlugin {
    counter: u32,
}

impl Plugin for MyPlugin {
    fn id(&self) -> &str {
        "com.example.myplugin"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        SurfaceResult {
            tree: Some(self.build_tree()),
            display_name: Some("My Plugin".into()),
        }
    }

    fn handle_event(&mut self, ctx: SurfaceEventCtx) -> SurfaceResult {
        if let UiEvent::Click { node_id } = ctx.event {
            if node_id == "btn_inc" {
                self.counter += 1;
            }
        }
        SurfaceResult {
            tree: Some(self.build_tree()),
            display_name: None,
        }
    }
}

impl MyPlugin {
    fn build_tree(&self) -> UiNode {
        vbox([
            label(format!("Count: {}", self.counter)),
            tasty_plugin_sdk::ui::button("btn_inc", "Increment"),
        ])
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(MyPlugin { counter: 0 })
}
```

`tasty_plugin_sdk::run`이 환경변수 로딩 → connect → auth → hello 송신 → 메시지 dispatch loop를 처리한다. plugin 작성자는 trait 메서드만 채우면 된다.

## 3. UI tree DSL

전체 위젯 종류는 `crates/tasty-plugin-protocol/src/ui_tree.rs`의 `UiNode` enum 참조.

### 컨테이너

| 빌더 | 설명 |
|------|------|
| `vbox(children)` / `vbox_spacing(s, children)` | 세로 박스 |
| `hbox(children)` / `hbox_spacing(s, children)` | 가로 박스 |
| `scroll_v(child)` | 세로 스크롤 영역 |
| `splitter(dir, ratio, first, second)` | 비율 기반 2분할 |

### 표시

| 빌더 | 설명 |
|------|------|
| `label(text)` / `label_styled(text, style)` / `label_color(text, color)` | 텍스트 |
| `icon(name)` | 아이콘/이모지 |
| `text_preview(content)` / `text_preview_lang(content, lang)` | 멀티라인 텍스트 |
| `spacer(size)` | 간격 |

색상 토큰: `text` / `subtext0` / `subtext1` / `blue` / `green` / `red` / `yellow` 또는 `#aabbcc`.

### 상호작용

| 빌더 | 설명 |
|------|------|
| `button(id, label)` / `button_primary(...)` | 클릭 → `UiEvent::Click { node_id: id }` |
| `addressbar(id, text)` | 입력 → `AddressbarChange/Submit` |
| `tree_view(id, nodes, selection_mode)` | 트리 → `TreeSelect/TreeExpand` |

`id`는 plugin이 정한 문자열로, 이벤트 라우팅에 그대로 echo된다.

## 4. 이벤트 처리

`UiEvent`는 호스트가 사용자 입력을 모아 `surface.event`로 한 번에 보낸다. plugin은 `handle_event`에서 받아 처리하고 새 tree를 반환하면 된다.

| 이벤트 | 발생 조건 |
|--------|-----------|
| `Click { node_id }` | Button 클릭 |
| `Key { key, mods }` | surface가 포커스된 상태에서 키 입력 |
| `TreeSelect { node_id, selected }` | Tree 선택 변경 |
| `TreeExpand { node_id, path, expanded }` | Tree 노드 펼침/접힘 |
| `AddressbarChange { node_id, text }` | 주소바 텍스트 편집 중 |
| `AddressbarSubmit { node_id, text }` | 주소바 Enter |
| `ContextMenu { node_id, path, x, y }` | 우클릭 |
| `Scroll { node_id, delta_y }` | 스크롤 |
| `FocusChanged { focused }` | 포커스 변화 |
| `Resize { width, height }` | surface 크기 변경 |

이벤트로 인한 부수 효과(예: 디렉터리 다시 읽기)는 동기적으로 처리해도 된다 — 호스트는 plugin 응답을 기다리지 않고 다음 프레임을 그린다(이전 tree 사용). 응답이 도착하면 새 tree로 교체된다.

## 4-1. 단축키 (commands & shortcuts)

Plugin이 surface를 추가하는 경우, 그 위에서 동작하는 단축키는 plugin이 매니페스트로 선언한다. 호스트가 이를 모아 설정 UI(설정 → 단축키 → Plugins 탭)와 키 매칭 로직에 통합한다.

### 매니페스트 선언

```toml
[[contributes.commands]]
id = "explorer.refresh"                # plugin 내 유일 식별자
title_i18n_key = "explorer.command.refresh"
default_keybinding = "F5"              # 사용자가 변경하지 않았을 때의 기본 키
binding_mode = "independent"           # 또는 "inherit:<host_action>"
```

### binding_mode 두 정책

| 값 | 의미 | 사용 예 |
|----|------|---------|
| `"independent"` (기본) | 호스트와 무관한 plugin 자체 키. 사용자가 따로 변경 가능 | "트리 새로 고침", "특정 plugin UI 열기" |
| `"inherit:<host_action>"` | 호스트의 의미론적 액션 키를 따라간다. 호스트 키가 바뀌면 plugin도 동행 | Explorer의 "선택 파일 복사" → `inherit:clipboard.copy` |

inherit는 plugin **작성자가 의미가 같다고 판단했을 때**만 선택한다. 사용자도
설정 UI에서 inherit를 풀어 독립 키로 떼어낼 수 있다 (반대로 plugin이
independent로 선언한 command를 사용자가 호스트에 inherit시킬 수는 없다 —
의미론적 매핑은 작성자만 안다).

### 호스트 → plugin 디스패치

호스트가 키 매칭에 성공하면 다음 IPC 메시지가 plugin에 도착한다.

```jsonc
// surface.event 와 별개의 메시지
{ "method": "command.invoke",
  "params": { "command_id": "explorer.refresh", "surface_id": 42 } }
```

SDK는 이를 `Plugin::handle_command(&mut self, ctx: CommandInvokeCtx)` 콜백으로
전달한다. 응답 형태는 `surface.event`와 동일한 `SurfaceResult { tree, display_name }`
이라 새 트리로 화면이 갱신된다. inherit 모드인 command도 plugin 입장에서는
동일 메시지로 도착 — 호스트 키가 매핑되어 있을 뿐 dispatch 경로는 같다.

```rust
use tasty_plugin_sdk::{CommandInvokeCtx, Plugin, SurfaceResult};

impl Plugin for MyPlugin {
    fn handle_command(&mut self, ctx: CommandInvokeCtx) -> SurfaceResult {
        match ctx.command_id.as_str() {
            "myplugin.refresh" => {
                self.reload(ctx.surface_id);
                SurfaceResult { tree: Some(self.build_tree()), display_name: None }
            }
            _ => SurfaceResult { tree: None, display_name: None },
        }
    }
}
```

### 충돌 우선순위

plugin 키와 호스트 키가 겹치면 **focused surface가 plugin 소유일 때만** plugin
키가 우선한다. 그 외 영역(터미널, 다른 plugin surface)에서는 호스트 키가
정상 동작한다.

## 4-2. IPC namespace 처리 (handle_ipc_method)

매니페스트에 `[[contributes.ipc_namespace]]`를 선언한 plugin은 해당 prefix로
시작하는 모든 IPC 메서드를 받는다. SDK가 자동으로 dispatch하므로 작성자는
`handle_ipc_method` 콜백만 구현하면 된다.

```rust
use tasty_plugin_sdk::{IpcMethodCtx, IpcMethodError, Plugin};
use serde_json::Value;

impl Plugin for MyPlugin {
    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        match ctx.method.as_str() {
            "codex.spawn" => self.handle_spawn(ctx.params),
            "codex.wait"  => self.handle_wait(ctx.params),
            other => Err(IpcMethodError::not_found(other)),
        }
    }
}
```

- `ctx.params`는 CLI에서 들어온 경우 매니페스트의 arg 스키마대로 직렬화된 JSON,
  다른 plugin이 호출한 경우 호출자가 보낸 임의 JSON. plugin은 자기 namespace
  내 모든 메서드의 시그니처를 책임진다.
- `ctx.caller_plugin_id`로 호출자가 plugin인지(`Some("com.example.other")`)
  사용자(CLI/IPC)인지(`None`) 구분 가능.
- `Ok(value)`를 반환하면 호스트가 JSON-RPC success response로 client에 전달,
  `Err(IpcMethodError)`이면 error response로 전달. 표준 코드 헬퍼:
  `IpcMethodError::not_found(method)` / `invalid_params(msg)` / `with_code(msg, code)`.

### 매니페스트 검증 / 예약 prefix

namespace prefix는 소문자 + 숫자 + `_` 만 허용되며, 다음 호스트 예약어는 사용
불가: `system`, `surface`, `tab`, `pane`, `workspace`, `claude`, `plugin`,
`hook`, `global_hook`, `message`, `tool`, `notification`, `window`, `debug`,
`ui`, `ime`, `split`, `tree`. CLI 서브커맨드의 `ipc_method`는 같은 plugin이
contribute한 namespace prefix와 매칭되어야 한다 (그래야 라우팅이 자기 자신으로
돌아온다).

다른 plugin의 namespace를 호출하려면 매니페스트 권한에
`"ipc.invoke:<prefix>"`를 선언하고 사용자가 grant해야 한다. 자기 namespace를
자기가 호출하는 무한 forward는 호스트가 `-32001`로 거부한다.

## 5. 영속화 (snapshot/restore)

세션 복원이 필요한 경우 두 메서드를 구현한다.

```rust
fn snapshot_surface(&mut self, ctx: SurfaceSnapshotCtx) -> serde_json::Value {
    // 영속화할 데이터 (root path, 선택 항목 등)
    serde_json::json!({ "root": "/some/path" })
}

fn restore_surface(&mut self, ctx: SurfaceRestoreCtx) -> SurfaceResult {
    let root = ctx.data.get("root").and_then(|v| v.as_str()).unwrap_or("/");
    // ... 복원 로직
}
```

호스트가 `layout.json`에 snapshot 결과를 함께 저장하고, 재시작 시 같은 데이터로 `restore_surface`를 호출한다.

## 6. 호스트 IPC 호출

Plugin이 호스트의 IPC API(예: `notification.create`, `surface.list`)를 호출하려면 매니페스트에 해당 권한을 선언하고 `PluginEvent::IpcCall`을 송신한다.

> **주의**: 현재 SDK 0.1은 `Plugin` trait에서 IPC 호출 헬퍼를 제공하지 않는다 — `connection::Connection::send_event`를 직접 사용해야 한다. 향후 `PluginContext` 추가 예정.

권한 토큰 → IPC 메서드 매핑은 `docs/agent-guide/plugins.md`의 권한 표 참조. 권한이 없는 메서드를 호출하면 `ipc.result`에 `permission_denied` 에러가 담겨 회신된다.

## 7. 환경변수

`tasty_plugin_sdk::env::PluginEnv::load()`가 일괄 로딩하지만 직접 읽을 수도 있다.

| 환경변수 | 용도 |
|----------|------|
| `TASTY_PLUGIN_ID` | 매니페스트 id (검증용) |
| `TASTY_HOST_IPC_PORT` | 호스트 listener port |
| `TASTY_PLUGIN_TOKEN` | 핸드셰이크 토큰 (1회용) |
| `TASTY_HOST_API_VERSION` | 호스트 protocol 메이저 버전 |
| `TASTY_PLUGIN_DIR` | 매니페스트 디렉터리 (정적 자산용) |
| `TASTY_PLUGIN_DATA_DIR` | 런타임 데이터 (DB, 캐시) — 업그레이드 시 보존 |
| `TASTY_PLUGIN_CONFIG_PATH` | 사용자 편집 설정 파일 경로 |
| `TASTY_PLUGIN_LOG_PATH` | stdout/stderr가 자동 redirect되는 로그 파일 |

호스트가 디렉터리들을 미리 만들어두므로 plugin은 `fs.write` 권한 없이도 자기 영역에 자유롭게 쓸 수 있다. (단, IPC 게이트는 plugin이 호스트 fs API를 호출하는 경우에만 적용된다 — plugin 자체 프로세스의 직접 fs 접근은 OS-level 샌드박스가 없는 한 강제되지 않는다.)

## 8. 빌드 & 설치

### 개발 중

```bash
# plugin 빌드
cargo build --release -p my-plugin

# ~/.tasty/plugins/com.example.myplugin/ 에 매니페스트 + 바이너리 배치
mkdir -p ~/.tasty/plugins/com.example.myplugin
cp tasty-plugin.toml ~/.tasty/plugins/com.example.myplugin/
cp target/release/my-plugin ~/.tasty/plugins/com.example.myplugin/

# 또는 CLI로 한 번에 설치
tasty plugin install ./
```

`tasty plugin install` 은 매니페스트의 모든 권한을 자동 grant한다 (사용자 의도적 명령으로 간주). 이후 호스트가 자동으로 spawn한다.

### Builtin plugin 자동 sync (workspace 내 plugin 개발)

`src/plugin/builtin.rs`의 `BUILTINS` 목록에 등록된 plugin(예: `tasty-plugin-explorer`, `tasty-plugin-codex`)은 매번 수동 복사할 필요가 없다. dev 빌드일 때 호스트가 부팅 직후 다음 두 단계를 자동 수행한다:

1. **번들 동기화** (`ensure_dev_bundle`): workspace의 `crates/<crate>/tasty-plugin.toml` + `target/<profile>/<bin>` + `crates/<crate>/lang/`을 `target/<profile>/builtin-plugins/<id>/`로 복사. mtime이 더 새것일 때만 덮어쓰므로 매 부팅 비용은 작다.
2. **사용자 디렉터리 sync** (`install_builtins_if_needed`): 위 번들 → `~/.tasty/plugins/<id>/`로 sync. 기존 설치본도 번들이 더 새것이면 자동 갱신.

호스트 CLI는 부팅 시 `~/.tasty/plugins/*/tasty-plugin.toml`을 다시 읽어 dynamic clap을 구성하므로, **매니페스트 변경분은 호스트만 재시작하면 즉시 `tasty <plugin> --help`에 반영**된다. plugin 바이너리 변경분은 호스트가 plugin process를 (재)spawn할 때 반영된다.

주의 — workspace 루트의 `cargo build` 또는 `cargo run`은 **현재 패키지(루트 `tasty`)만 빌드**하므로 plugin 코드를 고치면 다음 중 하나가 필요하다:

```bash
cargo build -p tasty-plugin-codex    # 특정 plugin만
cargo build --workspace              # 워크스페이스 전체
```

위 명령으로 plugin 바이너리를 다시 빌드해야 다음 `cargo run` 부팅 시 sync 메커니즘이 새 binary를 사용자 디렉터리로 복사한다.

### 디버깅

```bash
# 로그 실시간 확인
tasty plugin logs com.example.myplugin --follow

# plugin이 spawn되지 않거나 즉시 종료되면:
cat ~/.tasty/plugins-logs/com.example.myplugin.log
```

`RUST_LOG=debug` 환경변수로 plugin 시작 시 더 자세한 로그가 남는다 (위 예제처럼 `tracing-subscriber`를 init한 경우).

호스트의 plugin manager 로그는 호스트의 stderr 또는 `RUST_LOG=tasty::plugin=debug`로 활성화.

## 9. 검증 체크리스트

- [ ] `cargo build --release -p my-plugin` 성공
- [ ] `tasty-plugin.toml`이 매니페스트 검증 통과 (`tasty plugin install`이 거부하지 않음)
- [ ] `tasty plugin list`에 `running: true`로 표시됨
- [ ] `~/.tasty/plugins-logs/<id>.log`에 hello 송신 흔적
- [ ] surface 생성 후 호스트 UI에 plugin이 그린 tree가 보임
- [ ] surface event(클릭/키)가 `handle_event`에 들어옴

## 10. 참조: 동봉 explorer plugin

본 저장소의 `crates/tasty-plugin-explorer/`는 SDK만 사용한 동작하는 시연이다. 살펴볼 만한 부분:

- `ExplorerSurface`: surface별 상태(root, expanded set, selected, preview)를 plugin 측에서 관리
- `build_tree`: `splitter`로 좌우 분할(트리/미리보기), `addressbar` + `tree_view` + `scroll_v` 조합
- `handle_event`: `TreeExpand`로 lazy-load 흐름, `AddressbarSubmit`으로 root 변경
- `snapshot_surface`/`restore_surface`로 root path 영속화

새 plugin을 만들 때는 이 코드를 시작점으로 복사해 수정하는 것이 빠르다.

## 11. 한계 (현재 SDK 0.1)

- IPC 호출 헬퍼 미제공 — `Connection`을 직접 사용해야 함
- async 지원 안 됨 — 모든 plugin 메서드는 동기. 무거운 I/O는 plugin 내부에서 thread/runtime을 띄워야 함
- HotReload 미지원 — 코드 변경 시 `tasty plugin disable && tasty plugin enable`로 재시작 필요
- 위젯 종류는 v1 (vbox/hbox/scroll/splitter/label/icon/button/tree/addressbar/text_preview/spacer) — host 버전 업과 함께 추가
