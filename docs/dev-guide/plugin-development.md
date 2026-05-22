# Plugin 제작 가이드

외부 Tasty plugin을 작성·빌드·설치하는 방법.

이 문서는 본 저장소의 `crates/tasty-plugin-explorer/`를 reference로 인용한다 — SDK API의 구체적인 사용 예시가 필요하면 그 코드를 참조하라.

## 개요

Plugin은 별도 OS 프로세스로 실행되어 호스트와 TCP+JSON으로 통신한다. 호스트는 `~/.tasty/plugins/<id>/` 디렉터리에서 매니페스트를 발견하면 자동으로 spawn한다.

Plugin이 호스트에 contribute하는 카테고리(상세는 `docs/agent-guide/plugins.md`):

1. 새 Window 추가 — 현재 매니페스트 schema 없음(향후)
2. 새 Surface 추가 — `[[surface_kinds]]`
3. 새 Popup 추가 — `[[contributes.popup]]` + `permissions = ["ui.popup"]`. trigger는 `event`(host/plugin event 발화 시 자동 open) 또는 `ipc`(plugin 명시 호출). SDK trait의 `open_popup/handle_popup_event/on_popup_closed`로 구현한다.
4. 새 Tool 추가 (좌측 사이드바 도구 메뉴 항목) — `[[contributes.tool]]` + `permissions = ["ui.tool_item"]`. 클릭 시 dispatch는 `kind = "event"` (Event Bus 발화) / `"open_surface"` (탭 추가) / `"open_popup"` (`[[contributes.popup]]` 인스턴스 open, `popup_id`는 `<plugin_id>/<id>` 형식)
5. 이벤트별 동작 추가 — `[[contributes.commands]]`(키 입력) / `event_subscribe`(Event Bus 구독, 예: `"surface.closed"`) / `[[contributes.ipc_namespace]]`(IPC 호출) / `[[contributes.cli]]`(CLI 호출)

작성자가 다뤄야 할 것:

- **매니페스트** (`tasty-plugin.toml`) — `id`, `name`, `version`, `api_version`, `entry`는 필수. 위 카테고리 중 plugin이 실제로 contribute하는 항목만 선언한다. **contribute가 0개여도 valid**다 (예: 다른 plugin 모니터링만 하는 보조 plugin).
- **`Plugin` trait 구현** — contribute한 항목에 대응하는 콜백만 채우면 된다. surface를 추가하지 않으면 `create_surface` 등은 호출되지 않는다.
- **UI tree 빌드** (surface가 있을 때) — `ui::*` 헬퍼로 `UiNode`를 조립

> **다른 plugin을 확장하는 plugin**: 매니페스트 권한에 `"ipc.invoke:<대상 prefix>"`를 추가하고 사용자가 grant하면 `host.call("<대상 prefix>.<method>", ...)`로 다른 plugin의 메서드를 호출할 수 있다. 대상 plugin이 미설치/비활성이면 호스트가 `method not found`로 회신하므로 plugin이 분기 처리하면 된다. 설치 여부를 사전에 확인하려면 `plugin.list` IPC를 사용한다.

호스트가 알아서 처리하는 것 (SDK가 감춤):

- TCP connect + 토큰 핸드셰이크 (호스트 AuthAck 5초 대기 포함 — 거부 시 `PluginError::HandshakeRejected`, 무응답 시 `HandshakeTimeout`)
- NDJSON 직렬화/역직렬화
- 메시지 dispatch loop
- ping 응답
- shutdown 처리

### 핸드셰이크 진단

`tasty_plugin_sdk::run()`은 내부에서 [`Connection::connect_and_authenticate`]를 호출한다. 토큰 검증에 문제가 있으면 plugin은 silent hang 없이 다음 에러로 즉시 종료된다:

- `PluginError::HandshakeRejected { reason }` — 호스트가 명시적으로 거부 (예: 토큰 만료/불일치). `reason`은 호스트가 AuthAck에 담아 보낸 사유.
- `PluginError::HandshakeTimeout` — 5초 안에 AuthAck를 받지 못함. 호스트가 spawn 직후 죽었거나 네트워크가 막혔을 가능성.

진단을 직접 제어하고 싶다면 (`run()`을 쓰지 않고 SDK 저수준 API 사용 시) `Connection::connect`로 TCP만 연결한 뒤 `Connection::authenticate`를 분리 호출할 수 있다.

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

surface kind를 contribute하는 일반적인 plugin:

```toml
manifest_version = 1
id = "com.example.myplugin"           # 역도메인, 전역 유일
name = "My Plugin"
version = "0.1.0"                      # plugin 자체 버전 (필수). semver 권장
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

surface도 contribute도 없는 최소 plugin (예: 다른 surface 닫힘만 관찰하는 보조 plugin):

```toml
manifest_version = 1
id = "com.example.observer"
name = "Surface Observer"
version = "0.1.0"
api_version = "1"
event_subscribe = ["surface.closed"]   # Event Bus 구독 패턴 (옛 surface_observer 대체)

