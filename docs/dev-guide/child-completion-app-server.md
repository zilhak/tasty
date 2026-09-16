# Codex 부모의 child 완료 전달

부모 종류로 채널을 고른다. Codex 부모는 App Server `turn/start.toolOutput`, Claude 부모는
기존 completion-log/Monitor를 사용한다. child는 어느 CLI여도 된다. Stop의 last_assistant_message를 최대 4096자로 전달한다. 결과 요약이 없으면 빈 summary와 child surface result_reference를 전달하며 성공 결과를 합성하지 않는다. 완료 이벤트에
`terminal.tell`, 키 입력, `turn/steer`, `codex queue`를 사용하지 않는다.

## 연결

검증 기준은 Codex 0.154.0이다. 현재 sender는 initialize의 server 버전이 이 기준과 다르면
unbound로 남긴다. 버전 이름만으로 기존 TUI에 접근할 수 있다고 판단하지 않는다.
일반 TUI는 같은 home의 별도 daemon에 loaded되지 않을 수 있다. 이 경우 디스크 이력을
읽을 수 있어도 resume하지 않는다. 기존 endpoint를 지정한 `codex --remote` TUI에서
SessionStart가 관측된 뒤 바인딩한다. remote daemon이 현재 Tasty의 hook 환경을 갖지 않는 경우 `bind --register`로 명시 등록하며, 훅 관측과 구별해 출처를 기록한다. 이 경로는 재시작 후 명시 재등록이 필요하다. 서버를 자동 기동하거나 사용자 세션을 이전하지 않는다.

`unix://<절대경로>`는 Unix 소켓 위 WebSocket이다. JSONL이나 proxy 변환을 가정하지 않는다.
`ws://127.0.0.1:<port>`, `ws://localhost:<port>`, `wss://<host>`도 WebSocket으로 연결한다.
Windows에서는 Unix socket 바인딩을 명시적으로 거절하고 TCP/TLS를 사용한다. 복원 명령이 cmd/PowerShell/Git Bash에서 같은 인자를 받도록 이 플랫폼의 endpoint에는 셸 메타문자를 허용하지 않는다.
인증은 `auth_env`로 호스트 환경변수 이름만 기록한다. 토큰 자체는 상태/로그에 기록하지 않는다.

CLI는 `tasty codex completion <action>`, plugin IPC는 `codex.completion`, host IPC는
`terminal.completion`이다. endpoint를 설정하는 bind는 Network 권한도 검사하는 `terminal.completion_bind`로 분리되어 있다. 공통 인자 `surface`는 부모를 가리킨다.

| action | 추가 입력 | 결과 |
|---|---|---|
| bind | endpoint, thread_id, session_id, hook_session, 선택 auth_env | 검증 대기 바인딩 |
| status / diagnose | 선택 all; diagnose는 local_config_file | 세션·바인딩·구독·outbox 상태와 원인 |
| retry | event | 확정 미송신 pending/blocked 이벤트 재시도 |
| unsubscribe | subscription | 해당 부모의 구독 종료 |

CLI 플래그는 `--surface`, `--endpoint`, `--thread-id`, `--session-id`, `--hook-session`,
`--auth-env`, `--codex-home`, `--register`, `--all-parents`, `--event`, `--subscription`이다. `--surface` 생략 시 caller의
`TASTY_SURFACE_ID`를 사용한다. `status --all-parents`/`diagnose --all-parents`은 닫힌 부모의 기록까지 host journal 전체를 조회하므로 옛 surface가 사라진 뒤에도 취소·수락불명 근거를 볼 수 있다. 내부 producer는 session/subscribe/observe/route 및 실행 한정 오류 관측용 watch_error/observe_error 액션을
사용한다. host 진입점은 기존 SurfaceWrite 권한 게이트 뒤에 있다.

hook session_id, App Server sessionId, thread.id, surface generation은 별도 보관한다.
같은 sessionId만으로 부모를 고르지 않는다. 지원 기준 0.154.0 TUI의 root/fork에서 관측한 hook session_id와 실제 thread.id의 일치도 검증하며, 다른 대응은 추측하지 않고 unbound로 거절한다. 정확한 thread가 endpoint의 loaded 목록에
있는지 모든 페이지에서 찾고, initialize의 codexHome과 diagnostics의 실제 daemon PID도 대조한다. 이어서 read의 id/sessionId를 대조한 뒤, 설정 override 없는 resume으로
해당 연결의 이벤트 구독을 복원한다. read만 성공한 연결을 구독 완료로 표시하지 않는다.

