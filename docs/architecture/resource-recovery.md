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

Journal이 멈춘 경우의 plugin control pump는 회수 요청과 해당 응답만 실행한다.
새 create/restore·hook·namespace continuation·자동 재시작은 실행하지 않는다.
일반 응답은 제한된 backlog에 남기며 정상 pump가 명시적으로 재개될 때 원 process
binding을 확인한다. backlog가 차면 읽기를 멈추고 완료를 추측하지 않는다.

## EngineSession 해제

닫힌 View나 실패한 View 생성의 EngineSession도 필수 의무를 먼저 배출한다.
`EngineRelease`는 남은 kind 인스턴스, standalone을 포함한 PTY, 기존 installation 및
private candidate의 회수 receipt를 보관한다. 실제 reap·plugin 응답이 끝나기 전에는
registry owner를 버리지 않는다. stream 보존 close는 구조·metadata를 지우지 않는다.

이 경계는 TaskScope 취소가 아니다. runner stop·task cancel·OS kill은 기존의 별도
명령 의미를 따른다. surface에 묶인 observer와 hook을 해제한 뒤 PTY를 retire하며,
TaskScope의 비취소 Drop 의미를 바꾸지 않는다. 소멸 시점의 미완 관측을 성공으로
보고하거나 raw 입력을 재전송하지 않는다.

## 대조 결과 확정

현재 runtime의 정확한 effect lease와 retained resource receipt가 있어야 준비·회수의
불명 결과를 다시 대조한다. 늦은 완료를 처리할 때도 stream, operation, attempt,
runtime epoch, engine incarnation, activation 및 physical generation을 구분한다.
대조 evidence payload와 결과 event/effect 갱신은 journal의 같은 commit 경계를 지난다.
그 뒤에 전체 batch publication ACK와 최초 command 결과 완료를 처리한다.

이 문서는 구현의 소유 계약을 설명한다. 장애 재현·플랫폼별 실행 결과는 별도의 검증
기록으로 확인하며, source에 이 경계가 있다는 사실이 실행 검증 통과를 뜻하지 않는다.