[entry]
type = "process"
command = "my-observer"
```

`event_subscribe`에 `"surface.closed"`를 적으면 모든 surface 종료 envelope을 받을 수 있다. 구독 자체는 `on_start` 콜백 안에서 `bus.subscribe("surface.closed")`로 등록한다.

매니페스트 검증 규칙(version 등 필수 필드, ipc_namespace 예약어, 옵셔널 필드 등)은
`docs/agent-guide/plugins.md` 참조.

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

### 캔버스 (픽셀 출력)

플러그인이 픽셀 단위 그림을 그리는 경로 — 이미지 디코딩, 차트, 픽셀 아트 등.

| 빌더 | 설명 |
|------|------|
| `canvas(buffer_id, w, h)` | RGBA8 sRGB + Linear filter, hit-test 비활성 |
| `canvas_with_id(id, buffer_id, w, h)` | 마우스 입력을 받는 canvas (`UiEvent::CanvasPointer` 라우팅) |
| `canvas_full(id, buffer_id, w, h, fmt, filter)` | 포맷·필터 직접 지정 (`PixelFormat::{Rgba8,Bgra8}` / `PixelFilter::{Linear,Nearest}`) |

1. `host.shared_buffer.create(size)`로 SharedBuffer 확보 — 크기는 `w × h × bpp + tasty_shm::footer::SIZE`.
2. `SharedBuffer::as_mut_slice()`로 user 영역에 RGBA 직접 write (footer는 SDK가 가린다).
3. `SharedBuffer::commit(rect)` — atomic generation을 1 증가하고 host에 dirty 영역 통지. `rect=None`은 전체 갱신.
4. 다음 frame에서 호스트가 `(plugin_id, buffer_id) → wgpu::Texture` 캐시에 부분 업로드 후 egui로 합성.

**입력**: 호스트가 캔버스 hit-test 결과를 `UiEvent::CanvasPointer { node_id, x, y, phase, button }`으로 전달. 좌표는 canvas-local 픽셀 단위. `phase`는 `Move/Down/Up/Drag/Leave`.

**제한** (Phase 1):
- 한 buffer 위에서 동시에 mutate하지 말 것 — 같은 frame 안에서는 한 `commit`만.
- `width × height × bpp + footer > buffer.len()`이면 호스트가 frame 단위로 거부하고 warn 로그.
- 텍스처 격리 GC는 plugin 종료 시 일괄 처리. 한 plugin이 buffer를 destroy해도 그 entry는 plugin 전체가 끝날 때까지 cache에 머문다 (Phase 1 한계).

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

## 4-1-A. 도구 메뉴 항목 (Tool Contribute)

좌측 사이드바 하단 "도구" 팝업에 항목을 꽂는다. 호스트 빌트인(클립보드 히스토리
등)과 plugin 항목이 같은 메뉴에 합쳐져 표시된다.

### 매니페스트

```toml
permissions = ["ui.tool_item"]

[[contributes.tool]]
id = "todo"
label_i18n_key = "com.example.todo.tool.label"
icon = "✅"
order_hint = 100

[contributes.tool.action]
kind = "event"
event_key = "com.example.todo.menu_clicked"
```

세 가지 `action.kind`별 동작:

| kind | 추가 필드 | 클릭 시 동작 |
|------|----------|------------|
| `event` | `event_key` | 호스트가 Event Bus로 `event_key` 발화 (payload `{"tool_id": "<plugin_id>/<tool_id>"}`). plugin은 `event_subscribe`에 같은 키를 선언해 받는다. |
| `open_surface` | `surface_kind` | 포커스된 pane에 해당 kind의 새 탭을 추가. `surface_kind`는 같은 매니페스트의 `[[surface_kinds]]`에 선언돼 있어야 한다. |
| `open_popup` | `popup_id` | `<plugin_id>/<id>` 형식. 매니페스트의 `[[contributes.popup]]`에 같은 id가 선언돼 있어야 한다. 클릭 시 호스트가 popup 인스턴스를 만들어 plugin에 `popup.open` IPC를 보낸다. |

### plugin 측 처리

`kind = "event"`로 선언한 경우 plugin은 SDK의 이벤트 콜백에서 받는다:

```rust
event_subscribe = ["com.example.todo.menu_clicked"]
```

`Plugin::handle_event` 콜백이 `event_key == "com.example.todo.menu_clicked"`로
호출된다. payload의 `tool_id`는 plugin이 여러 도구 항목을 contribute했을 때
어떤 항목이 트리거됐는지 식별하는 용도.

`kind = "open_surface"`는 plugin이 별도 코드를 작성할 필요가 없다 — 호스트가
`AppState::add_kind_tab`으로 탭을 생성하고, plugin의 `create_surface` 콜백이
일반 surface 생성 흐름과 동일하게 호출된다.

### 표시 조건과 정렬

- `permissions = ["ui.tool_item"]`이 매니페스트에 있고 사용자가 grant했을 때만 노출.
- plugin을 disable하거나 권한을 revoke하면 즉시 메뉴에서 사라진다.
- 전체 메뉴는 `order_hint` 오름차순 (기본 100), 동률은 키 순. 호스트 빌트인 `0..99`.
- 항목 키는 호스트가 합성: `<plugin_id>/<tool_id>` 형식.

### 디버그/테스트

debug 빌드에서 `tasty debug tool list` / `tasty debug tool invoke --key <key>`로
실제 메뉴 렌더링을 거치지 않고 IPC로 검증할 수 있다 (`docs/dev-guide/debug-ipc.md`).

## 4-1-B. Popup 항목 (Popup Contribute)

Plugin이 자기 popup을 contribute할 수 있다. host는 popup마다 `instance_id`(u64)를
발급해 동일 `popup_id`의 여러 인스턴스를 동시에 띄울 수 있게 추적한다.

### 매니페스트

```toml
permissions = ["ui.popup"]

