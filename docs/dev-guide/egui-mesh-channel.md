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
[host]  SharedBuffer Acquire-load → decode_paint → (ClippedPrimitive, TexturesDelta, ppp)
          → frame_seq 체인 검증(아래 "텍스처 상태 수명 + delta 체인"): delta 적용은 체인
            연속(또는 full)일 때만, mesh 채택은 참조 텍스처 상주 시 seq 불연속이어도 수행
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
| 보조 핸들 채널 (shared buffer 핸들 전송) | host `crates/tasty-host-plugin/src/handle_channel.rs` · plugin `crates/tasty-plugin-sdk/src/handle_channel.rs` · 매핑 `crates/tasty-shm/` |
| host 측 surface stand-in | `src/plugin_bridge/egui_mesh_surface.rs` |
| 화이트리스트 + api_version gate + registry 등록 | `src/engine/surface_registry/egui_mesh.rs` |
| PoC 소비자 | `crates/tasty-plugin-mesh-demo/` |

### 보조 핸들 채널 — shared buffer 를 plugin 에 넘기는 전송 (크로스플랫폼)

mesh 는 GPU shared memory 버퍼에 써서 host 가 합성한다. 그 버퍼를 만드는 건 host 지만
(`manager/buffer.rs::create_shared_buffer_for`), 매핑 핸들을 plugin 프로세스로 넘기는 건
메인 TCP 채널이 아니라 **보조 핸들 채널**이다(메인 채널은 fd/HANDLE 을 운반 못 함). OS 별
전송 수단만 다르고 상위 프로토콜(NDJSON `HandleAttach` + `Dirty`)은 동일하다:

| | Unix | Windows |
|---|---|---|
| 채널 | `AF_UNIX` socket | Named Pipe (`\\.\pipe\tasty-handle-…`, **overlapped I/O**) |
| 핸들 전달 | `sendmsg` + `SCM_RIGHTS`(ancillary fd) | `DuplicateHandle` 로 plugin 핸들 테이블에 복제 → 결과 HANDLE u64 를 `HandleAttach.handle` in-band |
| plugin 수신 | `recvmsg` fd → `tasty_shm::receive(Fd)` | 라인 파싱 HANDLE u64 → `tasty_shm::receive(Handle)` |

`HandleChannelMessage::HandleAttach` 의 `handle: Option<u64>` 는 Windows 전용이며,
`skip_serializing_if` 로 Unix wire 는 변경되지 않는다. 핸들 복제/매핑 자체는 `tasty_shm`
플랫폼 레이어가 담당한다. end-to-end 라운드트립 검증은
`crates/tasty-host-plugin/src/handle_channel/channel_tests_windows.rs`(Windows) ·
`channel_tests.rs`(Unix).

**Unix 수신측 fd 형태 검증은 OS 별로 기준이 다르다 (필수)**: `tasty_shm::receive` 는 넘겨받은
fd 를 소유권 편입 전에 `fstat` 으로 형태 검증한다. 이때 통과 기준이 Linux 와 macOS 에서
다르다 — Linux 의 memfd/shm fd 는 `S_IFMT == S_IFREG` 로 보이지만, **macOS 의 `shm_open` fd 는
파일시스템에 없는 커널 객체라 `S_IFMT == 0`** 으로 보인다(`st_size` 등 나머지 필드는 정상).
그래서 `platform/macos.rs` 는 `S_IFMT ∈ {0, S_IFREG}` 를, `platform/linux.rs` 는 `S_IFREG` 를
요구한다. 한쪽 조건을 다른 쪽에 그대로 복사하면 **정상 fd 가 전량 거부되어 mesh paint 가
100% 실패**하고, 그 결과는 에러 없는 빈 surface 로만 나타난다. 회귀는
`crates/tasty-shm/tests/round_trip.rs` 가 잡지만, 이 테스트는 **각 OS 에서 실제로 실행되어야**
의미가 있다 — 통합테스트라 `--lib --bins` 만 도는 CI 잡에서는 컴파일조차 되지 않는다.

**Windows overlapped I/O (필수)**: 이 채널은 full-duplex 다 — host 는 HandleAttach 를
write 하면서 동시에 reader 스레드가 plugin 의 Dirty 를 blocking read 한다. Windows 의
*동기* 파일 핸들은 같은 file object 의 I/O 를 직렬화하고 `DuplicateHandle`(try_clone)은
같은 file object 를 가리키므로, sync I/O 로 두면 reader 의 blocking `ReadFile` 이
`WriteFile` 을 막아 HandleAttach 전송이 데드락된다(→ plugin paint hang → 팝업 빈 화면).
그래서 파이프를 `FILE_FLAG_OVERLAPPED` 로 열고 per-op event overlapped I/O 로 read/write
를 비직렬화한다. 회귀 방지는 `channel_tests_windows.rs` 의
`windows_handle_channel_concurrent_read_write_no_deadlock`.

## set_context 송신 정책

정적 화면을 매 frame 무조건 보내지 않는다. surface 마다 마지막 (크기, ppp, theme,
focused) 를 추적해 **크기/ppp 변경 · 누적 입력 · 테마 변경 · 미paint(bootstrap) ·
포커스 변화 · plugin 의 `SurfaceInvalidated` 알림** 중 하나일 때만 보낸다. 포커스 변화를
트리거에 넣는 이유: egui 자체가 `ctx.input(|i| i.focused)` 로 커서 블링크·포커스 링 등
위젯 시각을 바꾸므로, 입력 없이 포커스만 잃는 경우(다른 surface 클릭)에도 재전송돼야
그 변화가 즉시 반영된다.
`SurfaceInvalidated`(`MeshForwardState::invalidated`, 단계 06)는 plugin 이 out-of-band
로 감지한 변경(예: idle 상태에서 외부 파일 수정)을 host 에 알려 **입력 없이도** 재forward
를 트리거하는 유일한 plugin-발 경로다 — 아래 "idle invalidate" 절 참조. focused 를 안 쓰는
plugin 은 출력 바이트가 불변이라 SDK 출력 해시 dedup 이 PaintFrame 을 흡수 — 스퍼리어스
재합성 없음. plugin 이 paint 를 보낸 뒤(=`egui_mesh_frame` 존재)엔 보내지 않고, crash 로
frame 이 사라지면 다시 bootstrap 한다.

