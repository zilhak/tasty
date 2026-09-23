# ADR-0620: 원격 연결과 attach 설정을 분리한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: remote, ssh, profiles
- **Group**: terminal

## Context

서버는 loopback 연결만 받으므로 SSH 원격성과 프로필 해석은 client가 담당한다. 같은 SSH 주소에 여러 attach 구성이나 다른 소비자를 연결할 수 있어야 한다.

## Decision

release CLI는 remote attach를 제공하고 로컬 attach 검증 진입점은 debug에 둔다. 서버 attach 수신은 공통으로 유지한다. 단발 화면 조회는 attach 세션 대신 read API를 사용한다. GUI client가 자기 서버 포트로 동기 attach하면 교착하므로 연결 전에 거절한다. 별도 프로세스의 debug attach 검증과 구분한다.

원격 프로필은 열린 kind 문자열과 Str/List 필드의 레지스트리다. 동작은 소비자가 해석하며 자격증명은 passkey 이름으로 참조한다. 미등록 kind는 등록을 허용하되 경고한다. plugin은 선언한 kind만 접근하고 native 기능은 필요한 종류를 사용 시 검증한다.

ssh 프로필은 연결·셸 감지 정보, tasty-attach 프로필은 attach 설정을 담는다. attach는 ssh_ref를 매번 이름으로 다시 읽거나 인라인 SSH 정보를 사용한다. remote_tasty, port_mode, port_file은 attach 소유다. 명시 port_file이 자동 발견보다 우선한다.

포트 발견에는 SSH 연결 시간, 자식 프로세스 실행 시간, 전체 발견 호출 시간을 각각 제한한다. ConnectTimeout 기본값은 10초, 단계별 자식 감시는 20초, 전체 호출은 45초다. 각 단계는 남은 전체 시간과 단계 상한 중 작은 값을 사용하고, 시간이 없으면 다음 자식을 시작하지 않는다. 시간 초과 자식은 kill 후 wait로 회수하고 TimedOut으로 구분한다.

## Consequences

서버는 SSH 구현을 알 필요가 없다. 동일 SSH 정보의 변경은 이를 참조하는 attach에 즉시 적용된다. 참조가 없거나 비활성인 경우는 명시적으로 실패한다. 구 ssh-profiles 파일 자동 이관과 ssh 안의 attach 필드 해석은 제공하지 않는다.

SSH extra_options의 ConnectTimeout은 기본값보다 먼저 전달해 사용자 값이 우선한다. 그러나 전체 45초는 넘길 수 없다. 느린 다중 hop 연결은 오탐으로 끝날 수 있다. 포트 발견의 제한된 출력 수집에서 파이프가 가득 차도 감시 시간이 끝나면 종료한다.

## Alternatives Considered

서버에서 모든 loopback attach를 막으면 SSH 원격 attach도 막는다. 닫힌 kind enum은 plugin 확장을 제한한다. 프로필에 액션을 넣으면 한 주소를 여러 용도로 쓰기 어렵다. ssh에 attach 필드를 섞거나 별도 attach 파일을 두기보다 같은 레지스트리에서 종류를 나눈다. 참조를 복사하면 SSH 수정이 반영되지 않는다.

ConnectTimeout만으로 인증 후 정지·원격 명령·TTY 프롬프트 대기를 막지 못한다. 프로세스 상한만으로는 응답 없는 호스트에서 매 단계 상한을 다 쓰며, 전체 예산이 없으면 시도 개수만큼 대기한다. 모든 발견 방식을 병렬 실행하면 정상 요청에도 SSH 자식을 여러 개 만든다.

## Reconsideration Triggers

release 로컬 attach의 실제 용례, 미등록 타입의 관리 문제, 강제 스키마 요구가 생기면 범위를 검토한다. ssh와 attach가 항상 일대일이거나 attach 전용 파일이 필요한 소비자가 생기면 분리 방식을 다시 비교한다. 특정 원격 셸에서 port_file 읽기가 실패하면 셸별 처리를 검토한다.

정상 연결이 반복해서 제한에 걸리거나, 취소 가능한 비동기 발견·새 탐색 단계·BatchMode 변경이 생기면 세 제한의 값과 역할을 다시 평가한다.

## References

- [attach 구현](../dev-guide/attach-behavior.md)
- [원격 프로필](../features/remote-profiles/index.md)
- [원격 attach](../features/remote-attach/index.md)
