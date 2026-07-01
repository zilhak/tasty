# egui-mesh 렌더 채널

plugin 이 **자기 프로세스에서 egui 를 tessellate** 한 vector mesh 를 host 가
전용 `egui_wgpu::Renderer` 로 surface 영역에 합성하는 채널. 결정·대안·재검토 조건은
[ADR-0028](../adr/0028-plugin-egui-mesh-render-channel.md) (Accepted). 이 문서는
**현재 동작 상태**만 기술한다.

## 데이터 흐름

```
[host]  surface.set_context { surface_id, width_px, height_px, ppp, raw_input, theme }
          │  (크기/ppp 변경 · 사용자 입력 · 테마 변경 · 첫 bootstrap 시에만)
          ▼
[plugin] egui::Context::run(raw_input) → tessellate(ppp) → POD 인코드
          │  SharedBuffer 에 write + commit(footer generation)
          ▼  PluginEvent::PaintFrame { surface_id, buffer_id, generation }
[host]  SharedBuffer Acquire-load → decode_paint → (ClippedPrimitive, TexturesDelta, ppp)
          → 전용 egui_wgpu::Renderer 에 update_texture/update_buffers/render
          → surface 물리 rect 으로 평행이동 + scissor 합성
```

epaint 의 `serde` feature 가 꺼져 있어 paint 타입은 JSON 직렬화가 불가하다. 그래서
`Vertex`/`indices`(이미 `bytemuck::Pod`)는 바이트 직카피하고 나머지는 수동 POD 로
인코딩한다 (코덱: `mesh_wire`). `Primitive::Callback` 은 미지원(skip).

## 구성 요소 (현재 코드 맵)

| 역할 | 위치 |
|---|---|
| POD 와이어 코덱 (encode/decode_paint) | `crates/tasty-plugin-protocol/src/mesh_wire.rs` |
| 프로토콜 타입 (set_context / PaintFrame / RawInputWire) | `crates/tasty-plugin-protocol/src/protocol.rs` |
| plugin SDK 헬퍼 (run_frame / paint / encode, 폰트 atlas 소유) | `crates/tasty-plugin-sdk/src/egui_surface.rs` (`egui-mesh` feature) |
| host 합성 (decode + 전용 Renderer + scissor) | `src/gfx/gpu/egui_mesh_prepare.rs` (`gpu.rs::render` 가 호출) |
| host→plugin set_context + 입력 forward | `src/view/main/egui_mesh.rs` |
| paint_frame 수신 라우팅 / 송신 헬퍼 | `crates/tasty-host-plugin/src/manager/{pump,events,buffer}.rs` |
| host 측 surface stand-in | `src/plugin_bridge/egui_mesh_surface.rs` |
| 화이트리스트 + api_version gate + registry 등록 | `src/engine/surface_registry/egui_mesh.rs` |
| PoC 소비자 | `crates/tasty-plugin-mesh-demo/` |

## set_context 송신 정책

정적 화면을 매 frame 무조건 보내지 않는다. surface 마다 마지막 (크기, ppp, theme) 를
추적해 **크기/ppp 변경 · 누적 입력 · 테마 변경 · 미paint(bootstrap)** 중 하나일 때만
보낸다. plugin 이 paint 를 보낸 뒤(=`egui_mesh_frame` 존재)엔 보내지 않고, crash 로
frame 이 사라지면 다시 bootstrap 한다.

## Theme 스냅샷 (generic parity)

`set_context.theme` 는 host 가 resolve 한 현재 Theme 의 POD 스냅샷(`ThemeWire` =
색 집합 `ThemeColors` + `is_light` + UI zoom)이다. egui 의존이 없어 default 빌드에도
포함된다. plugin 은 `Theme::with_colors_and_zoom` 으로 host 와 동일한 `Theme` 인스턴스를
재구성해 디자인 토큰대로 그린다(sizing 은 zoom 으로 재도출). 모든 egui-mesh surface 가
공유하는 generic 필드다 — markdown/git-viewer 등이 같은 경로로 Theme parity 를 얻는다.
테마 변경은 위 송신 정책의 트리거이므로, 사용자가 테마를 바꾸면 입력이 없어도 재forward 된다.

