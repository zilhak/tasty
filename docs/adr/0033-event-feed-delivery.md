# ADR-0033: 이벤트 피드는 짧게 보관하고 소비자가 읽은 위치를 관리한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: plugins, runtime
- **Group**: plugins

## Context

실시간 구독만 있으면 늦게 접속한 소비자는 지나간 사건과 그 유실을 알 수 없다.
그렇다고 소비자마다 호스트에 읽은 위치와 수신 확인 상태를 두면 느리거나 사라진 소비자의 관리가 서버에 남는다.
필요한 것은 짧은 메모리 보존과 유실을 드러내는 조회 계약이다.

## Decision

debug와 release는 같은 메모리 ring을 사용한다.
각 사건에 증가하는 offset을 부여하고 오래된 사건을 지워도 번호를 재사용하지 않는다.
보존은 개수와 JSON 직렬화 바이트 제한을 함께 적용한다. 최신 사건 하나는 읽을 기회를 주기 위해 크기와 무관하게 남긴다.
디스크 journal과 재전송 보장은 제공하지 않는다.

events.fetch는 소비자가 가져온 위치로 읽는 Local 전용 Read다.
서버는 소비자 cursor를 보관하거나 전진시키지 않는다. Plugin은 기존 subscription을 쓴다.
보존 밖에는 truncated·skipped, 끝보다 큰 위치에는 ahead_of_stream·stream_end를 답한다.
기존 소비자와의 호환을 위해 미래 위치도 요청한 대기 시간과 next_offset 의미를 유지한다.

소비자는 offset과 epoch를 함께 보존한다.
CLI follow는 연결마다 처음에 즉시 조회하고 세대 변경이나 미래 위치를 알린 뒤 처음부터 다시 읽는다.
자동 재연결은 선택 사항이며 사건은 stdout, 유실·연결 알림은 stderr로 나눈다.

agent 사건은 이미 모든 경로가 거치는 task 종결과 barrier 닫힘 지점에서 발행한다.
비종결 전이나 조회 때 평가하는 lease 만료를 완전한 사건 흐름으로 약속하지 않는다.
실패 상세·명령 출력은 event payload에 넣지 않고 해당 조회 API로 읽는다.
Experimental 등급은 호환 경고이며 추가 구독 승인 조건은 아니다.

plugin 재발행은 미응답 event.dispatch가 있는 동안 호스트가 hop 하한을 올린다.
판정은 publish 도착 시점에 하며 plugin은 dispatch에 응답해야 한다.
이는 callback 안의 반응을 제한하는 규칙이고 모든 비동기 루프의 완전한 방지는 아니다.

## Consequences

여러 소비자가 서로의 읽기 위치를 바꾸지 않고 유실을 확인할 수 있다.
재시작하면 기록을 잃으며 epoch 없는 재부착은 옛 offset이 새 범위 안에 들어간 경우를 구별하지 못한다.
같은 요청을 반복해도 서버의 읽기 위치를 바꾸지는 않지만 사건 생성·보존 만료 때문에 답은 달라질 수 있다.

최신 단일 대형 사건과 JSON 메모리 표현 때문에 바이트 제한을 RSS 상한으로 볼 수 없다.
대기 요청 하나는 스레드를 차지하며 개별 timeout이 동시 요청 수까지 제한하지는 않는다.
먼저 응답한 뒤 발행하는 plugin은 hop 하한을 벗어나며 무관한 publish가 하한을 적용받을 수도 있다.
피드는 사용자 키·마우스 원문을 재생하거나 화면 상태를 복원하는 통로로 쓰지 않는다.

## Alternatives Considered

- 조용히 보존 시작점부터 주면 소비자는 유실을 알아채지 못한다.
- 서버가 읽기 위치를 관리하면 소비자 등록·해제와 상태 회수도 필요하다.
- 미래 위치에 즉답하면 그 표지를 모르는 client가 빠른 재시도 루프를 만들 수 있다.
- 개수만 제한하면 큰 payload가 호스트 메모리를 오래 점유한다.
- 시간 창이나 trace 자기 신고만으로 재발행을 제한하면 오탐이나 우회가 남는다.
- Experimental opt-in을 뒤늦게 강제하면 현재 구독자 동작이 달라진다.

## Reconsideration Triggers

실제 skipped 비율과 큰 사건 분포가 보존 요구에 맞지 않으면 상한을 조정한다.
영속 journal·낮은 지연 push·서버측 epoch 검사가 필요하면 기존 조회 의미를 보존하며 비교한다.
비종결 전이가 공통 처리 지점을 갖거나 lease 만료가 명시 전이가 되면 사건 범위를 다시 본다.
SDK 응답 순서와 pump 순서가 바뀌거나 비동기 재발행 루프가 발생하면 hop 정책을 재검토한다.

## References

- [Event Bus 계약과 카탈로그](../reference/event-catalog.md)
- [플러그인 이벤트 사용](../dev-guide/plugin-development.md)
- [터미널 출력의 위치 조회](0034-output-cursor-contract.md)
