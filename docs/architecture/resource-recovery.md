# 자원 회수와 Recovery 관측

구조 journal의 operation 결과와 실제 실행 자원의 관측은 구별한다. 새 writer lease,
View 소멸, timeout, PID 부재만으로 이전 PTY·plugin 실행이 끝났다고 기록하지 않는다.
실행 결과를 모르면 `Uncertain`과 원래 generation·attempt·자료 참조를 유지한다.

## 현재 프로세스의 회수

`ResourceRetirement`와 `Installation`은 정확한 이전 PTY의 `PtyRetirement`를 보유한다.
PTY의 `Reaped` 관측이 있어야 OS 회수가 끝난다. `WaitFailed`나 기한 경과는 불명 결과다.
설치 중 후속 단계가 실패해도 이전 owner receipt와 남은 candidate는 버리지 않는다.

Plugin 회수는 원 surface binding과 process binding에 연결한 destroy RPC의 응답으로
관측한다. FIFO enqueue나 host 등록 제거는 응답이 아니다. 원 process의 성공 응답 또는
host FIFO에서 생성 요청이 아직 전달되지 않았다는 정확한 취소 근거가 있어야 회수를
완료한다. 오류·연결 종료·process 교체는 불명이며, 살아 있는 process의 timeout만으로
pending 응답을 없애지 않는다. 이후 실제 응답이 오면 같은 retained owner가 확인한다.
일반 remote surface도 create/restore가 실제 큐에 수락된 process identity를 등록과 함께
보관한다. destroy와 별도 완료 응답을 기다리지 않는 명시 회수 통지는 그 process에만 보내며 같은 plugin ID로 재시작한
process에 대신 보내지 않는다. 생성 요청의 큐 미수락은 알려진 미전송이지만, 등록 부재나
원 process 소멸은 미전송 증거가 아니다.

Mesh도 각 runtime 인스턴스의 불투명 binding으로 bootstrap을 추적한다. 큐에 제출하지
못한 create는 미실행으로 구별하며, 제출된 create는 원 process와 request identity를
고정한다. 명시 bootstrap 요청이 새 process에 도착하면 별도 세대를 추가한다. 이전
세대도 보존하며 process 교체를 회수 성공으로 취급하지 않는다. 회수는 각 원 process에만
destroy를 보내고 모든 세대의 실제 응답을 기다린다. 같은 surface
ID의 새 인스턴스나 새 process를 찾아 대신 종료하지 않는다. View는 binding을 전달할
수 있지만 내부 상태는 plugin manager만 변경한다.

정상 구조 공개 장벽에서도 plugin control pump는 필수 회수 요청과 해당 응답을 처리한다.
새 create/restore는 원 FIFO 순서로 보관하고 일반 callback은 실행하지 않는다. 아직
보내지 않은 create/restore와 정확히 같은 binding의 retire가 만나면 그 한 쌍만 취소한다.
장벽이 풀리면 보관한 publication부터 정상 FIFO 처리를 재개한다. 보관 예산을 넘으면
초과 항목의 소유를 유지한 채 journal의 기존 halt/disposal 경계로 전이하며 성공이나
계속 진행 가능한 대기로 표시하지 않는다.

Journal이 멈춘 경우의 plugin control pump는 회수 요청과 해당 응답만 실행한다.
새 create/restore·hook·namespace continuation·자동 재시작은 실행하지 않는다.
일반 응답은 제한된 backlog에 남기며 정상 pump가 명시적으로 재개될 때 원 process
binding을 확인한다. backlog가 차면 읽기를 멈추고 완료를 추측하지 않는다.

## EngineSession 해제

닫힌 View나 실패한 View 생성의 EngineSession도 필수 의무를 먼저 배출한다.
`EngineRelease`는 남은 kind 인스턴스, standalone을 포함한 PTY, 기존 installation 및
private candidate의 회수 receipt를 보관한다. 실제 reap·plugin 응답이 끝나기 전에는
registry owner를 버리지 않는다. stream 보존 close는 구조·metadata를 지우지 않는다.
private candidate가 아직 publication closure를 보유하면 그 closure를 실행하지 않고
폐기했다는 원 binding의 완료 receipt를 전달한다. closure가 Installation으로 이동한
경우도 해당 owner가 증거를 보관한다. 이미 소비한 closure나 등록 부재로 이 증거를
만들지는 않는다.

이 경계는 TaskScope 취소가 아니다. runner stop·task cancel·OS kill은 기존의 별도
명령 의미를 따른다. surface에 묶인 observer와 hook을 해제한 뒤 PTY를 retire하며,
TaskScope의 비취소 Drop 의미를 바꾸지 않는다. 소멸 시점의 미완 관측을 성공으로
보고하거나 raw 입력을 재전송하지 않는다.

## 대조 결과 확정

