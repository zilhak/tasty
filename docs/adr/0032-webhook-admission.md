# ADR-0032: Webhook은 정해진 작업을 접수하고 고정 응답을 보낸다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: plugins, runtime
- **Group**: plugins

## Context

외부 HTTP 발신자는 로컬 IPC 호출자와 신뢰 범위가 다르다.
발신자가 실행할 메서드와 응답 데이터를 고르게 하면 webhook이 원격 명령·조회 API가 된다.
또 인증에 실패하는 요청도 HTTP parser와 body 읽기에 자원을 사용한다.

application body 길이만 제한해서는 충분하지 않았다.
tiny_http는 응답 뒤 요청을 정리할 때 읽지 않은 Content-Length body를 끝까지 소비했고 큰 임시 할당도 만들었다.

## Decision

owner가 등록한 IPC 시퀀스가 흐름을 정하고 외부 payload는 params의 값만 채운다.
method·객체 key·순서를 치환하지 않는다. ShellCommand는 webhook에 직접 바인딩할 수 없다.
HTTP 응답은 실행 결과와 분리한 고정 ACK이며 실행 전에 보낸다.
owner가 선택한 IPC 자체의 영향까지 안전하다고 보장하지는 않는다.

HTTP는 blocking tiny_http를 쓰며 TLS는 외부 종단에 맡긴다.
선택적 공유 토큰과 비순차 URL, 출처 IP별 남용 제한을 사용한다.
인증 실패는 등록의 실행 횟수를 차감하지 않지만 출처 실패에는 포함한다.
차단 확인은 application body 읽기 전에 수행하고 통과한 요청만 제한된 parser에 전달한다.

body는 기본 1MiB로 제한한다. 선언 길이와 실제 읽기 모두 검사하고 초과는 413으로 끝낸다.
413과 선차단 429는 vendor의 respond_and_close로 잔여 body를 읽지 않고 연결을 종료한다.
거절 연결을 재사용하지 않는 대가를 받아들여 drain 대기와 길이 비례 할당을 없앤다.

남용 제한은 IP의 고정 창 실패 수와 고정 cooldown으로 처리한다.
차단 중 요청이 cooldown을 연장하지 않고 만료 때 카운터를 초기화한다.
출처 표의 정리 기준은 메모리 상한이 아니며 유효한 차단 항목을 강제로 밀어내지 않는다.

## Consequences

발신자는 미리 등록한 작업만 촉발하고 ACK에서 내부 실행 결과를 읽을 수 없다.
200은 접수이며 실제 실행 실패·부분 적용은 호스트 로그로 확인한다.
인증 없는 URL에 도달한 사람은 누구나 등록 작업을 촉발할 수 있고 HMAC은 지원하지 않는다.

본문 상한은 전체 RSS나 동시 연결 수 상한이 아니다. 작은 body의 라이브러리 선읽기와 idle 연결 제한도 남는다.
IP 공유 때문에 한 발신자의 실패가 NAT 이웃을 막을 수 있고 분산 출처나 고정 창 미만 반복까지 일정 비율로 묶지는 못한다.
거절 연결의 RST 때문에 client가 ACK를 읽지 못할 수도 있다.

vendor 사본은 상류 보안 수정이 자동 반영되지 않는다.
기존 SSE 소비자와 정상 keep-alive를 유지하도록 패치 범위를 요청별 종료 API에 한정한다.

## Alternatives Considered

- payload가 메서드를 고르면 값 전달만 허용한다는 경계가 사라진다.
- 인증 실패로 등록 횟수를 차감하면 토큰을 모르는 발신자가 owner의 예산을 소모한다.
- Content-Length만 검사하면 길이가 없는 chunked를 놓친다.
- Connection:close 헤더만 추가해도 기존 reader의 Drop drain은 사라지지 않는다.
- 모든 요청의 drain을 없애면 다른 소비자의 정상 연결 재사용까지 바뀐다.
- 현재 문제만으로 HTTP 스택을 전면 교체하면 SSE와 parser 동작을 모두 다시 검증해야 한다.

## Reconsideration Triggers

정상 payload가 자주 상한을 넘거나 NAT 오탐·출처 표 메모리 압박이 나타나면 해당 요청 분포를 확인한다.
IPv6·신뢰 proxy 헤더·서명 인증·자체 TLS를 도입할 때 출처와 신뢰 정책을 다시 정한다.
상류가 요청별 폐기 API를 제공하거나 vendor를 갱신할 때는 응답 수신과 worker·소켓 종료를 따로 확인하고 SSE도 검사한다.
조회형 응답이나 직접 OS action이 필요하면 통지 전용 계약을 별도로 재설계한다.

## References

- [Webhook 처리·상한·운영](../features/webhook/index.md)
- [공유 hook handler](../features/hooks/index.md)
- [tiny_http 패치 목록](../../vendor/tiny_http/PATCHES.md)
- [의존성 관리](../dev-guide/dep-issues.md)
