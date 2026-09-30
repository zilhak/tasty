# ADR-0063: 이벤트 저장소는 payload를 journal DB에 두고 파일 잠금과 writer 세대로 쓰기를 제한한다

- **Status**: Accepted — 구현 상태: 이 결정의 payload 저장, 독점 writer 잠금과 세대 검사, effect·명령 상태 전이, schema·파일 식별은 `tasty-event-store`에 구현됐다. 미이행: 제품 경로 연결(아직 어떤 크레이트도 이 저장소를 쓰지 않는다), 새 journal로 가져올 때의 payload 복사, 로그 보존·정리, projection 출력과 cursor의 동시 갱신
- **Date**: 2026-09-30
- **Tags**: event-sourcing, storage, sqlite, durability, effects, fencing
- **Group**: foundation

## Context

[ADR-0055](0055-structural-domain-event-sourcing.md)는 구조 도메인의 원본을 영속 이벤트로 정했고,
[ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md)은 저장 계약을 별도 crate `tasty-event-store`에 두기로 했다.
이 crate는 한 journal 파일(SQLite) 안에서 stream별 revision, 원자 batch, 명령 identity, effect 의무와 시도 기록,
domain snapshot, consumer checkpoint, 불변 payload를 제공한다. 제품 경로에는 아직 연결되지 않았다.

구현하면서 저장 형식과 보장 범위에 관한 선택 네 가지가 남았다.

- 큰 불변 내용(surface snapshot·scrollback 같은 payload)을 journal DB 안에 둘지 별도 파일로 둘지.
  계획 단계에서는 임시 파일 작성→sync→rename→부모 디렉터리 sync를 거친 파일 blob을 가정했다.
- 한 journal의 활성 writer를 무엇으로 하나로 제한할지. journal 메타데이터의 writer 세대(epoch)를 쓰기마다 검사하는 것만으로는
  같은 파일을 연 다른 프로세스가 언제든 새 세대를 등록할 수 있고, 그 순간부터 살아 있던 writer의 다음 쓰기가 Fenced 오류로 거절된다.
  writer 자리를 빼앗는 쪽은 아무 확인 없이 성공한다.
- effect와 명령 상태의 허용 전이. 특히 결과 불명(Uncertain) effect의 종료 방법, 재시도 뒤 이전 결과의 처리, 비종료 명령 상태의 역행 여부.
- 이 빌드보다 새 schema의 journal과 journal이 아닌 SQLite 파일을 열 때의 처리.

## Decision

### payload는 journal DB의 BLOB이다

- 불변 payload는 같은 journal DB의 표에 BLOB으로 저장하고 sha256을 함께 둔다. 읽을 때마다 checksum을 검증하며, 맞지 않으면 손상으로 알리고 내용을 돌려주지 않는다.
- payload는 한 번 쓰면 바꾸지 않는다. 같은 내용을 다시 넣어도 새 generation(새 참조)을 만든다. 갱신 API는 두지 않는다.
- 참조는 보유자 이름(pin)으로 건다. 이벤트·snapshot이 참조하는 payload는 그 기록을 commit하는 같은 transaction에서 pin한다.
  undo·View 복원 기록·import 같은 외부 보유자도 pin으로 참조한다. 삭제는 pin 해제이며 실제 행은 모든 pin이 풀린 payload만 지우는 GC가 지운다.
- 새로 넣은 payload는 참조가 생기기 전까지 GC 대상이다. 참조할 기록을 commit하기 전에 GC가 돌았으면 그 commit은 payload 없음으로 실패하고, 호출자가 다시 넣는다.
  확정된 참조가 가리키는 payload가 사라지는 경로는 없다.
- 새 journal로 가져오는 경우 payload는 대상 journal에 복사해 독립 소유하게 한 뒤 원본의 pin을 푼다. 다른 journal의 행을 가리키지 않는다.
- 이벤트 자체의 작은 payload는 이벤트 행에 inline BLOB으로 둔다. 불변 payload 표는 여러 기록이 참조하거나 크기가 큰 내용을 위한 것이다.

### writer는 독점 파일 잠금과 세대 검사를 함께 쓴다

