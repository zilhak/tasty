# 플러그인 제작 가이드

외부 Tasty 플러그인을 작성·빌드·설치하는 법. 개념(배포/통합 축·권한)은 [concepts/plugins](../concepts/plugins.md) 먼저. 권한 모델 상세는 [plugin-permissions](plugin-permissions.md), 민감 데이터는 [plugin-sensitive-data](plugin-sensitive-data.md).

**번들 플러그인이 곧 reference 예제다** — 각 기여 타입을 만들 때 아래 표의 해당 플러그인 코드를 시작점으로 복사·수정하는 게 가장 빠르다.

## 기여 타입 → 예제 플러그인

| 만들고 싶은 것 | 보면 되는 번들 플러그인 | 난이도 |
|---------------|------------------------|--------|
| **egui-mesh surface** (자가 렌더 mesh 합성) | [image](../plugins/image/index.md) · [mesh-demo](egui-mesh-channel.md)(최소 PoC) | ★★ |
| **webview surface** | [html](../plugins/html/index.md) · [markdown](../plugins/markdown/index.md)(+파일 핸들러·settings, ADR-0065) | ★★ |
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

- **`rendering = "egui-mesh"`** (image/mesh_demo, 그리고 markdown 의 확인 팝업 2개만): 플러그인이 **자기 프로세스에서 egui 를 tessellate** 한 mesh 를 host 가 전용 `egui_wgpu::Renderer` 로 합성. SDK 를 `features=["egui-mesh"]` 로 받아 `paint_surface` 에서 `EguiMeshSurface::paint(...)` 호출. bundled 화이트리스트 + api_version gate. 채널 상세는 [egui-mesh-channel](egui-mesh-channel.md).
- **`rendering = "webview"`** (html, markdown — [ADR-0065](../adr/0065-markdown-webview-render-channel.md)): host 의 네이티브 WebView 오버레이로 그림. html 은 surface 의 URL 을 host 가 동기화하고, markdown 은 plugin 이 직접 sanitize 된 HTML 문서를 생성해 로드시킨다.
- **`rendering = "remote"` (기본)**: webview 와 같은 `RemoteSurface` stand-in 등록만 하는 marker — host 는 이 kind 의 콘텐츠를 그리지 않는다. `snapshot_surface`/`restore_surface` 로 세션 복원.

surface kind 선언에는 host 가 kind-agnostic 하게 소비하는 메타가 함께 실린다 — host 본체에 `if kind == "..."` 를 박지 않기 위한 것들이다:

- **`icon`** — 탭/프리셋 leading 아이콘의 **이름**. host 가 자기 아이콘 세트에서 `icons::from_name` 으로 glyph 에 매핑한다(현재 이름: `markdown`/`folder`/`image`/`html`/`terminal`/`file`; 미지의 이름은 `file` 로 fallback). 예: markdown 의 `icon = "markdown"`.
- **`preset_fields`** — 프리셋 편집기가 이 kind 를 편집할 때 노출할 입력 필드 스키마. `required = true` 인 `param_key` 는 surface 생성 IPC(`pane.split`/`workspace.new`)의 **필수 파라미터**로도 쓰인다(단일 진실원). 예: markdown 은 `file` 필드 하나(required).
- **`param_aliases`** — 옛 caller 가 넘기는 alias 키 → canonical 키 매핑. host 가 convert 경로에서 정규화한다. 예: markdown 의 `{ file_path = "file" }`.
- **`default_params`** — surface 생성 시 params 에 없으면 host 가 주입하는 기본값(키 → 리터럴 또는 정책 토큰). 정책 토큰: `@settings.explorer_view_mode`(Settings 의 마지막 explorer view mode), `@home`(홈 디렉토리 — **새 탭 생성 fresh-context 에서만** 해석; split/preset/workspace 처럼 cwd 를 상속·carry 하는 경로에선 건너뛴다). 예: explorer(builtin)의 `{ view_mode = "@settings.explorer_view_mode", path = "@home" }`.
- **capability flags**(모두 기본 false) — host 의 입력/줌/복사/붙여넣기 게이트를 kind 하드코딩 없이 판정한다:
  - **`consumes_egui_input`** — host 가 이 kind 를 host egui 위젯으로 렌더해 winit 키/IME 를 host egui 로 흘린다(예: explorer). egui-mesh 렌더 kind 는 false(중앙 키 디스패처가 forward).
  - **`zoomable`** — 줌 in/out/reset 단축키로 폰트 크기 override 조절(예: markdown/explorer).
  - **`egui_copy`** — copy 단축키를 이 kind 의 egui-mesh surface 에 `Copy` wire 이벤트로 forward한다. plugin 자신의 egui `Context` 가 텍스트 선택(selectable label/`TextEdit`)을 복사하고, plugin 이 그 텍스트를 OS 클립보드에 직접 쓴다(ADR-0009 — host round-trip 없음). markdown 이 webview 로 전환된 뒤([ADR-0065](../adr/0065-markdown-webview-render-channel.md)) 현재 이를 선언하는 번들 plugin 은 없다.
  - **`copy_path`** — select-all / copy-path 단축키(선택 항목 경로 복사) 소비(예: explorer).
  - **`egui_paste`** — paste 를 이 kind 가 자체 소비(host 가 terminal paste 로 흘리지 않음, 예: image).