## idle invalidate (SurfaceInvalidated, 단계 06)

위 5개 host-side 트리거와 별개로, plugin 은 `HostHandle::notify(&PluginEvent::SurfaceInvalidated
{ surface_id })` 로 **입력과 무관하게** 재forward 를 요청할 수 있다. host 수신 스레드는
어떤 이벤트든 라인마다 waker 를 깨우므로(`process.rs`) idle(입력 없는) 상태에서도 이
알림이 도착하면 다음 tick 에서 즉시 처리된다:

1. `PluginManager::pump()` 가 이벤트를 `invalidated_surfaces` 에 누적하고
   `take_invalidated_surfaces()` 로 드레인된다(`crates/tasty-host-plugin/src/manager/pump.rs`).
2. `App::event_handler` 가 pump 직후 이를 소비해, surface_id 가 속한 `MainView` 를 찾아
   `MeshForwardState::set_invalidated()` + `mark_dirty()`(redraw 요청)를 건다
   (`src/app/event_handler.rs`).
3. 다음 `forward_egui_mesh_context` 게이트에서 `invalidated` 플래그가 (다른 트리거 없이도)
   빈 입력 `set_context` 를 1회 통과시키고, 송신 시 플래그를 소거한다(`src/view/main/egui_mesh.rs`).
4. plugin 의 `paint_surface` 가 이 무입력 frame 을 받아 자기 상태를 재확인·재-read 한다.

**과거 소비자(현재는 다른 경로로 대체됨)**: markdown plugin 이 egui-mesh 로 본문을 그리던
시절엔 `crates/tasty-plugin-markdown/src/watch.rs` 의 idle mtime 폴링 worker 가 이
채널로 `SurfaceInvalidated` 를 emit 해 재-read 를 트리거했다. markdown 이 webview 로
전환된 뒤([ADR-0065](../adr/0065-markdown-webview-render-channel.md))로는 webview-kind
surface 가 `paint`/`set_context` 자체를 받지 않으므로 이 경로가 무의미해졌다 — 지금
`watch.rs` 는 mtime 변경 감지 시 이 채널 대신 host 를 왕복해 `markdown.reload` IPC 를
직접 호출한다. 이 문서의 이 절이 설명하는 `SurfaceInvalidated` 채널 자체는 여전히
유효한 일반 인프라이나, 현재 이를 실제로 쓰는 번들 plugin 은 없다.

### popup 대응 — `PopupInvalidated`

`SurfaceInvalidated` 는 surface 전용이다. egui-mesh popup(git-viewer/clipboard-viewer 등,
아래 "egui-mesh popup 채널")도 동일하게 무입력 재-forward 가 필요한 경우가 있어(아래
"egui 내장 애니메이션과 이벤트 기반 게이팅의 상호작용" 참조) `PluginEvent::PopupInvalidated
{ instance_id }` 를 별도로 둔다. 처리 경로는 surface 와 대칭이되 종착지만 다르다:

1. `PluginManager::pump()` 가 `invalidated_popups` 에 누적, `take_invalidated_popups()` 로 드레인.
2. `App::mark_invalidated_popups_dirty`(`src/app/event_handler.rs`, `about_to_wait()` 에서
   `mark_invalidated_surfaces_dirty` 직후 호출)가 이를 소비한다. popup instance 는 surface 와
   달리 특정 window 에 귀속되지 않으므로(단일 인스턴스 가드로 host 전체에 하나), 몇 번째
   window 가 그 popup 을 그리는지 알 수 없다 — 그래서 **전 main window** 를
   `main_windows_iter_mut()` 로 순회하며 broadcast 한다(`attach_client.rs` 의 기존
   `plugin_mesh_popup_pending_repaint` 예약 패턴과 동형).
3. 새 set_context 트리거가 아니라, popup 이 이미 갖고 있던
   `AppState::plugin_mesh_popup_pending_repaint`(ADR-0056 — 비동기 host→plugin push 후 강제
   repaint 예약)에 그대로 얹는다. `popup_render.rs` 의 forward 게이트(`need_repaint`)가 다음
   프레임에 무입력 `popup.set_context` 를 1 회 통과시킨다 — surface 의 `invalidated` 플래그와
   동일 역할을 이미 있던 필드가 겸한다(별도 상태 필드 신설 불필요).
4. banner 는 아직 이 경로가 없다 — 현재 banner egui-mesh 채널은 검증용 PoC 소비자
   (`tasty-plugin-mesh-demo`)뿐이라 실질 영향이 없고, 실제 소비자가 생기면 popup 과 동형으로
   `BannerInvalidated` + `plugin_mesh_banner_pending_repaint` 를 추가하면 된다.

## plugin self-repaint (out-of-band 상태 변경)

위 5개 트리거(크기/ppp · 입력 · 테마 · bootstrap · 포커스)는 전부 **host-side** 요인이라, plugin 이
IPC 메서드나 파일 변경 등으로 **자기 상태를 out-of-band 로 바꿔도** host 는 새 set_context 를
보내지 않는다. 이 경우 plugin 은 SDK 의 `EguiMeshSurface::repaint_last`(popup/banner 동형)로
스스로 재-paint 한다:

- `EguiMeshCore` 가 마지막 set_context 의 **geom/ppp/theme/focused** 를 캐시한다(입력
  이벤트는 저장 안 함).
- `repaint_last(&host, run_ui)` 는 캐시된 geom/ppp 로 **빈 이벤트 + 직전 focused 보존**
  재-run → 출력이 바뀌면 `PaintFrame`(popup/banner 는 각자 `*PaintFrame`) 을 송신한다.
  host 의 기존 wake·재합성 경로가 이 프레임에 깨어난다(1-hop, 재-forward 왕복 불필요).
- 첫 set_context 도착 전(캐시 없음)이면 no-op, 출력 무변화면 `last_hash` dedup 으로 생략.

**identity 불변식**: 재-paint 의 `events` 는 빈 배열 — `set_context.raw_input` 에 가짜
사용자 입력을 주입하지 않는다. `focused` 는 이벤트가 아니라 지속 상태이므로 직전
set_context 값을 그대로 재현한다(불변식 무위반) — false 로 떨어뜨리면 egui `has_focus()`
의 viewport 게이트가 꺼져 커서·드롭다운 등 포커스 의존 UI 가 재-paint 프레임에서만
퇴행한다(markdown 주소창 진동 버그의 원인이었다). 캐시된 theme 은 `last_theme()` 로
노출돼 plugin 이 draw closure 를 같은 토큰으로 재구성한다.

소비자 예: image(`image.next`/`prev`/`paste`/`save` IPC 뒤). git-viewer 는 모든 상태
변경이 egui draw closure 내 사용자 클릭에서 일어나(in-band) 이 경로가 필요 없다. (markdown
은 이 문서의 이전 리비전까지 대표 소비자였으나, [ADR-0065](../adr/0065-markdown-webview-render-channel.md)
로 본문 surface 가 webview 전환되며 egui-mesh self-repaint 경로 자체를 타지 않게 됐다 —
`markdown.reload` IPC 는 지금은 host 가 webview 를 직접 재로드하는 별개 경로다.)

## egui 내장 애니메이션과 이벤트 기반 게이팅의 상호작용

위 "set_context 송신 정책"의 이벤트 기반 게이팅(host-side 이벤트가 있을 때만 pass 를
구동)은 **egui 자신의 다중 프레임 애니메이션**(스크롤 스무딩, `ctx.request_repaint_after`
류 전반)이 매 프레임 무관하게 이어지는 pass 를 전제로 설계됐다는 사실과 충돌할 수 있다.
egui-mesh 도입 초기엔 이 결함이 방치돼 있었다 — `EguiMeshCore::render`(SDK)가
`ctx.run()` 의 반환값 `FullOutput::viewport_output`(egui 가 "다음 pass 도 그려달라"고
신호하는 채널, `repaint_delay: Duration`)을 전혀 읽지 않고 버렸다.

**증상**: 트랙패드로 스크롤(휠 드래그 제스처)하고 손을 떼면, egui 내부에
`unprocessed_scroll_delta`(egui 0.31 `input_state/mod.rs` — `Point` 단위 8pt 이상이거나
`Line`/`Page` 단위 델타는 즉시 반영되지 않고 지수완화로 여러 pass 에 걸쳐 drain 된다)가
아직 남아있어도, host 는 더 이상 host-side 이벤트가 없으므로 다음 pass 를 구동하지 않는다
— 스크롤이 입력한 양만큼 반영되지 않고 멈춘 채 방치된다. 이후 무관한 입력(마우스 이동
등)이 들어와야 host 가 다시 pass 를 구동해 남은 delta 가 그 시점에 몰아서 반영된다.

**고친 지점(두 가지, 병행)**:

1. **self-repaint 편승** (`crates/tasty-plugin-sdk/src/egui_surface.rs`) —
   `EguiMeshCore::render` 가 매 pass 마다 `full.viewport_output.get(&ViewportId::ROOT)`
   (egui-mesh 는 단일 ROOT viewport 만 씀)의 `repaint_delay` 를 읽어
   `pending_self_repaint`(`Duration::MAX` = 요청 없음 → `None`)로 캐시한다.
   `EguiMeshSurface`/`EguiMeshPopup` 의 `paint`/`repaint_last` 는 매 호출 뒤(frame 이
   `None` 이어도) 이 값을 확인해 `Some(delay)` 면 `delay` 뒤 위 "idle invalidate" 채널
   (`SurfaceInvalidated`/`PopupInvalidated`)로 host 에 재-forward 를 요청하는 타이머
   스레드를 스폰한다 — `self_repaint_armed`(`AtomicBool`) 로 중복 스레드를 막고, 타이머가
   fire 하면 풀려 다음 `render()` 가 여전히 필요하면 재-arm 한다(자연 수렴, idle 상태에서
   스레드가 폭주하지 않음). 이 채널은 host-side 코드 변경 없이(surface) 또는 이미 있던
   popup pending-repaint 필드에 편승해(popup) 동작한다 — `EguiMeshCore` 를 공유하는
   surface/popup 전체에 적용된다(banner 는 실 소비자가 아직 없어 미적용, 위 "popup 대응"
   참조).