- journal은 잠금 없이 열 수 있고 이 상태에서는 읽기만 한다. writer가 되려면 OS 독점 잠금을 얻은 뒤 새 세대를 등록한다.
  잠금은 기다리지 않고 시도하며, 다른 저장소(다른 프로세스 포함)가 쥐고 있으면 이미 사용 중이라는 오류로 실패하고 writer가 되지 않는다.
  잠금은 한 journal의 활성 writer를 하나로 제한하고, 같은 journal의 resume는 명시한 journal 선택과 이 잠금으로만 허용한다.
- 잠금 대상은 journal DB 파일이 아니라 그 옆의 `<journal 파일 경로>.writer-lock` 파일이다. Windows의 파일 잠금(LockFileEx)은 강제 잠금이라
  DB 파일 자체를 잠그면 같은 파일에 대한 SQLite 자신의 읽기·쓰기까지 막히기 때문이다. 세 플랫폼에서 같은 방식을 쓴다.
- 잠금 파일은 writer가 끝나도 지우지 않는다. 잠금을 놓는 사이에 파일을 지우면 다음 두 프로세스가 서로 다른 새 파일을 만들어 각각 잠금을 얻을 수 있기 때문이다.
  잠금은 writer를 놓거나 저장소를 닫을 때, 그리고 프로세스가 비정상 종료할 때 OS가 푼다.
- writer 세대(epoch) fencing은 잠금과 함께 유지한다. 모든 쓰기 API는 잠금 보유를 먼저 확인하고, 이어서 현재 세대를 확인한 transaction에서만 쓴다.
  잠금 없이 쓰는 것은 journal을 열 때의 WAL 전환(`PRAGMA journal_mode = WAL`), schema migration, journal ID 기록뿐이며, migration과 journal ID 기록은 SQLite의 쓰기 transaction으로 직렬화된다.
  한계: 두 프로세스가 빈 파일을 동시에 처음 열면 migration 버전 확인이 transaction 밖이라 한쪽 open이 오류로 실패할 수 있다. 파일이 손상되지는 않는다.
  잠금이 없으면 현재 세대 값을 알아도 쓰지 못한다. 잠금을 가진 저장소가 새 세대를 등록하면 이전 세대의 다음 쓰기부터 Fenced로 거절된다.
  세대 검사는 같은 저장소 안의 이전 writer·worker가 늦게 보낸 쓰기를 막는 장치이고, 잠금은 다른 저장소·프로세스를 막는 장치다. 한쪽이 다른 쪽을 대체하지 않는다.
- 잠금 파일을 만들 수 없거나 잠금을 쓸 수 없는 파일시스템·플랫폼에서는 writer가 되지 않고 잠금을 쓸 수 없다고 알린다. 세대 검사만으로 계속하지 않는다.

### effect 상태 전이

| 현재 | 허용 다음 상태 | 조건 |
|---|---|---|
| Pending | Running, Deferred, Cancelled, Superseded | Running은 activation claim 필요 |
| Deferred | Pending, Running, Cancelled, Superseded | 명시 활성화로만 진행 |
| Running | Succeeded, Failed, Uncertain | 결과를 낸 attempt가 현재 attempt여야 함 |
| Failed | Pending | 같은 effect identity의 다음 attempt로 재시도 |
| Uncertain | Succeeded, Failed, Cancelled | 대조 결과로만. Cancelled는 실행되지 않았다는 증거를 결과로 함께 기록할 때만 |
| Succeeded, Cancelled, Superseded | 없음 | 종료 |

- 상태와 resource generation은 모든 전이에서, attempt는 Running에서 벗어날 때 저장값과 대조한다(compare-and-set). 다르면 늦은 결과로 보고 거절한다.
- Running 진입은 activation claim(journal·엔진·surface·runtime epoch·activation generation)을 얻어야 하며, 같은 generation의 실행권은 effect 하나만 갖는다.
- Uncertain은 새 generation이 생겼다는 이유만으로 성공·취소로 바꾸지 않는다. 대조로 실제 실행 여부를 확인한 뒤 그 결과로만 벗어나며, 확인 전에는 원래 자원의 대조·정리 의무를 유지한다.
  Running에서 바로 Superseded로 가지 않는다. 결과를 모르는 시도를 대체된 것으로 덮지 않기 위해서다.
- Uncertain→Cancelled는 증거가 없으면 거절하고 effect를 Uncertain으로 둔다. 저장소는 증거가 있는지만 확인하며 내용의 타당성은 effect 종류별 대조가 판단한다.
- 결과는 attempt마다 보존한다. effect의 현재 결과는 가장 최근 attempt 또는 그 대조의 결과이며, 재시도(Failed→Pending)로 새 attempt를 시작하면 현재 결과를 비운다.
  attempt 기록에는 Running에서 벗어날 때의 상태와 결과를 남긴다. 복구기가 결과 유무로 상태를 추정하지 않도록 하기 위해서다.
