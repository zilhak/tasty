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
- `release` (holder 본인) / `force_detach` (서버 권한) / `release_all_for_client`(EOF 시 일괄 — workspace + 멤버 + 잔여 surface).
- **입력 격리**: `apply_send_to_surface` 가 `is_hard_occupied` 면 서버 로컬 입력 거부, client 입력만 `feed_attached_input` 우회 경로로 PTY 도달. (soft 점유는 write 를 막지 않는다 — hard 만 격리.)

## 초기 스냅샷 + delta

attach 직후 서버가 현재 visible 화면을 `snapshot_as_vt` 로 **1회** 직렬화 push(셀 속성 + 커서 + alt-screen/DECCKM/bracketed 모드 복원). 이후 변화는 output tap delta(Data 프레임). client 는 받은 바이트를 PTY 없는 mirror 터미널(`Terminal::new_detached` + `feed_bytes`)에 먹여 같은 termwiz 파서로 grid 재구성.

## workspace mux

한 연결로 N 터미널 출력을 나르므로 workspace 모드 Data 프레임은 **surface-prefixed**(`encode_mux`/`decode_mux`). surface 단위 단일 연결은 prefix 없음. attach 직후 서버가 `attached_workspace` Control 로 트리(분할 방향/비율) + per-surface(`{remote_id, role, cols, rows, kind}`)를 보내고, client 는 원격↔로컬 surface_id 재매핑으로 트리를 재구성한다.

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
- **`StreamControl` 확장성**: `event` 태그 기반 enum. mid-session 이벤트는 새 `StreamTag` 없이 variant 로 추가한다 — 현재 `Resize`(server→client, 확정 echo), `Activity`(server→client, busy/idle 활동 상태), `ClientResize`(client→server, geometry 구동 요청), `StructuralOp`(client→server, 구조 변경 forward), `StructuralResult`(server→client, forward 회신), `StructuralDelta`(server→client, 구조 역반영). 알 수 없는 event 는 역직렬화 실패로 무시(전방/후방 호환) — 구버전 서버는 `ClientResize` 를 무시해 기존 remote-authoritative 로 graceful degrade 한다. 단발 핸드셰이크 디스크립터(`attached`/`attached_workspace`/`attach_error`)와 `force_detached` 는 여전히 ad-hoc JSON 으로 위치 기반 파싱.

## 활동(busy) 상태 전파

사이드바 워크스페이스 리스트의 "실행 중" status dot(`WorkspaceEntryView.busy_count`, `docs/features/remote-attach/index.md` "GUI mirror" 참조)은 surface 의 busy/idle 상태를 본다. mirror(detached) 터미널은 로컬 PTY 가 없어 `CoreState::refresh_busy_surfaces`(foreground-process 폴링, `src/core/state/busy.rs`)가 절대 채울 수 없으므로, **원격이 직접 자기 surface 의 busy 상태를 계산해 client 로 forward** 한다 — resize 의 client-driven 협상과 반대로, busy 는 순수 **server→client** 단방향이다(원격 foreground 프로세스 이름은 애초에 client 에 존재하지 않는 정보라 client 가 요청할 수도 없다).

