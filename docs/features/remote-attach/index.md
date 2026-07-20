# 원격 attach (Remote attach)

- **Status**: Implemented
- **주체**: 원격 접속 사용자(점유 후 조작) · AI Agent(원격 mirror 를 정당한 행동으로 attach) · 로컬 사용자(force-detach 권한)
- **ADR**: [ADR-0007](../../adr/0007-attach-targets-remote.md) (attach 는 원격 대상 · 로컬 self-attach 는 debug 격리). 보안 위임 근거는 [ADR-0004](../../adr/0004-ipc-transport-tcp.md) (loopback trust boundary)
- **코드**: `src/core/attach.rs`(`OccupancyRegistry`), `src/core/attach_runtime.rs`, `src/app/auto_attach.rs`, `src/adapters/ipc/handler/attach.rs`, `src/app/ipc/app_methods.rs`(`remote.workspaces`/`remote.attach`), CLI `crates/tasty-cli/src/remote_browse.rs`, `crates/tasty-cli/src/commands/{remote,attach,remote_check,remote_workspaces}.rs`
- **화면**: [screens/remote-attach.md](screens/remote-attach.md)

## 목적

다른 호스트(또는 동일 머신의 다른 인스턴스)의 surface/workspace 를 **점유(occupy)** 해 mirror 로 보고 조작하는 기능. tasty 는 자체 원격 프로토콜·암호화를 만들지 않고 **SSH 에 위임**한다 — "그 호스트에 SSH 로 들어올 수 있는 사람 = attach 자격"(tmux 모델). 원격 접속 사용자가 tasty 를 쓰는 유일한 경로이며, [actors](../../concepts/actors.md) 의 **점유 모델**이 실제로 구현되는 지점이다.

## 내부 동작 (headless-valid)

### 점유 (Occupation)

