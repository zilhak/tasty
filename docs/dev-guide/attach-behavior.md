# Attach 메커니즘

attach의 서버·클라이언트 처리와 연결·점유·복구 규칙을 설명한다.
사용법과 사용자에게 보이는 동작은 [원격 attach](../features/remote-attach/index.md)를 따른다.
서버는 loopback 연결을 받고, 로컬 직결과 SSH 터널의 선택은 클라이언트가 맡는다.

## 서버 / 클라이언트 계층 (가장 먼저 읽을 것)

attach 는 **server**(피점유 — PTY/grid 소유)와 **client**(점유 — mirror 표시) 두 쪽이다.

- **서버측** (`src/core/attach_runtime.rs`, IPC `attach.*`) — **transport 를 모른다.** 항상 `127.0.0.1` 로만 client 를 받는다. 로컬에서 붙든 SSH 터널 너머에서 붙든 서버 입장엔 전부 loopback 이다. 서버는 SSH 를 전혀 모른다.
  - 연결마다의 push sink·입력 프레임 분류·bulk 연결 결속은 `StreamHub`(`crates/tasty-ipc/src/stream_hub.rs`)가 든다. sink 는 채널이라 허브도 TCP 를 모른다 — 소켓을 읽고 쓰는 accept 스레드는 본체 adapter `src/adapters/production/tcp_ipc_server.rs` 에 있다. core 는 adapter 를 거치지 않고 크레이트의 허브를 직접 부른다([ADR-0001](../adr/0001-crate-dependency-boundaries.md)).
- **클라이언트측** — "원격성" 을 전부 흡수한다. 두 종류:
  - **로컬 client**: 포트 파일(`~/.tasty/tasty.port`)을 읽어 그 loopback 포트로 직결. **release 에서 제거 → debug 전용**(`tasty debug attach`).
  - **원격 client**: `ssh -L 127.0.0.1:<localport>:127.0.0.1:<remoteport> -N` 터널 후 그 **localport 로 직결**. 터널은 바이트 파이프라 스트림 프로토콜에 투명 — 원격 client 도 결국 자기 머신 loopback 에 붙는다(`tasty remote attach --ssh|--profile`).

### "로컬 attach 제거" 의 정확한 의미

> **attach 는 원격을 대상으로 한다** — 로컬 self-attach 는 release 에서 제거하고 debug 격리한다. 이 결정의 *근거·대안·재검토 조건* 은 [ADR-0020](../adr/0020-remote-connection-profiles.md). 아래는 그 결정이 구현에 어떻게 드러나는지다.

원격 attach 도 **서버 입장엔 loopback** 이다. 따라서 release 에서 "로컬 attach 제거" 는 **서버를 바꾼 게 아니라 client 의 로컬 진입점(`tasty attach` → `tasty debug attach`)만 제거**한 것이다. 서버의 attach 수신 경로는 로컬/원격 공용으로 보존된다. SSH 터널 + attach 세션 머신(`run_attach_*`)은 `crates/tasty-cli/src/local/attach.rs` 에 공용으로 남고, `remote`/`debug` 네임스페이스는 그 위에서 디스패치만 한다.

### `remote` / `debug` 는 CLI 디스패치 네임스페이스 (IPC 와 비대칭)

`remote attach`/`remote check`/`debug attach`는 CLI 명령 이름이다.
이 이름을 그대로 IPC 네임스페이스로 사용하지 않는다. CLI는 `attach.*`와 `system.info`를
이용하며, 원격 연결 여부와 debug 전용 진입점을 명령별로 구분한다.

## 점유 레지스트리 (`OccupancyRegistry`)

`src/core/attach.rs`. 휘발성(직렬화/복원 안 함 — 재시작 시 빈 registry → 전부 free).

이 registry 는 [ADR-0021](../adr/0021-occupancy-and-attach-admission.md) 의 **약한/강한(soft/hard) 2계층 점유**를 함께 담는다. **attach 는 강한(hard) 점유의 한 사례** — 아래는 그 hard 경로다. 약한(soft) 점유(advisory 마커, write 미차단; 현 소비자 = `terminal` 명령의 child-terminal)는 [`features/child-terminal`](../features/child-terminal/index.md) 를 보라. 두 계층은 같은 registry 에 있지만 별도 저장이다(hard=`surface_locks`/`workspace_locks`, soft=별도 soft 엔트리).

- `surface_locks: HashMap<SurfaceId, AttachLock>` — surface 단위 배타(hard) lock. `acquire` 가 동시 점유를 `AlreadyAttached{holder}` 로 거부.
- `workspace_locks` + `surface_to_workspace` — workspace 단위 점유. workspace 점유 시 멤버 *터미널* 은 `surface_locks` 에도 동일 holder 로 등록(서버측 placeholder 렌더·입력차단을 surface 단위와 동일 적용). 비-터미널은 역매핑(`surface_to_workspace`)으로만 "점유 표시".
- `release` (holder 본인) / `force_detach` (서버 권한) / `release_all_for_client`(연결 생존 판정 실패 시 일괄 — workspace + 멤버 + 잔여 surface). 트리거는 4 종: **self-release**(holder 의 명시적 release) · **force-detach**(로컬 사용자 강제 해제) · **EOF-or-TTL**(연결 종료 EOF 또는 attach heartbeat TTL 만료 — 둘 다 `tcp_ipc_server.rs` read 루프의 `Err(_) => break` 를 거쳐 `StreamInbound::Disconnected` → `release_all_for_client` 로 합류한다; TTL 은 소켓 read timeout 이 heartbeat 미수신으로 만료되는 경우로, 실제 EOF 와 동일한 `io::Error` 취급이라 코드 경로가 갈라지지 않는다) · **forwarded-op cascade purge**(forward 된 구조 변경 자체가 workspace 를 통째로 지우는 경우 — 예: workspace 의 마지막 surface 를 forward `CloseSurface` 로 닫으면 `close_case_workspace` 의 "Case 4: last pane in workspace" 가 workspace 를 purge 한다. `execute_forwarded_structural_op`(`src/core/attach_runtime.rs`)이 실행 후 `ws_id` 로 재조회해 delta 를 만들려다 실패하면, 더 이상 존재하지 않는 workspace 를 향한 delta 대신 `force_detach_workspace` 를 호출해 holder 를 강제 detach 하고 stale lock 을 정리한다). 강한 점유 해제 사유 확장의 근거는 [ADR-0021](../adr/0021-occupancy-and-attach-admission.md).
- **끊긴 holder 는 재attach 를 막지 못한다** ([ADR-0021](../adr/0021-occupancy-and-attach-admission.md)). inbound 한 배치(`PumpOutcome`)의 적용 순서는 attach 연결이 먼저, 연결 종료 *정리*가 마지막이다 — 끊긴 client 의 잔여 입력 프레임이 그 client 의 점유가 살아 있는 동안 적용돼야 하기 때문이다. 그래서 한 배치에 "C1 끊김" 과 "C2 attach" 가 함께 실리면 C2 가 곧 사라질 C1 의 lock 에 막힌다. 이를 막기 위해 두 pump(`App::apply_stream_outcome` · `boot::headless_stream::apply`)가 배치 **머리**에서 `mark_clients_disconnected` 로 *사실만* 먼저 알리고, `acquire`/`acquire_workspace` 는 자기를 막고 선 holder 가 그 표시를 가지면 그 자리에서 점유를 회수한다. 회수는 경쟁이 있을 때만 하므로 경쟁이 없는 잔여 입력은 그대로 처리된다. 표시는 `release_all_for_client` 가 lock 과 함께 지워 한 배치를 넘지 않는다.
- **입력 격리**: `apply_send_to_surface` 가 `is_hard_occupied` 면 서버 로컬 입력 거부, client 입력만 `feed_attached_input` 우회 경로로 PTY 도달. (soft 점유는 write 를 막지 않는다 — hard 만 격리.)
- **점유는 핸드셰이크가 검증된 뒤에만 잡힌다** ([ADR-0021](../adr/0021-occupancy-and-attach-admission.md)). 점유를 잡는 유일한 진입점은 `dispatch_stream_attach` → `attach_workspace_for_stream`/`attach_surface_for_stream` 인데, 그 **앞에** `tcp_ipc_server.rs::validate_stream_proto` 가 있다. `stream.open` params 의 `proto` 가 `STREAM_PROTO` 와 다르면(생략 시 serde default `0`) attach 를 dispatch 하지 않고 `StreamAck{ok:false, proto, error}` 로 거절한다 — 점유가 애초에 잡히지 않는다. 없을 때의 문제: 프로토콜이 안 맞는 client 는 그 점유를 **쓸 수 없는데도** 가져가고, 소켓을 닫지 않는 구버전/hung peer 면 아래 EOF 가 오지 않아 heartbeat TTL(20초)까지 그 workspace 가 붙잡혀 정상 attach 가 `already_attached` 로 거절됐다. 거절 ack 는 client(`StreamConnection::open_with`)가 이미 검사하는 형식이라 실패 사유가 그대로 사용자에게 전달된다.
- **self-attach 는 그보다 앞, client 측 dispatch 에서 거절된다**: `attach_client/dispatch.rs::connect_unless_self` 가 요청 포트를 이 인스턴스의 IPC 포트와 비교한다(debug/release 공통). 이 경로의 핸드셰이크는 GUI 메인 스레드에서 동기 블로킹으로 도는데 그 응답을 만드는 것도 같은 메인 스레드라 자기 자신을 대상으로 하면 같은 스레드가 응답을 만들 수 없어 대기하다 실패하고, 실패하는 동안 대상 workspace 점유만 남는다. 서버 accept 층에서는 막을 수 없다 — 자기 자신과 `ssh -L` 로 도착하는 정상 원격 mirror 는 둘 다 loopback 연결이라 구분되지 않고, 요청 포트와 자기 IPC 포트를 함께 아는 것은 client 측 dispatch 뿐이다. 로컬 self-mirror 검증은 별도 프로세스인 `tasty debug attach` 로 한다.

## 초기 스냅샷 + delta

attach 직후 서버가 현재 visible 화면을 `snapshot_as_vt` 로 **1회** 직렬화 push(셀 속성 + 커서 + alt-screen/DECCKM/bracketed 모드 복원). 이후 변화는 output tap delta(Data 프레임). client 는 받은 바이트를 PTY 없는 mirror 터미널(`Terminal::new_detached` + `feed_bytes`)에 먹여 같은 termwiz 파서로 grid 재구성.

## workspace mux

한 연결로 N 터미널 출력을 나르므로 workspace 모드 Data 프레임은 **surface-prefixed**(`encode_mux`/`decode_mux`). surface 단위 단일 연결은 prefix 없음. attach 직후 서버가 `attached_workspace` Control 로 트리(분할 방향/비율) + per-surface 디스크립터를 보내고, client 는 원격↔로컬 surface_id 재매핑으로 트리를 재구성한다. per-surface role 은 5종:

- `{remote_id, role:"terminal", cols, rows}` — mirror 가능한 터미널.
- `{remote_id, role:"mesh", kind, plugin_id, display_name}` — bundled egui-mesh 화이트리스트 통과(mesh mirror, 아래 절).
- `{remote_id, role:"explorer", root}` — 활성 탭의 현재 디렉토리만 싣고 목록은 `list_dir` 채널로 lazy 조회([ADR-0022](../adr/0022-remote-mirror-content-and-queries.md)).
- `{remote_id, role:"markdown", file, display_name}` — 원문은 안 싣고 `markdown_content` 채널로 lazy 조회(아래 절, [ADR-0022](../adr/0022-remote-mirror-content-and-queries.md)). `file` 은 표시·제목 전용 opaque 문자열이라 client 가 그 경로로 자기 로컬 파일을 열지 않는다. **비어 도착할 수 있다** — 그 값은 plugin 의 `surface.create` snapshot 에서 오는데 그 도착이 `tab.create` 반환보다 뒤라, 갓 연 surface 에 곧바로 attach 하면 `role` 만 맞고 `file` 이 빈 문자열이다(핸드셰이크는 동기 경로라 host 가 기다리지 않는다). wire 상 "파일 없이 열린 surface" 와 구별되지 않으므로, 원문은 아래 채널로 가져온다(그 회신의 `file` 은 요청 시점 값이라 채워져 있다).
- `{remote_id, role:"placeholder", kind}` — 그 외 비-터미널, mirror 불가.

이 role 분류는 `build_workspace_tree_surfaces`(`src/core/attach_runtime.rs`)가 만든다. mesh 와 markdown 은 **두 단**이다 — `tasty-model` 이 `Surface::attach_mesh_info()`/`attach_content_info()` 로 후보만 모으고(crate 가 화이트리스트를 모른다), 앱 계층이 `is_egui_mesh_allowed`/`is_attach_content_allowed` 로 재검증해 떨어진 후보를 placeholder 로 내린다.

## 프레임 전송 지연 (Nagle 금지)

attach 스트림은 **프레임 하나 = 상호작용 하나**(키 입력 · 리사이즈 요청 · 구조 op · 출력 delta)다. 그래서 프레임 단위 전송 지연이 그대로 체감 지연이 된다 — 본문·헤더를 나눠 쓰거나 Nagle을 켜면 전송 지연이 늘 수 있다.

