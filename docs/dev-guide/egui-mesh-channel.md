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
          ▼  PluginEvent::PaintFrame { surface_id, buffer_id, generation,
          │                            frame_seq, full_textures }
[host]  frame_seq 체인 검증(아래 "텍스처 상태 수명 + delta 체인") 통과 시
          SharedBuffer Acquire-load → decode_paint → (ClippedPrimitive, TexturesDelta, ppp)
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

## plugin self-repaint (out-of-band 상태 변경)

위 4개 트리거(크기/ppp · 입력 · 테마 · bootstrap)는 전부 **host-side** 요인이라, plugin 이
IPC 메서드나 파일 변경 등으로 **자기 상태를 out-of-band 로 바꿔도** host 는 새 set_context 를
보내지 않는다. 이 경우 plugin 은 SDK 의 `EguiMeshSurface::repaint_last`(popup/banner 동형)로
스스로 재-paint 한다:

- `EguiMeshCore` 가 마지막 set_context 의 **geom/ppp/theme** 를 캐시한다(입력은 저장 안 함).
- `repaint_last(&host, run_ui)` 는 캐시된 geom/ppp 로 **빈 raw_input** 재-run → 출력이 바뀌면
  `PaintFrame`(popup/banner 는 각자 `*PaintFrame`) 을 송신한다. host 의 기존 wake·재합성
  경로가 이 프레임에 깨어난다(1-hop, 재-forward 왕복 불필요).
- 첫 set_context 도착 전(캐시 없음)이면 no-op, 출력 무변화면 `last_hash` dedup 으로 생략.

**identity 불변식**: 재-paint 는 `RawInputWire::default()`(events 빈 배열)로만 재현한다 —
`set_context.raw_input` 에 가짜 사용자 입력을 주입하지 않는다. 캐시된 theme 은
`last_theme()` 로 노출돼 plugin 이 draw closure 를 같은 토큰으로 재구성한다.

소비자 예: image(`image.next`/`prev`/`paste`/`save` IPC 뒤), markdown(`markdown.reload` IPC 뒤).
git-viewer 는 모든 상태 변경이 egui draw closure 내 사용자 클릭에서 일어나(in-band) 이 경로가
필요 없다. markdown 의 mtime 아이들 auto-reload(입력 없이 파일 변경)는 plugin 에 주기 tick 이
없어 별도 과제다 — `poll_reload` 는 paint 시점(입력 발생 시)에만 돈다.

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

## 텍스처 상태 수명 + delta 체인

wire 의 `textures_delta` 는 **증분**(full atlas 는 Context 첫 run 에만)이고 SharedBuffer 는
latest-wins 라, host 가 중간 frame 을 못 보면 그 frame 의 텍스처 delta(font atlas, image
비트맵 등)가 유실된다. 두 겹으로 막는다:

1. **surface 수명 귀속** — 전용 Renderer/디코드 캐시는 "보이는 동안"이 아니라 **layout 에
   존재하는 동안**(전 workspace, 비활성 탭 포함) 유지한다
   (`AppState::egui_mesh_surfaces_existing`). 비가시 surface 의 도착 frame 도 매 tick
   디코드해 delta 체인을 유지한다(합성만 skip) — 탭/workspace 전환 후 복귀 시 재전송
   왕복 없이 즉시 정상 합성된다. 비가시 GPU 텍스처 상주는 의도된 비용이다.
2. **frame_seq 체인 검증 + full 재전송** — plugin SDK 는 송신 frame 마다 단조 시퀀스
   `frame_seq`(buffer 재생성과 무관)를 `PaintFrame` 메타에 싣는다. host 는
   `frame_seq == last + 1` 이 아니면(관측 누락) frame 을 **수락하지 않고** 다음
   set_context 에 `need_full_textures` 를 실어 보낸다. SDK 는 자기가 보낸 텍스처 상태를
   누적 보관(`EguiMeshCore::tex_state` — full 교체 / patch 합성 / free 제거)하다가, 이
   요청에 dedup 을 우회하고 **전체 텍스처 상태를 full image 로 동봉**한 frame 을
   `full_textures = true` 로 재송신한다. host 는 full frame 을 체인과 무관하게 수락하고
   자기 텍스처 상태를 리셋한다(full 미포함 텍스처는 free). Context 생성 직후 첫 frame 도
   자연-full 로 마킹돼, bootstrap 직후 gen1 이 덮여도(생성 race) 같은 경로로 회복된다.