[[contributes.popup]]
id = "search"
size_hint = { width = 480, height = 320 }
anchor = "screen-center"            # "active-surface-center" | "cursor" 도 가능
dismiss_on_outside_click = true

[contributes.popup.trigger]
kind = "event"
event_key = "com.example.search.opened"

[[contributes.popup]]
id = "result"
[contributes.popup.trigger]
kind = "ipc"
```

`trigger` 종류별 동작:

| kind | 추가 필드 | 동작 |
|------|----------|------|
| `event` | `event_key` | 매칭 envelope이 host/plugin 어디서든 발화되면 호스트가 자동으로 popup을 open. envelope payload가 `popup.open` IPC의 `context`로 전달된다. |
| `ipc` | — | 외부(다른 plugin/도구 메뉴/debug CLI)가 명시적으로 open할 때만. |

`[[contributes.tool]] action = { kind = "open_popup", popup_id = "com.example.foo/search" }`
로 도구 메뉴 항목과 연결할 수 있다 (cross-reference 검증).

### plugin 측 콜백

SDK가 popup 라이프사이클을 trait 콜백으로 노출한다:

```rust
use tasty_plugin_sdk::{
    Plugin, PopupOpenCtx, PopupEventCtx, PopupClosedCtx,
    PopupOpenResult, PopupEventResult,
};

impl Plugin for SearchPlugin {
    fn open_popup(&mut self, ctx: PopupOpenCtx) -> PopupOpenResult {
        // ctx.instance_id로 인스턴스를 식별. ctx.context는 매니페스트에 선언된
        // event_key의 payload (또는 ipc trigger의 명시 인자).
        PopupOpenResult {
            tree: Some(self.build_search_tree(&ctx.context)),
        }
    }

    fn handle_popup_event(&mut self, ctx: PopupEventCtx) -> PopupEventResult {
        // ctx.event는 UiEvent (Click, AddressbarChange, TreeSelect 등).
        let close_now = self.process_event(ctx.instance_id, ctx.event);
        PopupEventResult {
            tree: Some(self.rebuild_tree(ctx.instance_id)),
            close: close_now,           // true면 호스트가 자동 close
        }
    }

    fn on_popup_closed(&mut self, ctx: PopupClosedCtx) {
        // ctx.reason: OutsideClick | Escape | PluginRequest | HostShutdown
        self.cleanup_state(ctx.instance_id);
    }
}
```

핵심 invariant:
- 동일 `popup_id`라도 `instance_id`가 다르면 별개 인스턴스다. plugin이 인스턴스
  상태를 보관할 때는 `instance_id`를 키로 쓴다.
- `popup.event` 응답의 `close=true`는 plugin이 능동 close를 요청하는 신호.
  호스트는 인스턴스를 즉시 제거하고 `popup.closed`를 `PluginRequest` 사유로 보낸다.
- 비동기로 close하고 싶으면 host IPC `popup.close`를 호출한다 (`ui.popup` 권한 필요):
  `host.call("popup.close", json!({"instance_id": id}))`. 자기 plugin이 소유한
  인스턴스만 닫을 수 있다 (다른 plugin의 instance_id를 넘기면 에러).
- `popup.open` 응답의 `tree=None`은 "아직 그릴 트리 없음" 의미 (호스트는 인스턴스를
  계속 추적, 다음 event 응답에서 tree를 세팅 가능).

### 디버그/테스트

debug 빌드에서 `tasty debug popup list` / `tasty debug popup open` /
`tasty debug popup close --instance-id <N>`로 IPC 경로를 직접 호출 가능
(`docs/dev-guide/debug-ipc.md`).

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

## 4-3. Surface 닫힘 알림 (Event Bus)

다른 plugin이나 사용자가 surface를 닫았을 때 알림을 받고 싶다면 매니페스트에 `event_subscribe = ["surface.closed"]`를 선언한 뒤 `on_start`에서 `bus.subscribe("surface.closed")`를 호출한다. (자기 자신이 만든 surface의 정리는 `destroy_surface`가 별도로 호출되므로 추가 구독이 필요 없다.)

```toml
event_subscribe = ["surface.closed"]
```

```rust
use tasty_plugin_sdk::{BusHandle, EventDispatchCtx, HostHandle, Plugin};

impl Plugin for MyPlugin {
    fn on_start(&mut self, _host: HostHandle, bus: BusHandle) {
        let _ = bus.subscribe("surface.closed");
    }

