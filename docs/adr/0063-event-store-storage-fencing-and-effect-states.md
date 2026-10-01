# ADR-0063: 이벤트 저장소는 payload를 journal DB에 두고 파일 잠금과 writer 세대로 쓰기를 제한한다

- **Status**: Accepted — 구현 상태: payload 저장, 독점 writer 잠금과 세대 검사, effect·명령 상태 전이, schema·파일 식별, 영속 ID 예약 및 projection 출력/consumer 위치 원자 확정은 `tasty-event-store`에 구현됐다. App 초기 엔진 구성·선택 slot import·자원 준비는 데이터 홈 worker에 연결 중이다. 기존 숫자 surface metadata를 피하는 예약 기준도 이 worker에서 영속 반영한다. 저장 leaf는 scoped consumer 위치와 신규 admission의 활성 저장량 예산을 제공한다. 일반 writer·Recovery·ID 소비의 제품 끝점과 전체 실행 검증은 별도로 대조한다. 정상 슬롯 resume와 폐기 뒤 incarnation 전환은 App bootstrap·retirement에 연결돼 있다.
- **Date**: 2026-09-30
- **Tags**: event-sourcing, storage, sqlite, durability, effects, fencing
- **Group**: foundation

## Context

[ADR-0055](0055-structural-domain-event-sourcing.md)는 구조 도메인의 원본을 영속 이벤트로 정했고,
[ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md)은 저장 계약을 별도 crate `tasty-event-store`에 두기로 했다.
이 crate는 한 journal 파일(SQLite) 안에서 stream별 revision, 원자 batch, 명령 identity, effect 의무와 시도 기록,
domain snapshot, consumer checkpoint, 불변 payload를 제공한다. 이 문단은 저장소를 처음 도입하던 시점의 배경이며, 현재 제품 연결 범위는 Status와 아래 결정 절에서 구분한다.

구현하면서 저장 형식과 보장 범위에 관한 선택 다섯 가지가 남았다.

- 큰 불변 내용(surface snapshot·scrollback 같은 payload)을 journal DB 안에 둘지 별도 파일로 둘지.
  계획 단계에서는 임시 파일 작성→sync→rename→부모 디렉터리 sync를 거친 파일 blob을 가정했다.
- 한 journal의 활성 writer를 무엇으로 하나로 제한할지. journal 메타데이터의 writer 세대(epoch)를 쓰기마다 검사하는 것만으로는
  같은 파일을 연 다른 프로세스가 언제든 새 세대를 등록할 수 있고, 그 순간부터 살아 있던 writer의 다음 쓰기가 Fenced 오류로 거절된다.
  writer 자리를 빼앗는 쪽은 아무 확인 없이 성공한다.
- effect와 명령 상태의 허용 전이. 특히 결과 불명(Uncertain) effect의 종료 방법, 재시도 뒤 이전 결과의 처리, 비종료 명령 상태의 역행 여부.
- 이 빌드보다 새 schema의 journal과 journal이 아닌 SQLite 파일을 열 때의 처리.
- [ADR-0055](0055-structural-domain-event-sourcing.md)가 정한 재사용하지 않는 typed ID의 영속 예약을 저장소에서 어떤 형태로 보장할지.
  예약 ID는 `decide`의 고정 입력이므로 그 ID를 쓰는 이벤트 commit보다 먼저 정해져야 한다.

## Decision

### payload는 journal DB의 BLOB이다

- 불변 payload는 같은 journal DB의 표에 BLOB으로 저장하고 sha256을 함께 둔다. 읽을 때마다 checksum을 검증하며, 맞지 않으면 손상으로 알리고 내용을 돌려주지 않는다.
- payload는 한 번 쓰면 바꾸지 않는다. 같은 내용을 다시 넣어도 새 generation(새 참조)을 만든다. 갱신 API는 두지 않는다.
- 참조는 보유자 이름(pin)으로 건다. 이벤트·snapshot이 참조하는 payload는 그 기록을 commit하는 같은 transaction에서 pin한다.
  undo·View 복원 기록·import 같은 외부 보유자도 pin으로 참조한다. 삭제는 pin 해제이며 실제 행은 모든 pin이 풀린 payload만 지우는 GC가 지운다.