- **`name_from_param`** — 자동 탭 명명 시 basename 을 파생할 params 키. 선언하면 그 키 값의 basename 을 탭 표시명으로 쓴다(예: markdown/image 는 `"file"`, explorer(builtin)는 `"path"` → `README.md`). 미선언이면 kind 표시명(`display_name_i18n_key`)으로 fallback. host 의 `kind == "markdown"` basename 명명 하드코딩을 대체.
- **`records_recent`**(기본 false) — 이 kind 의 surface 를 파일로 열 때 host 가 "최근 연 파일" 목록에 kind 별로 기록할지. host 는 특정 kind 이름을 모르고 이 플래그로 기록 대상을 판정한다(generic per-kind). plugin 은 host 의 generic `recent.query {kind}` IPC 로 자기 최근 목록을 조회한다(예: markdown 주소창 드롭다운). 예: markdown 은 `true`.
- **`convert_requires_input`**(기본 false) + **`convert_input_popup`** — 이 kind 로 convert 하려면 host 가 먼저 "파일 입력 팝업"을 띄워야 하는지, 그리고 그때 열 이 plugin 의 팝업 **local id**. host 는 kind 이름·event key 하드코딩 없이 이 데이터만 따라 `<plugin_id>/<popup_id>` 팝업을 `open_popup_instance` 로 연다(payload 의 `surface_id` 로 제자리 변환 / 새 탭 분기). 예: markdown 은 `convert_requires_input = true`, `convert_input_popup = "file-open"`([ADR-0043](../adr/0043-convert-input-popup-capability.md)). 미선언이면 빈 params 즉시 변환.

> 대용량 파일 확인 게이트는 SurfaceKindDef 필드가 아니라 **플러그인 소유**다. 플러그인이 자기 프로세스에서 크기를 감지(`std::fs::metadata`)해 event 를 publish 하고, event trigger `[[contributes.popup]]`(아래 "도구 메뉴 항목 + popup")로 확인 팝업을 자가 렌더한다(예: markdown). host 는 파일 크기를 알지 않는다.

### 파일 핸들러 (detector + handler)

확장자 → surface 매핑. `[[contributes.detector]]`(확장자 규칙) + `[[contributes.handler]]`(`action = open_surface{surface_kind}`). 권한: `file_handler.define`(신규 detector) / `file_handler.extend:<id>` / `file_handler.handle:<id>`. handler `id` 는 short name — install 단계가 `<plugin_id>/<id>` 로 자동 prefix. priority 동순위면 owner tiebreak `user > plugin > host`. 예: [image](../plugins/image/index.md)·[markdown](../plugins/markdown/index.md).

### 훅 핸들러 (webhook/hook 트리거)

`[[contributes.hook_handler]]` — 인바운드 웹훅 또는 내부 hook 이벤트가 발사됐을 때 실행할 **IpcSequence**(고정 IPC 호출들 + 페이로드 값 치환)를 선언한다. 권한: `hook_handler.define`. plugin 은 `action.kind = "ipc_sequence"` 만 쓸 수 있고 셸(`shell_command`)은 타입 레벨에서 배제된다(host/user 전용). `source` 로 트리거 출처를 게이트한다 — `webhook`(외부 HTTP) / `hook`(내부 이벤트) / `any`. handler `id` 는 short name(`[a-z0-9-]{1,32}`) → install 단계가 `<plugin_id>/<id>` 로 자동 prefix. priority 동순위면 owner tiebreak `user > plugin > host`.

