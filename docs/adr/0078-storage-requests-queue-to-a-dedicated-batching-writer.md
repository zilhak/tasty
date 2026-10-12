# ADR-0078: 저장 요청은 제한된 큐에 접수하고 저장소를 소유한 전담 writer가 묶어 실행한다

- **Status**: Accepted — memory.db writer는 부팅·종료에 연결됐고 소비자를 차례로 옮기는 중이다(현재 소비자는 [저장 계약](../design/systems/storage-contract.md#소비자-분류)에 있다). state DB·파일 writer는 이후 단계다.
- **Date**: 2026-10-12
- **Tags**: storage, sqlite, concurrency, backpressure, batching
- **Group**: foundation

## Context

memory.db의 소비자(메인 루프, observer sink, runner, 승인 대기, 결과 스레드)는 공유 `Mutex`를 잡고 쓰기마다 commit했다. 저장이 느려지면 같은 잠금을 쓰는 메인 루프와 다른 호출자가 그 시간을 그대로 기다렸고, 요청 100건이 몰리면 commit도 100회였다([저장 기준선](../design/systems/storage.md#저장-기준선)). 실행 단위·쓰기 묶음으로 여러 쓰기를 transaction 하나에 담을 수 있게 됐지만, 서로 다른 호출자의 요청을 모으려면 누군가 시간을 정해 모아야 한다.

조건:
- 호출 스레드는 디스크·DB 잠금·완료를 기다리지 않고 진행해야 한다.
- 메모리 사용에 상한이 있어야 하고, 넘치면 호출자가 알 수 있어야 한다.
- 요청 순서와 강한 읽기, 종료 때의 마지막 반영을 보장해야 한다.
- 실패는 [ADR-0010](0010-storage-failure-reporting.md)의 분류를 요청마다 전달해야 한다.

## Decision

저장소마다 그 저장소를 소유한 전담 worker 하나를 두고, 호출자는 제한된 큐에 요청을 접수한 뒤 receipt를 받아 바로 돌아간다. worker는 시간·건수·바이트 한도 안에서 요청을 모아 묶음 하나를 transaction 하나로 실행하고 요청마다 결과를 돌려준다. 접수·수집·순서·예산·재시도·종료 규칙은 저장소와 관계없는 공용 실행기(`tasty_utils::batch_writer`)가 정하고, 저장 방법은 저장소별 실행기(memory.db는 `tasty_memory::writer`)가 정한다. 현재 규칙과 한도는 [저장 실행기](../design/systems/storage-contract.md#저장-실행기)에 있다.

- 예산은 접수부터 결과 소비(또는 receipt drop)까지 잡힌 요청 수와 바이트로 센다. 차면 접수를 거절하며 호출 스레드에서 대신 실행하지 않는다.
- 강한 읽기와 flush는 접수 순서에 들어가며, 그 앞의 요청이 모두 끝난 뒤 실행된다.
- 읽기-판단-쓰기는 호출자가 나눠 보내지 않고 writer 안에서 한 요청으로 실행한다. 요청은 빌린 상태나 잠금 guard를 실을 수 없다.
- busy는 같은 순서 자리에서 시한 안에 재시도하고, 확정 여부를 알 수 없는 실패는 따로 알린다.
- 종료는 그 저장소에 쓰는 producer를 모두 멈춘 뒤 접수 중단 → 마지막 flush → join 순서로 하며 시한을 넘기면 경고 후 진행한다.
- 이행 중에는 writer도 기존 공유 잠금으로 저장소에 닿아 직접 잠금 소비자와 직렬화된다(`SharedStore`).
- 공유 잠금을 직접 잡는 기존 코드(`TaskService`)는 호환 구간 요청(`MemoryRequest::Section`)으로 writer 순서에 넣는다. 구간은 앞뒤 요청과 묶이지 않고(barrier), worker는 구간 동안 저장소 잠금을 잡지 않으며 본문을 한 번만 실행한다. 구간 안의 쓰기는 각자 commit하고, 본문이 panic하면 확정 여부를 모른다고 알린다.

## Consequences

호출 스레드의 저장 대기가 접수 시간(측정 p99 7~703µs)으로 줄고, 동시에 몰린 요청은 commit 하나로 묶인다. 느린 저장에서 동시 100건의 지연이 수백 ms(직접 잠금 요청 p50 270~295ms)에서 십여 ms(완료 p50 7.1~12.4ms, p99 8.2~13.9ms)로 줄었다([저장 실행기 측정](../design/systems/storage.md#저장-실행기-측정)).

완료는 최대 수집 창만큼 늦어진다. 결과를 기다리는 호출자는 그 시간을 감수한다. 예산이 차면 쓰기가 거절되므로 호출자는 거절을 처리해야 한다. 묶음의 저장 실패 하나가 같은 묶음의 다른 요청까지 실패시킨다.

이행 기간에는 writer와 직접 잠금 소비자가 같은 잠금을 다투며, 느린 저장에서 직접 잠금 쪽의 대기가 크게 늘 수 있다(측정에서 p99 최대 376ms).

## Alternatives Considered

- 호출 스레드에서 각자 실행 단위를 열기: 서로 다른 호출자의 쓰기를 묶을 수 없고 잠금 대기가 남는다.
- 큐가 차면 호출 스레드에서 직접 실행(caller-runs): 상한을 넘는 부하에서 메인 루프가 다시 디스크를 기다린다. 거절을 명시하는 쪽을 택했다.
- 무제한 큐: 저장이 멈추면 메모리가 끝없이 는다.
- 공용 채널 하나에 완료를 모으기: 소비가 늦으면 채널이 차 worker가 멈춘다. 요청마다 결과 칸을 두고 예산으로 상한을 잡았다.
- 저장소마다 실행기를 따로 구현: 순서·예산·종료 규칙이 저장소마다 갈린다.
- 새 crate: 공용 실행기는 이미 모든 사용처가 의존하는 leaf인 `tasty-utils`에 두면 새 의존이 생기지 않는다.

## Reconsideration Triggers

- 소비자를 writer로 옮긴 뒤에도 메인 루프나 다른 호출자의 저장 대기가 줄지 않으면 수집 창·잠금 구조를 다시 본다. `system.pressure.memory_writer`와 harness의 대리 루프 지연으로 확인한다.
- 예산 거절(`rejected_saturated`)이 정상 사용에서 관측되면 한도나 거절 처리를 다시 정한다.
- 직접 잠금 소비자가 모두 writer로 옮겨지면 `SharedStore` 대신 worker가 연결을 소유하는 구성으로 바꾼다.
- 호환 구간을 쓰는 코드가 writer 요청으로 옮겨지면 구간 요청을 없앤다. 구간은 다시 실행하지 않으므로 그 전에 busy 재시도가 필요한 소비자가 생기면 구간 대신 요청으로 옮긴다.
- 완료 지연이 사용자 조작 응답에 보이면 수집 창 기본값(2ms)을 다시 잰다.

## References

- [저장 계약과 writer 명부 — 저장 실행기](../design/systems/storage-contract.md#저장-실행기)
- [저장 실행기 측정](../design/systems/storage.md#저장-실행기-측정)
- [ADR-0010](0010-storage-failure-reporting.md) — 묶음의 저장 실패 분류.
- 구현: `crates/tasty-utils/src/batch_writer.rs`, `crates/tasty-memory/src/writer.rs`, `src/app/services.rs`, `src/app/shutdown_machine.rs`.