- **서버측 계산·forward**: 서버는 1Hz busy-poll tick(gui `app/busy.rs` 의 `poll_busy_states`, headless `boot.rs` 의 `AppEvent::BusyPoll` — 아래 참조)마다 `CoreState::forward_busy_activity`(`core/attach_runtime.rs`)를 호출한다. 이 메서드는 `busy_activity_forwards`(`core/state/busy.rs`)가 계산한 **점유 중인 surface 의 busy 값 변화분**만 `StreamControl::Activity{surface_id, busy}` 로 그 workspace/surface 의 holder client 에 push 한다. `last_forwarded_busy` 캐시로 값이 실제로 바뀐 경우에만 forward(중복 억제)하되, surface 가 점유 해제됐다가 재점유되면(다른 client 일 수 있음) 캐시를 버려 값이 이전과 같아도 항상 fresh 하게 1회 다시 push한다 — resize 의 `last_forwarded_resize` dedup 과 동형이나 방향이 반대(client→server 아닌 server→client)다.
- **headless 는 자체 ticker 필요**: headless 메인 루프(`boot::run_headless`)는 `rx.recv()` 로 이벤트를 기다리기만 할 뿐 자체 타이머가 없다 — gui 의 `boot::busy_tick`(winit `EventLoopProxy` 기반, `#[cfg(feature = "gui")]`)이 없으면 `AppEvent::BusyPoll` 이 영원히 발화하지 않는다. `HeadlessWaker::spawn_busy_ticker`(`adapters/production/headless_waker.rs`)가 같은 1Hz cadence 로 `mpsc::Sender<AppEvent>` 에 `BusyPoll` 을 직접 보내 이를 미러링한다. **원격 attach 의 주 시나리오가 headless 서버**이므로 이 ticker 가 없으면 활동 상태 forward 가 전혀 동작하지 않는다.
- **client 적용**: reader 스레드가 `Activity` Control 프레임을 `MirrorEvent::Activity(remote_surface_id, busy)` 로 버퍼링하고, `apply_attach_client_output` 이 세션의 `remote_to_local` 매핑으로 로컬 mirror surface id 를 찾아 `CoreState::set_mirror_surface_busy` 를 호출한다. 이 값은 **`busy_surfaces`(로컬 폴링 결과)와 분리된 `mirror_busy_surfaces` 별도 집합**에 저장된다 — 같은 집합에 합쳤다면 1Hz `refresh_busy_surfaces` 가 매 tick 로컬 폴링 결과로 집합을 통째로 교체하며 mirror 값을 지워버렸을 것이다. `is_surface_busy`/`any_busy`/`busy_count`(사이드바·상태바·`surface.list` IPC 가 공유하는 단일 진입점)는 두 집합의 합집합을 본다.
- **정리**: mirror surface 가 없어지면(`cleanup_mirror_workspace`, `apply_mirror_structural_delta` 의 removed 처리) `CoreState::forget_mirror_surface_busy` 로 `mirror_busy_surfaces` 에서도 제거해, 로컬 id 가 재사용될 때 stale busy 값이 새 surface 에 잘못 붙는 것을 막는다.

## mirror 구조 변경 forward

