# Attach 동작 명세

attach 의 *정확한 현재 동작* 단일 출처. attach 는 "**서버는 transport 무관하게 항상 loopback 으로 받고, 로컬/원격 구분은 전적으로 클라이언트 측 개념**" 이라는 헷갈리기 쉬운 핵심이 있어, 후속 작업자가 서버 핸들러를 잘못 건드리지 않도록 한 곳에 못 박는다.

사용자향 요약은 [features.md](../features.md) 의 "attach/detach" 섹션을, 보안 위임 근거는 CLAUDE.md "원칙 1" 과 attach 권한 결정(decision 5)을 본다.

## 서버 / 클라이언트 계층 구분 (가장 먼저 읽을 것)

attach 는 **server**(피점유 — PTY/grid 소유)와 **client**(점유 — mirror 표시) 두 쪽으로 나뉜다.

- **서버측** (`src/core/attach_runtime.rs`, IPC `attach.*`) — **transport 를 모른다.** 항상 `127.0.0.1` 로만 클라이언트를 받는다. 로컬에서 붙든 SSH 터널 너머에서 붙든, 서버 입장에선 전부 loopback 접속이다. 서버는 SSH 를 전혀 알지 못한다(`crates/tasty-cli/src/ssh.rs` 헤더: "호스트·IPC 서버는 SSH 를 전혀 모르고 loopback 만 안다").
- **클라이언트측** — "원격성" 을 전부 흡수하는 쪽. 두 종류:
  - **로컬 클라이언트**: 포트 파일(`~/.tasty/tasty.port`)을 읽어 그 loopback 포트로 직결. **release 빌드에서 제거 → debug 전용**(`tasty debug attach`).
  - **원격 클라이언트**: `ssh -L 127.0.0.1:<localport>:127.0.0.1:<remoteport> -N` 터널을 세운 뒤, 그 터널의 **localport 로 직결**. 터널은 바이트 파이프라 스트림 프로토콜에 투명하다 — 즉 원격 클라이언트도 결국 자기 머신의 loopback 포트에 붙는다(`tasty remote attach --ssh|--profile`).

### 핵심 사실: "로컬 attach 제거" 의 정확한 의미

원격 attach 도 **서버 입장에선 loopback** 이다. 따라서 release 에서 "로컬 attach 를 제거" 한 것은 **서버를 바꾼 게 아니라 클라이언트의 로컬 진입점(`tasty attach` → `tasty debug attach`)만 제거**한 것이다. 서버의 attach 수신 경로(`attach_surface_for_stream` 등)는 로컬/원격 공용으로 그대로 보존된다. 실제 SSH 터널 + attach 세션 머신(`run_attach_on_port` / `run_attach_workspace_on_port` / `run_attach_ssh`)은 `crates/tasty-cli/src/commands/attach.rs` 에 로컬·원격 공용으로 남아 있고, `remote` / `debug` 네임스페이스는 그 위에서 디스패치만 한다.

## CLI 표면 매핑 (변경 후)

| 용도 | 명령 | 빌드 |
|------|------|------|
| 원격 surface attach | `tasty remote attach [SURFACE] --ssh\|--profile [옵션…]` | release |
| 원격 workspace attach | `tasty remote attach --workspace <id> --ssh\|--profile [옵션…]` | release |
| 원격 생존 확인 | `tasty remote check --ssh\|--profile` | release |
| 로컬 loopback attach | `tasty debug attach [SURFACE] [--workspace <id>] …` | **debug 전용** |
| 화면 스크래핑(정식) | `tasty read screen` / `tasty read since-mark` | release |

