# 이벤트 저장소 (`tasty-event-store`)

데이터 홈 구조 journal의 저장 계약 중 수치·형식·보조 규칙과, App이 그 journal을 원본으로 연결한 현재 상태를 적는다. 결정과 기각한 대안은 [ADR-0063](../adr/0063-event-store-storage-fencing-and-effect-states.md),
저장소가 제공하는 기능의 개요는 [아키텍처](index.md)의 `tasty-event-store` 문단에 있다. 구현은 `crates/tasty-event-store/src/`다.

## import의 전송 identity

내부 import는 전송 ID로 source snapshot/View alias를 고정한다. alias의 최초 source key와 전체 요청 digest도
같은 fenced transaction에 저장한다(schema V8의 restore_alias_inputs). alias/pin만 남고 입력 identity가
빠지는 중간 상태는 commit하지 않는다. 같은 입력 재시도는 source의 최신 manifest 대신 원 고정 cut을
재사용하며, 같은 source journal에 이미 있는 alias의 source key나 destination slot 등 입력이 달라지면
명시 충돌이다. 다른 source DB에 alias가 없는 미완 전송까지 전역 선점하는 정책은 두지 않는다. 입력 identity가 없는
과거 alias는 현재 요청에 소급 귀속하지 않고 보존한 채 거절한다. 일반 View 저장은 고정 alias를
덮어쓸 수 없고 alias 삭제는 그 입력 identity도 FK cascade로 해제한다. destination의 새 ID mapping·초기 events·
최초 응답·domain snapshot·View manifest를 같은 transaction에 확정한 뒤 source alias를 해제한다.
실패한 준비 payload는 admission holder로 보존하며 다음 fenced writer가 미수락 잔여 pin을 정리한다.
destination commit 뒤 응답 유실은 같은 전송 키 조회로 합류하고 source의 command/effect/cleanup을 복사하지 않는다.

## 잠금 없이 하는 쓰기

잠금 없이 쓰는 것은 journal을 열 때의 WAL 전환(`PRAGMA journal_mode = WAL`), schema migration, journal ID 기록뿐이며, migration과 journal ID 기록은 하나의 SQLite 쓰기 transaction으로 함께 직렬화된다.
journal을 열 때 migration 버전 확인·적용과 journal ID 기록은 그 한 transaction에서 함께 한다. 여러 연결이 빈 파일을 동시에 처음 열어도 migration은 한 번만 적용되고 모든 open이 성공한다.
WAL 전환은 transaction 밖에서 하며, 다른 연결의 전환과 겹쳐 잠금 오류가 나면 busy 대기 시간 안에서 다시 시도한다.

## ID 예약의 경계값

- 첫 ID는 1이다. 0은 호출자가 예약 상수로 쓸 수 있도록 내주지 않는다.
- 호출자가 준 상한과 SQLite 정수에 저장할 수 있는 상한 중 작은 값을 넘는 예약은 되감지 않고 거절하며 상태를 바꾸지 않는다. 0개 예약도 거절한다.

## 로그 보존

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

## projection 출력과 consumer 위치

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

## 활성 저장량의 신규 admission 예산

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
현재 과금량+기존 pending credit+새 credit이 ceiling 안일 때만 수락한다. 기존 pending credit
때문에 맞지 않으면 바로 거절하지 않고 worker 안에서 도착 순서대로 기다린다. pending admission이
끝나 credit이 해제되면 다시 판단하고, pending이 하나도 없는데도 맞지 않으면 거절한다. 기다리는
요청도 아래 worker queue 64건 한도에 포함하며, 앞선 대기가 있으면 새 admission은 그 뒤에 선다. 각 미확정 admission의
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
Global/scoped projection의 incremental/replace도 같은 64 MiB 논리 쓰기 상한을 쓴다.
consumer·scope 이름·upsert key와 row payload·모든 delete key를 합산하고 각 항목의
고정 비용도 센다. row payload는 단일 BLOB 상한도 검사한다. scope/digest 구성과 기존
출력 삭제 전에 거절하므로 초과 요청이 cursor나 이전 출력을 바꾸지 않는다.
이 제한은 SQLite 파일이나 메모리 할당의 hard cap이 아니며 이미 생성한 입력을 쓰기 전에
거절하는 경계다. 초과하면 WriteSizeExceeded로 transaction을 되돌린다. 기존 의무/claim을
없애거나 결과 bytes를 잘라 성공으로 저장하지 않는다. effect 오류·Uncertain 대조와
checkpoint의 이전 snapshot/pin 유지·다음 기회 재시도는 기존 오류 처리 경로를 따른다.