- Uncertain에서 대조로 벗어나면(Succeeded·Failed·Cancelled) 대조한 상태와 결과를 해당 attempt 기록의 별도 칸(대조 상태·대조 결과)에 남긴다.
  Running 시점의 상태(Uncertain)와 결과는 덮어쓰지 않는다. 이후 재시도로 effect의 현재 결과가 비워져도 대조 결과는 attempt 기록에 남는다.

### 명령 상태 전이

- 명령 상태는 Accepted → InProgress → 종료(Completed·Failed·Cancelled)로만 진행한다. Accepted에서 바로 종료할 수 있지만 InProgress에서 Accepted로 역행하지 않는다.
  역행하는 갱신은 거절하며 그 갱신을 담은 commit 전체가 반영되지 않는다.
- 종료된 명령은 바꾸지 않는다. 같은 키의 재요청은 종료 기록을 돌려받는다.
- 진행 중 응답은 새 값으로 교체할 수 있고, 종료할 때 최종 응답을 확정한다.

### schema 버전과 파일 식별

- journal은 적용한 migration 버전을 버전 표에 한 줄씩 남긴다. migration은 뒤에 추가만 하고 배포한 항목은 고치지 않으며, 한 버전씩 별도 transaction으로 적용한다.
- 이 빌드가 아는 버전보다 새 journal은 열지 않고 schema가 너무 새롭다고 알린다. 읽기 전용으로 열거나 모르는 형식을 건너뛰며 읽지 않는다.
  새 journal을 읽지 못하는 옛 바이너리가 그 journal이나 그 상태에서 만든 export를 덮어쓰지 않게 하기 위해서다.
- 비어 있지 않은데 버전 표가 없는 SQLite 파일은 journal이 아니라고 거절한다. 이 판정은 WAL 전환·migration·journal 바인딩보다 먼저 하고 파일을 바꾸지 않는다.
  잘못된 경로로 받은 다른 DB(memory.db 등)를 journal로 바꾸지 않기 위해서다. 빈 파일과 새 경로만 새 journal로 만든다.
- 파일에 기록된 journal ID가 요청한 ID와 다르면 열지 않는다. 최근 파일이나 다른 단서로 대상을 추측하지 않는다.
- WAL, `synchronous=FULL`, foreign key 검사가 실제로 적용됐는지 되읽고, 하나라도 다르면 열지 않는다. 완화된 설정이나 in-memory로 대체하지 않는다.

## Consequences

payload와 이벤트가 같은 SQLite transaction에 있으므로, 파일 blob과 DB commit 사이에 종료돼 참조가 끊기거나 고아 파일이 남는 경우를 따로 다루지 않아도 된다.
파일 이름·부모 디렉터리 영속화를 플랫폼별로 검증할 필요도 없다. 대신 journal 파일이 payload 크기만큼 커지고, 큰 payload가 WAL과 checkpoint 비용을 늘린다.
GC로 지운 공간은 VACUUM 전까지 파일 크기로 돌아오지 않는다.

잠금과 세대 검사를 함께 쓰면 두 프로세스가 같은 journal에 번갈아 쓰는 경우와 같은 프로세스 안의 늦은 결과를 모두 막는다.
잠금을 지원하지 않는 환경(일부 네트워크 파일시스템 등)에서는 writer로 열 수 없으므로 데이터 홈 위치에 제약이 생긴다.
일부 NFS·FUSE 구성은 잠금을 로컬에서만 성공시켜 호스트 간 배타를 보장하지 않는다.
journal마다 잠금 파일 하나가 옆에 남는다. 이 파일을 지우는 정리 작업은 실행 중인 writer가 없음을 확인한 뒤에만 한다.

전이표를 고정하면 복구기가 상태 이름만으로 다음 동작을 정할 수 있다. Uncertain에서 Cancelled로 가려면 실행되지 않았다는 증거가 필요하므로
대조 수단이 없는 effect는 Uncertain으로 오래 남을 수 있다. 이는 결과를 아는 척하지 않기 위해 감수한다.