- 새로 넣은 payload는 참조가 생기기 전까지 GC 대상이다. 참조할 기록을 commit하기 전에 GC가 돌았으면 그 commit은 payload 없음으로 실패하고, 호출자가 다시 넣는다.
  확정된 참조가 가리키는 payload가 사라지는 경로는 없다.
- 새 journal로 가져오는 경우 payload는 대상 journal에 복사해 독립 소유하게 한 뒤 원본의 pin을 푼다. 다른 journal의 행을 가리키지 않는다.
  내부 import는 전송 ID로 source snapshot/View alias를 고정한다. destination의 새 ID mapping·초기 events·
  최초 응답·domain snapshot·View manifest를 같은 transaction에 확정한 뒤 source alias를 해제한다.
  실패한 준비 payload는 admission holder로 보존하며 다음 fenced writer가 미수락 잔여 pin을 정리한다.
  destination commit 뒤 응답 유실은 같은 전송 키 조회로 합류하고 source의 command/effect/cleanup을 복사하지 않는다.
- 이벤트 자체의 작은 payload는 이벤트 행에 inline BLOB으로 둔다. 불변 payload 표는 여러 기록이 참조하거나 크기가 큰 내용을 위한 것이다.

### View 복원 manifest

View checkpoint 바이트는 domain snapshot의 cache가 아니라 별도 원본이다. slot key와
incarnation/runtime epoch/sequence, domain snapshot ID, View payload를 같은 journal의
restore manifest에 둔다. 새 snapshot과 manifest 및 참조 pin은 한 transaction으로 저장한다.
이전 sequence·incarnation의 늦은 저장은 새 record를 덮지 못하며, 같은 sequence의 다른
내용도 거절한다. old retirement는 일치하는 incarnation의 manifest만 지운다.

기존 sidecar는 DB manifest가 없을 때 최초 이관 자료로 읽는다. DB source가 생긴 뒤에는
손상된 DB를 오래된 sidecar로 덮는 fallback을 하지 않는다. 새 incarnation을 선택한 시작은
옛 incarnation의 View 바이트에 의존하지 않는다. manifest가 참조한 snapshot의 외부 pin은
compaction 하한을 제한한다. View 기록을 도메인 이벤트에 넣는 안은 사용자 선택을 replay로
다시 만들어 원본 의미가 달라지므로 사용하지 않는다.

### writer는 독점 파일 잠금과 세대 검사를 함께 쓴다

- journal은 잠금 없이 열 수 있고 이 상태에서는 읽기만 한다. writer가 되려면 OS 독점 잠금을 얻은 뒤 새 세대를 등록한다.
  잠금은 최대 2초 동안 간격을 늘려 가며(10ms에서 두 배씩, 최대 100ms) 다시 시도하고, 그래도 다른 저장소(다른 프로세스 포함)가 쥐고 있으면 이미 사용 중이라는 오류로 실패하고 writer가 되지 않는다.
  자식 프로세스는 생성부터 exec까지 부모의 잠금 파일 설명을 복제해 가지므로, 방금 놓은 잠금이 그동안 남아 있을 수 있기 때문이다.
  잠금은 한 journal의 활성 writer를 하나로 제한하고, 같은 journal의 resume는 명시한 journal 선택과 이 잠금으로만 허용한다.
- 잠금 대상은 journal DB 파일이 아니라 그 옆의 `<journal 파일 경로>.writer-lock` 파일이다. Windows의 파일 잠금(LockFileEx)은 강제 잠금이라
  DB 파일 자체를 잠그면 같은 파일에 대한 SQLite 자신의 읽기·쓰기까지 막히기 때문이다. 세 플랫폼에서 같은 방식을 쓴다.
- 잠금 파일은 writer가 끝나도 지우지 않는다. 잠금을 놓는 사이에 파일을 지우면 다음 두 프로세스가 서로 다른 새 파일을 만들어 각각 잠금을 얻을 수 있기 때문이다.
  잠금은 writer를 놓거나 저장소를 닫을 때, 그리고 프로세스가 비정상 종료할 때 OS가 푼다.
