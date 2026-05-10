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
