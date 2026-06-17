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

## 점유 레지스트리 (`AttachRegistry`)

`src/core/attach.rs`. 휘발성(직렬화/복원 안 함 — 재시작 시 빈 registry → 전부 free).

- `surface_locks: HashMap<SurfaceId, AttachLock>` — surface 단위 배타 lock. `acquire` 가 동시 점유를 `AlreadyAttached{holder}` 로 거부.
- `workspace_locks` + `surface_to_workspace` — workspace 단위 점유. workspace 점유 시 멤버 *터미널* 은 `surface_locks` 에도 동일 holder 로 등록(서버측 placeholder 렌더·입력차단을 surface 단위와 동일 적용). 비-터미널은 역매핑(`surface_to_workspace`)으로만 "점유 표시".
- `release` (holder 본인) / `force_detach` (서버 권한) / `release_all_for_client`(EOF 시 일괄 — workspace + 멤버 + 잔여 surface).
- **입력 격리**: `apply_send_to_surface` 가 `is_attached` 면 서버 로컬 입력 거부, client 입력만 `feed_attached_input` 우회 경로로 PTY 도달.

## 초기 스냅샷 + delta

attach 직후 서버가 현재 visible 화면을 `snapshot_as_vt` 로 **1회** 직렬화 push(셀 속성 + 커서 + alt-screen/DECCKM/bracketed 모드 복원). 이후 변화는 output tap delta(Data 프레임). client 는 받은 바이트를 PTY 없는 mirror 터미널(`Terminal::new_detached` + `feed_bytes`)에 먹여 같은 termwiz 파서로 grid 재구성.

## workspace mux

한 연결로 N 터미널 출력을 나르므로 workspace 모드 Data 프레임은 **surface-prefixed**(`encode_mux`/`decode_mux`). surface 단위 단일 연결은 prefix 없음. attach 직후 서버가 `attached_workspace` Control 로 트리(분할 방향/비율) + per-surface(`{remote_id, role, cols, rows, kind}`)를 보내고, client 는 원격↔로컬 surface_id 재매핑으로 트리를 재구성한다.

## 갱신 cadence 분리

- **서버측 readonly 뷰**(피점유 — 대상 부하 절약): **3초 polling**(`AttachPoll` tick, `src/app/attach_poll.rs`)으로 self-snapshot 적용.
- **client mirror**(내가 다루는 대상): 원격 출력이 올 때마다 즉시 갱신, 3초 tick 은 backstop(누락 출력 적용·끊긴 세션 정리)으로만.

## SSH 터널 (원격 client 공통)

`crates/tasty-cli/src/ssh.rs` — `remote attach` 와 `remote check` 가 공유.

- **시스템 ssh 위임**: 자체 암호화 없이 시스템 `ssh` 를 자식 프로세스로 실행. 사용자 `~/.ssh/config`·agent·known_hosts 재사용. **Windows 는 시스템 OpenSSH 풀경로**(`%WINDIR%\System32\OpenSSH\ssh.exe`) 우선 — git 번들 ssh 는 윈도우 ssh-agent(named pipe)를 못 봐 무암호 인증 실패.
- **원격 포트 발견**: 기본 `auto` = subcommand → file-unix → file-windows 순서로 원격 DefaultShell 4종(PowerShell/cmd/git bash/unix) 커버. `--remote-port-mode` 로 고정, `--remote-tasty <path>` 로 원격 바이너리 경로(기본 `tasty`).
- **터널 생명주기**: detach/종료 시 자식 ssh kill(고아 터널 방지)하되 **원격 데몬은 생존**(server-owns-PTY persistence = detach 의 본질). 자동 재연결(attach 한정): 지수 백오프(0.5s→30s)로 터널+attach 재수립(`--no-reconnect` 로 끔).
- **loopback 직결**: 인라인 host 가 `127.0.0.1:PORT`/`localhost:PORT` 면 SSH 없이 직접 attach(동일 머신 다중 인스턴스 검증).

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
- SSH 프로필 관리: [`features/ssh-tool`](../features/ssh-tool/index.md)
- 로컬 self attach 격리: [`dev-guide/debug-ipc`](debug-ipc.md)
</content>