- **로컬 self(loopback) attach = 사용자 mirror 조작의 자동 재현** 성격이라 release 표면에 두지 않고 `tasty debug attach`(debug 빌드)로 격리한다(CLAUDE.md 원칙 1 ②, [debug-ipc.md](debug-ipc.md)). 원격 attach 는 *다른 호스트의 surface/workspace 를 mirror* 하는 에이전트의 정당한 행동이라 release 에 노출한다(원칙 2).
- **단발 화면 읽기**는 attach 세션을 열 필요가 없다. 정식 스크래핑 경로는 `tasty read screen`(현재 화면) / `tasty read since-mark`(마크 이후 출력)다. attach 의 `--dump-after` 는 *mirror 검증용* 으로, 일반 화면 읽기 용도가 아니다.

### `remote` / `debug` 는 CLI 디스패치 네임스페이스 (IPC 와 비대칭)

`remote` 와 `debug attach` 는 **IPC 네임스페이스가 아니다.** attach 의 IPC 표면은 그대로 `attach.*`(아래)이고, `remote attach`/`remote check`/`debug attach` 는 그 위(+`system.info`)에서 *원격성·debug 격리만 분기*하는 CLI 디스패치 계층이다. 그래서 `attach` IPC 네임스페이스 메서드 수(6)는 CLI 재편과 무관하게 유지된다([cli-naming.md](cli-naming.md) 의 host namespace 카운트 표).

## 점유 모델 (`AttachRegistry`)

- **배타 점유**: 한 surface 는 한 client 만 점유한다. attach 는 `stream.open{target}` 핸드셰이크의 `attach.acquire` 로 lock 을 잡고, 동시 attach 는 `already_attached`(holder 정보 포함)로 거부된다.
- **자동 해제**: client 연결 종료(EOF) 시 lock 이 자동 free 환원된다. 점유는 휘발성이라 서버 재시작 시에도 free 로 돌아온다.
- **점유 중 입력 격리**: 점유 surface 의 서버 로컬 입력(GUI 키 / `surface.send`)은 차단되고(`apply_send_to_surface` 의 is_attached 거부), **client 입력만** 우회 경로(`feed_attached_input`)로 PTY 에 도달한다.

## 초기 스냅샷 + delta

attach 성립 직후 서버는 현재 visible 화면을 `snapshot_as_vt` 로 **1회** 직렬화해 push 한다(셀 속성 + 커서 위치 + alt-screen/DECCKM/bracketed 모드 복원). 이후 화면 변화는 output tap delta(Data 프레임)로 흐른다. client 는 받은 바이트를 PTY 없는 mirror 터미널(`Terminal::new_detached` + `feed_bytes`)에 먹여 같은 termwiz 파서로 grid 를 재구성한다.

## 모드