2. **raw_input.time 보정** (`src/view/main/egui_mesh.rs`) — `forward_egui_mesh_context`
   가 surface 로 보내는 `set_context.raw_input.time` 이 과거엔 항상 `None` 이었다. egui는
   `time` 이 없으면 `predicted_dt`(1/60초 고정)로만 dt 를 추정하므로, 1번의 idle-invalidate
   재forward 처럼 실제 forward 간격이 그보다 훨씬 길어도 egui 는 매번 "짧은 프레임"으로
   착각해 스크롤 스무딩의 지수완화 계수(`exponential_smooth_factor`)가 실제보다 느리게
   수렴한다. `mesh_time_now()`(프로세스 시작 시 고정한 `Instant` 로부터의 경과 초 — 절대
   기준은 의미 없고 단조 증가만 필요) 를 채워 보내 실제 경과 시간을 반영한다. 현재는
   local surface forward 경로(가시·비가시 pending_full 재전송)에만 적용했고, attach mesh
   mirror 경로(`attach_mesh_input.rs`/`mesh_forward.rs`/`stream_hub.rs`/`mesh_mirror.rs`)와
   popup/banner forward(`popup_render.rs`/`banner_render.rs`)는 아직 `time: None` 그대로다
   — 위 1번(self-repaint)만으로 "유휴 상태 방치" 증상 자체는 해소되므로 필수는 아니었고,
   범위를 넓히면 손대는 파일이 늘어 이번 TODO 는 실제 버그 재현 경로(markdown surface)에
   한정했다. 필요해지면 같은 패턴으로 확장 가능.

**popup(git-viewer/clipboard-viewer)도 같은 결함을 안고 있었다** — `EguiMeshCore` 를
공유하므로 스크롤 가능한 popup 콘텐츠도 이론상 동일 증상을 재현할 수 있었다. 1번을 popup
에도 적용했으므로(`PopupInvalidated`, 위 "popup 대응" 절) 이 TODO 로 함께 해소된다.

## Theme 스냅샷 (generic parity)

`set_context.theme` 는 host 가 resolve 한 현재 Theme 의 POD 스냅샷(`ThemeWire` =
색 집합 `ThemeColors` + `is_light` + UI zoom)이다. egui 의존이 없어 default 빌드에도
포함된다. plugin 은 `Theme::with_colors_and_zoom` 으로 host 와 동일한 `Theme` 인스턴스를
재구성해 디자인 토큰대로 그린다(sizing 은 zoom 으로 재도출). 모든 egui-mesh surface 가
공유하는 generic 필드다 — image/git-viewer 등이 같은 경로로 Theme parity 를 얻는다
(markdown 은 [ADR-0065](../adr/0065-markdown-webview-render-channel.md) 로 본문 surface 가
webview 전환돼 이 경로 밖 — 대신 `theme.query`/`theme.changed` 를 쓴다. 대용량/파일열기
확인 팝업 2개는 여전히 이 경로를 탄다).
테마 변경은 위 송신 정책의 트리거이므로, 사용자가 테마를 바꾸면 입력이 없어도 재forward 된다.

## 콘텐츠 전달 (surface.create bootstrap)

egui-mesh surface 는 plugin 이 콘텐츠를 소유하므로(예: image 의 파일 경로), host 는
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
2. **frame_seq 체인 검증 + full 재전송 (텍스처 delta 한정)** — plugin SDK 는 송신 frame
   마다 단조 시퀀스 `frame_seq`(buffer 재생성과 무관)를 `PaintFrame` 메타에 싣는다. host
   는 `frame_seq == last + 1` 이 아니면(관측 누락) 그 frame 에 실렸던 `textures_delta` 가
   유실됐다고 보고 **delta 를 적용하지 않는다**(`chain_accepts`). 대신 다음 set_context 에
   `need_full_textures` 를 실어 보낸다. SDK 는 자기가 보낸 텍스처 상태를 누적 보관
   (`EguiMeshCore::tex_state` — full 교체 / patch 합성 / free 제거)하다가, 이 요청에 dedup
   을 우회하고 **전체 텍스처 상태를 full image 로 동봉**한 frame 을 `full_textures = true`
   로 재송신한다. host 는 full frame 을 체인과 무관하게 수락하고 자기 텍스처 상태를
   리셋한다(full 미포함 텍스처는 free). Context 생성 직후 첫 frame 도 자연-full 로 마킹돼,
   bootstrap 직후 gen1 이 덮여도(생성 race) 같은 경로로 회복된다.

   **재무장(single-shot deadlock 제거)**: 요청한 full frame 이 다시 latest-wins 버퍼에서
   유실될 수 있으므로, host 는 수락될 때까지 **매 tick full 재전송을 재요청**한다(로그는
   최초 1회만). frame 수락 시 해제되어 다음 단절 때 다시 요청·로그한다.
