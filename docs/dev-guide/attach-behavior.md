# Attach 메커니즘

attach 가 *어떻게 구현되는가*. 사용자/에이전트가 보는 **동작·점유 규칙·인터페이스는** [`features/remote-attach`](../features/remote-attach/index.md) 에, 여기엔 **후속 작업자가 서버 핸들러를 잘못 건드리지 않도록 못 박는 내부 메커니즘**만 둔다.

가장 헷갈리는 핵심: **서버는 transport 를 모르고 항상 loopback 으로 받는다 — 로컬/원격 구분은 전적으로 클라이언트 측 개념이다.**

## 서버 / 클라이언트 계층 (가장 먼저 읽을 것)

attach 는 **server**(피점유 — PTY/grid 소유)와 **client**(점유 — mirror 표시) 두 쪽이다.

- **서버측** (`src/core/attach_runtime.rs`, IPC `attach.*`) — **transport 를 모른다.** 항상 `127.0.0.1` 로만 client 를 받는다. 로컬에서 붙든 SSH 터널 너머에서 붙든 서버 입장엔 전부 loopback 이다. 서버는 SSH 를 전혀 모른다.
- **클라이언트측** — "원격성" 을 전부 흡수한다. 두 종류:
  - **로컬 client**: 포트 파일(`~/.tasty/tasty.port`)을 읽어 그 loopback 포트로 직결. **release 에서 제거 → debug 전용**(`tasty debug attach`).
  - **원격 client**: `ssh -L 127.0.0.1:<localport>:127.0.0.1:<remoteport> -N` 터널 후 그 **localport 로 직결**. 터널은 바이트 파이프라 스트림 프로토콜에 투명 — 원격 client 도 결국 자기 머신 loopback 에 붙는다(`tasty remote attach --ssh|--profile`).

### "로컬 attach 제거" 의 정확한 의미

> **attach 는 원격을 대상으로 한다** — 로컬 self-attach 는 release 에서 제거하고 debug 격리한다. 이 결정의 *근거·대안·재검토 조건* 은 [ADR-0007](../adr/0007-attach-targets-remote.md). 아래는 그 결정이 구현에 어떻게 드러나는지다.

원격 attach 도 **서버 입장엔 loopback** 이다. 따라서 release 에서 "로컬 attach 제거" 는 **서버를 바꾼 게 아니라 client 의 로컬 진입점(`tasty attach` → `tasty debug attach`)만 제거**한 것이다. 서버의 attach 수신 경로는 로컬/원격 공용으로 보존된다. SSH 터널 + attach 세션 머신(`run_attach_*`)은 `crates/tasty-cli/src/commands/attach.rs` 에 공용으로 남고, `remote`/`debug` 네임스페이스는 그 위에서 디스패치만 한다.

### `remote` / `debug` 는 CLI 디스패치 네임스페이스 (IPC 와 비대칭)

`remote`·`debug attach` 는 **IPC 네임스페이스가 아니다.** attach 의 IPC 표면은 `attach.*`(아래) 그대로이고, `remote attach`/`remote check`/`debug attach` 는 그 위(+`system.info`)에서 *원격성·debug 격리만 분기*하는 CLI 계층이다.

## 점유 레지스트리 (`OccupancyRegistry`)

`src/core/attach.rs`. 휘발성(직렬화/복원 안 함 — 재시작 시 빈 registry → 전부 free).

이 registry 는 [ADR-0040](../adr/0040-occupancy-soft-hard-tiers-agent-occupant.md) 의 **약한/강한(soft/hard) 2계층 점유**를 함께 담는다. **attach 는 강한(hard) 점유의 한 사례** — 아래는 그 hard 경로다. 약한(soft) 점유(advisory 마커, write 미차단; 현 소비자 = `terminal` 명령의 child-terminal)는 [`features/child-terminal`](../features/child-terminal/index.md) 를 보라. 두 계층은 같은 registry 에 있지만 별도 저장이다(hard=`surface_locks`/`workspace_locks`, soft=별도 soft 엔트리).

- `surface_locks: HashMap<SurfaceId, AttachLock>` — surface 단위 배타(hard) lock. `acquire` 가 동시 점유를 `AlreadyAttached{holder}` 로 거부.
- `workspace_locks` + `surface_to_workspace` — workspace 단위 점유. workspace 점유 시 멤버 *터미널* 은 `surface_locks` 에도 동일 holder 로 등록(서버측 placeholder 렌더·입력차단을 surface 단위와 동일 적용). 비-터미널은 역매핑(`surface_to_workspace`)으로만 "점유 표시".
- `release` (holder 본인) / `force_detach` (서버 권한) / `release_all_for_client`(연결 생존 판정 실패 시 일괄 — workspace + 멤버 + 잔여 surface). 트리거는 3 종: **self-release**(holder 의 명시적 release) · **force-detach**(로컬 사용자 강제 해제) · **EOF-or-TTL**(연결 종료 EOF 또는 attach heartbeat TTL 만료 — 둘 다 `tcp_ipc_server.rs` read 루프의 `Err(_) => break` 를 거쳐 `StreamInbound::Disconnected` → `release_all_for_client` 로 합류한다; TTL 은 소켓 read timeout 이 heartbeat 미수신으로 만료되는 경우로, 실제 EOF 와 동일한 `io::Error` 취급이라 코드 경로가 갈라지지 않는다). 강한 점유 해제 사유 확장의 근거는 [ADR-0052](../adr/0052-attach-heartbeat-ttl-hard-occupancy-release.md).
- **입력 격리**: `apply_send_to_surface` 가 `is_hard_occupied` 면 서버 로컬 입력 거부, client 입력만 `feed_attached_input` 우회 경로로 PTY 도달. (soft 점유는 write 를 막지 않는다 — hard 만 격리.)

## 초기 스냅샷 + delta

attach 직후 서버가 현재 visible 화면을 `snapshot_as_vt` 로 **1회** 직렬화 push(셀 속성 + 커서 + alt-screen/DECCKM/bracketed 모드 복원). 이후 변화는 output tap delta(Data 프레임). client 는 받은 바이트를 PTY 없는 mirror 터미널(`Terminal::new_detached` + `feed_bytes`)에 먹여 같은 termwiz 파서로 grid 재구성.

## workspace mux

한 연결로 N 터미널 출력을 나르므로 workspace 모드 Data 프레임은 **surface-prefixed**(`encode_mux`/`decode_mux`). surface 단위 단일 연결은 prefix 없음. attach 직후 서버가 `attached_workspace` Control 로 트리(분할 방향/비율) + per-surface 디스크립터를 보내고, client 는 원격↔로컬 surface_id 재매핑으로 트리를 재구성한다. per-surface role 은 3종: `{remote_id, role:"terminal", cols, rows}`(mirror 가능) / `{remote_id, role:"mesh", kind, plugin_id}`(bundled egui-mesh 화이트리스트 통과 — mesh mirror, 아래 절) / `{remote_id, role:"placeholder", kind}`(그 외 비-터미널, mirror 불가). 이 role 분류는 `build_workspace_tree_surfaces`(`src/core/attach_runtime.rs`)가 `Surface::attach_mesh_info()`(TODO 16) + `is_egui_mesh_allowed` 화이트리스트 재검증으로 만든다.

## 갱신 cadence 분리

- **서버측 readonly 뷰**(피점유 — 대상 부하 절약): **3초 polling**(`AttachPoll` tick, `src/app/attach_poll.rs`)으로 self-snapshot 적용.
- **client mirror**(내가 다루는 대상): 원격 출력이 올 때마다 즉시 갱신, 3초 tick 은 backstop(누락 출력 적용·끊긴 세션 정리)으로만.

## 리사이즈 전파 (mirror geometry)

mirror grid 는 **client 가 구동(client-driven)** 한다(ADR-0045) — mirror 를 띄운 **로컬 pane 의 크기**가 grid 를 정하고, 원격 PTY 를 그 크기로 reflow 시킨다. "remote authoritative" 는 **메커니즘으로만 유지**: 원격 PTY 가 실제 크기의 단일 진실원(reflow 담당)이고 그 settled 크기를 echo 로 되돌린다. 즉 **의도(intent)는 client, 확정(confirm)은 remote** 의 요청→확정 협상이다.

