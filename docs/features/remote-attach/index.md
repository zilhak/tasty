# 원격 attach (Remote attach)

- **Status**: Implemented
- **주체**: 원격 접속 사용자(점유 후 조작) · AI Agent(원격 mirror 를 정당한 행동으로 attach) · 로컬 사용자(force-detach 권한)
- **ADR**: [ADR-0007](../../adr/0007-attach-targets-remote.md) (attach 는 원격 대상 · 로컬 self-attach 는 debug 격리). 보안 위임 근거는 [ADR-0004](../../adr/0004-ipc-transport-tcp.md) (loopback trust boundary)
- **코드**: `src/core/attach.rs`(`AttachRegistry`), `src/core/attach_runtime.rs`, `src/app/auto_attach.rs`, `src/adapters/ipc/handler/attach.rs`, `src/app/ipc/app_methods.rs`(`remote.workspaces`/`remote.attach`), CLI `crates/tasty-cli/src/remote_browse.rs`, `crates/tasty-cli/src/commands/{remote,attach,remote_check,remote_workspaces}.rs`
- **화면**: [screens/remote-attach.md](screens/remote-attach.md)

## 목적

다른 호스트(또는 동일 머신의 다른 인스턴스)의 surface/workspace 를 **점유(occupy)** 해 mirror 로 보고 조작하는 기능. tasty 는 자체 원격 프로토콜·암호화를 만들지 않고 **SSH 에 위임**한다 — "그 호스트에 SSH 로 들어올 수 있는 사람 = attach 자격"(tmux 모델). 원격 접속 사용자가 tasty 를 쓰는 유일한 경로이며, [actors](../../concepts/actors.md) 의 **점유 모델**이 실제로 구현되는 지점이다.

## 내부 동작 (headless-valid)

### 점유 (Occupation)

