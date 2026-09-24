# ADR-0021: 점유한 작업은 연결 소유권에 따라 보호한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: attach, occupancy, permissions
- **Group**: terminal

## Context

에이전트가 계속 사용하는 작업임을 알리는 표시와 원격 사용자의 입력을 독점하는 잠금은 서로 다른 요구다. 잠금이 남거나 우회되면 다른 사람이 사용하는 터미널을 변경할 수 있다.

## Decision

점유는 soft와 hard로 구분한다. soft는 지속적인 사용 관계를 표시하지만 로컬 입력을 허용한다. hard는 attach 소유자 외의 입력과 구조 변경을 제한한다. 대상은 한 소유자에게 속하고 소유자는 여러 대상을 사용할 수 있다. 점유 대상은 생성 방식과 무관하며 각 소비자가 허용 범위를 정한다.

명시 해제는 소유자와 로컬 사용자(force-detach)가 수행한다. soft는 부모 surface를 기록해 부모가 사라진 뒤 사용자가 실제로 포커스했을 때 정리할 수 있다. hard는 연결 종료나 생존 확인 실패 때 회수한다.

hard 점유에서도 로컬 선택·복사는 허용한다. 좌표와 복사 문자열은 사용자가 보는 readonly mirror에서 읽는다. 클릭 트래킹을 로컬 선택으로 처리하며 휠과 링크 실행은 차단한다. 휠은 live scroll_offset을 바꾸어 해제 뒤 화면을 어긋나게 할 수 있고, 오래된 mirror의 링크 실행에는 외부 부수효과가 있다.

hard 점유의 연결 생존은 EOF와 heartbeat TTL로 판단한다. heartbeat 간격의 네 배 동안 아무 프레임도 받지 못하면 EOF와 같은 정리 경로를 사용한다. 사용자가 조작하지 않는 유휴 시간만으로 점유를 해제하지 않는다.

hard 점유된 workspace의 terminal.spawn은 자원을 만들기 전에 거절한다. 만들어도 즉시 hard lock에 포함되어 요청자가 자기 자식에 입력할 수 없기 때문이다. workspace 이름·ID 해소는 실제 spawn과 같은 함수를 사용한다. 점유자의 구조 변경 forward는 별도 소유권 검증을 거쳐 실행한다.

terminal.spawn의 제한은 최종 pane ID를 해소한 뒤 실제 소속 workspace에서 검사한다. workspace 인자와 pane override가 다를 수 있으므로 workspace 인자만 검사하지 않는다. mirror workspace도 생성을 시작하거나 forward 큐에 넣기 전에 거절한다. child registry는 즉시 생성 ID가 필요한데 일반 mirror forward는 그 ID를 동기로 반환하지 않기 때문이다.

점유 획득 전 stream.open의 proto를 현재 STREAM_PROTO와 비교한다. 생략된 0이나 불일치는 기존 StreamAck{ok:false,proto,error}로 거절한다. GUI self-attach 포트 검사는 debug에서도 적용한다. 정상 연결 이후의 EOF·TTL 회수는 계속 필요하다.

원격 조회·프로필 CRUD의 신뢰는 SSH와 loopback에 의존하지만, 로컬 mirror workspace를 만드는 remote.attach는 local_only로 둔다. 원격 접근 자격만으로 무권한 plugin에 로컬 구조 변경 권한을 주지 않는다.

로컬 사용자·에이전트의 닫기 요청은 대상 중 hard 점유 surface가 하나라도 있으면 전체를 거절한다. 공용 검사에 각 요청이 삭제할 surface 집합을 넘긴다. 점유자 자신의 forward와 이미 종료된 프로세스의 사후 정리는 정상 진행한다. 점유자 면제를 요청자가 임의로 넣는 플래그로 표현하지 않는다.

새 attach와 이전 점유자의 종료가 같은 inbound 배치에 있어도 끊긴 점유자가 재접속을 막지 않는다. 배치 시작에서 disconnected 사실만 registry에 표시하고, acquire가 그 점유자와 경쟁할 때만 즉시 회수한다. 경쟁이 없으면 기존 순서대로 잔여 입력을 처리한 뒤 배치 끝에서 해제한다. 표시도 배치 끝에서 제거한다.

## Consequences

soft 마커는 명령이 사용자의 미제출 입력과 독립적으로 실행되도록 돕는 정책을 표현하지만 모든 셸·TUI·암호 입력에서 안전한 줄 초기화를 보장하지 않는다. hard는 읽기·관찰과 로컬 사용자의 점유 해제 권한을 유지한다. soft/hard는 다른 표시를 사용한다. 테두리 우선순위는 NeedsInput, 점유, Completion 순서이며 탭 제목과 workspace 배지도 유지한다.