## 수명과 영속성

호스트 데이터 루트의 `completion.sqlite3`가 단일 outbox 정본이다. SQLite exclusive locking과
synchronous FULL을 사용하고, 네트워크 쓰기 전에 in_flight 상태를 영속한다. sender는
호스트 background worker 하나이며 plugin dispatch와 별개다. 같은 host의 여러 윈도우는
서비스를 공유한다. 각 부모에서 송신 가능한 이벤트는 증가하는 event ID 순서로 한 건씩 처리한다. unknown/accepted 이력 재조회는 다음 시각으로 미뤄 뒤의 미송신 결과를 영구 차단하지 않는다. 따라서 수동 retry는 더 늦게 소비될 수 있으며 event ID·epoch로 원래 발생 순서를 구분한다. 다른 프로세스가 같은 journal을 동시에 소유하면 저장소 열기가 실패한다.

spawn 구독은 부모·child 실행/관계 세대를 가진다. release는 확정 미수락 이벤트를
cancelled로 남기며 이후 상태 이벤트 생성을 막는다. 전송 claim과 release가 같은 journal
잠금을 사용하므로 claim 전 해제는 송신되지 않는다. claim 이후는 in-flight로 취급한다.
tell은 별도 구독이며 release로 종료되지 않는다. 명시 unsubscribe, 대상 실행 세대 종료,
부모 바인딩 종료가 그 수명을 끝낸다. 새 관계에 옛 이벤트를 이관하지 않는다.

SessionStart로 등록한 종류가 Claude인 부모는 기존 로그를 사용한다. 새 host에서 기존
meta를 사용할 때에는 전경 프로세스도 대조한다. 부모를 판별할 수 없으면 로그로 추측해서
보내지 않고 unbound 이벤트로 보존한다.

| phase | 의미 |
|---|---|
| unbound | 검증된 목적지가 없음 |
| pending | 확정 미송신, 재시도 대기 |
| in_flight | 전송 claim 영속, 부분 쓰기/수락 가능 |
| accepted | 서버 turn 응답 수신, 영속·모델 소비 보장은 아님 |
| unknown | 쓰기/응답 유실로 수락 여부 불명 |
| recorded | 같은 event_id의 functionCallOutput 이력 확인, 모델 소비는 추정하지 않음 |
| blocked | 확정 RPC 거부 또는 자동 재시도 상한 |
| cancelled | 구독 종료 전에 미수락이 확정된 이벤트 |

연결 실패는 최대 8회, 2초부터 최대 60초의 backoff로 재시도한다. 버전/식별 불일치·Unix
소켓 교체는 재바인딩이 필요하다. 자동 탐색 상한으로 unbound가 된 바인딩도 원인을 해결한 뒤 같은 부모로 다시 bind하고 blocked 이벤트를 retry한다. 바인딩이 unbound인 동안 retry는 조용히 대기하지 않고 이 복구 순서를 오류로 안내한다. unknown/accepted는 자동 재전송하지 않는다. 설치본에서
busy ACK 뒤 interrupt/crash가 queued output을 잃은 실측이 있어 ACK를 영속 완료로
표시하지 않는다. server_response의 turn_id/turn_status/rpc_id는 받은 응답의 근거이며 영속 receipt나 중복 방지 키가 아니다. 이미 수락된 결과를 release로 회수했다고도 표시하지 않는다.

복구는 실제 historyMode와 RPC 지원을 따른다. legacy full read 또는 paginated items의
후속 cursor를 조회한다. summary/notLoaded·중간 오류·미지원은 미수락 증거가 아니다.
전체 저장 이력에 없어도 busy 큐에만 있을 수 있어 unknown을 유지한다. 조회 예산 초과와
cursor 순환도 불완전 이력으로 남는다. notification과 이력 확인은 모델 소비 확인과 다르다.