    fn on_event(&mut self, ctx: EventDispatchCtx) {
        if ctx.envelope.key != "surface.closed" { return; }
        let surface_id = ctx.envelope.payload.get("surface_id").and_then(|v| v.as_u64());
        let reason = ctx.envelope.payload.get("reason").and_then(|v| v.as_str());
        // reason: "user" | "ipc" | "crash"
        // ...
    }
}
```

### close 사이트 분류

| close 경로 | `reason` |
|----------|---------|
| PTY ProcessExited (자연 종료) | `"user"` |
| 단축키 `close_surface` | `"user"` |
| 탭 우클릭 → Close | `"user"` |
| IPC `surface.close` / `surface.close_self` | `"ipc"` |
| plugin 프로세스 크래시 cascade | `"crash"` |

cascade 닫힘(탭/팬/워크스페이스 전체 삭제로 따라가는 surface)은 현재 broadcast 대상이 아니다. 명시적으로 close된 surface_id 만 envelope이 발사된다.

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

Plugin이 호스트의 IPC API(예: `notification.create`, `surface.list`)를 호출하려면 매니페스트에 해당 권한을 선언하고 `HostHandle::call`을 사용한다. `IpcMethodCtx`의 `host` 필드로 제공된다.

```rust
use tasty_plugin_sdk::{HostHandle, IpcMethodCtx, IpcMethodError};

fn handle_my_method(host: &HostHandle, params: Value) -> Result<Value, IpcMethodError> {
    // ? 한 번으로 PluginError → IpcMethodError 자동 변환.
    let surfaces = host.call("surface.list", serde_json::json!({}))?;
    Ok(serde_json::json!({ "count": surfaces.as_array().map(|a| a.len()).unwrap_or(0) }))
}
```

권한 토큰 → IPC 메서드 매핑은 `docs/agent-guide/plugins.md`의 권한 표 참조. 권한이 없는 메서드를 호출하면 `host.call(...)`이 `PluginError::HostCall { message: "permission_denied: ..." }`로 회신된다.

### 에러 분기

대부분의 경우 `?`만으로 충분하지만, host 호출 실패와 호스트 응답 에러를 구분해서 처리해야 할 때는 `PluginError` variant로 매칭한다.

```rust
use tasty_plugin_sdk::{HostHandle, PluginError};

fn fetch_surface_meta(host: &HostHandle, id: u32) -> Result<Value, IpcMethodError> {
    match host.call("surface.meta.get", serde_json::json!({"surface_id": id})) {
        Ok(v) => Ok(v),
        Err(PluginError::HostCallTimeout { method, timeout }) => {
            tracing::warn!("'{method}' timed out after {timeout:?}; returning empty meta");
            Ok(Value::Null)
        }
        Err(PluginError::HostCall { method, message }) if message.contains("not_found") => {
            tracing::warn!("'{method}' returned not_found; surface gone?");
            Ok(Value::Null)
        }
        Err(other) => Err(other.into()), // 그 외는 IPC 응답으로 전파.
    }
}
```

variant 요약:

| variant | 의미 |
|---------|------|
| `EnvMissing(name)` | 필수 환경변수 누락 — bootstrap 단계에서만 발생 |
| `EnvParse { var, message }` | 환경변수 파싱 실패 (예: 포트 번호 형식 오류) |
| `Connect { port, source }` | TCP 호스트 connect 실패 |
| `HostClosed` | 호스트가 연결을 닫음 |
| `HandshakeRejected { reason }` | 호스트가 AuthAck `ok=false`로 거부 (토큰 불일치 등) |
| `HandshakeTimeout` | 호스트 AuthAck가 5초 안에 도착 안함 |
| `HostCall { method, message }` | 호스트가 에러 응답을 보냄 (permission_denied 등) |
| `HostCallTimeout { method, timeout }` | 호스트 응답이 timeout 안에 도착 안함 |
| `Io(io::Error)` | 일반 IO 에러 |
| `Json(serde_json::Error)` | 인코딩/디코딩 실패 |
| `LockPoisoned(name)` | 내부 mutex poison (다른 thread의 패닉 후) |

## 6-1. Extension (다른 plugin 확장)

특정 plugin(target)의 이벤트 발화/IPC 호출을 가로채는 **extension plugin**을 작성할 수 있다. extension은 매니페스트에 `[extends]` 블록을 선언하고 SDK의 `Plugin::handle_extension_hook`을 구현한다.

대상 plugin 1개당 활성 extension은 최대 1개다 (단일 A+ 제약). 동일 target을 가리키는 extension이 둘 이상이면 lexicographic 우선순위로 winner 1개만 `Active`가 되고 나머지는 `Conflict` 상태로 비활성화된다.

### 매니페스트 `[extends]`

```toml
id = "com.example.clipboard-redactor"
name = "Clipboard Redactor"
version = "0.1.0"
api_version = "1"
entry = { command = "clipboard-redactor" }

permissions = ["ext:com.tasty.clipboard"]

[extends]
plugin_id = "com.tasty.clipboard"
version_req = ">=0.1, <0.2"
api_version = "1"

[[extends.pre_ipc]]
method = "clipboard.add"
mode = "transform"
timeout_ms = 200
modifies = ["text"]