- writer 세대(epoch) fencing은 잠금과 함께 유지한다. 모든 쓰기 API는 잠금 보유를 먼저 확인하고, 이어서 현재 세대를 확인한 transaction에서만 쓴다.
  잠금 없이 쓰는 것은 journal을 열 때의 WAL 전환(`PRAGMA journal_mode = WAL`), schema migration, journal ID 기록뿐이며, migration과 journal ID 기록은 하나의 SQLite 쓰기 transaction으로 함께 직렬화된다.
  journal을 열 때 migration 버전 확인·적용과 journal ID 기록은 그 한 transaction에서 함께 한다. 여러 연결이 빈 파일을 동시에 처음 열어도 migration은 한 번만 적용되고 모든 open이 성공한다.
  WAL 전환은 transaction 밖에서 하며, 다른 연결의 전환과 겹쳐 잠금 오류가 나면 busy 대기 시간 안에서 다시 시도한다.
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

- journal은 적용한 migration 버전을 버전 표에 한 줄씩 남긴다. migration은 뒤에 추가만 하고 배포한 항목은 고치지 않는다.
  아직 적용하지 않은 버전은 버전 확인·journal ID 기록과 같은 transaction에서 모두 적용하며, 중간에 실패하면 어느 버전도 남지 않는다.
- 이 빌드가 아는 버전보다 새 journal은 열지 않고 schema가 너무 새롭다고 알린다. 읽기 전용으로 열거나 모르는 형식을 건너뛰며 읽지 않는다.
  새 journal을 읽지 못하는 옛 바이너리가 그 journal이나 그 상태에서 만든 export를 덮어쓰지 않게 하기 위해서다.
- 비어 있지 않은데 버전 표가 없는 SQLite 파일은 journal이 아니라고 거절한다. 이 판정은 WAL 전환·migration·journal 바인딩보다 먼저 하고 파일을 바꾸지 않는다.
  잘못된 경로로 받은 다른 DB(memory.db 등)를 journal로 바꾸지 않기 위해서다. 빈 파일과 새 경로만 새 journal로 만든다.
- 파일에 기록된 journal ID가 요청한 ID와 다르면 열지 않는다. 최근 파일이나 다른 단서로 대상을 추측하지 않는다.
- WAL, `synchronous=FULL`, foreign key 검사가 실제로 적용됐는지 되읽고, 하나라도 다르면 열지 않는다. 완화된 설정이나 in-memory로 대체하지 않는다.

### 영속 ID 예약

- journal은 kind별 다음 예약 ID(high-water)를 저장한다. 예약은 그 ID를 쓰는 이벤트 commit보다 먼저, writer 잠금과 현재 세대를 확인한 자기 transaction으로 확정한다.
  잠금이 없거나 이전 세대인 저장소의 예약은 거절하고 아무것도 바꾸지 않는다.
- 예약한 범위는 재오픈 뒤에도 다시 내주지 않는다. 예약 뒤 commit이 실패하거나 예약한 ID를 다 쓰지 않으면 그 구간은 빈 채로 남는다.
- 첫 ID는 1이다. 0은 호출자가 예약 상수로 쓸 수 있도록 내주지 않는다.
- 호출자가 준 상한과 SQLite 정수에 저장할 수 있는 상한 중 작은 값을 넘는 예약은 되감지 않고 거절하며 상태를 바꾸지 않는다. 0개 예약도 거절한다.
- 구조 journal은 데이터 홈에 하나 둔다. 엔진은 그 journal 안의 stream이며 엔진마다 journal 파일을 따로 두지 않는다.
  구조 ID는 이 journal의 kind별 예약에서 발급하므로 journal 하나로 모든 엔진에 걸쳐 유일하다. 프로세스가 공유하는 runtime ID 발급기는 이 예약에서 받은 구간을 나눠 주는 캐시가 된다.