plugin 이 자기 훅 핸들러를 웹훅에 붙이려면 `webhook.register` 를 호출한다 — 이때 `network` 권한이 필요하고, plugin 은 **인라인 sequence 를 못 쓰고 자기 소유(`<plugin_id>/…`) 핸들러 id 만** 바인딩할 수 있다(임의 시퀀스는 owner=Local 전용 채널).

### 완료 판정 전략 (agent task `Custom` dispatch 완료 판정)

`[[contributes.completion_strategy]]` — `agent.task_create` 의 `TaskCommand::Custom.poll` 이 이름으로 참조할 수 있는 완료 판정 전략을 선언한다(자세한 모델·결정 사항은 [agent-runner](agent-runner.md)의 "완료 판정 전략 레지스트리" 참고). 권한: `completion_strategy.define`. `spec.kind = "poll"`(`poll_method`/`state_field`/`terminal_states` 등, `PollSpec` 과 1:1) 또는 `"push"`(`notify_via`: 자기 자신 또는 host 소유 훅 핸들러 id + 필수 `timeout_ms`). `poll_method` 와 `default_for_methods`(결정 6 — 이 전략이 기본 판정이 되는 IPC 메서드 목록)는 plugin 소유면 자기 namespace(`<plugin_id>.*`) 만 가리킬 수 있다. id 는 short name → install 단계가 `<plugin_id>/<id>` 로 자동 prefix.

### 도구 메뉴 항목 + popup

- `[[contributes.tool]]`(`ui.tool_item`) — [도구 메뉴](../features/tools-menu/index.md)에 항목. `action.kind`: `event`(Event Bus 발화) / `open_surface`(탭 추가) / `open_popup`(`popup_id = <plugin_id>/<id>`). `order_hint` 오름차순(빌트인 0..99).
- `[[contributes.popup]]`(`ui.popup`) — trigger `event`(자동 open) 또는 `ipc`(명시 호출). SDK 콜백 `open_popup`/`paint_popup`/`on_popup_closed`(egui-mesh). 동일 `popup_id` 라도 `instance_id` 가 다르면 별개 인스턴스. 예: [git-viewer](../plugins/git-viewer/index.md)·[clipboard-viewer](../plugins/clipboard-viewer/index.md).

### CLI + IPC namespace

`[[contributes.ipc_namespace]]`(prefix) + `[[contributes.cli]]`(`tasty <name> …`). 플러그인은 `handle_ipc_method` 로 `<prefix>.*` 메서드를 받는다. prefix 는 소문자+숫자+`_`, 호스트 예약어 금지 — 목록은 `tasty_plugin_manifest::validators::RESERVED_IPC_PREFIXES` 하나뿐이고 46 개다. 매니페스트 검증이 여기 걸리면 plugin 이 뜨지 않는다. **호스트가 자기 메서드에 쓰는 prefix 는 전부 예약이라고 보면 된다** — 유일한 예외가 `image`·`markdown` 이고, 번들 plugin 이 이미 같은 이름의 namespace 를 갖고 있어서 예약할 수 없다(예약하면 그 plugin 의 매니페스트가 거절된다). 그래서 이름은 자기 plugin 고유어로 짓는다 — `theme`·`session`·`preset` 처럼 호스트가 쓰는 일반 명사는 거절된다. 목록과 호스트 메서드 표의 정합은 `every_host_method_prefix_is_reserved_or_carries_a_reason` 이 양방향으로 지킨다. 결정과 감수한 비용은 [ADR-0140](../adr/0140-host-ipc-prefixes-are-reserved-where-they-can-be-enforced.md). **그 예외 둘에는 따라오는 의무가 있다** — host 가 같은 prefix 아래 구현한 메서드는 그 plugin 의 `handle_ipc_method` 가 self-call trampoline(`host.call(&ctx.method, ctx.params)`)으로 host 에 되돌려 줘야 한다. arm 이 없으면 그 host 구현은 **plugin 이 설치돼 있는 동안에만** 외부에서 안 닿아, 같은 호출의 결과가 설치 상태에 따라 흔들린다. `bundled_plugin_namespace_coverage` 가 매니페스트 prefix 마다 이를 강제한다 — 판정은 `handle_ipc_method` **본문만** 본다(plugin 이 host 로 *거는* 같은 이름의 `host.call` 이 파일 안에 있어서, 파일 전체를 세면 빠진 arm 을 놓친다). 근거는 [ADR-0153](../adr/0153-a-bundled-namespace-hands-host-methods-back.md). CLI 서브커맨드의 `ipc_method` 는 자기 prefix 와 매칭돼야 한다. **CLI top-level 이름은 매니페스트가 판정하지 않는다** — 호스트 명령(그 alias 포함)과 겹쳐도 매니페스트는 통과하고, 대신 등록 시점에 그 이름만 건너뛰며 경고가 뜬다(`tasty <name>` 은 호스트 명령이 그대로 받는다). 매니페스트 크레이트는 CLI 크레이트 아래에 있어 실제 clap 명령 집합을 볼 수 없고, 거기 손목록을 두면 호스트 명령이 늘 때마다 늙기 때문이다 — 실제로 늙어 있었고, 목록 밖 이름은 debug 빌드에서 `tasty --help` 를 포함한 CLI 전체를 패닉시켰다. 예: [codex](../plugins/codex/index.md)·[claude](../plugins/claude/index.md).