[[extends.post_event]]
event = "clipboard.changed"
mode = "observe"
timeout_ms = 100
```

필드 의미:

- `plugin_id`: 확장 대상 plugin id (정확 일치).
- `version_req`: 대상 버전 범위 (semver). 대상이 이 범위를 벗어나면 extension은 `Pending` 상태로 대기한다.
- `api_version`: extension 자체가 따르는 호스트 protocol 버전. 호스트의 `HOST_API_VERSION`과 같아야 한다.
- `pre_event` / `post_event`: 대상이 publisher인 envelope의 fan-out **이전**/**이후**에 fire.
- `pre_ipc` / `post_ipc`: 대상 namespace의 IPC 호출의 invoke **이전**(`params` 가공)/**응답 이후**(`result` 가공)에 fire.
- 각 hook 항목은 정확한 키(이벤트 키 또는 IPC method)와 `mode`, `timeout_ms`를 지정한다. 와일드카드 불가. `timeout_ms` 상한은 `HOOK_TIMEOUT_MS_MAX = 1000`.

`mode`:

- `transform` — payload를 새 값으로 교체할 수 있다. 가장 강력.
- `filter` — `pass: bool`만 반환. 차단 가능하지만 payload 변경 불가.
- `observe` — 응답은 호스트가 무시. 단순 관찰/로깅. timeout/실패도 후속 흐름에 영향 없음.

### 권한 토큰 `ext:<target>`

`[extends]` 블록을 선언한 plugin은 매니페스트 `permissions[]`에 `ext:<plugin_id>` 토큰을 반드시 포함해야 한다 (검증 실패 시 매니페스트 거부). 사용자가 `tasty plugin install`로 grant하면 extension이 등록된다.

### SDK 훅 구현

`Plugin::handle_extension_hook(&mut self, ctx: ExtensionHookCtx) -> ExtensionHookOutcome`을 구현한다.

```rust
use tasty_plugin_sdk::{ExtensionHookCtx, ExtensionHookOutcome, Plugin};
use tasty_plugin_protocol::{ExtensionHookKind, ExtensionHookMode, ExtensionHookPhase};

