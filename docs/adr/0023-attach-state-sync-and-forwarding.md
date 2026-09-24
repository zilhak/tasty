# ADR-0023: attach 연결의 출력과 구조 변경을 같은 순서로 동기화한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: attach, stream, synchronization
- **Group**: terminal

## Context

mirror는 출력 바이트, 화면 크기, surface 구조가 같은 순서로 적용되어야 원본을 재현할 수 있다. 창이 숨겨지거나 일부 프레임을 잃은 경우에도 오래된 상태를 최신처럼 보이면 안 된다.

## Decision

mirror 출력은 창이 있는 engine과 parked engine 모두에 즉시 적용한다. 적용 대상을 찾은 뒤에만 outbox를 비운다. MirrorOutbox의 내부 buffer를 숨기고 take_for가 MirrorHost를 요구하도록 하여 대상 없이 데이터를 꺼내지 못하게 한다. Data·Resize·StructuralDelta는 도착 순서대로 처리한다.

mirror의 닫은 항목 복원은 서버에서 수행한다. PTY와 scrollback이 서버에 있으므로 로컬 항목을 대신 복원하지 않는다. 서버의 닫은 항목은 출처 workspace ID를 저장하고 원격 복원은 anchor workspace의 항목만 선택한다. workspace 전체 항목은 그 범위에서 제외한다. 로컬 사용자의 복원은 기존 전역 LIFO를 유지한다.

프레임을 잃으면 수신을 선언한 client에게 Loss{frames}를 보낸다. server는 ipc.stream.loss-notify capability를 알리고 client는 ClientLossNotify를 보낸다. 기존 프로토콜 번호를 올려 모든 구 client를 끊지 않는다. 선언하지 않은 연결의 바이트 흐름은 유지한다.

전송 큐가 가득 차 손실 통지도 보낼 수 없으면 pending_loss에 보관한다. 통지를 실제로 넣은 뒤에만 그 수를 지운다. 통지 전송을 정상 데이터 소비로 간주해 연속 drop의 lag를 초기화하지 않는다.

손실 복구는 연결이 운반하는 데이터 중 가장 강한 요구에 맞춘다. PTY delta는 snapshot을 다시 받고, 상태 snapshot은 오래됐다고 표시하며, mesh 조립은 종료하고 처음부터 구독한다. bulk는 결과 불명인 중단으로 처리해 자동 재시도하지 않는다. 이미 commit됐을 수 있기 때문이다.

현재 workspace mirror는 재attach로 복구한다. Detach를 보내고 이전 연결의 EOF를 확인한 뒤 새 연결을 만든다. 서버는 Disconnected를 inbound에 넣은 뒤 소켓을 닫아 새 attach가 이전 해제 뒤에 도착하게 한다. GUI는 한 세션에서 재attach를 한 번에 하나만 진행한다. 진행 중 추가 Loss는 합산하며, 완료 뒤 다시 손실이 나면 재attach할 수 있다. parked engine의 복구는 창이 돌아올 때까지 미루되 연결과 출력 처리는 유지한다.

CLI dump는 최대 세 번 재attach하고 이후에도 손실이 있으면 stderr에 알린 채 결과를 출력한다. --send는 최초 연결에만 수행한다. raw bridge는 횟수 제한 없이 재attach하되 Ctrl+\와 stdin EOF는 대기 중에도 종료한다. 손실과 재연결 snapshot 때 출력 stream ID를 바꾸어 이전 위치 커서를 가진 독자가 불연속을 알 수 있게 한다.

밀린 Loss는 write 스레드가 큐에서 프레임 하나를 꺼내 빈 공간을 만든 직후 넣는다. 뒤따르는 출력이나 heartbeat를 기다리지 않는다. push 앞 재시도와 pump_inbound 끝 재시도도 유지해 선언이 손실보다 늦게 온 경우를 처리한다. 평소에는 원자 owes_notice만 읽고 필요한 때만 sink map을 잠근다. 수신자는 sender를 직접 소유하지 않고 Weak로 map에 접근해 연결 해제를 막지 않는다.

forward 구조 요청은 origin을 전달한다. 새 client는 user_triggered에 따라 user 또는 agent를 항상 보낸다. 새 server는 user close만 복원 스택에 넣는다. 필드 생략과 null은 옛 client 호환을 위해 User, 모르는 값은 프레임을 버리지 않고 Agent로 해석한다. tab 선택도 같은 origin을 사용한다.

PTY 종료나 서버 로컬 멤버 추가로 바뀐 점유 workspace도 StructuralDelta로 보낸다. 변경 workspace 집합을 기록하고 정리가 끝난 뒤 전송한다. 새 멤버의 트리는 snapshot tap보다 먼저 보낸다. workspace가 사라지면 강제 detach하고 lock을 정리한다. forward는 자신의 Result와 Delta를 보낸 뒤 별도 변경 표시를 지워 중복을 피한다.