- 엔진의 stream 이름은 그 엔진이 쓰는 레이아웃 슬롯 번호로 정한다(`structure:slot-<N>`). runtime 엔진 ID는 재시작마다 새로 매겨지고,
  [ADR-0059](0059-id-targets-and-view-owned-selection.md)에서 엔진마다 슬롯이 하나라 슬롯 번호가 엔진의 영속 식별이기 때문이다.
- wire 표현은 `u32`로 유지한다. surface는 standalone PTY ID 기준값 미만, 그 밖의 구조 kind는 `u32` 최대값을 상한으로 예약한다.
  상한을 넘는 예약은 위 규칙대로 되감지 않고 거절한다. surface와 PTY의 범위 분리는 [ADR-0059](0059-id-targets-and-view-owned-selection.md)를 따른다.
- journal 밖의 ID(PTY·hook·observer·notification)는 기존 카운터를 쓴다.
- 원격 mirror의 로컬 구조 ID도 같은 예약에서 받는다. mirror는 로컬 journal에 기록하지 않으므로 이벤트 없이 예약만 소비한다([ADR-0061](0061-external-remote-module-and-attach-sync.md)).
- 이 절의 journal 배치와 전역 발급은 구조 journal을 제품에 연결하기 전에 정했다. 처음 결정은 값 공간을 journal 내부로 한정하고 runtime ID와의 통합을 범위 밖으로 두었다.

### 슬롯 재사용과 엔진 incarnation

- 정상 복원은 명시된 slot stream의 구조 ID·명령 identity·incarnation을 유지한다. 프로세스 재시작의 writer/runtime epoch는 별도로 바뀐다.
- 복원을 끄고 창을 버릴 때는 `engine.retired`를 먼저 확정한다. 실행 중인 materialization은 기존 owner에서 마무리하고 새 활성화는 받지 않는다. App registry의 retiring 관계가 자원을 유지하며, publication 완료 뒤 그 owner를 버린다. 종료는 이 확정을 비동기로 기다린다.
- 폐기된 슬롯을 다시 사용하거나 복원 없이 새로 시작하면 `engine.incarnation_started`로 incarnation을 올리고 새 구조를 확정한다. stream history와 ID·activation high-water는 되감지 않는다. 이전 Running/Uncertain 의무는 incarnation 변경만으로 성공·폐기 처리하지 않는다.
- 첫 창은 journal을 읽은 뒤 미점유 활성 저장 슬롯을 우선 선택한다. 추가 창도 같은 파생 목록을 사용하고, retired 슬롯을 저장된 레이아웃 후보로 취급하지 않는다. legacy 파일의 존재·수정 시각으로 journal의 폐기 상태를 덮지 않는다.
- 슬롯 없는 headless 새 시작은 영속 예약한 engine 번호의 새 stream을 연다. 과거 임의 stream을 선택해 실행하지 않는다.

## Consequences

payload와 이벤트가 같은 SQLite transaction에 있으므로, 파일 blob과 DB commit 사이에 종료돼 참조가 끊기거나 고아 파일이 남는 경우를 따로 다루지 않아도 된다.
파일 이름·부모 디렉터리 영속화를 플랫폼별로 검증할 필요도 없다. 대신 journal 파일이 payload 크기만큼 커지고, 큰 payload가 WAL과 checkpoint 비용을 늘린다.
GC로 지운 공간은 VACUUM 전까지 파일 크기로 돌아오지 않는다.

잠금과 세대 검사를 함께 쓰면 두 프로세스가 같은 journal에 번갈아 쓰는 경우와 같은 프로세스 안의 늦은 결과를 모두 막는다.
잠금을 지원하지 않는 환경(일부 네트워크 파일시스템 등)에서는 writer로 열 수 없으므로 데이터 홈 위치에 제약이 생긴다.
일부 NFS·FUSE 구성은 잠금을 로컬에서만 성공시켜 호스트 간 배타를 보장하지 않는다.
journal마다 잠금 파일 하나가 옆에 남는다. 이 파일을 지우는 정리 작업은 실행 중인 writer가 없음을 확인한 뒤에만 한다.
잠금이 실제로 잡혀 있으면 writer 획득 실패까지 재시도 상한만큼 걸린다.