impl Plugin for ClipboardRedactor {
    fn handle_extension_hook(&mut self, ctx: ExtensionHookCtx) -> ExtensionHookOutcome {
        match (ctx.kind, ctx.phase, ctx.mode, ctx.target.as_str()) {
            (ExtensionHookKind::Ipc, ExtensionHookPhase::Pre,
             ExtensionHookMode::Transform, "clipboard.add") => {
                let mut params = ctx.payload;
                if let Some(text) = params.get_mut("text").and_then(|v| v.as_str()) {
                    let redacted = redact_secrets(text);
                    params["text"] = serde_json::Value::String(redacted);
                }
                ExtensionHookOutcome::transformed(params)
            }
            (ExtensionHookKind::Event, ExtensionHookPhase::Post,
             ExtensionHookMode::Observe, "clipboard.changed") => {
                tracing::info!(target = %ctx.target, "observed clipboard.changed");
                ExtensionHookOutcome::pass()
            }
            _ => ExtensionHookOutcome::pass(),
        }
    }
}
```

헬퍼:

- `ExtensionHookOutcome::pass()` — observe / filter pass / transform no-op 공통.
- `ExtensionHookOutcome::block()` — filter mode에서 흐름 차단.
- `ExtensionHookOutcome::transformed(new_payload)` — transform mode에서 payload 교체.

`ctx.payload`는 phase에 따라 다르다:

| kind | phase | payload |
|---|---|---|
| event | pre | envelope.payload (fan-out 직전) |
| event | post | envelope.payload (fan-out 직후 — observe-only 효과) |
| ipc | pre | 호출 `params` (target invoke 직전) |
| ipc | post | 호출 `result` (target 응답 후, caller에게 반환 직전) |

### 상태 머신

호스트가 각 extension에 대해 추적하는 상태:

| 상태 | 설명 |
|---|---|
| `Active` | hook이 정상 fire 중. |
| `Pending(reason)` | 활성화 대기. reason 예: `target_missing`(대상 plugin 미설치), `target_version_mismatch`, `api_version_mismatch`, `host_starting`. 대상이 install/enable되면 자동 전환. |
| `Disabled` | 사용자가 disable 또는 권한 미부여. |
| `Conflict` | 동일 target에 다른 winner가 이미 있어 비활성. winner가 떠나면 lexicographic 다음 후보가 `Active`로 승격. |

CLI: `tasty plugin extension list`로 전체 extension의 상태를 조회.

### Fail-open 정책

hook 호출 결과가 timeout/에러/체인 통신 실패면 호스트는 **원래 값을 그대로 사용**하고 흐름을 계속한다 (요청 전체를 막지 않음). 같은 `(extension_id, target_key)` 쌍에 대해 연속 3회 실패가 누적되면 60초 backoff 동안 그 hook은 skip된다. 성공 시 카운터는 reset.

### Self-loop 방지

extension plugin이 자기가 hook 거는 target의 IPC를 호출하더라도 caller_plugin_id가 자신과 같으면 hook은 skip된다. 무한 재귀를 막기 위함.

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
| `TASTY_PLUGIN_HANDLE_ENDPOINT` | (선택) 보조 핸들 채널 endpoint — Unix: socket path, Windows: pipe 이름. 없거나 빈 문자열이면 SDK가 보조 채널을 skip한다. |

호스트가 디렉터리들을 미리 만들어두므로 plugin은 `fs.write` 권한 없이도 자기 영역에 자유롭게 쓸 수 있다. (단, IPC 게이트는 plugin이 호스트 fs API를 호출하는 경우에만 적용된다 — plugin 자체 프로세스의 직접 fs 접근은 OS-level 샌드박스가 없는 한 강제되지 않는다.)

### 보조 핸들 채널 (TASTY_PLUGIN_HANDLE_ENDPOINT)

메인 채널은 NDJSON TCP라 fd/HANDLE 같은 OS 리소스를 운반할 수 없다. shared buffer
등 핸들 전달이 필요한 기능을 위해 호스트는 **보조 채널**을 별도 endpoint로 연다:

- Unix (Linux/macOS): `std::env::temp_dir()` 안의 `tasty-handle-<pid>-<nonce>.sock` `AF_UNIX` 소켓
- Windows: `\\.\pipe\tasty-handle-<random>` Named Pipe (구현 진행 중)

SDK `tasty_plugin_sdk::run`이 메인 채널 인증 직후 자동으로 connect를 시도한다.
endpoint env가 없거나 connect/auth가 실패해도 fatal이 아니라 `warn` 로그만 남기고
본 루프를 계속 진행한다 — 보조 채널을 안 쓰는 plugin은 그대로 동작한다.

인증 메시지는 메인 채널과 동일한 `AuthMessage`/`AuthAckEnvelope`을 재사용 (구분은
endpoint 자체로 함). plugin 작성자가 직접 보조 채널을 다룰 일은 없으며, shared
buffer 같은 상위 API를 통해 간접적으로 사용된다.

#### 보조 채널 위에서 운반되는 메시지 (`HandleChannelMessage`)

| kind | 방향 | 내용 |
|------|------|------|
| `ping` / `pong` | 양방향 | 살아있는지 확인. 받은 쪽은 동일 `seq`로 응답. |
| `handle_attach` | host → plugin | NDJSON 본문 + SCM_RIGHTS로 fd 동행. `request_id`는 메인 채널 `host.shared_buffer.create` call_id와 1:1. plugin은 `tasty_shm::receive`로 매핑. |
| `dirty` | plugin → host | 특정 buffer의 dirty 영역(`Option<Rect>`, `None`=전체). fire-and-forget. host 리더가 union(coalesce). |

호스트는 plugin마다 보조 채널 reader 스레드 1개를 띄워 `dirty`/`ping`을 수신하고,
같은 stream의 write 쪽으로 `pong`을 회신한다. `dirty` 누적 결과는
`PluginManager::take_plugin_dirty_rects(plugin_id)`로 drain한다.

#### `host.shared_buffer.create` 와이어 흐름

1. plugin SDK가 `HostHandle::create_shared_buffer(size)` 호출.
2. SDK가 메인 채널로 `IpcCall { method: "host.shared_buffer.create", params: {size} }` 송신.
3. host가 `tasty_shm::create` → `prepare_send`로 fd payload를 만들고, **먼저 보조
   채널로 `HandleAttach { request_id, id, size }` + fd 동행 전송**, 그 후
   메인 채널로 `SharedBufferCreateResult { id, size }`를 회신.
4. plugin SDK는 양쪽이 같은 `request_id == call_id`로 매칭될 때까지 wait. 두
   정보가 모인 시점에 `SharedBuffer` 핸들을 호출자에게 반환.
5. plugin은 `as_mut_slice`로 직접 쓰고, 필요 시 `SharedBuffer::mark_dirty(rect)`로
   host에 변경 영역 통지.

manifest 한도(`max_shared_buffer_bytes`) 도입 전까지는 host가 1 GiB 임시 상한으로
거절한다. 0 크기 요청은 무조건 reject.

## 7-1. 데이터 저장 위치

Plugin 이 영속 데이터를 두는 곳은 성격에 따라 갈린다. 잘못 고르면 빌드 폴더에 두거나 memory.db 를 비대하게 만든다.

| 데이터 성격 | 위치 | 비고 |
|---|---|---|
| 정적 자산 (아이콘, 템플릿, README, lang 파일) | `TASTY_PLUGIN_DIR` 아래 매니페스트와 함께 | 사용자가 plugin 을 업그레이드하면 통째 교체됨. plugin 빌드 산출물의 일부로 다룬다. |
| 사용자 편집 설정 | `TASTY_PLUGIN_CONFIG_PATH` | 사용자가 직접 편집할 수 있는 single TOML/JSON. 업그레이드 시 보존. |
| 자체 DB, 캐시, 큰 binary, 로그-스타일 누적 데이터 | `TASTY_PLUGIN_DATA_DIR` | 호스트가 미리 만들어둠. plugin 이 마음대로 파일 트리 구성. plugin uninstall 시 plugin 책임으로 삭제. |
| **작업 메타데이터, 토큰, 진행 상태, 세션 메모** (KB ~ 1 MiB) | **`memory.*` / `memory.secret.*`** | 호스트가 SQLite + 암호화로 관리. 아래 7-2 참조. |

### Memory 사용 원칙

Tasty memory 는 **합리적인 한도 안에서는 범용적인 키-값 저장소** 다 — 토큰 같은 수 KB 부터 캐시된 응답이나 누적 작업 상태 같은 수백 KB ~ 1 MiB 까지. 다만 **"어떤 크기든 받아주는 만능 저장소" 는 아니다**. cap 을 넘는 데이터는 memory 가 책임지지 않는다.

- 단일 entry 의 value 가 cap (default 1 MiB) 을 넘으면 `ValueTooLarge` 로 거부된다.
- Plugin 별 secret quota (default 10 MiB) 또는 regular global quota (default 1 GiB) 를 넘으면 `QuotaExceeded`.

cap 에 부딪쳤을 때 first response 는 "config 를 올리자" 가 아니라 **"이 데이터가 정말 memory 에 들어가야 하는가, 파일로 분리할 수 있는가"** 를 먼저 묻는 것이다.

```rust
// 안티패턴: 큰 blob 을 memory 에 직접
host.call("memory.put", json!({
    "scope": "workspace:1",
    "key": "screenshot.png.b64",
    "value": <2MB base64>,    // ❌ ValueTooLarge
}))?;