- **헤더와 payload를 한 버퍼로 보낸다** — `tasty_ipc::stream::write_frame` 이 `[tag][len][payload]` 를 한 버퍼에 합쳐 `write_all` 1 회로 보낸다. 헤더와 payload를 나눠 쓰면 Nagle과 delayed ACK 때문에 전송 지연이 생길 수 있다. write_all 호출 수가 실제 시스템 호출이나 TCP 세그먼트 수를 보장하지는 않는다. `crates/tasty-ipc` 의 유닛 테스트 `stream::tests::write_frame_emits_one_write_call` 이 시험 writer의 호출 횟수를 확인한다.
- **양쪽 소켓 모두 `TCP_NODELAY`** — 서버 `prepare_stream`(`src/adapters/production/tcp_ipc_server.rs`, 일반 JSON-RPC 연결 포함)과 client `StreamConnection::open_with`(`crates/tasty-ipc/src/client/stream.rs`). 위에서 합쳐도 payload 가 MSS 를 넘으면 마지막 조각이 다시 Nagle 에 걸리므로 이중 방어다.
- 호스트 ↔ plugin 메인 채널도 같은 두 가지를 한다 — 한 줄을 한 번의 write 로(`tasty_plugin_protocol::write_line`), 양 끝 `TCP_NODELAY`. [`plugin-development.md` "전송 지연"](plugin-development.md#전송-지연-nagle-금지).

**SSH 터널은 이 지연을 흡수해 주지 않는다.** 터널 너머든 아니든 tasty 소켓의 양 끝은 항상 loopback이고, 위 지연은 그 loopback 구간에서 발생한다. 그리고 mirror 를 다시 mirror 하는 다단 구성(A → B → C)에서는 홉마다 입력·출력 양방향으로 얹히므로 지연이 홉 수에 비례해 누적된다.

## 밀어내기 실패와 누적 손실

`StreamHub::push`는 client 때문에 host를 멈추지 않는다. 큐가 가득 차면 프레임을 버리고
Dropped를 반환하며, 연속 실패가 LAG_LIMIT을 넘으면 연결을 끊는다.
호출자가 Dropped 뒤 다음 데이터를 계속 보낼 수 있으므로 연결이 살아 있어도 출력 중간이 빠질 수 있다.

| 값 | 의미 | 외부 조회 |
|---|---|---|
| `lag` | 연결별 연속 drop. 데이터 전송 성공 시 0 | 내부 연결 해제 판단 |
| `frames_dropped` | 살아 있는 연결에서 버린 프레임 누계 | system.pressure.stream_push |
| `clients_lagged_out` | 지연 한도로 끊은 연결 누계 | system.pressure.stream_push |
| `backlog` | 살아 있는 연결의 큐에 아직 남은 프레임 합 | system.pressure.stream_push |

backlog는 연결별 queued 카운터로 계산한다. push 성공에 증가하고 SinkReceiver가 꺼내면 감소한다.
끊긴 연결은 합에서 제외한다. sink_capacity와 함께 읽으면 현재 적체를 해석할 수 있다.
누계는 어느 client의 어느 출력이 빠졌는지를 알려주지 않으므로 별도 Loss 통지를 사용한다.

### client 에게 공백을 알린다 (`Loss` / `client_loss_notify`)

서버는 `ipc.stream.loss-notify` capability를 알린다. client가 `ClientLossNotify {}`를 선언하면
직전 통지 뒤 이 연결에서 버린 프레임 수를 `Loss { frames }`로 보낸다.
GUI mirror와 CLI surface/workspace dump·raw bridge가 선언한다. attach가 아닌 debug echo 스트림은 선언하지 않는다.
선언하지 않은 peer에는 Loss를 보내지 않는다. STREAM_PROTO는 정확히 같은 버전만 연결하므로
기능 추가만으로 번호를 올려 구 peer를 거절하지 않는다.

큐가 가득 차 통지도 넣을 수 없으면 `pending_loss`에 수를 보관한다.
`SinkReceiver`가 한 프레임을 꺼내는 즉시 통지를 넣어 마지막 보존 데이터 뒤에 도착하게 한다.
성공한 때만 pending 수를 지운다. push 앞과 pump_inbound 끝에서도 시도하므로
손실 뒤 늦게 선언한 client가 이미 큐를 비운 경우도 처리한다.

평소 write 스레드는 원자 `owes_notice`만 읽으며 통지가 있을 때만 sink map을 잠근다.
선언·drop·통지 성공 시 원자 사본을 갱신한다. 수신 측은 map의 Weak를 사용하며 sender를 소유하지 않는다.
따라서 registry에서 연결을 지우면 write 스레드도 채널 종료를 알 수 있다.

Loss를 보냈다고 lag를 초기화하지 않는다. 서버가 만든 통지 성공은 client가 데이터를 따라잡았다는 뜻이 아니다.
이 때문에 push 한 번당 한 칸만 비우는 client는 통지가 그 공간을 차지해 데이터 전송에 계속 실패할 수 있다.
두 칸 이상을 비우는 경우와 구분해 부하를 검증한다.

알려진 경합도 있다. push가 통지를 넣지 못한 직후 write 스레드가 공간을 만들면 데이터 한 장이 Loss보다
먼저 들어갈 가능성이 있다. 과거 실행 실험에서는 재현하지 못했다. 실제 앞지르기 관측이나 원자적 삽입 수단이
생기면 이를 다시 검토한다. 손실 client는 아래 재attach로 화면을 다시 받는다.

### 통지를 받은 client 가 하는 일 (재동기화 계약)

Loss는 surface나 데이터 종류를 담지 않는다. 한 연결이 여러 종류를 운반하면 가장 강한 복구 방법을 적용한다.

| 종류 | 복구 |
|---|---|
| PTY delta | 새 snapshot을 받기 전에는 불연속 출력을 정상 화면으로 간주하지 않음 |
| Resize·Activity·Attention·Cwd·구조 상태 | 새로운 값을 받을 때까지 오래된 상태로 표시 |
| mesh | 조립기와 캐시를 버리고 full texture부터 다시 구독 |
| bulk | 결과 불명인 중단으로 처리; 이미 commit됐을 수 있어 자동 재시도하지 않음 |

snapshot과 output tap을 같은 순간에 확보하는 경로가 attach이므로 재attach를 사용한다.
Detach를 보낸 뒤 이전 연결의 EOF를 확인하고 새로 연결한다.
서버는 Disconnected를 inbound에 넣은 뒤 소켓을 닫아 이전 점유 정리가 새 요청보다 먼저 처리되게 한다.
raw bridge는 서버가 HEARTBEAT_TIMEOUT 안에 닫지 않으면 대기를 끝내고 연결을 다시 시도한다.

- GUI mirror는 손실을 표시하고 한 세션에서 재attach를 한 번에 하나만 진행한다.
  진행 중 추가 통지는 합산하며, 완료 뒤 다시 손실이 나면 재attach할 수 있다.
  기존 reconnect_session이 survivor ID와 scrollback을 유지하며 mesh frame·구독 중복 방지 캐시는 초기화한다.
  실패하면 anchor가 있는 세션은 Reconnecting, 나머지는 정리한다.
- parked engine은 stream ID와 오래됨 표시만 갱신하고 연결은 유지한다. 창을 복원하면
  `resume_resync_in_window`가 재attach를 시작한다. AttachClientData 또는 3초 AttachView가 이 처리를 실행한다.
  이 호출 연결은 기존 단위 테스트만으로 검증되지 않는다. 실제로 park→손실→창 복원을 재현해야 한다.
- 창에서 시작한 재attach가 EOF를 기다리는 사이 park되면 anchor 없는 세션이 정리될 수 있다.
  소스에서 확인한 한계이며 실행 재현이 확인된 것은 아니다.
- CLI dump는 최대 세 번 재attach하고 수집을 처음부터 시작한다. 그 뒤 손실은 결과를 출력하면서 stderr로 알린다.
  `--send`는 첫 attach에서만 실행한다. raw bridge는 제한 없이 재attach하고 매번 stderr로 알린다.
  전환 중 일반 stdin은 버릴 수 있지만 EOF와 Ctrl+\는 종료로 처리해야 한다.
- bulk는 Loss를 받으면 Detach 후 중단 오류를 반환한다. 서버 거부를 뜻하는 BULK_REJECT_PREFIX는 붙이지 않으므로
  이미지 업로드 UI의 수동 재시도는 가능하다.

Loss와 재연결 snapshot에서 mirror Terminal의 output stream ID를 바꾼다.
이전 ID를 가진 위치 읽기는 stream 불일치를 받으며, ID 없이 읽는 기존 소비자를 위해 위치 숫자는 이어 센다.
재attach snapshot 때문에 scrollback에 화면 한 벌이 중복될 수 있다.

parked 복구 검증은 두 격리 인스턴스에서 실제로 park 가능한 OS 경로를 사용한다.
client 수신을 늦춰 LAG_LIMIT 미만의 drop을 만든 뒤 복원하고, 지연 복구 로그와 새로운 attach를 확인한다.
Linux·Windows의 일반 최소화는 창을 유지하므로 macOS의 parked 동작을 확인한 것으로 보고하지 않는다.

## 갱신 cadence 분리

- **서버측 readonly 뷰**(피점유 — 대상 부하 절약): **3초 polling**(`Tick::AttachView`, `src/app/attach_poll.rs`)으로 self-snapshot 적용.
- **client mirror**(내가 다루는 대상): 원격 출력이 올 때마다 즉시 갱신, 3초 tick 은 backstop(누락 출력 적용·끊긴 세션 정리)으로만.

hard 점유된 서버 surface의 드래그 선택과 복사는 허용한다.
좌표 변환과 복사 문자열은 CoreState::visible_terminal이 반환하는 readonly mirror를 사용한다.
live 터미널을 읽으면 사용자가 보고 선택한 내용과 달라질 수 있다.
클릭 트래킹은 None으로 처리해 로컬 선택을 사용한다. 휠은 live scroll_offset이 바뀌는 문제 때문에 차단하고,
링크 실행은 지연된 화면에서 외부 파일·URL을 여는 부수효과 때문에 차단한다.
IME·vi 커서·링크·검색 하이라이트는 표시하지 않는다. soft 점유는 이 제한을 받지 않는다.

## 리사이즈 전파 (mirror geometry)

mirror grid 는 **client 가 구동(client-driven)** 한다(ADR-0022) — mirror 를 띄운 **로컬 pane 의 크기**가 grid 를 정하고, 원격 PTY 를 그 크기로 reflow 시킨다. "remote authoritative" 는 **메커니즘으로만 유지**: 원격 PTY 가 실제 크기를 확정하는 주체(reflow 담당)이고 그 settled 크기를 echo 로 되돌린다. 즉 **의도(intent)는 client, 확정(confirm)은 remote** 의 요청→확정 협상이다.

- **client 구동 (forward)**: mirror 는 detached 터미널(PTY 없음)이라, 매 프레임 도는 로컬 레이아웃 리사이즈 스윕(`Core::resize_all_terminals` / `AppState::resize_all`)이 detached 터미널을 로컬에 적용하는 대신, 목표 grid `(cols, rows)` 를 `CoreState.pending_resize_forward`(로컬 surface id → grid)에 넣는다(목표가 이미 현재 mirror grid 면 생략). 이 한 곳에서 걸러 모든 리사이즈 진입점(창 resize·divider drag·단축키·redraw)을 커버한다. `App::dispatch_pending_resize_forwards`(`about_to_wait`, gui)가 drain 해 로컬 id 를 세션 매핑으로 원격 id 로 치환하고 `StreamControl::ClientResize{surface_id, cols, rows}` 를 `Control` 프레임으로 forward 한다. 세션의 last-forwarded dedup 이 echo 왕복(약 1 RTT) 동안의 매 프레임 재전송을 억제한다(coalesce; 서버측 동일값 `resize_grid=false` no-op 이 2차 방어).
- **서버 적용 (server)**: `StreamHub::pump_inbound` 이 `ClientResize` 를 `PumpOutcome.resize_requests` 로 분류 → 메인루프(gui `event_handler`/headless `boot`)가 anchor 워크스페이스의 **holder 를 검증**(hard 점유 = geometry 구동 권한, ADR-0021)한 뒤 `CoreState::apply_attached_workspace_resize` 로 원격 **실제 PTY** 를 `Terminal::resize` 한다(reflow).
- **로컬 grid 는 echo 로만 갱신 (desync 방지)**: client 는 목표 크기를 로컬 mirror grid 에 **낙관적으로 먼저 적용하지 않는다**. 원격 reflow 전 잘못된 grid 에 바이트가 재생되는 desync 를 막기 위해, mirror grid 는 아래 `Resize` echo 가 도착할 때만 바뀐다.
- **원격→client 확정 echo**: 원격 터미널 grid 가 실제로 바뀌면(`TerminalState::resize_grid` 이 `true`) 서버의 resize tap(`Terminal::add_resize_tap`)이 새 `(cols, rows)` 를 fan-out 하고, attach forwarder 스레드가 `Control` 프레임에 `StreamControl::Resize{surface_id, cols, rows}` 를 실어 client 에 push 한다. workspace 모드는 `surface_id` 에 remote surface_id 를 실어 client 가 remote→local 매핑으로 해당 mirror 만 리사이즈한다. **이 echo 경로는 client-driven 전환 전과 무변경 재사용** — client 요청이 원격을 구동하면 결과가 이 경로로 되돌아온다.
- **순서 보존**: client reader 는 Data(출력)와 Resize 를 **한 버퍼에 도착 순서대로**(`MirrorEvent`) 쌓고, 메인 스레드가 순서대로 적용한다 — resize 앞뒤 출력이 올바른 그리드에서 재생되도록.
- **`StreamControl` 확장성**: `event` 태그 기반 enum. mid-session 이벤트는 새 `StreamTag` 없이 variant 로 추가한다 — 현재 `Resize`(server→client, 확정 echo), `Activity`(server→client, busy/idle 활동 상태), `Cwd`(server→client, surface cwd — 아래 "surface cwd 전파" 절), `ClientResize`(client→server, geometry 구동 요청), `ClientAttentionClear`(client→server, 주의 환기 해제 edge), `StructuralOp`(client→server, 구조 변경 forward), `StructuralResult`(server→client, forward 회신), `StructuralDelta`(server→client, 구조 역반영), `MeshContext`(client→server, mesh 구독+geometry/theme/focus — 아래 "mesh mirror 채널" 절), `MeshInput`(client→server, 누적 입력), `MeshFullResendRequest`(client→server, 텍스처 delta 체인 복구 요청), `MeshError`(server→client, mesh 단발 실패 통지), `Attention`(server→client, 주의 환기), `BulkBegin`/`BulkCommit`(client→server, bulk 전송)·`BulkResult`(server→client), `Loss`(server→client, 이 연결에서 버린 프레임 수 — 위 "client 에게 공백을 알린다"), `ClientLossNotify`(client→server, 그 통지를 받겠다는 선언). 알 수 없는 event 는 역직렬화 실패로 무시(전방/후방 호환 — **다만 그 무시가 공짜일 때만 안전하다**: `Loss` 는 못 읽으면 "손실 없음" 으로 읽히므로 위 선언으로 게이트한다) — 구버전 서버는 `ClientResize` 를 무시해 기존 remote-authoritative 로 graceful degrade 한다. 단발 핸드셰이크 디스크립터(`attached`/`attached_workspace`/`attach_error`)와 `force_detached` 는 여전히 ad-hoc JSON 으로 위치 기반 파싱.

## 활동(busy) 상태 전파

사이드바 워크스페이스 리스트의 "실행 중" status dot(`WorkspaceEntryView.busy_count`, `docs/features/remote-attach/index.md` "GUI mirror" 참조)은 surface 의 busy/idle 상태를 본다. mirror(detached) 터미널은 로컬 PTY 가 없어 `CoreState::refresh_busy_surfaces`(foreground-process 폴링, `src/core/state/busy.rs`)가 절대 채울 수 없으므로, **원격이 직접 자기 surface 의 busy 상태를 계산해 client 로 forward** 한다 — resize 의 client-driven 협상과 반대로, busy 는 순수 **server→client** 단방향이다(원격 foreground 프로세스 이름은 애초에 client 에 존재하지 않는 정보라 client 가 요청할 수도 없다).

- **서버측 계산·forward**: 서버는 1Hz `Tick::Busy`(gui/headless 공통 — `crates/tasty-timer` 의 중앙 타이머 허브가 발화, [`timer-hub.md`](timer-hub.md) 참조)마다 `CoreState::forward_busy_activity`(`core/attach_runtime.rs`)를 호출한다. 이 메서드는 `busy_activity_forwards`(`core/state/busy.rs`)가 계산한 **점유 중인 surface 의 busy 값 변화분**만 `StreamControl::Activity{surface_id, busy}` 로 그 workspace/surface 의 holder client 에 push 한다. `last_forwarded_busy` 캐시로 값이 실제로 바뀐 경우에만 forward(중복 억제)하되, surface 가 점유 해제됐다가 재점유되면(다른 client 일 수 있음) 캐시를 버려 값이 이전과 같아도 항상 fresh 하게 1회 다시 push한다. 캐시는 **(holder, 값)** 을 기억한다 — 점유 해제 엔트리를 버리는 것은 점유 공백이 tick 경계를 넘을 때만 효과가 있고, 해제와 다른 client 의 획득이 한 tick 창 안에 끝나면 엔트리가 살아남으므로 holder 가 달라진 것을 그 자체로 변화로 센다(cwd push 와 같은 규칙) — resize 의 `last_forwarded_resize` dedup 과 동형이나 방향이 반대(client→server 아닌 server→client)다.
- **headless 는 별도 ticker 스레드가 필요 없다**: headless 메인 루프(`boot::run_headless`)가 `rx.recv_timeout(next_deadline)` 로 직접 타이머 허브의 다음 데드라인까지만 대기하다 `Tick::Busy` 를 스스로 드레인한다 — gui 전용 스레드나 이벤트 브리지에 의존하지 않는다([`timer-hub.md`](timer-hub.md) "headless — `run_headless`" 절 참조). **원격 attach 의 주 시나리오가 headless 서버**이므로 이 tick 이 없으면 활동 상태 forward 가 전혀 동작하지 않는다.
- **client 적용**: reader 스레드가 `Activity` Control 프레임을 `MirrorEvent::Activity(remote_surface_id, busy)` 로 버퍼링하고, `apply_attach_client_output` 이 세션의 `remote_to_local` 매핑으로 로컬 mirror surface id 를 찾아 `CoreState::set_mirror_surface_busy` 를 호출한다. 이 값은 **`busy_surfaces`(로컬 폴링 결과)와 분리된 `mirror_busy_surfaces` 별도 집합**에 저장된다 — 같은 집합에 합쳤다면 1Hz `refresh_busy_surfaces` 가 매 tick 로컬 폴링 결과로 집합을 통째로 교체하며 mirror 값을 지워버렸을 것이다. `is_surface_busy`/`any_busy`/`busy_count`(사이드바·상태바·탭 바 busy dot·`surface.list` IPC 가 공유하는 단일 진입점)는 두 집합의 합집합을 본다.
- **정리**: mirror surface 가 없어지면(`cleanup_mirror_workspace`, `apply_mirror_structural_delta` 의 removed 처리) `CoreState::forget_mirror_surface_busy` 로 `mirror_busy_surfaces` 에서도 제거해, 로컬 id 가 재사용될 때 stale busy 값이 새 surface 에 잘못 붙는 것을 막는다.

## surface cwd 전파

mirror 터미널은 로컬 PTY 가 없어 `Terminal::get_cwd` 의 pid 폴백이 돌지 않는다 — 원격 셸이 OSC 7 을 방출해 출력 바이트에 실려 올 때만 cwd 를 안다. 서버는 PTY 를 소유하므로 OSC 7 이 없어도 OS 조회로 안다. 그래서 busy·attention 과 같은 형태로 **서버가 cwd 를 push** 한다(server→client 단방향, [ADR-0022](../adr/0022-remote-mirror-content-and-queries.md)).

- **서버측 계산·forward**: 같은 1Hz `Tick::Busy` 에서 `CoreState::forward_surface_cwd`(`core/attach_runtime.rs`)가 `surface_cwd_forwards`(`core/state/surface_cwd.rs`)의 계산 결과를 `StreamControl::Cwd{surface_id, cwd}` 로 holder 에 push 한다. 값은 서버 **자기 트리** 기준의 `CoreState::surface_cwd` — terminal 은 OSC 7 캐시 → PTY 프로세스의 OS cwd, 그 외 kind 는 `source_cwd()`(explorer root · markdown 파일 부모) — 이고 **모든 kind** 가 대상이다. `inherit_cwd` 설정은 보지 않는다(관측이지 실행이 아니다). 연결 지점은 busy/attention 과 같은 3 곳(`src/app/busy.rs` 의 main window·parked engine, `src/boot.rs` 의 headless).
- **diff 캐시는 (holder, 값)**: `last_forwarded_cwd` 는 값과 함께 holder 를 기억한다. 값만 기억하면 같은 tick 창 안에서 점유가 풀리고 다른 client 가 잡았을 때 엔트리가 점유 해제 `retain` 을 살아남아 새 holder 가 초기값을 못 받는다. 초기 push 는 값이 `None` 이어도 나간다.
- **값 소멸은 `cwd: null`**: 원격도 cwd 를 모르게 되면 `null` 이 나가고 client 는 엔트리를 지운다 — 표현이 없으면 옛 원격 경로가 영구히 남는다.
- **client 적용**: `MirrorEvent::Cwd(remote_surface_id, cwd)` → 세션 `remote_to_local` 치환 → `CoreState::set_mirror_surface_cwd`. 값은 `Terminal` 캐시가 아니라 **별도 맵 `mirror_surface_cwd`** 에 `RemoteCwd` 로 저장되고, `CoreState::surface_cwd` 가 mirror surface 에 대해 이 값을 우선한다. 원격 경로라 로컬 실행 자리로는 나가지 않는다([surface-cwd §3-2](../design/policies/cwd.md#3-2-원격-출처-cwd-는-로컬-실행-경로로-새지-않는다)).
- **정리**: busy 와 같은 teardown 두 곳(`remove_mirror_workspace_from_engine` · delta 의 removed 처리)에 더해, delta 의 survivor **kind 전환 전부**에서 `forget_mirror_surface_cwd` 를 부른다 — busy 는 terminal 에서 출발한 전환만 정리하지만 cwd 는 비-terminal kind 에도 있다.
- **구버전 호환**: 구버전 client 는 `Cwd` 를 파싱 실패로 무시하고, 구버전 server 는 보내지 않아 맵이 빈다 — 그때 mirror convert 는 종전대로 서버측 resolve 폴백(`execute_forwarded_structural_op`)으로 동작한다.

## 주의 환기(attention) 전파

attention의 생성·해제·표시 규칙은 [주의 환기 기능](../features/surface-highlight/index.md)에 모아 둔다.
원격 전달은 다음 순서로 처리한다.

1. 서버의 `forward_attention`이 1Hz `Tick::Busy`에서 `attention_forwards`의 변화분을 읽는다.
   main·parked·headless 모두 실행한다. 캐시는 holder와 kind를 함께 비교한다.
2. `Attention { surface_id, kind }`를 해당 holder에 보낸다. 원격 ID와 completion/needs_input 또는 null을 사용한다.
   host 타입과 wire 타입은 `AttentionKind::to_wire`/`from_wire`로 변환한다.
3. client reader가 MirrorEvent에 넣고, 적용할 engine을 찾은 뒤 원격 ID를 로컬 ID로 바꾼다.
   `set_mirror_surface_attention`이 기존 AttentionStore에 적용하므로 같은 테두리·탭·배지가 사용된다.
4. mirror의 실제 사용자 확인은 레코드를 제거한 경우에만 `pending_attention_clear_forward`에 넣는다.
   `dispatch_pending_attention_clear_forwards`가 원격 ID로 바꾸어 ClientAttentionClear를 보낸다.
5. server의 `apply_attached_attention_clear`는 surface의 workspace holder인지 검증하고 기본 clear 함수를 부른다.
   서버 로컬 GUI에 적용되는 hard 점유 검사를 이 함수에 다시 넣지 않는다.

서버 push와 mirror 삭제는 로컬 raise/clear 함수를 사용하지 않는다.
raise에는 mirror 차단이 있고 clear에는 서버로 전달할 메시지 생성이 있기 때문이다.
삭제 시에는 `forget_mirror_surface_attention`으로 정리한다. kind 변환은 같은 surface이므로 레코드를 유지한다.

차분 전송은 같은 값의 재전송을 보장하지 않는다. send가 실패해도 다음 tick의 값이 같으면 다시 보내지 않는다.
손실은 이 문서의 Loss·재attach 규칙으로 처리한다. 새 surface의 null 초기값이 매핑보다 먼저 도착해
버려져도 양쪽에 레코드가 없으므로 결과는 같다. 신규 attach는 매핑을 동기 구성한 뒤 reader를 시작한다.

## mesh mirror 채널

bundled egui-mesh surface(image/mesh_demo — `is_egui_mesh_allowed` 화이트리스트. markdown 은 WebView 본문을 사용해 이 목록에 포함되지 않는다 — attach 시 mesh 가 아니라 아래 "markdown content 채널" 로 mirror 된다)가 mirror pane 에 뜨면, 원격의 실제 plugin 프로세스가 그리는 GPU mesh 프레임을 client 로 스트리밍해 렌더하고 client 입력을 원격으로 forward 한다. 개념·기능 범위는 [features/remote-attach](../features/remote-attach/index.md#surface-단위-vs-workspace-단위), 헤드리스 부트스트랩·로컬 egui-mesh 파이프라인 자체는 [egui-mesh-channel.md](egui-mesh-channel.md#attach-mesh-mirror-소비-경로), 여기엔 attach 프로토콜 전송 경로만 설명한다.

- **구독 = `MeshContext` (별도 핸드셰이크 없음)**: client 가 mesh surface 를 그리기 시작하면 `StreamControl::MeshContext{surface_id, width_px, height_px, pixels_per_point, theme, focused}` 를 보내는 것 자체가 구독 신호를 겸한다 — capability negotiation 을 위한 별도 확인/ack 프레임이 없다. 서버 `MeshMirrorRegistry::upsert`(`src/core/mesh_mirror.rs`)가 이 정보를 최초 수신 시점에 등록하고, 이후 값이 실제로 바뀔 때만(geometry/theme/focus 변경 시) client 가 재전송한다(`forward_attach_mesh_context`, `src/view/main/attach_mesh_input.rs`) — 매 프레임 재전송하지 않는다.
- **`MeshInput` 누적**: 포인터/키/스크롤/IME 이벤트는 `AttachMeshForwardState.events`(client, dedup 없이 순서 보존)에 프레임마다 쌓였다가, 다음 redraw 의 `forward_attach_mesh_context` 호출에서 `RawInputWire{modifiers, events, ..}` 로 묶여 `MeshInput{surface_id, input}` 1회 전송된다. 서버는 `MeshMirrorRegistry::push_input` 이 `pending_events` 에 extend 하고 `last_modifiers` 를 갱신 + `dirty=true` 로 표시 — 구독이 없는 surface_id 면 `false` 를 반환해 서버 dispatch 가 `MeshError` 로 회신한다(아래).
- **frame 소비·forward (headless-as-server / gui parked engine)**: 실제 구동/relay 로직은
  `plugin_bridge::mesh_forward::forward_mesh_frames_for_engine`(`src/plugin_bridge/mesh_forward.rs`,
  gui/headless 공용)에 있다 — `mesh_mirror.take_dirty`/`take_pending_events`/`last_modifiers`
  를 읽어 로컬 `PluginManager` 에 `SurfaceSetContextParams.raw_input` 으로 흘려보내고, plugin
  이 그린 결과 mesh 프레임을 `StreamTag::MeshData` 청크로 client 에 push 한다(청크 분할
  이유: 텍스처 델타 포함 paint frame 이 `MAX_FRAME_LEN` 을 초과할 수 있음). 이 함수는 **로컬
  authoritative render loop(살아있는 window)가 없는 engine 전용** — 두 소비처가 있다:
  헤드리스 부트스트랩(`src/boot/headless_plugins.rs::pump_plugins` 가 매 tick 호출)과, gui
  인스턴스에서 macOS 최소화로 window 가 파괴되고 `CoreState` 만 `App::parked_states` 로
  옮겨진 engine(`App::about_to_wait`, `src/app/event_handler.rs` 가 매 tick `parked_states`
  전부를 순회하며 호출 — `window_lifecycle.rs` 의 창 복원이 `remove(0)` 으로 1개씩만
  꺼내므로, 여러 window 가 동시에 최소화돼 있어도 나머지는 계속 이 순회 대상으로 남는다).
  client reader 스레드는 `tasty_ipc::mesh_stream::MeshFrameAssembler` 로 청크를 재조립해
  `MirrorEvent::Mesh(remote_surface_id, generation, frame_seq, full_textures, bytes)` 를
  메인 스레드 버퍼에 쌓고, `AttachMeshFrameStore::update`(`src/core/attach_mesh_frames.rs`)에
  저장된 것을 GPU 렌더 경로(`render_attach_mesh_surfaces`, `src/gfx/gpu/egui_mesh_prepare.rs`)
  가 소비해 화면에 그린다.
- **frame 소비·forward (gui-as-server, 살아있는 window)**: gui 인스턴스가 attach 서버이고
  그 mesh surface 가 실제로 window 를 가진 경우는 `MainView::forward_mesh_to_attach_subscribers`
  (`src/view/main/egui_mesh.rs`, 매 redraw 마다 `forward_egui_mesh_context` 직후 호출)가
  대칭 역할을 한다. 다만 로컬 창의 `forward_egui_mesh_context` 가 화면에 보이는 mesh surface
  의 `set_context` 를 이미 매 프레임 권위 있게 구동하므로, 이 훅은 위 `forward_mesh_frames_for_engine`
  전체가 아니라 그 꼬리 로직(`plugin_bridge::mesh_forward::relay_mesh_frame_if_new`)만
  재사용해 별도 `set_context` 를 보내지 않고 **이미 만들어진 `EguiMeshFrame` 바이트를 읽어
  relay** 만 한다(로컬 authoritative loop 와 경합 회피 — mesh_mirror ctx 의 width_px/height_px
  는 attach client 가 요청한 값이라 로컬 렌더 해상도와 다를 수 있다). 예외는 attach 구독
  대상이 로컬 어디에서도 렌더되지 않는 surface(다른 탭/워크스페이스에 있어 로컬 target
  목록에 전혀 없음)인 경우뿐 — 이땐 plugin 이 그 surface_id 자체를 모르므로 경합 없이 이
  훅이 최소 `surface.create` + `set_context` bootstrap 을 1회 대신 보낸다(`find_egui_mesh_surface`
  로 메타데이터 조회). 이미 렌더 중인 surface 에 새 구독이 들어와 전체 텍스처가 필요하면
  (신규 구독/명시 재전송), 직접 보내지 않고 로컬 `MeshForwardState::pending_full` 메커니즘에
  위임해 다음 tick 의 authoritative loop 가 `need_full_textures` 를 실어 보내게 한다 —
  그동안(next tick 까지) 이 훅은 캐시된(델타뿐일 수 있는) frame 을 새 구독자에 흘리지 않고
  건너뛴다.
- **`MeshFullResendRequest` 복구**: 텍스처 델타 체인이 깨졌다고 판단되면(로컬 `EguiMeshRenderTarget` 의 generation 검증 실패 등) client 가 `attach_mesh_full_requests`(`GpuState`)에 surface_id 를 쌓고, `App::dispatch_pending_mesh_full_resend_forwards`가 `MeshFullResendRequest{surface_id}` 를 서버로 forward 한다. 서버는 이를 받으면 `MeshMirrorRegistry::request_full_resend` 로 해당 surface 의 다음 프레임을 풀 텍스처 포함(full_textures=true)으로 강제한다.
- **`MeshError` — 명시적 단발 실패**: 구독 안 된 surface 로의 `MeshInput`(홀더 불일치 포함, `CoreState::apply_attached_mesh_input` 의 holder 검증) 등은 조용히 drop 하지 않고 `MeshError{surface_id, reason}` 를 1회 회신한다(설계 결정: 조용한 drop 보다 명시적 실패가 디버깅에 유리). client 측 소비는 현재 로그 레벨 처리만(재시도/toast 없음) — 세션이 정상이면 애초에 발생하지 않는 방어적 경로.
- **App/CoreState 경계를 건너는 forward-queue 패턴**: `MainView`(redraw 시점)는 `App.attach_client_sessions`(소켓 writer 보유)에 접근할 수 없다. `forward_attach_mesh_context`/입력 캡처(`mouse.rs`/`keyboard.rs`/`ime.rs`)는 `CoreState.pending_mesh_context_forward`/`pending_mesh_input_forward`/`pending_mesh_full_resend_forward` 에 쌓아두기만 하고, `App::about_to_wait`(`attach_client.rs`)의 `dispatch_pending_mesh_*_forwards` 가 다음 tick 에 drain 해 세션 매핑으로 원격 id 를 치환한 뒤 실제 소켓 write 를 한다 — `pending_resize_forward`/`dispatch_pending_resize_forwards`(ADR-0022)와 동형 패턴을 3개 방향(context/input/full-resend)에 재사용한 것.
- **gui-as-server 의 살아있는 window 는 attach client 의 입력 역방향 forward 를 아직 로컬 plugin 에 되먹이지 않는다**: 위 "gui-as-server, 살아있는 window" 훅은 mesh 바이트 forward(서버→client)만 구현한다 — `MeshInput` 으로 도착해 `MeshMirrorRegistry::push_input`/`pending_events` 에 쌓인 attach client 의 클릭/키 입력을 로컬 plugin 의 `raw_input` 에 병합하는 연결은 `forward_mesh_frames_for_engine`(`take_pending_events` 소비)에만 있다 — gui 살아있는 window 는 이 함수를 쓰지 않으므로(경합 회피, 위 참조) 후속 작업으로 남는다. **예외**: gui parked engine 은 headless 와 동일하게 `forward_mesh_frames_for_engine` 을 그대로 쓰므로, window 가 최소화돼 있는 동안은 오히려 입력 forward 가 이미 동작한다 — window 복원 후 살아있는 window 로 전환되면 다시 이 제약이 적용된다.
- **surface 디스크립터의 display_name**: `build_workspace_tree_surfaces` 가 보내는 mesh 디스크립터는 `{remote_id, role:"mesh", kind, plugin_id, display_name}` — `Surface::attach_mesh_info()`(kind/plugin_id 만 반환)와 별개로, 이미 존재하는 `Surface::display_name()`(`EguiMeshSurface` 는 실제 파일명 등을 반환)도 함께 조회해 실어보낸다. client 의 `MirrorMeshInfo` 구성(`merge_survivor_mapping`)은 이 필드를 우선 쓰고, 필드가 없는 경우(구버전 서버 등)에만 `kind` 문자열로 fallback한다 — 과거엔 이 필드 자체가 없어 탭 타이틀이 항상 mesh kind(예: `"image"`)로 뭉뚱그려졌다(image/mesh_demo 전부 동일 증상 — 당시엔 markdown 도 이 경로를 탔으나 현재 WebView를 사용해 mesh 디스크립터 대상이 아니다).

## markdown content 채널

markdown surface 는 webview kind 라 plugin 에 egui-mesh paint 채널이 없다([ADR-0029](../adr/0029-webview-host-integration.md)). 그래서 mirror 는 렌더 결과가 아니라 **원문 문자열**을 나르고, client 의 markdown plugin 이 자기 테마·자기 recent 로 다시 그린다. 결정의 근거·대안·재검토 조건은 [ADR-0022](../adr/0022-remote-mirror-content-and-queries.md), 여기서는 전송 경로를 설명한다.

- **핸드셰이크는 좌표만 싣는다**: `{remote_id, role:"markdown", file, display_name}`. 원문은 트리 디스크립터를 문서 크기만큼 부풀리므로 싣지 않는다 — client 가 필요할 때 아래 채널로 따로 가져온다(lazy).
- **client 의 surface 구성**: `merge_survivor_mapping`(`src/app/attach_client.rs`)이 `role:"markdown"` leaf 를 `EmptySurface` 대신 registry 의 `markdown` kind 로 만든다 — 조건은 그 kind 가 **번들 plugin(`com.tasty.markdown`)의 것**인가 하나다. kind 가 **아직 등록되지 않았으면**(꺼져 있던 plugin 을 세션 중에 켜는 경우 등) leaf 는 layout 복원과 같은 kind 대기 placeholder(`EmptySurface::new_deferred_plugin`, `deferred_mirror_markdown_surface`)가 되고, 표시 시점의 reify(`CoreState::reify_plugin_surface`)가 kind 등록 뒤 registry 의 `restore` 로 실제화한다 — plugin 에는 `surface.restore` 의 data 로 생성 params 와 같은 모양이 가서 plugin 은 어느 경로든 `remote` 키로 mirror 문서임을 안다(탭 제목은 그 경로에 생성 params 가 없어 plugin 이 응답의 `display_name` 으로 돌려준다). 같은 이름을 **다른** plugin 이 이미 등록했으면 대기하지 않고 빈 surface 로 남는다 — reify 는 소유자를 가리지 않아 그 plugin 으로 실제화되기 때문이다. 생성 params 는 `{display_name, remote:{file}}` 이다: 최상위 `file` 이 아니라 `remote.file` 에 싣는 것은 plugin 이 그 경로로 **자기 로컬 파일을 읽지 않게** 하려는 것이다(최상위 `file` 은 로컬 열기·cwd 도출·recent 기록을 모두 켠다). 구조 delta 로 트리가 다시 와도 이미 markdown 이던 leaf 는 `RemoteSurface::share_handles` 로 **같은 plugin surface 를 재사용**한다 — 새로 만들면 plugin 이 문서를 다시 받는다. `RemoteSurface` 는 drop 이 plugin 에 destroy 를 보내지 않으므로, 트리에서 빠진 leaf·재연결로 사라진 leaf·mirror 정리는 `destroy_mirror_markdown_surfaces` 가 명시적으로 destroy 한다. 세션이 든 로컬 markdown id 집합은 `AttachClientSession.markdown_locals` 이고 아래 요청·회신의 인가 대상도 이 집합이다.
- **plugin → host 요청**: markdown plugin 은 `markdown_mirror.content_request {surface_id(로컬), agent_origin?}` 를 호출한다(`agent_origin: true` 는 바깥 호출자의 `markdown.reload` 가 건 요청 표시 — 아래 절단 표시 참조. `src/adapters/ipc/handler/markdown_mirror.rs`, 권한 `fs.read`). host 는 `request_id` 만 즉시 회신하고 `CoreState.pending_markdown_content_forward` 에 쌓는다 — 원문은 attach 왕복을 기다려야 해서 `git_viewer.query` 와 같은 비동기 accept 다. namespace 가 `markdown.` 이 아닌 것은 그 prefix 가 plugin 으로 forward 되기 때문이다([ADR-0026](../adr/0026-plugin-registration-and-lifecycle.md) 참고). `App::dispatch_pending_markdown_content_forwards`(`about_to_wait`)가 drain 해 원격 id 로 치환한 뒤 아래 조회 이벤트를 보낸다. 세션을 못 찾거나 송신이 실패하면 그 자리에서 같은 `request_id` 로 `ok:false` 결과를 plugin 에 돌려준다.
- **host → plugin 결과**: 회신 이벤트는 로컬 id 로 치환돼 `markdown_mirror.content_result {surface_id, request_id, ok, file, source, truncated, reason}` host event 로 번들 plugin 에만 unicast 된다. 연결이 끊겨 Reconnecting 에 들어가면(`enter_reconnecting`) 그 세션의 모든 markdown leaf 에 `request_id: 0` + `ok:false` 를 보낸다 — `0` 은 발급되지 않는 값이라 "연결이 끊겼다" 는 abandon 신호다. plugin 은 대기 중인 요청을 끝내고, 원문을 이미 표시 중인 문서도 **끊김 상태**로 둔다(`MdDoc::apply_remote_result`) — 끊김 상태의 렌더는 로딩·실패·원문보다 앞서 끊김 문구 하나다(`render_document`). 끊김은 다음 성공 회신이 지운다. **재연결**(`reconnect_session`)은 survivor leaf 의 plugin surface 를 `share_handles` 로 이어 받아 원문 요청이 저절로 나가지 않으므로, `Connected` 로 되돌린 직후 세션의 로컬 markdown id 마다 아래 변경 신호(`markdown_mirror.changed`)를 한 번 보낸다. 근거·대안은 [ADR-0022](../adr/0022-remote-mirror-content-and-queries.md).
- **조회 (client→server)**: `{"event":"markdown_content_request", "request_id", "surface_id"}`. `list_dir`/`git_query` 와 같은 형태 — `StreamControl` enum **밖**의 raw JSON `event` 태그를 같은 `StreamTag::Control` 채널에 싣고, 서버는 알 수 없는 event 를 조용히 무시한다(전방/후방 호환). 파싱은 `tasty_ipc::stream_hub::MarkdownContentRequestMsg`, 소비는 `event_handler.rs::apply_markdown_content_request_msg`(gui, holder engine 순회)와 `boot/headless_stream.rs`(headless, 단일 engine).
- **회신 (server→client)**: 성공은 `{"event":"markdown_content_result", request_id, surface_id, ok:true, file, source, truncated}`, 실패는 같은 event 에 `ok:false` + `reason`. **에러 채널은 하나다** — client 가 그 `reason` 을 렌더의 `load_error` 로 옮긴다. **파일 없이 열린 markdown surface 는 에러가 아니다**: 서버에서도 빈 문서가 보이므로 `ok:true` + 빈 `file`/`source` 로 답한다.
- **파일은 서버 host 가 직접 읽는다** (`attach_runtime.rs::handle_markdown_content_request`). 서버측 markdown plugin 에 되묻지 않는다 — 이 핸들러는 동기 경로이고 plugin 왕복은 비동기라 그 안에서 기다릴 수 없다(`handle_git_query_request` 가 `tasty-git-core` 를 host 에서 직접 부르는 것과 같은 형태). 에러 구분 수준도 `list_dir_for_request` 와 같다(`permission denied` 대 그 외 io 에러 문자열).
- **예산**: `MARKDOWN_CONTENT_BYTE_BUDGET = 700 KiB` — `LIST_DIR_ENTRIES_BYTE_BUDGET`·`GIT_QUERY_BYTE_BUDGET` 과 같은 근거(프레임 하드 상한 `MAX_FRAME_LEN` 1 MiB 보다 충분히 작게)이자 **같은 재는 대상**: 원문 바이트가 아니라 `serde_json` 문자열이 된 뒤의 바이트다(감싸는 따옴표 포함). 이스케이프는 `"`·`\`·개행에서 2 배, 그 밖의 제어문자에서 6 배(`\u00XX`)까지 부푸므로 원문으로 재면 이스케이프가 많은 문서가 예산을 통과한 뒤 프레임 상한을 넘어 세션이 끊긴다 — 예산이 막으려던 바로 그 사고다. 그래서 실리는 **원문** 길이는 이스케이프가 많을수록 짧다(최악 약 116 KiB). 비용표는 `json_escaped_char_len` 이고 BMP 전수 대조 테스트가 serde_json 과의 어긋남을 잡는다. 자르는 자리는 **UTF-8 문자 경계**(`char` 단위로 걷는다), 잘리면 `truncated: true` 로 알린다. markdown plugin 의 대용량 게이트(1 MiB)와는 다른 층이며, 그 게이트를 건드릴 파일은 직렬화하면 더 커지므로 항상 잘려서 도착한다.
- **인가**: `client_holds_workspace(client_id)` 하나("attach 점유 = 신뢰"). 새 permission 토큰을 만들지 않는다 — 이 채널이 나르는 것은 SSH 로 붙은 사용자가 이미 읽을 수 있는 그 호스트의 파일 원문이다. **인가되는 집합은 engine 전체다** — 그 술어는 **어떤** 워크스페이스든 점유했는가만 보고(`workspace_locks` 전체 스캔), 대상 조회는 `find_surface_by_id`(전 워크스페이스 순회)라 W2 만 점유한 client 도 W1 의 markdown 원문을 받는다. list_dir(wire 에 `dir` 문자열만·워크스페이스 바인딩 필드 없음)·git_query(engine 전역 `TerminalStore` 조회 또는 임의 `worktree_path`)와 같은 갈래이며, anchor 의 holder 를 직접 검증하는 채널(입력·resize·구조 op forward, `workspace_holder(ws) == client`)과는 다르다.
- **절단 표시**: client 는 `truncated: true` 를 **toast**(`attach.toast.mirror_markdown_truncated`, `apply_markdown_content_result_event`) 로 알리고 문서 본문에는 심지 않는다 — `list_dir` 의 `filepicker.remote_listing_truncated` 선례와 같다(본문에 심으면 원문의 일부인지 구분되지 않고, markdown 은 심는 자리가 코드펜스 안일 수 있다). 단 `agent_origin: true` 로 건 요청(에이전트의 `markdown.reload`)의 회신은 toast 없이 로그로만 남긴다 — 송신 때 세션의 `agent_requests`(`src/app/attach_client/agent_origin.rs`)에 request_id 를 넣고 회신 때 꺼내 가른다([ADR-0036](../adr/0036-overlay-scope-and-lifetime.md)).
- **변경 신호 (server→client)**: `{"event":"markdown_changed", "surface_id"(원격)}`. 발신은 `attach_runtime.rs::notify_markdown_changed` 이고 호출처는 `webview.set_url` 핸들러(`src/adapters/ipc/handler/webview.rs`) 하나다 — markdown plugin 의 재렌더(파일 감시·`markdown.reload`·테마 변경·최초 생성)는 전부 그 IPC 로 host 에 오므로 plugin 을 고치지 않고 신호원을 갖는다. 그래서 신호는 실제 파일 변경의 **상위 집합**이다. 화이트리스트(`is_attach_content_allowed`) 밖 kind 는 신호가 없다. 수신자는 `OccupancyRegistry::workspace_holders` — 위 인가 술어가 참인 그 집합(이 engine 의 워크스페이스를 하나라도 점유한 client 전부, 중복 없이)이고, 그 surface 를 담은 워크스페이스의 holder 로 좁히지 않는 근거는 [ADR-0022](../adr/0022-remote-mirror-content-and-queries.md) 에 있다. 점유가 없거나 notifier 가 없으면 아무것도 안 한다. client 는 reader 체인의 `parse_markdown_changed` → `MirrorEvent::MarkdownChanged` 로 받아 `markdown_mirror_local` 로 로컬 mirror markdown leaf 에 매핑되는 것만 `markdown_mirror.changed {surface_id(로컬)}` 로 plugin 에 unicast 한다(자기가 mirror 하지 않는 문서의 신호는 버린다). plugin 의 반응은 문서가 무엇을 보여 주는가로 갈린다(`MdDoc::on_remote_changed`): 원문을 보여 주는 중이면 **다시 받지 않는다** — 새로고침 버튼 색만 바꾸고, 원문은 사용자가 누를 때 위 조회로 온다. 원문을 못 보여 주는 중(끊김·실패·로드 전)이고 요청이 진행 중이 아니면 다시 요청한다(재연결 신호가 이 갈래를 쓴다). 요청이 진행 중이면 무시한다. **서버가 헤드리스면 신호가 없다**: `webview.set_url` 이 gui 전용이라 헤드리스에서는 plugin 의 재렌더가 host 에 닿지 않는다(조회·회신은 헤드리스에서도 된다). 테스트: `attach_runtime.rs` 의 `markdown_changed_tests`(수신자 집합·중복 없음·점유 없음·화이트리스트 밖). `webview.set_url` 에서 그 함수로 이어지는 호출은 `webview.rs` 의 `set_url_on_markdown_surface_signals_attached_clients` 가 잰다(점유 client 의 스트림에 신호 한 번, 터미널 leaf 에는 없음 — 호출을 빼는 변이에서 실패한다).
- **서버는 헤드리스여도 된다.** 이 채널의 서버측 코드에는 feature 게이트가 하나도 없고(파일 read + control 프레임), 요청 소비도 두 조합에 각각 있다(gui `event_handler.rs`, headless `boot/headless_stream.rs`). 다만 **surface 를 만들 수 있어야** 그 앞이 성립하는데, 한때 헤드리스가 `webview` kind 를 등록하지 않아 `tab.create {type:"markdown"}` 이 `unknown surface kind` 로 죽었다 — 지금은 `boot/headless_plugins.rs` 의 `register_one_surface_kind` 가 세 rendering 을 전부 등록하고, 그 kind 를 지목한 생성 요청이 plugin 기동을 유발한다(`ensure_plugin_for_surface_kind`). 판정과 실측은 [headless-ipc-surface.md](headless-ipc-surface.md) 의 "등록된 kind 조회" 절.
- **테스트**: `tests/attach_markdown_content_loopback.rs` 가 role 직렬화·왕복·없는 파일·점유 없는 client 거절·예산 초과 절단·**다른 워크스페이스만 점유한 client 의 인가**(engine 전체임을 값으로 고정) 여섯을 loopback 프로토콜 레벨로 고정한다. 그 여섯의 **자동 채널은 헤드리스 조합 하나뿐**이다(`check-headless` 의 전체 스위트 — [ci-gates.md](ci-gates.md)), 그래서 위 항목이 헤드리스에서 성립하지 않으면 이 시험들에는 도는 자리가 없다. 절단 테스트는 본문을 따옴표로만 채워(원문 400 KiB → 직렬화 800 KiB) **무엇을 세어 잘랐는지**까지 잰다 — 원문으로 쟀다면 잘리지 않고 통과한다.

## mirror 구조 변경 forward

mirror 워크스페이스의 구조 변경(split/new-tab/close/move-tab/닫은 항목 복원)은 로컬에서 실행하지 않고 원격(authoritative)에서 실행되도록 forward 한다. intent 로 표현되는 경로(split·CLI/IPC)는 `Core::apply` 로 수렴해 거기서 forward 큐에 쌓이고, `Core::apply` 를 우회하는 UI-layer 직접 조작(`close_active_*`/`close_tab`/`add_tab`/`add_kind_tab`, 탭 드래그·컨텍스트 메뉴 이동)은 `AppState::forward_mirror_structural` 가 같은 큐(`pending_structural_forward`)에 대응 op 를 직접 쌓는다 — 두 경로 모두 동일 큐로 모여 아래 전송 경로를 공유한다. 개념·정책은 [features/remote-attach](../features/remote-attach/index.md#mirror-워크스페이스-내-구조-변경), 여기서는 전송 경로를 설명한다.

- **request (client→server)**: `Core::apply` 가 mirror 구조 op 를 로컬 차단(`MirrorStructuralBlocked`)하면서 `StructuralOp`(anchor = **로컬** surface id)를 `CoreState.pending_structural_forward` 에 push. `App::dispatch_pending_structural_forwards`(`about_to_wait`, gui)가 drain 해 anchor 를 세션 매핑(`AttachClientSession.remote_to_local` 역방향)으로 **원격 id 로 치환**(`StructuralOp::with_anchor_surface_id`)한 뒤 `StreamTag::Control` 로 전송. anchor 를 surface id 로 잡는 이유: client 는 pane/tab 의 원격 id 매핑을 갖지 않으므로, 원격이 surface 로부터 pane/tab/workspace 를 자기 트리에서 resolve 한다.
- **구조 변경 응답**: intent로 표현한 GUI·CLI/IPC 요청은 Core::apply에서 원격 큐로
  전달한다. MirrorStructuralBlocked의 forwarded=true는 로컬에서 실행하지 않고 원격 큐에
  넣었다는 뜻이다. GUI의 report_apply_error는 이를 로컬 차단 toast로 표시하지 않는다.
  IPC/CLI 구조 변경 핸들러는 structural_apply_error를 통해
  `{forwarded:true, workspace_index}`를 성공 응답으로 보낸다. 원격 완료는 이후 delta로 확인한다.
- **실패 표시**: 사용자 GUI 요청의 원격 실패는 toast로, 에이전트 요청은 silent_failure에
  따라 warn 로그로 남긴다([ADR-0036](../adr/0036-overlay-scope-and-lifetime.md)).
  markdown.navigate는 큐 등록 직후 `{accepted:true}`를 보내는 예외다. 이 응답에는 실행·forward
  결과가 없으며 에이전트의 로컬/원격 실패는 warn 로그로 남는다. 사용자 GUI의 같은 원격 작업은
  attach.toast.mirror_structural_forward_failed를 사용한다.
- **전달할 수 없는 요청**: mirror와 local 사이의 move-surface 등은 forwarded=false로
  오류를 반환한다. 헤드리스에는 이 큐를 보내는 GUI attach client가 없어 mirror 구조 작업을
  forwarded=false로 거절하고 빌드 조합을 사유에 적는다. 현재 헤드리스에는 mirror workspace를
  만드는 경로도 없다([ADR-0003](../adr/0003-headless-behavior.md)).

- **닫은 항목 복원(`RestoreClosedItem`)**: forward 대상이다 — 복원은 새 PTY spawn 이고 스냅샷의 스크롤백은 서버 디스크 참조(`ClosedScrollback::Persisted`)라 서버만 실행할 수 있다([ADR-0023](../adr/0023-attach-state-sync-and-forwarding.md)). 무엇이 복원될지는 클라이언트가 정하지 않고 **서버 스택이** 정하므로 op 에는 anchor 밖에 없다. **원격 스택이 비어도 로컬 스택으로 폴백하지 않는다** — 그 폴백의 결과가 곧 이 연결이 없애려던 버그(로컬 PTY 탭이 mirror 안에 생김)다. 실패 회신은 일반 forward 실패와 구분된 사유(`restore: nothing to restore in this workspace`)로 나가고 client 는 전용 안내 toast(`attach.toast.mirror_restore_empty`)를 띄운다.
  - **서버 복원 스택은 워크스페이스로 스코프된다**: `ClosedItemStore` 의 각 엔트리가 닫힐 당시의 출처 워크스페이스 id 를 함께 싣고(`CoreState::push_closed_item` 이 항목의 구조 id 로 판정 — push 는 항상 트리 재배치 **전**이라 그 시점 트리가 답을 갖는다), forward 된 복원은 **anchor 워크스페이스 출처 항목만** pop 한다(`RestoreScope::Workspace`). 서버 로컬 복원은 **기존 전역 LIFO 그대로**다(`RestoreScope::Local`) — forward 된 close 는 서버 자신의 트리에서 탭을 없애므로 서버 앞 사용자에게도 undo 가 남아야 한다. 워크스페이스 통째 항목은 출처가 `None` 이라 forward 스코프에 원리적으로 안 걸린다 — 그 갈래는 anchor 밖에 새 workspace 를 만들어 delta 에 안 잡히므로, pop 뒤 거부(항목 유실)가 아니라 pop 이전 배제로 닫는다.
  - **forward된 close는 사용자가 닫았을 때만 복원 스택에 남는다.** 사용자 GUI와
    에이전트 CLI/IPC 요청을 구분하기 위해 `StreamControl::StructuralOp`에
    `origin`(`"user"` 또는 `"agent"`)을 싣는다. 클라이언트는 forward 큐 항목의
    `user_triggered`로 이 값을 정해 항상 보낸다(위 "구조 변경 응답" 항목).

    서버의 `ForwardOrigin::of_wire`는 origin이 없으면 `user`로 읽어 옛 클라이언트의
    복원 동작을 유지한다. 모르는 값은 프레임 전체를 버리지 않고 `agent`로 읽는다.
    이 경우 복원 스택을 바꾸지 않는다. 옛 서버는 origin 필드 자체를 무시하므로
    새 클라이언트의 에이전트 close도 복원 스택에 남을 수 있다.

    `user`의 `CloseSurface`는 `structural_exec::close_surface`를
    `save_snapshot=true`로 호출한다. 일반 IPC 핸들러는 false를 넘기며, 이 인자는
    요청 params로 지정할 수 없다. `CloseTab`과 `ClosePane`의 도메인 함수에는 이
    인자가 없어 `execute_forwarded_structural_op`이 먼저 `capture_closed_tab` 또는
    `capture_closed_pane`으로 캡처하고 `push_closed_item`을 호출한다. pane은 트리를
    재배치하기 전에 캡처해야 분할 정보를 보존할 수 있다.

    `agent` close는 `save_snapshot=false`로 호출하고 별도 캡처도 하지 않는다.
    복원 스택은 에이전트가 바꾸지 않는 사용자 상태다([identity](../identity.md) 원칙 1,
    [ADR-0023](../adr/0023-attach-state-sync-and-forwarding.md)). `is_user_close`는
    origin과 별개이며 이 경로에서는 항상 false다.
  - **실행은 `Core::apply` 직접 호출이다**: 복원은 IPC/CLI 로 노출된 적이 없어 대응하는 도메인 실행 함수가 없다 — `ConvertSurface`/`MoveSurface` 와 같은 형태로 `execute_forwarded_structural_op` 이 `DomainIntent::RestoreClosedItem` 을 직접 부르고 `CoreEvent::ClosedItemRestored{restored}` 로 성공을 판정한다. 복원된 터미널은 이 함수의 기존 before/after diff 에 잡혀 `added_terminals` → 점유 편입 → tap 을 그대로 탄다(별도 연결 없음).
  - **실행측은 cascade 를 재현하지 않는다**: 로컬 경로가 부르는 `cascade_closed_item_restored` 는 (사용자 발화이면) `AppState::active_workspace`/`focused_pane` 을 바꾼다 — forward 경로에서 그것을 부르면 원격 사용자의 조작이 서버 앞 로컬 사용자의 화면을 움직인다(원칙 1·3 위반). forward 실행은 engine 변경만 하고 AppState 를 안 만지며, client focus 는 `PendingOpFocus::NewResource` 가 delta 적용 시점에 client-only 로 보정한다.
- **convert/move-surface**: 둘 다 forward 대상이다. convert(`ConvertSurface`)는 항상 forward. move-surface(`MoveSurface`)는 **source/target 이 같은 mirror workspace 안에 있을 때만** `build_mirror_forward_op` 가 forward 한다 — mirror↔local(또는 서로 다른 mirror) workspace 경계를 넘는 이동은 로컬 전용 surface_id 를 원격에 그대로 보내는 꼴이 되어(u32 네임스페이스 분리가 없어 원격 트리의 무관한 surface 와 우연히 겹칠 위험) 여전히 로컬 차단으로 남는다. `execute_forwarded_structural_op` 의 move-surface 실행은 target(B) 의 PTY cleanup 을 `core::structural_cascade::cascade_surface_closed` 로 명시 재현한다(대응하는 도메인 실행 함수가 없어 이 함수가 직접 호출해야 함 — CloseSurface 가 `structural_exec::close_surface` 안에서 부르는 것과 같은 cascade). convert 실행 성공(`replaced:true`) 시 `ForwardedDelta.converted_surface` 에 대상 surface_id 를 실어, 메인루프(`event_handler.rs`/`boot/headless_stream.rs`)가 `PluginManager::drop_egui_mesh_frame` 을 호출해 egui-mesh(image 등) stale frame 을 방지한다 — 로컬(비-forward) 변환 경로의 `SurfaceConverted` cascade(`app/dispatch_domain.rs`)와 동일 처리를 forward 경로에도 재현한 것. plugin manager 는 forward 실행이 받는 상태 어디에도 없고 두 빌드 모두 호출자가 소유하므로, 결과를 값으로 돌려주고 호출자가 반영한다(ADR-0002).

## 서버 로컬(비-holder) 구조 변경 차단

위 절이 다루는 forward 실행(`execute_forwarded_structural_op`)은 hard 점유 **holder** 가 mirror 안에서 만든 정당한 구조 변경이 서버에 도달해 실행되는 경로다. 이 절은 반대 경우 — **서버 자신의 workspace 가 hard-occupied 상태일 때, holder 가 아닌 서버 로컬 IPC/CLI/agent 가 직접** `split`/`tab.create`/`terminal.spawn`/`pane.close`/`tab.close`/`tab.move`/`surface.close`/`workspace.close`/`markdown.navigate`/`image.open` 를 호출하는 경로를 다룬다. 개념·사용자 영향은 [features/remote-attach "서버(피점유)측 비-holder 구조 변경 차단"](../features/remote-attach/index.md#서버피점유측-비-holder-구조-변경-차단), 여기엔 메커니즘만.

- **왜 `Core::apply` 나 핸들러 함수 내부가 아닌가**: `execute_forwarded_structural_op`(`src/core/attach_runtime.rs`)는 holder 의 forward 실행 시 split/tab.create/tab.close/pane.close/surface.close/tab.move 6종은 IPC 핸들러가 부르는 것과 같은 도메인 실행 함수(`core::structural_exec` 의 `split`/`create_tab`/`close_tab`/`close_pane`/`close_surface`/`move_tab`)를 **직접 함수 호출**하고(IPC 디스패치와 핸들러는 거치지 않는다 — ADR-0002), convert/move-surface 는 대응하는 도메인 실행 함수가 없어 `Core::apply(DomainIntent)` 를 직접 호출한다 — 어느 쪽이든 이 함수 안에서, 또는 그 안에서 공통으로 거치는 `Core::apply` 에 가드를 걸면 holder 본인의 forward 요청까지 함께 막혀 위 "mirror 구조 변경 forward" 기능 전체가 회귀한다. `terminal.spawn` 은 forward 대상 8종(위 6종 + convert/move-surface)에 애초에 포함되지 않으므로([ADR-0021](../adr/0021-occupancy-and-attach-admission.md)) 이 가드를 추가해도 같은 회귀 위험이 없다.
- **가드 위치**: `hard_occupied_structural_guard`(`src/adapters/ipc/handler.rs`) — `route_engine_handler` 의 method-string dispatch 최상단에서, `execute_forwarded_structural_op` 가 우회하는 그 dispatch 지점에서만 검사한다. params 에서 대상 pane_id/tab_id/surface_id(`split` 은 `target_pane`/`target_surface`, nickname 포함; `markdown.navigate`/`image.open` 은 `surface_id`)를 뽑아 소속 workspace 를 찾고, `OccupancyRegistry::workspace_holder`(`src/core/attach.rs`)가 `Some` 이면 `invalid_params` 로 거부한다("점유 중" + "다른 workspace 사용" 안내). `terminal.spawn` 만 이 가드에 없다 — 대상이 `workspace` 파라미터가 아니라 `pane` 오버라이드까지 반영해 확정된 **최종 pane** 이라, 그것을 아는 `spawn_target_guard`(같은 파일, `handle_spawn` 이 pane 확정 직후 호출)에서 건다. 거부 문구는 `hard_occupied_denial` 로 라우터 가드와 공유한다. 이 이동으로 `--workspace <비점유 ws>` + `--pane <hard-occupied ws 의 pane>` 조합으로 가드를 우회하던 구멍도 닫혔다([ADR-0021](../adr/0021-occupancy-and-attach-admission.md)). 대상을 resolve 할 수 없으면(params 누락 등) `None`(가드 미개입) — 원래 핸들러의 통상 검증 에러로 흘려보낸다. `markdown.navigate`/`image.open` 커버는 완전하지 않다 — convert 진입점은 kind 별로 흩어져 있고(host 범용 convert 팝업은 `state.dispatch_intent` 를 직접 호출해 이 IPC 라우팅 자체를 안 탐), 향후 새 kind 가 자기 전용 convert 진입 method 를 추가하면 이 목록에 없는 한 가드가 적용되지 않는다.
- **CLI 는 무수정**: `crates/tasty-ipc/src/client.rs` 의 `IpcConnection` 이 JSON-RPC `error.message` 를 그대로 노출하므로, `tasty new tab`/`tasty split`/`tasty close tab|pane|surface`/`tasty claude spawn`/`tasty codex spawn` 는 서버 에러 메시지를 그대로 보여준다.
- **테스트**: `src/core/attach_runtime.rs` `forward_exec_tests` 모듈 — `dispatch_denies_structural_create_when_hard_occupied`/`dispatch_denies_structural_close_move_when_hard_occupied`(비-holder 일반 IPC 호출 거부 + 트리 불변), `dispatch_denies_terminal_spawn_when_hard_occupied`(`terminal.spawn` 거부 + tab 미생성), `dispatch_denies_terminal_spawn_into_mirror_workspace`(mirror 거부 + forward 큐 미증가), `dispatch_still_forwards_tab_create_in_mirror_workspace`(mirror 의 다른 구조 변경은 여전히 forward — 설계 보존), `dispatch_denies_terminal_spawn_when_pane_override_targets_blocked_workspace`(`--pane` 교차 워크스페이스 우회 차단, mirror·hard-occupied 양쪽), `dispatch_denies_convert_entrypoints_when_hard_occupied`(`markdown.navigate`/`image.open` 거부), `dispatch_allows_tab_create_when_not_occupied`/`dispatch_allows_markdown_navigate_when_not_occupied`(비점유 workspace 오탐 방지), `forward_*_succeeds_when_hard_occupied` 계열(holder 의 forward 는 hard-occupied 여도 정상 성공 — 회귀 방지, convert/move-surface 포함). move-surface 의 same-mirror-workspace 검증은 `src/core/impl_mirror.rs` `mirror_structural_guard_tests` 모듈의 `mirror_move_surface_enqueues_forward_when_same_workspace`/`mirror_move_surface_blocked_when_crossing_workspace_boundary` 가 커버한다.
- **마지막 tab/pane도 같은 제한을 받는다.** 비-holder의 close는 dispatch에서 거절돼
  트리를 바꾸지 않는다. `dispatch_denies_structural_close_move_when_hard_occupied`는
  pane 하나·tab 하나인 fixture로 이 조건을 확인한다.
- **`terminal.spawn`도 제한한다.** 점유된 workspace에 자식을 만들면 호출자가 바로
  그 자식에게 입력을 보내지 못한다. 최종 pane을 결정한 뒤 `spawn_target_guard`로 거절해
  `--workspace`와 다른 workspace의 `--pane` 조합도 검사한다(ADR-0021).

- **mirror(원격 attach client) 워크스페이스로의 `terminal.spawn` 도 같은 자리에서 거부된다**: 이 절의 나머지는 **서버(피점유)측** 축이지만, `spawn_target_guard` 는 client 측 축인 mirror 도 함께 판정한다 — 같은 "최종 pane 이 어느 워크스페이스에 속하는가" 조회 하나로 둘 다 결정되기 때문이다. mirror 워크스페이스의 구조 변경은 원격으로 forward 되고 그 응답은 fire-and-forget 이라 생성된 surface id 를 담지 않는데, `handle_spawn` 은 그 id 를 동기로 꺼내 child registry 등록·soft 점유·command 주입까지 이어가야 한다. 막지 않으면 핸들러는 에러를 반환하지만 `pending_structural_forward` 는 IPC 응답과 무관하게 `about_to_wait` 에서 드레인되므로 **원격에만 탭이 남는 고아**가 생긴다. mirror 판정은 `terminal.spawn` 에만 적용한다 — 라우터 가드의 나머지 9종에 넣으면 mirror 구조 변경 forward 전체가 회귀한다. 근거 → [ADR-0021](../adr/0021-occupancy-and-attach-admission.md).
- **`pty.attach_surface` 는 여전히 이 가드 대상이 아니다(알려진 갭)**: `AdoptTerminal` intent 로 실행되는 이 경로도 아래와 동일한 `apply_adopt_terminal` → `tap_new_workspace_member` 후처리를 타 이론상 `terminal.spawn` 과 동일한 부작용(호출자 본인의 입력 차단)을 가질 수 있으나, 이번 변경의 확인된 필수 스코프는 `terminal.spawn` 로 한정했다 — 소비자(`tasty terminal` child-terminal, soft 점유 기반)에 대한 영향 검토가 아직 없다(재검토 조건은 ADR-0021 참고).
- **생성이 허용된 surface에도 점유와 스트림 tap을 등록한다.** hard-occupied
  workspace에 터미널 surface를 추가하면 기존 점유를 상속하고 attach 스트림에 연결해야
  한다. 이를 빠뜨리면 PTY와 화면 버퍼가 있어도 클라이언트에는 출력이 전달되지 않는다.
  대상은 `apply_create_tab`(`src/core/impl_tab.rs`), `apply_split_pane`과
  `apply_split_surface`(`src/core/impl_split.rs`), `apply_adopt_terminal`
  (`src/core/impl_attach.rs`)이다.

  네 경로의 공통 후처리인 `CoreState::tap_new_workspace_member`
  (`src/core/attach_runtime.rs`)는 멤버 등록(`OccupancyRegistry::add_workspace_member`),
  holder에게 새 트리 전송(`StructuralDelta`), 스트림 연결(`tap_surface_for_stream`)
  순서로 실행한다([ADR-0023의 서버 구조 변경 규칙](../adr/0023-attach-state-sync-and-forwarding.md#decision)).
  holder의 forward 요청도 같은 순서를 따른다. forward 실행 중의 중복 tap 방지는
  다음 항목을 참고한다.

  필요한 `StreamHub`는 부팅 시 등록한 `OccupancyRegistry::notifier()`에서 얻고,
  holder의 client ID는 `workspace_holder()`로 찾는다. `force_detach_workspace`의
  `notify_detached`도 같은 notifier를 쓴다. `Core::apply`의 호출 경로 전체에 hub와
  client ID를 인자로 추가할 필요는 없다.

  이 후처리는 생성 차단을 통과한 요청에만 적용된다. 비-holder의 `terminal.spawn`은
  앞의 가드가 거절하므로 여기까지 오지 않는다. 점유 등록은 `impl_mirror.rs`의
  `mirror_structural_guard_tests::*_in_occupied_workspace_inherits_occupancy` 4개
  단위 시험이 확인한다. `tests/attach_local_creation_tap.rs`는 loopback 통합 시험으로
  `pty.spawn` + `pty.attach_surface` 뒤 실제 mux `Data` 프레임 수신까지 확인한다.
- **forward-op 경로는 이 즉시-tap 을 스킵한다(이중 tap 방지)**: 위 4개 생성 경로는 `execute_forwarded_structural_op` 가 재사용하는 `handle_split`/`handle_tab_create` 에서도 그대로 호출된다 — 즉 holder 자신의 forward 실행 중에도 (가드를 통과한 뒤) `tap_new_workspace_member` 가 타지만, 그 경로는 위 §"역반영" 이 이미 delta 전송 **후** `tap_surface_for_stream` 을 직접 걸므로 여기서 또 tap 하면 같은 surface 에 tap 이 2개 등록돼 매 PTY 청크(타이핑 echo 포함)가 client 에 2번 도착한다("tttttest" 증상). `OccupancyRegistry::set_auto_tap_suppressed(true/false)` 로 `execute_forwarded_structural_op` 가 `pane::handle_split`/`tab::handle_tab_create` 호출 구간만 감싸 이 즉시-tap 을 억제하고(멤버 편입은 그대로 진행), 로컬 생성 경로(플래그 항상 `false` — `terminal.spawn`/`pty.attach_surface` 포함, 가드를 통과했든 안 했든 무관)는 영향받지 않는다. 테스트: `forward_exec_tests::forward_split_surface_taps_exactly_once_with_real_stream_hub`(실제 `StreamHub` 주입, tap 개수 검증 — `Terminal::output_tap_count`).

## SSH 터널 (원격 client 공통)

`crates/tasty-ssh/src/lib.rs` — `remote attach` 와 `remote check` 가 공유.

- **시스템 ssh 위임**: 자체 암호화 없이 시스템 `ssh` 를 자식 프로세스로 실행. 사용자 `~/.ssh/config`·agent·known_hosts 재사용. **Windows 는 시스템 OpenSSH 풀경로**(`%WINDIR%\System32\OpenSSH\ssh.exe`) 우선 — git 번들 ssh 는 윈도우 ssh-agent(named pipe)를 못 봐 무암호 인증 실패.
- **원격 포트 발견**: 기본 `auto` = subcommand → file-unix → file-windows 순서로 원격 DefaultShell 4종(PowerShell/cmd/git bash/unix) 커버. `--remote-port-mode` 로 고정, `--remote-tasty <path>` 로 원격 바이너리 경로(기본 `tasty`).
- **포트 발견 상한(no-hang)**: 포트 발견/셸 감지는 무기한 블록하지 않는다 — ssh `-o ConnectTimeout`(`SSH_CONNECT_TIMEOUT` 10초, 연결 수립) + ssh 자식 1개당 프로세스 레벨 감시(`PORT_DISCOVERY_STEP_TIMEOUT` 20초, kill + wait 로 좀비 없이 회수) + 호출 1회 전체 예산(`PORT_DISCOVERY_TOTAL_TIMEOUT` 45초, 체인 단계 수만큼 곱해지는 것을 차단) 3겹. 상수는 `crates/tasty-ssh/src/lib.rs` 에 `pub const`. 프로필 `extra_options` 의 `ConnectTimeout` 이 기본값을 이긴다(ssh(1) first-wins → 기본값을 뒤에 push). 근거 → [ADR-0020](../adr/0020-remote-connection-profiles.md), 상세 → [features/remote-attach "연결 시도 상한"](../features/remote-attach/index.md#연결-시도-상한-no-hang).
- **포트 발견용 ssh 자식의 취소**: `run_capture_with_budget`은 `spawn` + `try_wait`로
  자식 핸들을 관리하며, 시간 제한 전에도 사용자가 취소할 수 있다. 조회 워커가
  스레드 로컬 `ssh::SshCancel`의 `SshCancel::scope()`를 설치하면 다른 스레드에서
  `cancel()`을 호출해 해당 자식의 kill과 wait를 시도한다. 이 정리는 `SshTunnel::drop`과
  타임아웃 경로와 같다. 취소 후에는 auto 체인의 나머지 단계도 ssh를 실행하지 않는다.
  `PortDiscoveryFailureKind::Cancelled`는 예산 소진과 마찬가지로 fallback 대상이 아니다.

  현재 취소 스코프를 설치하는 곳은 GUI picker 조회 워커뿐이다.
  `remote.workspaces`/`remote.attach` IPC와 `auto_attach` 워커에는 취소 신호가 연결돼 있지 않다.
  요청자가 연결을 끊어도 이 취소 경로로 전달되지 않는다.
  도구 메뉴 > Remote connections의 셸 감지 워커
  (`remote_tool.rs`/`remote_profile.rs`의 `spawn_detect`)에도 취소 스코프가 없다.
- **포트 발견 실패 진단**: 전 단계 실패 시 `PortDiscoveryError`(`crates/tasty-ssh/src/lib.rs`)가 exit code 기반(로케일 무관)으로 SSH 연결 실패 / 원격 인스턴스 미실행 / 포트 파싱 실패 + 위 상한 초과 시 타임아웃 + 사용자 취소, 5분류한다 — 원격 raw stderr 는 `Display` 에 노출하지 않고 `tracing::debug!` 로만 남긴다(타임아웃·취소 경로도 동일). auto 체인이 전 단계 실패하면 가장 확정적인 분류를 대표로 고른다(`pick_most_informative` — 취소 > 타임아웃 순 우선. 취소는 사용자가 끊었다는 확정 사실이라 시간·연결 실패로 보고하면 오도한다). 상세 → [features/remote-attach "원격 포트 발견 실패 진단"](../features/remote-attach/index.md#원격-포트-발견-실패-진단).
- **터널 생명주기**: detach/종료 시 자식 ssh kill(고아 터널 방지)하되 **원격 데몬은 생존**(server-owns-PTY persistence = detach 의 본질). 자동 재연결(attach 한정): 지수 백오프(0.5s→30s)로 터널+attach 재수립(`--no-reconnect` 로 끔) — 이 재연결은 `run_attach_on_port` 가 반환하는 `AttachExit::Disconnected` 를 전제로 한다. mirror-dump/workspace-mirror-dump 모드는 처음부터 reader 를 별도 스레드 + `mpsc::channel` 로 분리해 `rx.recv_timeout` 의 `RecvTimeoutError::Disconnected`(끊김) vs `Timeout`(정상 deadline) 을 구분해 이를 반환해왔다. `--raw` 모드(`run_raw_bridge`)도 동일 계약을 만족한다 — server reader 스레드가 `mpsc` 채널에 보내고(`RawEvent::Server`/`ServerRecvErr`), main 은 `rx.recv()`(deadline 없이 블로킹)만 기다린다. server reader 의 `conn.recv()` 가 `Err` 면 `RawEvent::ServerRecvErr` 를 명시적으로 보내 `AttachExit::Disconnected` 로 이어진다.
- **stdin은 스레드 하나가 읽는다.** 프로세스 시작 때
  [`spawn_stdin_reader`](../../crates/tasty-cli/src/local/attach.rs)를 한 번 호출한다.
  읽은 청크는 `StdinSlot`(`Arc<Mutex<Option<mpsc::Sender<RawEvent>>>>`)의 현재 sender로
  보낸다. 재연결 때 `install_sender`로 sender만 교체해 두 리더가 같은 stdin을 읽는
  경합을 피한다.

- **남는 유실 창(정확한 범위)**: 세션 전환의 아주 짧은 순간 — 이전 세션이 끝나 슬롯이 아직 `None` 이거나 이미 rx 가 drop 된 죽은 sender 를 가리키는 동안 사용자가 타이핑하면, 리더 스레드는 그 청크의 송신에 실패한다. 이때 리더 스레드는 (좀비가 되지 않도록) **종료하지 않고 그 청크만 버린 뒤 계속 읽는다** — 슬롯 자체는 절대 건드리지 않는다(ABA 경쟁 방지 불변식: 슬롯에 대한 쓰기는 `install_sender` 만 수행). 이 창은 "재연결이라는 명확한 이벤트 근방"으로 국한되며, 좀비 누적처럼 시간이 지날수록 커지지 않는다. 진짜 stdin EOF/에러가 이 창(슬롯이 비어있는 동안)에 겹치면 별도 `AtomicBool` latch 에 기억해뒀다가, 다음 세션이 `install_sender` 로 sender 를 설치하는 시점에 즉시 `RawEvent::StdinEof` 를 전달한다(그러지 않으면 EOF 통지가 영영 유실돼 다음 세션이 이미 닫힌 stdin 을 무한정 기다리게 될 수 있다). 이 창을 버퍼링으로 완전히 닫는 것은 이번 스코프 밖이다(선택적 후속 확장).
- **loopback 직결**: 인라인 host 가 `127.0.0.1:PORT`/`localhost:PORT` 면 SSH 없이 직접 attach(동일 머신 다중 인스턴스 검증).

## 연결 생존 확인 (read timeout + heartbeat)

attach 스트림은 read timeout 이 없는 순수 blocking I/O 라 네트워크가 FIN/RST 없이 **조용히** 끊기면(케이블 단절·NAT 타임아웃 등) 소켓의 read 가 영원히 대기해 어느 쪽도 끊김을 감지하지 못한다. 아래는 이를 감지하는 연결이다 — 상수는 `crates/tasty-ipc/src/stream.rs` 의 `HEARTBEAT_INTERVAL`(5초)/`HEARTBEAT_TIMEOUT`(20초, interval 의 4배 — 일시적 jitter 로 인한 오탐 방지).

- **read timeout**: 서버(`tcp_ipc_server.rs::handle_stream_connection`)·GUI client(`attach_client.rs::start_gui_attach`)·CLI client(`tasty-ipc/src/client/stream.rs::StreamConnection::open_with`) 3곳 모두 소켓에 `HEARTBEAT_TIMEOUT` read timeout 을 건다. **서버는 그것을 attach dispatch 보다 먼저 건다**(`arm_stream_read_timeout` 이 `validate_stream_proto` 앞이다) — 즉 판정은 **연결 단위**이고 점유 유무와 무관하다: 아무 workspace 도 점유하지 않은 단순 upgrade 연결도 20초 침묵하면 닫힌다. 그래서 client 는 점유를 잡았든 안 잡았든 Ping 을 보내야 한다. `try_clone` 된 reader/writer 는 같은 소켓 옵션을 공유하므로 한 번만 걸면 양쪽 다 적용된다. CLI 는 핸드셰이크 ack/`attached(_workspace)` 디스크립터 대기(둘 다 `recv()`)에도 자동으로 적용된다.
- **`StreamTag::Ping`**: 빈 payload 의 keepalive 프레임(값 `2`, 코덱은 이전부터 예약돼 있었다). 수신측은 아무 처리도 하지 않는다 — `read_frame` 호출이 성공적으로 리턴하는 것 자체가 read timeout 을 리셋하므로, Ping 이든 실제 Data/Control 이든 도착만 하면 liveness 로 인정된다.
- **송신은 write 쪽이 idle 할 때만**: 서버의 write thread(`handle_stream_connection`)는 기존 `for frame in sink_rx` blocking iterator 대신 `sink_rx.recv_timeout(HEARTBEAT_INTERVAL)` 루프를 쓴다 — sink 에 실제 Data/Control 프레임이 흐르면 그게 곧 liveness 라 Ping 을 보내지 않고, `HEARTBEAT_INTERVAL` 동안 조용하면 빈 Ping 을 대신 흘린다. GUI/CLI(raw 브리지) client 는 각각 별도 heartbeat 스레드가 `HEARTBEAT_INTERVAL` 마다 무조건 Ping 을 보낸다(활성 트래픽 여부를 별도로 추적하지 않음 — 5바이트 프레임 오버헤드가 그 추적 비용보다 훨씬 싸다).
- **CLI dump 모드(mirror-dump/workspace-mirror-dump)도 client 발 heartbeat 을 보낸다 — 스레드 없이 수집 루프 안에서**: 수집 대기를 `min(창 끝, 다음 Ping)` 으로 끊고 `HEARTBEAT_INTERVAL` 마다 `Ping` 을 쓴다(`DumpHeartbeat`). 그래서 `--dump-after` 가 `HEARTBEAT_TIMEOUT` 보다 길어도 서버가 창 도중에 끊지 않는다. 첫 Ping 은 창을 연 지 한 주기 뒤라 기본 500 ms 창의 송신 프레임은 heartbeat 이 없던 때와 같다. `Ping` 쓰기가 실패하면 `Disconnected` 로 끝난다. writer 를 루프 하나가 쥐므로 raw 브리지(stdin 라우팅과 writer 공유 → 별도 heartbeat 스레드 + `Mutex`)와 형태가 다르다. 근거: [ADR-0023](../adr/0023-attach-state-sync-and-forwarding.md).
- **양방향이 필요한 이유**: 서버 write thread 의 Ping 은 client 의 read timeout 을(idle mirror 뷰), client 의 Ping 은 서버의 read timeout 을(client 가 오래 아무 입력도 안 보내는 세션) 각각 갱신한다 — 한쪽만 보내면 반대 방향의 read loop 가 오탐 disconnect 된다.
- **timeout 만료 → 기존 disconnect 경로 재사용**: read timeout 으로 인한 `WouldBlock`/`TimedOut` io 에러는 서버의 `Err(_) => break`(`handle_stream_connection`)·GUI client 의 `Err(_) => disconnected.store(true, ...)`·CLI 의 채널 기반 끊김 신호(`run_mirror_dump`/`run_workspace_mirror_dump` 의 `mpsc::RecvTimeoutError::Disconnected`, `run_raw_bridge` 의 `RawEvent::ServerRecvErr` — 위 "SSH 터널" 절 참고)를 그대로 타 EOF 와 동일하게 처리된다 — 별도 sweep 스레드나 새 상태 없이, 아래 "mirror 세션 종료"·[`features/remote-attach`](../features/remote-attach/index.md) 의 "자동 해제" 가 조용한 네트워크 단절까지 커버하게 된다.
- **GUI heartbeat 스레드 정리**: `cleanup_mirror_workspace` 가 세션 종료 시(원격발이든 사용자 close 든) `sess.disconnected` 를 set 해, `writer: Arc<Mutex<TcpStream>>` 를 계속 붙들고 있는 heartbeat 스레드가 다음 tick 에 스스로 종료하게 한다 — 안 하면 세션이 정리된 뒤에도 소켓/스레드가 새는 leak.
- **버전 skew 리스크(heartbeat 축은 완화 로직 없음, 의도적)**: read timeout(20초)이 걸린 이후부터 이 프로토콜을 도입한 버전이 적용된다. 구버전 프로세스(Ping 미전송)와 신버전 프로세스가 같이 떠 있는 상태(예: 호스트 재시작 없이 CLI/플러그인만 재배포)에서는 idle 상태의 mirror 세션이 20초 뒤 오탐 disconnect 될 수 있다 — 이 프로젝트는 단일 사용자 로컬 앱이라 그런 skew 창이 짧고 드물어 허용 가능하다고 판단했다. 다중 버전 동시운영이 필요해지면 capability 협상을 재검토한다. **단 스트림 프로토콜 버전(`proto`) 자체는 핸드셰이크에서 검증된다** — 위 "점유 레지스트리" 의 `validate_stream_proto` 항목 참고. 지금은 정확히 같은 값만 통과하는 엄격 매칭이라, `STREAM_PROTO` 를 올리면 구버전 client 는 조용한 실패가 아니라 명시적 거절을 받는다.

<a id="클라이언트측-비대칭서버는-점유-유지-클라이언트는-사라짐-원인"></a>

## SSH keepalive와 attach heartbeat의 차이

SSH 터널은 `ServerAliveInterval=15`와 `ServerAliveCountMax=3`을 사용한다.
이는 로컬 ssh 자식이 원격 sshd의 응답을 확인하는 장치다. ssh가 종료하면 client의
loopback 연결도 EOF를 받지만, 원격 Tasty의 소켓 생존을 직접 검사하는 것은 아니다.

attach의 `HEARTBEAT_TIMEOUT`은 별개다. SSH가 살아 있어도 Tasty의 데이터가
20초 동안 도착하지 않으면 끊김을 처리한다. SSH 없이 loopback에 직접 연결한 경우에도
적용된다. 두 제한을 더하거나 어느 쪽이 항상 빠르다고 가정하지 않는다.

2026-07-20의 격리 loopback 측정에서는 서버에 SIGSTOP을 보낸 뒤 client가 약 21초에
종료했고, 정상 서버의 idle raw 세션은 25초 넘게 유지됐다. 이는 attach heartbeat의
기록이며 실제 SSH 인증·keepalive를 측정한 결과는 아니다.

## GUI 자동 재연결 스코프

silent disconnect 정리(`cleanup_mirror_workspace`) 자체는 heartbeat TTL 만료로도 자동 발동한다
(위 "연결 생존 확인"). anchor(매핑된 로컬 워크스페이스)에 물린 세션이 이렇게 끊기면
mirror workspace 는 즉시 걷히지 않는다 — 세션이 `SessionState::Reconnecting` 으로 전이해
재연결 대기 상태를 유지하고, 아래 backoff 스케줄이 자동으로 재연결을 시도한다.
anchor 가 없는 mirror(IPC `remote.attach` 로 연 임시 mirror 등)는 대상이 아니라 기존처럼
즉시 정리된다.

- **레벨/엣지 트리거(기존, 신규 attach 전담)** — `src/app/auto_attach.rs::maybe_trigger_auto_attach`
  는 "활성 워크스페이스가 매핑 Some & `auto_attach_active` 에 없으면 트리거"를 **매 프레임**
  재평가한다. 예를 들어 이미 활성인 워크스페이스에 `attach_mapping` 을 방금 새로 설정하면
  (`tasty set workspace --ssh-profile ...`) 워크스페이스 전환 없이도 다음 프레임에 즉시
  트리거된다. "재진입 대기(pending reactivation)" anchor(과거엔 disconnect 로 정리된
  모든 anchor 가 여기 들어갔지만, 지금은 이 표시가 남아있는 것 자체는 드물다 — 아래
  backoff 스케줄이 대부분 먼저 처리한다)만 엣지(전환) 게이팅: 활성 워크스페이스 id 가
  **직전 프레임과 달라진 프레임에서만**(`App.auto_attach_last_active_ws` 로 직전 값을 들고
  비교, 술어는 `is_reactivation_edge`) 트리거를 허용한다. 최종 판정은
  `is_attach_trigger_allowed(pending_reactivation, current, previous)`(단위 테스트
  `new_mapping_triggers_immediately_without_transition`/
  `disconnected_anchor_waits_for_transition_before_retrigger`).
  - **backoff 재연결과의 레이스 회피**: anchor 에 이미 `Reconnecting` 세션이 있으면 이
    트리거는 즉시 스킵한다(`maybe_trigger_reconnect` 전담 대상). 이 가드가 없으면 두
    트리거가 같은 anchor 에 대해 동시에 `start_gui_attach`/`reconnect_session` 을 각각
    spawn 해 mirror workspace 가 중복 생성될 수 있다.
- **backoff 재연결 트리거(`maybe_trigger_reconnect`)** — `Reconnecting` 상태인
  세션이 있는 각 anchor 마다 매 프레임 확인한다: ① 사용자가 **지금** 그 anchor
  워크스페이스로 전환해 돌아왔으면(엣지, `current_ws_id == anchor` 로 한정 — 다른
  워크스페이스로의 무관한 전환까지 모든 Reconnecting anchor 를 깨우지 않는다) 즉시, ②
  아니어도 `App.auto_attach_reconnect: HashMap<u32, ReconnectSlot>` 에 저장된 `next_attempt`
  시각이 지났으면(Reconnecting 진입 직후 첫 시도는 슬롯이 없어 즉시). 두 조건 모두
  `auto_attach_active` 게이트를 공유해 중복 attach 를 막는다. anchor 워크스페이스 자체가
  삭제됐거나 `attach_mapping` 이 그 사이 사라지면 스케줄을 정리하고 skip 한다.
  - **GUI는 기다리지 않고 예약한다.** CLI의 `Backoff::sleep()` 대신 `current()`와
    `advance()`로 다음 시각을 계산하고 프레임마다 확인한다.

  - **성공/실패 처리(`drain_auto_attach_results`)**: `is_reconnect` 플래그로 신규
    attach(`start_gui_attach`)와 재연결(`reconnect_session`, survivor mapping — 아래
    "재연결 시 세션 상태 보존" 참고)을 분기한다. 재연결 성공 시 `auto_attach_reconnect`
    슬롯을 제거(다음 disconnect 때 새로 시작)한다. 실패는 `on_reconnect_attempt_failed`
    로 처리: 에러 메시지에 `already_attached` 가 포함되면(다른 클라이언트가 그 원격
    워크스페이스를 여전히 점유 — 영구적 충돌일 수 있음) 지수 증가 없이 max(30초) 간격에
    고정해 폭주 없이 계속 대기하고, 그 외(네트워크/SSH 등 일시적 실패)는 통상적인 지수
    백오프(0.5s→30s)로 늘린다. 간격에는 ±20% jitter 를 둬 여러 anchor 가 동시에 끊겼을
    때 재시도가 한 tick 에 몰리는 것을 막는다. 시도 횟수가 `MAX_RECONNECT_ATTEMPTS`(20)에
    도달하면 `ReconnectSlot.given_up` 을 세우고 안내 toast(`mirror_reconnect_giveup`)를
    1회 띄운다 — `auto_attach_pending_reactivation` 은 건드리지 않으므로 사용자가 그
    워크스페이스를 왕복하는 수동 재시도(엣지 트리거)는 계속 유효하다.
    - **give-up은 슬롯에 남긴다.** 슬롯을 지우면 `reconnect_due(None, _)`가 첫 시도로
      해석해 다시 시작한다. `given_up`이면 시각과 관계없이 자동 재시도를 막고, 사용자가
      해당 workspace로 돌아오는 `should_reset_given_up` 조건에서만 재개한다.
      실패 횟수는 이번 실패를 반영한 값이며 `attempts >= MAX_RECONNECT_ATTEMPTS`로 비교한다.

- **`detach_orphaned_mirror_sessions` 와의 관계**: 간섭 없음. `apply_attach_client_output`
  (disconnect 감지, anchor 있으면 `Reconnecting` 전이 / anchor 없으면
  `cleanup_mirror_workspace(sess, from_disconnect=true)`)과
  `detach_orphaned_mirror_sessions`(사용자가 mirror ws 자체를 닫은 고아 세션 정리,
  `from_disconnect=false`)는 서로 다른 세션 식별 기준(전자는 `disconnected` 플래그, 후자는
  `local_workspace` 를 들고 있는 engine 이 창 있는 쪽·parked 쪽 어디에도 없음)으로 동작하고, 둘 다 단일 스레드 메인루프에서
  순차 실행돼(레이스 없음) 겹치는 세션이 있어도 먼저 처리한 쪽이 세션을 vec 에서 제거해
  뒤쪽은 자연히 skip 한다. `from_disconnect` 플래그가 두 경로를 구분해 사용자가 mirror ws 를
  직접 닫은 경우는 `auto_attach_pending_reactivation`/`auto_attach_reconnect` 에 들어가지
  않는다(재진입 대기·재연결 스케줄 의미가 없음 — 사용자 스스로 걷어낸 것).

**서버측 점유 해제([ADR-0021](../adr/0021-occupancy-and-attach-admission.md))와는 독립이다**: 위 backoff 재연결은 순수 **client(GUI mirror)측** 복원력이고, 서버의 `OccupancyRegistry` 해제 타이밍(EOF-or-TTL, 위 "점유 레지스트리" 절)은 전혀 건드리지 않는다 — TTL 만료로 서버가 이미 lock 을 free 로 되돌린 뒤에도 client 는 그저 다시 처음부터 attach 를 재시도할 뿐이고, 재연결 도중 다른 client 가 그 lock 을 먼저 잡으면 `already_attached` 로 거부돼 위 영구 충돌 backoff 로 흡수된다. 서버가 원 client 를 위해 lock 을 더 오래 붙들어주는 "재연결 유예 창구"는 도입하지 않았으므로 서버가 점유를 유지하는 유예 기능과 구분해야 한다.

## 재연결 시 세션 상태 보존

backoff 재연결이 매번 `start_gui_attach` 로 완전 신규 mirror workspace/터미널을
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
  것일 수 있으므로) 스레드를 종료하지 않고 그 청크만 drop 하고 계속 기다린다.
- **`SessionState`(`Connected`/`Reconnecting`)**: 기존 `cleanup_mirror_workspace` 는
  disconnect 즉시 mirror workspace/터미널을 통째로 걷어내, 재연결 트리거가
  발동할 시점엔 survivor-mapping 을 적용할 대상 자체가 남아있지 않았다. 이를 막기 위해 anchor 가 있는 세션의 disconnect 는 `cleanup_mirror_workspace`
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
  - **anchor 있는 세션(매핑된 워크스페이스)**: `cleanup_mirror_workspace` 를 부르지 않고 `enter_reconnecting` 으로 전이(위 "재연결 시 세션 상태 보존" 참고) — mirror workspace/터미널을 살려둔 채 `Reconnecting` 상태로 두고 backoff 재연결에 맡긴다.
  - **anchor 없는 세션(임시 mirror, IPC `remote.attach` 등)**: 기존과 동일하게 세션 제거 + `cleanup_mirror_workspace(sess, from_disconnect=true)`. 그 mirror 가 창의 유일한 워크스페이스였으면 정리 뒤 기본 터미널 워크스페이스를 다시 만든다(`AppState::recreate_workspace_if_empty`) — 창을 닫지도, 워크스페이스 0 개로 남기지도 않는다([ADR-0023](../adr/0023-attach-state-sync-and-forwarding.md)).
  - 이미 `Reconnecting` 상태인 세션(재연결 시도 자체가 실패해 다시 disconnected 로 관측되는 경우)은 이 분기에 다시 들어오지 않는다 — `disconnected && state == Connected` 조건이라 진입 시 이미 걸러진다.
- **로컬발 종료(사용자 close)**: `close_workspace_at`으로 mirror 워크스페이스를 닫으면
  로컬 워크스페이스는 즉시 사라지지만, 세션과 소켓은 남아 원격 점유가 바로 해제되지는 않는다.
  `Connected`와 `Reconnecting` 모두 같은 정리가 필요하다.

  `App::detach_orphaned_mirror_sessions`는 `about_to_wait`에서 세션의 `local_workspace`를
  가진 engine이 있는지 `mirror_workspace_engine_alive`로 확인한다.
  창에 속한 engine과 parked engine을 모두 찾고, 없으면 `cleanup_mirror_workspace`로 정리한다.
  창이 없어도 parked engine에 mirror 워크스페이스가 남아 있다면 세션을 유지한다
  ([창 없는 상태의 세션 수명](../features/remote-attach/index.md#창-없는-상태parked에서의-세션-수명)).
  attach 설정 중에는 같은 동기 함수에서 워크스페이스를 만든 뒤 세션을 추가하므로,
  아직 워크스페이스를 만들지 않은 세션이 이 검사에 걸리지는 않는다.
- **`cleanup_mirror_workspace`(공용, `from_disconnect: bool` 파라미터로 위 두 트리거 구분)**: mirror ws·터미널·mirror busy 엔트리·mesh 프레임 캐시 제거(창 있는 engine → parked engine 순으로 찾는다. 고아 판정과 순회 범위가 같아야 잔류가 없다. 이미 없으면 skip) → 원격에 `Detach` push → anchor 게이트(`auto_attach_active`) 해제 → 터널 kill. `from_disconnect=true`(원격발, anchor 없는 세션 한정)일 때만 anchor 를 `auto_attach_pending_reactivation` 에 추가(위 "GUI 자동 재연결 스코프" 참고) — `false`(로컬발/사용자 close)는 그 항목과 `auto_attach_reconnect` 스케줄 모두 명시적으로 제거해 이미 걷어낸 세션에 대한 재연결 시도를 남기지 않는다. 원격은 `Detach` 수신 시 read loop break → `Disconnected` → `release_all_for_client`(workspace+surface lock 해제).
- **적용 순회도 같은 범위다**: `apply_attach_client_output` 은 mirror 이벤트의 적용 대상을 창 있는 engine → parked engine 순으로 찾고(`mirror_output_host`), 찾은 뒤에만 reader 버퍼를 drain 한다 — 창이 없는 parked 구간에 도착한 출력·구조 delta 도 그 engine 에 즉시 적용된다(ADR-0023). 고아 판정·정리·적용 세 순회의 범위가 같아야 "살아 있다고 판정된 engine 에 적용이 닿지 않는" 유실 구간이 생기지 않는다.

## force-detach

서버 권한으로 점유 강제 해제(attach 하지 않음). holder client 에 `Control{force_detached}` + `Detach` push → client 가 mirror 정리·종료, 서버는 lock free.

- IPC: `attach.force_detach{surface_id}` / `attach.force_detach_workspace{workspace_id}`.
- CLI: `tasty remote attach --force-detach`(surface) / `--workspace <id> --force-detach`(workspace).
- **`--ssh` + `--force-detach` 미지원**(에러): force-detach 는 *이 서버* 에 붙은 client 락을 끊는 로컬 JSON-RPC 이지 터널 너머 원격 서버의 락을 끊는 게 아니다.

## 원격 생존 확인 (`remote check`)

`crates/tasty-cli/src/local/remote_check.rs`. 포트 발견만으론 **stale 포트 파일**(죽은 인스턴스 잔재)을 오판하므로 3단계: ① 포트 발견 → ② `ssh -L` 터널 → ③ 터널 localport 로 `system.info` **1회**. 응답이 와야 alive(stdout `alive: …`, exit 0). 거부/EOF/타임아웃 = dead(stderr, exit≠0). 단계별 상한은 ① 포트 발견 45초(위 "SSH 터널" 절의 3겹 상한) → ② 터널 ready-probe 5초 → ③ IPC 프로브 5초(`remote_check.rs` 의 `PROBE_TIMEOUT`) — 어느 단계도 무한 대기하지 않으므로 "단발 판정" 계약이 전 구간에서 성립한다. 1회성이라 재연결 백오프 없음. SSH 부품은 attach 와 공유, list 핸들러는 안 건드림.

## IPC 표면 (`attach.*`)

`src/adapters/ipc/handler/attach.rs`.

| 메서드 | 경로 | 설명 |
|--------|------|------|
| `attach.acquire` / `attach.release` | `stream.open{target}` 핸드셰이크 | 배타 lock 획득/해제 |
| `attach.force_detach` / `attach.force_detach_workspace` | JSON-RPC | 점유 강제 해제 |
| `attach.into_gui` | JSON-RPC | 실행 GUI 가 원격 워크스페이스를 mirror 로 재구성(`App::start_gui_attach`) |
| `attach.list` | JSON-RPC | 현재 점유 목록 조회(read) |

**권한**: attach 보안은 **연결 경계(SSH + 127.0.0.1 loopback)** 에 위임. 자체 권한 레이어 없음 — 소켓에 도달한 caller 는 attach 제어를 호출할 수 있다. 근거는 [identity §2](../identity.md), 주체 계약은 [actors](../concepts/actors.md).

## 커스텀 이벤트 확장 (`StreamControl` 밖 raw JSON `event` 태그)

`StreamControl` enum 을 건드리지 않고 같은 `StreamTag::Control` 채널에 raw JSON `{"event": "...",
...}` 를 얹어 요청/응답 쌍을 왕복시키는 패턴 — 프로토콜 자체를 확장하지 않고 attach 채널을 다른
용도(원격 디렉토리 나열, 스크린샷 업로드, git 조회 등)로 재사용할 때 쓴다. 인가는 전부 "attach 점유
= 신뢰"(`engine.attach.client_holds_workspace(client_id)`, ADR-0021) — 별도 권한 게이트가 없다.
수신측(`StreamHub::pump_inbound`)은 알려진 `StreamControl` 파싱이 실패하면 이 raw 이벤트들을
순서대로 시도하고, 전부 실패하면 조용히 무시한다(전방 호환 — 모르는 이벤트를 에러로 취급하지
않음).

- **`list_dir_request`/`list_dir_result`**(ADR-0022) — 원격 디렉토리 나열(file picker). host popup
  wrapper 가 요청을 소유하고 응답을 직접 소비 — 소유자가 host 프로세스 내부라 `sent_at.elapsed()`
  로 매 프레임 soft timeout(8초)을 직접 판정할 수 있다.
- **`git_query_request`/`git_query_result`**(ADR-0022) — 원격 저장소 status/log/diff/worktrees 조회
  (git-viewer). 응답의 **소비자가 host 가 아니라 별도 프로세스인 plugin** 이라는 점이 file picker와
  다르다 — host(`attach_client.rs`)는 payload 를 해석하지 않고 그대로 `PluginManager::
  emit_host_event_to_plugin`(Event Bus 의 owner-unicast 경로, 구독 등록 여부 무관 —
  `command.invoked` 와 동일 메커니즘)으로 `git_viewer.query_result` 이벤트를 plugin 에 전달만
  한다. 요청 트리거도 plugin→host `git_viewer.query` IPC(비동기 accept — `request_id` 만 즉시
  회신하고 실제 forward 는 `CoreState.pending_git_query_forward` 를 다음 tick 에 drain)로 plugin
  이 직접 건다. **soft timeout 없음** — 소유자가 host 프로세스 밖(plugin)이라 파일 피커와 같은
  매 프레임 판정 루프를 둘 자리가 마땅치 않아 이번 스코프에서 구현하지 않았다. 세션 자체가
  끊기는 경우는 보내기 전(send-time 실패)이든 보낸 뒤(`cleanup_mirror_workspace` 가
  `request_id=0` sentinel 로 강제 abandon)든 즉시 `ok:false` 로 커버된다 — 빠진 건 "연결은 계속
  살아있는데 원격 tasty 프로세스 자체가 응답만 안 주는" 좁은 경우다. ADR-0022의
  원격 조회 한계를 참고한다. egui-mesh popup 은 `popup.set_context`
  가 dirty 상태 변경시에만 나가 비동기 push 만으로는 다음 프레임 렌더가 보장되지 않으므로,
  `AppState.plugin_mesh_popup_pending_repaint: HashSet<u64>` 로 강제 repaint 를 예약한다
  (`popup_render.rs` 의 dirty OR-체인에 합류).
- **조회할 cwd 선택**: 두 API는 경로를 얻는 방식이 다르다.
  `list_dir_request`는 클라이언트가 보낸 `dir` 문자열을 그대로 사용한다.
  `git_query_request`는 `worktree_path`가 없으면 서버가 `surface_id`로 해당 PTY를 찾아
  `Terminal::get_cwd()`를 호출한다. OSC-7 캐시를 먼저 보고 `/proc`·`proc_pidinfo`로
  보완하므로, 원격 셸이 OSC 7을 보내지 않아도 OS에서 cwd를 조회할 수 있다.
  클라이언트가 재생한 OSC-7 정보에 의존하지 않는다.

## 관련

- 결정 근거(원격 대상·로컬 debug 격리): [ADR-0020](../adr/0020-remote-connection-profiles.md)
- 동작·점유 규칙·CLI/IPC 사용법: [`features/remote-attach`](../features/remote-attach/index.md)
- 주체(원격 사용자)·점유 모델 개념: [`concepts/actors`](../concepts/actors.md)
- 원격 디렉토리 나열: [ADR-0022](../adr/0022-remote-mirror-content-and-queries.md)
- 원격 git 조회(git-viewer): [ADR-0022](../adr/0022-remote-mirror-content-and-queries.md)
- SSH 프로필 관리: [`features/ssh-tool`](../features/remote-profiles/index.md)
- 로컬 self attach 격리: [`dev-guide/debug-ipc`](debug-ipc.md)


## Self-attach 거절 검증

GUI의 IPC/user dispatch 두 경로는 `attach_client/dispatch.rs::dispatch_attach`를 공유한다.
자기 포트면 `RejectedSelf`로 반환하고 connector를 호출하지 않는다. 다른 포트면 connector를
정확히 한 번 호출하며 성공값/오류를 그대로 돌려준다. IPC는 성공해도 포커스를 옮기지 않고,
사용자 경로만 기존처럼 새 mirror를 포커스한다. 이 분기는 raw stream client의 정상 로컬
attach와 별개다.

정확성 시험은 connector 진입 횟수와 결과를 직접 단언한다. debug GUI의 통합시험은
`attach_dispatch_completed` 로그에서 자기 port/workspace/source에 대응하는 **완료**와
`connector_entries=0`, `outcome=rejected_self`를 확인한다. 이 기록은 dispatcher 반환 뒤에
생기며, connector에 들어갔으면 실패해 돌아와도 진입 횟수가 남는다. 기록 구현은 모듈 선언에
`cfg(debug_assertions)`가 붙은 별도 `dispatch/debug_completion.rs`에 있다. 새 IPC나 전역
진행 카운터는 없다.

`tests/attach_silent_disconnect.rs`는 점유가 잡히지 않는 관측과 정상 재attach도 유지한다.
6초 관측 창의 RTT 수열·최악·중앙값·2초 이상 표본 수는 진단이며 정확성 단언에 쓰지 않는다.
GUI debug 시험의 완료 기록이 없으면 queued 응답만으로 통과하지 않는다. headless에서
GUI 큐가 처리되지 않는 현재 동작은 별도 시험 이름으로 검사하므로, 그 통과를 GUI 거절
검증으로 세지 않는다. release의 공통 분기는 유닛시험 대상이며 debug 로그를 요구하는
통합시험은 debug 조합에만 있다. 근거와 대안은 [준비 부족과 제품 결함을 구분한다](self-verification.md#준비-부족과-제품-결함을-구분한다).