attach 의 본질은 **강한(hard) 배타 점유**다 — [ADR-0040](../../adr/0040-occupancy-soft-hard-tiers-agent-occupant.md) 2계층 점유 중 강한 점유 계층의 사례다(약한 점유는 [child-terminal](../child-terminal/index.md)). 개념 정의는 [actors 점유 모델](../../concepts/actors.md#점유-occupation-모델), 여기선 동작:

- **배타 lock**: 한 surface 는 한 client 만 점유한다(`OccupancyRegistry`). 점유는 `stream.open{target}` 핸드셰이크의 `attach.acquire` 로 잡고, 동시 attach 는 holder 정보를 담아 `already_attached` 로 거부.
- **점유 중 격리**: 점유된 surface 의 서버 로컬 입력(GUI 키 / `surface.send`)은 차단되고, **점유 client 입력만** PTY 에 도달한다. 로컬 사용자·AI Agent 는 그 대상에 대해 **readonly** — 내용은 보이되 조작은 막힌다. readonly 는 PTY/TUI 조작(키 입력·마우스 트래킹 보고·휠 스크롤·Ctrl+click 링크 열기)만 차단하는 것이고, **드래그로 텍스트를 선택해 클립보드로 복사하는 tasty 자체 기능은 예외적으로 계속 동작**한다 — PTY 에 아무것도 보내지 않는 순수 로컬 UI 동작이기 때문이다(좌표·복사 텍스트는 실제 렌더되는 mirror 기준). 근거: [ADR-0049](../../adr/0049-hard-occupancy-selection-exception.md).
- **자동 해제**: client 연결 종료(EOF) 시 lock 이 free 로 환원. 점유는 **휘발성** — 서버 재시작 시 전부 free(영속 안 함).
- **force-detach**: **로컬 사용자만** 점유를 강제로 끊을 수 있다(서버 권한). 끊으면 holder client 에 종료를 통지하고 대상은 **일반 surface/workspace 로 복귀**.

### surface 단위 vs workspace 단위

- **surface attach**: 단일 터미널 surface 를 mirror. 한 연결 = 한 터미널.
- **workspace attach**: 워크스페이스를 점유하면 그 안 **모든 터미널 surface 를 트리째 mirror**(분할 방향/비율 포함)하고, **비-터미널 surface**(markdown/image/explorer 등)는 mirror 불가라 placeholder 로 숨긴다. workspace lock 은 멤버 터미널 전부를 surface lock 에도 등록하므로, 멤버가 이미 다른 client 에 점유돼 있으면 workspace attach 를 **거부**(부분 점유 충돌 방지).

### 화면 동기화

attach 성립 직후 서버가 현재 화면을 **1회 스냅샷**으로 push 하고, 이후 변화는 delta 로 흐른다. client 는 PTY 없는 mirror 터미널에 바이트를 먹여 같은 grid 를 재구성한다. 프로토콜·mux 상세는 → [dev-guide/attach-behavior](../../dev-guide/attach-behavior.md).

**그리드 크기는 client 가 구동(client-driven)** — mirror 의 cols×rows 는 그것을 띄운 **로컬 pane 크기**를 따르고, 원격 PTY 를 그 크기로 reflow 시킨다(ADR-0045). "remote authoritative" 는 메커니즘으로만 남는다: 원격 PTY 가 실제 크기의 단일 진실원이라 콘텐츠 래핑(reflow)을 담당하고, 그 확정 크기를 client 에 되돌린다. client 는 **의도를 밀고(요청)** 원격은 **결과를 확정(echo)** 한다. 구현:

- mirror 는 detached 터미널(PTY 없음)이라 로컬 레이아웃 리사이즈 스윕(`Core::resize_all_terminals` / `AppState::resize_all`)이 detached 터미널을 로컬에 직접 적용하지 않고, 목표 grid 를 forward 큐에 넣는다. `about_to_wait` 에서 **`StreamControl::ClientResize`**(client→server) 로 원격에 요청한다.
- 서버가 그 surface 의 **실제 원격 PTY 를 요청 크기로 resize**(reflow)한다(holder 만 구동 가능 — 배타 점유라 구동자는 항상 유일). 서버가 **GUI 인스턴스**(창 보유)면 그 host 창의 레이아웃 sweep(`Core::resize_all_terminals`)이 **hard-점유 surface 를 skip**(`is_hard_occupied`)해 자기 창 grid 로 되돌리지 않는다 — skip 이 없으면 host 창이 client-driven grid 를 덮어써 mirror 가 host 창 크기에 고정된다(레터박스). headless 서버는 창이 없어 무해하지만 GUI-hosted 서버엔 필수. detach 시 원복.
- 원격 grid 가 실제로 바뀌면 서버가 기존 **`Control` 프레임(`StreamControl::Resize`, server→client)** 으로 확정 cols/rows 를 통지하고, client 가 그 echo 로만 mirror 를 리사이즈한다. → 로컬을 낙관적으로 먼저 바꾸지 않아(원격 reflow 전 잘못된 grid 재생 방지) desync 가 없다.
- 렌더러는 mirror 의 실제 grid 크기로 셀을 pane 좌상단에 배치한다. mirror 가 pane 크기로 reflow 되므로 pane 을 채운다(과거의 80×24 좌상단 소영역 + 배경 레터박스는 사라진다). 초기 attach 순간(원격 기본 80×24 → 첫 forward reflow)에는 약 1 RTT 의 짧은 깜빡임이 있을 수 있다.

`StreamControl` 은 `event` 태그 기반 확장 enum 이라, 새 이벤트도 새 `StreamTag` 없이 variant 로 추가된다. 구버전 서버는 `ClientResize` 를 무시하므로(전방호환) 기존 remote-authoritative 동작으로 graceful degrade 한다.

### 모드

- **mirror-dump**(`--dump-after <ms>`): N ms 출력을 수집해 grid 를 재구성하고 텍스트를 stdout 으로 출력 후 종료. GUI 없이 attach 파이프라인을 검증하는 경로.
- **raw 브리지**(`--raw`): stdin↔stdout passthrough(detach 키 `Ctrl+\`). **surface 단위만** — workspace 모드는 다중 터미널이라 불가.
- **단발 입력**(`--send <str>`): attach 직후 1회 입력(escape 디코딩). workspace 모드는 `--send-to <remote_surface_id>` 로 대상 지정.

### GUI mirror

`tasty remote attach --into-gui --target-port <원격포트> --workspace <원격ws>` → 이 명령을 받은 **로컬 GUI 인스턴스**가 client 가 되어 원격 워크스페이스를 mirror 로 재구성한다(`attach.into_gui`). mirror Workspace 는 일반 워크스페이스로 사이드바에 노출되되 **이름과 subtitle 사이 별도 줄의 하늘색 "REMOTE" pill**(`>_→` glyph 포함; collapsed 레일은 아바타 우하단 하늘색 corner chip)로 로컬과 구분(`Workspace.mirror`). status dot 은 실행상태(running/idle) 전용이며 mirror 색을 싣지 않는다 — 원격 origin 은 별도 시각 축(디자인 `workspace-mirror-fg`, notif=우상단 / attached=둘레 ring 과 채널 분리). mirror 콘텐츠(grid) 갱신은 원격 출력이 올 때 즉시, 3초 tick 은 backstop. 실행상태(status dot 의 초록/회색)는 별도 채널로, 원격이 1Hz 로 자신의 busy 상태를 계산해 attach 스트림으로 forward 하고(mirror 터미널은 로컬 PTY 가 없어 스스로 계산할 방법이 없다 — 이 forward 가 유일한 소스) client 가 그 값을 반영한다. 메커니즘 상세는 [dev-guide/attach-behavior "활동(busy) 상태 전파"](../../dev-guide/attach-behavior.md#활동busy-상태-전파).

### mirror 워크스페이스 내 구조 변경

mirror 워크스페이스는 "통째로 원격" 인 원격 워크스페이스의 뷰다 — 입력(키스트로크)은 이미 원격 PTY 로 forward 된다. 그 안에서의 **구조 변경**(surface/pane split · 새 탭 · 닫기 · 탭 순서 변경)을 로컬에서 실행하면 로컬 셸 PTY 가 mirror 에 섞여 "workspace 전체가 remote" 불변식을 깬다. 따라서 mirror 워크스페이스 구조 변경은 **로컬에서 실행하지 않고**, 대신 **원격 인스턴스에서 실행되도록 forward** 한다.

- **판별·로컬 차단**: 단일 mutate 진입점 `Core::apply` 가 대상 워크스페이스가 mirror 면 로컬 실행을 거부(`MirrorStructuralBlocked`, `CoreState::mirror_workspace_index_for_structural`) — 로컬 트리/PTY 는 절대 바뀌지 않는다. `Core::apply` 를 우회하는 UI 직접 조작(`AppState::add_tab`/`add_kind_tab`/`close_active_*`/`close_tab`, 탭 드래그·컨텍스트 메뉴 이동)은 `AppState::forward_mirror_structural` 가드가 로컬 실행 대신 대응 `StructuralOp`(new-tab→`NewTab`, close→`CloseSurface`/`CloseTab`/`ClosePane`, 순서변경→`MoveTab`; anchor = focused/대상 pane 의 로컬 surface id)를 같은 forward 큐(`pending_structural_forward`)에 직접 쌓아 원격으로 보낸다 — 로컬 차단만 하던 과거와 달리 `Core::apply` 경로와 동형으로 forward 된다.
- **forward (요청)**: `Core::apply` 가 로컬 차단과 동시에 그 구조 op 를 `StructuralOp` 로 만들어(anchor = **로컬** surface id) forward 큐에 넣고, App 이 `about_to_wait` 에서 drain 해 anchor 를 **원격 surface id 로 치환**한 뒤 attach stream 의 `StreamTag::Control`(`StreamControl::StructuralOp`)로 원격에 보낸다. attach 연결 자체가 hard 점유 holder 이므로 연결이 곧 구조 변경 권한을 증명한다(ADR-0040). op 는 **원격 surface id 로 anchor** 되어 원격이 자기 트리에서 pane/tab/workspace 를 resolve 한다 — client 는 surface 매핑만 보유하면 된다.
- **원격 실행**: 원격이 `StructuralOp` 를 수신(`StreamHub::pump_inbound` 분류)해 holder 를 검증한 뒤, 기존 IPC 핸들러(split/tab.create/tab.close/tab.move/pane.close/surface.close)를 재사용해 실제로 실행한다(원격 ws 는 mirror 가 아니라 실제 PTY 를 spawn). 결과는 `StreamControl::StructuralResult{op_id, ok, reason?}` 로 회신.
- **점유 상속 (필수 불변식)**: forward 로 원격에 **새로 생긴 터미널은 그 workspace 의 hard 점유를 상속**한다("workspace 전체가 remote" 유지 — ADR-0040 은 점유가 surface 생성 방식과 무관함을 못박는다). 점유는 attach 시점 멤버 스냅샷으로 끝나는 게 아니라, 구조 변경으로 늘어난 멤버까지 확장돼야 한다. `execute_forwarded_structural_op` 이 added 터미널을 `OccupancyRegistry::add_workspace_member` 로 `surface_locks`(→`is_hard_occupied`: 서버 입력차단·resize sweep skip·readonly) + `surface_to_workspace`(→`feed_attached_workspace_input`/`apply_attached_workspace_resize` 의 holder 검증)에 같은 holder 로 등록한다. 이 등록이 빠지면 새 surface 가 비점유로 남아 (1) host 창 sweep 이 자기 grid 로 되돌리는 레터박스 (2) 점유 미표시 (3) client 입력·resize 거부가 발생한다.
- **실패 회신**: 원격이 op 를 실패 처리(대표적으로 **원격에 등록되지 않은 plugin surface kind** — 원격의 kind 레지스트리가 그 호스트에서 생성 가능한 kind 의 authority)하면 `ok:false`+`reason` 을 회신하고, client 가 실패 toast(`attach.toast.mirror_structural_forward_failed`)를 띄운다. 요청/응답이라 실패 시 로컬·원격 어느 쪽도 구조가 바뀌지 않는다.
- **역반영 (성공 시)**: 성공한 forward 로 원격에 생긴/사라진 surface 를 mirror 트리에 반영한다. 원격이 실행 후 워크스페이스 **전체 트리+surfaces** 를 `StreamControl::StructuralDelta` 로 push(`StructuralResult` 성공 회신 **직후**)하고, client 가 이를 받아 mirror 트리를 증분 재구성한다. survivor(이미 mirror 로 존재하는 원격 surface)는 **기존 mirror 터미널을 그대로 유지**(scrollback 보존)하고, 새 원격 surface 만 새 mirror 로 추가, 사라진 surface 는 제거한다. 최소 증분(surface 별 diff) 대신 full-tree 재동기화를 쓰는 이유: client 는 surface 매핑만 보유한다는 불변식을 지키면서 split·새 탭·닫기(cascade)·탭 이동을 균일하게 반영하기 위함. pane 상위 배치(direction/ratio)도 핸드셰이크와 동일한 트리 필드로 정확히 승계된다.
- **focus 는 원격이 아니라 client 가 보존한다**: 위 역반영 트리가 담는 focus(어느 pane/탭이 focused 인지)는 원격 값 그대로다 — 순수 pane/탭 전환은 forward 되지 않으므로 원격의 focus 는 사실상 워크스페이스 생성 시점에 고정돼 있다. 매 역반영마다 이 고정값으로 로컬을 통째로 교체하면 사용자가 mirror 안에서 실제로 보고 있던 pane/탭이 매번 첫 pane/첫 탭으로 튀는 문제가 있었다. client 는 교체 직전 로컬 focus 위치를 remote surface id 기준으로 기억해뒀다가 교체 직후 그 위치로 되돌린다(그 surface 자체가 이번 op 로 없어졌으면 복원하지 않음) — 메커니즘 상세는 [dev-guide/attach-behavior "focus 보존"](../../dev-guide/attach-behavior.md#mirror-구조-변경-forward).
- **mirror 워크스페이스 자체를 닫는 것**은 로컬 mirror 뷰를 걷어내는 정당한 로컬 동작이라 차단·forward 대상이 아니다.

**현재 범위**: surface split / pane split / 새 탭 / surface·tab·pane 닫기 / 탭 이동이 forward 대상이며, 성공 시 원격 실행 결과가 mirror 트리에 역반영된다. surface convert 와 surface 이동(move-surface)은 재사용할 원격 IPC 핸들러가 없어 아직 forward 하지 않고 로컬 차단 toast(`mirror_structural_blocked`)를 유지한다(단, 나중에 이 둘이 forward 되면 full-tree 역반영이 자동으로 커버한다). `Core::apply` 를 우회하는 UI 직접 경로(탭 드래그 등)도 아직 차단만 한다.

### 서버(피점유)측 비-holder 구조 변경 차단

위 절이 다루는 것은 **client(점유 holder)측** 구조 변경이 원격(서버)에서 실행되도록 forward 되는 경로다. 반대 방향 — **서버 자신이 hard-occupied 상태인 자기 workspace 에 대해, 점유 holder 가 아닌 제3자(서버 로컬 IPC/CLI/agent)가 직접** 구조 변경 IPC(`split`/`tab.create`/`pane.close`/`tab.close`/`tab.move`/`surface.close`)를 호출하는 경우도 배타성 위반이다 — [ADR-0040](../../adr/0040-occupancy-soft-hard-tiers-agent-occupant.md) 이 정의하는 hard 점유의 배타성은 입력(`apply_send_to_surface`)·resize(`resize_all_terminals`)뿐 아니라 구조 변경까지 적용돼야 한다.

- **차단 대상**: 위 IPC 6종을 **일반 IPC/CLI 진입점**(서버 로컬 호출)으로 직접 호출하고, 대상 pane/tab/surface 가 hard-occupied workspace 에 속한 경우. 요청은 `invalid_params` 에러(안내 문구: "점유 중이라 불가능, 다른 workspace 사용")로 거부되고 트리는 전혀 바뀌지 않는다.
- **차단 대상이 아닌 경우(중요)**: 점유 holder 본인이 mirror 안에서 실제로 만든 구조 변경이 위 forward 경로로 서버에 도달해 실행되는 것은 **정상 동작이며 이 차단의 대상이 아니다** — "attach 연결 자체가 그 workspace 에 대한 구조 변경 권한을 증명한다"는 forward 모델(위 절)을 그대로 유지한다.
- **차단 근거**: [`docs/identity.md`](../../identity.md) 원칙1(에이전트 행동의 부수효과가 사용자 상태에 닿지 않아야 함) — 서버 로컬에서 만든/닫은/옮긴 탭이 점유 client 화면에 통지 없이 편입/소멸/재배치되면, 원격 사용자가 보고 있는 화면에 자신이 하지 않은 변화가 일어나는 셈이라 이 원칙을 위반한다.
- **메커니즘**: [dev-guide/attach-behavior "서버 로컬(비-holder) 구조 변경 차단"](../../dev-guide/attach-behavior.md#서버-로컬비-holder-구조-변경-차단).

### 자동 매핑

`tasty set workspace --id <id> --ssh-profile <name> --remote-workspace <N>`(또는 `--ssh <user@host>`)로 로컬 워크스페이스에 원격 대상을 선언적으로 매핑한다(`Workspace.attach_mapping`, layout.json 영속). 매핑된 워크스페이스를 **활성화하면** 호스트가 자동으로 프로필 resolve → SSH 터널 → GUI mirror 를 띄운다. `remote_workspace` 가 None 이면 skip(ID 명시 필요), 이미 attach 중이면 재트리거 안 함. 자동 attach 는 mirror 를 *추가*만 하고 포커스/active 전환을 강제하지 않는다([포커스 독립성](../../identity.md)).

### 원격 생존 확인

`tasty remote check --ssh|--profile` — 원격 인스턴스가 *지금 살아있는지* 단발 판정. 포트 발견만으론 stale 포트 파일을 오판할 수 있어, 터널 수립 후 가벼운 IPC(`system.info`) 1회 응답까지 확인해야 alive(exit 0). 실패(거부/EOF/타임아웃)는 dead(exit≠0).

### 원격 워크스페이스 브라우징 (Browse)

`remote attach` 가 대상 workspace id 를 **미리 알아야** 동작하는 것과 달리, 브라우징은 그 id 를 **발견**한다 — attach 프로필/ssh 대상에 붙어 원격 인스턴스의 워크스페이스 목록(각 `id`/`name`/`pane_count`/`busy_count`/`attached`)을 받아온다. 흐름: 접속 스펙 resolve → (SSH 터널 or `127.0.0.1:PORT` loopback 직결) → 그 포트로 `workspace.list` + `attach.list` **2회 IPC** → workspace 단위 lock 을 join 해 `attached`(타 client 점유 여부)/`holder` 를 채운다(서버측 변경 0). 순수 조회라 로컬 사용자 상태(focus/닫은항목/선택)에 닿지 않는다([포커스 독립성](../../identity.md)).

이 능력은 **CLI(`remote workspaces`)와 로컬 IPC method(`remote.workspaces`) 양면**으로 노출된다(원칙 2 — 에이전트가 CLI 없이 소켓만으로도 브라우징 가능). 둘 다 동일한 코어(`tasty_cli::remote_browse`)를 공유하며, 블로킹 SSH I/O 는 호스트 IPC 경로에서 **워커 스레드**로 돌려 이벤트루프를 막지 않는다. RA02 원격 추가 팝업의 우측 목록이 이 출력을 데이터 소스로 소비한다.

### 원격 attach (IPC — focus 중립)

로컬 IPC method `remote.attach` { `remote_workspace`, `profile?`/`ssh?` } — 선택한 원격 워크스페이스를 **로컬 mirror 로 attach**(호스트가 워커 스레드에서 SSH 터널을 세우고 mirror 를 재구성). **focus 중립**이 핵심: 이 IPC/에이전트 경로는 mirror workspace 를 *조용히 생성만* 하고 focus 를 그 ws 로 옮기지 않는다(`active_workspace` 불변). 새 mirror 로의 focus 이동은 **사용자 입력 경로 전용 별도 단계**(RA02 팝업에서 사용자가 확정할 때)이며, release IPC 에는 focus 변경 API 가 없다(원칙 3). 회신은 즉시 `{attaching:true}`(fire-and-forget) — mirror 는 비동기로 나타나고 `list workspaces`(mirror 플래그)로 확인한다.

### 원격 워크스페이스 추가 팝업 (GUI picker — 사용자 경로)

위 브라우징/attach 능력을 **로컬 사용자가 직접 조작**하는 GUI 표면. 사이드바에서 **카테고리 헤더 우클릭(카테고리 on) / 새 워크스페이스(+) 버튼 우클릭 · 빈 배경 우클릭(그룹·플랫 모드 공통) → "원격 워크스페이스 추가"** 로 연다(`remote_attach` headless 팝업, 680×460 2-pane). 워크스페이스 카드 우클릭에는 없다(카테고리 ON/OFF 에 따라 노출 위치가 갈리도록 재배치 — [`sidebar/screens/sidebar.md`](../sidebar/screens/sidebar.md) 참고). 좌측은 `tasty-attach` 프로필 목록(remote_tool 이 편집하는 같은 스토어를 **소비만** 함), 우측은 선택 프로필의 원격 워크스페이스를 **4상태**(initial / connecting / error+retry / loaded[+empty])로 표시한다. 조회는 위 browse 코어(`tasty_cli::remote_browse`)를 **워커 스레드**로 돌려(폴링 슬롯) UI 를 막지 않는다. 이미 타 client 가 점유한 원격 ws 는 lavender `in use` 배지 + 선택 불가(중복 mirror 방지).

**Connect 확정 = 사용자 동작 → focus 이동**: 원격 ws 를 골라 Connect 하면 조회에 쓴 SSH 터널을 재사용해 mirror 로 attach 하고, **새 mirror ws 로 focus 가 이동**한다(사용자가 확정한 결과). 이 focus 이동은 IPC/에이전트 경로(위 `remote.attach`, focus 중립)와 분리된 **사용자 입력 전용 큐**(`CoreState.pending_gui_attach_user`)를 통해서만 일어난다 — release IPC 는 이 큐에 push 하지 못한다(원칙 1②). 컨텍스트 메뉴 진입은 `from_user_context_menu()` 로 마킹하고, self(loopback) attach 는 release 에서 `dispatch_pending_gui_attach` 게이트가 차단한다. Cancel/Esc/× 는 진행 중 조회(터널)를 정리하며 닫는다.

### mirror workspace 비영속

원격 attach 로 생긴 mirror workspace(`Workspace.mirror`)는 **원격 점유가 살아있는 세션 동안만** 유효하다. layout.json 에 저장하면 재시작 시 원격 없는 **죽은 일반 workspace** 로 복원되므로, `SavedLayout::capture`(`src/engine/layout_persistence/capture.rs`)가 캡처 순회에서 mirror workspace 를 **제외**하고 `active_workspace` 인덱스도 필터 후 위치로 remap 한다(자동 attach mirror·GUI picker mirror 공통). 팝업 세션 상태(선택 프로필/조회 결과/선택 ws)도 egui temp 메모리(비영속)라 tasty 종료 시 함께 사라진다.

## 인터페이스

- **AI Agent / 원격 (CLI)**:
  - `tasty tool attach <name> [SURFACE] [--workspace <id>] [옵션…]` — **tasty-attach 프로필**로 attach(profile-우선 편의 표면). `--list` = tasty-attach 목록만.
  - `tasty remote attach [SURFACE] --ssh|--profile [옵션…]` — 원격 surface attach. `--profile` 은 **tasty-attach kind**(ADR-0032; ref/inline resolve).
  - `tasty remote attach --workspace <id> --ssh|--profile …` — 원격 workspace attach.
  - `tasty remote attach --force-detach [--workspace <id>]` — 점유 강제 해제(로컬 JSON-RPC; `--ssh` 와 상호배타).
  - `tasty remote attach --into-gui --target-port <p> --workspace <ws>` — 실행 GUI 에 mirror.
  - `tasty remote check --ssh|--profile` — 원격 생존 확인(`--profile` = tasty-attach).
  - `tasty remote workspaces --ssh|--profile [--json]` — 원격 워크스페이스 목록 조회(browse). `--ssh 127.0.0.1:<port>` 로 loopback 직결(터널 없이 로컬 e2e).
  - `tasty set workspace --id <id> --ssh-profile <name> --remote-workspace <N>` — 자동 매핑 선언.
- **IPC (`attach.*`)**: `acquire`/`release`(stream 핸드셰이크), `force_detach`/`force_detach_workspace`, `into_gui`, `list`(점유 목록 조회). 표 상세 → [dev-guide/attach-behavior](../../dev-guide/attach-behavior.md#ipc-표면-attach).
- **IPC (`remote.*` — 원격 브라우징/attach, 원칙 2)**: `remote.workspaces` { `profile?`/`ssh?` } → 원격 ws 목록(browse, 워커 스레드+지연 회신). `remote.attach` { `remote_workspace`, `profile?`/`ssh?` } → 원격 ws 를 로컬 mirror 로 attach(**focus 중립**: mirror 생성만, focus 이동 없음). CLI `remote workspaces` 와 코어(`tasty_cli::remote_browse`) 공유.
- **로컬 self attach**: 사용자 mirror 조작 재현 성격이라 release 에 없음 — `tasty debug attach`(debug 빌드 전용, [`dev-guide/debug-ipc`](../../dev-guide/debug-ipc.md)).
- **프로필**: `--profile`/`tool attach` 이 참조하는 tasty-attach 프로필(및 그것이 `ssh_ref` 로 참조하는 ssh 프로필)은 [remote-profiles](../remote-profiles/index.md) 이 관리.

## 비-목표 (Out of scope)

- **자체 원격 프로토콜/암호화/인증** — 전부 SSH 에 위임. attach 채널에 별도 토큰 없음(연결 경계 = 권한 경계).
- **단발 화면 읽기** — attach 세션을 열 필요 없음. 정식 경로는 `tasty read screen` / `tasty read since-mark`(별도 기능).
- **로컬 loopback attach 의 release 노출** — debug 전용.
- **프로토콜 프레임/터널 결선/재연결 백오프 등 메커니즘** — [dev-guide/attach-behavior](../../dev-guide/attach-behavior.md).
- **SSH 프로필 CRUD** — [ssh-tool](../remote-profiles/index.md).

## Acceptance Criteria

- [ ] Given 동일 머신 두 인스턴스 When 한쪽이 다른 쪽 surface 를 attach Then mirror grid 가 원본과 일치한다(`--dump-after` 로 검증).
- [ ] Given surface 가 이미 점유됨 When 다른 client 가 attach 시도 Then holder 정보를 담아 거부된다.
- [ ] Given 점유된 surface When 서버측 GUI 키/`surface.send` Then 입력이 차단되고 client 입력만 도달한다.
- [ ] Given 점유 상태 When 로컬 사용자가 `--force-detach` Then holder 가 종료되고 대상이 일반 surface 로 복귀한다.
- [ ] Given client 연결 종료(EOF) Then 점유 lock 이 자동 free 된다.
- [ ] Given workspace attach When 멤버 터미널 하나가 이미 다른 client 점유 Then workspace attach 가 거부된다.
- [ ] Given stale 포트 파일만 있는 죽은 인스턴스 When `tasty remote check` Then dead(exit≠0)로 판정한다.

> 전부 headless 검증 가능 — 동일 머신 다중 인스턴스 + loopback 직결(`127.0.0.1:PORT`)로 SSH 없이도 attach 파이프라인을 재현, `--dump-after` 로 grid 일치 확인.

## 구현

- 점유 레지스트리: `src/core/attach.rs` `OccupancyRegistry`(hard: `surface_locks` / `workspace_locks` / `surface_to_workspace`, acquire/release/force_detach/release_all_for_client · soft: 별도 엔트리 + acquire_soft/release_soft, ADR-0040). 휘발성.
- 런타임/스냅샷: `src/core/attach_runtime.rs`(서버측 수신, transport 무관 loopback), `src/core/attach_readonly.rs`(서버측 readonly mirror), `src/app/attach_poll.rs`(3초 tick).
- 자동 매핑: `src/app/auto_attach.rs`(`Workspace.attach_mapping` 활성화 시 SSH 터널 + GUI mirror).
- IPC: `src/adapters/ipc/handler/attach.rs`(`attach.*`). 원격 브라우징/attach IPC(`remote.workspaces`/`remote.attach`)는 `src/app/ipc/app_methods.rs`(워커 스레드+지연 회신). focus 중립 mirror 생성은 `src/app/auto_attach.rs`(수동 트리거 `anchor=None` 재사용) → `src/app/attach_client.rs::start_gui_attach`(`workspaces.push` 만, `active_workspace` 불변; 새 mirror ws id 반환).
- GUI picker 팝업(사용자 경로): `src/adapters/ui/popup/remote_attach.rs`(2-pane 상태머신 + browse 워커 폴링), `defs.rs`(headless PopupDef), 진입 컨텍스트 메뉴 2곳 `src/view/main/redraw.rs`(NewWorkspaceButton / Workspace). Connect → `CoreState.pending_gui_attach_user` 큐 → `App::dispatch_pending_gui_attach`(사용자 경로 drain) → `start_gui_attach` + `focus_mirror_workspace`(새 mirror 로 focus 이동, 사용자 경로 전용). 갤러리 specimen: `crates/tasty-gallery/src/catalog/components/remote_attach.rs`(4상태).
- mirror 비영속: `src/engine/layout_persistence/capture.rs`(`SavedLayout::capture` 가 `ws.mirror` 제외 + active 인덱스 remap). 회귀 테스트 `core::state` `mirror_workspace_not_persisted`.
- 브라우징 코어(CLI/IPC 공유): `crates/tasty-cli/src/remote_browse.rs`(`browse`/`resolve_endpoint`/`probe_method` — loopback 직결 + `workspace.list`+`attach.list` 병합).
- CLI: `crates/tasty-cli/src/commands/remote.rs`(디스패치), `attach.rs`(`run_attach_*` 세션 머신), `remote_check.rs`, `remote_workspaces.rs`(browse 얇은 래퍼), `ssh.rs`(SSH 결선).

## 화면

- [screens/remote-attach.md](screens/remote-attach.md) — GUI mirror 워크스페이스 + 점유 readonly 표시(사이드바 mirror glyph/chip / 작업영역 렌더로 연결).
</content>
