# ADR-0608: 요청 압력은 프로세스 단위의 제한된 진단 정보로 제공한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: ipc, telemetry, diagnostics
- **Group**: foundation

## Context

응답이 늦다는 사실만으로는 연결 포화, 큐 적체, handler 실행과 plugin 대기를 구분할 수 없다.
기존 caller별 telemetry에 요청별 상세 로그를 추가하면 진단 때문에 DB와 메모리 부하가 커진다.
누계 집계만으로는 특정 느린 요청의 단계별 원인도 알기 어렵다.

## Decision

진단은 메모리의 제한된 프로세스 집계로 유지하고 `system.pressure`와 CLI 한 곳에서 읽는다.
다른 caller의 부하가 포함되므로 Local 전용이다.
큐 입장·게이트 전 대기·게이트 거절·handler·plugin 왕복·DB·재시도는 측정 대상별 객체로 구분한다.
각 snapshot의 이름과 의미를 응답에서도 유지한다.

연결 정보는 요청 없는 stream까지 포함한다.
accept 대기는 실제 도착 시각 대신 마지막 빈 큐 관측부터 계산한 상한임을 이름으로 밝힌다.
시간 분포는 고정 경계와 비누적 bucket 수를 함께 내고 정밀한 분위수를 만들어내지 않는다.
평균은 합계를 올린 같은 지점의 관측 횟수로 나눈다. 관측이나 저장소가 없으면 null로 구분한다.

호스트는 요청별 RequestSeq를 발급하고 느린 요청만 고정 크기 링에 남긴다.
큐·호스트·plugin hop을 같은 번호로 연결하며 실제 대기자가 받은 답을 공유 결과 칸에서 읽는다.
RPC ID와 event trace를 요청 번호로 재사용하지 않고, 알 수 없는 plugin 부모 관계를 추측하지 않는다.
params·토큰·멱등 키 원문은 저장하지 않는다.

재시도 누계는 판정하는 Store에서 센다. 처리하지 않는 층의 임시 집계는 되돌린다.
caller별 cap 입력과 Allow 관측의 기존 의미는 바꾸지 않는다.
전역 진단을 얻으려고 거절 요청을 caller 사용량에 더하지 않는다.

## Consequences

운영자가 같은 응답에서 지연 원인을 비교하고 최근 느린 요청을 추적할 수 있다.
기록량은 요청 수에 비례해 무한히 늘지 않는다.

전체 snapshot은 원자적이지 않고 프로세스 누계는 오래된 최대값을 포함한다.
DB pragma는 초기화 상태여서 누계와 성격이 다르다.
결과 불명 뒤 계속되는 작업, 큐를 안 지난 plugin host-call과 일부 intent 이후 forward는 완전히 추적되지 않는다.
링은 재시작 시 사라지며 method 이름도 길이 제한으로 잘릴 수 있다.

## Alternatives Considered

- caller별 영속 이벤트에 모든 지연을 기록하면 진단이 저장 부하를 만든다.
- 서로 다른 집계를 한 객체로 합치면 두 수의 차이를 거절 수로 오해하기 쉽다.
- 정확한 p99처럼 보이는 값은 histogram의 해상도와 넘침 범위를 숨긴다.
- 전체 요청이나 가장 느린 요청을 영구 보관하면 현재 원인을 찾기 어렵고 크기 제한도 복잡해진다.
- 별도 진단 메서드를 계속 만들면 호출 사이 상태가 달라져 비교가 더 어려워진다.

## Reconsideration Triggers

정상 사용에서 거절이 발생하거나 histogram 양 끝에 관측이 몰리면 상한과 경계를 재검토한다.
링이 빠르게 덮이거나 결과 null이 오래 남으면 기록 연결과 기준값을 확인한다.

새 게이트·대기자·plugin hop·producer는 대응 집계와 결과 기록을 추가한다.
caller별 자기 진단이나 재시작을 넘는 분석이 필요해지면 권한·보존 정책을 별도로 정한다.
계측 함수의 단위 테스트와 실제 요청에 따른 값 증가를 구분해 검증한다.

## References

- [IPC 압력 진단의 필드 의미](../architecture/ipc-server.md#요청-압력-게이지)
- [텔레메트리 기능](../features/telemetry/index.md)
- 구현: `crates/tasty-telemetry/src/pressure.rs`, `crates/tasty-telemetry/src/slow_requests.rs`, `src/adapters/ipc/handler/pressure.rs`.