### 단축키 (commands)

`[[contributes.commands]]` — `id` · `default_keybinding` · `binding_mode`(`independent` 또는 `inherit:<host_action>`) · `scope`(`global`(기본) 또는 `surface`) · `action`(선택, `[[contributes.tool]].action` 과 동일한 `ToolAction`).

**scope 별 발화 조건**:

- `scope = "global"`(기본값) — **어디서나 동작한다.** 포커스된 surface 가 무엇이든(다른 plugin surface, 터미널 tab, 아무 surface 도 없는 상태 포함) 등록된 키를 누르면 발화. 단일 키는 다른 곳(터미널 입력 등)과 충돌하기 쉬우므로 **조합키만 권장** — `default_keybinding` 이 modifier 없는 단일 키면 매니페스트 validate 단계에서 거부된다(`scope = "surface"` 로 바꾸거나 조합키를 쓸 것).
- `scope = "surface"` — 이 플러그인이 만든 surface(`RemoteSurface`)에 포커스가 있을 때만 발화. 단일 키(F5 등)도 허용.
- 포커스된 plugin surface 가 있으면(어떤 plugin 이든) 그 plugin 의 커맨드가 scope 무관하게 최우선 후보가 된다 — "그 plugin surface 가 포커스되어 있다"는 조건 자체가 `surface` scope 의 조건을 이미 만족하기 때문. 포커스된 plugin surface 가 없을 때는 등록된 모든 plugin 의 `global` 커맨드만 후보가 된다. 호스트 `KeybindingSettings` 와 같은 키가 겹치면 **plugin 이 항상 우선**(자세한 우선순위 규칙은 [`key-mapping.md`](../design/policies/key-mapping.md#plugin-커맨드-단축키-우선순위)).

**동작 방식(`action` vs `handle_command`)**: `action` 을 선언하면 호스트가 `[[contributes.tool]]` 과 동일하게 그 액션(`event`/`open_surface`/`open_popup`)을 직접 실행하고, 옛 `command.invoke` IPC(SDK `handle_command`)는 이 커맨드에 대해 발사되지 않는다 — popup 을 여는 것뿐인 커맨드라면 `handle_command` 를 구현할 필요가 없다. `action` 을 선언하지 않으면 기존처럼 `command.invoke` → SDK `handle_command` 왕복 경로를 쓴다. Event Bus `command.invoked` owner-unicast 통지는 `action` 유무와 무관하게 항상 발사된다(관찰용, 구독 안 해도 무방).

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

`rendering = "egui-mesh"` surface 와 popup/banner 는 egui-mesh 채널 하나로 통한다 —
plugin 이 자기 프로세스에서 egui 를 구동해 tessellate 한 `(ClippedPrimitive,
TexturesDelta, ppp)` 를 SharedBuffer 로 host 에 보내고 host 가 합성한다. 위젯 어휘
제한이 없고(egui 전부 사용 가능) 색·간격은 host 가 forward 한 `Theme` 토큰에서
가져온다. 상세·SDK 헬퍼(`EguiMeshSurface`/`EguiMeshPopup`/`EguiMeshBanner`)는
[egui-mesh-channel](egui-mesh-channel.md). (`rendering = "webview"` surface(html/markdown)
의 본문은 이 채널을 타지 않는다 — host native WebView 가 직접 렌더한다. 단 markdown
의 대용량/파일열기 확인 팝업 2개는 여전히 egui-mesh 채널을 쓴다.)

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

**`TASTY_PLUGIN_DATA_DIR` 수명 계약** — 실경로는 `~/.tasty/plugin-data/<plugin-id>` 로 **설치 디렉터리(`TASTY_PLUGIN_DIR`)와 분리**돼 있고(`crates/tasty-host-plugin/src/process.rs`), 번들 plugin 재동기화(`upgrade-builtins`)는 설치 디렉터리 내용만 mirror 하므로(`crates/tasty-host-plugin/src/builtin.rs`) data dir 을 건드리지 않는다. 즉 §9.1 의 `disable` → `upgrade-builtins` → `enable` 절차나 plugin 업그레이드·재설치를 건너 **data dir 내용은 보존된다** — plugin 을 지우기 전까지 살아 있어야 하는 상태(claude plugin 의 checklist 라운드, 프로필 부착 기록 등)를 여기 두어도 안전하다. 반대로 설치 디렉터리에 쓴 것은 다음 업그레이드에 사라진다.

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
| `TASTY_LOCALE` | 활성 로케일(`general.language`) — host 본 바이너리가 부팅 시 자기 프로세스 env 에 set 하고(`src/boot/locale.rs`) spawn 시 그대로 propagate 한다(host-plugin 은 `tasty-i18n` 비의존). SDK `Translator` 가 소비. spawn 시점 고정 — 언어 변경은 재시작 후 반영([ADR-0103](../adr/0103-plugin-locale-via-host-process-env.md)) |
| `TASTY_LOCALE_FONT` | 언어팩이 제공하는 폰트 파일의 절대경로 — **언어팩 폰트가 resolve 됐을 때만** 주입(내장 폰트 · 미제공이면 미설정, 셸에서 상속된 값도 자식에서 제거). 출처와 고정 시점은 `TASTY_LOCALE` 과 같다 |
| `TASTY_HOST_PID` | 호스트 프로세스 PID (**macOS 만** — SDK watchdog 가 부모 사망 감지에 사용) |

### 생명주기 (healthcheck / 자동 재시작·비활성화)

`manager.rs` 상수 기준:

- **부팅**: `~/.tasty/plugins/` 스캔 → enabled 전부 spawn.
- **헬스체크**: `PING_INTERVAL`(15s)마다 ping, `HEALTHCHECK_TIMEOUT`(60s) 무응답이면 강제 재시작.
  판정은 ping 을 보내는 tick 에서 함께 하므로 **비응답 검출 상한은 60s + 15s = 75s** 다
  ([timer-hub](timer-hub.md#계층을-넘는-허브-합성)). 프로세스가 실제로 죽은 경우는 이 경로가
  아니라 event 채널 Disconnected 로 즉시 잡히므로 이 상한의 영향을 받지 않는다.
- **자동 비활성화**: `RESTART_FAILURE_WINDOW`(10s) 내 `RESTART_FAILURE_LIMIT`(3)회 spawn 실패 → 정지(사용자가 `tasty plugin enable` 로 수동 재개까지).
- **종료**: shutdown 메서드 송신 후 timeout, 초과 시 kill.

### 프로세스 수명 결박 (3 OS — 크래시·강제종료 포함)

위 "종료" 경로는 `PluginProcess::shutdown` / `Drop` 의 `child.kill()` 에 의존하므로 **정상 종료만** 커버한다. 하드 크래시·`taskkill /f`·디버거 강제종료 등 Drop 이 돌지 않는 경로에서는 플러그인이 고아로 잔존할 수 있다. 이를 OS 커널 레벨에서 막기 위해, 호스트가 어떤 식으로 죽든 플러그인이 함께 종료되도록 결박한다 (`crate::reaper::PluginReaper`, spawn 시 `prepare`/`adopt` 배선). OS 별 메커니즘이 비대칭이라 단일 추상화 뒤에 숨긴다:

| OS | 메커니즘 | 통합 지점 | 손자(node/chrome) |
|----|----------|-----------|--------------------|
| **Windows** | Job Object (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`). 호스트가 Job 핸들을 `PluginManager` 수명 동안 소유 → 호스트 사망 시 핸들 닫히며 OS 가 Job 내 전 프로세스 강제 종료. | 호스트 `adopt`(각 자식 assign) | **자동 커버**(Job 멤버십 자식 상속) |
| **Linux** | `prctl(PR_SET_PDEATHSIG, SIGKILL)` (자식 `pre_exec`). 부모 사망 시 커널이 직속 플러그인에 SIGKILL. **PDEATHSIG 는 fork 한 *스레드* 종료에 발화**하므로(man prctl 경고) 모든 spawn 은 `PluginReaper::spawn_bound` 가 프로세스 수명의 영속 spawner 스레드에서 fork 한다 — 단명 스레드(부트 워커 등)에서 직접 spawn 하면 그 스레드 종료 시 plugin 전원이 SIGKILL 된다. | 호스트 `prepare`(pre_exec) + `spawn_bound`(영속 스레드 fork) | 고아 허용(범위 밖) |
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
- **i18n**: 매니페스트 `*_i18n_key` 는 host 가 lookup. 플러그인이 직접 그리는 텍스트는 `tasty_plugin_sdk::i18n::Translator`(`TASTY_LOCALE` 주입 — host 가 부팅 시 `general.language` 에서 set, §7 표). 키는 자기 prefix 안에만(`surface.kind.<own>` 만 예외).
- **권한 표기**: 실제 필요한 것만. 자기 namespace `ipc.invoke:<self>` 금지(self-loop 차단으로 무용).
- **모듈 분리**: `main.rs` 가 ~300 줄을 넘으면 `state.rs`/`handlers.rs`/`install.rs` 로 분리한다(`crates/tasty-plugin-claude/src/` · `crates/tasty-plugin-codex/src/` 가 reference). 커지면 실제로 가른다 — `crates/tasty-plugin-image/src/` 가 그 예로, 렌더와 문서 처리를 `crates/tasty-plugin-image/src/render.rs` · `crates/tasty-plugin-image/src/doc.rs` 로 냈다. 아직 단일 `main.rs` 인 것도 있다(`crates/tasty-plugin-html/src/main.rs`). **줄 수는 적지 않는다** — 커밋마다 바뀌는 값이라 적는 순간 낡고, 예시가 낡으면 규칙이 자기 반대를 가르친다(ADR-0139).
- **Cargo**: `tasty-plugin-protocol` 직접 의존 금지 — SDK 가 re-export. `[lints] workspace = true`.

## 9. 빌드 & 설치

```bash
cargo build --release -p my-plugin
tasty plugin install ./          # 매니페스트 권한 자동 grant + spawn
```

**워크스페이스 내 번들 플러그인 개발**: `BUILTINS`(`crates/tasty-host-plugin/src/builtin.rs`) 등록 플러그인은 워크스페이스 빌드 시 호스트가 부팅에 자동 sync(`ensure_dev_bundle` → `install_builtins_if_needed`). 단 루트 `cargo build` 는 본 바이너리만 빌드하므로 플러그인 변경은 `cargo build -p <crate>` 또는 `--workspace` 필요. **부팅 없이, 실행 중인 tasty 에 플러그인 변경만 반영하는 절차(호스트 재빌드·재시작 불필요)는 아래 §9.1.**

디버깅: `tasty plugin logs <id> --follow` / `~/.tasty/plugins-logs/<id>.log` / `RUST_LOG=debug`.

### 9.1 실행 중인 tasty 에 번들 플러그인만 반복 갱신 (호스트 재빌드·재시작 불필요)

**플러그인은 호스트(tasty 본체)와 별개의 자식 프로세스다.** 번들 플러그인의 코드/매니페스트/임베드 리소스만 고쳤다면 **본체를 재빌드·재시작할 필요 없이 그 플러그인만** 갱신하면 실행 중인 tasty 에 반영된다. (호스트 코드 `src/`·다른 크레이트를 고쳤을 때만 본체 재빌드+재시작이 필요하고, 그건 실행 중 exe 잠금 때문에 보통 사용자 도움이 필요하다.)

**0) 실행 중인 tasty 의 프로필을 먼저 확인** — 그 프로필로 플러그인을 빌드해야 한다. 번들 스테이징 경로가 `target/<profile>/builtin-plugins/` 라, 프로필이 어긋나면 엉뚱한(옛) 바이너리가 sync 된다.
```bash
# 어떤 exe 가 떠 있나: Windows  tasklist | findstr tasty  ·  Linux/macOS  ps aux | grep tasty
# 경로로 profile 판별 (…/target/release/tasty → release, …/debug/… → debug, 설치본 → dist).
# 개발 중이면 보통 target/release/tasty → 이하 예시도 --release.
```

**1) 그 프로필로 플러그인만 빌드**
```bash
cargo build --release -p tasty-plugin-<name>      # 실행 중 tasty 와 같은 프로필
```

**2) 재서명 (release/dist 호스트는 매니페스트 서명을 검증)** — 안 하면 다음 단계가 `untrusted: UnknownKey` 로 skip. debug 호스트는 서명 안 보므로 불필요.
```bash
./scripts/sign-bundle.sh --key ~/.tasty-keys/dev.pem --manifest crates/tasty-plugin-<name>/tasty-plugin.toml
```

**3) 정지 → 재동기화 → 재기동 (순서 중요)**
```bash
tasty plugin disable com.x.<name>     # 먼저 정지. 안 하면 실행 중 .exe 를 잠가 upgrade 가 'os error 5(액세스 거부)'
tasty plugin upgrade-builtins         # 번들→user dir(~/.tasty/plugins) 재sync. 매니페스트 version 올렸으면 upgraded
#   ※ version 을 안 올려도 반영된다 — 같은 버전 갈래는 **내용으로** 판정해 다른 파일만 옮긴다
#      (2026-09-07 부터. 그전에는 mtime 비교였고, `cp -p`·아카이브처럼 mtime 이 보존되면 조용히 건너뛰었다).
#      보고문은 여전히 'skipped' 로 나오지만 사유가 갈린다 — 'content resync: files rewritten' 이면 옮긴 것이고
#      'nothing to write' 면 이미 같았다는 뜻이다. `--force` 는 **내용까지 같은데도** 다시 쓸 때만 필요하다.
tasty plugin enable com.x.<name>      # 재기동 — 호스트가 새 매니페스트를 레지스트리에 재적재
```

내용 비교를 **해시가 아니라 바이트로** 하는 근거와 잰 값·대안·재검토 조건은
[ADR-0191](../adr/0191-two-local-files-are-compared-bytewise-not-hashed.md).

**4) 실행 중 tasty 에 대해 실동작 검증**
```bash
tasty <plugin-cli> --help             # 새 서브커맨드/매니페스트 반영 확인
tasty <plugin-cli> <cmd> ...          # CLI→IPC→실행 중 호스트→플러그인 경로로 실제 동작 확인
```

> **왜 호스트 재시작이 필요 없나**: `tasty <plugin> --help` 는 매번 새 CLI 프로세스가 **설치된 매니페스트**(`~/.tasty/plugins/<id>/`)를 읽으므로 upgrade 즉시 반영되고, IPC 디스패치는 `disable/enable` 이 플러그인 프로세스를 재기동하며 호스트 레지스트리를 갱신한다. 따라서 **본체 GUI 를 껐다 켤 필요가 없다.**

## 10. 한계 (현재 SDK)

- async 미지원 — 모든 콜백 동기(무거운 I/O 는 플러그인 내부 thread).
- HotReload 미지원 — 코드 변경은 재빌드 후 `disable && enable`(전체 반복 절차는 [§9.1](#91-실행-중인-tasty-에-번들-플러그인만-반복-갱신-호스트-재빌드재시작-불필요)). 단 플러그인만 갱신하면 되고 **호스트 재빌드·재시작은 불필요**.
- 권한 게이트는 **호스트 IPC 호출만** 막는다 — 플러그인 프로세스의 직접 `std::fs` 는 OS 샌드박스가 없는 한 강제 안 됨([plugin-permissions 한계](plugin-permissions.md#한계)).

## 관련

- [concepts/plugins](../concepts/plugins.md) — 분류 축·권한 개요
- [plugin-permissions](plugin-permissions.md) · [plugin-sensitive-data](plugin-sensitive-data.md)
- [plugins/](../plugins/index.md) — 번들 플러그인(= 예제) 카탈로그
- [features/plugin-system](../features/plugin-system/index.md) — 설치/관리 UI
</content>
