# 원격 attach (Remote attach)

- **Status**: Implemented
- **주체**: 원격 접속 사용자(점유 후 조작) · AI Agent(원격 mirror 를 정당한 행동으로 attach) · 로컬 사용자(force-detach 권한)
- **ADR**: [ADR-0007](../../adr/0007-attach-targets-remote.md) (attach 는 원격 대상 · 로컬 self-attach 는 debug 격리). 보안 위임 근거는 [ADR-0004](../../adr/0004-ipc-transport-tcp.md) (loopback trust boundary)
- **코드**: `src/core/attach.rs`(`AttachRegistry`), `src/core/attach_runtime.rs`, `src/app/auto_attach.rs`, `src/adapters/ipc/handler/attach.rs`, CLI `crates/tasty-cli/src/commands/{remote,attach,remote_check}.rs`
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

### 모드

- **mirror-dump**(`--dump-after <ms>`): N ms 출력을 수집해 grid 를 재구성하고 텍스트를 stdout 으로 출력 후 종료. GUI 없이 attach 파이프라인을 검증하는 경로.
- **raw 브리지**(`--raw`): stdin↔stdout passthrough(detach 키 `Ctrl+\`). **surface 단위만** — workspace 모드는 다중 터미널이라 불가.
- **단발 입력**(`--send <str>`): attach 직후 1회 입력(escape 디코딩). workspace 모드는 `--send-to <remote_surface_id>` 로 대상 지정.

### GUI mirror

`tasty remote attach --into-gui --target-port <원격포트> --workspace <원격ws>` → 이 명령을 받은 **로컬 GUI 인스턴스**가 client 가 되어 원격 워크스페이스를 mirror 로 재구성한다(`attach.into_gui`). mirror Workspace 는 일반 워크스페이스로 사이드바에 노출되되 **항상 하늘색 dot** 으로 로컬과 구분(`Workspace.mirror`). 갱신은 원격 출력이 올 때 즉시, 3초 tick 은 backstop.

### 자동 매핑

`tasty set workspace --id <id> --ssh-profile <name> --remote-workspace <N>`(또는 `--ssh <user@host>`)로 로컬 워크스페이스에 원격 대상을 선언적으로 매핑한다(`Workspace.attach_mapping`, layout.json 영속). 매핑된 워크스페이스를 **활성화하면** 호스트가 자동으로 프로필 resolve → SSH 터널 → GUI mirror 를 띄운다. `remote_workspace` 가 None 이면 skip(ID 명시 필요), 이미 attach 중이면 재트리거 안 함. 자동 attach 는 mirror 를 *추가*만 하고 포커스/active 전환을 강제하지 않는다([포커스 독립성](../../identity.md)).

### 원격 생존 확인

`tasty remote check --ssh|--profile` — 원격 인스턴스가 *지금 살아있는지* 단발 판정. 포트 발견만으론 stale 포트 파일을 오판할 수 있어, 터널 수립 후 가벼운 IPC(`system.info`) 1회 응답까지 확인해야 alive(exit 0). 실패(거부/EOF/타임아웃)는 dead(exit≠0).

## 인터페이스

- **AI Agent / 원격 (CLI)**:
  - `tasty remote attach [SURFACE] --ssh|--profile [옵션…]` — 원격 surface attach.
  - `tasty remote attach --workspace <id> --ssh|--profile …` — 원격 workspace attach.
  - `tasty remote attach --force-detach [--workspace <id>]` — 점유 강제 해제(로컬 JSON-RPC; `--ssh` 와 상호배타).
  - `tasty remote attach --into-gui --target-port <p> --workspace <ws>` — 실행 GUI 에 mirror.
  - `tasty remote check --ssh|--profile` — 원격 생존 확인.
  - `tasty set workspace --id <id> --ssh-profile <name> --remote-workspace <N>` — 자동 매핑 선언.
- **IPC (`attach.*`)**: `acquire`/`release`(stream 핸드셰이크), `force_detach`/`force_detach_workspace`, `into_gui`, `list`(점유 목록 조회). 표 상세 → [dev-guide/attach-behavior](../../dev-guide/attach-behavior.md#ipc-표면-attach).
- **로컬 self attach**: 사용자 mirror 조작 재현 성격이라 release 에 없음 — `tasty debug attach`(debug 빌드 전용, [`dev-guide/debug-ipc`](../../dev-guide/debug-ipc.md)).
- **SSH 프로필**: `--profile` 이 참조하는 프로필은 [ssh-tool](../ssh-tool/index.md) 이 관리.

## 비-목표 (Out of scope)

- **자체 원격 프로토콜/암호화/인증** — 전부 SSH 에 위임. attach 채널에 별도 토큰 없음(연결 경계 = 권한 경계).
- **단발 화면 읽기** — attach 세션을 열 필요 없음. 정식 경로는 `tasty read screen` / `tasty read since-mark`(별도 기능).
- **로컬 loopback attach 의 release 노출** — debug 전용.
- **프로토콜 프레임/터널 결선/재연결 백오프 등 메커니즘** — [dev-guide/attach-behavior](../../dev-guide/attach-behavior.md).
- **SSH 프로필 CRUD** — [ssh-tool](../ssh-tool/index.md).

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
- IPC: `src/adapters/ipc/handler/attach.rs`(`attach.*`).
- CLI: `crates/tasty-cli/src/commands/remote.rs`(디스패치), `attach.rs`(`run_attach_*` 세션 머신), `remote_check.rs`, `ssh.rs`(SSH 결선).
- trait marker: `AttachedSurface`(`kind:"attached"`, placeholder/mirror) — [work-area Surface 종류](../work-area/index.md#surface-종류).

## 화면

- [screens/remote-attach.md](screens/remote-attach.md) — GUI mirror 워크스페이스 + 점유 readonly 표시(사이드바 dot / 작업영역 렌더로 연결).
</content>
