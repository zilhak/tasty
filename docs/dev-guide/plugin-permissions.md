# 플러그인 권한 모델

플러그인이 호스트 IPC 를 호출할 때 적용되는 권한 게이트의 동작 원리 + 토큰 전체 목록(각 토큰이 실제로 무엇을 여는지) + 새 IPC 메서드/토큰 추가 절차. 권한이 plugin 모델 어디에 놓이는지는 [concepts/plugins](../concepts/plugins.md#권한-permissions), 제작 흐름은 [plugin-development](plugin-development.md).

## 구성 요소

| 위치 | 역할 |
|------|------|
| `crates/tasty-plugin-manifest/src/types.rs::Permission` | 권한 enum + 토큰 매핑(`from_token`/`as_token`). 새 토큰은 여기 |
| `crates/tasty-ipc/src/method_meta.rs::method_meta` | IPC 메서드 → 필요 권한 / plugin 호출 가능 여부 (단일 진실원) |
| `crates/tasty-ipc/src/caller.rs::CallerContext` | 호출자 종류 (Local / Internal / Plugin / Agent) + `ensure_allowed` |
| `src/adapters/ipc/handler.rs::handle_with_caller` | 라우터 진입에서 `ensure_allowed` + capability elevation 자동 발행 + audit |
| `crates/tasty-host-plugin/src/manager.rs::plugin_permissions` | plugin id → `Arc<HashSet<Permission>>` 캐시 |
| `crates/tasty-host-plugin/src/registry_state.rs` | `plugins.toml` 의 grant 영속화 |

## 토큰 형식 — `<name>[:<scope>]`

- **`.`** 는 *이름의 일부*다 (`surface.read` 의 `.` 는 분류용 — 호스트는 쪼개지 않음).
- **`:`** 는 *scope 구분자*다 — 권한이 적용되는 대상을 한정해야 의미가 생기는 권한에만 등장.

전체 토큰 목록은 아래 [토큰 전체 — 무엇을 여나](#토큰-전체--무엇을-여나). scoped 검증 규칙:

- `ipc.invoke:<prefix>` — `is_valid_ipc_prefix`(소문자 시작+소문자/숫자/`_`, ≤32) **그리고** 호스트 예약어 거부.
- `ext:<plugin_id>` — `is_valid_plugin_id`(reverse-DNS).
- `file_handler.extend:<id>` / `file_handler.handle:<id>` — `is_valid_detector_id`, `$unknown` 거부.
- `hook_handler.handle:<id>` — `is_valid_hook_handler_id`(소문자+숫자+`-`, ≤32). `$`-prefix reserved 개념 없음. `hook_handler.define` 은 scope 없는 base 토큰.

형식 위반 토큰은 `from_token` 이 `None` → 매니페스트 로드 단계에서 거부.

## 토큰 전체 — 무엇을 여나

`Permission` enum 의 `as_token` 이 토큰 문자열의 단일 출처다(exhaustive match — variant 를 늘리면 팔이 강제된다). 각 토큰이 **실제로 여는 것**은 두 종류다: `method_meta` 가 그 권한을 요구하는 **호스트 IPC 메서드**, 그리고 매니페스트 로드·런타임이 그 토큰 없이는 거부하는 **contribute/호출 게이트**. 둘 다 없는 토큰은 아직 아무것도 열지 않는다 — 표에 그대로 적는다. 한 메서드가 여러 권한을 **함께** 요구할 수 있으므로(`image.paste` 는 `surface.write` + `clipboard.read`) 같은 메서드가 여러 행에 나온다.

| 토큰 | 여는 호스트 IPC | 그 밖의 게이트 |
|------|-----------------|----------------|
| `surface.read` | 트리·메타 조회 (`tree` · `workspace.list` · `tab.list` · `surface.list` · `surface.meta.get` · `hook.list` · `message.read` · `recent.query` 등) | — |
| `surface.write` | 트리 변경 (`workspace.create` · `tab.create` · `tab.close` · `split` · `surface.close` · `surface.set_cwd` · `surface.meta.set` · `hook.set` · `message.send` 등) | — |
| `notification` | `notification.create` · `notification.list` · `surface.completion` · `surface.attention.get` · `surface.attention.clear` | — |
| `clipboard.read` | `image.paste` **하나뿐** | — |
| `clipboard.write` | `clipboard.set_text` | — |
| `fs.read` | `fs.pick_file` · `file_handler.dispatch` · `image.open` · `markdown.navigate` · `git_viewer.query` · `file_picker.trigger` | — |
| `fs.write` | `image.save` · `image.export_png` | — |
| `process.spawn` | **없음** — `method_meta` 어느 메서드도 요구하지 않는다 | 없음 |
| `terminal.spawn` | `terminal.spawn` · `terminal.respawn` · `pty.spawn` · `pty.attach_surface` · `surface.wake` · `surface.respawn_terminal` | — |
| `terminal.write` | `surface.send` · `surface.send_key` · `surface.send_to` · `terminal.tell` · `terminal.broadcast` · `pty.write` · `pty.kill` 등 | — |
| `terminal.read` | `surface.read_since_mark` · `surface.screen_text` · `surface.commands` · `output.observe_*` · `pty.read` · `pty.wait` 등 | — |
| `network` | `webhook.register` **하나뿐** (아래 "network" 절) | — |
| `memory.read` | `memory.get/list/query/export` · `memory.exists/count/scopes/stats` · `memory.bb_*` 조회 · `memory.plan_*` 조회 · `memory.cache_get/cache_list` · `memory.goal_get` · `approval.summary.get` | — |
| `memory.write` | `memory.put/delete/import` · `memory.bb_*` 변경 · `memory.plan_*` 변경 · `memory.cache_*` · `memory.goal_set/clear` · `approval.summary.set` | — |
| `memory.secret` | `memory.secret.*` 전부 | — |
| `approval` | `approval.*` 전부 · `plugin.request_permission`. `approval.summary.get/set` 은 `memory.read`/`memory.write` 를 함께 요구 | — |
| `telemetry` | `telemetry.*` 전부(기록·조회·cap·anomaly) | — |
| `agent` | `agent.*` 협업 primitive 전부 · `session.issue` · `session.revoke` | — |
| `ui.tool_item` | 없음 | `[[contributes.tool]]` 매니페스트 게이트 + 도구 메뉴 노출 조건(grant 없으면 항목이 뜨지 않음) |
| `ui.popup` | `popup.close` | `[[contributes.popup]]` · `[[contributes.commands]]`(`action.kind = "open_popup"`) 매니페스트 게이트 + popup contribute 노출 조건 |
| `ui.banner` | `banner.open` · `banner.close` | `[[contributes.banner]]` 매니페스트 게이트 |
| `ui.settings_page` | `settings.get_plugin_setting` | `[[contributes.settings_pages]]` 매니페스트 게이트 |
| `window.spawn` | 없음 | `[[contributes.window]]` 매니페스트 게이트. spawn 핸들러 자체가 아직 schema + stub 이라 이 토큰이 여는 실행 경로는 없다 |
| `file_handler.define` | 없음 | `[[contributes.detector]]` 로 **신규** detector id 를 선언할 때 요구 |
| `hook_handler.define` | 없음 | `[[contributes.hook_handler]]` 매니페스트 게이트 |
| `completion_strategy.define` | 없음 | `[[contributes.completion_strategy]]` 매니페스트 게이트 |
| `ipc.invoke:<prefix>` | 그 prefix 를 점유한 plugin 의 namespace 메서드 | `manager/ipc_dispatch.rs` 런타임 게이트(없으면 `-32001`). 자기 namespace 는 별도로 차단 |
| `ext:<plugin_id>` | 없음 | `[extends]` 매니페스트 게이트 + grant 되어야 extension 이 활성(`recompute_extensions`) |
| `file_handler.extend:<id>` | 없음 | 기존 detector id 재선언(rule 추가) 시 요구 |
| `file_handler.handle:<id>` | 없음 | `[[contributes.handler]]` 가 그 detector 에 붙을 때 요구 |
| `hook_handler.handle:<id>` | 없음 | **없음** — 형식 검증만 있고 강제하는 지점이 아직 없다(핸들러 id 단위 grant 가시성을 위해 예약된 토큰) |

### 선언 범위와 실제 개방 범위는 같지 않다

토큰 이름은 넓은 범주를 가리키지만, 그 토큰이 지금 여는 호스트 IPC 는 표의 가운데 열이 전부다. 특히:

- **`clipboard.read` 는 `image.paste` 만 연다.** 클립보드 내용을 읽는 번들 plugin(clipboard-viewer)은 호스트를 거치지 않고 자기 프로세스에서 직접 OS 클립보드를 읽는다([ADR-0026](../adr/0026-clipboard-history-removal-plugin-direct-read.md)) — 그 읽기는 이 토큰이 통제하지 않는다.
- **`process.spawn` 과 `window.spawn` 은 아무 IPC 도 열지 않는다.** 전자는 요구하는 메서드가 없고, 후자는 매니페스트 게이트만 있다.
- **`hook_handler.handle:<id>` 는 강제 지점이 없다.** 매니페스트에 적을 수 있고 형식 검증도 받지만, 그 토큰 유무로 갈리는 동작이 아직 없다.

토큰을 넓은 이름 그대로 읽으면 grant 화면이 실제보다 큰 권한을 넘기는 것처럼 보인다. 반대 방향의 오해가 더 중요하다 — **plugin 프로세스가 자기 손으로 하는 일은 어느 토큰도 막지 못한다**(아래 [한계](#한계)).

### `network` — 여는 것 하나 + 정직한 선언

`network` 가 여는 호스트 IPC 는 `webhook.register` 하나다. plugin 은 이 권한으로 인바운드 웹훅을 등록하되 인라인 `sequence` 는 쓸 수 없고 자기 소유(`<plugin_id>/…`) hook 핸들러 id 만 바인딩할 수 있다 — 임의 시퀀스 정의는 owner(Local) 전용 채널로 남는다.

그와 별개로 이 토큰은 **호스트가 강제할 수 없는 네트워크 사용을 사용자에게 알리는 선언**으로도 쓴다. 권한 게이트는 호스트 IPC 호출만 막으므로, plugin 프로세스가 자기 소켓을 여는 것은 어느 토큰으로도 통제되지 않는다. 그래도 매니페스트에 `network` 를 적으면 사용자가 **grant 시점에** 그 사실을 본다. 번들 plugin agent-stream 이 이 용법이다 — SSE 엔드포인트가 자기 프로세스에서 TCP 포트를 열고(`agent_stream.serve`), 노출 정책(loopback 기본 · 광역 bind 시 토큰 필수)은 호스트가 아니라 그 plugin 이 스스로 지키는 규약이다([ADR-0100](../adr/0100-agent-stream-sse-endpoint-exposure.md)). `fs.read` / `fs.write` 를 직접 파일 접근에 대해 정직하게 선언하는 것과 같은 관례다.

즉 `network` 는 두 의미를 겸한다: **호스트가 강제하는 것**(`webhook.register` 호출 자격)과 **호스트가 강제하지 못해 선언으로만 남는 것**(plugin 프로세스의 소켓). 표의 가운데 열은 전자만 센다.

## Scope 의 출처 — 동적 이름공간

`ipc.invoke:<X>` 의 `X` 는 호스트 enum 이 아니다. 각 플러그인이 `[[contributes.ipc_namespace]]` 로 prefix 를 선언함으로써 그 scope 이름이 시스템에 존재하기 시작한다. 호스트는 매니페스트 parse 시 **형식만 검증**하고 **owner 존재는 검증하지 않는다**:

| 검증한다 | 검증 안 한다 |
|----------|--------------|
| 형식 valid / 예약어 아님 | 그 namespace 를 점유한 플러그인이 설치/활성/running 인가 |

owner 미검증의 이유 — **install 순서 무관성**(B 가 A 보다 늦게 깔려도 A 매니페스트가 거부되면 안 됨), **disable/enable 견고성**, dangling 호출은 runtime 에 `-32601 method not found` 로 **명확히** 실패. 같은 prefix 는 두 플러그인이 동시에 점유 불가(두 번째 install 거부) — 임의 시점에 scope 는 정확히 한 플러그인에 귀속 또는 무소속.

**자기 namespace `ipc.invoke:<self>` 는 무용**(self-loop 를 `-32001` 로 차단) — 매니페스트에 두지 않는다.

## 새 IPC 메서드 추가 절차

핸들러를 추가하면 **반드시** `method_meta` 에 매핑 등록:

```rust
"surface.my_new_method" => plugin(&[Permission::SurfaceWrite]),
```

누락 메서드는 `method_meta` 가 `None` → plugin 호출 시 자동 `UnknownMethod` 거부(Local 은 fallthrough 통과). debug/호스트 자체 메서드(`plugin.*`/`window.*`)는 `local_only()`.

**권한은 "무엇을 실제로 건드리는가"로 정한다** — Surface 트리를 건드리면 `Surface*` 가 섞이고, 순수 PTY IO 만 하면 `Terminal*` 만 쓴다. headless PTY primitive(`pty.*`, [ADR-0050](../adr/0050-headless-pty-primitive.md))가 이 규칙의 예시다 — 새 `Pty*` 토큰 없이 기존 `Terminal*` 3종만 재사용한다:

| 메서드 | 권한 | Surface 를 건드리는가 |
|--------|------|----------------------|
| `pty.spawn` | `TerminalSpawn` | 아니오 (Surface 없이 PTY 만) |
| `pty.write` / `pty.kill` | `TerminalWrite` | 아니오 |
| `pty.read` / `pty.wait` / `pty.list` | `TerminalRead` | 아니오 |
| `pty.attach_surface` | `SurfaceWrite, TerminalSpawn` | **예** (실제 Tab 생성 — `terminal.spawn` 과 동일 이유로 `SurfaceWrite` 추가) |

## 새 권한 토큰 추가

1. `Permission` enum 에 variant 추가(scoped 면 `<Name>(String)`).
2. `from_token`/`as_token` 매핑(scoped 면 `strip_prefix` + scope 검증 함수).
3. `is_valid_<x>` 검증 함수 — **형식만**, owner 존재는 검증 안 함.
4. runtime 게이트(`method_meta` 또는 manager) 배선.
5. 이 문서의 [토큰 전체](#토큰-전체--무엇을-여나) 표 + [concepts/plugins](../concepts/plugins.md#권한-permissions) 나열 갱신 — `tests/permission_token_docs_parity.rs` 가 둘 다 CI 에서 강제한다.

`ipc.invoke`/`ext` 두 사례가 reference.

## contributes 권한 게이트

IPC 외에 일부 contribute 는 권한을 강제(매니페스트 로드 단계 거부):

이 표는 위 [토큰 전체](#토큰-전체--무엇을-여나) 와 **방향이 반대인 색인**이다 — 위는 `토큰 → 여는 것`,
여기는 `contributes 항목 → 요구 토큰`. 둘 다 `crates/tasty-plugin-manifest/src/validate.rs` 의 같은
게이트를 가리키므로 한쪽만 갱신하면 어긋난다.

| contributes | 요구 권한 |
|-------------|-----------|
| `[[contributes.tool]]` | `ui.tool_item` |
| `[[contributes.popup]]` | `ui.popup` |
| `[[contributes.commands]]` (`action.kind = "open_popup"`) | `ui.popup` |
| `[[contributes.banner]]` | `ui.banner` |
| `[[contributes.settings_pages]]` | `ui.settings_page` (카테고리 무관) |
| `[[contributes.window]]` | `window.spawn` |
| `[extends]` | `ext:<target>` |
| `[[contributes.detector]]` (신규 id) | `file_handler.define` |
| `[[contributes.detector]]` (기존 id 재선언) | `file_handler.extend:<id>` |
| `[[contributes.handler]]` | `file_handler.handle:<detector>` |
| `[[contributes.hook_handler]]` | `hook_handler.define` |
| `[[contributes.completion_strategy]]` | `completion_strategy.define` |

`event_subscribe` 는 별도 권한 없음 — 패턴 자체가 게이트.

## Builtin 자동 grant

번들 플러그인 매니페스트 권한은 `install_builtins_if_needed` 가 자동 grant — 최초는 전체, 기존 사용자에 새 버전이 토큰을 추가하면 `apply_builtin_permission_diff` 가 **신규 토큰만 증분** grant(기존 deny 보존).

## 권한 변경 즉시 반영

grant/revoke → `plugins.toml` 저장 → `refresh_plugin_permissions` 가 (매니페스트 ∩ granted)를 새 `Arc<HashSet>` 로 교체. `CallerContext::Plugin` 이 호출 시점에 `Arc::clone` 을 쥐므로 호출 도중 갱신돼도 일관(snapshot semantics).

## Agent caller — session token + temp grants

`claude.spawn` 같은 호스트-launched 자식은 `session.issue` 로 64-char hex 토큰을 받아 `TASTY_SESSION_TOKEN` 으로 전달, IPC envelope 의 `session_token` 으로 첨부 → `CallerContext::Agent`. invalid/expired/revoked 는 `-32001` 즉시 거부(Local fallback 안 함 — 환경변수 위조 방어).

- **base_permissions**: `session.issue` 시점 고정. caller 권한의 부분집합만(escalation 방지).
- **temp_grants**: runtime `plugin.grant_agent_permission` 추가, 만료 lazy evict. `effective = base ∪ non-expired temp`.

**Capability elevation 자동 발행**: Agent 가 `MissingPermission` 으로 거부되면 `approval.request{kind:capability_elevation}` 발행(같은 (agent,permission) Pending 은 approval_id 재사용). `approve`(TTL grant) / `approve_permanently`(무기한) / `deny`.

## Audit log

`handle_with_caller` 가 allow/deny 양쪽에서 `audit::record` → `tasty.audit.{ts}.{seq}` Global scope(기본 30일, lazy evict). `plugin.audit_query/summary/follow/clear` 로 조회.

## 한계

권한 게이트는 **호스트 IPC 호출만** 막는다. 플러그인이 자기 프로세스에서 `std::fs::write` 로 임의 경로에 쓰면 호스트는 모른다 — 진짜 격리는 OS 샌드박스(seccomp/sandbox-exec/WASM)가 필요하고 현재 범위 밖. 즉 매니페스트 `permissions[]` 는 **"호스트 API 호출 권한"** 이지 "OS 자원 권한"이 아니다 — UI/문서에서 grant 요청 시 이 표현을 유지해 false security 를 만들지 않는다.

**이 문서의 권한 모델과 attach 의 신뢰 모델은 서로 다른 축이라 섞지 않는다.** 이 문서는 "플러그인이 호스트 IPC 를 호출할 수 있는가"만 통제한다. 플러그인이 그린 화면(렌더 결과)이 attach 로 원격에 얼마나 노출되는지는 이 권한 모델과 무관하게 **SSH+loopback 연결 경계**([ADR-0004](../adr/0004-ipc-transport-tcp.md), [attach-behavior "IPC 표면"](attach-behavior.md#ipc-표면-attach))에 이미 위임돼 있다 — attach 로 새 콘텐츠(예: 플러그인 렌더)를 노출하는 기능을 설계할 때, "더 민감해 보이니 이 권한모델에 신규 토큰을 추가해야 한다"고 판단하지 않는다. SSH 접속 권한은 이미 그 이상(임의 파일 접근 등)을 허용하기 때문이다.

**구현 사례 — mesh mirror**: bundled egui-mesh surface(image/mesh_demo — markdown 은 [ADR-0065](../adr/0065-markdown-webview-render-channel.md) 로 webview 전환되어 이 채널 대상에서 제외됨)의 attach mirror([attach-behavior "mesh mirror 채널"](attach-behavior.md#mesh-mirror-채널), [egui-mesh-channel "attach mesh mirror 소비 경로"](egui-mesh-channel.md#attach-mesh-mirror-소비-경로))는 위 원칙을 그대로 따른 결과다 — 렌더 콘텐츠를 원격으로 흘려보내는 새 채널(`StreamControl::MeshContext`/`MeshInput`/`MeshFullResendRequest`/`MeshError`)을 추가하면서도 이 문서의 `Permission`/`method_meta`엔 어떤 신규 토큰도 추가하지 않았다. 노출 범위 통제는 오직 **①** 기존 화이트리스트(`is_egui_mesh_allowed` — 이 채널 자체의 개방 정책, plugin permission 과 무관)를 서버가 attach 트리 직렬화 시점에 재검증하는 것과 **②** attach 의 holder 점유 모델(hard 점유 = 입력 forward 수신 자격, `CoreState::apply_attached_mesh_input` 의 holder 검증)뿐이다 — "화면을 그리는 콘텐츠니 더 민감하다"는 이유로 별도 `Permission::AttachMeshMirror` 류 토큰을 만들지 않았다.
</content>