forward anchor가 없을 때는 모든 engine에서 surface 생존 여부를 먼저 확인한다. client가 실제 workspace를 점유하고 surface가 서버 전체에 없을 때만 IPC와 같은 no live surface 사유를 반환한다. 다른 곳에 살아 있거나 점유 workspace가 없으면 workspace not found를 유지한다. 사유에는 실제 structural_op 이름을 넣고 포커스로 대체하지 않는다.

CLI surface·workspace dump도 수집 루프에서 5초마다 Ping을 보낸다. 대기는 수집 종료와 다음 Ping 중 먼저 오는 때까지만 한다. 별도 스레드를 만들지 않으며 송신 실패는 Disconnected로 처리한다. 긴 --dump-after 값을 거절하지 않는다.

anchor 없는 mirror가 끊겨 마지막 workspace가 사라지면 기본 터미널 workspace를 다시 만든다. 사용자 요청이 아닌 연결 사건으로 창을 닫지 않는다. 창 있는 engine과 parked engine 모두 같은 복구를 사용하며 시스템 복구에 사용자 생성 event를 내지 않는다.

forward convert 실패는 SurfaceConverted.failure에 도메인이 남긴 사유를 그대로 전달한다. 사유가 없으면 ‘surface N was not converted’처럼 결과만 말한다. 대상이 없다고 추측하지 않는다. Core::apply의 replaced:false 반환 방식은 유지한다.

## Consequences

parked engine도 VT를 파싱하지만 toast·repaint는 생략하고 필요한 진단은 로그로 남긴다. 복원 때 이미 갱신된 grid를 그린다. 대상 없는 세션은 고아 세션 정리에서 제거한다. 생존 판단·정리·출력 적용은 같은 engine 범위를 순회해야 한다.

원격 스택이 비면 해당 안내를 반환하고 로컬 스택을 소비하지 않는다. 로컬 사용자는 서버에서 원격 사용자가 닫은 항목도 복원할 수 있어 한 방향의 기록 간섭은 남는다. snapshot 저장과 plugin의 사용자 닫기 이벤트는 별도 값으로 다룬다. 복원된 과거 scrollback 전체는 mirror에 전달하지 않고 기존 snapshot+delta 규칙을 따른다. 구 server가 모르는 복원 op는 응답 없이 무시될 수 있다.

통지는 마지막으로 보존된 프레임 뒤와 손실 이후 데이터 앞에 위치한다. 다만 통지도 큐 한 칸을 쓰므로 매 push마다 한 칸만 비우는 client는 데이터가 계속 거절되어 lag 한도에서 끊길 수 있다. 선언하지 않은 client는 같은 속도에서도 유지될 수 있는 비용을 감수한다.

연결 전체 snapshot을 다시 받아 큰 workspace는 비용이 크고 survivor scrollback에 화면이 한 번 더 남을 수 있다. mesh cache와 구독 중복 방지 상태도 지워야 한다. 창에서 시작한 재attach의 EOF 대기 중 그 창이 park되면 anchor 없는 세션이 정리될 수 있는 한계가 있다.

lag는 연속 실패 횟수, frames_dropped와 clients_lagged_out은 누계, backlog는 살아 있는 연결의 미전송 큐 길이 합이다. 누계와 backlog는 system.pressure에 sink_capacity와 함께 노출한다. 연결별 큐 길이를 합산해야 끊긴 연결의 backlog가 남지 않는다.

Loss 재시도와 데이터 삽입 사이에 write 스레드가 공간을 만드는 경합에서는 데이터 한 장이 통지보다 앞설 가능성이 소스상 남는다. 과거 실행 실험에서는 재현하지 못했고 앞지르기 가드를 추가하지 않았다.

구 server는 origin 필드를 무시하므로 새 client의 agent close도 복원 스택에 남는다. 서버 업데이트가 필요하다. forward user origin은 snapshot 여부와 탭 선택을 정하지만 plugin lifecycle의 is_user_close와는 구분한다. StreamReady만으로 전송하는 기타 구조 변경은 다음 stream 활동까지 지연될 수 있다.

5초 이하 dump에는 Ping이 추가되지 않고 더 긴 수집에만 작은 주기 프레임이 추가된다. 서버 멈춤 감지는 기존 read timeout을 따른다. 마지막 mirror 정리는 사용자가 요청하지 않은 기본 PTY 하나를 만들지만 창의 유효한 작업 영역을 유지한다. convert의 failure가 누락되면 원인 정보는 부족해지지만 잘못된 원인을 알리지는 않는다. image.open의 기존 오류 문구까지 이 결정이 고치지는 않는다.

## Alternatives Considered