readonly mirror는 3초 주기로 갱신하므로 선택 화면에도 지연이 있다. 선택 이외의 IME·vi 커서·링크·검색 하이라이트는 계속 억제한다. soft 점유는 이 입력 제한을 적용하지 않는다. 네트워크 지연이 TTL보다 크면 살아 있는 연결도 해제될 수 있다.

pty.attach_surface가 점유된 workspace에 들어가는 유사한 경우까지 해결됐다고 가정하지 않는다. 이 경로는 소비자 동작과 함께 별도로 검토해야 한다.

사용자는 점유 표시 또는 사이드바의 강제 끊기로 점유를 회수한 뒤 닫을 수 있다. 사용자 거절은 방법을 안내하고 에이전트 오류는 문제 surface를 포함한다. 복원 기록은 새 세션을 만들 뿐 기존 원격 작업을 되살리지 못하므로 무경고 닫기를 허용하지 않는다.

## Alternatives Considered

점유 없이 send/read만 제공하면 지속적인 제어 관계가 보이지 않는다. hard만 사용하면 협조적인 에이전트 작업에도 사용자 입력을 막는다. 같은 표시를 쓰면 사용자가 현재 입력 가능 여부를 알 수 없다.

선택까지 막으면 readonly 화면의 내용을 복사할 수 없다. live 터미널에서 복사하면 보이는 mirror와 문자열이 달라진다. mirror 휠을 지원하려면 별도 mutable 접근과 스크롤 상태 정책이 필요하다. heartbeat에 별도 만료 registry를 만들지 않고 기존 socket timeout·Disconnected 정리를 재사용한다.

spawn 뒤 tell 오류만 개선하면 사용할 수 없는 자식을 성공으로 반환하는 문제는 남는다. 새 자식만 hard lock을 우회하면 점유자의 배타 사용 규칙을 깨뜨린다.

forward 응답에서 ID를 못 찾은 뒤 오류만 바꾸면 원격 고아 탭이 남는다. 라우터의 workspace 검사만으로는 다른 pane 지정이 우회한다. 모든 mirror 구조 변경을 막거나 동기 왕복으로 바꾸는 것은 정상적인 forward까지 변경하므로 terminal.spawn에만 적용한다.

버전이 틀린 연결의 TTL만 줄이면 정상 느린 연결도 끊으며, 애초에 사용할 수 없는 점유를 막지 못한다. 서버는 loopback의 자기 GUI와 SSH client를 구별하지 못해 self 검사 위치가 될 수 없다. 모든 닫기 공통 후처리에 검사를 두면 종료된 프로세스의 surface가 영구히 남는다. 원격 attach를 plugin에 무권한으로 열거나 소비자도 없는 새 권한을 미리 추가하지 않는다.

종료 정리를 무조건 배치 앞에 두면 경쟁이 없는 마지막 입력도 잃는다. client에게 already_attached 재시도를 맡기면 실제 충돌과 구별하지 못한다. StreamHub sink의 존재 여부만으로 연결 종료를 추정하면 등록 순서 때문에 살아 있는 점유자를 빼앗을 수 있다.

## Reconsideration Triggers

활발히 입력하는 사용자 surface의 hard 점유 정책, 세 번째 점유 단계, 여러 소유자의 동시 점유, attach와 다른 lease 수명이 필요해지면 모델을 재검토한다.

mirror 휠·실시간 갱신·선택 중 좌표 변화 문제가 생기면 readonly 조작을 다시 검토한다. heartbeat 오탐, 다른 transport, 재접속 유예 요구가 생기면 생존 판정과 해제 시간을 함께 검토한다.

pty.attach_surface에서 같은 입력 불가 문제가 확인되거나 점유된 workspace에 독립적인 background 자식을 만들 정당한 요구가 생기면 생성 제한 범위를 검토한다.

forward가 생성 ID를 반환하고 원격 child의 소유·입력을 다룰 수 있게 되면 mirror spawn을 다시 검토한다. 동기 ID가 필요 없는 spawn이 추가되면 메서드 전체 거절보다 좁은 조건을 비교한다.

프로토콜 호환 범위·인증·새 점유 진입점이 생기면 점유 전 검사를 함께 확장한다. 비동기 self-attach 필요가 생기면 교착과 제품 정책을 따로 검토한다. plugin의 정당한 remote.attach 소비자가 생기면 명시 권한을 첫 대안으로 검토한다. 로컬 강제 해제 경로가 모두 사라지거나 새 close 경로에서 누락이 발생하면 닫기 보호 구현을 재검토한다.

배치 순서나 pump가 추가되면 disconnected 표시가 attach보다 먼저 전달되는지 확인한다. sink 등록·해제 순서가 acquire와 명확히 보장되면 registry의 정보 전달을 단순화할 수 있다.

## References

- [주체와 점유](../concepts/actors.md)
- [attach 구현](../dev-guide/attach-behavior.md)
