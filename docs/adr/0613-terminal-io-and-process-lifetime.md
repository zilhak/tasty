# ADR-0613: PTY 처리와 자식 프로세스 수명을 GUI에서 분리한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: terminal, pty, lifecycle
- **Group**: terminal

## Context

백그라운드 터미널이 많은 출력을 내도 사용자의 키 입력과 화면 조작이 늦어지지 않아야 한다. GUI 없이 실행할 때도 같은 터미널 동작이 필요하다.

## Decision

각 터미널의 PTY reader 스레드가 VTE 파싱과 grid 갱신을 맡는다. 메인 이벤트 루프는 렌더링과 이벤트 전달을 맡으며 VTE를 파싱하지 않는다.

터미널 상태는 터미널별 `Arc<Mutex<TerminalState>>`로 공유한다. reader는 8KB 청크를 처리한 뒤 락을 놓는다. 렌더링은 해당 터미널의 락을 한 번 잡고 필요한 값을 읽는다. 락 밖으로 grid 참조를 반환하지 않고, 클로저나 소유한 값으로 접근한다.

Windows 절전 복귀는 `WM_POWERBROADCAST`로 감지한다. 종료한 자식은 정리하고 살아 있는 PTY에는 현재 크기로 resize를 보내 입력 처리를 다시 시작하도록 유도한다. 복구가 어려울 수 있는 surface는 사용자에게 알린다. 응답이 없다는 이유로 자식을 강제 종료하거나 재생성하지 않는다.

PTY 자식은 PTY를 소유한 호스트와 수명을 함께한다. Windows는 `tasty-reaper`의 `KILL_ON_JOB_CLOSE` Job Object에 셸을 등록한다. 정상 닫기에서는 `PtyBackend::Drop`이 자식을 명시적으로 종료한다. Unix의 비정상 종료 처리는 PTY hangup과 SIGHUP에 의존한다. 플러그인과 터미널은 종료 구현을 공유하지만 Job Object는 각 소유자가 별도로 보관한다.

화면 없는 작업은 `pty.*` API와 별도 registry로 제공한다. Surface를 만들지 않고 실제 종료 코드를 보관한다. 사용자 화면에 표시할 때는 `pty.attach_surface`가 기존 Terminal을 새 surface ID로 옮겨 연결한다. Surface를 만드는 `terminal.*`와 역할을 구분한다.

출력 스캐너는 에이전트의 mark와 독립된 scan 커서를 사용한다. `take_since_scan_mark`는 읽기와 커서 전진을 한 번에 처리해 그 사이 출력이 빠지지 않게 한다. 에이전트의 set/read/parse mark API는 유지한다. `surface.read_since_scan_mark`는 TerminalRead 권한으로 호출하며 커서를 소비하므로 재전달 분류는 Mutate다. CLI 명령은 만들지 않지만 일반 로컬 IPC 호출을 차단하는 API는 아니다.

PTY EOF는 자식 프로세스 종료와 구분한다. EOF 뒤 parser는 종료가 확인되거나 소유권이 이전되거나 Terminal이 사라질 때까지 host를 다시 깨운다. 간격은 10ms부터 두 배씩 늘려 최대 500ms로 제한한다. EOF 직후 한 번의 try_wait가 아직 실행 중이라고 반환해도 나중에 종료를 확인할 수 있어야 한다.

## Consequences

기존 reader를 사용하므로 파싱 때문에 추가 스레드를 만들지 않는다. 백그라운드 터미널의 락은 사용자가 보는 터미널의 락과 독립이다. 새 접근자는 상태 락을 거쳐야 하며, poisoned mutex는 내부 상태를 회수한다.

Windows에서는 호스트 종료 시 `nohup`이나 백그라운드 작업도 종료한다. Unix에서는 SIGHUP을 무시한 프로세스가 남을 수 있다. Job 등록 실패는 경고를 남기고 기존 정리 방법을 사용한다. 클라이언트의 attach/detach는 서버가 소유한 셸의 수명을 바꾸지 않는다.

화면 없는 PTY는 사용자가 탭을 보고 정리할 수 없어 개수 제한과 idle 회수가 필요하다. registry, TerminalStore, wake 등록을 같은 함수에서 함께 정리한다.

scan 커서는 surface마다 하나이므로 소비자가 둘이면 서로 읽을 데이터를 가져갈 수 있다. 첫 호출은 현재 보관한 출력을 모두 반환하고 이후 호출은 새 출력만 반환한다. plugin은 델타를 자신의 제한된 버퍼에 모아 기존 에러 감지·중복 제거 판단을 유지한다. host와 plugin의 보관 상한은 같은 값이어야 한다.