attach 의 본질은 **배타 점유**다. 개념 정의는 [actors 점유 모델](../../concepts/actors.md#점유-occupation-모델), 여기선 동작:

- **배타 lock**: 한 surface 는 한 client 만 점유한다(`AttachRegistry`). 점유는 `stream.open{target}` 핸드셰이크의 `attach.acquire` 로 잡고, 동시 attach 는 holder 정보를 담아 `already_attached` 로 거부.
- **점유 중 격리**: 점유된 surface 의 서버 로컬 입력(GUI 키 / `surface.send`)은 차단되고, **점유 client 입력만** PTY 에 도달한다. 로컬 사용자·AI Agent 는 그 대상에 대해 **readonly** — 내용은 보이되 조작은 막힌다.
- **자동 해제**: client 연결 종료(EOF) 시 lock 이 free 로 환원. 점유는 **휘발성** — 서버 재시작 시 전부 free(영속 안 함).
- **force-detach**: **로컬 사용자만** 점유를 강제로 끊을 수 있다(서버 권한). 끊으면 holder client 에 종료를 통지하고 대상은 **일반 surface/workspace 로 복귀**.

### surface 단위 vs workspace 단위

- **surface attach**: 단일 터미널 surface 를 mirror. 한 연결 = 한 터미널.
- **workspace attach**: 워크스페이스를 점유하면 그 안 **모든 터미널 surface 를 트리째 mirror**(분할 방향/비율 포함)하고, **비-터미널 surface**(markdown/image/explorer 등)는 mirror 불가라 placeholder 로 숨긴다. workspace lock 은 멤버 터미널 전부를 surface lock 에도 등록하므로, 멤버가 이미 다른 client 에 점유돼 있으면 workspace attach 를 **거부**(부분 점유 충돌 방지).

### 화면 동기화

attach 성립 직후 서버가 현재 화면을 **1회 스냅샷**으로 push 하고, 이후 변화는 delta 로 흐른다. client 는 PTY 없는 mirror 터미널에 바이트를 먹여 같은 grid 를 재구성한다. 프로토콜·mux 상세는 → [dev-guide/attach-behavior](../../dev-guide/attach-behavior.md).

**그리드 크기는 원격이 authoritative** — mirror 의 cols×rows 는 로컬 창/pane 크기가 아니라 **원격 터미널 크기**를 따른다. 원격 PTY 는 자기 grid 로 콘텐츠를 래핑하므로 mirror grid 가 로컬 크기로 바뀌면 줄바꿈·커서가 어긋난다. 구현:

- mirror 는 detached 터미널(PTY 없음)이라 매 프레임 도는 로컬 레이아웃 리사이즈 스윕(`Core::resize_all_terminals` / `AppState::resize_all`)이 **detached 터미널을 skip** 한다(`Terminal::is_detached`). → 로컬 창 크기가 mirror grid 를 덮어쓰지 못한다.
- 원격 터미널이 리사이즈되면 서버가 attach stream 의 **`Control` 프레임(`StreamControl::Resize`)** 으로 새 cols/rows 를 통지하고, client 가 그 값으로 mirror 를 리사이즈한다. → 원격 grid 변경이 mirror 에 반영된다.
- 렌더러는 mirror 의 실제 grid 크기로 셀을 pane 좌상단에 배치한다 — pane 이 원격 grid 보다 크면 남는 영역은 배경으로 남는 자연 레터박스(스케일·스트레치 없음).

`StreamControl` 은 `event` 태그 기반 확장 enum 이라, 향후 구조 변경 이벤트(탭/pane open·close forward)도 새 `StreamTag` 없이 variant 로 추가된다.

### 모드

- **mirror-dump**(`--dump-after <ms>`): N ms 출력을 수집해 grid 를 재구성하고 텍스트를 stdout 으로 출력 후 종료. GUI 없이 attach 파이프라인을 검증하는 경로.
- **raw 브리지**(`--raw`): stdin↔stdout passthrough(detach 키 `Ctrl+\`). **surface 단위만** — workspace 모드는 다중 터미널이라 불가.
- **단발 입력**(`--send <str>`): attach 직후 1회 입력(escape 디코딩). workspace 모드는 `--send-to <remote_surface_id>` 로 대상 지정.

### GUI mirror

`tasty remote attach --into-gui --target-port <원격포트> --workspace <원격ws>` → 이 명령을 받은 **로컬 GUI 인스턴스**가 client 가 되어 원격 워크스페이스를 mirror 로 재구성한다(`attach.into_gui`). mirror Workspace 는 일반 워크스페이스로 사이드바에 노출되되 **이름 앞 하늘색 `>_→` glyph**(collapsed 레일은 아바타 우하단 하늘색 corner chip)로 로컬과 구분(`Workspace.mirror`). status dot 은 실행상태(running/idle) 전용이며 mirror 색을 싣지 않는다 — 원격 origin 은 별도 시각 축(디자인 `workspace-mirror-fg`, notif=우상단 / attached=둘레 ring 과 채널 분리). 갱신은 원격 출력이 올 때 즉시, 3초 tick 은 backstop.

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

위 브라우징/attach 능력을 **로컬 사용자가 직접 조작**하는 GUI 표면. 사이드바에서 **워크스페이스 카드 우클릭 / 새 워크스페이스(+) 버튼 우클릭 → "원격 워크스페이스 추가"** 로 연다(`remote_attach` headless 팝업, 680×460 2-pane). 좌측은 `tasty-attach` 프로필 목록(remote_tool 이 편집하는 같은 스토어를 **소비만** 함), 우측은 선택 프로필의 원격 워크스페이스를 **4상태**(initial / connecting / error+retry / loaded[+empty])로 표시한다. 조회는 위 browse 코어(`tasty_cli::remote_browse`)를 **워커 스레드**로 돌려(폴링 슬롯) UI 를 막지 않는다. 이미 타 client 가 점유한 원격 ws 는 lavender `in use` 배지 + 선택 불가(중복 mirror 방지).

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

- 점유 레지스트리: `src/core/attach.rs` `AttachRegistry`(`surface_locks` / `workspace_locks` / `surface_to_workspace`, acquire/release/force_detach/release_all_for_client). 휘발성.
- 런타임/스냅샷: `src/core/attach_runtime.rs`(서버측 수신, transport 무관 loopback), `src/core/attach_readonly.rs`(서버측 readonly mirror), `src/app/attach_poll.rs`(3초 tick).
- 자동 매핑: `src/app/auto_attach.rs`(`Workspace.attach_mapping` 활성화 시 SSH 터널 + GUI mirror).
- IPC: `src/adapters/ipc/handler/attach.rs`(`attach.*`). 원격 브라우징/attach IPC(`remote.workspaces`/`remote.attach`)는 `src/app/ipc/app_methods.rs`(워커 스레드+지연 회신). focus 중립 mirror 생성은 `src/app/auto_attach.rs`(수동 트리거 `anchor=None` 재사용) → `src/app/attach_client.rs::start_gui_attach`(`workspaces.push` 만, `active_workspace` 불변; 새 mirror ws id 반환).
- GUI picker 팝업(사용자 경로): `src/adapters/ui/popup/remote_attach.rs`(2-pane 상태머신 + browse 워커 폴링), `defs.rs`(headless PopupDef), 진입 컨텍스트 메뉴 2곳 `src/view/main/redraw.rs`(NewWorkspaceButton / Workspace). Connect → `CoreState.pending_gui_attach_user` 큐 → `App::dispatch_pending_gui_attach`(사용자 경로 drain) → `start_gui_attach` + `focus_mirror_workspace`(새 mirror 로 focus 이동, 사용자 경로 전용). 갤러리 specimen: `crates/tasty-gallery/src/catalog/components/remote_attach.rs`(4상태).
- mirror 비영속: `src/engine/layout_persistence/capture.rs`(`SavedLayout::capture` 가 `ws.mirror` 제외 + active 인덱스 remap). 회귀 테스트 `core::state` `mirror_workspace_not_persisted`.
- 브라우징 코어(CLI/IPC 공유): `crates/tasty-cli/src/remote_browse.rs`(`browse`/`resolve_endpoint`/`probe_method` — loopback 직결 + `workspace.list`+`attach.list` 병합).
- CLI: `crates/tasty-cli/src/commands/remote.rs`(디스패치), `attach.rs`(`run_attach_*` 세션 머신), `remote_check.rs`, `remote_workspaces.rs`(browse 얇은 래퍼), `ssh.rs`(SSH 결선).
- trait marker: `AttachedSurface`(`kind:"attached"`, placeholder/mirror) — [work-area Surface 종류](../work-area/index.md#surface-종류).

## 화면

- [screens/remote-attach.md](screens/remote-attach.md) — GUI mirror 워크스페이스 + 점유 readonly 표시(사이드바 mirror glyph/chip / 작업영역 렌더로 연결).
</content>