로그 보존·정리는 아직 설계하지 않았다. 현재 V1 schema는 batch를 참조하는 외래 키, 앞 구간 전체를 가정하는 cut 계산,
유효한 snapshot이 없으면 처음부터 읽는 재구성을 전제로 한다. 오래된 구간을 지우려면 보존 경계 표와 schema migration이 필요하다.
projection 출력과 cursor를 같은 transaction으로 갱신하는 API와 stream별 부분 소비자의 위치 표현도 아직 없다. 현재 checkpoint는 batch 단위로 따로 저장한다.
두 항목은 후속 설계 대상이며 이 결정이 해결하지 않는다.

schema 이름 기반 식별은 같은 이름의 버전 표를 가진 다른 앱 DB를 걸러내지 못한다. 이 저장소의 다른 DB는 그 표를 쓰지 않는다.

## Alternatives Considered

- payload를 파일 blob으로 두는 안: DB 크기와 WAL 부담은 줄지만 파일 작성·sync·rename·디렉터리 sync와 DB commit의 순서, 고아 파일 GC, 플랫폼별 영속성 검증이 필요하다.
  현재 payload 크기 분포를 측정하기 전에는 이 복잡도를 감수할 근거가 없다.
- 세대 검사만 쓰는 안: 다른 프로세스가 같은 파일을 열어 아무 확인 없이 새 세대를 등록할 수 있고, 살아 있던 writer는 다음 쓰기부터 Fenced로 거절된다. 두 인스턴스가 같은 데이터 홈을 쓰는 실수를 막지 못한다.
- journal DB 파일 자체를 잠그는 안: Windows에서는 강제 잠금이라 SQLite가 같은 파일을 읽고 쓰지 못한다. 플랫폼별로 잠금 대상을 달리하면 동작 확인 범위가 늘어난다.
- 파일 잠금만 쓰는 안: 같은 프로세스 안에서 이전 writer·worker가 늦게 보낸 결과를 막지 못한다.
- Uncertain을 새 generation 등장 시 Superseded로 닫는 안: 실제로 실행된 자원의 정리 의무를 잃는다.
- 재시도 때 이전 결과를 effect에 남겨 두는 안: 새 attempt가 진행 중인데 이전 실패 결과가 보여 복구기가 오독할 수 있다.
- 새 schema journal을 읽기 전용으로 여는 안: 모르는 이벤트 형식을 해석할 수 없어 조용히 건너뛰게 되고, 옛 바이너리가 만든 결과가 최신 상태처럼 보인다.
- 다른 SQLite 파일도 journal로 초기화하는 안: 잘못된 경로 하나로 사용자 데이터 파일에 journal 표가 생기고 WAL로 바뀐다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- 제품에 연결할 때 모든 쓰기 경로가 잠금을 얻은 writer를 거치는지 확인한다. 잠금 없이 쓰는 경로가 생기면 연결하지 않는다.
- 로그 보존·정리를 설계하면 외래 키, cut 계산, snapshot+tail 재구성의 보존 경계를 함께 바꾸고 이 ADR의 schema 절을 다시 본다.
- projection lane에서 출력과 cursor의 동시 갱신 API를 설계하면 checkpoint 키 형태(batch 단위 또는 stream별)를 이 결정과 대조한다.
- 여러 호스트나 여러 프로세스가 같은 journal에 써야 하는 요구가 생기면 잠금·세대 모델을 다시 정한다.

### 실행 결과로 확인

- payload 크기 분포와 journal 파일 크기, WAL checkpoint 시간, 구조 명령 commit 지연을 측정한다. 큰 payload가 commit 지연이나 디스크 사용을 눈에 띄게 늘리면 크기 기준으로 파일 blob을 도입한다.
- Uncertain effect가 대조 수단 없이 오래 남는 사례가 쌓이면 effect 종류별 대조 방법을 추가한다.

## References

- [ADR-0055](0055-structural-domain-event-sourcing.md) · [ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md) · [ADR-0057](0057-command-identity-for-mutation-retries.md) · [ADR-0010](0010-storage-failure-reporting.md)
- 현재 구조: [아키텍처](../architecture/index.md)의 도메인-IO 절
- 현재 구현: `crates/tasty-event-store/src/store.rs`, `crates/tasty-event-store/src/schema.rs`, `crates/tasty-event-store/src/payload.rs`, `crates/tasty-event-store/src/effect.rs`, `crates/tasty-event-store/src/command.rs`.