창 없는 동안 쌓기만 하면 메모리 상한·구조 delta 보존·재생 지연 문제가 생긴다. 원격에 전송 보류를 요청해도 서버가 버퍼를 소유해야 한다. 최소화만으로 세션을 끊으면 사용자의 점유가 풀린다.

holder별 별도 스택은 점유 전환 때 기록 위치를 바꾸고 로컬 undo 동작도 달라진다. 원격이 전역 스택을 pop한 뒤 거절하면 항목과 scrollback 수명을 되돌려야 한다. 서버에서 로컬 복원의 focus 후처리까지 실행하면 원격 조작이 서버 사용자의 화면을 바꾼다.

모르는 이벤트를 무시하는 구 client에게 Loss를 무조건 보내도 손실을 이해하지 못한다. server 전체 누적 카운터 폴링은 특정 연결의 손실 위치를 알려주지 않는다. Loss가 막혔다고 버리면 가장 필요한 순간에 통지가 사라진다. 통지 성공으로 lag를 초기화하면 계속 뒤처지는 client가 해제되지 않는다.

서버가 임의 시점에 snapshot을 push하면 이미 큐에 들어간 tap과 순서가 섞인다. 새 Resync 요청은 구 server의 무응답과 추가 협상이 필요하다. mesh만 따로 복구해도 같은 연결의 PTY 때문에 재attach가 필요하다. 손실 때 mirror를 닫으면 사용자 작업 공간이 사라진다.

dump 종료 직전 Ping·임의 대기 또는 주기 tick보다 큐 dequeue 시점이 실제 공간 확보를 직접 알 수 있다. 수신자에게 sender를 보관시키면 연결 종료 후에도 채널이 열려 남는다.

origin 부재를 Agent로 읽으면 옛 사용자 undo를 깨고, 새 V2 variant는 구 server에서 op 전체가 사라진다. 모든 forward를 사용자나 에이전트로 통일하면 어느 쪽의 복원 규칙도 만족하지 못한다. 서버 구조 변화를 매 루프 fingerprint로 검사하기보다 공통 제거·편입 지점에서 표시한다. 구조 정리 도중 즉시 delta를 보내면 중간 트리와 Result 순서가 노출된다.

긴 dump에 경고만 해도 20초에서 끊기는 문제는 남고 값 제한은 기존 사용을 막는다. dump writer는 한 루프만 쓰므로 heartbeat 스레드가 필요 없다. 빈 창 렌더만 건너뛰면 IPC·입력이 같은 빈 workspace 상태에서 실패한다. convert를 Err로 바꾸면 로컬 toast와 image.open의 오류도 바뀌므로 이벤트의 사유만 확장한다.

## Reconsideration Triggers

parked 파싱의 배터리·CPU 비용이 실제 문제가 되거나 로컬 PTY도 파싱을 멈추게 되면 흐름 제어를 검토한다. 창이 필수인 부수효과나 새 engine 보관 위치가 추가되면 세 순회와 창 존재 검사를 함께 수정한다.

holder가 바뀐 뒤 이전 사용자의 기록을 복원하는 문제가 생기거나 로컬 전역 LIFO가 혼란을 만들면 workspace 대신 사용자 스코프 또는 로컬 스코프를 검토한다.

프로토콜 번호의 호환 판정이 바뀌거나 선언형 기능이 늘면 일반 capability 협상을 검토한다. 일시 과부하 뒤 통지 순서와 지속 과부하의 연결 해제를 나누어 검증한다.

Loss에 surface·종류가 실리거나 attach 밖의 안전한 snapshot 생산자, 상태 전용 연결이 생기면 복구 범위를 줄일 수 있다. 큰 workspace 복구 시간과 느린 링크의 반복 재attach는 실제 손실·완료 로그로 함께 평가한다.

SinkReceiver 밖에서 큐를 비우는 경로, 원자적 통지·데이터 삽입 방법, 실제 앞지르기 관측이 생기면 손실 순서를 검토한다. drop 중 IPC 지연이 커지면 sink 잠금 비용을 측정한다. user/agent로 표현할 수 없는 요청자가 생기거나 구 server가 사라지면 origin 호환을 재검토한다. 구조 변경을 표시하지 않는 새 경로와 코드 기반 forward 오류가 필요한 소비자가 생기면 전송·오류 규칙을 확장한다.

dump가 writer를 공유하게 되거나 server가 timeout을 협상하면 heartbeat 구조를 검토한다. anchor 없는 세션도 재연결을 지원하면 마지막 workspace 복구를 재검토한다. workspace를 제거하는 새 경로는 사용자 창 닫기 또는 빈 영역 복구를 수행해야 한다. convert 반환 방식이 바뀌거나 reason을 코드처럼 파싱하는 소비자가 생기면 오류 전달 형식을 검토한다.

## References

- [attach 구현](../dev-guide/attach-behavior.md)
- [원격 attach](../features/remote-attach/index.md)