- **client 구동 (forward)**: mirror 는 detached 터미널(PTY 없음)이라, 매 프레임 도는 로컬 레이아웃 리사이즈 스윕(`Core::resize_all_terminals` / `AppState::resize_all`)이 detached 터미널을 로컬에 적용하는 대신, 목표 grid `(cols, rows)` 를 `CoreState.pending_resize_forward`(로컬 surface id → grid)에 넣는다(목표가 이미 현재 mirror grid 면 생략). 이 한 곳에서 걸러 모든 리사이즈 진입점(창 resize·divider drag·단축키·redraw)을 커버한다. `App::dispatch_pending_resize_forwards`(`about_to_wait`, gui)가 drain 해 로컬 id 를 세션 매핑으로 원격 id 로 치환하고 `StreamControl::ClientResize{surface_id, cols, rows}` 를 `Control` 프레임으로 forward 한다. 세션의 last-forwarded dedup 이 echo 왕복(약 1 RTT) 동안의 매 프레임 재전송을 억제한다(coalesce; 서버측 동일값 `resize_grid=false` no-op 이 2차 방어).
- **서버 적용 (server)**: `StreamHub::pump_inbound` 이 `ClientResize` 를 `PumpOutcome.resize_requests` 로 분류 → 메인루프(gui `event_handler`/headless `boot`)가 anchor 워크스페이스의 **holder 를 검증**(hard 점유 = geometry 구동 권한, ADR-0040/0045)한 뒤 `CoreState::apply_attached_workspace_resize` 로 원격 **실제 PTY** 를 `Terminal::resize` 한다(reflow).
- **로컬 grid 는 echo 로만 갱신 (desync 방지)**: client 는 목표 크기를 로컬 mirror grid 에 **낙관적으로 먼저 적용하지 않는다**. 원격 reflow 전 잘못된 grid 에 바이트가 재생되는 desync 를 막기 위해, mirror grid 는 아래 `Resize` echo 가 도착할 때만 바뀐다.
- **원격→client 확정 echo**: 원격 터미널 grid 가 실제로 바뀌면(`TerminalState::resize_grid` 이 `true`) 서버의 resize tap(`Terminal::add_resize_tap`)이 새 `(cols, rows)` 를 fan-out 하고, attach forwarder 스레드가 `Control` 프레임에 `StreamControl::Resize{surface_id, cols, rows}` 를 실어 client 에 push 한다. workspace 모드는 `surface_id` 에 remote surface_id 를 실어 client 가 remote→local 매핑으로 해당 mirror 만 리사이즈한다. **이 echo 경로는 client-driven 전환 전과 무변경 재사용** — client 요청이 원격을 구동하면 결과가 이 경로로 되돌아온다.
- **순서 보존**: client reader 는 Data(출력)와 Resize 를 **한 버퍼에 도착 순서대로**(`MirrorEvent`) 쌓고, 메인 스레드가 순서대로 적용한다 — resize 앞뒤 출력이 올바른 그리드에서 재생되도록.
- **`StreamControl` 확장성**: `event` 태그 기반 enum. mid-session 이벤트는 새 `StreamTag` 없이 variant 로 추가한다 — 현재 `Resize`(server→client, 확정 echo), `Activity`(server→client, busy/idle 활동 상태), `ClientResize`(client→server, geometry 구동 요청), `StructuralOp`(client→server, 구조 변경 forward), `StructuralResult`(server→client, forward 회신), `StructuralDelta`(server→client, 구조 역반영), `MeshContext`(client→server, mesh 구독+geometry/theme/focus — 아래 "mesh mirror 채널" 절), `MeshInput`(client→server, 누적 입력), `MeshFullResendRequest`(client→server, 텍스처 delta 체인 복구 요청), `MeshError`(server→client, mesh 단발 실패 통지). 알 수 없는 event 는 역직렬화 실패로 무시(전방/후방 호환) — 구버전 서버는 `ClientResize` 를 무시해 기존 remote-authoritative 로 graceful degrade 한다. 단발 핸드셰이크 디스크립터(`attached`/`attached_workspace`/`attach_error`)와 `force_detached` 는 여전히 ad-hoc JSON 으로 위치 기반 파싱.

## 활동(busy) 상태 전파

사이드바 워크스페이스 리스트의 "실행 중" status dot(`WorkspaceEntryView.busy_count`, `docs/features/remote-attach/index.md` "GUI mirror" 참조)은 surface 의 busy/idle 상태를 본다. mirror(detached) 터미널은 로컬 PTY 가 없어 `CoreState::refresh_busy_surfaces`(foreground-process 폴링, `src/core/state/busy.rs`)가 절대 채울 수 없으므로, **원격이 직접 자기 surface 의 busy 상태를 계산해 client 로 forward** 한다 — resize 의 client-driven 협상과 반대로, busy 는 순수 **server→client** 단방향이다(원격 foreground 프로세스 이름은 애초에 client 에 존재하지 않는 정보라 client 가 요청할 수도 없다).

- **서버측 계산·forward**: 서버는 1Hz busy-poll tick(gui `app/busy.rs` 의 `poll_busy_states`, headless `boot.rs` 의 `AppEvent::BusyPoll` — 아래 참조)마다 `CoreState::forward_busy_activity`(`core/attach_runtime.rs`)를 호출한다. 이 메서드는 `busy_activity_forwards`(`core/state/busy.rs`)가 계산한 **점유 중인 surface 의 busy 값 변화분**만 `StreamControl::Activity{surface_id, busy}` 로 그 workspace/surface 의 holder client 에 push 한다. `last_forwarded_busy` 캐시로 값이 실제로 바뀐 경우에만 forward(중복 억제)하되, surface 가 점유 해제됐다가 재점유되면(다른 client 일 수 있음) 캐시를 버려 값이 이전과 같아도 항상 fresh 하게 1회 다시 push한다 — resize 의 `last_forwarded_resize` dedup 과 동형이나 방향이 반대(client→server 아닌 server→client)다.
- **headless 는 자체 ticker 필요**: headless 메인 루프(`boot::run_headless`)는 `rx.recv()` 로 이벤트를 기다리기만 할 뿐 자체 타이머가 없다 — gui 의 `boot::busy_tick`(winit `EventLoopProxy` 기반, `#[cfg(feature = "gui")]`)이 없으면 `AppEvent::BusyPoll` 이 영원히 발화하지 않는다. `HeadlessWaker::spawn_busy_ticker`(`adapters/production/headless_waker.rs`)가 같은 1Hz cadence 로 `mpsc::Sender<AppEvent>` 에 `BusyPoll` 을 직접 보내 이를 미러링한다. **원격 attach 의 주 시나리오가 headless 서버**이므로 이 ticker 가 없으면 활동 상태 forward 가 전혀 동작하지 않는다.
- **client 적용**: reader 스레드가 `Activity` Control 프레임을 `MirrorEvent::Activity(remote_surface_id, busy)` 로 버퍼링하고, `apply_attach_client_output` 이 세션의 `remote_to_local` 매핑으로 로컬 mirror surface id 를 찾아 `CoreState::set_mirror_surface_busy` 를 호출한다. 이 값은 **`busy_surfaces`(로컬 폴링 결과)와 분리된 `mirror_busy_surfaces` 별도 집합**에 저장된다 — 같은 집합에 합쳤다면 1Hz `refresh_busy_surfaces` 가 매 tick 로컬 폴링 결과로 집합을 통째로 교체하며 mirror 값을 지워버렸을 것이다. `is_surface_busy`/`any_busy`/`busy_count`(사이드바·상태바·`surface.list` IPC 가 공유하는 단일 진입점)는 두 집합의 합집합을 본다.
- **정리**: mirror surface 가 없어지면(`cleanup_mirror_workspace`, `apply_mirror_structural_delta` 의 removed 처리) `CoreState::forget_mirror_surface_busy` 로 `mirror_busy_surfaces` 에서도 제거해, 로컬 id 가 재사용될 때 stale busy 값이 새 surface 에 잘못 붙는 것을 막는다.

## mesh mirror 채널