## 콘텐츠 전달 (surface.create bootstrap)

egui-mesh surface 는 plugin 이 콘텐츠를 소유하므로(예: markdown 의 파일 경로), host 는
**첫 set_context bootstrap 직전에 `surface.create{params}` 를 plugin 에 1회 보낸다**
(`MainView::forward_egui_mesh_context` → `send_egui_mesh_surface_create`). 같은 plugin
req 채널 FIFO 라 create 가 set_context 보다 먼저 도착해, plugin 이 생성 params 를 렌더 전에
받는다(set_context-before-create 레이스 제거). host 측 `EguiMeshSurface` stand-in 은
`file` 을 보관해 layout 영속화(snapshot→restore)에서 같은 params 를 재전달한다.

## TextureId 격리 / ppp 가드

egui-mesh surface 마다 **독립 `egui_wgpu::Renderer`** 를 둬, plugin 의
`TextureId::Managed(0)`(폰트 atlas)가 host 폰트와 충돌하지 않게 한다(remap 불필요).
디코드된 ppp 가 host 의 현재 ppp 와 어긋나면(리사이즈/DPI 전환 직후 stale) 그 frame
합성을 미뤄 잘못된 스케일 합성을 막는다.

## 입력 forward · identity 경계

host 가 받은 **실제 사용자 입력**(클릭/스크롤/포인터 이동)만 surface-local 좌표로
변환해 `set_context.raw_input` 으로 forward 한다. set_context 송신 자체는 host 렌더
파이프라인의 일부라 사용자 상태(focus/스크롤/선택)에 부수효과가 없다. 에이전트
IPC/CLI 가 raw_input 을 합성·주입하는 진입로는 **release 에 없다** — 입력 주입은
`#[cfg(debug_assertions)]` debug 격리(`debug.inject_window_mouse`)로만 존재한다
(불가침 원칙 1·3, [debug-ipc](debug-ipc.md)).

## crash 격리

plugin 프로세스가 죽으면(reader 스레드 종료 → event_rx Disconnected) host 는 그
plugin 의 `egui_mesh_frames` 를 즉시 비운다. 합성기는 frame 이 없으면 skip 하므로
surface 가 곧장 blank 로 전환되어 마지막 mesh 가 stale 합성되지 않는다. host 는 죽지
않으며, 60초 healthcheck 가 plugin 을 재시작하면 bootstrap set_context 로 재합성된다.

## 개방 정책

bundled 전용. `(kind, plugin_id)` 화이트리스트 + plugin `api_version` 이 호스트와 일치할
때만 등록된다 (epaint 와이어가 host·plugin 동일 컴파일을 강제하는 동안의 보호). 현재
허용: `(markdown, com.tasty.markdown)`, `(mesh_demo, com.tasty.mesh-demo)`.

## plugin 작성

`tasty-plugin.toml` 의 `[[surface_kinds]]` 에 `rendering = "egui-mesh"` 를 선언하고,
SDK 를 `features = ["egui-mesh"]` 로 받아 `EguiMeshSurface::paint(&ctx.host,
&ctx.params, |egui_ctx| { ... })` 를 `Plugin::paint_surface` 에서 호출하면 된다.
코덱/송신은 SDK 가 은닉한다. 최소 예시는 `crates/tasty-plugin-mesh-demo/src/main.rs`.

## egui-mesh popup 채널 (A2)

surface 뿐 아니라 **plugin popup 콘텐츠도 egui-mesh 로 자가 렌더**할 수 있다. surface
채널을 재사용하되 popup 의 셸/생명주기 차이를 반영한다.

### chrome 소유 경계 — host 가 셸, plugin 은 내용만

팝업 셸(scrim / bg_panel / border / outside-click / Esc / 단일 인스턴스 가드 / 크기)은
**host 가 소유**하고, plugin 은 **콘텐츠만 mesh 로** 그린다. 근거: identity 원칙 1·3 —
팝업 open/close/focus 같은 *사용자 조작*은 host 소유다(에이전트/plugin 이 사용자 상태를
좌우하지 않는다). host 는 `draw_plugin_popups`(`src/plugin_bridge/popup_render.rs`)에서
셸을 egui 로 그리고, plugin mesh 는 셸 내부 content_rect 에 **host egui pass 후** 합성한다
(mesh 는 content_rect 로 clip 되어 border/close 를 덮지 않는다).

