# ADR-0061: 외부 원격 연결은 Remote가 소유하고 attach 동기화는 서버의 확정 순서를 따른다

- **Status**: Accepted — 구현 상태: 단계적 이행 중. 현재 연결·재연결·ID mapping 상태가 App 필드와 attach client·Core attach runtime에 나뉘어 있다
- **Date**: 2026-09-30
- **Tags**: attach, remote, stream, synchronization, plugins
- **Group**: terminal

## Context

SSH 실행·터널·포트 발견은 `tasty-ssh`, 원격 workspace 조회·생성은 `tasty-remote`에 있다. 본체에서는 `src/app/attach_client.rs`가
socket·SSH 터널·remote/local ID mapping·mirror 이벤트 처리를 함께 갖고, `src/app/auto_attach.rs`·`src/app/attach_poll.rs`와 App 필드가
연결 시도·재연결 상태를 나눠 가진다. 서버 쪽 attach는 `src/core/attach_runtime.rs`와 `src/core/impl_attach.rs` 등 Core 안에 있다.
plugin 프로세스 통신은 `tasty-host-plugin`의 process·listener·handle channel이 담당한다.

[ADR-0023](0023-attach-state-sync-and-forwarding.md)은 이 배치에서 mirror 출력·resize·구조 변경의 순서, 손실 통지와 재attach 복구,
forward 요청의 출처, parked 엔진의 즉시 적용을 정했다. mirror 적용 대상을 창 있는 엔진과 parked 엔진 순으로 찾고
`MirrorOutbox`를 대상 없이 비우지 못하게 하는 방식은 엔진이 창 상태와 parked 목록에 나뉘어 있다는 전제에서 나왔다.

[ADR-0054](0054-app-core-view-layers-and-state-ownership.md)에서는 엔진 목록이 `EngineSession` 집합 하나이고,
[ADR-0055](0055-structural-domain-event-sourcing.md)에서는 구조 변경의 순서를 stream revision이 정한다.
외부 연결 상태를 App과 Core에 흩어 두면 재연결·epoch·점유의 경계가 흐려지고, 소켓을 쓴다는 공통점 때문에 plugin 내부 통신과 섞이기 쉽다.

## Decision

### Remote 모듈의 범위

외부 Tasty와의 SSH·attach 연결은 remote 모듈이 맡는다. 외부 진입은 `Remote`, 상태 원본은 `RemoteState`다.

- `RemoteState`: 연결 시도·세션별 상태·connection epoch·재시도·remote/local ID mapping·구독 cursor·pending 요청·표시 context(geometry·theme·focus).
  attach 세션은 이 안의 세션 단위이며 별도 전역 관리자가 아니다. transport와 상태를 나누되 같은 데이터를 두 State에 복제하지 않는다.
- transport: socket·SSH 자식·reader/writer 핸들·timeout·취소. 기존 `tasty-ssh` 터널을 재사용하며 State에 직렬화하지 않는다.
- 포함: SSH 연결·포트 발견, 원격 조회·생성 요청, attach handshake·입력·출력·제어, 재연결, 서버 쪽 attach 세션의 stream·heartbeat·손실 처리,
  원격 파일·캡처·bulk·mesh 전송의 연결 수명과 wire 처리.
- 제외: host↔plugin 자식 프로세스의 내부 protocol·권한·handshake·shared memory·handle channel, local CLI·에이전트 JSON-RPC 진입, Webhook 같은 다른 외부 기능.
  모든 통신을 포괄하는 NetworkManager는 만들지 않는다.
- crate 배치는 기존 `tasty-remote` 확장이다([ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md)). 이 crate는 root 도메인을 참조하지 않으므로
  mapping·연결 관측·typed action을 반환하고 App의 adapter가 명령을 실행한다.

### 도메인·화면과의 경계

- 누구의 입력을 허용할지, surface 생존, hard 점유 판정은 Core와 `LiveDomainState`가 맡는다([ADR-0021](0021-occupancy-and-attach-admission.md)). Remote는 검증된 연결 주체를 전달한다.
  점유는 connection epoch에 묶이며, 이전 실행의 점유를 재시작 뒤 현재 잠금으로 복원하지 않는다.