// 권장: filesystem + reference
let path = data_dir.join("screenshot-2024.png");
std::fs::write(&path, &bytes)?;
host.call("memory.put", json!({
    "scope": "workspace:1",
    "key": "screenshot.path",
    "value": path.display().to_string(),
}))?;
```

상세 모델(regular/secret 두 계층, owner 자동 도출, quota 정책)은 [design/memory-system.md](../design/memory-system.md) 참조.

### Regular vs Secret 선택 기준

| 데이터 성격 | 영역 |
|---|---|
| 다른 plugin 이 알아도 무방하거나 협업해야 하는 작업 상태 (예: 활성 세션 id, 진행률) | regular |
| Plugin 내부 캐시·세션 메모 / UI 옵션 (다른 plugin 이 굳이 볼 필요 없음) | secret (격리가 default 가 안전) |
| 토큰, API 키, OAuth refresh token, 사용자 자격 증명 등 **진짜 민감 데이터** | **memory 아닌 OS keyring** (`docs/dev-guide/plugin-sensitive-data.md`) |

**"몰라도 되는 데이터" 는 secret 에 두는 것이 default 라고 생각해도 된다.** Regular 는 "다른 plugin 과의 공유 가치가 있을 때" 의 선택. **secret 영역의 보호는 "다른 plugin 의 IPC 접근 차단" 한 가지** — 디스크에는 평문 BLOB 으로 저장되므로, 디스크 노출 / 백업 sync / 도난을 견뎌야 하는 데이터는 secret 에 두지 말고 위 가이드 참고.

## 7-2. 매니페스트·코드 규약

기존 번들 plugin (`com.tasty.claude` / `codex` / `image` / `explorer` / `clipboard-history`) 의 공통 규약. 새 plugin 도 같은 규약을 따른다.

### 이름·식별자

| 항목 | 규칙 | 예 |
|---|---|---|
| Crate 이름 | `tasty-plugin-<name>` | `tasty-plugin-clipboard-history` |
| Binary 이름 | crate 이름과 동일 | `tasty-plugin-clipboard-history` |
| Plugin id | reverse-DNS, 다어절은 hyphen | `com.tasty.clipboard-history` |
| IPC namespace prefix | id 마지막 segment 의 underscore 변환 | `clipboard_history` |
| i18n key root prefix | namespace prefix 와 동일 (`<prefix>.*`) | `clipboard_history.popup.title` |

호스트가 IPC namespace 에서 hyphen 을 거부하므로 다어절 plugin id 는 IPC prefix 만 underscore 로 변환된다 — id 자체는 hyphen 유지.

### Manifest 필드 순서

다음 순서로 두면 grep 일관성이 유지된다:

```toml
manifest_version = 1
id = "com.example.foo"
name = "Foo"
version = "0.1.0"
description = "..."
api_version = "1"
permissions = [...]
lang_dir = "lang"
event_subscribe = [...]        # 있는 경우만

[entry]
type = "process"
command = "tasty-plugin-foo"

# 그 다음 contributes.*
```

`name` / `description` 류 자유 텍스트 필드는 **작성자가 원하는 언어로 자유롭게 적는다** — 강제 규칙 없음. 사용자에게 다국어로 보여야 하는 영역은 별도 i18n key 를 통해 노출하고, 매니페스트의 자유 텍스트 필드는 fallback / 기록용으로 작성자 편의대로 둔다.

### i18n key namespace 규칙

Plugin 의 i18n key 는 반드시 **자기 prefix (`<plugin_prefix>.*`) 안** 에 둔다.

```toml
# 좋음 — 자기 namespace
label_i18n_key = "clipboard_history.tool.label"
description_i18n_key = "clipboard_history.cli.desc"