호스트 재시작은 옛 live surface 주소를 무효화하고 in_flight를 unknown으로 바꾼다.
새 SessionStart의 UUID만으로 현재 TUI가 예전 endpoint에 붙었다고 판단하지 않는다. 기존 바인딩의 새 SessionStart는 unbound로 보류하며, 실제 remote TUI를 확인한 `bind --register`가 저장된 논리 세션을 새 surface로 매핑하고 endpoint/thread를 재검증한다. 동일 UUID의 일반/private resume를 예전 daemon으로 연결하지 않는다.
옛 번호를 다른 세션이 먼저 써도 저장된 논리 부모·child의 구독을 종료하거나 그 세션에 붙이지 않는다. 부모 식별자를 확보하지 못한 구독은 재시작 시 `restart_parent_identity_unresolved`로 종료하고 기록을 보존한다. 부모는 알지만 child 식별자가 없던 구독은 새 관측을 중단하되 기존 영속 이벤트는 원래 부모에 전달할 수 있다. 늦게 도착한 다른 세션의 hook은 상태·종료 전이와 같은 journal 잠금에서 거절한다.
Codex reboot와 복원 명령도 검증된 remote endpoint/thread를 승계한다. Tasty의 reboot는 자신이 보낸 remote resume 명령의 새 TUI 배너를 확인한 뒤 캡처한 문맥으로 bind를 다시 요청한다. remote frontend의 종료는 daemon의 SessionEnd를 보장하지 않으므로 확인된 detach 시 전달을 보류한다. 훅 검토 등으로 새 배너가 확인되지 않으면 unbound로 남고 안내 입력도 보내지 않는다. 설치된 0.154.0은 resume 시 bypass 플래그가 있어도 훅 검토 화면을 표시할 수 있으므로 사용자 신뢰 결정을 자동 승인하지 않는다. 일반 수동 resume나 호스트 재시작 후에는 실제 연결을 확인한 명시 bind가 필요하다. hook 식별자에서
다른 thread를 유추하거나 다른 daemon에 저장 이력을 복제 resume하지 않는다.

## 설치와 검증

`tasty codex install --codex-home <절대 디렉터리>`는 해당 Codex home의 hook을 설치한다.
`--config-file <절대 파일>`은 명시 설정 파일을 선택하며 `--codex-home`과 함께 사용할 수 없다. 기존 설정을 읽거나 파싱할 수 없으면 덮어쓰지 않고 실패한다.
생략 시 host 환경 CODEX_HOME, 없으면 HOME/USERPROFILE 아래 .codex를 쓴다. remote
서버의 설정을 로컬 설치 성공만으로 갱신했다고 간주하지 않는다. 설치와 현재 부모의
SessionStart 관측, endpoint 바인딩, 서버 지원은 별도 상태다. status의 cli_version은
로컬 CLI이며 server는 연결된 App Server가 반환한 값이다. 직접 WebSocket을 사용하므로
proxy 실행파일은 연결 필수 조건이 아니다. `diagnose --local-config-file <절대 파일>`은 선택한 로컬 파일의 managed hook 존재와 trust metadata를 읽기만 한다. 생략 시 Tasty 호스트 환경의 Codex 설정을 읽는다. command hash의 실제 수락이나 remote daemon의 trust를 이 파일로 추정하지 않으며, 현재 세션의 hook 관측/명시 등록 출처는 별도로 표시한다.

도메인 시험은 `src/core/completion/tests.rs`, plugin 어댑터는
`crates/tasty-plugin-agent-common/src/completion.rs`, 호스트 진입점은
`src/adapters/ipc/handler/completion.rs`다. 격리 실행은
[self-verification](self-verification.md)을 따른다. Windows/macOS 실행 결과를 Linux
시험으로 대신하지 않는다. 결정 근거는 [ADR-0288](../adr/0288-codex-parent-tool-output-completion.md).

## 호환성 진단 경계