- Remote는 도메인 모델을 직접 바꾸지 않는다. 서버에서 받은 구조 변경은 명령으로 CommandExecutor에 넣고, client mirror 구조는 원격 확정 결과의 local projection이다.
- 원격 terminal bytes와 VT 응답은 Terminal ingest와 원격 sink로 연결하며 local Pty를 요구하지 않는다([ADR-0060](0060-terminal-and-pty-separation.md)).
- plugin mesh의 생산과 plugin IPC는 plugin 모듈에 남는다. Remote는 명시한 frame·context·input 경계로 교환한다.
  네트워크 오류가 내부 plugin 채널을 끝내거나 plugin 재시작이 무관한 SSH 연결을 끝내지 않는다.
- `RemoteState`는 구조 저널의 원본이 아니다. 재생만으로 socket·점유·SSH 프로세스를 복원하지 않는다. 영속 프로필과 runtime 연결 상태를 합치지 않는다.

### 동기화 순서

- 원격 구독은 revision R의 snapshot과 R 이후 확정 기록으로 이어진다. snapshot과 구독 등록 사이의 구조 변경을 놓치지 않는다.
  terminal bytes의 snapshot·tap은 Terminal의 일관성 경계를 따른다. revision 기반 재동기화 확장은 capability로 협상하고 기존 wire는 유지한다.
- mirror의 Data·Resize·StructuralDelta는 도착 순서대로 적용한다. 적용 대상 엔진을 찾은 뒤에만 수신 buffer를 비우며, 대상 없이 데이터를 꺼내지 않는다.
- mirror 출력은 창이 있는 엔진과 parked 엔진 모두에 즉시 적용한다. 엔진 조회는 `EngineSession` 집합 하나를 사용한다.
  parked 엔진도 VT를 파싱하지만 toast·repaint는 생략한다. 대상 없는 세션은 고아 세션 정리에서 제거한다.

### 유지하는 wire·복구 규칙

아래 규칙은 새 배치에서도 유지한다. 세부 동작은 [attach 구현](../dev-guide/attach-behavior.md)이 설명한다.

- 손실: 수신을 선언한 client에 `Loss{frames}`를 보낸다(`ipc.stream.loss-notify`). 기존 프로토콜 번호를 올려 구 client를 끊지 않는다.
  큐가 가득 차면 `pending_loss`에 보관하고 실제로 넣은 뒤에만 지운다. 통지 전송으로 lag를 초기화하지 않는다.
  밀린 통지는 write 스레드가 프레임 하나를 꺼내 공간을 만든 직후 넣는다.
  프레임의 기존 뜻이 바뀔 때만 `STREAM_PROTO`를 올리고, 더해지는 기능은 `ipc.stream.<기능>` 이름으로 선언한다.
- 전송 압력: lag는 연결별 연속 전송 실패 횟수, `frames_dropped`와 `clients_lagged_out`은 누계, `backlog`는 살아 있는 연결들의 미전송 큐 길이 합이다.
  누계와 `backlog`는 `system.pressure`에 `sink_capacity`와 함께 노출한다. 연결별 큐 길이를 합산해 끊긴 연결의 backlog가 남지 않게 한다.
- 복구: 손실 복구는 연결이 전달하는 데이터 중 가장 강한 요구를 따른다. workspace mirror는 Detach 후 이전 연결의 EOF를 확인하고 재attach한다.
  한 세션에서 재attach는 한 번에 하나이고 진행 중 추가 손실은 합산한다. parked 엔진의 복구는 창이 돌아올 때까지 미루되 연결과 출력 처리는 유지한다.
  bulk 중단은 결과 불명으로 처리해 자동 재시도하지 않는다.
- CLI: dump는 최대 세 번 재attach하고, 수집 루프에서 5초마다 Ping을 보낸다. raw bridge는 횟수 제한 없이 재attach한다.
  손실과 재연결 snapshot 때 출력 stream ID를 바꿔 이전 위치의 독자가 불연속을 알 수 있게 한다.
- 요청 출처: forward 구조 요청은 origin(user·agent)을 전달한다. 필드 생략과 null은 옛 client 호환을 위해 user, 모르는 값은 agent로 해석한다.
  서버는 user close만 복원 기록에 넣고, mirror의 닫은 항목 복원은 PTY와 scrollback이 있는 서버에서 수행한다. 원격 복원은 anchor workspace의 항목만 고른다.
- 서버 쪽 변경: PTY 종료나 서버 로컬 변경으로 바뀐 점유 workspace도 정리가 끝난 뒤 구조 변경으로 보낸다. workspace가 사라지면 강제 detach하고 잠금을 정리한다.
  forward 실패 사유는 도메인이 남긴 문구를 그대로 전달하고 추측하지 않는다.
