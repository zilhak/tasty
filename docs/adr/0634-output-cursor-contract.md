# ADR-0634: 터미널 출력은 스트림과 바이트 위치로 이어 읽는다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: plugins, runtime
- **Group**: plugins

## Context

surface에 하나씩 있는 read mark와 scan mark를 여러 소비자가 공유하면 서로 읽는 범위를 바꾼다.
출력이 보존 범위를 벗어났을 때 조용히 처음부터 읽는 것도 잘못된 연속성을 보여준다.
기존 메서드에 위치 인자를 추가하면 구 서버가 그 인자를 무시하고 성공으로 답할 수 있다는 호환 문제도 생긴다.

## Decision

surface.read_since_mark에 소비자 보유 cursor와 stream을 받는다.
cursor가 없으면 기존 mark 의미를 유지한다. 명시 위치 조회는 그 surface만 읽고 focus fallback을 하지 않는다.
원문 바이트의 절대 위치는 trim 후에도 유지하고 보존 시작·끝, 실제 읽은 범위, next_cursor와 skipped를 응답한다.
소비자는 text 길이가 아니라 next_cursor로 진행한다.

stream은 버퍼 교체를 구별한다. cursor에는 stream이 필수이고 서로 다른 stream이나 끝보다 큰 cursor는 구조화된 사유로 거절한다.
max_bytes는 현재 보존 크기 이내의 원문 길이이며 0은 한 바이트로 올린다.
UTF-8 경계는 가능한 한 보존하되 읽을 바이트가 있는데 계속 빈 답을 주지는 않는다.
출력 보존은 메모리이며 기존 scanner cursor와 사용자 화면 상태는 변경하지 않는다.

서버는 ipc.output-cursor capability를 선언한다.
CLI는 cursor·stream·max_bytes 중 하나라도 보낼 때 이름과 지원 버전을 확인한다.
지원하지 않으면 요청을 보내지 않고 sent:false를 포함한 구조화된 오류로 끝낸다.
인자 없는 기존 호출은 협상 왕복 없이 유지한다.

--cursor는 --stream을 요구하지만 --stream 단독은 mark 조회 중 스트림 교체 검사로 허용한다.
scan-mark CLI는 만들지 않는다. 여러 소비자의 독립 읽기는 이 cursor 계약을 사용한다.

## Consequences

소비자마다 호스트 상태를 만들지 않고 같은 터미널을 독립적으로 읽을 수 있다.
retention보다 느린 소비자는 여전히 출력을 잃지만 잃은 바이트 수를 확인할 수 있다.
원문과 디코딩·ANSI 제거된 text 길이는 다르다.

한 메서드가 기존 mark와 명시 cursor 두 방식을 제공하므로 호출자는 차이를 알아야 한다.
capability 확인에는 연결별 왕복이 추가된다. 직접 IPC를 쓰는 다른 client도 스스로 확인해야 구 서버의 인자 무시를 피한다.
이 조회는 PTY 출력이며 사용자 키 입력 기록이나 화면 복원 기능은 아니다.

## Alternatives Considered

- 새 메서드는 같은 자원·권한에 두 API를 유지하는 비용이 생긴다.
- consumer별 서버 mark는 등록·해제·누수 관리가 필요하다.
- stream 없이 숫자 cursor만 쓰면 respawn 뒤 다른 출력에 옛 위치가 적용된다.
- 미래 위치에 빈 답을 주면 뒤늦게 도착한 무관한 구간을 연속 출력으로 오인한다.
- 응답 필드를 보고 사후 판정하면 구 서버가 이미 다른 범위의 text를 돌려준 뒤다.

## Reconsideration Triggers

실제 skipped 빈도가 보존 요구를 충족하지 못하면 메모리 크기와 별도 저장 요구를 검토한다.
새 인자를 추가하거나 기존 의미를 좁힐 때 capability 이름과 버전 정책을 함께 확인한다.
Plugin client가 cursor 읽기를 사용하면 그 경로에도 협상을 적용한다.
scanner 전용 소비자가 사라지면 별도 scan mark를 유지할 이유를 다시 본다.

## References

- [출력 읽기·보존·오류](../features/terminal-output/index.md)
- [API 호환 협상](../dev-guide/api-conventions.md)
- [이벤트 위치 조회](0633-event-feed-delivery.md)