전이표를 고정하면 복구기가 상태 이름만으로 다음 동작을 정할 수 있다. Uncertain에서 Cancelled로 가려면 실행되지 않았다는 증거가 필요하므로
대조 수단이 없는 effect는 Uncertain으로 오래 남을 수 있다. 이는 결과를 아는 척하지 않기 위해 감수한다.

로그 보존은 batch/revision 헤더를 남기고 검증된 fallback snapshot까지의 이벤트 본문만 정리한다.
`retention_anchor`가 snapshot과 batch cut을 연결하고 `retained_stream_revisions`가 stream별
재동기화 하한을 보관한다. 이벤트 본문·그 event holder·보존 경계 갱신은 한 transaction이다.
명령 identity·최초 응답·effect와 시도 기록·ID 예약은 정리하지 않는다. 삭제한 이벤트의 ID도
별도 명부에 남기며 같은 ID를 새 이벤트로 다시 넣는 쓰기를 거절한다. snapshot/live/undo/import/
View 및 읽기 lease의 payload pin도 남는다. snapshot 두 개의 본문과 참조 checksum을 검증하며,
외부 holder가 잡은 더 오래된 snapshot이 있으면 보존 경계를 그 cut 이하로 제한한다.
보존 경계 이전 cursor는 명시적 재동기화 오류다. snapshot이 손상돼도 보존 로그 없이 처음부터
재생하는 성공 fallback을 만들지 않는다. 헤더까지 제거하는 안은 역사 cut과 effect 외래 키를
다시 쓰게 하므로 채택하지 않았다. 이 방식은 이벤트 본문과 고아 payload 공간을 회수하지만
batch/revision/명령/effect 헤더의 크기까지 제한하지 않는다.
projection 출력은 consumer·projection version별 key→바이트 행으로 저장하고, 행 변경과 consumer 위치(batch 단위)를 한 transaction으로 확정한다.
위치가 뒤로 가거나 batch가 없으면 행 변경도 반영하지 않는다. projection 행을 가진 consumer는 위치만 저장하는 API로 위치를 옮길 수 없고, 행 변경과 함께 확정하는 API로만 옮긴다.
부분 stream 소비자는 `ScopedProjectionWrite`에 비어 있지 않은 stream 집합과 실제 batch ID를 지정한다.
저장소가 그 batch의 revision vector에서 선택한 stream만 도출하므로 서로 다른 시점의 임의
revision을 조합해 원자 cut으로 저장할 수 없다. cut에 아직 없는 stream도 거절한다.
출력·scope·cursor·재시도 digest는 같은 transaction이다. 이 cut은 선택 범위의 적용 위치이며
다른 stream이나 외부 DB의 적용을 보장하지 않는다.

consumer ID와 projection version의 scope는 첫 기록 뒤 바꾸지 않는다. global consumer를
같은 version의 scoped consumer로 전환하거나 반대로 읽기/쓰기할 수 없다. scope/코덱을
변경하면 새 projection version을 쓰고 전체 snapshot으로 초기화한다. 같은 batch의 같은
write만 retry로 받아들이며 다른 output 변경은 거절한다. digest에는 모드와 컬렉션 개수,
각 field 길이를 넣어 upsert/delete 경계를 모호하게 합치지 않는다.
선택 stream의 이전 cursor가 retention floor보다 뒤처지면 incremental write와 read는
`ResyncRequired`다. `replace_scoped_projection`만 전체 선택 출력으로 재동기화할 수 있고,
위치 역행이나 scope 변경은 허용하지 않는다. 관련 없는 stream의 compaction만으로 부분
consumer를 재동기화시키지는 않는다. 기존 global projection API의 cut 의미는 유지한다.

### 활성 저장량의 신규 admission 예산

`AdmissionBudget`은 내부 기본값으로 1 GiB 운영 기준, 128 MiB 완료·정리 여유, 신규 admission
ceiling 896 MiB, 미확정 명령당 64 MiB credit을 사용한다. `EventStore::open_with_admission_budget`
으로 명시적으로 바꿀 수 있으며 값 검증을 거친다. 이 값은 측정된 처리량·디스크 성능이나
filesystem quota가 아니다. SQLite `max_page_count`를 걸어 기수락 효과의 완료를 막지 않는다.