- 연결 사건: anchor 없는 mirror가 끊겨 마지막 workspace가 사라지면 기본 터미널 workspace를 다시 만들고, 사용자 요청이 아닌 연결 사건으로 창을 닫지 않는다.

## Consequences

외부 연결의 epoch·재연결·mapping이 한 곳에 있어 재시작·재연결 뒤 오래된 점유나 mapping이 살아남는 경로를 찾기 쉽다.
plugin 내부 채널과 외부 연결의 수명·권한·큐 예산이 섞이지 않는다. 서버 구조 변경이 명령 경계를 거치므로 원격 경로가 기록에서 빠지지 않는다.

기존 wire를 유지하므로 구 client·server와의 호환 비용은 그대로다. 구 server는 origin을 무시해 새 client의 agent close를 복원 기록에 남긴다.
Loss 재시도와 데이터 삽입의 경합으로 데이터 프레임 하나가 통지보다 앞설 가능성은 소스상 남아 있다.
큰 workspace의 재attach는 전체 snapshot을 다시 받아 비용이 크다.

이행 중에는 App 필드·attach client·Core attach runtime에 남은 상태를 Remote로 옮기며, 한 상태의 원본이 두 곳에 있는 기간이 없도록 한 번에 옮긴다.

## Alternatives Considered

- 현재처럼 App과 Core에 연결 상태를 나눠 두는 안: 재연결·epoch·점유 회수의 경계가 흐려지고 창 수명과 연결 수명이 다시 엮인다.
- 모든 소켓 통신을 NetworkManager 하나로 합치는 안: Remote·Webhook·local IPC·plugin IPC의 권한·수명·재시도가 서로 다르다.
- Remote가 mirror 구조를 도메인 모델에 직접 쓰는 안: 원격 경로가 기록 밖 writer가 된다.
- 창이 없는 동안 mirror 데이터를 쌓기만 하는 안: 메모리 상한과 구조 delta 보존, 재생 지연 문제가 생긴다. 최소화만으로 세션을 끊으면 점유가 풀린다.
- 손실 때 mirror를 닫는 안: 사용자의 작업 공간이 사라진다. 서버가 임의 시점에 snapshot을 push하면 이미 큐에 들어간 tap과 순서가 섞인다.
- 요청 출처 생략을 agent로 읽는 안: 옛 client의 사용자 undo가 깨진다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- 원격 호스트 간 원자적 변경이나 여러 writer가 같은 stream을 쓰는 요구가 생기면 동기화 모델을 다시 정한다.
- 헤드리스 attach client가 제품 요구가 되면 Remote의 client 쪽 범위를 헤드리스에 연결한다([ADR-0058](0058-headless-without-local-views.md)).
- CLI에 GUI·host 전용의 큰 의존이 `tasty-remote`를 통해 들어오면 그 부분만 feature나 crate로 나눈다.
- user·agent로 표현할 수 없는 요청자가 생기거나 구 server가 사라지면 origin 호환을 재검토한다.

### 실행 결과로 확인

- 큰 workspace 복구 시간과 느린 링크의 반복 재attach를 손실·완료 로그로 평가해 snapshot 범위를 다시 본다.
- Loss와 데이터의 앞지르기가 실제로 관측되면 통지 삽입 방식을 바꾼다.
- parked 파싱의 CPU 비용이 문제로 관측되면 흐름 제어를 검토한다.

## References

- 대체 대상: [ADR-0023](0023-attach-state-sync-and-forwarding.md)
- [ADR-0020](0020-remote-connection-profiles.md) · [ADR-0021](0021-occupancy-and-attach-admission.md) · [ADR-0022](0022-remote-mirror-content-and-queries.md) · [ADR-0026](0026-plugin-registration-and-lifecycle.md) · [ADR-0028](0028-egui-mesh-rendering.md)
- [ADR-0054](0054-app-core-view-layers-and-state-ownership.md) · [ADR-0055](0055-structural-domain-event-sourcing.md) · [ADR-0060](0060-terminal-and-pty-separation.md)
- [attach 구현](../dev-guide/attach-behavior.md), [원격 attach](../features/remote-attach/index.md)
- 현재 구현: `src/app/attach_client.rs`, `src/app/auto_attach.rs`, `src/app/attach_poll.rs`, `src/core/attach_runtime.rs`, `src/core/impl_attach.rs`, `crates/tasty-remote`, `crates/tasty-ssh`.