bundled egui-mesh surface(markdown/image/mesh_demo — `is_egui_mesh_allowed` 화이트리스트)가 mirror pane 에 뜨면, 원격의 실제 plugin 프로세스가 그리는 GPU mesh 프레임을 client 로 스트리밍해 렌더하고 client 입력을 원격으로 forward 한다. 개념·기능 범위는 [features/remote-attach](../features/remote-attach/index.md#surface-단위-vs-workspace-단위), 헤드리스 부트스트랩·로컬 egui-mesh 파이프라인 자체는 [egui-mesh-channel.md](egui-mesh-channel.md#attach-mesh-mirror-소비-경로), 여기엔 attach 프로토콜 결선만.

- **구독 = `MeshContext` (별도 핸드셰이크 없음)**: client 가 mesh surface 를 그리기 시작하면 `StreamControl::MeshContext{surface_id, width_px, height_px, pixels_per_point, theme, focused}` 를 보내는 것 자체가 구독 신호를 겸한다 — capability negotiation 을 위한 별도 확인/ack 프레임이 없다. 서버 `MeshMirrorRegistry::upsert`(`src/core/mesh_mirror.rs`)가 이 정보를 최초 수신 시점에 등록하고, 이후 값이 실제로 바뀔 때만(geometry/theme/focus 변경 시) client 가 재전송한다(`forward_attach_mesh_context`, `src/view/main/attach_mesh_input.rs`) — 매 프레임 재전송하지 않는다.
- **`MeshInput` 누적**: 포인터/키/스크롤/IME 이벤트는 `AttachMeshForwardState.events`(client, dedup 없이 순서 보존)에 프레임마다 쌓였다가, 다음 redraw 의 `forward_attach_mesh_context` 호출에서 `RawInputWire{modifiers, events, ..}` 로 묶여 `MeshInput{surface_id, input}` 1회 전송된다. 서버는 `MeshMirrorRegistry::push_input` 이 `pending_events` 에 extend 하고 `last_modifiers` 를 갱신 + `dirty=true` 로 표시 — 구독이 없는 surface_id 면 `false` 를 반환해 서버 dispatch 가 `MeshError` 로 회신한다(아래).
- **frame 소비·forward (headless-as-server)**: 헤드리스 부트스트랩의 `forward_mesh_frames`(`src/boot/headless_plugins.rs`)가 매 tick `mesh_mirror.take_dirty`/`take_pending_events`/`last_modifiers` 를 읽어 로컬 `PluginManager` 에 `SurfaceSetContextParams.raw_input` 으로 흘려보내고, plugin 이 그린 결과 mesh 프레임을 `StreamTag::MeshData` 청크로 client 에 push 한다(청크 분할 이유: 텍스처 델타 포함 paint frame 이 `MAX_FRAME_LEN` 을 초과할 수 있음). client reader 스레드는 `tasty_ipc::mesh_stream::MeshFrameAssembler` 로 청크를 재조립해 `MirrorEvent::Mesh(remote_surface_id, generation, frame_seq, full_textures, bytes)` 를 메인 스레드 버퍼에 쌓고, `AttachMeshFrameStore::update`(`src/core/attach_mesh_frames.rs`)에 저장된 것을 GPU 렌더 경로(`render_attach_mesh_surfaces`, `src/gfx/gpu/egui_mesh_prepare.rs`)가 소비해 화면에 그린다.
- **frame 소비·forward (gui-as-server)**: gui 인스턴스가 attach 서버일 때는 `MainView::forward_mesh_to_attach_subscribers`(`src/view/main/egui_mesh.rs`, 매 redraw 마다 `forward_egui_mesh_context` 직후 호출)가 headless 와 대칭 역할을 한다. 다만 로컬 창의 `forward_egui_mesh_context` 가 화면에 보이는 mesh surface 의 `set_context` 를 이미 매 프레임 권위 있게 구동하므로, 이 훅은 별도 `set_context` 를 보내지 않고 **이미 만들어진 `EguiMeshFrame` 바이트를 읽어 relay** 만 한다(로컬 authoritative loop 와 경합 회피). 예외는 attach 구독 대상이 로컬 어디에서도 렌더되지 않는 surface(다른 탭/워크스페이스에 있어 로컬 target 목록에 전혀 없음)인 경우뿐 — 이땐 plugin 이 그 surface_id 자체를 모르므로 경합 없이 이 훅이 최소 `surface.create` + `set_context` bootstrap 을 1회 대신 보낸다(`find_egui_mesh_surface` 로 메타데이터 조회). 이미 렌더 중인 surface 에 새 구독이 들어와 전체 텍스처가 필요하면(신규 구독/명시 재전송), 직접 보내지 않고 로컬 `MeshForwardState::pending_full` 메커니즘에 위임해 다음 tick 의 authoritative loop 가 `need_full_textures` 를 실어 보내게 한다 — 그동안(next tick 까지) 이 훅은 캐시된(델타뿐일 수 있는) frame 을 새 구독자에 흘리지 않고 건너뛴다.
- **`MeshFullResendRequest` 복구**: 텍스처 델타 체인이 깨졌다고 판단되면(로컬 `EguiMeshRenderTarget` 의 generation 검증 실패 등) client 가 `attach_mesh_full_requests`(`GpuState`)에 surface_id 를 쌓고, `App::dispatch_pending_mesh_full_resend_forwards`가 `MeshFullResendRequest{surface_id}` 를 서버로 forward 한다. 서버는 이를 받으면 `MeshMirrorRegistry::request_full_resend` 로 해당 surface 의 다음 프레임을 풀 텍스처 포함(full_textures=true)으로 강제한다.
- **`MeshError` — 명시적 단발 실패**: 구독 안 된 surface 로의 `MeshInput`(홀더 불일치 포함, `CoreState::apply_attached_mesh_input` 의 holder 검증) 등은 조용히 drop 하지 않고 `MeshError{surface_id, reason}` 를 1회 회신한다(TODO 15 결정 — 조용한 drop 보다 명시적 실패가 디버깅에 유리). client 측 소비는 현재 로그 레벨 처리만(재시도/toast 없음) — 세션이 정상이면 애초에 발생하지 않는 방어적 경로.
- **App/CoreState 경계를 건너는 forward-queue 패턴**: `MainView`(redraw 시점)는 `App.attach_client_sessions`(소켓 writer 보유)에 접근할 수 없다. `forward_attach_mesh_context`/입력 캡처(`mouse.rs`/`keyboard.rs`/`ime.rs`)는 `CoreState.pending_mesh_context_forward`/`pending_mesh_input_forward`/`pending_mesh_full_resend_forward` 에 쌓아두기만 하고, `App::about_to_wait`(`attach_client.rs`)의 `dispatch_pending_mesh_*_forwards` 가 다음 tick 에 drain 해 세션 매핑으로 원격 id 를 치환한 뒤 실제 소켓 write 를 한다 — `pending_resize_forward`/`dispatch_pending_resize_forwards`(ADR-0045)와 동형 패턴을 3개 방향(context/input/full-resend)에 재사용한 것.
- **gui-as-server 는 attach client 의 입력 역방향 forward 를 아직 로컬 plugin 에 되먹이지 않는다**: 위 gui-as-server 훅은 mesh 바이트 forward(서버→client)만 구현한다 — `MeshInput` 으로 도착해 `MeshMirrorRegistry::push_input`/`pending_events` 에 쌓인 attach client 의 클릭/키 입력을 로컬 plugin 의 `raw_input` 에 병합하는 배선은 headless(`forward_mesh_frames` 가 `take_pending_events` 소비)에만 있다 — gui 는 후속 작업.
- **surface 디스크립터엔 display_name 없음**: `build_workspace_tree_surfaces` 가 보내는 mesh 디스크립터는 `{remote_id, role:"mesh", kind, plugin_id}` 뿐이라(`Surface::attach_mesh_info()` 가 `(&str, &str)` = kind/plugin_id 만 반환) client 의 `MirrorMeshInfo.display_name` 은 `kind` 문자열로 대체된다 — 탭 타이틀이 원격의 실제 display_name(예: 파일명)이 아니라 mesh kind(예: `"markdown"`)로 보일 수 있는 사소한 표시상 제약.

## mirror 구조 변경 forward

mirror 워크스페이스의 구조 변경(split/new-tab/close/move-tab)은 로컬에서 실행하지 않고 원격(authoritative)에서 실행되도록 forward 한다. intent 로 표현되는 경로(split·CLI/IPC)는 `Core::apply` 로 수렴해 거기서 forward 큐에 쌓이고, `Core::apply` 를 우회하는 UI-layer 직접 조작(`close_active_*`/`close_tab`/`add_tab`/`add_kind_tab`, 탭 드래그·컨텍스트 메뉴 이동)은 `AppState::forward_mirror_structural` 가 같은 큐(`pending_structural_forward`)에 대응 op 를 직접 쌓는다 — 두 경로 모두 동일 큐로 모여 아래 결선을 공유한다. 개념·정책은 [features/remote-attach](../features/remote-attach/index.md#mirror-워크스페이스-내-구조-변경), 여기엔 결선만.

- **request (client→server)**: `Core::apply` 가 mirror 구조 op 를 로컬 차단(`MirrorStructuralBlocked`)하면서 `StructuralOp`(anchor = **로컬** surface id)를 `CoreState.pending_structural_forward` 에 push. `App::dispatch_pending_structural_forwards`(`about_to_wait`, gui)가 drain 해 anchor 를 세션 매핑(`AttachClientSession.remote_to_local` 역방향)으로 **원격 id 로 치환**(`StructuralOp::with_anchor_surface_id`)한 뒤 `StreamTag::Control` 로 전송. anchor 를 surface id 로 잡는 이유: client 는 pane/tab 의 원격 id 매핑을 갖지 않으므로, 원격이 surface 로부터 pane/tab/workspace 를 자기 트리에서 resolve 한다.
- **진입 경로별 응답 정합성**: mirror 구조 op 는 사용자 GUI·에이전트 CLI/IPC 어느 경로든 `Core::apply` 로 수렴해 forward 된다(에이전트의 원격 mirror 조작은 금지가 아니라 정식 기능 — 원칙2). `MirrorStructuralBlocked` 의 `forwarded` 플래그가 "로컬 차단했으나 원격으로 큐잉됨" 을 표시하며, 두 경로가 이를 **성공으로** 처리한다: GUI 는 `report_apply_error` 가 `forwarded` 마커를 삼켜 차단 toast 를 띄우지 않고(성공 무음, 실패는 회신 toast), IPC/CLI 는 구조변경 핸들러(split·tab.create/close/move·pane.close·surface.close)가 `structural_apply_error`(`adapters/ipc/handler.rs`)로 `{forwarded:true, workspace_index}` **success** 를 회신한다. forward 는 비동기라 이 응답은 "원격에 큐잉됨" 을 뜻하고 실제 결과는 역반영 delta(아래)로 관측한다 — 성공을 `internal_error` 로 오보하지 않는다. `forwarded:false`(convert/move-surface 등 forward 불가 op)는 기존대로 에러.
- **실행 (server)**: `StreamHub::pump_inbound` 이 client Control 프레임을 `StructuralOp` 로 분류(`PumpOutcome.structural_ops`) → 메인루프(gui `event_handler`/headless `boot`)가 anchor 워크스페이스의 **holder 를 검증**(hard 점유 = 구조 변경 권한, ADR-0040)한 뒤 `execute_forwarded_structural_op`(`core/attach_runtime.rs`)로 기존 IPC 핸들러(split/tab.create/tab.close/tab.move/pane.close/surface.close)를 재사용해 실행. 서버 ws 는 mirror 가 아니라 `Core::apply` 가드를 통과해 실제 PTY 를 spawn 한다.
- **회신 (server→client)**: 실행 결과를 `StructuralResult{op_id, ok, reason?}` 로 push. 원격 미등록 kind 는 `create_surface_via_registry` 가 Err → `ok:false`+reason. client reader 가 실패 회신을 `MirrorEvent::StructuralFailed` 로 올려 실패 toast. 성공은 무음.
- **역반영 (server→client)**: 성공한 forward 는 원격에 생긴/사라진 surface 를 mirror 트리에 반영한다. `execute_forwarded_structural_op` 이 실행 **전/후** anchor 워크스페이스의 `all_surface_ids` diff 로 added(신규 터미널)를 계산하고, 실행 후 전체 트리+surfaces(`build_workspace_tree_surfaces`, 핸드셰이크 디스크립터와 공유)를 `StructuralDelta{workspace_id, tree, surfaces}` 로 만든다. 순서는 **`StructuralResult` → `StructuralDelta` → 신규 surface tap**(`tap_surface_for_stream`; client 가 매핑을 만든 뒤 스냅샷을 받도록 tap 을 delta 다음으로). 핸들러 응답 파싱이 아니라 트리 diff 라 close cascade·move-tab 도 균일 커버. client(`apply_mirror_structural_delta`)는 surfaces 를 세션 매핑과 대조해 survivor(기존 remote_id)는 **로컬 id 재사용**(터미널 재생성 금지 → scrollback 보존), 신규는 `Terminal::new_detached`+입력 forwarder, 사라진 것은 제거한 뒤 `build_mirror_workspace` 를 재실행해 같은 local ws id 로 in-place 교체한다. pane 상위 배치는 핸드셰이크와 동일한 방식(`to_attach_tree_json`의 `pane_layout` 트리 필드, direction/ratio 보존)으로 정확히 승계된다(3단계도 동일 정합성).
- **focus 보존 (client-only 보정)**: 위 트리 재구성이 실어 보내는 `tree.focused_pane`/pane 별 `active_tab` 은 **원격의** 값이다 — 순수 pane/tab 전환(클릭·키보드 이동)은 forward 되는 `StructuralOp` 가 없으므로(위 목록에 없음), 원격의 이 값은 대개 워크스페이스 생성 시점(첫 pane/첫 탭)에 고정된 채 절대 바뀌지 않는다. 과거엔 매 delta 마다 이 고정값을 그대로 실어 로컬 워크스페이스를 통째로 교체해, 사용자가 로컬에서만 다른 pane/탭으로 이동해둔 focus 가 구조 변경(새 탭/닫기 등) 때마다 첫 pane/첫 탭으로 되돌아가는 버그가 있었다. `apply_mirror_structural_delta` 는 교체 **전** 로컬에서 실제로 focus 돼 있던 surface 를 remote id 기준으로 캡처(`capture_focused_remote` — local pane/tab id 는 매 delta 마다 재발급돼 불안정하므로 remote surface id 를 기준으로 삼는다)해뒀다가, 교체 **후** 새 트리에서 그 surface 의 위치를 찾아(`find_pane_and_tab_for_surface`, 실제 갱신은 공용 헬퍼 `set_focus_to_surface`) `ws.focused_pane`/해당 `Pane.active_tab`/그 `Tab.focused_surface` 를 되돌린다(`restore_focus_after_delta`, 성공하면 `true`). 서버 상태는 전혀 건드리지 않는 순수 client-side 보정이다.
- **신규 리소스로 focus 추적 + close 인접 fallback (client-only, 사용자 GUI 조작 한정)**: 위 "옛 focus 복원"만으로는 두 상황을 못 다뤘다 — (1) 사용자가 mirror 안에서 새 탭/split 을 만들면 옛 focus(=새 리소스를 만들기 전 위치)가 그대로 살아남아 복원되므로 새로 생긴 리소스로 focus 가 전혀 안 옮겨간다. (2) focus 중인 surface 자체를 닫으면 캡처한 옛 focus 가 이번 op 로 사라져 복원이 실패하고, 그 경우 재구성된 `ws` 는 원격의 고정값(대개 워크스페이스의 첫 pane/첫 탭/첫 surface)을 그대로 담고 있어 그리로 튄다.
  - **user_triggered 태그**: `CoreState.pending_structural_forward` 의 각 원소(`PendingStructuralForward{op, user_triggered, close_focus_candidates}`)는 이 op 이 실제 사용자 GUI 조작(단축키/버튼/컨텍스트 메뉴)에서 나왔는지를 함께 싣는다. `AppState::forward_mirror_structural`(UI-layer 직접 조작 경로)는 호출부가 전부 GUI 라 항상 `true`. `Core::apply` 의 mirror-block 경로는 origin 을 모르므로 기본 `false`로 push 하고, origin 을 아는 GUI intent 핸들러(`intent::pane`/`intent::surface`/`intent::tab`)가 `core.apply` 실패 시 `crate::core::mark_last_forward_user_triggered` 로 방금 push 된 마지막 op 를 `origin.is_user()` 일 때만 뒤집는다 — IPC/CLI/플러그인 유래는 이 뒤집기가 없어 항상 `false` 로 남고, 그 결과 기존과 동일하게 focus 가 움직이지 않는다("포커스 독립성" 유지).
  - **close 인접 후보 계산(client, 닫기 전)**: `close_active_surface`/`close_active_tab`/`close_tab`(`state/pane.rs`, `state/tab.rs`)은 forward 하기 **직전** — 아직 로컬 트리가 마지막 delta 기준 그대로일 때 — 닫히는 surface/tab 이 focus 였을 경우의 client-side fallback 후보(로컬 surface id, 우선순위 순)를 계산해 `close_focus_candidates` 에 싣는다: split 된 tab 이면 같은 tab 안의 다른 leaf surface, split 안 된 tab(닫으면 탭 전체가 사라짐)이면 같은 pane 의 다른 탭의 focused surface(로컬의 "탭 하나만 남기고 닫음" 규칙과 동형 — 마지막 탭이 아니면 다음 탭, 마지막이면 이전 탭이 1순위). `close_active_pane`(pane 자체를 닫음)은 후보를 계산하지 않는다 — 로컬도 pane cascade close 에서는 무조건 첫 pane 으로 이동하므로 이 스코프 밖.
  - **op_id 상관 + PendingOpFocus**: `forward_one_structural_op` 이 op 을 세션에 실어 보낼 때, `user_triggered` 면 `pending_op_focus_for(op, close_focus_candidates, remote_to_local)` 로 이 op 이 `NewResource`(new-tab/split)인지 `Close{candidates}`(close 계열, 로컬 후보를 원격 id 로 치환한 것)인지 판정해 `AttachClientSession.pending_op_focus: HashMap<op_id, PendingOpFocus>` 에 등록한다. 이 op 의 성공 회신(`StructuralResult{ok:true, op_id}`, reader 가 `MirrorEvent::StructuralSucceeded(op_id)` 로 전달)이 오면 그 엔트리를 `next_delta_focus` 로 옮겨두고, 프로토콜이 보장하는 대로 그 직후 도착하는 `StructuralDelta` 적용 시 1회 소비(`take`)한다.
  - **적용 순서(`apply_mirror_structural_delta`)**: `NewResource` 면 이번 delta 의 surfaces 중 이전 매핑에 없던(=새로 생긴) remote id 를 찾아 그 local surface 로 `set_focus_to_surface` — 옛 focus 복원은 아예 건너뛴다(안 그러면 옛 focus 가 거의 항상 살아남아 복원이 새-리소스-focus 를 덮어써 버린다). 그 외의 경우 기존처럼 `restore_focus_after_delta` 를 먼저 시도하고, **그게 실패했을 때만**(=캡처해둔 focus 가 이번 op 로 사라짐) `Close{candidates}` 가 있으면 후보를 원격 id 순서대로 순회해 delta 이후에도 살아있는 첫 번째로 `set_focus_to_surface` — 후보가 다 없으면(예상 밖) 기존 그대로 원격의 고정값이 남는다.
  - **서버는 무관**: 위 어떤 보정도 `ws.focused_pane`/`pane.active_tab` 등 **서버측 상태를 바꾸지 않는다** — client 가 재구성한 로컬 `Workspace` 값만 조정하는 순수 client-only 보정이다. IPC/CLI(`tasty new tab` 등 진짜 에이전트 호출)로 같은 mirror workspace 를 조작하면 `user_triggered=false` 로 남아 이 보정 전부가 스킵되고, focus 는 여전히 움직이지 않는다(회귀 없음).
- **미구현**: convert/move-surface 는 재사용할 원격 IPC 핸들러가 없어 아직 forward 아닌 차단(`build_mirror_forward_op`·`execute_forwarded_structural_op` 모두 미지원). 그 외 구조 변경(split/new-tab/close/move-tab)은 intent 경로든 UI 직접 조작 경로든 모두 forward 된다.

## 서버 로컬(비-holder) 구조 변경 차단

위 절이 다루는 forward 실행(`execute_forwarded_structural_op`)은 hard 점유 **holder** 가 mirror 안에서 만든 정당한 구조 변경이 서버에 도달해 실행되는 경로다. 이 절은 반대 경우 — **서버 자신의 workspace 가 hard-occupied 상태일 때, holder 가 아닌 서버 로컬 IPC/CLI/agent 가 직접** `split`/`tab.create`/`pane.close`/`tab.close`/`tab.move`/`surface.close` 를 호출하는 경로를 다룬다. 개념·사용자 영향은 [features/remote-attach "서버(피점유)측 비-holder 구조 변경 차단"](../features/remote-attach/index.md#서버피점유측-비-holder-구조-변경-차단), 여기엔 메커니즘만.

- **왜 `Core::apply` 나 핸들러 함수 내부가 아닌가**: `execute_forwarded_structural_op`(`src/core/attach_runtime.rs`)는 holder 의 forward 실행 시 `tab::handle_tab_create`/`pane::handle_split`/`tab::handle_tab_close`/`pane::handle_pane_close`/`surface::handle_surface_close`/`tab::handle_tab_move` 를 **직접 함수 호출**한다 — 이 여섯 함수 내부, 또는 그 안에서 공통으로 거치는 `Core::apply` 에 가드를 걸면 holder 본인의 forward 요청까지 함께 막혀 위 "mirror 구조 변경 forward" 기능 전체가 회귀한다.
- **가드 위치**: `hard_occupied_structural_guard`(`src/adapters/ipc/handler.rs`) — `route_engine_handler` 의 method-string dispatch 최상단에서, `execute_forwarded_structural_op` 가 우회하는 그 dispatch 지점에서만 검사한다. params 에서 대상 pane_id/tab_id/surface_id(`split` 은 `target_pane`/`target_surface`, nickname 포함)를 뽑아 소속 workspace 를 찾고, `OccupancyRegistry::workspace_holder`(`src/core/attach.rs`)가 `Some` 이면 `invalid_params` 로 거부한다("점유 중" + "다른 workspace 사용" 안내). 대상을 resolve 할 수 없으면(params 누락 등) `None`(가드 미개입) — 원래 핸들러의 통상 검증 에러로 흘려보낸다.
- **CLI 는 무수정**: `crates/tasty-cli/src/transport.rs` 가 JSON-RPC `error.message` 를 그대로 노출하므로, `tasty new tab`/`tasty split`/`tasty close tab|pane|surface` 는 서버 에러 메시지를 그대로 보여준다.
- **테스트**: `src/core/attach_runtime.rs` `forward_exec_tests` 모듈 — `dispatch_denies_structural_create_when_hard_occupied`/`dispatch_denies_structural_close_move_when_hard_occupied`(비-holder 일반 IPC 호출 거부 + 트리 불변), `dispatch_allows_tab_create_when_not_occupied`(비점유 workspace 오탐 방지), `forward_*_succeeds_when_hard_occupied` 계열(holder 의 forward 는 hard-occupied 여도 정상 성공 — 회귀 방지).
- **극단 케이스(마지막 tab/pane close)는 이 가드로 도달 불가능해짐**: close/이동 계열을 다루던 원 TODO(11)는 "비-holder 가 hard-occupied workspace 의 마지막 tab/pane 을 서버 로컬에서 직접 닫으면 `workspaces_now_empty` → auto-recreate 경로가 발동해, 원래 `attach.workspace_locks` 가 가리키던 workspace_id 가 트리에서 사라지고 holder 는 더 이상 존재하지 않는 workspace_id 를 계속 점유 중인 것으로 남는 게 아닌지" 코드로 확인할 것을 요구했다. `hard_occupied_structural_guard` 가 `pane.close`/`tab.close`/`surface.close` 를 **dispatch 레벨에서 대상 workspace 가 hard-occupied 인 한 통째로 차단**하므로, 애초에 그 요청이 `Core::apply`/cascade 근처에 도달하지 못한다 — 마지막 tab/pane 인지 아닌지와 무관하게 비-holder 요청은 전부 거부되고 트리는 항상 불변이라, 이 극단 케이스 자체가 이 가드 도입 이후로는 발생할 수 없는 경로가 됐다. `dispatch_denies_structural_close_move_when_hard_occupied` 테스트가 단일 pane·단일 tab fixture(=정확히 "마지막 tab/pane" 상황)로 이를 검증한다.

## SSH 터널 (원격 client 공통)

`crates/tasty-cli/src/ssh.rs` — `remote attach` 와 `remote check` 가 공유.

- **시스템 ssh 위임**: 자체 암호화 없이 시스템 `ssh` 를 자식 프로세스로 실행. 사용자 `~/.ssh/config`·agent·known_hosts 재사용. **Windows 는 시스템 OpenSSH 풀경로**(`%WINDIR%\System32\OpenSSH\ssh.exe`) 우선 — git 번들 ssh 는 윈도우 ssh-agent(named pipe)를 못 봐 무암호 인증 실패.
- **원격 포트 발견**: 기본 `auto` = subcommand → file-unix → file-windows 순서로 원격 DefaultShell 4종(PowerShell/cmd/git bash/unix) 커버. `--remote-port-mode` 로 고정, `--remote-tasty <path>` 로 원격 바이너리 경로(기본 `tasty`).
- **터널 생명주기**: detach/종료 시 자식 ssh kill(고아 터널 방지)하되 **원격 데몬은 생존**(server-owns-PTY persistence = detach 의 본질). 자동 재연결(attach 한정): 지수 백오프(0.5s→30s)로 터널+attach 재수립(`--no-reconnect` 로 끔) — 이 재연결은 `run_attach_on_port` 가 반환하는 `AttachExit::Disconnected` 를 전제로 한다. mirror-dump/workspace-mirror-dump 모드는 처음부터 reader 를 별도 스레드 + `mpsc::channel` 로 분리해 `rx.recv_timeout` 의 `RecvTimeoutError::Disconnected`(끊김) vs `Timeout`(정상 deadline) 을 구분해 이를 반환해왔다. `--raw` 모드(`run_raw_bridge`)도 동일 계약을 만족한다 — stdin 스레드와 server reader 스레드가 하나의 `mpsc` 채널에 merge 해 보내고(`RawEvent::Stdin`/`StdinEof`/`Server`/`ServerRecvErr`), main 은 `rx.recv()`(deadline 없이 블로킹)만 기다린다. server reader 의 `conn.recv()` 가 `Err` 면 `RawEvent::ServerRecvErr` 를 명시적으로 보내 `AttachExit::Disconnected` 로 이어진다 — 과거엔 이 reader 스레드가 종료 사유와 무관하게 `std::process::exit(0)` 을 호출해 이 반환 경로 자체에 절대 도달하지 못했다(재연결 완전 불능이던 결함, 해소됨). **트레이드오프(기능 리스크 — 단순 메모리 낭비 아님)**: stdin 스레드는 blocking `read()` 를 깨울 방법이 없어 종료 신호를 못 받는다 — 재연결 루프에서 `run_raw_bridge` 가 재호출될 때마다 이전(좀비) stdin 스레드는 join 되지 않고 blocking read 에 갇힌 채 버려지고, 새 호출이 또 다른 stdin 스레드를 스폰한다. 이때 두 스레드는 서로 다른 `mpsc` 채널(각자 자기 `run_raw_bridge` 호출의 채널)에 연결돼 있지만 **같은 프로세스의 같은 stdin fd** 를 공유한다 — Rust `std::io::Stdin` 은 내부 `Mutex<BufReader<..>>` 로 동시 접근을 직렬화할 뿐, *다음 입력을 어느 스레드가 읽을지* 순서를 보장하지 않는다. 즉 좀비 스레드가 살아있는 동안 사용자가 입력하면, 그 입력을 좀비 스레드가 먼저 읽어갈 수 있다 — 좀비 스레드는 자신의 (이미 rx 가 drop 된) 죽은 채널로 `tx.send()` 를 시도하다 실패해 루프를 빠져나오지만, **이미 읽어버린 바이트는 유실된다**(새 `run_raw_bridge` 호출의 채널로는 절대 전달되지 않음). 결과적으로 재연결 1 회 이후 사용자 키 입력이 비결정적으로 유실될 수 있는 실제 기능 결함이다 — CPU 를 쓰지 않고 스택 메모리만 소모한다는 점은 여전히 사실이지만, 그것이 이 트레이드오프의 전부가 아니다. 후속 조치(stdin 리더를 취소 가능하게 만들거나 단일 영속 스레드로 통합)는 `.claude-workspace/todo/`(완료 후 삭제되는 작업 큐라 여기선 파일명을 고정 링크하지 않음)의 좀비 stdin 리더 TODO 참고. 완전 non-blocking stdin(플랫폼별 poll/self-pipe/`WaitForMultipleObjects`)은 크로스플랫폼 복잡도가 커 이번 스코프에서 배제했다.
- **loopback 직결**: 인라인 host 가 `127.0.0.1:PORT`/`localhost:PORT` 면 SSH 없이 직접 attach(동일 머신 다중 인스턴스 검증).

## 연결 생존 확인 (read timeout + heartbeat)

attach 스트림은 read timeout 이 없는 순수 blocking I/O 라 네트워크가 FIN/RST 없이 **조용히** 끊기면(케이블 단절·NAT 타임아웃 등) 소켓의 read 가 영원히 대기해 어느 쪽도 끊김을 감지하지 못한다. 아래는 이를 감지하는 배선이다 — 상수는 `crates/tasty-ipc/src/stream.rs` 의 `HEARTBEAT_INTERVAL`(5초)/`HEARTBEAT_TIMEOUT`(20초, interval 의 4배 — 일시적 jitter 로 인한 오탐 방지).

- **read timeout**: 서버(`tcp_ipc_server.rs::handle_stream_connection`)·GUI client(`attach_client.rs::start_gui_attach`)·CLI client(`tasty-cli/src/stream.rs::StreamConnection::open_with`) 3곳 모두 소켓에 `HEARTBEAT_TIMEOUT` read timeout 을 건다. `try_clone` 된 reader/writer 는 같은 소켓 옵션을 공유하므로 한 번만 걸면 양쪽 다 적용된다. CLI 는 핸드셰이크 ack/`attached(_workspace)` 디스크립터 대기(둘 다 `recv()`)에도 자동으로 적용된다.
- **`StreamTag::Ping`**: 빈 payload 의 keepalive 프레임(값 `2`, 코덱은 이전부터 예약돼 있었다). 수신측은 아무 처리도 하지 않는다 — `read_frame` 호출이 성공적으로 리턴하는 것 자체가 read timeout 을 리셋하므로, Ping 이든 실제 Data/Control 이든 도착만 하면 liveness 로 인정된다.
- **송신은 write 쪽이 idle 할 때만**: 서버의 write thread(`handle_stream_connection`)는 기존 `for frame in sink_rx` blocking iterator 대신 `sink_rx.recv_timeout(HEARTBEAT_INTERVAL)` 루프를 쓴다 — sink 에 실제 Data/Control 프레임이 흐르면 그게 곧 liveness 라 Ping 을 보내지 않고, `HEARTBEAT_INTERVAL` 동안 조용하면 빈 Ping 을 대신 흘린다. GUI/CLI(raw 브리지) client 는 각각 별도 heartbeat 스레드가 `HEARTBEAT_INTERVAL` 마다 무조건 Ping 을 보낸다(활성 트래픽 여부를 별도로 추적하지 않음 — 5바이트 프레임 오버헤드가 그 추적 비용보다 훨씬 싸다).
- **CLI dump 모드(mirror-dump/workspace-mirror-dump)는 client 발 heartbeat 이 없다**: `--dump-after` 기본 500ms 로 짧게 끝나 `HEARTBEAT_TIMEOUT`(20초) 안에서 항상 종료되고, 서버가 보내는 Ping 만으로 client 쪽 read timeout 은 충분히 방지된다. raw 브리지(대화형, 장시간 idle 가능)만 client 쪽 heartbeat 스레드를 띄운다.
- **양방향이 필요한 이유**: 서버 write thread 의 Ping 은 client 의 read timeout 을(idle mirror 뷰), client 의 Ping 은 서버의 read timeout 을(client 가 오래 아무 입력도 안 보내는 세션) 각각 갱신한다 — 한쪽만 보내면 반대 방향의 read loop 가 오탐 disconnect 된다.
- **timeout 만료 → 기존 disconnect 경로 재사용**: read timeout 으로 인한 `WouldBlock`/`TimedOut` io 에러는 서버의 `Err(_) => break`(`handle_stream_connection`)·GUI client 의 `Err(_) => disconnected.store(true, ...)`·CLI 의 채널 기반 끊김 신호(`run_mirror_dump`/`run_workspace_mirror_dump` 의 `mpsc::RecvTimeoutError::Disconnected`, `run_raw_bridge` 의 `RawEvent::ServerRecvErr` — 위 "SSH 터널" 절 참고)를 그대로 타 EOF 와 동일하게 처리된다 — 별도 sweep 스레드나 새 상태 없이, 아래 "mirror 세션 종료"·[`features/remote-attach`](../features/remote-attach/index.md) 의 "자동 해제" 가 조용한 네트워크 단절까지 커버하게 된다.
- **GUI heartbeat 스레드 정리**: `cleanup_mirror_workspace` 가 세션 종료 시(원격발이든 사용자 close 든) `sess.disconnected` 를 set 해, `writer: Arc<Mutex<TcpStream>>` 를 계속 붙들고 있는 heartbeat 스레드가 다음 tick 에 스스로 종료하게 한다 — 안 하면 세션이 정리된 뒤에도 소켓/스레드가 새는 leak.
- **버전 skew 리스크(완화 로직 없음, 의도적)**: read timeout(20초)이 걸린 이후부터 이 프로토콜을 도입한 버전이 적용된다. 구버전 프로세스(Ping 미전송)와 신버전 프로세스가 같이 떠 있는 상태(예: 호스트 재시작 없이 CLI/플러그인만 재배포)에서는 idle 상태의 mirror 세션이 20초 뒤 오탐 disconnect 될 수 있다 — 이 프로젝트는 단일 사용자 로컬 앱이라 그런 skew 창이 짧고 드물어 허용 가능하다고 판단했다. 향후 다중 버전 동시운영이 필요해지면 capability 협상 또는 프로토콜 버전 필드 도입을 재검토한다.

## 클라이언트측 비대칭("서버는 점유 유지, 클라이언트는 사라짐") 원인

read timeout 프로토콜(위 절) 반영 *이전에도* "네트워크가 불안정해지면 클라이언트 mirror
workspace 는 사라지는데 서버는 여전히 점유 상태로 남는" 비대칭이 관찰됐다. 서버측 무한 블록
문제는 서버가 자기 소켓에 read timeout 을 걸어 스스로 해소한다(위 절). 아래는 **클라이언트가
왜 서버보다 먼저 사라지고 있었는가**의 실측 확인 결과다.

- **원인**: SSH 원격 attach(`crates/tasty-cli/src/ssh.rs::push_common_opts`)는 최초 SSH 터널
  구현부터 `ServerAliveInterval=15` + `ServerAliveCountMax=3` 를 걸어왔다. 이는 **로컬 ssh
  자식 프로세스 자신의 keepalive** 다 — 원격 sshd 가 15초
  간격 keepalive 요청에 3회(최대 45초) 응답하지 않으면 로컬 ssh 프로세스가 스스로 종료한다.
  ssh 가 죽으면 그 프로세스가 열어둔 로컬 loopback 포트(`ssh -L` 의 local 소켓)도 함께 닫혀
  GUI/CLI client 의 `read`가 **EOF** 를 받는다 — 이건 이미 있던 "원격발 종료" 경로(위 "연결
  생존 확인" 참조)를 그대로 타므로 read timeout 프로토콜 없이도 동작했다. 원격 tasty 서버
  자신의 소켓은 이 keepalive 와 무관하게 열린 채로 남으므로(서버는 SSH 를 전혀 모른다 — 항상
  loopback 만 본다) 무한 블록됐다 — 이게 서버/클라이언트 비대칭의 실체다.
- **read timeout 프로토콜 반영 후의 위치**: 이 SSH self-kill 경로는 여전히 유효하지만,
  이제는 **두 독립 메커니즘 중 하나**일 뿐이다 — SSH 터널이 죽지 않고 데이터만 조용히
  막히는 경우(예: 같은 머신 loopback attach, 또는 SSH 는 살아있지만 tasty 프로세스 자체가
  멎은 경우)에도 `HEARTBEAT_TIMEOUT`(20초) 가 독자적으로 클라이언트를 깨운다. 즉 SSH
  self-kill 은 클라이언트 정리를 **더 빠르게**(최대 45초) 만들 뿐, read timeout 프로토콜의
  전제 조건이 아니다.
- **실측 검증(2026-07-20)**: `ps`/`kill` 로 실제 SSH 프로세스 종료를 관찰하는 대신(이 sandbox
  는 self-loopback SSH 인증 수단이 없어 재현 불가), 동일한 근본 시나리오("피어가 조용히
  응답을 멈춤")를 `SIGSTOP`으로 직접 재현했다. 격리된 debug 인스턴스(`--port-file` 커스텀)를
  loopback 으로 띄우고 `tasty debug attach
  <surface> --dump-after 30000`로 attach, 3초 뒤 서버 프로세스에 `SIGSTOP` → 클라이언트가
  `--dump-after` 로 요청한 30초를 다 기다리지 않고 **~21초**(≈`HEARTBEAT_TIMEOUT`)만에
  스스로 종료함을 확인(`AttachExit::Disconnected` 경로). `SIGCONT` 로 서버를 살려도 이미
  종료한 클라이언트는 영향 없음 — 정상 동작.
- **회귀 확인**: 서버가 건강한 상태에서 `HEARTBEAT_TIMEOUT` 보다 오래(raw 브리지, idle 25초+)
  열어둔 세션은 끊기지 않고 유지됨을 같은 방식으로 확인(client 발 heartbeat 스레드가
  서버측 read timeout 을 계속 갱신). 단, CLI **mirror-dump 모드**(`--raw` 미사용)는
  client 발 heartbeat 이 없어 `--dump-after` 를 `HEARTBEAT_TIMEOUT`(20초) 이상으로 주면
  **서버가 먼저** client 를 idle 로 보고 끊는다(서버도 20초 read timeout 을 걸기 때문) —
  이건 버그가 아니라 문서화된 설계(위 "연결 생존 확인"의 "CLI dump 모드는 client 발
  heartbeat 이 없다" 참조): 기본값 500ms 로 짧게 끝나는 검증 전용 모드라 20초를 넘기는
  사용은 애초에 지원 대상이 아니다.

## GUI 자동 재연결 스코프

silent disconnect 정리(`cleanup_mirror_workspace`) 자체는 heartbeat TTL 만료로도 자동 발동한다
(위 "연결 생존 확인"). 정리 이후 **자동으로 재attach 를 재시도하는 로직은 없다** — 의도적
결정이다: 조용히 재연결을 반복 시도하는 것보다, 정리까지만 자동으로 하고 재진입은 사용자가
결정하게 두는 쪽이 최소 동작이라 골랐다(자동 재연결이 필요해지면 CLI `--ssh` 의
`run_attach_ssh` 백오프 루프(아래)와 유사한 패턴을 별도로 설계할 수 있다).

- **트리거는 기본적으로 레벨(level)** — `src/app/auto_attach.rs::maybe_trigger_auto_attach`
  는 "활성 워크스페이스가 매핑 Some & `auto_attach_active` 에 없으면 트리거"를 **매 프레임**
  재평가한다. 예를 들어 이미 활성인 워크스페이스에 `attach_mapping` 을 방금 새로 설정하면
  (`tasty set workspace --ssh-profile ...`) 워크스페이스 전환 없이도 다음 프레임에 즉시
  트리거된다.
- **단, "재진입 대기(pending reactivation)" anchor 만 엣지(edge) 게이팅** — silent
  disconnect(원격발 EOF/force-detach/heartbeat TTL)로 `cleanup_mirror_workspace` 가 정리한
  anchor 는 `App.auto_attach_pending_reactivation` 에 들어간다(`from_disconnect=true` 로
  호출됐을 때만 — 아래 "mirror 세션 종료"의 두 트리거 종류 참고). 그 집합에 속한 anchor 는
  활성 워크스페이스 id 가 **직전 프레임과 달라진 프레임에서만**(`App.auto_attach_last_active_ws`
  로 직전 값을 들고 비교, 술어는 `is_reactivation_edge`) 트리거를 허용한다 — "이미 활성
  상태로 계속 남아있는 것"은 허용하지 않는다. 두 조건을 합친 최종 판정은
  `is_attach_trigger_allowed(pending_reactivation, current, previous)`(단위 테스트
  `new_mapping_triggers_immediately_without_transition`/
  `disconnected_anchor_waits_for_transition_before_retrigger`). 트리거에 성공(워커 spawn)
  하면 그 anchor 를 `auto_attach_pending_reactivation` 에서 제거한다.
  - **왜 신규 mapping 과 구분해야 하는가**: 앵커가 활성 상태로 남아있는지만으로 재연결
    억제를 판정하면, "disconnect 직후 그 워크스페이스를 계속 보고 있는 것"과 "그 워크스페이스에
    매핑을 막 새로 설정한 것"을 구분할 수 없다 — 둘 다 "워크스페이스 전환 없이 활성 상태"이기
    때문. 엣지를 모든 anchor 에 무차별 적용하면 후자(흔한 CLI 시나리오)까지 워크스페이스
    전환 전까지 트리거되지 않는 회귀가 생긴다.
- **`detach_orphaned_mirror_sessions` 와의 관계**: 간섭 없음. `apply_attach_client_output`
  (disconnect 정리, anchor 있으면 `Reconnecting` 전이 / anchor 없으면
  `cleanup_mirror_workspace(sess, from_disconnect=true)`)과
  `detach_orphaned_mirror_sessions`(사용자가 mirror ws 자체를 닫은 고아 세션 정리,
  `from_disconnect=false`)는 서로 다른 세션 식별 기준(전자는 `disconnected` 플래그, 후자는
  `local_workspace` 가 어느 창에도 없음)으로 동작하고, 둘 다 단일 스레드 메인루프에서
  순차 실행돼(레이스 없음) 겹치는 세션이 있어도 먼저 처리한 쪽이 세션을 vec 에서 제거해
  뒤쪽은 자연히 skip 한다. `from_disconnect` 플래그가 두 경로를 구분해 사용자가 mirror ws 를
  직접 닫은 경우는 `auto_attach_pending_reactivation`/`auto_attach_reconnect` 에 들어가지
  않는다(재진입 대기·재연결 스케줄 의미가 없음 — 사용자 스스로 걷어낸 것).

## 재연결 시 세션 상태 보존 (TODO 28)

TODO 27 의 backoff 재연결이 매번 `start_gui_attach` 로 완전 신규 mirror workspace/터미널을
만들면 scrollback 과 local id 가 재연결마다 사라진다. 이를 막기 위해 `reconnect_session`
(`src/app/attach_client.rs`)은 `resolve_endpoint`/handshake 는 `start_gui_attach` 와 동일하게
수행하되, 워크스페이스/터미널은 **기존 것을 그대로 재사용**한다:

- **`merge_survivor_mapping`**: 기존 `apply_mirror_structural_delta` 의 survivor-mapping
  로직(옛 remote_id→local_id 매핑과 새 원격 구조를 비교해 양쪽에 다 있는 surface 는
  local id/Terminal 인스턴스를 그대로 재사용)을 공용 함수로 추출해, `start_gui_attach`(빈
  old_map)·`apply_mirror_structural_delta`·`reconnect_session` 세 곳이 공유한다.
  `reconnect_session` 은 재연결 직전의 `remote_to_local` 매핑을 old_map 으로 넘겨 재연결
  전후 살아있는 surface 의 scrollback/local id 를 보존한다.
- **`SharedFrameSender`(`Arc<Mutex<FrameSender>>`)**: 입력 forwarder 스레드(`make_mirror_surface`
  가 만드는, surface 입력을 원격에 쓰는 장기 생존 스레드)는 연결이 바뀌어도(재연결로
  reader/writer/heartbeat 스레드와 소켓 자체가 통째로 교체된다) 살아있는 채로 새 연결의
  sender 를 봐야 한다. 그래서 `frame_tx` 를 값이 아니라 `Arc<Mutex<>>` 로 감싸 forwarder
  스레드에 공유하고, `reconnect_session` 은 이 Arc 는 그대로 두고 **내부의 raw sender만
  교체**한다. forwarder 스레드는 send 실패 시에도 (구 채널이 재연결 중 잠깐 죽어있는
  것일 수 있으므로) 스레드를 종료하지 않고 그 청크만 drop 하고 계속 루프한다 — Codex
  크로스체크가 지적한 "재연결 후 forwarder 가 죽은 채널을 계속 참조해 survivor 터미널
  입력이 조용히 유실되는" 결함의 수정.
- **`SessionState`(`Connected`/`Reconnecting`)**: 기존 `cleanup_mirror_workspace` 는
  disconnect 즉시 mirror workspace/터미널을 통째로 걷어내, TODO 27 의 재연결 트리거가
  발동할 시점엔 survivor-mapping 을 적용할 대상 자체가 남아있지 않았다(Codex 크로스체크
  지적). 이를 막기 위해 anchor 가 있는 세션의 disconnect 는 `cleanup_mirror_workspace`
  를 부르지 않고 `enter_reconnecting`(mirror workspace/터미널을 그대로 둔 채 상태만
  `Reconnecting` 으로 전이, `auto_attach_active` 에서 제거해 재연결 트리거가 자유롭게
  spawn 하게 함)으로 분기한다. anchor 가 없는 세션(임시 mirror)은 기존처럼 즉시
  `cleanup_mirror_workspace`.
- **레이스**: 재연결 워커가 엔드포인트 해석(`resolve_endpoint` — 프로필/포트 발견/SSH
  터널 수립)을 끝내고 결과를 메인 루프로 보내기 전에, 사용자가 그 mirror workspace 를
  직접 닫으면 `detach_orphaned_mirror_sessions` 가 먼저 세션을 정리해 vec 에서 제거한다.
  `drain_auto_attach_results` 는 그 시점의 `anchor_ws_id() == Some(anchor) && state() ==
  Reconnecting` 조건으로 세션을 다시 찾으므로, 이미 사라진 세션에 대해서는 매치가 없어
  `reconnect_session` 자체를 부르지 않고 no-op(성공 취급)으로 넘어간다 — 해석해둔
  터널 핸들은 그 자리에서 drop 되며(Drop 시 자식 ssh kill), 되살아난 연결이 이미 닫힌
  workspace 에 잘못 쓰이는 사고는 없다.

## mirror 세션 종료 (client → 원격 점유 해제)

client 가 mirror 를 걷어내면 원격에 `Detach` 를 보내 원격 점유(hard workspace lock)를 해제해야 한다. 종료 트리거는 두 가지다.

- **원격발 종료(EOF/force-detach/read timeout)**: reader 스레드가 `Detach`/`force_detached`/EOF 를 받거나(위 "연결 생존 확인" 참조) read timeout 으로 소켓 read 가 에러를 반환하면 `disconnected` 플래그가 선다. `apply_attach_client_output` 이 이를 상태별로 분기한다 —
  - **anchor 있는 세션(매핑된 워크스페이스)**: `cleanup_mirror_workspace` 를 부르지 않고 `enter_reconnecting` 으로 전이(위 "재연결 시 세션 상태 보존" 참고) — mirror workspace/터미널을 살려둔 채 `Reconnecting` 상태로 두고 backoff 재연결(TODO 27)에 맡긴다.
  - **anchor 없는 세션(임시 mirror, IPC `remote.attach` 등)**: 기존과 동일하게 세션 제거 + `cleanup_mirror_workspace(sess, from_disconnect=true)`.
  - 이미 `Reconnecting` 상태인 세션(재연결 시도 자체가 실패해 다시 disconnected 로 관측되는 경우)은 이 분기에 다시 들어오지 않는다 — `disconnected && state == Connected` 조건이라 진입 시 이미 걸러진다.
- **로컬발 종료(사용자 close)**: 사용자가 mirror 워크스페이스 **자체**를 닫으면(`close_workspace_at`, context menu/단축키) 로컬 ws 는 즉시 사라지지만 세션은 남는다 — 소켓이 열린 채라 원격은 계속 점유로 본다("사용 중" 잔류). 이는 세션이 `Connected`/`Reconnecting` 어느 쪽이어도 동일하다. `App::detach_orphaned_mirror_sessions`(`about_to_wait`)가 매 프레임 세션의 `local_workspace` 존재 여부(`find_main_with_workspace`)를 확인해, 없으면 고아로 보고 `cleanup_mirror_workspace` 로 정리한다. 세션 push 는 항상 ws 생성(같은 동기 함수) 뒤라 attach 셋업 중 false-positive 는 없다.
- **`cleanup_mirror_workspace`(공용, `from_disconnect: bool` 파라미터로 위 두 트리거 구분)**: mirror ws·터미널 제거(이미 없으면 skip) → 원격에 `Detach` push → anchor 게이트(`auto_attach_active`) 해제 → 터널 kill. `from_disconnect=true`(원격발, anchor 없는 세션 한정)일 때만 anchor 를 `auto_attach_pending_reactivation` 에 추가(위 "GUI 자동 재연결 스코프" 참고) — `false`(로컬발/사용자 close)는 그 항목과 `auto_attach_reconnect` 스케줄 모두 명시적으로 제거해 이미 걷어낸 세션에 대한 재연결 시도를 남기지 않는다. 원격은 `Detach` 수신 시 read loop break → `Disconnected` → `release_all_for_client`(workspace+surface lock 해제).

## force-detach

서버 권한으로 점유 강제 해제(attach 하지 않음). holder client 에 `Control{force_detached}` + `Detach` push → client 가 mirror 정리·종료, 서버는 lock free.

- IPC: `attach.force_detach{surface_id}` / `attach.force_detach_workspace{workspace_id}`.
- CLI: `tasty remote attach --force-detach`(surface) / `--workspace <id> --force-detach`(workspace).
- **`--ssh` + `--force-detach` 미지원**(에러): force-detach 는 *이 서버* 에 붙은 client 락을 끊는 로컬 JSON-RPC 이지 터널 너머 원격 서버의 락을 끊는 게 아니다.

## 원격 생존 확인 (`remote check`)

`crates/tasty-cli/src/commands/remote_check.rs`. 포트 발견만으론 **stale 포트 파일**(죽은 인스턴스 잔재)을 오판하므로 3단계: ① 포트 발견 → ② `ssh -L` 터널 → ③ 터널 localport 로 `system.info` **1회**. 응답이 와야 alive(stdout `alive: …`, exit 0). 거부/EOF/타임아웃(5초) = dead(stderr, exit≠0). 1회성이라 재연결 백오프 없음. SSH 부품은 attach 와 공유, list 핸들러는 안 건드림.

## IPC 표면 (`attach.*`)

`src/adapters/ipc/handler/attach.rs`.

| 메서드 | 경로 | 설명 |
|--------|------|------|
| `attach.acquire` / `attach.release` | `stream.open{target}` 핸드셰이크 | 배타 lock 획득/해제 |
| `attach.force_detach` / `attach.force_detach_workspace` | JSON-RPC | 점유 강제 해제 |
| `attach.into_gui` | JSON-RPC | 실행 GUI 가 원격 워크스페이스를 mirror 로 재구성(`App::start_gui_attach`) |
| `attach.list` | JSON-RPC | 현재 점유 목록 조회(read) |

**권한**: attach 보안은 **연결 경계(SSH + 127.0.0.1 loopback)** 에 위임(decision 5). 자체 권한 레이어 없음 — 소켓에 도달한 caller 는 attach 제어를 호출할 수 있다. 근거는 [identity §2](../identity.md), 주체 계약은 [actors](../concepts/actors.md).

## 관련

- 결정 근거(원격 대상·로컬 debug 격리): [`ADR-0007`](../adr/0007-attach-targets-remote.md)
- 동작·점유 규칙·CLI/IPC 사용법: [`features/remote-attach`](../features/remote-attach/index.md)
- 주체(원격 사용자)·점유 모델 개념: [`concepts/actors`](../concepts/actors.md)
- SSH 프로필 관리: [`features/ssh-tool`](../features/remote-profiles/index.md)
- 로컬 self attach 격리: [`dev-guide/debug-ipc`](debug-ipc.md)
</content>