| 항목 | 확인과 미지원 처리 |
|---|---|
| initialize | experimentalApi=true는 diagnostics 식별·이력 복구용이며 toolOutput 자체의 요건으로 주장하지 않는다. 알림 opt-out은 요청하지 않는다. |
| daemon | initialize의 userAgent/codexHome과 diagnostics.process.id; 로컬 CLI 버전과 별도다. |
| toolOutput | 실제 turn/start 응답으로 판정한다. bind 성공만으로 지원을 확정하지 않는다. 명시 거부는 blocked다. |
| legacy history | includeTurns=true와 full items를 읽는다. summary/notLoaded는 불완전하다. |
| paginated history | thread/items/list와 cursor를 순회한다. 미지원·중간 오류·조회 상한은 unknown을 해소하지 않는다. |
| 구독 | 연결마다 loaded 대조 후 thread/resume; 연결 사이의 공백은 무알림 증거로 쓰지 않는다. |
| 인증 | 새 연결의 HTTP 401/403을 구분한다. 기존 연결 성공이나 토큰 파일 변경은 새 연결 인증 성공을 보장하지 않는다. |
| 운영체제 | Linux Unix WebSocket과 loopback TCP가 격리 검증 기준이다. Windows/macOS 및 TLS 실행 실측은 별개이며 Linux 결과로 대신하지 않는다. |

인증 환경변수는 실행 중 호스트의 환경에서 읽는다. 다른 셸에서 같은 이름의 값을 바꿔도
이미 실행한 호스트의 환경은 바뀌지 않는다. 토큰 갱신 후에는 해당 환경을 가진 호스트에서
다시 연결해야 한다. endpoint가 바뀌면 현재 TUI가 실제로 붙은 새 endpoint를 명시적으로 bind한다. 기존 thread.id/sessionId와 알려진 codexHome을 유지하고 새 daemon의 loaded 소유를 다시 검증한다. 이전 바인딩은 superseded로 보존하며 미완료 이벤트에는 origin_binding을 남긴다. accepted/unknown은 새 endpoint에서도 이력만 대조하고 재송신하지 않는다. 이 명령이 기존 TUI를 이동시키지는 않는다.

## 실행 종료와 지연 관측

release는 현재 host의 spawn/adopt 관계 세대에 속한 spawn 구독을 종료한다. 관계 세대는 부모 SessionStart 등록과 독립적이므로 부모 훅이 지연돼도 실제 관계 해제는 확정된다. 구독에는 세대를 영속 기록하되 live 관계 주소 맵은 재시작 때 복원하지 않는다. 재시작한 관계에 새 live 세대가 없다면 알려진 논리 부모·child 실행을 대조하며, 새 관계 세대가 있으면 그것과 다른 옛 영속 구독은 취소하지 않는다. unsubscribe는 현재 또는 막 종료된 논리 부모 식별자를 검증한다. 재시작 뒤 숫자 surface가 재사용돼도 옛 구독을 취소할 권한을 얻지 않는다. 원 관계의 target_exited 미송신 기록은 같은 소유자의 release로 취소할 수 있다.

Claude 오류 callback은 `watch_error`가 발급한 observer를 command에 보존한다. observer는 구독과 첫 child 실행 세대에 묶이며, 기동 전 등록은 첫 SessionStart에서만 세대를 채운다. `observe_error`는 같은 journal 잠금에서 현재 부모·child·구독 수명을 검증한다. release/실행 교체/호스트 재시작 뒤 늦은 callback과 observer 없는 구형 callback은 새 이벤트나 Claude 로그를 만들지 않는다.

child-profile의 await_session tell은 같은 UUID의 SessionStart가 돌아와도 다음 실행에 연결된다. 계획된 재개가 확인되면 SessionEnd 관측 유실과 무관하게 이전 실행 구독을 종료한다. 정상 Claude SessionEnd는 종료 전이를 한 번 기록한 후 metadata·복원 명령·legacy 알림을 정리하고, 중복 idle 관측으로 종료를 되돌리지 않는다. 다른 세션의 늦은 종료는 이 정리에도 진입하지 않는다.

## 프레임 수신 시간 제한

3초 I/O 예산은 WebSocket 메시지 바깥의 반복문뿐 아니라 소켓 Read/Write마다 남은 절대 시간으로 적용한다. TCP/TLS는 TLS 아래의 raw TCP stream을 감싸고 Unix도 같은 경계를 사용하므로, 유효한 continuation이나 부분 TLS 입력이 도착해도 예산을 연장하지 않는다. RPC 응답의 5초 마감은 다음 수신에 전달해 notification마다 새로 시작하지 않는다. 프레임·누적 메시지는 각각 8MiB로 제한한다. 한 endpoint의 미완성 응답이 시간 초과하면 단일 worker는 다음 부모를 처리한다.