3. **mesh(기하) 채택은 delta 체인과 분리** — reflow frame 의 mesh 는 자기완결적 기하라
   중간 frame 유실(delta 손실)과 무관하다. 따라서 위 체인 가드는 **텍스처 delta 적용
   여부만** 막고, mesh 채택은 `decode_mesh_into_target` 이 디코드 후 `classify_decode` 로
   세 결과 중 하나로 판정한다. 입력 축은 셋: `chain_ok`(full 이거나 `frame_seq == last+1`),
   참조 상주(`all_textures_live` — 이 frame 의 mesh 가 참조하는 모든 `TextureId` 가 이미
   상주), delta 경계 정합(`deltas_fit_live` — patch delta 가 상주 텍스처 크기 안에 들어감,
   3d74217c 의 오버런 방어선).

   | 결과 | 조건 | mesh | `textures_delta` | `last_seq` | full 재요청 |
   |---|---|---|---|---|---|
   | **Accepted** | `chain_ok` + 경계 정합 | 채택 | 적용 | 전진 | 없음 |
   | **AcceptedStale** | 체인 단절(seq 점프)이나 참조 상주 + 경계 정합 | 채택 | **미적용** | **미전진** | **매 tick** |
   | **NeedsFull** | 첫 콘텐츠 없음 · 미상주 참조 · 오버런(경계 초과) | 보류 | 미적용 | 미전진 | 매 tick |

   **불변식**: delta 를 실제 적용하지 못한(체인 단절으로 스킵한) frame 의 mesh 는 채택하되
   `last_seq` 를 전진시키지 않는다(**AcceptedStale**). 그러면 다음 tick 도 체인 단절로 남아
   full 재전송이 계속 무장되고, 유실로 stale 해진 atlas 는 다음 full frame 으로 정합
   복구된다. mesh(기하)는 최신으로 갱신되므로 리사이즈/split 로 폭이 바뀌어 seq 가 튀어도
   mesh-demo 등 mesh surface 가 옛 폭에 고정(우측 잘림)되지 않고 즉시 reflow 되고, 그 사이
   글리프 uv 만 다음 full 까지 stale atlas 를 가리킨다. 참조가 하나라도 미상주(image plugin
   신규 비트맵 등)거나 delta 가 오버런이면 **NeedsFull** — mesh 도 보류하고 full 을
   재요청한다(오버런은 적용 시 egui-wgpu 가 리사이즈 없이 write 해 크래시하므로 보류가
   방어선이다). mesh-demo 처럼 텍스처가 없는 순수 위젯 surface 는 참조가 항상 폰트
   atlas(상주)라 단절 시 AcceptedStale 경로다.

요청 플래그의 흐름: 렌더 prepare 가 `NeedsFull` 또는 `AcceptedStale` 을 판정하면 `GpuState` 의
요청 대기열에 적재 → redraw 가 drain 해 surface 는 forward 추적 상태
(`MeshForwardState::pending_full`)에, popup/banner 는 `AppState` 의
`plugin_mesh_{popup,banner}_full_requests` 에 옮김 → 다음 tick 의 forward 가
`need_full_textures` set_context 를 송신(비가시 surface 는 마지막 geom/theme 으로 송신).
plugin generation 이 정지해 새 frame 이 안 와도 이미 무장된 surface 는 매 tick 재요청을
유지한다(재-tessellation·업로드 없이 IPC 메시지만). popup/banner 도 같은 체인 규칙·재무장·
mesh 분리 규칙을 공유한다.

## 입력 forward · identity 경계

host 가 받은 **실제 사용자 입력**만 surface-local 좌표로 변환해 `set_context.raw_input`
으로 forward 한다. 포인터(클릭/스크롤/이동)에 더해 **포커스된 egui-mesh surface** 는
키보드도 받는다:

| 입력 | wire 이벤트 | 누적 지점 |
|---|---|---|
| 포인터 버튼/이동/스크롤 | `PointerButton`/`PointerMoved`/`Scroll` | `egui_mesh_push_pointer_*`/`push_scroll` |
| 포인터가 surface 밖으로 나감 | `PointerGone` | `egui_mesh_push_pointer_gone`/`attach_mesh_push_pointer_gone` ← `mouse.rs` `update_mesh_hover` |
| 키 누름(press-only) | `Key { key: egui Key::name(), … }` | `egui_mesh_push_key` ← `keyboard.rs` `forward_key_to_egui_mesh` |
| 텍스트 입력 | `Text { text }` | `egui_mesh_push_text` (게이트 `should_forward_text`) |
| IME 조합(라이브 preedit + commit) | `Ime { event: ImeWire::… }` | `egui_mesh_push_ime` ← `ime.rs` `forward_ime_to_egui_mesh` |
| 복사 단축키(`egui_copy` capability 를 가진 kind 한정) | `Copy` | `egui_mesh_push_copy` ← `copy_paste.rs` `handle_copy_shortcut` |

`MainView.mesh_pointer_hover`(`Option<MeshHoverTarget>`, `Local(surface_id)`/`Attach(surface_id)`)가
마지막으로 `PointerMoved` 를 받은 mesh surface 1개를 추적한다. `handle_cursor_moved`
(egui-mesh·attach mesh mirror 판정 지점)와 `handle_cursor_left`(`WindowEvent::CursorLeft`)가
매 `CursorMoved`/`CursorLeft` 이벤트마다 `update_mesh_hover(new_target)` 를 거쳐 슬롯을
갱신하며 — `egui_consumed`/오버레이(설정창)/팝업/배너/modifier-hint hover 로 인한
early-return 경로도 포함: 이 경로에 진입한 이벤트는 mesh 판정 자체를 건너뛰고
곧장 `update_mesh_hover(None)` 을 호출한다, 즉 "이번 프레임은 mesh surface 위가
아니다"로 취급 — 대상이 바뀌면(다른 mesh surface 로 전환되거나 `None` 이 되면)
**이전** 대상에 `PointerGone` 을 1 회 forward 한다. 안 그러면 plugin 쪽 egui 가 마지막
`PointerMoved` 위치에 포인터가 계속 있다고 착각해 hover 하이라이트가 잔류할 수 있다.

키/IME 는 **포커스된 egui-mesh surface 에만** 간다(`focused_egui_mesh_surface_id`
downcast 판정). 중앙 키 디스패처(`keyboard.rs handle_keyboard_input`)가 단축키·vi·
escape 소비를 **먼저** 처리한 뒤, 소비되지 않은 키를 이 forward 로 넘긴다 — 단축키
선점 순서는 터미널 forward 와 동일하다. IME 는 조합 중 preedit 문자열까지 나르므로
(commit-only 아님) plugin 의 egui `TextEdit` 이 조합 중간 상태를 인라인 표시한다.
image/mesh_demo 는 이 forward 로 host egui 를 거치지 않으므로(`main.rs` 의 host-egui 키
피드에서 제외) host egui 가 그 키/IME 를 삼키지 않는다. (markdown 은 본문 surface 가
[ADR-0065](../adr/0065-markdown-webview-render-channel.md) 로 webview 전환되어 이
경로 밖 — 대용량/파일열기 확인 팝업 2개만 여전히 이 forward 대상이다.)

