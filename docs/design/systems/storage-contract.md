# 저장 계약과 writer 명부

호스트가 디스크에 쓰는 경로와 각 경로의 성공·오류·실행 계약을 정리한다.
파일 위치와 SQLite 설정, 실패 분류는 [저장소 시스템](storage.md)에, 측정값은 그 문서의 [저장 기준선](storage.md#저장-기준선)에 있다.

## 네 시점

저장 요청에는 네 시점이 있다. 현재 경로마다 이 시점이 어떻게 겹치는지 적는다.

- 접수: 호출자가 요청을 넘기고 다음 일을 시작할 수 있는 시점.
- commit 완료: SQLite transaction 또는 파일 쓰기가 끝난 시점. 전원 장애 내구성과는 다르다(storage.md의 `synchronous=NORMAL` 절).
- 메모리 표시: 다른 호출자나 화면이 새 값을 보는 시점.
- 외부 실행: 저장 결과에 따라 프로세스 실행·응답 전송 같은 되돌릴 수 없는 일이 일어나는 시점.

| 경로 | 접수 | commit 완료 | 메모리 표시 | 외부 실행 |
|---|---|---|---|---|
| memory.db IPC 쓰기 | 별도 접수 없음. 핸들러가 메인 루프에서 `with_memory` 잠금을 잡고 실행한다 | 핸들러 안에서 끝난다 | DB가 원본이므로 commit과 같다. `memory.changed`는 commit 뒤 버퍼에 쌓였다가 GUI 이벤트 루프가 같은 회차의 IPC 처리 뒤에 발행한다 | IPC 응답은 핸들러가 돌아온 직후 같은 루프 회차에 보낸다. 응답 시점에는 commit이 끝나 있다 |
| memory.db background writer(observer sink, `approval.await` 결과 기록, runner 결과·보고) | 각 스레드가 직접 잠금을 잡는다 | 그 스레드 안에서 끝난다 | commit과 같다 | 해당 없음 |
| task runner 실행 | Ready task를 tick snapshot으로 고른다(별도 잠금) | lease·semaphore 획득, 실행 handle 기록, `Running` 전이가 각각 별도 잠금·commit이다 | commit과 같다 | 점유 획득 뒤, `Running` 기록 **전에** 프로세스를 시작한다. `Running` 기록이 실패하면 경고만 남고 handle을 보관하지 않는다 |
| 구조 journal | 전용 worker 채널에 넣는다 | worker가 commit한다 | commit된 batch만 CoreState에 적용한다([ADR-0064](../../adr/0064-journal-source-and-core-state-projection.md)) | 대상 엔진 공개와 ACK 뒤에 응답·checkpoint를 진행한다 |
| 최근 파일(state.db) | 별도 접수 없음 | 호출 스레드에서 끝난다 | 공유 캐시를 DB 쓰기 전에 같은 잠금 안에서 바꾼다. 저장이 실패해도 캐시는 바뀐 채 남는다 | 해당 없음 |
| 튜토리얼 진행(state.db) | 전용 worker에 넣고 receipt를 받는다 | worker가 요청마다 transaction으로 commit한다 | receipt 결과로 반영한다 | 해당 없음 |
| 설정·프리셋·공용 목록 등 파일 | 별도 접수 없음 | 호출 스레드에서 끝난다 | 호출자가 메모리 값을 먼저 갖고 있다 | 해당 없음 |

## memory.db 소비자와 계약

모든 소비자는 같은 `Arc<Mutex<dyn MemoryStorage>>`를 공유한다. `AppServices::with_memory`, `TaskService`, runner 문맥, observer sink가 같은 잠금을 잡고 콜백이 끝날 때까지 유지한다. 전용 writer 스레드는 없다.

### 연산별 계약

| 연산 | 성공 | 거절·오류 | 부분 반영 | owner·CAS·TTL |
|---|---|---|---|---|
| `put` | 새 version(신규 1, 갱신 +1) | NotFound는 없음. CasConflict(행이 없는데 cas가 있으면 actual 0), OwnedByOther, QuotaExceeded, Invalid*, ValueTooLarge, Db | 한 transaction이다. 거절과 commit 실패는 롤백되고 quota 카운터·변경 버퍼를 바꾸지 않는다 | `_host`는 모든 owner의 항목을 바꾼다. 기존 행 조회는 `expires_at`을 거르지 않아 아직 정리되지 않은 만료 행도 owner·CAS·quota 판단에 들어간다 |
| `delete` | `()` | NotFound, CasConflict, OwnedByOther, Db | 한 transaction이다 | put과 같은 owner 규칙. 만료 행도 지울 수 있다 |
| 조회(`get`·`list`·`count`·`query`·`export_regular`) | 값 | Db | 해당 없음 | 만료 행을 제외한다. `scopes`는 만료 행을 포함한다 |
| `import_regular` | applied·skipped 수 | put의 오류 | 항목마다 `put`을 따로 commit한다. 중간 오류가 나면 앞 항목은 반영된 채 오류를 돌려준다 | 항목의 `expires_at`을 그대로 쓰고 cas는 쓰지 않는다. `memory.import` 핸들러는 이 함수를 부르기 전에 모든 항목의 scope를 put과 같은 조건(PTY id 공간, 열린 surface)으로 검사하고, 하나라도 걸리면 아무 항목도 넣지 않고 거절한다 |
| secret 계열 | put·delete와 같다 | 같다 | 같다 | owner가 키의 일부이고 owner별 quota다. 변경 이벤트를 만들지 않는다 |
| `purge_scope` | 영역별 삭제 수 | Db | 이벤트 대상 SELECT와 regular·secret DELETE 두 문장이 한 transaction이다. 어느 문장이든 실패하면 전부 롤백되고 quota 카운터·변경 버퍼를 바꾸지 않는다. commit이 성공한 뒤에만 지운 regular 키마다 변경 이벤트를 쌓고 카운터를 다시 센다 | owner를 보지 않는다. 삭제 전 SELECT로 이벤트 대상 키를 모은다 |
| `purge_expired` | 영역별 삭제 수 | Db | `purge_scope`와 같은 구조다 | `expires_at <= now`인 행 |
| `prune_prefix_keep_recent`·`prune_prefix_older_than` | 삭제 수 | Db | 한 문장을 transaction 하나로 commit한다. 행을 지웠을 때만 카운터를 다시 센다 | owner·scope를 보지 않고 변경 이벤트를 만들지 않는다 |
| `take_pending_changes` | 쌓인 변경 | 없음 | 해당 없음 | 버퍼는 commit 성공 뒤에만 쌓인다 |
| `write_batch` | 요청 순서의 요청별 결과(새 version 또는 삭제) | 원자 묶음의 거절, 저장 실패, commit 실패는 묶음 전체의 `BatchFailure` | 한 transaction이다. 아래 "실행 단위와 쓰기 묶음" 절 | 요청마다 단독 연산과 같은 판정을 앞 요청의 반영 위에서 한다 |
| `run_unit` | 본문의 결과 | 본문의 오류, 저장 실패, commit 실패 | 한 transaction이다. 거절된 쓰기만 되돌리고 앞선 쓰기는 commit한다 | 본문 안의 각 연산이 단독 연산과 같은 판정을 받는다 |

`vacuum_if_fragmented`와 `checkpoint_truncate`는 trait 밖의 `MemoryStore` 메서드이며 부팅 정비에서만 호출한다.

### 실행 단위와 쓰기 묶음

여러 쓰기를 SQLite transaction 하나로 확정하는 엔진 API다(`crates/tasty-memory/src/batch.rs`). 현재 호스트 소비자는 없고 모든 소비자는 위의 단독 연산을 쓴다.

- 실행 단위(`run_unit`, 값을 돌려받는 `with_unit`): 본문을 transaction 하나에서 실행하고 끝난 뒤 한 번 commit한다. 직렬화·commit의 단위이며 rollback의 단위가 아니다. 본문 안의 각 쓰기는 savepoint로 감싸 거절된 쓰기만 되돌린다. 본문이 오류를 돌려줘도 그전까지 반영된 쓰기는 commit되므로, 여러 번 쓰는 복합 연산(blackboard 삭제, `import_regular` 등)의 부분 반영 의미가 단독 실행과 같다. 본문 안의 읽기는 앞선 쓰기를 본다. 이미 열린 단위 안에서 다시 부르면 그 단위에 합류한다.
- 쓰기 묶음(`write_batch`): put·delete·secret put·secret delete 요청 목록을 transaction 하나로 실행한다. 원자 묶음(`Atomic`)은 요청 하나라도 거절되면 전체를 되돌리고 거절된 요청의 위치를 알린다. 독립 요청 묶음(`Independent`)은 요청마다 판정해 거절된 요청만 오류 결과를 받는다. 열린 단위 안의 원자 묶음은 묶음만 되돌린다.
- 판정 순서: 요청은 목록 순서대로 앞 요청의 반영을 본 상태에서 판정한다. version 증가, CAS, delete 뒤 재생성(version 1, 새 owner), owner, regular 전체 quota, secret owner별 quota, 정리되지 않은 만료 행을 포함하는 put의 기존 행 조회가 단독 연산과 같다. 같은 단위 안의 `purge_expired`·`purge_scope`·`prune_*`도 단위의 transaction에 들어가 뒤 요청이 정리 결과를 본다. 단위 밖의 이 정리 연산은 위 표처럼 각 호출이 자기 transaction 하나로 commit한다.
- 저장 실패: 단위 안의 쓰기·읽기(`get`·`list`·`count`·`scopes`·`stats`·`query`·`export_regular`·secret 조회)에서 `MemoryError::Db`가 나면 단위 전체를 되돌린다. SQLite가 아닌 행 변환 오류도 같다. SQLite는 일부 오류에서 transaction을 스스로 되돌리므로 같은 단위의 이후 쓰기는 실행하지 않고 같은 분류의 저장 실패를 돌려준다. 단위가 열려 있는데 연결이 autocommit 상태가 된 것(SQLite가 transaction을 끝냄)도 저장 실패로 보고 이후 쓰기와 commit을 실행하지 않는다. 본문이 이 오류를 무시해도 단위의 결과는 저장 실패다. 단위 밖 단독 호출의 실패는 그 호출만의 결과다. 묶음에서는 독립 요청 묶음이라도 묶음 전체의 `BatchFailure`이며 index가 원인 요청을 가리킨다. transaction 시작·commit 실패의 index는 없다. commit이 실패하면 열린 transaction을 되돌리고 확정하지 않는다.
- bookkeeping 공개: regular quota 카운터와 변경 버퍼(`memory.changed` 원본)는 단위가 쌓아 두었다가 commit이 성공한 뒤에만 저장소에 반영한다. 되돌린 요청·묶음·단위는 아무것도 남기지 않는다. 본문이 panic하면 단위를 되돌리고 panic을 다시 전파한다.
- commit 계측: `DbLatencyStats.commits`는 물리 commit 수다. 단위·묶음 하나는 쓰기가 하나 이상 확정됐을 때 commit 1회로 센다. 요청 수는 세지 않는다. 쓰기가 없는 단위와 되돌린 단위는 세지 않는다. 단위 안의 정리 연산은 그 단위의 commit에 포함되어 세어진다. 단위 밖의 정리 연산은 행을 하나 이상 지웠을 때 commit 1회로 세고, 지운 행이 없으면 세지 않는다.
- 시험용 저장소(`InMemoryStorage`): regular put·delete가 CAS·owner·NotFound를 운영 저장소와 같이 판정하고, 쓰기 묶음은 원자 묶음의 되돌림과 독립 묶음의 요청별 판정을, 실행 단위는 부분 반영을 따른다. `purge_scope`는 regular 행을 한 번에 지워 전부 지우거나 아무것도 지우지 않는다. quota·TTL·secret·변경 이벤트·저장 실패·commit은 흉내 내지 않는다. 대조 시험이 같은 요청 순서의 결과를 운영 저장소와 비교한다.

### 소비자 분류

| 분류 | 소비자 | 스레드 | 한 잠금 안에서 하는 일 |
|---|---|---|---|
| 단순 쓰기 | `memory.put`·`memory.delete`·secret, approval 기록, surface meta, telemetry·audit 기록 | 메인 루프(IPC) | 한 연산. 기록 시점 로그 정리가 붙는 경로는 prune 후 put |
| 복합 읽기-판단-쓰기 | blackboard·plan·cache·goal, session·barrier·semaphore·lease·ratelimit, command index, `memory.import` | 메인 루프(IPC·PTY 출력 처리) | 읽고 판단한 뒤 여러 번 쓴다. semaphore·lease의 저장은 CAS 없이 이 잠금의 직렬화에 의존한다. 여러 항목을 지우거나 되돌리는 연산은 원자적이지 않다 |
| task | `TaskService`(생성·graph submit·삭제·purge), runner tick | 메인 루프, `agent-runner-ws{N}` | graph submit은 plan·stage·activate를 한 잠금에서 한다. purge는 계획과 적용을 별도 잠금으로 한다. runner는 snapshot·점유 획득·상태 전이·완료를 각각 별도 잠금으로 한다 |
| background 쓰기 | observer memory sink, `approval.await` 결과, Run 결과·후처리·보고 | 각 전용 스레드 | observer는 put과 보존 delete를 같은 잠금 안에서 한다 |
| 읽기 | `memory.get`·`list`·`count`, approval·telemetry 조회, DAG 화면, task 대기 | 메인 루프, `dag-read`, task 대기 worker | 여러 scope를 순회하는 조회는 scope마다 잠금을 다시 잡아 하나의 시점을 보장하지 않는다. DAG 화면은 `try_lock`으로 폴링한다 |
| 변경 구독 | `memory.changed` 발행 | GUI 메인 루프 | `take_pending_changes`의 호출부는 GUI 이벤트 루프 하나다. headless 빌드에는 꺼내는 곳이 없다 |
| 삭제 cleanup | surface·workspace 은퇴의 `purge_scope`, 범위 밖 surface meta 정리 | 메인 루프 | 대상이 다시 살아 있으면 정리하지 않는다 |
| TTL 정리 | `memory.gc`(Local 전용)의 `purge_expired` | 메인 루프(IPC) | 주기 타이머는 없다 |
| 로그 prune | 관측 로그 보존(부팅·기록 시점·`Tick::LogPrune`) | 메인 루프 | 시간 기준 후 개수 기준. 한 시간 간격 제한 |
| 정비 | 부팅의 prune·VACUUM·WAL truncate | 부팅 메인 | port로 감싸기 전 구체 `MemoryStore`에서 실행 |

### 보존할 실행 계약

- `agent.task_graph_submit`은 저장소가 in-memory 대체 상태이고 요청 durability가 required이면 잠금을 잡기 전에 `StoreNotDurable`(-32602, `/durability`)로 거절한다. dry-run도 같다.
- runner는 lease·semaphore 획득 결과로 dispatch 여부를 정한다. 획득은 읽기-판단-쓰기를 한 잠금 안에서 하므로 두 runner가 같은 이전 상태를 보고 같은 permit을 얻지 않는다. 이 판단을 읽기와 쓰기로 나누면 이 보장이 사라진다.
- graph submit의 stage 실패는 롤백하고, activate 뒤 단계 실패는 `GraphPartiallyActivated`로 알린다.
- in-memory 대체 상태의 쓰기 성공 응답은 `durable:false`를 더한다([ADR-0010](../../adr/0010-storage-failure-reporting.md)).

## writer 명부

### SQLite DB

| DB | 소유자 | 쓰는 스레드 | PRAGMA | transaction | GUI 초기화 | headless 초기화 | 종료 |
|---|---|---|---|---|---|---|---|
| `memory.db` | `AppServices`의 공유 `Arc<Mutex<dyn MemoryStorage>>` | 메인 루프, observer sink, runner, `approval.await`, 결과·보고 스레드 | WAL·NORMAL, 되읽어 degraded 표시 | put·delete·secret·purge·prune은 명시 transaction, VACUUM은 자동 commit. 실행 단위·쓰기 묶음은 transaction 하나(현재 소비자 없음) | `run_gui`가 열고 부팅 정비 후 `App::new`에 주입. 실패 시 in-memory 대체 | `boot_memory`가 같은 절차로 연다 | 명시 close·checkpoint 없음. 마지막 `Arc`가 drop될 때 연결이 닫힌다 |
| `state.db` | 전역 `OnceLock<Mutex<Db>>`, `with_state_db`로 접근 | 메인 루프(최근 파일), `tutorial-progress` worker | WAL·NORMAL | 튜토리얼 저장과 스키마 생성은 transaction. `RecentFiles::add`의 같은 키 정리·upsert·개수 정리는 각각 자동 commit | GUI 부팅의 `init_boot_db_and_theme`과 새 창 생성 | 열지 않는다. 최근 목록 쓰기를 건너뛴다 | static이라 프로세스 끝까지 닫지 않는다. 튜토리얼 worker는 종료 단계에서 stop·join한다 |
| `structure/journal.db` | `JournalWorker`(App의 첫 필드) | 전용 `structure-journal` 스레드 | WAL·FULL이 아니면 열지 않는다([ADR-0063](../../adr/0063-event-store-storage-fencing-and-effect-states.md)) | 모든 쓰기가 즉시 잠금 transaction, writer 잠금과 세대 검사 | 부팅 첫머리에 writer 잠금을 선점하고 `App::new`에서 worker를 만든다 | 선점 없이 `App::new_headless`에서 만든다 | stop 후 Drop에서 join하며 worker가 마지막 checkpoint를 쓴다 |

### 파일

원자 교체는 임시 파일 작성 뒤 rename·persist를 뜻한다.

| 파일 | 쓰는 함수 | 스레드 | 원자 교체 | 다른 writer |
|---|---|---|---|---|
| `config.toml` | `Settings::save` | 메인 루프 | 아니요(직접 덮어씀). 해석하지 못한 원본만 저장 직전 백업 | 없음 |
| `presets/**.toml` | `PresetStore`의 `atomic_write` | 메인 루프 | 예(fsync 없음) | 없음 |
| `port-favorites.toml`·`explorer-favorites.toml` | `SharedList::update`가 목록 잠금 안에서 저장 | 호출 스레드 | 아니요 | 없음 |
| `child-terminals.json` | `child_terminal::save`(읽기-병합-쓰기) | 메인 루프 | 아니요 | 같은 홈의 다른 프로세스와 잠금 없음 |
| `tasty.port` | `write_port_file_to` | IPC 시작 | 아니요 | 없음. 서버 Drop이 지운다 |
| 단일 실행 instance 파일 | `instance_file` | 메인 루프 | 예(sync 포함) | GUI만 쓴다 |
| `webhooks.toml` | 호스트 `webhook::persist`, CLI `webhook_port_file` | 메인 루프·webhook 요청 스레드, CLI | 예 | CLI는 실행 중인 호스트가 없다고 판단할 때만 쓴다 |
| `remote-profiles.toml` | `remote_profiles` 저장 | 메인 루프(IPC·탐지), CLI | 아니요 | CLI가 IPC 없이 직접 쓴다 |
| `passkeys.toml`과 `passkeys/<이름>` | `passkeys.toml`은 `Passkeys::save`, inline 파일은 `materialize_inline_in`(`Passkeys::upsert_inline` 경유) | 메인 루프(IPC), CLI | `passkeys.toml`은 아니요(권한 0600은 쓴 뒤 설정). Unix의 inline 파일은 예: 0600으로 새로 만든 임시 파일에 쓰고 sync한 뒤 rename한다. 비 Unix는 직접 쓴다 | CLI(`tasty-cli`의 local passkey)가 IPC 없이 직접 쓴다 |
| `plugins.toml` | 플러그인 등록 상태 저장 | 메인 루프, GUI boot worker(내장 플러그인 설치) | 아니요 | 없음 |
| `known-plugins.toml`, hook·파일 handler 사용자 설정, 테마 파일 | 각 crate의 저장 함수 | 메인 루프 | 테마만 아니요, 나머지는 예 | 없음 |
| `debug.log`·crash/hang 보고·`launch.log`·observer file sink | 로그 기록 | 각 기록 스레드 | append | `launch.log`는 두 번째 프로세스가 파일 잠금 안에서 쓴다 |

`notify/<surface>.log`는 호스트가 쓰지 않는다. 플러그인 프로세스가 쓰고 호스트는 IPC 시작 때 디렉터리를 정리한다.

### port를 우회하는 직접 호출

- `SettingsStorage`는 `AppServices`에 주입되지만 읽는 곳이 없다. 설정 저장은 모두 `Settings::save` 직접 호출이다. `AppServices`를 가진 `intent/headless.rs`의 설정 적용과 `dispatch_domain.rs`의 구조 후 설정 적용도 port를 거치지 않는다.
- `PresetStorage`(`AppServices.presets`)도 읽는 곳이 없다. 저장·삭제·이름 변경은 구체 `PresetStore` 핸들로 한다.
- 테마 초기화는 `ThemeStorage` port 대신 `tasty_themes`의 자유 함수를 직접 부른다.
- memory.db의 부팅 정비는 port로 감싸기 전 구체 `MemoryStore`를 쓴다.
- state.db에는 port가 없다. 최근 파일과 튜토리얼이 `with_state_db`를 직접 쓴다.

### 종료 순서

GUI는 종료 상태기계가 layout 저장(S1) → 부팅 worker 회수(S2) → surface 닫기와 observer sink join(S3) → 플러그인·보조 worker 정리와 튜토리얼 join(S4) 순으로 진행한 뒤 이벤트 루프를 끝낸다. 그다음 instance 파일을 지우고 App을 drop한다. App의 첫 필드인 journal worker가 먼저 join되고, `services`(memory 연결)와 IPC 서버(포트 파일 삭제)는 그 뒤에 drop된다.

headless는 publication을 비운 뒤 journal worker를 stop하고(join은 App drop 때), runner·보조 worker를 회수한다. layout 저장·observer 일괄 join·튜토리얼 단계는 없다.

memory.db에는 종료 flush 단계가 없다. 각 쓰기가 호출 시점에 commit되므로 종료 때 남은 큐가 없다.