현재 runtime의 정확한 effect lease와 retained resource receipt가 있어야 준비·회수의
불명 결과를 다시 대조한다. 늦은 완료를 처리할 때도 stream, operation, attempt,
runtime epoch, engine incarnation, activation 및 physical generation을 구분한다.
대조 evidence payload와 결과 event/effect 갱신은 journal의 같은 commit 경계를 지난다.
여러 leaf를 복원하는 묶음에서 미실행 member의 취소도 준비 결과 집계에 반영한다.
이미 준비한 private peer에는 원 generation의 폐기 의무를 만들고 receipt 뒤 묶음을 닫는다.
모든 member가 알려진 실패·취소로 끝났다면 대상이 사라져도 실패로 완료하며, 실제 설치나
불명 결과가 있는 peer를 새 준비 대상으로 다시 허가하지 않는다.
그 뒤에 전체 batch publication ACK와 최초 command 결과 완료를 처리한다.

재시작 시 미확정 creation이 남은 placeholder는 Recovery 차단 상태를 갖는다.
선택 복원은 이를 건너뛰며 이전 실행을 새 factory 호출로 대체하지 않는다. 준비 단계는
실행에 들어가기 전 실패와 factory·spawn 진입 이후의 불명 오류를 구분한다.

이 문서는 구현의 소유 계약을 설명한다. 장애 재현·플랫폼별 실행 결과는 별도의 검증
기록으로 확인하며, source에 이 경계가 있다는 사실이 실행 검증 통과를 뜻하지 않는다.

원격 user close의 undo 권한은 로컬 사용자 lifecycle origin과 별도로 저장한다.
원격 사용자가 닫은 항목은 undo에 남길 수 있지만 plugin의 `is_user_close` 통지는
로컬 사용자 동작으로 바뀌지 않는다. 기존 저장 기록에는 원격 flag가 없으므로 false로 읽는다.

원격 forward의 `op_id`는 Result/Delta 상관관계 값이며 durable 멱등 키를 만들지 않는다.
서버 journal은 각 접수의 command identity를 발급하고 원 connection registration과
응답 binding을 유지한다. client/runtime epoch/registration/op_id는 내부 요청 envelope의
correlation 값으로 원 command 입력 바이트에 보존하며 중복 제거에 사용하지 않는다.
공개 IPC에서 명시한 멱등 키의 기존 계약과는 별개다.

새 pane의 live 생성 완료는 원 target pane과 확정 split 방향으로 `pane.split`을 통지한다.
이 통지는 최초 성공 command continuation에서만 생성한다. Stored 재시도, journal replay와
기존 surface의 materialization은 같은 사용자 생성 통지를 다시 만들지 않는다.

Headless 종료도 journal 실행을 먼저 중단한 뒤 미공개 후보와 기존 설치·PTY·plugin 회수 receipt를 같은 EngineRelease에 모은다. 종료의 5초 상한 안에서 runner join, 감지 worker join, PTY reap 및 원 plugin destroy 응답을 각각 관측한다. 시간초과는 회수 완료나 durable 명령 취소를 뜻하지 않으며, 미완 operation은 다음 시작의 Recovery 대상으로 남는다.

View의 구조 정합은 실제 Explorer·DAG cache 및 셸 안내 기록의 ID를 현재 descriptor와 대조해 사라진 항목을 회수한다. terminal viewport는 기존 retain 경로를 유지한다. 늦게 도착한 닫기 통지의 ID만으로 cache를 지우지 않으므로 동일 ID에 현재 descriptor가 있는 후속 owner의 표시 상태는 보존된다.

튜토리얼 연습 workspace 준비는 최초 생성 완료를 원 View와 원 준비 요청 token에 전달한다. 성공 시 실제 생성 surface의 현재 activation·physical binding을 확인해 practice workspace/pane/tab을 설정하고, 실패 시 같은 요청의 preparing을 해제한다. 중단·재시작된 튜토리얼의 이전 결과나 replay는 새 준비 요청을 완료시키지 않는다.

구조 완료 통지는 journal의 live completion과 host event에서 전달한다. 옛 동기 CoreEvent의 create/close/move/restore 결과와 즉시 owner 삭제 API는 제품 실행 경로로 유지하지 않는다. snapshot과 전체 이력 replay 비교용 보조는 시험에 한정하고, 제품 저장은 live pin과 View manifest를 포함한 checkpoint를 사용한다.

App journal의 publication pump는 `journal/publication.rs`에서 batch 설치와 ACK 순서를 소유한다. `commands/completion.rs`는 worker 결과와 원 응답을 결합하고, GUI의 고정 대상 해소·원 View 후속 처리는 `commands/view_completion.rs`에서 수행한다. 모듈 분리는 동일한 큐·owner·barrier를 유지하며 별도 실행 경로나 worker를 추가하지 않는다.