- **mirror-dump**(`--dump-after <ms>`): attach 후 N ms 동안 출력을 수집해 mirror Terminal 로 grid 를 재구성하고 `screen_text` 를 stdout 으로 출력한 뒤 종료한다. GUI 없이 attach 파이프라인(초기 스냅샷 + delta)을 자동 검증하는 경로. workspace 모드에서는 surface 별 화면을 `=== surface <id> ===` 섹션으로 출력(비-터미널은 `(placeholder: <kind>)`).
- **raw 브리지**(`--raw`): stdin↔stdout passthrough. detach 전용 키는 `Ctrl+\`. workspace 모드에서는 사용할 수 없다.
- **`--send <str>`**: attach 직후 **1회** 비대화형 입력을 보낸다(escape 디코딩 `\n \r \t \xNN`). raw TTY 없이 입력 라우팅을 검증하기 위함. workspace 모드에서는 `--send-to <remote_surface_id>` 로 대상 surface 를 지정한다.

## workspace 단위 attach

한 workspace 를 점유하면 그 안 **모든 터미널 surface 를 트리째 mirror** 하고, 비-터미널 surface(markdown/image/explorer 등)는 placeholder 로 숨긴다.

- workspace lock 은 멤버 터미널 전부를 surface lock 에도 등록 → 서버측 placeholder 렌더·입력차단이 surface 단위와 동일하게 적용된다. 멤버 터미널이 이미 다른 client 에 점유돼 있으면 workspace attach 를 거부한다(부분 점유 충돌 방지).
- 한 연결로 N 개 터미널 출력을 나르므로 workspace 모드 Data 프레임은 **surface-prefixed**(`encode_mux`/`decode_mux`)다. surface 단위 단일 연결은 prefix 없이 그대로다.
- attach 직후 서버가 `attached_workspace` Control 로 트리(분할 방향/비율) + per-surface 정보(`{remote_id, role, cols, rows, kind}`)를 보낸다. client 는 원격↔로컬 surface_id 재매핑으로 트리를 재구성한다.
- **`--raw` 불가** (다중 터미널이라 단일 stdin/stdout 브리지 불가능).

## GUI mirror (원격 워크스페이스를 GUI 에 띄우기)

`tasty remote attach --into-gui --target-port <원격포트> --workspace <원격ws>` → 이 명령을 받은 **로컬 GUI 인스턴스**가 client 가 되어 원격 워크스페이스를 mirror 로 재구성한다(`attach.into_gui` IPC → `App::start_gui_attach`). mirror Workspace 는 일반 워크스페이스로 사이드바에 노출되고(항상 하늘색 상태 dot 으로 로컬과 구분) 기존 렌더러가 그대로 그린다.

- **갱신 cadence 분리**: 서버측 readonly 뷰(피점유 — 대상 부하 절약)는 **3초 polling**(`AttachPoll` tick)으로 self-snapshot 을 적용한다. 반면 client mirror(내가 직접 다루는 대상)는 원격 출력이 올 때마다 즉시 실시간 갱신하고, 3초 tick 은 backstop(누락 출력 적용·끊긴 세션 정리)으로만 쓴다.

## 자동 매핑 (워크스페이스 ↔ 원격 컴퓨터)

`tasty set workspace --id <id> --ssh-profile <name> --remote-workspace N`(또는 `--ssh <user@host>`)으로 로컬 워크스페이스에 원격 대상을 선언적으로 매핑한다(`Workspace.attach_mapping`, `layout.json` 영속). 매핑된 워크스페이스를 **활성화하면** 호스트가 자동으로 ① 프로필 resolve → ② SSH 터널 수립(워커 스레드, 메인 루프 무블록) → ③ `start_gui_attach` 로 GUI mirror 를 띄운다(`src/app/auto_attach.rs`).

- **loopback 직결**: 인라인 host 가 `127.0.0.1:PORT` / `localhost:PORT` 면 SSH 터널 없이 그 포트로 직접 attach(동일 머신 다중 인스턴스 검증).
- **원칙 3**: `remote_workspace` 가 None 이면 자동 attach 를 skip 한다(ID 명시 필요). 자동 attach 는 mirror 워크스페이스를 *추가*만 하며 사용자의 포커스/active 전환을 강제하지 않는다(원칙 1 ①).
- **중복 방지**: 이미 attach 중인(anchor) 워크스페이스는 재트리거하지 않는다. force-detach/EOF 시 정리돼 재활성 시 재attach 가능.

## force-detach

서버 권한으로 점유를 강제 해제한다(attach 하지 않음). holder client 에 `Control{force_detached}` + `Detach` 를 push → client 가 mirror 를 정리·종료하고 서버는 lock 을 free 환원한다.

- IPC: `attach.force_detach{surface_id}` / `attach.force_detach_workspace{workspace_id}`(JSON-RPC).
- CLI: `tasty remote attach --force-detach`(surface) / `--workspace <id> --force-detach`(workspace).
- **`--ssh` + `--force-detach` 조합은 미지원**(에러). force-detach 는 이 서버에 붙은 원격 클라이언트의 락을 끊는 **로컬 JSON-RPC** 이지, 터널 너머 원격 서버의 락을 끊는 동작이 아니다.

## 원격 생존 확인 — `tasty remote check`

원격 tasty 인스턴스가 *지금 살아있는지* 단발 판정한다(`crates/tasty-cli/src/commands/remote_check.rs`).

- **왜 포트 발견만으론 부족한가**: 포트 발견(`tasty port` 서브커맨드 / 포트 파일)만으로는 **stale 포트 파일**(이미 죽은 인스턴스가 남긴 파일)을 살아있다고 오판할 수 있다.
- **3단계 판정**: ① 포트 발견 → ② `ssh -L` 터널 수립 → ③ 터널 localport 로 가벼운 IPC(`system.info`) **1회**. 응답이 와야만 **alive**. 연결 거부 / EOF / 타임아웃(5초) = **dead(stale 포트)**.
- **출력 규약**: alive 면 `alive: <dest> (port …, version …, N workspaces)` 를 stdout 에 출력하고 **exit 0**. 어느 단계든 실패하면 시도한 발견 모드·실패 단계를 담은 에러를 stderr 로 내고 **exit≠0**.
- attach 와 달리 1회성이라 백오프 재연결을 하지 않는다. SSH 부품(포트 발견/터널)은 attach 와 완전히 공유하고, 서버측·로컬 list 핸들러는 건드리지 않는다.

## SSH 터널 동작 (원격 클라이언트 공통)

`tasty remote attach --ssh|--profile` 과 `remote check` 가 공유하는 SSH 결선(`crates/tasty-cli/src/ssh.rs`).

- **시스템 ssh 위임**: tasty 는 자체 원격 프로토콜/암호화를 만들지 않고 시스템 `ssh` 를 자식 프로세스로 실행한다. 사용자의 `~/.ssh/config`·agent·known_hosts 를 그대로 재사용한다. **Windows 는 시스템 OpenSSH 풀경로**(`%WINDIR%\System32\OpenSSH\ssh.exe`)를 우선한다 — git 번들 ssh 는 윈도우 ssh-agent(named pipe)를 못 봐 무암호 인증이 실패한다.
- **원격 포트 발견**: 기본 `auto` 모드는 **subcommand → file-unix → file-windows** 를 순서대로 시도해 원격 SSH DefaultShell 4 종(PowerShell/cmd/git bash/unix)을 모두 커버한다. `--remote-port-mode` 로 고정 가능. `--remote-tasty <path>` 로 원격 바이너리 경로 지정(기본 `tasty`, 원격 PATH 가정).
- **터널 생명주기**: detach/종료 시 자식 ssh 를 kill 해 고아 터널을 막되, **원격 데몬은 생존**한다(server-owns-PTY persistence = detach 의 본질). 자동 재연결(attach 한정): SSH/터널 끊김 시 지수 백오프(0.5s→30s)로 터널 재수립 + 재attach(`--no-reconnect` 로 끈다).
- **보안**: 로컬 끝점도 `127.0.0.1` 한정(`-L 127.0.0.1:…`, `-g` 금지). attach 채널에 자체 토큰을 강제하지 않고 SSH 사용자 인증 = attach 권한 경계로 환원한다("SSH 로 그 호스트에 들어올 수 있는 사람 = attach 자격", tmux 모델과 동일).

## IPC 표면 (`attach.*`)

| 메서드 | 경로 | 설명 |
|--------|------|------|
| `attach.acquire` / `attach.release` | `stream.open{target}` 핸드셰이크 | 배타 lock 획득/해제 |
| `attach.force_detach` / `attach.force_detach_workspace` | JSON-RPC | 점유 강제 해제 |
| `attach.into_gui` | JSON-RPC | 실행 GUI 가 원격 워크스페이스를 mirror 로 재구성 |
| `attach.list` | JSON-RPC | 현재 점유 목록 조회(read) |

권한: attach 보안은 **연결 경계(SSH + 127.0.0.1 loopback)** 에 위임한다(decision 5). 자체 권한 레이어를 두지 않으므로 추가 Permission 을 요구하지 않는다 — 소켓에 도달한 caller(별도 인스턴스 client·인증된 agent)는 attach 제어를 호출할 수 있다.