## 구조 journal의 App 연결

원본과 projection의 관계는 [ADR-0065](../adr/0065-journal-source-and-core-state-projection.md)가 정한다.

`src/app/journal.rs`가 데이터 홈의 worker 하나와 엔진별 비동기 continuation을 연결한다.
worker의 순수 구조 모델을 초기 논리 projection과 확정 batch 적용에 사용하며,
App이 별도 가변 JournalModel 원본을 유지하지 않는다. 준비 요청·완료 채널과 복원 읽기 개수,
App 한 회의 완료 처리량은 제한한다.

생성·변환은 Pending operation과 outbox를 확정하고 claim한 뒤 후보를 만든다.
준비 성공 다음 commit은 설치 권한·옛 owner 정리 의무만 확정한다. 외부 게시·설치와 정확한
옛 PTY 회수 뒤 최종 commit이 구조 변경·Ready·원 요청 완료를 함께 확정하고 공개한다.
설치 전 kind 철회·등록 교체는 후보를 폐기하고 기존 인스턴스를 유지한다.
외부 게시 이후의 불명 결과는 Recovery의 원 attempt·receipt 증거와 대조하며, 알려진 실패로
바꿔 자동 재실행하지 않는다.

표시면은 첫 capture 전에도 생성 자료 참조를 유지한다. 복원은 snapshot을 우선하고,
없으면 generic 생성 params/CWD를 사용한다. terminal은 현재 셸 설정과 저장된 명시적
복원 명령을 사용하며 과거 실행 인자·입력을 자동 재전송하지 않는다.
복원 capture는 기존 DataRef로 읽고, 같은 자료를 준비 요청에 복사해 다시 저장하지 않는다.
선택되지 않은 terminal과 아직 등록되지 않은 kind는 지연 활성화하며, 일반 명령도 필요한 원 대상의 activation에 합류한다. source 경계가 존재한다는 사실을 모든 writer·장애 복구 시나리오의 검증 완료로 해석하지 않는다.

선택한 legacy slot은 worker가 원본 파일과 자료를 읽어 한 import batch로 확정하고 초기 View의
ID 대응을 함께 보존한다. 이미 journal stream이 있으면 legacy 파일을 다시 원본으로 읽지 않는다.
CoreState 생성자도 제품에서 legacy 파일을 선행 해석하지 않는다. 기존 위치가 범위를 벗어나면
workspace와 tab은 마지막 항목, pane은 첫 항목을 고르는 복원 규칙을 유지한다.

정상 resume는 journal/stream/incarnation과 확정 cut/revision을 붙인 최신 View checkpoint를 초기 import 선택보다 우선한다. 이 선택은 구조 이벤트가 아니다. `src/runtime/journal_product/view_record.rs`는 DB restore manifest를 먼저 읽고 그 domain checkpoint와 View의 binding을 대조한다. DB 원본이 없는 경우에만 legacy sidecar를 최초 이관 자료로 읽으며, DB 자료가 손상됐다고 옛 sidecar로 조용히 돌아가지 않는다.

현재 incarnation을 선택하지 않은 시작에서는 과거 View를 읽지 않는다. worker는 옛 incarnation과 더 늦게 도착한 과거 sequence의 저장을 거절한다. 저장 실패의 후보와 dirty 상태는 재시도를 위해 남기며 종료는 기존 tick 저장과 별도로 최신 final capture를 요청한다. View manifest와 도메인 checkpoint·payload 참조를 연결하는 pin/보존 구현은 저장 계층에 있다([ADR-0063](../adr/0063-event-store-storage-fencing-and-effect-states.md)). 해당 코드의 존재가 crash·전원 장애 검증을 완료했다는 뜻은 아니다.
