# ADR-0609: 사용 기록과 진단 로그는 수명에 맞춰 저장한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: storage, retention, state
- **Group**: foundation

## Context

최근 파일과 튜토리얼 진행은 사용자 사용 기록이고 에이전트 메모리와는 수명이 다르다.
창마다 최근 목록을 따로 캐시하면 같은 DB를 쓰면서도 조회 결과가 달라진다.
정상 IPC마다 로그를 무한 저장하면 사용 기록보다 진단 데이터가 저장 공간을 차지하게 된다.

## Decision

state.db는 GUI 부팅이 열고 사용자 사용 기록을 저장한다.
`with_state_db`의 None은 현재 열려 있지 않다는 상태만 말하며 원인을 추정하는 값으로 쓰지 않는다.
헤드리스와 GUI 초기화 실패 모두 빈 최근 목록을 보일 수 있다.

최근 파일 캐시는 Db가 한 벌 소유하고 모든 창이 공유한다.
기록은 캐시 lock 안에서 메모리 변경과 DB 저장을 순서대로 수행한다.
DB 저장 실패에도 세션의 목록은 모든 창에서 같은 상태를 유지한다.

튜토리얼은 주제·콘텐츠 revision·완료·재개 단계·row version을 별도 테이블에 저장한다.
오래된 View의 재개 위치 덮어쓰기는 버전 검사로 막고 완료 상태는 되돌리지 않는다.
런타임 객체 ID는 저장하지 않아 실습 재개는 준비부터 수행한다.

audit·telemetry event·anomaly는 공용 보존 정책으로 정리한다.
audit는 Deny만 영속하며 raw telemetry의 별도 rollup은 현재 만들지 않는다.
부팅, append와 주기 timer가 같은 정리 함수와 주기 제한을 사용한다.

출력 observer의 memory 레코드는 시각과 sink 순번을 함께 키에 넣는다.
같은 밀리초에 여러 항목이 나와도 서로 덮어쓰지 않고 최근 N개 정리가 최신 항목을 지우지 않게 한다.

## Consequences

창이 달라도 최근 목록이 일치하고 튜토리얼 진행이 레이아웃 설정이나 runtime ID에 종속되지 않는다.
로그는 무한히 누적되지 않으며 동일 시각의 observer 결과도 구분된다.

Allow 행동 감사는 남지 않아 사고 뒤 모든 정상 호출을 복원할 수 없다.
telemetry 조회 기간은 최근 이벤트 수와 유입량에 따라 달라진다.
정리는 주기적으로 실행하므로 순간 사용량은 정책 상한보다 클 수 있다.
최근 캐시는 다른 프로세스의 직접 DB 변경을 감지하지 않는다.

## Alternatives Considered

- 최근 목록을 매번 DB에서 읽으면 동일 timestamp의 즉시 순서와 읽기 부수효과 문제가 생긴다.
- 튜토리얼을 config나 layout에 넣으면 사용자 선호 또는 창 복원 수명과 섞인다.
- allow 폴링 제외 목록은 새 polling 메서드를 계속 관리해야 한다.
- raw event rollup은 장기 조회 의미를 다시 설계해야 하므로 현재 보류한다.
- observer timestamp만 쓰면 같은 밀리초의 결과가 충돌한다.

## Reconsideration Triggers

외부 프로세스와 최근 목록을 공유하거나 실습 객체까지 복원해야 하면 저장 범위를 재검토한다.
정식 schema migration과 외부 튜토리얼 기여가 도입되면 additive ensure와 revision 소유권을 정리한다.

장기 비용 분석, 사고 후 행동 감사, 자동 anomaly 대응이 필요해지면 로그 보존을 다시 설계한다.
observer의 매우 큰 동일-ms 유입이나 시계 역행·재시작 key 충돌이 보고되면 key 형식도 검토한다.

## References

- [저장소 시스템](../design/systems/storage.md)
- [튜토리얼](../features/tutorial/index.md)
- 구현: `src/db.rs`, `src/store/recent_files.rs`, `src/store/tutorial_progress.rs`, `src/store/log_retention.rs`.