# 나쁨 — host namespace 침범
label_i18n_key = "tools_menu.clipboard_history"
```

호스트가 합쳐 표시하는 surface kind 이름 (`surface.kind.<own_kind>`) 은 예외다 — host UI 가 같은 key 공간을 사용하므로 이 채널만 plugin 이 `surface.kind.<kind>` 키를 자기 lang 파일에서 정의한다. 그 외의 host namespace (`tools_menu.*`, `settings.*`, `popup.*` 등) 는 침범하지 않는다.

### Plugin process 내부에서 i18n 사용

매니페스트의 `*_i18n_key` 필드는 호스트가 `t()` 로 lookup 해 그리지만, **plugin process 가 직접 그리는 UI 텍스트** (예: `UiNode::Label { text }`, `display_name`, button label) 는 plugin 자신이 번역해야 한다. plugin process 는 host 의 i18n 전역 카탈로그에 접근할 수 없으므로 SDK 가 같은 lang 파일을 다시 읽어주는 [`tasty_plugin_sdk::i18n::Translator`] 를 제공한다.

```rust
use tasty_plugin_sdk::{PluginEnv, Translator};

fn main() -> anyhow::Result<()> {
    // ... tracing_subscriber init ...
    let env = PluginEnv::load()?;
    let tr = Translator::from_plugin_env(&env);
    tasty_plugin_sdk::run(MyPlugin::new(tr))
}
```

- `Translator::from_plugin_env` 는 `env.plugin_dir/lang/en.toml` 을 base 로 읽고, `env.locale != "en"` 이면 `<locale>.toml` 을 overlay 한다 (host 카탈로그와 동일 규칙). 누락 시 키 자체 반환.
- 호스트가 spawn 시 `TASTY_LOCALE` 환경변수를 주입한다. 따로 사용자에게 묻거나 settings 파일을 직접 읽을 필요 없음.
- 빌더/렌더 함수에 `&Translator` 를 명시 인자로 전달한다 — 전역 OnceLock 패턴은 plugin process 의 hot-reload / 테스트 격리에 방해된다.
- `tr.t(key)` 는 `&str` 을, `tr.t_fmt(key, arg)` 는 `{}` 첫 occurrence 치환을, `tr.t_replace(key, token, value)` 는 임의 토큰 치환을 한다.

자유 텍스트(고유명사, 브랜드명, 시스템 식별자, 패턴 매칭용 영문 문자열 등)는 번역 면제 — 무리하게 lang 키로 빼지 않는다.

### Cargo 의존성

```toml
[dependencies]
tasty-plugin-sdk = { path = "../tasty-plugin-sdk" }
serde = { version = "1", features = ["derive"] }      # 필요할 때만
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1"
```

- **`tasty-plugin-protocol` 을 직접 의존하지 않는다.** SDK 가 `UiNode`/`UiEvent`/`Rect` 등 wire 타입을 re-export 한다. SDK 에서 못 가져오는 타입이 있으면 SDK 에 re-export 를 추가하는 PR 을 먼저 보낸다.
- `[lints] workspace = true` 도 모든 plugin 이 동일하게 둔다 (workspace clippy 룰 적용).

### 권한 표기

- 매니페스트에는 **실제로 필요한 권한만** 둔다. unused 권한은 사용자 grant prompt 의 정보 비대만 만든다.
- **자기 namespace 의 `ipc.invoke:<self>` 토큰 금지.** 호스트가 self-loop 를 차단하므로 무용하다. 자기 IPC 메서드는 `handle_ipc_method` 안에서 직접 호출하면 된다.
- Memory 권한은 영역별로 분리: `memory.read` (regular 읽기), `memory.write` (regular 쓰기), `memory.secret` (자기 secret 영역 R/W).

### src 모듈 분리

main.rs 가 ~300줄을 넘기면 별 모듈로 추출한다. 권장 분할:

| 파일 | 책임 |
|---|---|
| `src/main.rs` | `Plugin` trait impl + bootstrap (subscriber init, run) |
| `src/state.rs` | plugin 내부 상태 구조 (Mutex 등) |
| `src/handlers.rs` | IPC 메서드별 dispatch (`handle_ipc_method` 의 match 본문이 길어질 때) |
| `src/install.rs` | 외부 시스템 설치/제거 (예: Claude `~/.claude/settings.json` hook 등록) |

`tasty-plugin-codex` (state + handlers 분리) 와 `tasty-plugin-claude` (state + handlers + install + hook + error_scan) 가 reference. main.rs 만으로도 충분한 단순 plugin (image 73줄, codex 97줄) 은 분리하지 않아도 된다.

### Plugin 데이터 위치 환경변수 사용

`TASTY_PLUGIN_DIR` 와 `TASTY_PLUGIN_DATA_DIR` 를 헷갈리지 말 것:

- `TASTY_PLUGIN_DIR` 은 plugin 의 매니페스트가 있는 곳 — **읽기 전용으로 다룬다**. 업그레이드시 통째 덮어쓰여지므로 여기에 사용자 데이터를 쓰면 다음 업그레이드때 사라진다.
- `TASTY_PLUGIN_DATA_DIR` 은 사용자 데이터 영역 — **쓰기 OK**. 업그레이드시 보존된다.

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