과금은 `(page_count - freelist_count) * page_size + 실제 WAL 파일 바이트`다. DB 물리 파일
크기와 재사용 가능한 freelist도 조회 결과에 별도로 제공한다. GC 뒤 DB 파일이 커도 빈 페이지는
다음 쓰기에 재사용할 수 있어 과금에서 제외한다. 한도 판단이 실패하면
busy timeout 0의 WAL TRUNCATE를 시도한 뒤 재측정한다. payload GC는 메모리 reader lease를
잠근 채 최신 참조를 durable pin으로 옮기는 기존 제품 checkpoint에서만 수행한다. admission
gate는 그 잠금과 참조를 알 수 없으므로 독립 GC를 실행하지 않는다. reader가 막으면 기다리거나 성공을
추측하지 않고 남은 WAL을 그대로 과금한다. PASSIVE checkpoint만으로 물리 WAL이 줄었다고
간주하지 않는다. key·최초 응답·미완료 effect·ID 기록을 예산 때문에 삭제하지 않는다.

worker는 durable key hit와 현재 admission follower 합류를 먼저 처리한다. 새 명령은
현재 과금량+기존 pending credit+새 credit이 ceiling 안일 때만 수락한다. 각 미확정 admission의
새 준비 payload는 holder별 누적 64 MiB를 초과할 수 없고 구조 ID 예약은 총 16,384개로 제한한다.
credit은 최초 Resolve, Cancel, 준비 오류에서 해제한다. 성공/실패로 credit을 해제해도 DB의
payload 페이지는 계속 과금되고 pin은 기존 event/snapshot/GC 수명에 따른다.

새 engine 구조/legacy import는 같은 gate를 거친다. 기존 stream resume, 이미 수락된 effect의
결과·cleanup·Recovery·capture·checkpoint·GC는 신규 admission gate로 막지 않는다.
그 의무가 진행하며 기준값을 넘을 수 있고 SQLite page/index/WAL 증폭도 있어 총 물리 파일의
최대 초과량이나 1 GiB hard cap은 보장하지 않는다. 128 MiB는 완료를 보장하는 예약 디스크가
아니다. OS FULL/IO 실패는 여전히 별도의 실제 오류다. 큰 기존 DB는 읽기/복구를 유지한 채
신규 admission을 거절할 수 있으며 GC/freelist 재사용·reader 종료 후 WAL truncate 또는
명시한 더 큰 내부 예산으로 다시 수락할 수 있다.

기존 worker queue는 64건/64 MiB, 공개 원 요청은 8 MiB이며 App command 대기는 64건이다.
이 한도와 별도로 DB의 Pending/Deferred/Running/Uncertain effect 총수 기본 상한은 4,096개다.
`AdmissionBudget.max_pending_effects`로 정하며 실제 신규 `NewEffect` 개수가 고정된 commit의
write transaction에서 기존 총수와 더해 검사한다. 같은 key의 최초 결과는 이 검사보다 먼저
돌려준다. 기존 effect 전이만 있는 완료·cleanup·reconciliation은 상한을 넘은 기존 DB에서도
허용한다. 기록 삭제나 task/OS 취소로 개수를 줄이지 않는다. 한 batch에서 종결과 새 효과를
동시에 요구하면 종결 예정분을 미리 차감하지 않는 보수적 admission이다.