### 데이터 흐름 (surface 대비 차이만)

```
[host]  popup.set_context { instance_id, width_px, height_px, ppp, raw_input }
          │  surface 와 달리 surface_id 대신 host 발급 instance_id 로 키잉
          │  raw_input 은 draw_plugin_popups 가 ctx.input 의 egui 이벤트를 content-local
          │  좌표로 변환(콘텐츠 영역 안의 포인터 + 키/텍스트만) — host 가 받은 실제 입력만
          ▼
[plugin] EguiMeshPopup::paint(&ctx.host, &ctx.params, |ctx| { ... })
          │  SDK 헬퍼가 surface 와 동일한 코덱/버퍼 로직 공유(EguiMeshCore)
          ▼  PluginEvent::PopupPaintFrame { instance_id, buffer_id, generation }
[host]  popup_mesh_frames[instance_id] 갱신 → render_egui_mesh_popups 가 instance_id 로
          lookup → decode → 전용 egui_wgpu::Renderer 로 content_rect 에 합성
```

### bootstrap 은 1회만 (폰트 atlas 보존)

set_context 는 **geom 변경 · 입력 · bootstrap(미paint)** 일 때만 보낸다. 특히 bootstrap 은
1회만 — paint frame 도착 전 매 frame 스팸하면 plugin 이 여러 번 paint 하고, egui 는 폰트
atlas delta 를 **첫 frame 에만** 실어 보내므로 host 가 최신 frame 만 보관해 atlas 를 못 받아
`Missing texture: Managed(0)` 로 텍스트가 사라진다. 1회 보내고 frame 을 기다린다(surface
`bootstrap_sent` 와 동형; frame 이 보이면 해제돼 crash 후 재bootstrap).

### 개방 정책

surface 와 동일하게 bundled 전용이다 — egui-mesh popup(`rendering = "egui-mesh"`)은
plugin `api_version` 이 호스트와 일치할 때만 열린다(`open_popup_instance` 게이트). 단일
인스턴스 가드로 같은 `(plugin_id, popup_id)` 는 하나만 연다(중복 open 은 기존 instance_id
반환).

### 구성 요소

| 역할 | 위치 |
|---|---|
| 프로토콜 (popup.set_context / PopupPaintFrame) | `crates/tasty-plugin-protocol/src/protocol.rs` |
| manifest popup rendering 필드 | `crates/tasty-plugin-manifest/src/types.rs` (`PopupRendering`) |
| plugin SDK 헬퍼 | `crates/tasty-plugin-sdk/src/egui_surface.rs` (`EguiMeshPopup`) |
| host 라우팅 (popup_mesh_frames / set_context 송신) | `crates/tasty-host-plugin/src/manager/{pump,events,buffer,popup}.rs` |
| host 셸 + 입력 forward + 영역 수집 | `src/plugin_bridge/popup_render.rs` |
| host 합성 (decode + 전용 Renderer) | `src/gfx/gpu/egui_mesh_prepare.rs` (`render_egui_mesh_popups`) |
| PoC 소비자 | `crates/tasty-plugin-mesh-demo/` (`popup_id = "demo"`) |

### plugin 작성

`tasty-plugin.toml` 의 `[[contributes.popup]]` 에 `rendering = "egui-mesh"` 를 선언하고
(`permissions` 에 `ui.popup`), `Plugin::paint_popup` 에서 `EguiMeshPopup::paint(&ctx.host,
&ctx.params, |egui_ctx| { ... })` 를 호출한다. `open_popup`/`on_popup_closed` 로
인스턴스별 상태를 초기화/정리한다. 최소 예시는 `crates/tasty-plugin-mesh-demo/src/main.rs`
의 `draw_popup`.

> 현 단계는 채널 **인프라 + 검증용 더미 PoC popup** 까지다. UiNode popup 렌더 경로는
> 공존 유지된다(제거는 후속). git-viewer / clipboard-viewer 의 실제 전환은 별도 작업.