PTY를 닫고 계속 실행하는 자식은 최대 500ms마다 host를 깨운다. targeted polling은 해당 터미널만 처리하지만 GUI에서 보이는 surface는 그 창을 다시 그릴 수 있다. targeted polling을 끄면 전체 터미널을 처리하고 모든 창을 다시 그릴 수 있다. Terminal drop은 parser join을 기다리지 않으며 스레드는 최대 한 대기 간격 뒤 종료한다.

## Alternatives Considered

- 메인 루프에서 처리량만 제한하면 파싱 CPU 부하가 계속 키 입력과 경쟁한다.
- 워커 풀은 스케줄링과 터미널별 순서 보장을 추가로 구현해야 한다.
- 스냅샷과 actor 요청만 사용하면 동기 출력 조회와 이벤트 수집까지 파서의 응답을 기다리게 된다.

- 데스크톱의 winit `suspended`/`resumed`만으로 OS 절전 복귀를 감지할 수 없다.
- 절전 복귀 대응을 모든 OS에 적용하면 문제가 확인되지 않은 Unix PTY까지 불필요하게 다시 그리게 한다.
- hang과 정상 idle을 구분할 수 없으므로 자동 kill·재실행은 데이터 손실 위험이 있다. 절전과 무관한 상시 폴링도 같은 판단 문제를 해결하지 못한다.

- Drop만 사용하면 크래시와 강제 종료에서 Windows 자식이 남는다. 다음 부팅에서 고아 프로세스를 찾아 종료하는 방식은 다른 호스트의 자식과 혼동할 수 있다.
- 화면 없는 작업을 `terminal.*` 옵션으로 넣으면 Surface 권한과 사용자 화면 상태가 불필요하게 개입한다.
- DAG runner의 subprocess는 stdout/stderr 캡처를 위한 구조라 PTY 화면 조회와 입력 전달을 대신하지 못한다. 종료 코드 watcher의 구현 방식만 공유한다.
- `TerminalRead`·`TerminalWrite`·`TerminalSpawn` 권한으로 설명할 수 있어 별도 Pty 권한을 만들지 않는다.

- 전진하지 않는 별도 scan mark는 반복 전송 비용을 없애지 못한다.
- 기존 read_since_mark에 선택 인자를 붙이면 구 host가 인자를 무시하고 다른 범위를 반환할 수 있다. 새 이름은 미지원 오류로 구별된다.
- 소비자별 커서는 더 일반적이지만 응답 형식과 호환 협상이 필요하다. observe sink나 host로 스캐너를 옮기는 방법은 현재 plugin 감지 구조를 더 크게 바꾼다.

- EOF 뒤 테스트 시간만 늘려도 추가 wake가 없으면 종료를 발견하지 못한다.
- 모든 host에서 주기적으로 모든 터미널을 검사하면 원인과 관계없는 터미널까지 반복 처리한다.
- parser가 직접 child.wait를 소유하면 기존 핸들의 kill·회수 및 take_child와 충돌한다. SIGCHLD는 Windows에 같은 기능이 없어 공통 해법으로 쓰지 않는다.

## Reconsideration Triggers

터미널 수가 늘어 스레드 자원이 문제가 되면 워커 풀을 검토한다. 프로파일에서 청크 락 경합이 확인되면 읽기용 스냅샷이나 이중 버퍼를 검토한다. 파서 라이브러리가 병렬 처리를 제공하는 경우에도 이 구조를 다시 비교한다.

Unix에서도 절전 뒤 같은 문제가 재현되거나, 창 라이브러리가 데스크톱 전원 이벤트를 지원하거나, ConPTY 개선으로 resize가 불필요해지면 전용 처리를 다시 검토한다. hang과 idle을 신뢰할 수 있게 구분할 때만 자동 복구 범위를 넓힌다.

화면 프로세스와 영속 PTY 데몬을 분리하면 실제 PTY 소유자에게 자식 수명을 연결한다. Unix 자식 결박 API나 ConPTY의 종료 보장이 추가되면 OS별 차이를 줄일 수 있는지 검토한다. 화면 없는 PTY의 상시 표시, DAG PTY backend 통합, 권한 분리 요구가 생겨도 registry 경계를 다시 검토한다. 개수·TTL·회수 지연이 장수명 작업에 맞지 않는 사례는 기본값과 설정 범위를 재검토하는 근거다.

scan API의 두 번째 소비자가 생기면 소비자별 커서를 도입한다. 호출이 사라지거나 host와 plugin의 출력 상한이 달라지면 스캐너가 실제로 새 데이터를 받는지 다시 점검한다.

host가 wake와 무관하게 모든 Terminal을 주기적으로 검사하게 되면 EOF 재요청의 중복 여부를 검토한다. EOF 뒤 장수명 자식이 흔해지면 visible·hidden surface 및 targeted polling on/off를 나누어 wake·redraw 비용을 측정한다.

## References

- [터미널 기능](../features/terminal/index.md)
- `crates/tasty-terminal/src/`