키 wire 는 egui `Key::name()` 문자열을 나르고 plugin SDK(`map_event`)가
`Key::from_name` 으로 복원한다. winit→egui `Key` 변환은 논리 키 우선·물리 키 폴백
(egui-winit 미러)이라 비-라틴 레이아웃의 편집 단축키(Ctrl+A 등)도 물리 위치로 매칭된다.
Text 게이트는 command modifier·제어/사설영역 문자·IME 조합 중 non-ASCII 를 억제한다
(조합 결과는 `Ime` `Commit` 으로 도착).

set_context 송신 자체는 host 렌더 파이프라인의 일부라 사용자 상태(focus/스크롤/선택)에
부수효과가 없다. 에이전트 IPC/CLI 가 raw_input 을 합성·주입하는 진입로는 **release 에
없다** — 입력 주입은 `#[cfg(debug_assertions)]` debug 격리(`debug.inject_window_mouse`)로만
존재한다(불가침 원칙 1·3, [debug-ipc](debug-ipc.md)).

### 알려진 한계 (IME candidate 위치)

OS IME candidate 창(조합 후보 목록)의 화면 위치는 host `update_ime_cursor_area` 가
현재 **터미널 커서** 기준으로만 설정한다 — egui-mesh surface 편집 시 후보 창이 정확한
필드 위치에 안 뜰 수 있다. 라이브 preedit **인라인 표시**(egui `TextEdit`)는 정상
동작하며, 후보 창 위치는 별도 과제다. Copy 는 위 표대로 `egui_copy` capability 를
가진 kind 한정으로 wire 에 있지만, Cut/Paste 는 아직 wire 에 없어 egui-mesh 필드에서
Ctrl+V/X 는 동작하지 않는다(popup 미러 경로와 동일 한계).

## crash 격리

plugin 프로세스가 죽으면(reader 스레드 종료 → event_rx Disconnected) host 는 그
plugin 의 `egui_mesh_frames` 를 즉시 비운다. 합성기는 frame 이 없으면 skip 하므로
surface 가 곧장 blank 로 전환되어 마지막 mesh 가 stale 합성되지 않는다. host 는 죽지
않으며, 60초 healthcheck 가 plugin 을 재시작하면 bootstrap set_context 로 재합성된다.

### 빈 surface 감지 (host 로그)

blank 전환 자체는 정상 동작이지만, **왜** 비었는지는 host 가 알 수 없다 — plugin 의 paint
실패는 plugin 자체 로그(`tasty plugin logs <id>`)에만 남고 host 에는 통지되지 않는다. host
forward 루프는 frame 이 없는 surface 를 조용히 건너뛰므로, host stderr 만 보는 사람에게는
아무 징후 없이 빈 화면만 남는다.

그래서 `view/main/egui_mesh.rs` 는 bootstrap set_context 송신 시각을 기록해두고,
`BLANK_SURFACE_GRACE`(3초)가 지나도록 frame 이 하나도 오지 않으면 surface 당 **1회**
`ERROR` 로 그 사실과 plugin 로그 확인 경로를 남긴다. frame 이 도착하면 래치가 풀려, 이후
crash 로 다시 비면 재경고한다. 원인 자체는 여전히 plugin 로그에서 확인해야 한다 — 이
로그는 "어디를 볼지"를 가리키는 신호다.

검사는 forward 루프(= redraw) 안에서 돈다. 완전 idle 상태면 다음 redraw 까지 지연되지만,
빈 화면을 보고 조작하는 순간 발화한다.

## 개방 정책

bundled 전용. `(kind, plugin_id)` 화이트리스트 + plugin `api_version` 이 호스트와 일치할
때만 등록된다 (epaint 와이어가 host·plugin 동일 컴파일을 강제하는 동안의 보호). 현재
허용: `(image, com.tasty.image)`, `(mesh_demo, com.tasty.mesh-demo)`. (`markdown` 은
[ADR-0065](../adr/0065-markdown-webview-render-channel.md) 로 webview 전환되며 이
화이트리스트에서 빠졌다 — 대용량/파일열기 확인 팝업 2개는 이 화이트리스트와 무관하게
`[[contributes.popup]]` 로 별도 등록된다.)

## attach mesh mirror 소비 경로