요청 플래그의 흐름: 렌더 prepare 가 체인 단절을 감지하면 `GpuState` 의 요청 대기열에
적재 → redraw 가 drain 해 surface 는 forward 추적 상태(`MeshForwardState::pending_full`)에,
popup/banner 는 `AppState` 의 `plugin_mesh_{popup,banner}_full_requests` 에 옮김 → 다음
tick 의 forward 가 `need_full_textures` set_context 를 송신(비가시 surface 는 마지막
geom/theme 으로 송신). popup/banner 도 같은 체인 규칙을 쓴다.

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

### bootstrap 은 1회만 (불필요 paint 억제)

set_context 는 **geom 변경 · 입력 · bootstrap(미paint)** 일 때만 보낸다. 특히 bootstrap 은
1회만 — paint frame 도착 전 매 frame 스팸하면 plugin 이 불필요하게 여러 번 paint 한다.
1회 보내고 frame 을 기다린다(surface `bootstrap_sent` 와 동형; frame 이 보이면 해제돼
crash 후 재bootstrap). 스팸으로 첫 frame(full atlas)이 덮여도 이제는 frame_seq 체인
검증이 감지해 full 재전송으로 회복되지만(위 "텍스처 상태 수명 + delta 체인" — popup 도
동일 규칙), 회복 왕복 자체가 낭비이므로 1회 원칙은 유지한다.

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

> egui-mesh 는 plugin popup 콘텐츠의 **유일한 렌더 채널**이다 — git-viewer /
> clipboard-viewer 가 이 채널로 자가 렌더하며, 옛 UiNode popup 렌더 경로는 존재하지
> 않는다 (`PopupRendering` 은 `egui-mesh` 단일 variant).

## egui-mesh banner 채널 (A3)

surface·popup 에 이어 **plugin banner 콘텐츠도 egui-mesh 로 자가 렌더**할 수 있다. popup
채널을 형(型)으로 하되 banner 의 non-modal 공지 성질과 생명주기 차이를 반영한다. banner
전반의 정체성·위치·큐·TTL 규칙은 [`docs/design/systems/banner.md`](../design/systems/banner.md).

### chrome 소유 경계 — host 가 셸·생명주기, plugin 은 내용만

banner chrome(컨테이너/border/close X/카운트다운/그림자)과 **스택(스코프당 표시 1 + 큐 5)·
위치(스코프 콘텐츠 최상단, 탭바 아래)·z-order·dismiss 타이밍**은 전부 host 소유다. plugin 은
content_rect 안 content 만 mesh 로 그린다(popup 과 동일한 identity 경계). 핵심 설계(D2):
plugin banner 는 host `BannerManager` 의 **같은 큐/TTL/z-order 단일 지점**을 그대로 타고,
`BannerState.content` 만 `BannerContentSource::{Host, PluginMesh{..}}` 로 분기한다 — 별도
lane 을 두지 않아 생명주기 정책이 이중화되지 않는다. 동적 plugin 인스턴스는 `BannerKey`
(`Host(&'static str)` / `Plugin(instance_id)`)로 키잉해 정적 host 배너와 한 큐에서 공존한다.

### popup 대비 차이

- **non-modal**: scrim 없음, 키보드 포커스 없음. `banner.set_context` 는 content 영역 위
  **포인터/스크롤만** forward 하고 `focused=false` (키/텍스트 없음).
- **dismiss**: outside-click/Esc 없음. TTL 자동 소멸 + host 셸 우상단 close X + plugin 의
  `banner.close` IPC. persistent(TTL 없음)도 close X 는 항상 노출(무한 배너 금지).
- **높이 고정**: 너비는 스코프 폭 도킹, 높이는 manifest `size_hint.height`(없으면 기본값)
  로 host 가 셸을 고정하고 그 안을 content_rect 로 예약한다.