효과 완료 Work의 기존 64 MiB queued-request 상한과 별도로 저장소 쓰기 경계에도
64 MiB 논리 batch 상한과 64 MiB 단일 BLOB 상한을 둔다. batch 계산은 실제 생성된
CommitRequest의 모든 문자열·바이트 필드와 참조당 8 bytes, envelope/record당 256 bytes를
합산한다. stream append뿐 아니라 command 원입력·응답·갱신·effect 본문·전이 결과도 포함한다.
중복 key 결과는 이 검사보다 먼저 반환한다. immutable payload 공통 insert가 단일 BLOB을
검사하므로 capture·snapshot·View manifest·import도 우회하지 않는다. snapshot/manifest는
본문과 참조 목록의 논리 합계도 검사하고, import의 commit+snapshot+manifest 및 payload 복사
transaction도 합계를 제한한다. 단독 effect 전이도 같은 논리 상한을 적용한다.
이 제한은 SQLite 파일이나 메모리 할당의 hard cap이 아니며 이미 생성한 입력을 쓰기 전에
거절하는 경계다. 초과하면 WriteSizeExceeded로 transaction을 되돌린다. 기존 의무/claim을
없애거나 결과 bytes를 잘라 성공으로 저장하지 않는다. effect 오류·Uncertain 대조와
checkpoint의 이전 snapshot/pin 유지·다음 기회 재시도는 기존 오류 처리 경로를 따른다.

ID 예약은 이벤트 commit과 다른 transaction이므로 실패한 명령이 쓰지 않은 ID가 빈 구간으로 남는다. ID가 연속이라는 가정에 기대는 코드는 이 journal의 ID에 쓸 수 없다.

데이터 홈에 구조 journal이 하나이므로 모든 엔진의 구조 쓰기가 한 파일의 writer에서 직렬화된다. 이 지연은 구조 명령 commit 지연 측정으로 확인한다.
여러 엔진을 바꾸는 명령은 추가 장치 없이 한 transaction으로 확정할 수 있다.
legacy layout importer는 슬롯마다 해당 엔진의 구조 stream으로 가져오며, surface ID는 standalone PTY ID 기준값 아래에서 예약한다.

schema 이름 기반 식별은 같은 이름의 버전 표를 가진 다른 앱 DB를 걸러내지 못한다. 이 저장소의 다른 DB는 그 표를 쓰지 않는다.

강제 종료 시험(`crates/tasty-event-store/tests/crash.rs`)은 commit 직후, 준비한 commit을 확정하기 전(API 경계), effect Running 기록 직후, 두 프로세스의 동시 첫 open에서 실제 프로세스를 abort한 뒤 journal을 다시 열어 판정한다.
transaction 내부 지점의 abort와 전원 차단 수준의 쓰기 유실은 재현하지 않는다.

## Alternatives Considered

- payload를 파일 blob으로 두는 안: DB 크기와 WAL 부담은 줄지만 파일 작성·sync·rename·디렉터리 sync와 DB commit의 순서, 고아 파일 GC, 플랫폼별 영속성 검증이 필요하다.
  현재 payload 크기 분포를 측정하기 전에는 이 복잡도를 감수할 근거가 없다.
- 세대 검사만 쓰는 안: 다른 프로세스가 같은 파일을 열어 아무 확인 없이 새 세대를 등록할 수 있고, 살아 있던 writer는 다음 쓰기부터 Fenced로 거절된다. 두 인스턴스가 같은 데이터 홈을 쓰는 실수를 막지 못한다.
- journal DB 파일 자체를 잠그는 안: Windows에서는 강제 잠금이라 SQLite가 같은 파일을 읽고 쓰지 못한다. 플랫폼별로 잠금 대상을 달리하면 동작 확인 범위가 늘어난다.
- 파일 잠금만 쓰는 안: 같은 프로세스 안에서 이전 writer·worker가 늦게 보낸 결과를 막지 못한다.
- 잠금을 한 번만 시도하는 안: 방금 놓은 잠금을 exec 전의 자식 프로세스가 복제해 쥐고 있으면 실제 writer가 없는데도 이미 사용 중이라는 오류(WriterLocked)가 난다.
- Uncertain을 새 generation 등장 시 Superseded로 닫는 안: 실제로 실행된 자원의 정리 의무를 잃는다.
- 재시도 때 이전 결과를 effect에 남겨 두는 안: 새 attempt가 진행 중인데 이전 실패 결과가 보여 복구기가 오독할 수 있다.
- 새 schema journal을 읽기 전용으로 여는 안: 모르는 이벤트 형식을 해석할 수 없어 조용히 건너뛰게 되고, 옛 바이너리가 만든 결과가 최신 상태처럼 보인다.
- 다른 SQLite 파일도 journal로 초기화하는 안: 잘못된 경로 하나로 사용자 데이터 파일에 journal 표가 생기고 WAL로 바뀐다.
- ID를 이벤트 commit과 같은 transaction에서 배정하는 안: 빈 구간은 없지만 `decide`가 commit 전에 ID를 고정 입력으로 받을 수 없다.
- 상한에 닿으면 ID를 되감아 재사용하는 안: 예약 ID를 재사용하지 않는다는 ADR-0055의 전제를 깨고 옛 이벤트의 참조와 섞인다.
- 엔진마다 journal을 두고 `u32` 상위 비트에 엔진 번호를 넣는 안: 엔진당 surface ID 수가 줄고 기존 슬롯의 ID를 다시 매겨야 한다.
  여러 엔진을 바꾸는 명령을 한 transaction으로 확정할 수 없어 ADR-0055의 다중 엔진 원자성 조항을 고쳐야 한다.