mirror 워크스페이스의 구조 변경(split/new-tab/close/move-tab)은 로컬에서 실행하지 않고 원격(authoritative)에서 실행되도록 forward 한다. intent 로 표현되는 경로(split·CLI/IPC)는 `Core::apply` 로 수렴해 거기서 forward 큐에 쌓이고, `Core::apply` 를 우회하는 UI-layer 직접 조작(`close_active_*`/`close_tab`/`add_tab`/`add_kind_tab`, 탭 드래그·컨텍스트 메뉴 이동)은 `AppState::forward_mirror_structural` 가 같은 큐(`pending_structural_forward`)에 대응 op 를 직접 쌓는다 — 두 경로 모두 동일 큐로 모여 아래 결선을 공유한다. 개념·정책은 [features/remote-attach](../features/remote-attach/index.md#mirror-워크스페이스-내-구조-변경), 여기엔 결선만.

- **request (client→server)**: `Core::apply` 가 mirror 구조 op 를 로컬 차단(`MirrorStructuralBlocked`)하면서 `StructuralOp`(anchor = **로컬** surface id)를 `CoreState.pending_structural_forward` 에 push. `App::dispatch_pending_structural_forwards`(`about_to_wait`, gui)가 drain 해 anchor 를 세션 매핑(`AttachClientSession.remote_to_local` 역방향)으로 **원격 id 로 치환**(`StructuralOp::with_anchor_surface_id`)한 뒤 `StreamTag::Control` 로 전송. anchor 를 surface id 로 잡는 이유: client 는 pane/tab 의 원격 id 매핑을 갖지 않으므로, 원격이 surface 로부터 pane/tab/workspace 를 자기 트리에서 resolve 한다.
- **진입 경로별 응답 정합성**: mirror 구조 op 는 사용자 GUI·에이전트 CLI/IPC 어느 경로든 `Core::apply` 로 수렴해 forward 된다(에이전트의 원격 mirror 조작은 금지가 아니라 정식 기능 — 원칙2). `MirrorStructuralBlocked` 의 `forwarded` 플래그가 "로컬 차단했으나 원격으로 큐잉됨" 을 표시하며, 두 경로가 이를 **성공으로** 처리한다: GUI 는 `report_apply_error` 가 `forwarded` 마커를 삼켜 차단 toast 를 띄우지 않고(성공 무음, 실패는 회신 toast), IPC/CLI 는 구조변경 핸들러(split·tab.create/close/move·pane.close·surface.close)가 `structural_apply_error`(`adapters/ipc/handler.rs`)로 `{forwarded:true, workspace_index}` **success** 를 회신한다. forward 는 비동기라 이 응답은 "원격에 큐잉됨" 을 뜻하고 실제 결과는 역반영 delta(아래)로 관측한다 — 성공을 `internal_error` 로 오보하지 않는다. `forwarded:false`(convert/move-surface 등 forward 불가 op)는 기존대로 에러.
- **실행 (server)**: `StreamHub::pump_inbound` 이 client Control 프레임을 `StructuralOp` 로 분류(`PumpOutcome.structural_ops`) → 메인루프(gui `event_handler`/headless `boot`)가 anchor 워크스페이스의 **holder 를 검증**(hard 점유 = 구조 변경 권한, ADR-0040)한 뒤 `execute_forwarded_structural_op`(`core/attach_runtime.rs`)로 기존 IPC 핸들러(split/tab.create/tab.close/tab.move/pane.close/surface.close)를 재사용해 실행. 서버 ws 는 mirror 가 아니라 `Core::apply` 가드를 통과해 실제 PTY 를 spawn 한다.
- **회신 (server→client)**: 실행 결과를 `StructuralResult{op_id, ok, reason?}` 로 push. 원격 미등록 kind 는 `create_surface_via_registry` 가 Err → `ok:false`+reason. client reader 가 실패 회신을 `MirrorEvent::StructuralFailed` 로 올려 실패 toast. 성공은 무음.
- **역반영 (server→client)**: 성공한 forward 는 원격에 생긴/사라진 surface 를 mirror 트리에 반영한다. `execute_forwarded_structural_op` 이 실행 **전/후** anchor 워크스페이스의 `all_surface_ids` diff 로 added(신규 터미널)를 계산하고, 실행 후 전체 트리+surfaces(`build_workspace_tree_surfaces`, 핸드셰이크 디스크립터와 공유)를 `StructuralDelta{workspace_id, tree, surfaces}` 로 만든다. 순서는 **`StructuralResult` → `StructuralDelta` → 신규 surface tap**(`tap_surface_for_stream`; client 가 매핑을 만든 뒤 스냅샷을 받도록 tap 을 delta 다음으로). 핸들러 응답 파싱이 아니라 트리 diff 라 close cascade·move-tab 도 균일 커버. client(`apply_mirror_structural_delta`)는 surfaces 를 세션 매핑과 대조해 survivor(기존 remote_id)는 **로컬 id 재사용**(터미널 재생성 금지 → scrollback 보존), 신규는 `Terminal::new_detached`+입력 forwarder, 사라진 것은 제거한 뒤 `build_mirror_workspace` 를 재실행해 같은 local ws id 로 in-place 교체한다. pane 상위 배치는 핸드셰이크와 동일한 방식(`to_attach_tree_json`의 `pane_layout` 트리 필드, direction/ratio 보존)으로 정확히 승계된다(3단계도 동일 정합성).
- **focus 보존 (client-only 보정)**: 위 트리 재구성이 실어 보내는 `tree.focused_pane`/pane 별 `active_tab` 은 **원격의** 값이다 — 순수 pane/tab 전환(클릭·키보드 이동)은 forward 되는 `StructuralOp` 가 없으므로(위 목록에 없음), 원격의 이 값은 대개 워크스페이스 생성 시점(첫 pane/첫 탭)에 고정된 채 절대 바뀌지 않는다. 과거엔 매 delta 마다 이 고정값을 그대로 실어 로컬 워크스페이스를 통째로 교체해, 사용자가 로컬에서만 다른 pane/탭으로 이동해둔 focus 가 구조 변경(새 탭/닫기 등) 때마다 첫 pane/첫 탭으로 되돌아가는 버그가 있었다. `apply_mirror_structural_delta` 는 교체 **전** 로컬에서 실제로 focus 돼 있던 surface 를 remote id 기준으로 캡처(`capture_focused_remote` — local pane/tab id 는 매 delta 마다 재발급돼 불안정하므로 remote surface id 를 기준으로 삼는다)해뒀다가, 교체 **후** 새 트리에서 그 surface 의 위치를 찾아(`find_pane_and_tab_for_surface`) `ws.focused_pane`/해당 `Pane.active_tab`/그 `Tab.focused_surface` 를 되돌린다(`restore_focus_after_delta`). 캡처한 surface 자체가 이번 op 로 사라졌으면(예: 그 surface 를 닫은 `CloseSurface`) 복원을 시도하지 않고 원격 값 그대로 둔다 — 서버 상태는 전혀 건드리지 않는 순수 client-side 보정이다.
- **미구현**: convert/move-surface 는 재사용할 원격 IPC 핸들러가 없어 아직 forward 아닌 차단(`build_mirror_forward_op`·`execute_forwarded_structural_op` 모두 미지원). 그 외 구조 변경(split/new-tab/close/move-tab)은 intent 경로든 UI 직접 조작 경로든 모두 forward 된다.

## SSH 터널 (원격 client 공통)

`crates/tasty-cli/src/ssh.rs` — `remote attach` 와 `remote check` 가 공유.

- **시스템 ssh 위임**: 자체 암호화 없이 시스템 `ssh` 를 자식 프로세스로 실행. 사용자 `~/.ssh/config`·agent·known_hosts 재사용. **Windows 는 시스템 OpenSSH 풀경로**(`%WINDIR%\System32\OpenSSH\ssh.exe`) 우선 — git 번들 ssh 는 윈도우 ssh-agent(named pipe)를 못 봐 무암호 인증 실패.
- **원격 포트 발견**: 기본 `auto` = subcommand → file-unix → file-windows 순서로 원격 DefaultShell 4종(PowerShell/cmd/git bash/unix) 커버. `--remote-port-mode` 로 고정, `--remote-tasty <path>` 로 원격 바이너리 경로(기본 `tasty`).
- **터널 생명주기**: detach/종료 시 자식 ssh kill(고아 터널 방지)하되 **원격 데몬은 생존**(server-owns-PTY persistence = detach 의 본질). 자동 재연결(attach 한정): 지수 백오프(0.5s→30s)로 터널+attach 재수립(`--no-reconnect` 로 끔).
- **loopback 직결**: 인라인 host 가 `127.0.0.1:PORT`/`localhost:PORT` 면 SSH 없이 직접 attach(동일 머신 다중 인스턴스 검증).

## mirror 세션 종료 (client → 원격 점유 해제)

client 가 mirror 를 걷어내면 원격에 `Detach` 를 보내 원격 점유(hard workspace lock)를 해제해야 한다. 종료 트리거는 두 가지이고 정리 경로(`cleanup_mirror_workspace`)를 공유한다.

- **원격발 종료(EOF/force-detach)**: reader 스레드가 `Detach`/`force_detached`/EOF 를 받으면 `disconnected` 플래그 → `apply_attach_client_output` 이 세션 제거 + `cleanup_mirror_workspace`.
- **로컬발 종료(사용자 close)**: 사용자가 mirror 워크스페이스 **자체**를 닫으면(`close_workspace_at`, context menu/단축키) 로컬 ws 는 즉시 사라지지만 세션은 남는다 — 소켓이 열린 채라 원격은 계속 점유로 본다("사용 중" 잔류). `App::detach_orphaned_mirror_sessions`(`about_to_wait`)가 매 프레임 세션의 `local_workspace` 존재 여부(`find_main_with_workspace`)를 확인해, 없으면 고아로 보고 `cleanup_mirror_workspace` 로 정리한다. 세션 push 는 항상 ws 생성(같은 동기 함수) 뒤라 attach 셋업 중 false-positive 는 없다.
- **`cleanup_mirror_workspace`(공용)**: mirror ws·터미널 제거(이미 없으면 skip) → 원격에 `Detach` push → anchor 게이트 해제 → 터널 kill. 원격은 `Detach` 수신 시 read loop break → `Disconnected` → `release_all_for_client`(workspace+surface lock 해제).

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