### 데이터 흐름 (popup 대비 차이만)

```text
[host]  banner.open { banner_id, instance_id, surface_id }   (open_banner_instance)
[host]  banner.set_context { instance_id, width_px, height_px, ppp, raw_input, theme }
          │  draw_plugin_banners(src/plugin_bridge/banner_render.rs)가 BannerManager 가
          │  그린 content_rect 슬롯을 받아 forward. geom/입력/theme 변경 + bootstrap 시만.
          ▼
[plugin] EguiMeshBanner::paint(&ctx.host, &ctx.params, |ctx| { ... })
          ▼  PluginEvent::BannerPaintFrame { instance_id, buffer_id, generation }
[host]  banner_mesh_frames[instance_id] 갱신 → render_egui_mesh_banners 가 host egui pass
        *후* content_rect 에 mesh 를 합성(셸/affordance 위에 clip).
```

TTL 만료·close X 는 `BannerManager` 가 감지해 `closed_plugin_banners` 로 실어내고,
`draw_plugin_banners` 가 `state.plugin_banner_closes` 에 적재 → 메인 루프가
`close_banner_instance`(→ `banner.closed`)로 전파한다. plugin 이 죽어 mgr 에서 사라진 배너는
`draw_plugin_banners` 가 host UI 에서 제거(양방향 reconcile).

### 개방 정책 · scope 소유권 (D1)

surface/popup 과 동일하게 bundled 전용 + api_version 게이트다. 트리거는 phase1 **ipc 전용**
(`banner.open`, 권한 `ui.banner`). scope 는 `surface` 만 — host 는 그 plugin 이 **소유한**
surface 에만 배너를 허용한다(`open_plugin_banner` 가 surface→plugin 매핑으로 검증). 단일
인스턴스 가드로 같은 `(plugin_id, banner_id)` 는 하나만 연다.

### 구성 요소

| 조각 | 위치 |
|------|------|
| 프로토콜 (banner.set_context / BannerPaintFrame / open·closed) | `crates/tasty-plugin-protocol/src/protocol.rs` |
| manifest banner 기여 | `crates/tasty-plugin-manifest/src/types.rs` (`BannerContribute` / `BannerRendering`) |
| plugin SDK 헬퍼 | `crates/tasty-plugin-sdk/src/egui_surface.rs` (`EguiMeshBanner`) |
| host 라우팅 (banner_mesh_frames / set_context 송신) | `crates/tasty-host-plugin/src/manager/{pump,events,buffer,banner}.rs` |
| host 셸·큐·content 분기 | `src/adapters/ui/banner.rs` (`BannerContentSource` / `BannerKey`) |
| host 입력 forward + 영역 수집 + reconcile | `src/plugin_bridge/banner_render.rs` |
| host open/close 오케스트레이션 (D1 검증) | `src/app/dispatch/plugin_banner.rs` |
| host 합성 (decode + 전용 Renderer) | `src/gfx/gpu/egui_mesh_prepare.rs` (`render_egui_mesh_banners`) |
| PoC 소비자 | `crates/tasty-plugin-mesh-demo/` (`banner_id = "status"`) |

### plugin 작성

`tasty-plugin.toml` 의 `[[contributes.banner]]` 에 `rendering = "egui-mesh"` + `scope =
"surface"` 를 선언하고(`permissions` 에 `ui.banner`), `Plugin::paint_banner` 에서
`EguiMeshBanner::paint(&ctx.host, &ctx.params, |egui_ctx| { ... })` 를 호출한다.
`open_banner`/`on_banner_closed` 로 인스턴스별 상태를 초기화/정리한다. plugin 은
`banner.open { banner_id, surface_id }`(자기 소유 surface) 로 띄우고 `banner.close
{ instance_id }` 로 닫는다. 최소 예시는 `crates/tasty-plugin-mesh-demo/src/main.rs` 의
`draw_banner`. debug 검증은 `debug.plugin_banner.open/close`.

> 현 단계는 채널 **인프라 + 검증용 더미 PoC banner** 까지다. 실제 소비자 전환은 별도 작업.