- 엔진마다 journal을 두고 ID만 데이터 홈의 공용 할당 저장소에서 받는 안: 잠금과 파일이 둘이 되고 예약과 commit이 다른 저장소라 종료 경계가 늘어난다. 다중 엔진 원자성 문제는 앞의 안과 같다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- 제품에 연결할 때 모든 쓰기 경로가 잠금을 얻은 writer를 거치는지 확인한다. 잠금 없이 쓰는 경로가 생기면 연결하지 않는다.
- batch/revision 헤더까지 줄일 필요가 생기면 effect 외래 키와 역사 cut 재구성을 함께 다시 설계한다.
- snapshot 참조를 가진 독자·import가 추가되면 해당 holder가 compaction 전에 등록되고 실제 읽기 완료까지 유지되는지 확인한다.
- 고정 stream scope와 실제 batch cut으로 표현할 수 없는 소비 위치나 외부 projection 저장소가 필요해지면 checkpoint와 출력 원자성 계약을 다시 정한다.
- 엔진마다 journal 파일을 나눠야 하거나 여러 journal이 구조 ID를 나눠 써야 하는 요구가 생기면 ID 예약 절의 journal 배치와 발급 범위를 다시 정한다.
- 슬롯 선택·폐기 정책이 바뀌면 위 incarnation 규칙과 View checkpoint의 binding 검사를 함께 다시 본다. 과거 명령 identity·미완 효과를 슬롯 재사용 때문에 지우지 않는다.
- 구조 kind의 `u32` 범위가 고갈에 가까워지거나 surface·PTY 외의 ID 종류가 같은 공간을 쓰게 되면 좁힘 규칙과 wire 표현을 다시 본다.
- 여러 호스트나 여러 프로세스가 같은 journal에 써야 하는 요구가 생기면 잠금·세대 모델을 다시 정한다.

### 실행 결과로 확인

- payload 크기 분포와 journal 파일 크기, WAL checkpoint 시간, 구조 명령 commit 지연을 측정한다. 큰 payload가 commit 지연이나 디스크 사용을 눈에 띄게 늘리면 크기 기준으로 파일 blob을 도입한다.
- Uncertain effect가 대조 수단 없이 오래 남는 사례가 쌓이면 effect 종류별 대조 방법을 추가한다.

## References

- [ADR-0055](0055-structural-domain-event-sourcing.md) · [ADR-0056](0056-crate-boundaries-for-core-event-store-and-task-runtime.md) · [ADR-0057](0057-command-identity-for-mutation-retries.md) · [ADR-0010](0010-storage-failure-reporting.md)
- 현재 구조: [아키텍처](../architecture/index.md)의 도메인-IO 절
- 현재 구현: `crates/tasty-event-store/src/store.rs`, `crates/tasty-event-store/src/schema.rs`, `crates/tasty-event-store/src/payload.rs`, `crates/tasty-event-store/src/effect.rs`, `crates/tasty-event-store/src/command.rs`, `crates/tasty-event-store/src/identity.rs`(ID 예약), `crates/tasty-event-store/src/projection.rs`(projection 출력·cursor).