위 채널은 host 가 **자기 프로세스의 plugin** 을 구동하는 경로를 전제한다. attach(원격
피점유측)에서는 이 채널의 소비자가 하나 더 있다 — mirror 를 붙인 **client** 가 원격의
plugin 이 그린 mesh 를 자기 화면에 렌더하고, 자기 입력을 원격으로 되돌려 보낸다. 프로토콜
결선(어떤 `StreamControl` variant 로 무엇을 나르는지)은
[attach-behavior.md "mesh mirror 채널"](attach-behavior.md#mesh-mirror-채널) 에 있다.
여기서는 이 소비자가 위 채널의 각 구성 요소를 어떻게 재사용/대체하는지만 정리한다.

- **공용 구동/relay 함수**: `src/plugin_bridge/mesh_forward.rs` 가 `PluginManager` 접근권이
  필요한 두 함수를 gui/headless 양쪽 빌드 공용으로 제공한다(`plugin_bridge` 는 gui 여부와
  무관하게 컴파일되는 bin-side glue 모듈이라 아래 세 소비처 모두가 참조할 수 있다) — `mesh_mirror.rs`
  자신은 설계상 `PluginManager` 를 모른다(registry 전용).
  - `forward_mesh_frames_for_engine(engine, mgr, stream_hub)` — `MeshMirrorRegistry` 의
    dirty/누적 입력/modifiers 를 직접 읽어 `surface.set_context` 를 구동하고(bootstrap 포함),
    새 `PaintFrame` 을 relay 까지 한다. **로컬 authoritative render loop(살아있는 window)가
    없는 engine 전용** — 아래 "헤드리스"와 "GUI parked engine" 두 소비처가 그대로 재사용한다.
  - `relay_mesh_frame_if_new(engine, mgr, stream_hub, sid, client_id)` — 이미 만들어진
    `EguiMeshFrame`(있다면)을 새 `set_context` 없이 순수 byte relay 만 한다. 위 함수의 꼬리
    로직이자, "GUI 살아있는 window" 소비처가 유일하게 재사용하는 부분(아래 참조).
- **서버측(헤드리스): `PluginManager` 직접 구동 (기존 채널 그대로 재사용)** — attach 서버가
  헤드리스(`boot::run_headless`)일 때는, 위 표의 "host→plugin set_context + 입력
  forward"/"host 합성" 두 축 중 **입력 forward 축만** 대체되고 **plugin 프로세스 구동
  자체는 기존 `PluginManager`/`crates/tasty-host-plugin` 를 그대로 쓴다** — attach 전용
  plugin 매니저가 별도로 있는 게 아니다. `src/boot/headless_plugins.rs::pump_plugins` 가
  매 tick 위 `forward_mesh_frames_for_engine` 을 단일 engine 에 대해 호출한다 — 되돌아오는
  `PaintFrame` 을 그대로 받아 `StreamTag::MeshData` 로 client 에 재중계한다 — **host 합성
  (`egui_wgpu::Renderer`, TextureId 격리, frame_seq 체인 검증)은 여기서 전혀 일어나지
  않는다**(서버는 화면이 없다). 원본 바이트를 그대로 client 에 넘길 뿐이다.
- **서버측(GUI, 살아있는 window): 로컬 redraw 의 결과를 옆에서 relay** — attach 서버가
  GUI(창 보유)일 때는 그 mesh surface 의 `set_context` 를 이미 로컬 창의 매 프레임 redraw
  (`MainView::forward_egui_mesh_context`)가 권위 있게 구동 중이다. 헤드리스처럼
  `forward_mesh_frames_for_engine` 으로 `PluginManager` 를 직접 구동하면 이 로컬
  authoritative loop 와 경합하므로 — attach client 가 요청한 (로컬과 다를 수 있는)
  width_px/height_px 로 재구동해 로컬 화면이 튈 수 있다 — `MainView::forward_mesh_to_attach_subscribers`
  (`src/view/main/egui_mesh.rs`)는 위 `relay_mesh_frame_if_new` 만 재사용해 **별도
  `set_context` 를 보내지 않고 이미 만들어진 `EguiMeshFrame` 바이트만 읽어 `StreamTag::MeshData`
  로 relay** 한다. 유일한 예외는 attach 구독 대상이 로컬 어디에서도 렌더되지 않는 surface(다른
  탭/워크스페이스에 있어 로컬 target 목록에 전혀 없어 plugin 이 그 surface_id 자체를 모름)인
  경우뿐 — 이땐 경합할 로컬 루프가 없으므로 이 훅이 `find_egui_mesh_surface`
  (`src/core/state/pty.rs`)로 메타데이터를 조회해 최소 `surface.create` + `set_context`
  bootstrap 을 1회 대신 보낸다. 이미 렌더 중인 surface 에 새 구독(또는 명시 재전송 요청)이
  들어와 전체 텍스처가 필요하면, 직접 보내지 않고 로컬 `MeshForwardState::pending_full` 에
  위임해 다음 tick 의 authoritative loop 가 `need_full_textures` 를 실어 보내게 한다(그
  사이엔 캐시된 델타뿐일 수 있는 frame 을 새 구독자에 흘리지 않고 건너뛴다 — 텍스처 손상
  방지). attach client 의 입력을 로컬 plugin 에 되먹이는 축(`MeshMirrorRegistry::take_pending_events`)
  은 아직 이 경로에 배선되지 않았다 — gui-as-server 에서 mesh 콘텐츠는 보이지만 아직
  인터랙티브하지 않다.
- **서버측(GUI, parked engine): 헤드리스와 동일하게 직접 구동** — macOS 에서 window 를
  최소화하면 `App::handle_minimize` 의 macOS 분기가 그 window 의 `MainView` 를 파괴하고
  `(AppState, CoreState)` 를 `App::parked_states`(`src/app.rs`) 로 옮긴다. 이 engine 은
  더 이상 `handle_redraw` 가 돌지 않으므로 "GUI 살아있는 window" 항목이 전제하는 로컬
  authoritative loop 가 없다 — **처지가 헤드리스와 같다.** `App::about_to_wait`
  (`src/app/event_handler.rs`, plugin manager `pump()` 호출 직후)가 `parked_states` 전부를
  순회하며 각 engine 에 `forward_mesh_frames_for_engine` 을 호출한다(구독/입력 forward/
  full-resend 요청 자체는 `apply_mesh_context_on_owning_engine` 류의 owning-engine 순회
  패턴으로 이미 parked engine 에도 정상 반영되고 있었다 — 실제 frame 구동/relay 만
  빠져 있었다). `window_lifecycle.rs` 의 창 복원은 `parked_states.remove(0)` 으로 **1개씩만**
  꺼내므로, 여러 window 가 동시에 minimize 돼 있으면 나머지는 계속 이 순회의 대상으로
  남는다 — 첫 매치에서 멈추는 owning-engine 패턴과 달리, 이 순회는 매 tick `parked_states`
  전부를 무조건 방문한다.
- **client측: host 합성 축의 재구현** — client 가 원본 `PaintFrame` 바이트를 받아 **자기
  화면에서** 위 "host 합성" 단계(decode → 전용 `egui_wgpu::Renderer` → surface rect 합성)를
  그대로 재현한다. `decode_mesh_into_target` 은 이 재사용을 위해 SharedBuffer 파싱 부분과
  디코드 로직을 분리했다 — attach 경로는 SharedBuffer 가 없으므로(네트워크로 바이트가 옴)
  `decode_mesh_bytes_into_target`(source-neutral)을 직접 부른다. 텍스처 delta 체인 검증
  (`frame_seq`/`chain_ok`/`AcceptedStale`/`NeedsFull`, 위 "텍스처 상태 수명 + delta 체인")
  규칙은 로컬 경로와 **완전히 동일**하게 client 에서도 적용된다 — 다만 복구 요청
  (`need_full_textures`)이 로컬 IPC 대신 `StreamControl::MeshFullResendRequest` 네트워크
  프레임으로 나간다는 점만 다르다.
- **surface stand-in**: 로컬 `EguiMeshSurface`(`src/plugin_bridge/egui_mesh_surface.rs`)의
  attach 대응은 `AttachMeshSurface`(`crates/tasty-model/src/attach_mesh_surface.rs`) —
  plugin 콘텐츠(예: image 파일 경로)를 소유하지 않는 순수 표시용 stand-in이라 `file` 필드가
  없다(원격이 이미 콘텐츠를 소유·bootstrap 한 상태이므로 client 가 재전달할 게 없다).
- **입력 forward 축의 대응 모듈**: `src/view/main/egui_mesh.rs`(로컬) ↔
  `src/view/main/attach_mesh_input.rs`(attach) — 후자는 좌표/modifier 변환 헬퍼
  (`mesh_local_point`/`mesh_modifiers`/`mesh_theme_snapshot`/`map_button`/`key_wire_event`)를
  그대로 재사용하고, 목적지만 로컬 `PluginManager` 대신 `CoreState` forward 큐(네트워크)로
  바꾼다. IME candidate 위치 한계·클립보드 미지원 등 위 "알려진 한계"는 attach 경로에도
  동일하게 적용된다(입력 자체가 같은 wire 이벤트를 타므로).
- **개방 정책은 서버측에서 재검증**: client 는 서버가 보낸 디스크립터의 `role: "mesh"` 를
  신뢰하지 않고, 서버(`build_workspace_tree_surfaces`, `src/core/attach_runtime.rs`)가
  `Surface::attach_mesh_info()` + 위 "개방 정책" 화이트리스트(`is_egui_mesh_allowed`)로 재검증한
  결과만 `role: "mesh"` 로 내려보낸다 — 화이트리스트 밖 kind 는 attach 에서도 `role:
  "placeholder"` 로 남아 mirror 렌더 대상이 아니다.
## plugin 작성

`tasty-plugin.toml` 의 `[[surface_kinds]]` 에 `rendering = "egui-mesh"` 를 선언하고,
SDK 를 `features = ["egui-mesh"]` 로 받아 `EguiMeshSurface::paint(&ctx.host,
&ctx.params, |egui_ctx| { ... })` 를 `Plugin::paint_surface` 에서 호출하면 된다.
코덱/송신은 SDK 가 은닉한다. 최소 예시는 `crates/tasty-plugin-mesh-demo/src/main.rs`.

## plugin 콘텐츠의 clip 규약 (surface·popup·banner 공통)

**적용 범위는 plugin 이 자기 mesh 안에 그리는 콘텐츠뿐이다.** plugin 은 host 가 잡아준
영역(surface rect / popup content_rect / banner content_rect) 안에서만 그리고, 그 영역
자체를 정하는 것은 host 다. 그래서 plugin 쪽 clip 조작은 **항상 "더 좁히기" 방향**이며,
부모 clip 과 교집합하는 API 만 쓴다.

| 목적 | API | 시맨틱 |
|---|---|---|
| `Ui` 수준 clip 좁히기 | `Ui::shrink_clip_rect(rect)` | 현재 clip 과 **교집합** |
| 단발 그리기 clip | `Painter::with_clip_rect(rect)` | 현재 clip 과 **교집합** |
| plugin 콘텐츠에서 금지 | `Ui::set_clip_rect` / `Painter::set_clip_rect` | 부모 clip **덮어쓰기** |

`set_clip_rect` 계열은 부모가 걸어둔 clip 을 파기한다. `ScrollArea` 안에서 행 단위로
쓰면 특히 위험하다 — 행 rect 는 **스크롤된 가상 콘텐츠 좌표**라, 뷰포트 밖으로 밀려난
행의 clip 이 그대로 유효해져 pane 경계를 넘어 다른 pane 위에 그려진다. 가로 상한만
의도했더라도 세로까지 함께 풀린다.

교집합 API 는 "부모가 허용한 범위 안에서만 더 좁힌다" 는 의미라 이 실수가 구조적으로
불가능하다. 갤러리 specimen 은 보통 `ScrollArea` 없이 고정 목록을 그리므로 이 결함을
재현하지 못한다 — clip 회귀는 본체 popup 에서 스크롤시켜 확인해야 한다.

> **host 셸 코드에는 적용되지 않는다.** host 는 팝업 Area 처럼 *경계를 새로 세우는* 쪽이라
> `set_clip_rect(content_rect)` 로 clip 을 **확정**하는 것이 오히려 필수다 — 근거와 절차는
> [popup-implementation.md](popup-implementation.md) 의 "콘텐츠 레이어 — `egui::Area`
> 등록" 절.

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
