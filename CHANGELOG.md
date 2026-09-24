# Changelog

이 문서는 CLI 명령, IPC 메서드, 매니페스트 스키마, 플러그인 인터페이스와 사용자가 직접 체감하는 동작의 변경을 기록한다. 단축키, 입력·창 처리, OS 권한 요청, 디스크에 저장하는 파일도 포함한다. 내부 리팩터링, 성능 개선, 테스트와 문서 변경은 `git log`에서 확인할 수 있다.

형식: [Keep a Changelog 1.1](https://keepachangelog.com/en/1.1.0/). 버전: [SemVer](https://semver.org/lang/ko/).

각 변경은 다음 카테고리 중 하나에 속한다:

- `Added` — 새 기능, 새 메서드/명령
- `Changed` — 동작 변경 (BREAK는 머리에 `(BREAK)` 표기)
- `Deprecated` — 폐기 예정, 아직 동작은 함
- `Removed` — 제거된 기능
- `Fixed` — 버그 수정

안정성 정책, 호환성을 깨는 변경의 분류와 폐기 절차는 [API 규칙](docs/dev-guide/api-conventions.md)의 ‘안정성 정책’ 절을 따른다.

## [Unreleased]

### Added

- **앱별 Shift+Enter 입력 설정**: 설정 → 터미널 → 입력에서 실행 파일명별 줄바꿈 문자(LF) 전송을 켜고 끄거나 규칙을 추가·삭제할 수 있다. Claude Code 플러그인은 최초 활성화에서 `claude`의 LF 규칙을 등록하며 기존 사용자 설정과 이후 수정·삭제를 보존한다. CLI `tasty settings get-input-rules`, `set-input-rule`, `remove-input-rule`, `initialize-input-rule`과 대응 IPC를 제공한다. Windows 기본 키 전송은 Shift+Enter·Ctrl+J의 Win32 키 정보를 보존한다.

- **파일 형식 판별 규칙의 적용 결과를 조회할 수 있다.** `tasty file-handler detectors`(IPC `file_handler.detectors`, local-only)는 각 판별기의 최종 값(`display_name_i18n_key` · `icon` · `disabled` · `rules`)과 출처별 원본 `contributions`(`host` · `plugin:<id>` · `user`)를 함께 반환한다. 기본값, 플러그인, 사용자 설정이 어떻게 합쳐졌는지 확인할 수 있으며 `contributions`는 설치 순서로 나온다. 조회는 규칙을 변경하지 않는다.
- **플러그인이 자기 웹뷰의 외부 링크를 열 수 있다.** 새 IPC `webview.open_external { surface_id, url }`은 `surface.write` 권한을 요구하며 플러그인만 호출할 수 있다. 외부 호출자는 `-32016` plugin-only 오류를 받는다. 호스트는 호출 플러그인이 해당 서피스의 소유자인지, URL에 스킴이 있는지, `javascript:`가 아닌지 확인한 뒤 OS 기본 핸들러로 열고 `{ "opened": bool }`을 반환한다. 번들 마크다운 플러그인도 이 메서드를 사용한다. 외부 링크를 클릭하면 기본 브라우저가 열리는 동작은 그대로이며, debug 빌드에서는 `TASTY_DEBUG_OS_OPEN_LOG`에 기록한다. [ADR-0030](docs/adr/0030-bundled-plugin-data.md).
- **요청 거절 건수를 사유별로 조회할 수 있다.** `tasty list pressure`(IPC `system.pressure`)에 열두 번째 항목 `gate_refusals`가 추가됐다. `judged`는 진입 검사에서 판단한 요청 수이고, `permission_denied`(-32001), `cap_blocked`(-32007), `throttled`(-32010)는 각 사유로 거절한 수다. 인스턴스 시작 이후의 누계이며 재시작하면 0이 된다. 기존 항목은 유지한다. [ADR-0008](docs/adr/0008-ipc-pressure-observability.md).
- **`tasty new workspace --surface <ID>`로 새 워크스페이스를 만들 창을 지정할 수 있다.** IPC `workspace.create`의 `surface_id`에 대응하는 CLI 인자를 추가했다. 지정한 서피스가 속한 창에 워크스페이스를 만들며, 소유 창을 찾지 못하면 오류를 반환한다. 생략하면 이전처럼 사용자가 보고 있는 창에 만든다. `TASTY_SURFACE_ID`로 자동 지정하지 않는다. [ADR-0043](docs/adr/0043-cli-errors-and-diagnostic-logs.md).
- **CLI가 호스트 오류의 추가 정보(`error.data`)를 표시한다.** 호스트가 반환한 `storage_failure`, `reason`, `approval_id` 등의 정보를 버리지 않고 stderr 둘째 줄에 `data: <한 줄 JSON>`으로 그대로 출력한다. 첫 줄의 `Error (<code>): <message>`와 종료 코드 1은 유지하며, `data`가 없으면 출력도 이전과 같다. `tasty events follow`와 `tasty plugin audit-follow`에도 둘째 줄이 추가된다. 두 스트리밍 명령의 첫 줄은 기존 `Error: Error (<code>): <message>`를 유지한다. [ADR-0043](docs/adr/0043-cli-errors-and-diagnostic-logs.md).
- **`tasty list gpu-stats`(IPC `system.gpu_stats`)에 탐색기 화면 상태 수가 추가됐다.** `windows[]`의 `explorer_views`는 해당 창이 보관하는 탐색기 화면 상태 수다. 탐색기 서피스를 처음 그릴 때 생성하고 닫을 때 제거하므로, 열고 닫은 뒤 값이 돌아오는지 확인해 누수를 조사할 수 있다. main 이외 창은 `null`이다. 메모리 soak의 `s6`은 탐색기·이미지·마크다운을 보이는 분할에 열고, `explorer_views`와 `egui_mesh_targets`로 탐색기와 이미지의 렌더 여부를 확인한다. 기존 필드는 유지한다.

- **일시적인 API 오류로 멈춘 Claude 세션을 자동 재개할 수 있다.** `overloaded`나 `server_error`로 턴이 끝나면 설정한 시간 뒤 해당 서피스에 재개 문구를 제출한다. 기본값은 꺼짐이다. 설정 › 플러그인 › Claude Code에서 `auto_resume_enabled`, `auto_resume_delay_secs`(기본 10, 1초~하루), `auto_resume_max_attempts`(기본 5)로 설정한다. 전송 직전에 설정, `idle` 상태, 같은 PID의 전경 Claude 프로세스(Windows의 `claude.exe` 포함), 사용자 입력 여부를 다시 확인한다. 턴 시작 이후 사용자가 키를 누르거나 붙여넣었다면 초안을 보호하기 위해 보내지 않는다. 요청 한도나 인증 실패는 재개하지 않으며 연속 시도 상한에 도달하면 한 번 알린다. 전송 횟수는 서피스 메타데이터 `claude-auto-resume-count`에 남는다. [ADR-0041](docs/adr/0041-agent-state-and-completion.md).
- **느린 요청 목록에 처리 결과가 표시된다.** `tasty list pressure`(IPC `system.pressure`)의 `slow_requests`에서 `host`에 `outcome`(`ok`/`error`, 플러그인 호출과 같은 값)과 `error_code`(호출자에게 반환한 JSON-RPC 오류 코드)가 추가됐다. 큐 대기 중 만료된 요청(`-32067`), 권한 검사에서 거절한 요청(`-32001`), 성공한 요청을 구분할 수 있다. 아직 응답하지 않았다면 둘 다 `null`이다. 기존 필드는 유지한다. [ADR-0008](docs/adr/0008-ipc-pressure-observability.md).
- **새 연결을 받아들이기까지의 대기 시간 상한을 조회할 수 있다.** Tasty는 새 연결이 없으면 100 ms 기다린 뒤 다시 확인한다. 명령마다 연결하는 CLI 등은 요청을 읽기 전에도 기다릴 수 있지만, 이전에는 이 시간이 조회값에 나타나지 않았다. 이제 `tasty list pressure`(IPC `system.pressure`)의 `connections`에 `accept_waits`(기록 수), `accept_wait_bound_us_sum`, `accept_wait_bound_us_max`, `accept_wait_bound_us_mean`을 반환한다. 정확한 연결 도착 시각은 알 수 없어 마지막으로 새 연결이 없음을 확인한 뒤의 경과 시간을 사용한다. 따라서 실제 대기 시간이 아닌 상한이다. 기존 필드와 항목 수는 유지한다. [ADR-0008](docs/adr/0008-ipc-pressure-observability.md).

- **원격 attach가 화면 데이터 손실을 감지하면 재연결한다.** 이전에는 수신 측이 손실 통지를 로그로만 남기고 불완전한 화면을 계속 표시했다. 손실 통지를 지원하고 요청한 연결에서는 이제 GUI mirror가 경고 토스트를 띄우고 재연결하며, mesh 캐시를 비워 텍스처도 다시 받는다. 프로필 없이 연결한 mirror에도 적용된다. 터미널 출력의 `stream` 표지도 바뀌므로 에이전트가 이전 커서로 누락 구간을 이어 읽으려 하면 불일치 오류를 받는다.

  CLI `tasty attach`도 손실 통지를 요청하고 stderr에 알린 뒤 재연결한다. 화면을 출력하고 끝내는 모드는 한 실행에서 최대 세 번, `--raw`는 제한 없이 재연결한다. `--send` 입력은 재전송하지 않는다. 클립보드 이미지 업로드는 처리 결과를 알 수 없어 실패로 끝내며 자동 재전송하지 않는다.

  손실 통지가 늦어지던 문제도 수정했다. 이전에는 다음 전송 성공이나 수신 데이터가 있어야 통지해, heartbeat를 보내지 않는 CLI가 손실을 모를 수 있었다. 이제 수신 측이 손실 이전 데이터를 다 읽으면 바로 통지가 이어진다. [ADR-0023](docs/adr/0023-attach-state-sync-and-forwarding.md).

- **원격 화면 전송의 손실과 대기량을 조회할 수 있다.** `tasty list pressure`(IPC `system.pressure`)의 일곱 번째 항목 `stream_push`에 네 값이 추가됐다. `frames_dropped`는 연결이 유지된 동안 버린 프레임의 누계, `clients_lagged_out`은 과도하게 뒤처져 끊은 연결의 누계, `backlog`는 현재 연결들에서 전송을 기다리는 프레임 수의 합, `sink_capacity`는 연결 하나의 대기 상한이다. 느린 수신자가 전송을 막지 않도록 프레임을 버리는 동작을 확인할 수 있다. 네 값 중 `backlog`는 감소할 수 있다. 연결별 연속 손실 수는 전송 성공 시 0으로 돌아가 조회 시점에 따라 손실이 가려지므로 반환하지 않는다. [ADR-0023](docs/adr/0023-attach-state-sync-and-forwarding.md).

- **SQLite 설정의 요청값과 실제값을 비교할 수 있다.** `memory.db`와 `state.db`를 열 때 요청하는 `journal_mode=WAL` 등 네 설정은 호출이 성공해도 적용값이 다를 수 있다. 이전에는 경고 로그만 남겼지만, 이제 `tasty list pressure`(IPC `system.pressure`)의 여섯 번째 항목 `db_pragmas`에서 `requested`와 `effective`를 함께 반환한다. 하나라도 적용되지 않았으면 `degraded: true`가 된다. 이는 데이터베이스가 열린 상태에서 사용하는 설정 차이이며, 열기 실패를 뜻하지 않는다. 파일 데이터베이스는 `wal`, 메모리 데이터베이스는 `memory`를 정상으로 본다. 파일 데이터베이스가 `memory`로 열린 경우도 경고와 degraded 상태로 표시한다. 열지 않은 데이터베이스는 `null`이며 headless 인스턴스의 `state_db`는 항상 `null`이다.

- **메모리 저장 실패의 원인을 구조화해 반환한다.** 기존 `-32603 memory db error: …` 코드와 문장은 유지하고 `error.data.storage_failure`를 추가했다. 값은 `busy`, `disk_full`, `io`, `corrupt`, `permission_denied`, `other`이며 데이터베이스 초기화 오류와 같은 분류를 사용한다. 오류 문장을 파싱하지 않고 원인을 구분할 수 있다. 실패한 쓰기는 quota에 반영하지 않고 `memory.changed` 알림도 발생시키지 않는다.

- **작업 완료와 barrier 종료 이벤트를 구독할 수 있다.** 플러그인은 `agent.task_finished`(`workspace_id`·`task_id`·`state`)와 `agent.barrier_closed`(`workspace_id`·`name`·`count_required`)를 구독할 수 있다. 이전처럼 상태를 반복 조회하거나 커서가 없는 훅 로그만 추적할 필요가 줄었다. `state`는 `succeeded`, `failed`, `cancelled`, `skipped` 중 하나다. 시작·대기 같은 중간 전이나 조회 시 판단하는 lease 만료는 알리지 않는다. 실패 이유에는 명령 출력이 섞일 수 있어 모든 구독자에게 보내지 않으며, 필요하면 `task_id`로 조회한다. 두 이벤트는 Experimental 등급이므로 minor 버전에서 키나 payload가 바뀔 수 있다. 매니페스트의 `event_subscribe`가 해당 키를 허용해야 구독할 수 있다. `agent` 네임스페이스는 예약되어 플러그인이 이벤트를 발행할 수 없으며 구독은 허용된다.

- **이벤트를 커서로 이어 읽는 명령이 추가됐다.** `tasty events fetch --offset <N> [--max N] [--filter 'agent.*'] [--wait-ms N]`(IPC `events.fetch`)와 이를 반복하는 `tasty events follow`를 제공한다. `follow`는 이벤트 하나를 JSON 한 줄로 출력하므로 셸의 `while read`에서 처리할 수 있다. 소비자가 마지막 위치를 보관해 다음 요청에 전달하며, 아직 보존된 데이터는 같은 위치로 다시 조회할 수 있다. 조회 결과는 새 이벤트나 보존 범위 변화에 따라 달라질 수 있다.

  이벤트는 메모리에만 한정된 수로 보관한다. 오래 연결이 끊겨 데이터가 삭제됐다면 누락 건수를 알리며 CLI의 손실 안내는 stderr로 출력한다. Tasty를 재시작하면 데이터가 사라지고 위치는 0부터 다시 시작한다. 응답의 `epoch`으로 재시작 전 커서인지 구분할 수 있다. 서버가 소비자별 커서나 별도 이벤트 큐를 보관하지는 않지만, 대기 요청은 워커 스레드를 사용한다. 기존 장기 대기 명령 다섯 개(`agent task-await` 등)는 유지한다.

- **터미널 출력을 소비자별 커서로 이어 읽을 수 있다.** 이전에는 `tasty set mark`가 설정하는 터미널당 하나의 마크를 공유해, 한 에이전트가 마크를 바꾸면 다른 에이전트의 읽기 시작점도 바뀌었다. 이제 IPC `surface.read_since_mark`에 `cursor`(읽을 위치)와 `stream`(터미널 출력 스트림의 식별값)을 전달할 수 있다. Tasty는 소비자별 커서를 보관하지 않으므로 각 소비자가 독립적으로 읽고 재연결할 때도 마지막 위치를 전달할 수 있다. `max_bytes`로 한 번에 받을 양을 제한한다.
- **터미널 출력 버퍼에서 삭제된 구간을 응답으로 알린다.** 출력이 1 MiB를 넘으면 오래된 바이트를 삭제한다. 이전에는 마크가 삭제된 구간에 있어도 남은 버퍼의 처음부터 반환해 누락을 알 수 없었다. 이제 `surface.read_since_mark`는 `skipped`(삭제된 바이트 수), `retention_start`/`retention_end`(보존 범위), `next_cursor`(다음 위치), `raw_bytes`, `stream`을 함께 반환한다. `text`와 인자 없는 호출의 동작은 유지한다. 색 코드 제거나 잘못된 문자 대체로 길이가 달라질 수 있으므로 `text` 길이 대신 `next_cursor`를 사용한다. 재생성된 터미널에 이전 스트림의 커서를 쓰면 거절한다. 원문은 계속 메모리에만 보관한다.
- **`tasty read since-mark`에 커서 인자가 추가됐다.** `--cursor <N> --stream <S>`로 읽기 위치를 지정하고 `--max-bytes <N>`로 반환량을 제한한다. 첫 호출은 위치 없이 한 뒤 응답의 `next_cursor`와 `stream`을 다음 호출에 전달한다. `--cursor`는 `--stream`을 함께 지정해야 한다. 커서 인자가 없으면 요청과 동작은 이전과 같다. 구버전 호스트는 이 인자를 무시하고 공유 마크에서 읽으므로, CLI는 먼저 `ipc.output-cursor` 지원을 확인한다. 지원하지 않으면 해당 읽기 요청을 보내지 않고 stderr에 `{"error":{"kind":"unsupported_capability",…,"sent":false}}`를 출력한 뒤 종료 코드 1로 끝난다.
- **CLI에서 응답 대기 제한을 지정할 수 있다.** `tasty --response-timeout-ms <MS> <명령>`은 서버에 응답 대기 시간을 전달한다. 실행이 시작된 요청이 만료되면 `-32061`과 종료 코드 1을 받는다. 결과가 불명확하고 요청은 계속 실행될 수 있으므로 변경 요청을 다시 보내기 전에 상태를 조회해야 한다. 큐에서 실행 전에 만료되면 `-32067`로 끝나며 요청이 실행되지 않았으므로 재시도할 수 있다. 생략하거나 `0`을 주면 이전처럼 무기한 대기한다. 요청 한 번으로 끝나는 명령만 지원한다. `events follow`, `plugin audit-follow`, 원격 attach처럼 반복 조회하거나 스트리밍하는 명령은 이 플래그를 받으면 요청을 보내지 않고 종료 코드 2로 거절한다. 기능을 지원하지 않는 이전 Tasty에도 요청을 보내지 않고 `sent:false`로 거절한다.
- **주기적인 출력 검사에 사용하는 스캐너 커서를 분리했다.** IPC `surface.read_since_scan_mark`는 `surface.read_since_mark`와 별도 커서를 사용하고 읽은 끝으로 자동 전진한다. 이전에는 번들 Claude Code 플러그인의 오류 감시가 에이전트 마크를 공유해, 마크가 없으면 매번 최대 1 MiB 전체를 반환하고 `tasty set mark`를 호출하면 감시 시작점도 바뀌었다. `set mark`, `read since-mark`, `parse-since-mark`는 그대로 동작한다. 사용자가 스캐너 커서를 전진시키면 감시할 출력을 놓칠 수 있어 이 플러그인용 메서드에는 CLI 명령을 제공하지 않는다.
- **`list info`에 지원 기능 목록이 추가됐다.** `tasty list info`(IPC `system.info`)의 `capabilities`는 `{name, version}` 목록을 반환한다. 패키지 버전만으로 구분하기 어려운 빌드별 기능 지원을 확인할 수 있다. 기존 클라이언트는 추가 필드를 무시하며, 서버 단위 정보이므로 `window.list`에 반복하지 않는다.

  선언하는 기능은 아홉 가지다. 기능 목록 자체(`ipc.capabilities`), 메서드의 재시도 특성(`ipc.method-effect`), 동결 API 포함 여부(`ipc.method-since`), 응답 대기 제한(`ipc.response-timeout`), 실행 전 만료 거절(`ipc.response-timeout.not-run`), 멱등 키(`ipc.idempotency-key`), 스트림 프로토콜 버전(`ipc.stream`), 프레임 손실 통지(`ipc.stream.loss-notify`), 소비자별 터미널 출력 커서(`ipc.output-cursor`)다. 스트림 버전은 서버가 실제 동등 비교에 사용하는 상수다. CLI는 커서 인자처럼 새 기능이 필요한 요청에 한해 먼저 지원을 조회하고, 선언이 없으면 해당 요청을 보내지 않는다.
- **debug 빌드에서 입력란에 텍스트를 주입할 수 있다.** `tasty debug inject egui-text --text <s>`(IPC `debug.inject_egui_text`)는 명령 팔레트 검색란 등 포커스된 입력란에 텍스트 이벤트를 전달한다. 키 이벤트만 보내는 `inject egui-key`와 구분된다. 창 관리자가 없는 테스트 환경에서도 검색어를 입력해 결과를 확인할 수 있다. 빈 문자열과 제어문자는 주입하지 않고 `injected: false`를 반환한다. Enter·Tab·Backspace는 계속 `inject egui-key`를 사용한다. 사용자 조작을 재현하는 기능이므로 debug 빌드에서만 제공한다.
- **훅 핸들러를 CLI로 조회하고 수정할 수 있다.** 이전에는 `IpcSequence` 핸들러의 설정 UI와 목록 조회가 요약만 보여 주고, 수정하려면 `~/.tasty/hook-handlers.toml`을 직접 편집해야 했다. 이제 `tasty hook-handler get --id <id>`로 전체 동작을 읽고 `tasty hook-handler upsert --id <id> [--calls <json> | --action <json>] [--source ...] [--priority N] [--display-name-key K] [--disabled <bool>]`로 같은 ID의 핸들러를 수정한다. IPC `hook_handler.get`, `hook_handler.upsert`, `hook_handler.remove`는 모두 로컬 전용이다. 생략한 속성과 기존 훅 참조는 유지하며 `--calls`는 `webhook register --sequence`와 같은 형식을 사용한다. 변경은 즉시 저장하고 저장 실패는 오류로 반환한다.

  `tasty hook-handler remove --id <id>`는 사용자가 등록한 부분만 삭제한다. 같은 이름의 호스트·플러그인 기본값이 다시 적용되면 `still_present`로 알린다. 이미 등록한 웹훅은 등록 당시 동작을 보관하므로 새 동작을 쓰려면 다시 등록해야 한다. 설정 UI의 시퀀스 편집기는 아직 제공하지 않는다.

- Codex 훅 설치에서 Codex home 또는 정확한 설정 파일을 지정할 수 있다. 둘은 함께 쓸 수 없고, 기존 설정을 읽을 수 없으면 덮어쓰지 않고 실패한다.

- 사용자 언어팩으로 플러그인의 도구 라벨과 내부 UI 문구를 함께 덮어쓸 수 있다. 내장 언어는 `lang/plugins/<plugin-id>/<code>.toml`, 새 언어팩은 `lang/<code>/plugins/<plugin-id>.toml`을 사용한다. 설치본 영어·선택 언어 위에 사용자 파일을 얹으며, 수정 후 재시작하면 적용된다. 설치 자산을 수정하지 않아 플러그인 업그레이드 후에도 보존된다.

- **오류 문구 없이 멈춘 Claude 자식도 부모에게 알린다.** 이전에는 오류가 표시된 뒤 출력이 멈춘 경우만 감지하고 `stale` 자식은 제외해, 훅 없이 승인 입력을 기다리는 자식을 놓쳤다. 이제 출력이 2분 동안 변하지 않고 상태가 `active` 또는 `stale`이면 알림 로그에 기록한다. 화면에 오류가 있을 때의 30초 기준은 유지한다. 오류가 있으면 해당 문구를 덧붙이고, 없으면 출력과 완료 신호가 없다고 알린다. 기존 이벤트 키 `claude-error-stalled`와 빈도 제한(정지 구간당 1회, 서피스당 최소 5분)은 유지한다. 긴 추론도 정지로 감지할 수 있지만 부모가 계속 기다리는 상황을 줄이기 위해 허용한다.
- **자식 Claude의 권한 모드를 지정할 수 있다.** `tasty claude launch|spawn|respawn|reboot|child-profile`에 `--permission-mode acceptEdits|auto|bypassPermissions|manual|dontAsk|plan`이 추가됐다. IPC 인자는 `permission_mode`다. 호출 인자가 없으면 플러그인의 기본 설정을 사용하며, 그 값이 물려받음이면 CLI 플래그를 추가하지 않고 기존 Claude Code 설정을 따른다. Claude Code에는 별도 샌드박스 설정이 없어 Codex처럼 자동으로 승인 생략을 적용하지 않는다. 전역 기본값은 설정 › 플러그인 › Claude Code의 ‘자식 세션 기본 권한 모드’이며 기본은 물려받음이다. 호출별 플래그가 우선한다. `--profile`/`--profile-file`의 JSON에서 `permissions.defaultMode`를 지정했다면 `--permission-mode`와 함께 사용할 수 없다. `reboot`/`child-profile`의 지정값은 해당 재시작에만 적용하고 탭 복원에는 저장하지 않는다.
- **Codex 승인 대기를 `needs_input`으로 표시한다.** `Would you like to run the following command?` 승인 화면에서 `tasty codex state`와 `terminal.state`가 `needs_input`을 반환한다. 포커스되지 않은 대상에는 기존 노란 탭·워크스페이스 표시가 나타나고 `spawn`/`tell` 호출자의 알림 로그에도 기록한다. 이전의 ‘Codex CLI에 대응 이벤트가 없다’는 설명을 codex-cli 0.154.0에서 확인해 바로잡았다. `tasty codex install`의 훅이 세 개에서 여섯 개로 늘었으며, `PermissionRequest`는 대기 진입, `PostToolUse`는 도구 완료 후 실행 중 복귀, `Interrupt`는 거절·Esc·Ctrl-C에 따른 대기 종료를 처리한다. 기존 사용자는 install을 다시 실행해야 한다. 승인 직후가 아니라 도구가 끝난 뒤에야 실행 중으로 돌아오며, 일반 질문 입력은 감지하지 않는다.

- **원격 attach에서 마크다운 문서를 표시한다.** 이전의 빈 mirror 화면 대신 원격 문서 원문을 받아 로컬 테마로 렌더링한다. 오른쪽 위 새로고침 버튼으로 다시 읽을 수 있다. 원격 문서 변경 시 자동으로 내용을 바꾸지는 않고 버튼 색으로 알리며, 원격이 headless이면 이 변경 알림은 없다. 상대 경로 이미지·파일 링크와 주소창으로 다른 파일 열기는 지원하지 않는다. 로컬 마크다운 플러그인이 꺼져 있으면 활성화될 때까지 기다린다. 연결이 끊기면 연결 끊김 화면을 표시하고 재연결 뒤 원문을 자동으로 다시 받는다. 플러그인용 호스트 메서드 `markdown_mirror.content_request`(`fs.read`)가 추가됐다.

- **느린 요청 한 건의 처리 시간을 추적할 수 있다.** 이전의 분포값으로는 특정 요청의 큐 대기, 호스트 처리, 플러그인 대기를 구분하거나 플러그인 로그와 연결하기 어려웠다. 이제 요청별 `request_seq`를 부여하고 `tasty list pressure`(IPC `system.pressure`)의 열한 번째 항목 `slow_requests`에 합계가 `threshold_us`(100 ms) 이상인 요청을 기록한다.

  각 기록은 `request_seq`, `host`(`method`·`caller`·`queue_wait_us`·`host_us`), `total_us`를 포함한다. 플러그인에 전달했다면 `plugin_hops`에 플러그인, `host_request_id`, `wait_us`, `outcome`(`ok`/`error`/`expired`/`cancelled`)도 남긴다. 응답 없이 만료된 호출도 기록하며, 기존 분포에는 이 호출이 포함되지 않는다. 메모리에 최근 32건을 보관하고 `admitted`는 인스턴스 시작 이후 기록한 누계다. 조회 요청 자체와 요청 내용·토큰·멱등 키·원래 `id`는 기록하지 않는다. 플러그인 오류·무응답 경고 로그에도 `id=<host_request_id>`와 `request_seq=<번호>`를 추가했다. 플러그인 요청 형식과 기존 열 항목은 유지한다. 필드 추가이므로 BREAK 변경이 아니다.

- **요청 큐의 사용량·처리 상태와 멱등 키 처리 건수를 조회할 수 있다.** `tasty list pressure`(IPC `system.pressure`)에 다음 세 항목이 추가됐다.

  `queue_admission`은 현재 큐 사용량(`queued_bytes`·`queued_commands`·`queued_injected`), 최고 바이트 사용량(`peak_bytes`), 큐 진입 전에 거절한 누계(`refused_bytes`·`refused_depth`), 상한(`limit_bytes`·`limit_injected_depth`)을 반환한다. IPC 서버가 없으면 `null`이다.

  `queue_dispatch`는 처리 회차(`rounds`), 회차별 건수·시간 제한으로 중단한 횟수(`rounds_stopped_by_count`·`rounds_stopped_by_time`), 실행 전 만료한 명령(`expired_before_run`), 시작한 명령(`started`), 실행 중이며 호출자가 아직 기다리는 요청(`in_flight`)과 최댓값(`in_flight_max`)을 반환한다.

  `keyed_requests`는 멱등 키 요청의 `executed`, `replayed`, `conflicted`, `discarded`, `in_flight`를 센다. 여기서 `in_flight`는 같은 키로 이미 진행 중인 요청에 합류한 횟수이며 `queue_dispatch.in_flight`와 다르다. 기존 일곱 항목은 유지하며 필드 추가이므로 BREAK 변경이 아니다.

- **IPC 처리 지연과 연결 사용량을 조회할 수 있다.** 로컬 전용 `tasty list pressure`(IPC `system.pressure`)는 인스턴스 시작 이후의 누계를 반환한다. 이전에는 내부에서만 측정하던 시간을 조회할 수 없었다. 응답은 열두 항목이며, `db_pragmas`, `stream_push`, `queue_admission`, `queue_dispatch`, `keyed_requests`, `slow_requests`, `gate_refusals`는 위의 각 추가 기능에 설명했다.

  시간 측정의 범위는 서로 다르다. `queue_before_gate`는 권한 검사 전 대기를 측정해 나중에 거절할 요청도 포함한다. `handler_after_gate`는 검사 후 실제 처리한 요청, `plugin_round_trip`은 응답이 도착한 플러그인 호출의 왕복 시간, `db`는 성공한 저장 작업의 시간을 측정한다. 취소·만료된 플러그인 호출은 왕복 분포에 포함하지 않는다. 부팅 시 WAL 처리 미완료 횟수는 따로 센다. 이 값으로 큐 대기, 호스트 처리, 플러그인, 저장 작업 중 지연이 발생한 부분을 조사할 수 있다. 검사 후 창 처리 경로에서 바로 응답하는 요청도 있으므로 앞의 두 건수를 빼서 거절 건수로 해석하면 안 된다. 관측이 없는 평균은 `null`이며, 시간 구간별 집계가 아닌 전체 누계라 과거의 최대 지연이 계속 남는다. 호출자별로 분리하지 않은 값이므로 플러그인에는 제공하지 않는다. 플러그인의 자기 사용량 조회 `telemetry.*`는 유지한다.

  `connections`는 현재 연결 수 `live`, 최고 연결 수 `live_max`, 상한 `limit`, 수락 누계 `accepted`, 포화 거절 누계 `refused_saturated`를 반환한다. 이 중 누계·순간값에서는 `live`만 줄어들 수 있으며, 별도의 accept 대기 평균은 파생값이라 감소할 수 있다. 요청을 보내지 않는 attach 연결도 포함한다. 포화되면 요청을 읽기 전에 거절하므로 이 거절은 처리 시간 항목에 나타나지 않는다. `live`와 `limit`, 증가하는 `refused_saturated`로 연결 포화를 확인할 수 있다.

  시간 항목 중 세 개에는 `*_hist` 분포도 제공한다. `bounds_us`는 10 µs부터 1초까지 열한 경계값이며 `counts`의 마지막 칸은 1초 초과 건수라 칸 수가 하나 더 많다. 각 칸은 서로 겹치지 않아 합이 전체 관측 수다. 대부분이 느린지 일부만 느린지 구분하는 데 사용한다. 구간 단위 측정으로 정확한 p99 등을 구할 수 없어 분위수는 반환하지 않는다. 다른 항목에는 분포가 없다.

- **플러그인이 호스트 요청 큐의 손실 건수를 알 수 있다.** 호스트가 큐 포화로 요청을 버린 뒤 처음 수락한 요청에 `dropped_requests`를 함께 전달한다. 0이면 필드를 생략한다. 이전에는 호스트 로그로만 남아 플러그인이 손실을 알 수 없었다. SDK의 `Plugin::on_host_dropped_requests`로 통지를 처리하고 `HostHandle::dropped_by_host()`로 누계를 읽을 수 있다. 기본 콜백은 아무 동작도 하지 않는다. 선택 필드이므로 기존 플러그인도 그대로 동작한다.

- **완료 알림 로그에 삭제된 바이트의 누계가 추가됐다.** `<parent_home>/notify/<caller_surface>.log`의 크기가 쓰기 전 256 KiB 이상이면 파일을 비우며, 같은 폴더의 `<caller_surface>.log.meta`에 `retention_start=<누계>`를 기록한다. 소비자는 누계와 파일 내 위치를 함께 보관해 재개 시 누락을 확인할 수 있다. 이전에는 삭제량이 작성 플러그인의 진단 로그에만 남았다.

  정상적으로 메타 파일 잠금을 얻으면 Claude·Codex 자식의 쓰기와 비우기를 조정한다. 비우기용 잠금은 최대 200 ms 기다리며, 실패하면 알림 기록을 계속하기 위해 잠금 없이 비우고 해당 손실은 누계에 반영하지 않는다. 메타 파일 열기나 갱신 실패도 있어 소비자가 모든 손실을 감지한다고 보장할 수는 없다. 완료 로그의 형식·경로와 `tail -F`를 쓰는 Monitor는 유지한다. 메타 파일은 첫 기록 때 빈 파일로 생성하며 다음 인스턴스 시작 시 완료 로그와 함께 정리한다.

- **튜토리얼 완성판** — 화면 구조 5단계, 별도 워크스페이스의 분할·탭 전환 실습, 실제 단축키를 반영한 명령 안내를 제공합니다. 완료·중단 위치를 기억하고 다시 보기를 지원하며, 대상 유실 안내와 터미널 Escape 처리를 보완했습니다.

### Changed

- **서피스를 지정한 승인 요청은 해당 워크스페이스에 연결된다.** `approval.request`(CLI `tasty approval request`)에서 `workspace_id`가 없고 `surface_id`만 있으면, 이전에는 활성 워크스페이스에 기록했다. 이제 `workspace_id`, `surface_id`가 속한 워크스페이스, 활성 워크스페이스 순으로 결정한다. 둘 다 생략한 호출과 존재하지 않는 `surface_id`의 거절은 유지한다. 팝업은 계속 창 단위다. [ADR-0017](docs/adr/0017-workspace-identity-and-focus.md).
- (BREAK) **`tasty new workspace --surface <ID>`는 지정한 서피스의 작업 디렉터리를 상속한다.** 이전에는 IPC `workspace.create`에 `surface_id`를 지정하고 `cwd`를 생략해도 소유 창의 활성 서피스에서 경로를 읽어 사용자의 탭 선택에 따라 결과가 달라졌다. 이제 지정한 서피스에서 읽는다. `surface_id`가 없거나 `cwd`가 명시된 호출은 그대로이며, `inherit_cwd`를 끄면 상속하지 않는다. 숫자가 아닌 `surface_id`는 무시하지 않고 `invalid_params`로 거절한다. 포커스 독립성 원칙을 위반하던 동작이므로 0.x 정책의 한 minor 이상 폐기 유예를 거치지 않고 수정했다. [ADR-0043](docs/adr/0043-cli-errors-and-diagnostic-logs.md).
- **`file_handler.dispatch`로 파일을 열어도 사용자 탭 선택을 유지한다.** `tasty file-handler dispatch`를 `origin_surface_id` 없이 호출하면 이전에는 포커스된 페인의 새 탭을 선택했다. 이제 탭만 뒤에 추가한다. 사용자가 플러그인 팝업에서 연 파일은 새 선택 인자 `owner_popup_instance`로 구분한다. 호출 플러그인이 열린 팝업의 소유자이며 해당 팝업이 사용자의 클릭·키 입력을 받았을 때만 새 탭을 선택한다. 외부 IPC 호출자가 같은 인자를 보내도 에이전트 요청으로 처리하며 요청 자체를 거절하지는 않는다. 응답과 오류 코드는 유지한다. 번들 마크다운 파일 열기 팝업은 이 인자를 보내므로 이전처럼 열린 탭을 선택한다. [ADR-0031](docs/adr/0031-file-handler-routing.md).
- **headless 시작 시 무시되는 레이아웃 복원 설정을 알린다.** headless는 워크스페이스 배치를 저장·복원하지 않으며 해당 실행 동안만 유지한다. 기본으로 켜져 있는 `general.restore_layout` 때문에 재시작 시 복원을 기대할 수 있어, 이 설정이 켜져 있으면 시작 경고를 남긴다. `tasty list info`(IPC `system.info`)의 `layout_slot: null`로도 확인할 수 있다. headless의 저장 동작은 바뀌지 않았다. [ADR-0003](docs/adr/0003-headless-behavior.md).
- **에이전트가 만든 비터미널 탭도 활성 탭을 바꾸지 않는다.** 이전에는 `tasty new tab --type html|markdown|explorer|…`(IPC `tab.create`)가 새 탭을 선택하고 포커스된 페인에서는 서피스 포커스도 옮겼다. 터미널 탭은 원래 선택을 바꾸지 않았다. 이제 모든 새 탭을 페인 뒤에 추가하며 선택을 유지한다. 응답의 `active_tab`은 계속 해당 페인의 활성 탭을 뜻하므로 새 탭은 `surface_id`로 지정한다. 원격 에이전트가 만든 탭에도 적용한다. 사용자가 단축키·메뉴·파일 열기·mirror 조작으로 만든 탭은 계속 선택한다. [ADR-0017](docs/adr/0017-workspace-identity-and-focus.md).
- **에이전트가 만든 창은 기존 창의 포커스를 유지한다.** `tasty new window`(IPC `window.create`/`view.create`) 후 `tasty list windows`의 `focused`가 바뀌지 않는다. main 창이 하나도 없을 때만 새 창이 포커스를 받는다. 새 창에 워크스페이스를 만들 때는 그 창의 서피스를 `tasty new workspace --surface <ID>`로 지정한다. 응답의 `window_id`는 `tasty close window`처럼 창 자체를 다루는 명령에 사용한다.

  macOS·Windows에서는 기존 창 바로 아래에 새 창을 배치한다. X11에서는 창 관리자에게 포커스를 주지 않고 기존 창 아래에 놓도록 요청하지만 실제 배치는 창 관리자가 정한다(openbox는 맨 아래에 배치). Wayland에서는 컴포지터가 결정한다. 사용자가 단축키·메뉴·트레이·dock으로 만든 창은 이전처럼 포커스를 받는다. [ADR-0017](docs/adr/0017-workspace-identity-and-focus.md).
- **프리셋의 서피스 설정을 별도 편집 화면으로 옮겼다.** 작은 분할 안에 입력 폼을 표시해 일부 필드가 잘리던 문제를 해결했다. 분할 중앙을 누르면 선택하고, 선택된 분할의 톱니 핸들이나 더블클릭으로 오른쪽 영역 전체에 설정 화면을 연다. 종류와 선언된 필드를 표시하며 하단에 취소·확인 버튼을 고정한다. 서피스 파라미터는 확인할 때 적용·저장하며 변경이 없으면 확인을 비활성화한다. 취소·`Esc`는 변경을 버리고 한 줄 입력란의 `Enter`는 확인으로 처리한다. 저장 실패 시 화면과 입력값을 유지하고 오류 토스트를 표시한다. 편집 중에는 프리셋 목록, scope 탭, 구조 편집 단축키를 비활성화한다. 분할·제거·탭·페인 구조 편집의 즉시 자동 저장과 `preset.*` IPC/CLI는 유지한다.
- (BREAK) **`file_picker.trigger`는 플러그인 호출만 허용한다.** CLI·에이전트 호출에는 파일 선택 팝업 대신 `-32016`을 반환한다. 이전에는 사용자 포커스와 단축키를 가로막는 팝업을 열고 Tools 메뉴의 사용자 조작으로 기록했지만, 선택한 경로는 플러그인에만 전달해 CLI 호출자가 받을 수 없었다. 에이전트는 경로를 직접 지정해야 한다. 마크다운 찾아보기 등 플러그인 경로는 유지한다. 에이전트가 사용자 포커스를 바꾸지 않아야 한다는 원칙을 위반하던 동작이므로 폐기 유예 없이 수정했다. [ADR-0031](docs/adr/0031-file-handler-routing.md).
- **IPC 처리 회차에 16 ms 시간 제한을 추가했다.** 이전에는 한 번 꺼낸 요청을 최대 256건까지 모두 처리한 후 화면·타이머로 넘어갔다. 이제 요청 하나를 처리할 때마다 시간을 확인하고 16 ms가 지났으면 남은 요청을 순서대로 다음 회차에 처리한다. 이미 실행 중인 요청은 중단하지 않으므로 요청 하나가 오래 걸리면 해당 회차도 16 ms를 넘을 수 있다.
- **GUI의 연속 IPC 부하에서도 타이머·화면·창 입력을 처리한다.** 무거운 요청이 계속 들어오면 처리 회차를 짧게 제한해도 다음 회차가 바로 시작되어 화면과 타이머가 밀렸다. 측정에서는 20초 부하 동안 초 단위 훅이 21초 동안 실행되지 않았다. 이제 한 회차 뒤 타이머와 화면 처리를 진행해 같은 부하에서 훅 간격을 유지했다. 대신 해당 측정의 요청 처리량은 약 23% 감소했다. headless에는 이 문제가 없었다.
- **큐에서 기한이 지난 요청은 실행하지 않는다.** 이전에는 `response_timeout_ms`가 만료되면 대기 중인 요청에도 `-32061`(결과 불명)을 반환하고 나중에 실행했다. 이제 실행 직전에 기한을 확인해 만료됐으면 `-32067`로 거절한다. 실행되지 않았으므로 재시도할 수 있다. 이미 시작한 요청의 만료는 계속 `-32061`이며 취소하지 않는다. 지원 호스트는 `list info`에 `ipc.response-timeout.not-run`을 선언한다. 응답 대기 제한이 없는 호출은 유지한다.

- **플러그인 메시지 큐에 바이트 상한을 추가했다.** 기존 요청·응답·이벤트 큐의 각 1024건 제한에 더해, 큐 하나에 16 MiB, 모든 플러그인의 전체 큐에 64 MiB 상한을 둔다. 상한에 도달하면 호스트 요청은 버리고 다음 수락 요청에 손실 건수를 알린다. 플러그인의 응답·이벤트는 공간이 생길 때까지 기다린다. 빈 큐에는 상한보다 큰 메시지 한 건을 허용한다. ping과 종료 요청은 다른 플러그인의 부하로 재시작이나 강제 종료가 발생하지 않도록 전체 합계 제한에서 제외한다. 호스트 거절 로그는 건수(`request queue full`)와 바이트(`over its byte budget`·`total byte budget`)를 구분한다.
- **클라이언트가 계약 밖 메서드의 멱등 키를 전송 전에 거절한다.** `markdown.recent` 같은 플러그인 고유 메서드는 호스트의 멱등 키 저장소를 거치지 않아, 같은 키로 재시도해도 중복 실행될 수 있었다. 이제 `tasty-ipc`의 `IpcConnection::send_idempotent`는 이런 이름을 `KeyOutsideContract`로 거절하고 연결에 쓰지 않는다. 더 새 호스트의 메서드 등 클라이언트가 모르는 이름도 거절한다. 호스트 동작은 그대로여서 기존 클라이언트가 보낸 플러그인 요청의 키는 계속 무시한다. 플러그인 호출의 중복 제거가 필요하면 플러그인이 직접 처리해야 한다. [ADR-0005](docs/adr/0005-idempotent-mutation-retries.md).
- **앱에서 직접 처리하는 여섯 메서드도 멱등 키를 적용한다.** `window.create`, `view.create`, `ui.screenshot`, `remote.attach`, `plugin.install`, `plugin.request_permission`은 이전에 키를 무시해 재시도가 창 생성·스크린샷 등을 반복할 수 있었다. 권한 요청도 대기 중에는 재사용했지만 승인·거절 뒤 재시도하면 새 요청을 만들었다. 이제 같은 키·같은 요청에는 이전 응답과 `idempotent_replay: true`를 반환한다. 첫 실행이 진행 중이면 새로 실행하지 않고 완료를 기다린다. 같은 키의 다른 요청은 계속 `-32063`으로 거절한다. `ipc.idempotency-key` 기능 버전이 2로 올라갔으며 `send_idempotent`는 이 여섯 메서드에서 버전 2를 확인한 뒤 전송한다. 다른 메서드는 버전 1로도 사용할 수 있다. [ADR-0005](docs/adr/0005-idempotent-mutation-retries.md).
- **잘못된 길이의 멱등 키를 모든 메서드에서 거절한다.** 빈 키나 256바이트 초과 키는 이전에 일부 앱 메서드와 플러그인 경로에서 무시되고 실행됐다. 이제 목적지와 관계없이 실행 전 `-32602`로 거절한다. 권한이 없는 호출자에게 `-32001`을 먼저 반환하는 순서는 유지한다. 이미 요청 규칙을 위반한 입력이며 당시 저장소에 키를 보내는 제품 호출자가 없어 BREAK로 분류하지 않았다. 당시 `send_idempotent`는 테스트에서만 호출했고 CLI와 플러그인 호스트 호출은 키를 보내지 않았다. [ADR-0005](docs/adr/0005-idempotent-mutation-retries.md).
- **플러그인을 거치는 호스트 메서드와 debug 입력 주입에도 멱등 키를 적용한다.** 이미지·마크다운 플러그인이 실행하는 `image.open`, `image.export_png`, `image.next`, `image.prev`, `image.paste`, `markdown.navigate`는 선언과 달리 키를 무시해 재시도가 중복 실행됐다. debug 입력 주입과 `debug.lua.eval` 등도 기존에는 계약 밖으로 분류했다. 이제 두 경로 모두 같은 키·같은 요청에 이전 응답과 `idempotent_replay: true`를 반환한다. `ipc.idempotency-key` 기능 버전은 3이며 `send_idempotent`가 이 메서드들을 호출할 때 버전 3을 먼저 확인한다. 지원하지 않으면 전송하지 않는다. `markdown.recent` 등 플러그인 고유 메서드는 계속 계약 밖이다. [ADR-0005](docs/adr/0005-idempotent-mutation-retries.md).
- **`tasty list pressure --help`에 전체 열두 항목의 설명을 반영했다.** 실제 응답에 추가된 항목이 도움말에서 빠져 있던 문제를 수정했다. `connections`는 요청 처리 시간이 아니라 TCP 연결 수를 세며, 요청을 보내지 않는 attach·mesh 연결도 포함한다. `live`, `limit`, `refused_saturated`를 함께 읽어 포화를 구분하고, 요청 전 연결 거절은 시간 통계에 포함하지 않는다고 설명한다. 누계·순간값 중 `live`는 감소할 수 있고 파생값인 accept 대기 평균도 감소할 수 있다. 시간 항목 네 개 중 세 개의 `*_hist`는 누적 분포가 아니라 서로 겹치지 않는 칸별 건수다. `bounds_us`보다 `counts`가 한 칸 더 많으며 마지막 칸은 상한 초과 건수다. 합이 관측 수이고 분위수는 제공하지 않는다. `db`와 `connections`에는 분포가 없다. 긴 도움말만 확장했으며 짧은 `-h` 목록은 유지한다.
- **원격 도구의 SSH config 목록을 정리했다.** `~/.ssh/config` 섹션에 대문자 라벨·경로·호스트 수를 표시하고 각 호스트는 별칭과 `user@host:port` 두 줄로 보여 준다. 이전에 빠져 있던 사용자 이름도 표시한다. 오른쪽에는 ‘프로필 추가’ 버튼을 두고 이미 등록된 호스트는 ‘등록됨’으로 표시한다. 섹션 새로고침 아이콘은 제거했으며 파일을 다시 읽으려면 팝업을 다시 연다. 비어 있거나 읽지 못한 경우는 기존처럼 경고색 없이 한 줄로 안내한다.
- **서피스 팝업의 배경 가림을 해당 서피스로 제한했다.** 서피스 종류 변환, 그 안의 파일 선택, 마크다운 파일 열기 팝업은 해당 영역만 어둡게 한다. 인접 서피스·사이드바·탭 바·상태 표시줄은 가리지 않는다. 가림 강도는 창 범위와 같고 같은 서피스에 팝업이 겹쳐도 중복 적용하지 않는다. 기존에 가림이 없던 서피스 종류 변환 팝업에도 적용한다. 팝업은 서피스 안쪽에 배치하고 영역보다 크면 크기를 줄인다. 명령 팔레트·설정·플러그인 등 창 범위 팝업은 계속 창 전체를 가린다.
- **비활성 색상·상태 표시·토스트 치수를 디자인에 맞췄다.** 2026-09-17 결정에 따라 흐린 항목과 탭 바 스크롤 화살표의 글자색을 전용 비활성 색상으로 통일했다. 탭 hover 배경·구분선·토스트 테두리도 공용 값을 사용한다. 토스트 간격은 6에서 8, 점유 워크스페이스의 링 두께는 1.5에서 2로 바뀌고 대기 상태 점 색상도 변경됐다. 플러그인 창·알림 배지·첫 실행 화면·팔레트 힌트의 일부 글자 크기는 0.5px 단위에서 정수로 맞췄다. 상태바 테마 전환은 색 점 대신 밝은 테마의 해 아이콘과 어두운 테마의 글리프를 같은 색으로 표시한다. 팝업 테두리는 한 단계 밝은 회색이 됐다. 기능 동작은 유지한다.
- **마크다운 파일 열기 팝업을 대상 서피스에 연결했다.** 제자리 변환이면 변환 대상, 새로 열기이면 팝업을 연 순간의 서피스 중앙에 표시한다. 해당 서피스가 보이지 않으면 팝업도 숨기며 돌아오면 입력을 유지한 채 표시한다. 매니페스트 `[[contributes.popup]]`에 선택 속성 `scope`(`window` 또는 `surface`, 기본 `window`)가 추가됐다. 기존 매니페스트는 창 범위를 유지한다. `surface`는 변환 입력 팝업·도구 메뉴 등 호스트가 대상을 지정한 진입점에서만 적용한다. 플러그인이 IPC·이벤트로 직접 연 팝업은 창 범위다.
- (BREAK) **세션 토큰으로 플러그인 명령을 호출할 때 `ipc.invoke:<prefix>` 권한을 검사한다.** 이전에는 플러그인 간 호출과 달리 에이전트 토큰의 검사를 건너뛰어 권한 없는 토큰도 `tasty markdown recent` 등을 호출했다. 이제 권한이 없으면 `-32001 missing permission 'ipc.invoke:<prefix>'`를 반환하고 승인 요청을 만든다. `tasty session issue`로 발급할 때는 `--permission ipc.invoke:<prefix>`를 지정해야 한다. 자식 Claude는 `ipc.invoke:claude`와 `ipc.invoke:codex`를 받아 완료 훅·손자 생성·Codex 교차 검증을 유지한다. Claude 매니페스트에도 `ipc.invoke:codex`가 추가됐다. 플러그인은 자기 네임스페이스의 호출 권한을 보유하지 않아도 `session.issue`로 자식에게 줄 수 있다. 토큰 없는 로컬 CLI는 바뀌지 않는다.
- **파일 선택 창의 시작 폴더를 호출한 서피스에서 결정한다.** 도구 > 파일 열기, 해당 단축키, 마크다운 팝업의 찾아보기는 이전의 홈 대신 터미널·탐색기의 현재 폴더에서 시작한다. 원격 attach에서는 원격 셸의 폴더를 사용하며 OSC 7 전송 여부와 무관하다. 새 서피스의 cwd 상속 설정을 꺼도 파일 선택 시작 위치는 같다. 폴더를 알 수 없거나 로컬 경로가 사라졌다면 홈을 사용한다. 플러그인용 `file_picker.trigger`에 `start_dir`와 `origin_surface_id`가 추가됐다. 지정한 출처의 워크스페이스로 로컬·원격을 구분하고 생략 시 기존 방식을 쓴다. 팝업 열기 컨텍스트에 `observed_cwd`, `remote_cwd`, `origin_surface_id`를 추가했으며 기존 `cwd`의 뜻은 유지한다.
- **파일 핸들러 선택 창을 한 목록으로 정리했다.** 420px 폭에 제목·인식 형식·경로를 한 번씩 표시하고, 긴 경로는 파일명을 남기도록 앞부분을 줄인다. 후보와 최근 선택을 한 목록의 두 구역으로 보여 주며 하나만 선택할 수 있다. 각 행은 아이콘·이름·출처(기본 제공/직접 등록/플러그인)·전체 ID를, 최근 항목은 사용 시점도 표시한다. 아이콘은 열 화면의 종류를 따르며 표시명이 없으면 ID 마지막 부분을 고정폭 글꼴로 보여 준다. 플러그인 출처와 아이콘은 보라색이다.

  긴 목록은 목록 영역만 스크롤하고 하단을 흐리게 표시한다. 한 번 클릭하면 선택하고 두 번 클릭하면 열며, 미선택 상태에서는 열기를 비활성화한다. 바깥 클릭으로 닫히지 않고 `Esc`나 취소로 닫는다. 선택은 이번 열기에만 사용하며 핸들러를 등록하지 않는다. ‘형식 알 수 없음’ 표시가 배경과 구분되도록 창 배경도 한 단계 어둡게 했다.
- **드롭다운 그림자를 공용 popover 값으로 통일했다.** 설정의 선택 드롭다운, MultiSelect/Select, 포트 스캐너·원격 도구·DAG의 콤보박스가 테마별 프레임워크 기본 그림자 대신 배너·툴팁·자동완성과 같은 그림자를 사용한다.
- **상태바의 항목과 좁은 창에서 숨기는 순서를 정리했다.** 왼쪽은 Git 브랜치·서피스 ID·셸·격자 크기, 오른쪽은 팔레트 단축키·테마 아이콘이다. 서피스 ID는 `s3·p1`처럼 페인도 함께 표시하고 셸과 격자 크기는 별도 항목으로 분리했다. 팔레트는 키 모양만, 테마는 아이콘만 남긴다. 브랜치 앞의 초록 점은 가지 아이콘으로 바꿨다. 저장소가 아니면 브랜치 항목을, 단축키가 비어 있으면 팔레트 항목을 숨긴다. detached HEAD에는 `@`와 짧은 커밋 번호를 표시한다. 창이 좁아지면 격자 크기, 셸, 서피스 ID, 팔레트 단축키, 브랜치 이름 순으로 숨기고 테마 아이콘은 유지한다. 긴 브랜치 이름은 숨기기 전에 말줄임한다. 팔레트 열기와 테마 전환의 클릭 동작은 유지한다.
- **작업 DAG 창의 상세 화면 헤더를 하나로 합쳤다.** 돌아가기 행 오른쪽에 확대·축소·전체 맞춤·방향 전환·러너 표시를 모으고 별도 그래프 헤더와 캔버스 위 버튼을 제거했다. 이미 선택한 DAG의 상세 화면이므로 DAG 선택 항목도 뺐다. 탭으로 여는 DAG 화면은 기존 헤더와 캔버스 버튼을 유지한다.
- **숫자 설정을 직접 입력하는 필드로 통일했다.** 휠 거리·알림 병합·스크롤백·토스트 시간·글꼴 크기·줄 높이·플러그인 배율 등의 드래그 입력을 제거해 설정 화면 스크롤과 충돌하지 않게 했다. 숫자는 오른쪽으로 맞추고 단위(`pt`·`s`·`MiB`·`%`)는 밖에 표시한다. 입력 중에는 값을 보정하지 않고 포커스가 떠나거나 `Enter`를 누를 때 허용 범위로 맞춘다. 예를 들어 25~200 범위에도 `150`을 끝까지 입력할 수 있다. 범위 밖 값은 빨간 필드와 허용 범위·저장 예정값으로 알린다. 저장값과 허용 범위 자체는 바뀌지 않았다.
- **삭제할 수 없는 훅 핸들러를 자물쇠로 표시한다.** 설정 › Handler › Hook Handlers에서 호스트·플러그인이 제공한 행은 휴지통 대신 자물쇠와 삭제 불가 툴팁을 표시한다. 플러그인 출처는 실제 ID, 사용자 출처는 `you`로 표시한다. IPC 시퀀스는 `window.focus → layout.save → surface.close`처럼 순서를 고정폭 한 줄로 보여 준다. 목록에는 시퀀스 편집 기능이 없으므로 `tasty hook-handler upsert`로 수정한다. 활성화, 셸 명령 인라인 편집, 삭제 가능 조건은 유지한다.
- **완료 알림 로그를 비울 때 삭제량을 기록한다.** 쓰기 전 크기가 256 KiB 이상이면 최근 일부만 남기는 대신 파일 전체를 비우므로 읽지 않은 알림도 삭제될 수 있다. 이제 버린 바이트 수를 진단 로그에 기록한다. 완료 로그 자체는 한 줄이 알림 하나인 형식을 유지하며 비우기 정책은 바뀌지 않았다. 쓰기 전에 검사하므로 새 기록이나 동시 쓰기로 256 KiB를 넘을 수 있다.

  보존 범위는 현재 인스턴스 실행 동안이며 닫은 탭의 로그도 다음 재시작까지 남는다. 기간·파일 개수 제한은 없고 크기만 검사한다. 같은 데이터 폴더로 두 인스턴스를 실행하면 나중 인스턴스가 먼저 실행한 쪽의 로그를 지울 수 있다. 데이터 폴더 밖에 `--port-file`을 둔 경우의 예외는 아래 수정 항목에 설명했다. 인스턴스를 분리할 때는 `TASTY_HOME`을 사용한다.

- **연속으로 호출에 응답하지 않는 플러그인을 재시작한다.** 이전에는 ping에 응답하는 플러그인이 개별 호출을 무시해도 호출마다 150초 뒤 경고만 남겼다. 이제 응답 없이 세 호출이 연속 만료되면 healthcheck 실패와 같은 경로로 재시작한다. 응답이 한 번이라도 도착하면 연속 횟수를 초기화한다. 150초 뒤 늦게 도착한 응답도 횟수 초기화에는 반영하지만 이미 만료된 호출의 오류를 되돌리지는 않는다.

- **수동 훅 시퀀스도 순서대로 실행한다.** `tasty hook-handler dispatch`의 `ipc_sequence`를 터미널 훅과 같은 큐에 넣어 한 목록씩 실행한다. 이전에는 수동 호출끼리 또는 터미널 훅과 단계별로 섞일 수 있었다. 접수 응답은 계속 즉시 반환하지만 앞선 시퀀스가 느리면 실행 시작은 늦어진다. 대기 목록이 이미 256건이면 실행하지 않고 `-32603`과 `hook handler '<id>' not run — …`을 반환한다. 단계 로그에는 이전의 공통 `webhook` 대신 실제 출처 `webhook`, `surface hook`, `hook_handler.dispatch`를 표시한다. [ADR-0027](docs/adr/0027-lua-and-hook-execution.md).

### Fixed

- **Linux에서 마크다운의 파일 링크를 클릭하면 열린 탭을 선택한다.** 이전에는 호스트가 에이전트 요청으로 처리해 탭만 추가했다. 이제 네이티브 웹뷰가 사용자 제스처로 보고한 navigation이며 문서를 소유 플러그인이 작성했다면 클릭 기록을 남긴다. 해당 플러그인이 통지받은 URL을 `file_handler.dispatch`의 `user_navigation_url`로 보내고 `origin_surface_id`도 일치할 때 한 번만 사용자 조작으로 처리한다.

  제스처가 아닌 이동, 에이전트가 `tasty set url`로 작성한 페이지의 클릭, 플러그인이 페이지를 되찾은 첫 프레임의 클릭은 기존 기록도 지운다. 페이지를 되찾은 프레임 뒤에 클릭 통지가 도착하는 순서는 당시 확인하지 못했다. 외부 IPC 호출자가 같은 인자를 보내면 에이전트 요청으로 처리하며 요청 자체를 거절하지 않는다. 응답·오류 코드는 유지한다. macOS는 엔진이 제스처 값을 제공하지 않아 이전처럼 탭만 추가하며 Windows는 당시 미확인이다. [ADR-0031](docs/adr/0031-file-handler-routing.md).
- **프리셋 보기 화면에 외부 저장 내용을 반영한다.** 프리셋 창을 보는 동안 에이전트가 같은 이름으로 `tasty preset save`(IPC `preset.save`)를 호출하면 다음 프레임에 새 레이아웃을 표시한다. 이전에는 목록만 바뀌고 미리보기는 오래된 구조를 유지해 편집 전환 후 첫 수정도 충돌로 버렸다. 저장 뒤 첫 프레임에서 바로 Edit를 눌러도 새 내용으로 편집을 시작한다. press·release가 같은 이벤트 배치인 경우와 키보드 Edit도 포함한다. 토스트는 표시하지 않으며 미리보기에서 선택한 mini-tab은 저장된 활성 탭으로 돌아간다. 기존 편집 모드의 초안·선택·저장 시 경합 검사는 유지한다. [ADR-0038](docs/adr/0038-preset-drafts-and-store-conflicts.md).
- **명령 팔레트 붙여넣기도 사용자 입력으로 기록한다.** 이전에는 텍스트를 붙여넣어도 `tasty is-typing`(IPC `surface.is_typing`)이 입력이 없다고 판단해, `tasty send text --wait-idle`의 5초 보호나 Claude 자동 재개 억제가 적용되지 않았다. 이제 단축키·명령 팔레트, 텍스트·이미지 여부와 관계없이 기록한다. 수식키를 쓰는 붙여넣기는 이전에도 키 입력으로 감지했다. 마우스 조작과 파일 드롭은 계속 제외한다. [ADR-0015](docs/adr/0015-terminal-user-input-routing.md).
- **프리셋 편집 중 외부 저장과의 충돌을 확인한다.** 에이전트가 같은 이름으로 `tasty preset save`(IPC `preset.save`)를 호출한 뒤 사용자가 오래된 화면을 저장하면, 이전에는 새 레이아웃을 덮어썼다. 이제 저장 직전에 읽어 둔 레이아웃과 현재 저장소를 비교해 다르면 쓰지 않는다. 저장된 내용을 다시 불러오고 경고 토스트를 표시하며, 해당 구조 변경이나 설정 초안은 버린다. 이름·subtitle만 달라진 경우와 충돌이 없는 경우는 그대로 동작한다. [ADR-0038](docs/adr/0038-preset-drafts-and-store-conflicts.md).
- **비활성화하거나 제거한 플러그인의 서피스 종류로 새 화면을 만들지 않는다.** 이전에는 등록이 남아 탭 생성·분할·변환이 성공하고 응답할 플러그인이 없는 빈 화면을 만들었다. 이제 `tasty plugin disable`·`remove`나 설정 UI에서 끄거나 제거하면 해당 종류가 `surface.kinds`와 변환 목록에서 빠지고 `plugin.show`의 `registered`도 false가 된다. 생성 요청은 `surface kind '<kind>' is unavailable: the plugin '<id>' that provides it is disabled or removed, or has not reconnected since it was enabled again` 오류로, 사용자 조작은 같은 사유의 경고 토스트로 알린다. 기존 화면은 유지하고 다시 켜면 연결한다. 닫은 탭 복원·프리셋 적용에서는 대기 placeholder를 두었다가 활성화 후 채운다. [ADR-0026](docs/adr/0026-plugin-registration-and-lifecycle.md).
- **원격 서피스 변환 실패에 실제 오류를 표시한다.** 원격에 종류가 없을 때 엉뚱한 `surface 5 not found` 대신 원격이 반환한 `unknown surface kind: markdown` 등의 사유를 토스트에 표시한다. 사유가 없으면 추측하지 않고 `surface <id> was not converted`로 알린다.
- **attach 화면 수집 중 heartbeat를 전송한다.** `tasty remote attach`, `tasty tool attach`, debug의 `tasty debug attach` 기본 모드는 원격에 데이터를 보내지 않아 `--dump-after`가 20초를 넘으면 약 20초에 점유가 풀렸다. CLI는 요청한 시간을 채우지 못해도 정상 종료했다. 이제 수집 중에도 5초마다 생존 신호를 보내 연결을 유지한다. 5초 이하의 수집 기간과 기본 500 ms에서는 이전과 같은 데이터를 전송한다. [ADR-0023](docs/adr/0023-attach-state-sync-and-forwarding.md).
- **headless 탭 이름에도 셸의 현재 디렉터리를 반영한다.** OSC 7로 받은 경로를 사용해, 직접 지정한 이름과 프로그램 제목이 없으면 디렉터리 이름을 표시한다. GUI와 같은 규칙이며 `tasty list tree`와 attach 화면에 적용한다. `tab.list`의 `name`은 두 빌드 모두 원래 이름을 유지한다.
- **자식 프로세스 종료를 놓친 터미널이 남던 문제를 수정했다.** 종료를 제때 감지하지 못하고 더 이상 출력도 없으면 열린 탭이 계속 남을 수 있었다. 이제 종료가 확인될 때까지 다시 확인한다. 원격 attach mirror에도 정상적으로 탭 제거가 반영된다.
- **마지막 워크스페이스를 닫을 때 앱이 종료되던 문제를 수정했다.** 마크다운·HTML 화면의 `close_workspace` 단축키나 우클릭 닫기에서 창 제거가 늦어져, 다음 렌더가 빈 워크스페이스 목록을 읽었다(Linux 측정 12회 중 8회). 이제 winit 키 경로처럼 같은 처리에서 창을 제거한다. 세션은 계속 보관해 다음 창에서 이어간다.
- **Claude의 API 오류 종료도 상태와 부모 알림에 반영한다.** `tasty claude install`에 `StopFailure` 훅을 추가했다. 이전에는 `529 Overloaded`, 요청 한도, 인증 실패 등으로 재시도를 마친 세션도 `active`로 남고 30초 뒤 정지 알림만 보냈다. 이제 `idle`로 바꾸고 `claude-idle` 부모 알림에 오류 종류를 덧붙인다(`… — the turn ended on an API error (overloaded) …`). `claude-last-stop-failure` 메타데이터는 새 턴에 지우며 `claude-stop-failure` 서피스 훅도 발생한다. 메인 턴을 끝내지 않는 서브에이전트 오류는 무시한다. 기존 사용자는 install을 다시 실행해야 한다.
- **웹훅이 큰 본문을 거절한 뒤 연결을 즉시 닫는다.** 이전에는 `Content-Length` 초과로 `413 payload too large`를 보내고도 HTTP 계층이 나머지 본문을 읽으면서 스레드와 메모리를 점유했다. 본문 없이 `Content-Length: 1099511627776`만 보낸 측정에서는 같은 크기의 할당 실패로 debug 데몬이 SIGABRT 종료했다. 등록·인증보다 먼저 처리되어 어떤 경로에서도 발생할 수 있었다.

  이제 413에 `Connection: close`를 보내고 남은 본문을 읽지 않는다. keep-alive, 명시적 close, `Expect: 100-continue`에 모두 적용한다. 발신자는 새 연결로 다음 요청을 보내야 하며 전송 중에는 reset을 받을 수 있다. 본문을 읽지 않는 남용 차단 `429`도 같은 방식으로 닫는다. 본문을 읽은 뒤 반환하는 `200`·`404`·`405`·`401`·`410`과 Agent Stream SSE는 유지한다. [ADR-0032](docs/adr/0032-webhook-admission.md).
- **CLI가 연결 전에 잘못된 인자를 검사한다.** 깨진 `--sequence` JSON이나 없는 `--cwd`처럼 로컬에서 판단할 수 있는 오류는 Tasty가 실행 중이지 않아도 해당 인자 오류로 반환한다. 이전에는 먼저 연결해 `No running tasty instance found …`로 끝났다. 종료 코드는 대부분 1이며 `approval`·`telemetry` 일부 인자 오류는 2다. 인스턴스가 없으면 항상 1로 처리하던 스크립트는 2도 처리해야 한다. 인스턴스가 실행 중일 때의 결과는 유지한다.

  `tasty preset save --file -`도 연결 전에 표준 입력을 읽는다. 터미널에서 실행하면 입력 종료까지 기다린 뒤, 올바른 JSON이면 연결 오류를, 잘못된 JSON이면 파싱 오류를 반환한다. `--file <경로>` 방식은 그대로다. [ADR-0043](docs/adr/0043-cli-errors-and-diagnostic-logs.md).
- **터미널 훅의 IPC 시퀀스를 별도 스레드에서 실행한다.** `notification`, `bell`, `output-match`, `command-completed`, `process-exit`, `idle-timeout`에 연결한 `ipc_sequence`가 단계 수 × 10초 동안 화면·IPC를 막던 문제를 고쳤다. 이전에는 단계가 나중에 실행돼도 `failed: host_dispatch timeout after 10s`로 기록했다. 이제 발생 순서대로 한 시퀀스씩 처리하고 앞 단계의 응답을 받은 뒤 다음 단계를 실행한다. 성공은 성공으로 기록하며 대기 목록이 256건이면 새 목록은 실행하지 않고 오류를 남긴다. 웹훅과 수동 dispatch는 원래 화면을 막지 않았으며 수동 dispatch의 순서 변경은 위 항목에 설명했다. [ADR-0027](docs/adr/0027-lua-and-hook-execution.md).
- **플러그인 메시지 왕복의 불필요한 지연을 줄였다.** `tasty markdown recent` 등에서 작은 메시지 조각을 확인 응답까지 보류해 120~130 ms 걸리던 왕복이 같은 측정에서 최대 1.5 ms로 줄었다. 일반 호출이 `slow_requests`를 채우던 현상도 줄었다. 서드파티 SDK 플러그인은 호스트 쪽 개선을 바로 적용받고, 다시 빌드하면 응답 쪽 약 40 ms 지연도 줄어든다. 메시지 한 줄을 먼저 조립해 쓰도록 바꿨으며 TCP 수신이 반드시 한 번에 이루어진다는 뜻은 아니다. 플러그인 생성 전에 수신 준비를 마쳐, 빠른 인증 요청을 준비 전이라는 이유로 거절하던 문제도 고쳤다.
- **SDK가 늦게 도착한 요청 조각을 보관한다.** SDK 읽기가 50 ms마다 깨어날 때 한 줄의 조각 간격이 길면 앞부분을 버려 요청 전체가 사라지고 호스트가 시간 초과로 끝났다. 이제 앞 조각을 유지해 나머지와 합친다. 서드파티 플러그인은 SDK로 다시 빌드하면 적용된다.
- **번들 플러그인이 종료 요청을 처리한 뒤 스스로 종료한다.** 이전에는 9개 모두 2초 유예 후 강제 종료되어 정리 작업을 보장하지 못했고 그동안 호출은 `-32002`를 받았다. 이제 종료 요청보다 앞선 요청을 처리한 뒤 끝난다. 측정에서는 9/9 정상 종료했고 각각 55~160 ms였다. 남은 요청이 호스트를 호출하면 연결 종료 오류를 즉시 받아 응답을 기다리지 않는다. SDK 수정이므로 서드파티 플러그인도 다시 빌드하면 적용된다.
- **`tasty webhook register`의 기본 메서드를 바로잡았다.** `--method`가 없으면 빈 배열 대신 서버 기본값 `POST`로 등록한다. 잘못된 `--sequence` JSON도 버리지 않고 전송 전에 옵션 이름과 파싱 원인을 표시한 뒤 종료 코드 1로 끝낸다.
- **CLI의 stderr 파이프가 닫혀도 crash report를 만들지 않는다.** `tasty window list 2>&1 | head -20`처럼 수신 측이 먼저 종료하면 `failed printing to stderr: Broken pipe`로 abort하던 문제를 고쳤다. 이제 stderr 쓰기 실패를 무시하고 원래 종료 코드(파싱 오류 2, 일반 실패 1)를 유지한다. stdout 파이프 종료 시 코드 0을 반환하는 규칙은 그대로다. [ADR-0043](docs/adr/0043-cli-errors-and-diagnostic-logs.md).
- **원격 mirror 하나만 남은 창의 연결이 끊겨도 앱이 종료되지 않는다.** 프로필 매핑 없이 연결한 mirror가 원격 종료·강제 해제·무응답으로 사라지면 빈 워크스페이스 목록을 렌더링하다 종료되던 문제를 고쳤다. 이제 기존 창에 새 터미널 워크스페이스를 만든다. 연결 끊김 토스트는 유지한다. [ADR-0023](docs/adr/0023-attach-state-sync-and-forwarding.md).
- **에이전트의 원격 구조 변경 실패는 사용자 토스트 대신 로그로 남긴다.** `tasty split`, `tasty new tab`, 탭·페인·서피스 닫기, 탭 이동, IPC `image.open`이 mirror에서 원격으로 전달된 뒤 실패해도 사용자에게 경고 토스트를 표시하지 않는다. 사용자가 단축키·메뉴로 실행한 같은 동작은 계속 토스트로 알린다. 적용 전 `forwarded: true`를 반환하는 IPC 응답은 유지한다. [ADR-0036](docs/adr/0036-overlay-scope-and-lifetime.md).
- **에이전트가 원격 문서를 열거나 새로고침할 때도 오류 토스트를 제한한다.** `markdown.navigate`와 출처 없는 `file_handler.dispatch`의 원격 새 탭 요청이 실패하면 로그만 남긴다. 사용자 조작과 마크다운 파일 열기 팝업(`owner_popup_instance`)은 계속 토스트로 알린다. 적용 전 `accepted`를 반환하는 응답은 그대로다. 프리셋 적용·저장 실패도 사용자 요청에서만 토스트를 표시한다. 에이전트의 `tasty markdown reload --surface`에서 원문이 잘려 와도 토스트를 띄우지 않는다. 마크다운 플러그인 0.1.123에서 `markdown_mirror.content_request`에 선택 인자 `agent_origin`을 추가했다. [ADR-0036](docs/adr/0036-overlay-scope-and-lifetime.md).
- **플러그인 연결을 기다리는 동안 화면과 IPC를 계속 처리한다.** 이전에는 enable·재시작 시 최대 10초 동안 메인 처리를 막았다. 5초 후 연결하는 플러그인의 enable과 그 사이 `list info`가 모두 5.1초 걸렸던 측정에서, 비동기 대기로 바꾼 뒤 각각 0.13초와 최대 0.16초가 됐다. 연결 직후까지 요청을 보관하고 연결되면 전달한다. 확장 훅·플러그인 호출의 제한 시간은 계속 연결 후부터 센다.

  연결 전에도 `plugin list`는 `running: true`를 표시한다. 10초 안에 연결하지 못하면 `plugin.error`의 `spawn_failed`로 기록하고 `running: false`가 되며 프로세스도 정리한다. 자동 정지는 10초 안의 실행 실패 3회 기준이므로, 각각 10초 걸리는 연결 실패만으로는 발동하지 않고 빠른 실패와 섞일 때만 가능하다. 이 기준은 이전과 같다. [ADR-0026](docs/adr/0026-plugin-registration-and-lifecycle.md).
- **파일 판별기의 사용자 설정 우선순위를 유지한다.** `~/.tasty/file-handlers.toml`의 표시명·아이콘·활성화 설정을 부팅이나 플러그인 재시작 후에도 기본값 → 플러그인 → 사용자 순으로 적용한다. 이전에는 읽은 순서에 따라 플러그인 값이 덮어썼다. 사용자 `disabled = false`는 다른 출처에서 끈 판별기를 다시 켜며, 기본값·플러그인의 false 처리 방식은 유지한다.

  설정을 다시 읽어도 사용자 규칙의 출처 표시와 삭제 버튼을 유지한다. 규칙 내용이 플러그인과 같거나 아이콘·활성화만 바꾼 사용자 항목에도 삭제 버튼이 표시된다. 삭제하면 사용자 변경을 제거하고 기본값·플러그인 값으로 돌아간다. [ADR-0031](docs/adr/0031-file-handler-routing.md).
- **출력 관찰자의 메모리 기록을 고유 키로 저장한다.** 같은 밀리초의 여러 결과가 한 키를 덮어쓰고 오래된 기록 삭제가 최신 결과까지 지우던 문제를 고쳤다. `tasty output observe start --sink memory --max-records N`은 `tasty.observer.<id>.<ms>.<seq>` 키를 사용해 최근 N건을 유지한다. 이전에는 `--max-records 2`에 여섯 결과를 보내면 0건이 남았다. 키 마지막 부분은 시각 대신 관찰자별 순번이며 시각은 값의 `at_ms`로 읽는다. [ADR-0009](docs/adr/0009-state-storage-and-retention.md).
- **임시 메모리 저장소를 사용하는 다른 쓰기 API에도 `durable: false`를 반환한다.** `memory.db`를 열지 못했을 때 기존 `memory.*` 외에 `agent.*`(task·barrier·semaphore·lease·rate-limit), `approval.request/respond/cancel/summary.set`, `surface.meta.set/unset`(`tasty surface-meta set`), `telemetry.record/record_batch/cap.*`, `session.issue/revoke`에도 추가했다. 성공해도 재시작 후 사라지는 저장임을 구분할 수 있다. 정상 저장소의 응답은 유지한다. [ADR-0010](docs/adr/0010-storage-failure-reporting.md).
- **잘못된 협업 자원 이름은 입력값 기준으로 안내한다.** `agent.semaphore_*`, `agent.barrier_*`, `task_*`의 `id`, `rate_limit_remove`의 `id`에서 이름 규칙을 어기면 `-32602`를 반환한다. 기존 내부 키 기준 `invalid char at 24` 대신 `semaphore name "v6S": invalid char at 2: 'S' (allowed: …; at most 234 bytes)`처럼 입력 내 위치·허용 문자·길이를 표시한다. 영문 소문자·숫자·`.`·`_`·`-` 규칙과 정상 입력의 동작은 유지한다. lease의 `resource`에는 이 제한이 없다. [ADR-0042](docs/adr/0042-agent-coordination-and-task-views.md).
- **headless의 마지막 워크스페이스 닫기 오류를 바로잡았다.** 지원하지 않는 `window.close`를 권하지 않고 마지막 워크스페이스를 닫을 수 없다고 안내한다. GUI 문구와 두 빌드의 오류 코드는 유지한다.
- **마지막 창을 닫은 뒤 `ui.state`를 조회해도 panic하지 않는다.** debug GUI가 워크스페이스 없이 대기할 때 `pane_count`, `tab_count`, `active_tab`을 `null`로 반환한다. 다른 필드는 유지하며 release에는 이 메서드가 없다.
- **큐 대기 평균의 계산 기준을 바로잡았다.** `queue_before_gate.wait_us_mean`을 회차가 끝날 때 증가하는 `commands`로 나눠, 현재 회차의 대기가 분자에만 반영되고 평균이 최댓값보다 커지던 문제를 수정했다. 이제 실제 측정한 요청 수로 나누고 새 `waits` 필드로 반환한다. `waits`는 `wait_us_hist.counts`의 합과 같다. `commands`와 다른 키는 유지한다. [ADR-0008](docs/adr/0008-ipc-pressure-observability.md).
- **headless에서 IPC 깨우기 신호가 쌓이지 않도록 합쳤다.** 연속 요청마다 신호를 추가해 플러그인 응답·터미널 출력이 수천 신호 뒤에서 기다리던 문제를 고쳤다. 부하 중 `markdown.recent` 응답이 1–2초 걸리던 측정에서 GUI와 같은 수백 ms 수준으로 줄었다. 플러그인 heartbeat 등 주기 작업 시각마다 약 1초 동안 CPU를 반복 사용하던 문제도 수정했다. [ADR-0007](docs/adr/0007-ipc-scheduling-and-deadlines.md).
- **응답 제한 기능을 확인하는 요청에도 읽기 timeout을 적용했다.** 이전에는 `--response-timeout-ms` 지원 여부를 먼저 묻는 과정에 제한이 없어, 8초 멈춘 Tasty에 300 ms를 지정해도 7.7초 뒤 끝났다. 이제 기능 확인과 실제 요청에 같은 남은 시간 예산을 사용한다. 확인이 만료되면 본 요청을 보내지 않고 `Error (-32067): …`와 종료 코드 1로 끝나므로 재시도할 수 있다. 이전 호스트에 대한 기능 확인에도 적용한다. 다만 제한은 요청의 응답 대기와 개별 소켓 읽기에 걸리며, 쓰기나 부분 입력·빈 줄에 따른 반복 읽기까지 포함한 전체 호출의 절대 기한을 보장하지는 않는다.
- **tell의 제출 Enter가 만료된 뒤 실행되지 않게 했다.** `tasty claude tell`과 `terminal.tell`은 본문 다음에 `\r`을 보내는데, 이전에는 5초 timeout 뒤에도 큐에 남아 나중에 사용자의 입력을 제출할 수 있었다. 이제 실행 전에 만료되면 로그에 `nothing ran`을 남기고 실행하지 않는다. 에이전트 러너의 플러그인 호출도 5초 안에 시작하지 못하면 `… while still queued (nothing ran)`으로 끝난다. 웹훅, 서피스·idle 훅, 수동 훅 시퀀스는 이미 접수했거나 다시 발생하지 않는 이벤트이므로 이전처럼 대기 후 실행한다.
- **파일 핸들러의 사용자 우선순위를 재시작 후에도 적용한다.** `~/.tasty/file-handlers.toml`에서 `com.tasty.markdown/viewer` 등 플러그인 핸들러의 `priority`를 바꾸면, 부팅·플러그인 재시작 뒤에도 읽은 순서와 관계없이 사용자 값을 우선한다. 이전에는 플러그인 기본값이 덮어써 `tasty file-handler reload`를 직접 실행해야 했다.
- **훅 핸들러의 사용자 설정을 플러그인 재시작 후에도 유지한다.** `~/.tasty/hook-handlers.toml`의 `priority`, `source`, `action`, `display_name_i18n_key`가 플러그인 재시작이나 headless 첫 기동 때 기본값으로 덮이던 문제를 고쳤다. 이제 읽은 순서와 관계없이 사용자 값을 우선하며 별도 `tasty hook-handler reload`가 필요하지 않다.
- **crash·hang 보고서에 본체 버전을 기록한다.** `~/.tasty/crash-reports/`의 `crash-*.log`, `hang-*.log`의 `Version:`이 내부 크레이트 버전 `0.1.0` 대신 실행 중인 Tasty 버전(예: 0.10.4)을 표시한다. 보고서 코드를 분리한 뒤 잘못된 버전을 기록하던 문제를 수정했다.

- **`tasty events follow`가 재연결 후 이벤트 세대를 확인한다.** `--epoch <세대>`를 전달하면 재시작 여부를 stderr로 알리고 새 세대의 처음부터 읽는다. 생략해도 offset이 현재 피드 끝보다 뒤라면 같은 방식으로 알린다. 이전에는 epoch를 한 연결 안에서만 비교해 재시작 후 연결에서 이벤트를 놓칠 수 있었다. 연결이 끊기면 재개할 `--offset`과 `--epoch`를 stderr로 출력하고 끝내며 `--reconnect`를 지정하면 1초마다 재연결한다.

  기존 stdout·종료 조건은 유지하되 현재 끝보다 뒤의 offset 처리는 달라졌다. 해당 번호까지 기다리는 대신 경고 후 처음부터 읽는다. 같은 세대에 미래 offset을 지정하던 스크립트도 링에 보존된 과거 이벤트를 최대 1024건 받을 수 있다.

- **이벤트의 연속 재발행 횟수를 호스트가 보정한다.** 플러그인이 받은 이벤트에 아직 응답하지 않은 동안 새 이벤트를 발행하면 `hop`을 수신값 + 1로 올린다. 이전에는 SDK의 `publish_fresh`처럼 0으로 보내면 16회 초과 차단을 우회해 두 플러그인이 계속 반응할 수 있었다. 응답 후 발행은 이전처럼 플러그인이 보낸 값을 사용한다.

- **이벤트 피드 끝보다 뒤의 커서를 응답으로 구분한다.** `events.fetch`에 `ahead_of_stream`과 `stream_end`를 추가했다. 재시작 전의 큰 offset을 보내도 이전에는 빈 응답만 반환해 새 이벤트를 놓칠 수 있었다. 기존 필드의 값은 유지하며 `wait_ms`가 있으면 계속 대기 후 응답한다.

- **자동 재시작한 플러그인의 등록 상태를 복구한다.** ping 무응답 60초 초과나 연속 호출 만료로 재시작할 때도 비활성화·업그레이드와 같은 정리를 수행한다. 이전에는 등록 표시가 남아 새 hello를 무시하고, 지워진 이벤트 권한·설정 페이지를 다시 등록하지 않았다. 같은 정리에서 응답 없는 서피스 생성·복원 요청도 회수한다. 재시작마다 `plugin.surface_kind_registered`와 `plugin.loaded`를 한 번씩 다시 발생시키므로 구독자는 같은 플러그인의 loaded를 여러 번 받을 수 있다. 비활성화 후 다시 켤 때처럼 `SurfaceKindRegistry: kind '<kind>' overwritten` 경고도 남는다.
- **여러 자식이 같은 완료 로그를 쓰는 방식을 개선했다.** 같은 호출자 아래 Claude·Codex 자식이 서로 다른 프로세스에서 기록할 때 줄이 섞이거나 앞부분이 덮이던 문제를 줄였다. 기존 측정에서는 열 번 중 아홉 번 발생했다. 이제 append 모드로 열고 개행을 포함한 한 줄을 먼저 조립한 뒤 `write_all`로 기록한다. 문구와 쓰기 전 256 KiB 검사·전체 비우기 정책은 유지한다. `write_all`은 부분 쓰기를 재시도할 수 있으므로 단일 OS 쓰기나 줄 단위 원자성을 보장하지 않는다. 동시 쓰기·비우기와 잠금 실패에서도 줄 전체 보존을 보장할 수는 없다.
- **데이터 폴더 밖의 `--port-file`을 쓰는 인스턴스는 완료 로그를 초기화하지 않는다.** 이전에는 포트 파일만 분리한 두 번째 인스턴스도 `<데이터 폴더>/notify/`를 비워 실행 중인 다른 인스턴스의 기록을 삭제했다. 포트 파일이 데이터 폴더 안에 있거나 옵션을 생략하면 이전처럼 초기화한다. 두 인스턴스의 데이터 폴더 공유는 여전히 지원하지 않으며 `TASTY_HOME`으로 분리해야 한다.
- **응답 없는 플러그인 호출에 150초 제한을 적용했다.** 이전에는 프로세스 전체가 멈추면 75초 안에 정리했지만, ping에는 응답하면서 개별 호출만 무시하는 플러그인은 호출자를 무기한 기다리게 했다. 이제 `tasty markdown recent` 등과 플러그인 간 호출도 150초 뒤 무응답 오류로 끝난다. 정상적인 장기 작업은 즉시 접수 응답을 보내고 결과를 따로 알리는 방식을 사용한다.

- **부분 스크롤 영역에서 자동 줄바꿈이 영역 밖으로 넘어가지 않게 했다.** Codex 등 `DECSTBM`을 사용하는 TUI에서 긴 줄이 하단 입력란을 덮고 이후 출력이 스크롤백에서 사라지던 문제를 고쳤다. 이제 명시적 개행과 같은 규칙으로 영역 안을 한 줄 스크롤하고 밀려난 줄을 순서대로 보관한다. 영역 밖 행은 유지하며 대체 화면이 기본 스크롤백을 변경하지 않는다. 화면 밖이나 역순 범위(`CSI 3;100r` 등)는 xterm처럼 화면 범위로 제한한다. 자동 줄바꿈 끄기(DECAWM `?7l`)는 계속 지원하지 않는다.
- **마크다운의 목차·내부 링크·각주 이동을 고쳤다.** 불필요한 `<base>` 때문에 `#제목`을 문서 내부가 아닌 폴더 주소로 해석하던 문제를 제거했다. 같은 항목의 반복 클릭, 중복·한글 제목, 본문 앵커, 각주 번호와 되돌아가기를 처리한다. 문서를 다시 로드하지 않아 이동 중 로딩·오류 화면이 나타나지 않는다. 다른 파일·외부 링크·주소창 이동은 유지한다.
- headless PTY 종료도 공용 호스트 종료 처리에서 process-exit 훅을 발생시킨다.
- 자식 생성은 명령 전송 전에 부모 관계를 등록한다. 준비나 동기 전송이 실패하면 이번에 만든 서피스만 되돌리며, 기존 서피스의 자식 등록에 실패해도 원래 soft 소유권은 유지한다.
- Codex 훅 설치·제거 시 같은 matcher 그룹에 있는 사용자 핸들러를 보존한다.

- **링크 클릭 수식키의 `none` 선택지를 “좌클릭 열기 끄기”로 바로잡았다.** 좌클릭으로 링크를 연다고 잘못 안내하던 설정 라벨과 사용자 가이드를 실제 동작에 맞췄다. 링크 위 우클릭 메뉴는 계속 사용할 수 있으며, 수식키 없이도 링크 표시가 가능하지만 팝업·배너 등 기존 차단 조건은 적용된다. 클릭·hover 동작 자체는 바뀌지 않는다.

- **알림 목록이 모든 윈도우의 최신 생성 50개를 반환한다.** 알림 ID를 인스턴스 안에서 공유해 윈도우 사이의 중복을 없앴고, 포커스와 무관하게 생성 순서로 정렬한다. 화면의 알림 패널·읽음 처리·기존 병합 동작은 윈도우별로 유지한다.

- **시스템 정보에 워크스페이스 소유 ID를 추가했다.** 기존 창별 count/index를 유지하고 각 값이 어느 워크스페이스에 속하는지 함께 반환한다. 창 목록에서도 같은 정보를 조회할 수 있다. 전체 워크스페이스 목록의 전역 조회 규칙은 유지한다.

- **출처 서피스를 지정해 파일을 열어도 기존 탭 선택을 유지한다.** 비터미널 결과를 새 탭으로 추가하거나 핸들러 선택 창을 거쳐도 해당 페인의 활성 탭은 바뀌지 않는다. 출처를 생략한 사용자 새 탭 동작은 유지한다.

- 비동기 파일 형식 판별과 핸들러 선택 중에도 명시한 출처 페인을 유지한다. 출처가 닫히면 해당 열기 작업을 중단한다.

- **방향키의 보조키가 터미널 프로그램에 전달된다.** Shift·Ctrl·Alt(macOS Option)+방향키가 일반 방향키와 같은 입력으로 보내지던 문제를 고쳤다. Option as Meta 설정과 무관하게 조합을 구분하며, 일반 방향키의 애플리케이션 커서 모드와 Tasty 단축키 우선순위는 유지한다.

- **최근 파일 목록이 윈도우 사이에서 즉시 일치한다.** 다른 윈도우에서 연 파일도 마크다운 최근 목록과 주소창 후보에 반영된다. 종류별 최신순, 중복 제거, 최대 10개 상한과 기존 명령 형식은 유지한다.

- **찾아보기 파일 선택 창이 부모 팝업과 함께 숨고 돌아온다.** 다른 워크스페이스·탭으로 이동하면 부모와 함께 숨으며 키보드 입력을 막지 않는다. 돌아오면 입력·선택·대기 요청을 이어가고, 부모를 닫으면 기존처럼 한 번 취소된다.

- **긴 파일명이 크기·수정일을 덮던 문제를 수정했다.** 이름 열의 남은 폭에서 말줄임하고 선택·열기에는 전체 이름을 사용한다.

- 종료를 확정하면 모든 창의 웹 페이지·마크다운을 즉시 가려 종료 진행 화면을 덮지 않게 한다. 종료 확인 취소와 백그라운드 최소화는 기존 동작을 유지한다.

- 허용된 플러그인 네임스페이스 호출이 모든 설치 플러그인을 시작하던 문제를 수정했다. 이제 해당 소유 플러그인과 필요한 활성 IPC 훅 확장만 시작한다. 소유 플러그인이 비활성 상태이거나 호출을 거절하면 다른 플러그인도 시작하지 않는다.

- **에이전트 호출 제한과 사용량 기록을 모든 IPC 진입 경로에 한 번씩 적용한다.** GUI·headless의 앱 명령, 플러그인 명령, 합산 조회가 검사를 건너뛰던 문제와 일반 처리 경로의 중복 집계를 수정했다. 세션 토큰 없는 로컬 CLI 예외는 유지한다.

- **설치되지 않은 플러그인을 켜거나 끄면 오류로 답한다.** `tasty plugin enable <id>`와 `disable <id>`가 미설치 ID에도 성공하던 것을 고쳤다. GUI·헤드리스 모두 패키지 존재를 먼저 확인하며 실패한 요청은 설정 파일을 만들거나 변경하지 않는다.

- 상대 `TASTY_HOME`으로 실행해도 플러그인이 본체와 같은 사용자 번역 파일을 읽는다. 플러그인 작업 디렉터리를 기준으로 상대 경로를 잘못 해석하던 문제를 수정했다.

- 웹훅의 chunked body가 무효 UTF-8을 포함해도 요청당 바이트 상한을 넘으면 `413 payload too large`로 거부하며 시퀀스를 실행하지 않는다.

- **Agent Stream 웹훅 연동 안내를 실제 동작에 맞췄다.** 발급 URL 경로, 인증 실패의 고정 응답, 제한 종류별 재시작 복원 여부, 인증 전에 적용하는 요청 본문 상한을 명확히 했다.

- **POSIX 셸에서 Codex 실행 시 사용자 alias·함수를 우회한다.** `launch`/`spawn`/`respawn`(Windows Git Bash 포함)과 Linux·macOS의 `reboot`가 외부 실행 파일을 사용해 alias의 `--dangerously-bypass-approvals-and-sandbox`와 기본 `-a never` 충돌을 막는다. 승인·샌드박스 기본값과 명시 옵션은 유지한다. Windows `reboot`의 alias·함수 우회는 아직 지원하지 않는다.

- **원격 경로를 새 로컬 터미널의 시작 폴더로 사용하지 않는다.** mirror 탐색기에 포커스가 있을 때 새 로컬 워크스페이스·탭·분할, 프리셋의 터미널 폴더, 상태바 Git 조회가 원격 경로를 사용하던 문제를 고쳤다. 해당 경로를 상속하지 못하면 홈에서 시작하며 `tasty new workspace --cwd`의 명시값은 유지한다. 원격 서피스의 상태바에는 로컬 디스크로 잘못 조회한 브랜치를 표시하지 않는다.
- **중단한 Codex 자식도 대기 종료로 처리한다.** 승인 거절·Esc·Ctrl-C가 `Interrupt`만 보내 완료 이벤트가 없으면 상태가 계속 `active`로 남던 문제를 고쳤다. 이제 `spawn`/`tell`로 기다리는 호출자에게 중단을 알린다.

- **플러그인 팝업에 한글·일본어·중국어 IME 입력을 전달한다.** 호스트가 보내던 여섯 종류의 입력에 IME 조합이 없어 확정 문자가 사라지던 문제를 고쳤다. 이제 조합의 네 단계를 전달해 입력란에 preedit과 확정 문자를 표시한다. 팝업이 겹치면 문자·Esc처럼 최상단만 받는다. 영문·Esc·Enter 동작은 유지한다.
- **플러그인 입력란의 캐럿 위치로 IME 후보창을 배치한다.** 이전에는 터미널 커서 위치만 사용해 후보창이 입력란에서 떨어져 나타났다. 이제 플러그인이 렌더링한 입력란의 위치를 호스트에 전달하고 호스트가 OS에 알린다. 원격 attach mirror에는 이 정보를 전송하는 경로가 없어 아직 적용하지 않는다.

- **플러그인 간 호출에도 원래 오류 코드를 전달한다.** 권한 거부·대상 없음·만료·취소 등 호스트 오류가 플러그인 호출자에게 기본 코드로 바뀌던 문제를 고쳤다. 확장 후처리 훅이 있는 메서드에서도 대상 플러그인의 `-32602` 같은 코드를 그대로 전달한다. CLI·IPC와 플러그인 호출자가 같은 원인을 구분할 수 있다.
- **debug 확장 훅 호출이 무응답이면 시간 초과로 끝난다.** `tasty debug extension invoke-hook`(IPC `debug.extension.invoke_hook`)에 매니페스트가 허용하는 최대 1초 제한을 적용했다. 이전에는 응답이 없으면 호스트 종료까지 기다렸다. release에는 이 명령이 없다.

## [0.10.3] - 2026-09-09

### Added

- **`plugin.show`가 서피스 종류의 선언과 실제 등록을 구분한다.** 기존 `surface_kinds[].rendering`을 `declared_rendering`(매니페스트 선언), `registered`(해당 플러그인으로 등록됐는지), `effective_rendering`(실제 렌더링 방식), `registered_by`(`host` 또는 플러그인 ID)로 나눴다. 같은 이름이 다른 소유자로 등록돼 있으면 `registered`는 false다. headless의 webview·remote 제외, egui-mesh 허용 목록·api_version 검사, 호스트 종류의 remote 재선언 무시처럼 선언과 등록이 달라지는 경우를 확인할 수 있다. 기존 rendering 필드는 제거했으며 당시 0.x API이고 저장소 내 소비자가 없어 BREAK로 분류하지 않았다.
- **등록된 서피스 종류를 조회하는 `surface.kinds`를 추가했다.** CLI는 `tasty list surface-kinds`다. 이전에는 실제 생성 가능 여부를 알려면 탭이나 워크스페이스를 만들어 보아야 했고, 플러그인 선언 조회에는 호스트 종류(`terminal`·`empty`·`explorer`·`dag_graph`)도 없었다. 이제 종류별 표시명 번역 키, 아이콘, 실제 `rendering`(`host-egui`/`egui-mesh`/`webview`/`remote`), `source`(`host`/`plugin` 및 `plugin_id`), 필수 params를 이름순으로 반환한다. headless는 webview·remote 선언을 등록하지 않아 빌드별 결과가 다르다. 2026-09-08 번들 플러그인 9개 기동 후 측정은 GUI 8종, headless 6종이었다. 조회는 화면을 생성하지 않는다.
- **이미지 파일의 외부 변경 감지와 수동 reload를 추가했다.** CLI `tasty image reload --surface <ID>`와 IPC `image.reload`를 제공한다. 이전에는 툴바 새로고침만 가능했다. 먼저 stat으로 변화를 확인한 뒤 파일을 읽어 내용 해시를 비교하며, 내용이 같으면 다시 디코딩하지 않는다. 측정한 11210x4992 PNG는 디코딩에 1.84초, 읽기의 100~250배가 걸렸고 단일 플러그인 워커도 그동안 대기했다.

  같은 mtime 구간의 연속 쓰기를 확인하려고 쓰기 직후 짧은 기간에는 해시를 추가 비교한다. 측정한 FAT32 mtime 간격은 2.000초로 폴 주기의 두 배였다. 이미지 편집 중에는 밑그림이 바뀌지 않도록 적용을 미루고 편집 종료 후 반영한다.
- **debug 모달 닫기 요청을 추가했다.** `debug.modal.close_request`(CLI `tasty debug modal close-request`)는 모달에 창 닫기 요청을 전달하고 `{"closed": true|false}`를 반환한다. false는 닫을 모달이 없다는 뜻이다. 설정 모달은 당시 `SettingsView::handle_event`에 Escape 처리가 없고 `open_settings_modal`도 열린 상태에서 반환해 `Ctrl+,`로 닫을 수 없었다.

  창 관리자 없는 X 서버의 2026-09-07 측정에서 `xdotool windowclose`는 `WM_DELETE_WINDOW` 대신 `XDestroyWindow`를 호출해 winit을 panic시켰고, `wmctrl -i -c`의 `_NET_CLOSE_WINDOW`는 처리할 창 관리자가 없어 효과가 없었다. 그래서 닫기 요청 이후의 처리를 직접 호출하도록 했다. 사용자 조작 재현이므로 debug에서만 제공하며, release `window.close`의 대상에 모달을 포함하지 않는 원칙은 유지한다.

- **Claude·Codex 훅 응답에 호스트 호출 실패 수를 추가했다.** `claude.hook`, `codex.hook`과 대응 CLI 출력에 `host_call_failures`를 항상 숫자로 반환한다. 일부 호스트 호출은 실패해도 훅의 ok 응답을 유지하므로, 이전에는 모두 성공한 경우와 응답으로 구별할 수 없었다. 이런 호출만 집계하며 실패를 즉시 오류로 반환하는 Codex의 `terminal.set_state` 등은 제외한다. 값이 0이 아니면 `<tasty_home>/hook-failures.log`에도 기록해 CLI 출력을 버리는 훅 실행에서도 실패를 확인할 수 있다.
- **CLI 도움말에 설정 언어를 적용했다.** 실행 시 도움말 트리를 번역해 `general.language`에 맞는 `tasty --help`, `tasty -a`, 하위 명령 도움말과 인자 오류 안내를 표시한다. 번역이 없으면 영어를 사용하며 영어 출력은 유지한다. 루트 화면과 `tasty list`, `send`, `read`, `move`, `new`, `split`, `close`, `set`, `unset`, `tool`, `telemetry`, `plugin`, `agent`, `memory`, `approval`, `webhook` 하위 명령에 한국어·일본어를 추가했다. 사용자 언어팩도 `cli.help.*` 키로 번역할 수 있다. clap이 만드는 `Usage:`, `Options:`, `Commands:` 등의 형식 문구는 영어로 남는다.

- **마우스 추적 상태를 조회하는 `surface.mouse_tracking`을 추가했다.** CLI는 `tasty surface mouse-tracking --surface N`이다. `terminal_mode`·`terminal_tracking`은 DECSET 1000/1002/1003의 실효 상태, `sgr`은 1006 상태를 반환한다. `effective_click_mode`·`effective_click_tracking`은 실제 클릭 처리에 적용할 상태이며 `degraded_by`로 차이의 이유를 알린다. hard 점유의 읽기 전용 서피스나 마우스 캡처 차단 목록의 전경 프로세스에서는 클릭 보고를 끈 것처럼 처리한다.

  이를 통해 터미널이 추적을 요청하지 않은 경우와 호스트가 클릭 보고를 막은 경우를 구분할 수 있다. 드래그 선택을 앱에 보낼지 판단하는 데도 사용한다. 사용자 입력을 재현하지 않는 조회이므로 release에도 제공한다. 개별 레지스터 조합은 반환하지 않으며 휠은 클릭 제한과 별도다. 따라서 클릭 추적이 false여도 휠 보고는 가능하고, 이 조회만으로 PTY에 실제 보고가 전달됐는지는 알 수 없다.

- **워크스페이스·카테고리 이동에 ID 인자를 추가했다.** `workspace.move`와 `workspace_category.move`는 `id`를 받는다. CLI는 `tasty move workspace --id N --to M`, `tasty workspace-category move --id N --to M`이다. 기존 `from_index`는 활성 창 안의 순번이라 여러 창에서 사용자 포커스에 따라 대상이 달라졌다. ID는 인스턴스 전체에서 유일하므로 소유 창을 먼저 결정하고 그 창의 `to_index`로 이동한다. `tab.move`가 `pane_id`로 대상을 정하는 방식과 같다. 기존 from_index 호출은 유지하되 id와 함께 보내면 거절한다.

- **Markdown 표의 셀 손실을 검사한다.** 행별 셀 수가 헤더와 같은지, 표 본문에 다른 표의 구분행이 들어갔는지 확인한다. 코드 스팬 안의 이스케이프하지 않은 `|`도 셀을 나누며, 열 수가 같은 두 표를 빈 줄 없이 붙인 경우는 셀 수 검사만으로 찾지 못한다. 기존 소스 검사와 리뷰에서 세 문서의 렌더 손실을 놓친 사례에 대응했다. 당시 검사 범위에는 코드펜스 짝, 헤더·구분행 열 수 불일치, 들여쓴 표, 링크, 셀 내 HTML이 포함되지 않았다.
- IPC에만 있던 release 메서드 여섯 개에 CLI를 추가했다. `tasty list theme`(적용 테마), `tasty list recent --kind <kind>`(종류별 최근 파일), `tasty set cwd --surface <id> [--path <p>]`(원격 서피스의 보고 경로), `tasty set url --surface <id> --url <u>`(웹뷰 주소), `tasty file-handler dispatch <path> [--depth cheap|deep] [--origin-surface <id>] [--ignore-size-limit]`(탐색기와 같은 파일 열기), `tasty telemetry record-batch --events <json 배열>`(같은 시각의 순서 있는 이벤트 기록)이다. 당시 실행 중인 인스턴스로 호출을 확인했다.

- debug IPC에 CLI 진입점 15개를 추가했다. 조회는 `tasty debug focused-surface`, `selection`, `pending-menu`, `ui-state`, 탐색 재현은 `switch-workspace --index`, `switch-tab --index`, `close-workspace --index`다. `feed-bytes --surface (--text | --bytes <hex>)`는 파서에 바이트를 직접 전달한다. 입력 주입은 `inject key`, `inject mouse`, `inject window-mouse`, `inject egui-mouse`, `inject egui-key`, 플러그인 배너 조작은 `plugin-banner open|close`다. 호스트 배너의 `debug banner`와는 다르다. 모두 debug 빌드에만 제공한다. 실제 핸들러의 인자 이름과 맞추고 인스턴스에서 각 요청의 전달을 확인했다.

- `tasty memory secret list`에 `--since`, `--until`, `--offset`을 추가했다. 서버가 이미 지원하던 시간 필터와 페이지 이동을 CLI에서도 사용할 수 있다. 일반 `tasty memory list`처럼 `updated_at`이 since 이상, until 미만인 범위를 Unix 밀리초로 지정한다. offset은 limit과 함께 사용하는 건너뛸 개수다.
- **배너 조회에 표시 영역을 추가했다.** `debug.banner.list`(CLI `tasty debug banner list`)는 논리 좌표의 셸 `rect`, 물리 픽셀의 플러그인 합성 영역 `content_rect`, 좌표계를 설명하는 `coords`를 반환한다. host 배너의 content_rect는 null이다. rect는 `debug.host_popup.list`와 같은 키 형식이며 기존 hover 측정값 `card_rects`를 사용해 한 프레임 늦고 첫 프레임에는 null이다. 당시 mouse-capture 배너 측정은 배율 1에서 논리 50.0, 배율 2에서 49.5로 물리 50·99px였다. 물리 99는 논리 49.5 × 2의 결과다. debug 전용이며 release에는 없다.
- **headless에서 플러그인을 개별 활성화·비활성화할 수 있다.** `plugin.enable`·`plugin.disable`(CLI `tasty plugin enable|disable <id>`)의 `-32017` 거절을 해소했다. 이전에는 플러그인 네임스페이스를 호출해 설치된 9개를 모두 시작하는 간접 경로만 있었다. 2026-09-09 격리 홈 측정에서 부팅 후 9개 모두 running=false였고 `plugin enable com.tasty.image`는 이미지만 시작했다. 당시 네임스페이스 경로는 여전히 전체를 시작했다. 응답·오류·`plugin.enabled`/`plugin.disabled`/`plugin.unloaded` 이벤트는 GUI와 같은 함수로 처리한다. 당시 `plugin.install`, `remove`, `grant`, `revoke`, `upgrade_builtins`, `audit_follow` 여섯 메서드는 계속 `-32017`로 거절했다. [ADR-0003](docs/adr/0003-headless-behavior.md).
- **headless debug에서 Lua·이벤트·확장 조회를 지원한다.** `debug.lua.eval`, `debug.event_bus.list_subscribers|publish|trace`, `debug.extension.invoke_hook` 다섯 메서드가 기존 `-32601` 대신 동작한다. 창 없이 Lua 엔진·플러그인 관리자만으로 처리할 수 있지만 GUI 라우터에만 연결돼 있던 문제를 고쳤다. release에서는 계속 `-32601`이며 창·렌더러·입력 큐를 요구하는 나머지 31개는 headless에서 지원하지 않는다.
- **headless에서 앱 계층 메서드 다섯 개를 지원한다.** `clipboard.set_text`, `remote.workspaces`, `agent.task_await`, `approval.await`와 debug의 `system.shutdown`이 기존 `-32601` 대신 동작한다. shutdown을 통해 데몬을 프로토콜로 종료할 수 있다. 당시 창을 요구하는 `window.*`, `view.*`, `ui.screenshot`, `remote.attach`, `system.gpu_stats`는 계속 `-32601`로 거절했다.
- **화면 텍스트 조회에 터미널 상태 정보를 추가했다.** `surface.screen_text`, `pty.read`(CLI `tasty read screen`, `tasty pty read`)가 `is_terminal`, `scrollback_len`, `alt_screen`을 반환한다. `--lines N`보다 적게 받았을 때 스크롤백이 없는지 조사할 수 있다. TUI에서 primary 스크롤백이 비어 있으면 lines를 늘려도 더 반환할 내용이 없다. 스크롤백이 있어도 전체 보존량이 N보다 적으면 적게 반환하는 것이 정상이다. 비터미널 서피스나 없는 대상은 is_terminal=false, 다른 두 값은 null이며 빈 스크롤백의 0과 구분한다.
- `general.wheel_line_scroll`(설정 > 일반 > 휠 스크롤 거리)을 추가했다. 휠 한 칸의 논리 포인트 거리를 기본 50, 범위 10~200으로 조정한다. 같은 창의 호스트 위젯과 플러그인 화면에 같은 값을 적용한다.
- `agent.semaphore_set_permits`(CLI `tasty agent semaphore-set-permits --workspace-id <id> --name <n> --permits <N>`)로 한도를 직접 변경할 수 있다. 기존의 삭제·재생성·재획득 사이에 세마포어가 없어지는 문제를 피한다. 확대는 즉시 적용하고 축소는 기존 점유를 유지한 채 새 acquire를 제한한다. `permits_available`은 음수 대신 0을 반환한다. 권한은 `agent`다.
- `agent.semaphore_acquire`에 선택 인자 `ttl_ms`(CLI `--ttl-ms`)를 추가했다. 지정하면 now + ttl_ms 이후의 다음 acquire/list에서 점유를 회수한다. 같은 holder로 다시 획득하면 기한을 갱신한다. lease의 ttl_ms와 같은 방식이다. 생략하면 만료하지 않는다. 진행 중인 작업의 점유를 시간만으로 회수해 두 작업이 동시에 임계구역에 들어가지 않도록 한 기본값이다(ADR-0042).
- **번들 Agent Stream 플러그인**(`com.tasty.agent-stream`)을 추가했다. 서피스의 에이전트 transcript를 읽어 `text`, `thinking`, `tool_use`, `turn_end` 이벤트를 만들고 커서 조회 `agent_stream.poll`과 SSE `agent_stream.serve`의 `GET /events`로 제공한다. CLI는 `tasty agent-stream watch|turn-start|unwatch|list|poll|serve|serve-stop|serve-info`다. `turn-start` 이후 이벤트에는 호출자의 `request_id`가 붙어 웹훅 시퀀스에서 `claude.tell` 전에 사용할 수 있다. watch 중이 아니거나 이미 턴이 열렸다면 거절한다. serve는 명시 포트가 필요하고 다른 포트로 자동 전환하지 않으며 serve-info는 토큰을 반환하지 않는다.
- `timer.list`(CLI `tasty list timers`)로 등록된 타이머를 조회할 수 있다. 유휴 상태에서 계속 깨어나는 원인을 조사할 때 사용한다.
- `memory.goal_set`, `memory.goal_get`, `memory.goal_clear`(CLI `tasty memory goal set|get|clear`)로 서피스별 세션 목표를 저장·조회·삭제할 수 있다.
- **원격 attach 선택 창에서 워크스페이스를 만들고 바로 연결할 수 있다.** 첫 행의 ‘+ 새 워크스페이스’를 선택하면 목록 조회에 사용한 터널로 `workspace.create`를 한 번 보내고 기존 Connect와 같은 경로로 mirror를 연다. 이름·cwd는 원격 기본값을 사용한다. 원격 워크스페이스가 0개일 때도 이 창에서 계속 진행할 수 있다.
- macOS 부팅 직후 인터랙티브 캡처에 필요한 화면 기록 권한을 요청하고, 전체 디스크 접근 권한이 없는 것으로 판단되면 설정 패널 안내를 한 번 표시한다. 앱 번들의 TCC usage description으로 요청 이유도 보여 준다. 기능을 처음 사용할 때 실패하던 권한 확인을 시작 단계로 옮겼다.
- 이벤트 루프 콜백이 5초 안에 반환하지 않으면 워치독이 `~/.tasty/crash-reports/hang-<타임스탬프>.log`를 남긴다. 모든 빌드에 적용하며 한 정지 구간당 한 번 기록한다. 응답하지 않는 콜백·처리 단계를 조사하기 위한 기능이고 자동 복구하지는 않는다.
- **사용자 언어팩을 발견하고 선택할 수 있다.** `~/.tasty/lang/<code>/pack.toml`을 하나의 팩으로 취급한다. `[meta] name`은 표시 이름이며 없으면 코드를 사용한다. `[font]`에는 `builtin = true`, `file`, `family`, `candidates` 중 하나가 필요하다. 문자열은 영어 기본값 위에 적용하고 빈 값은 번역 없음으로 처리한다. Settings › General의 언어 목록에 내장 en/ko/ja와 발견한 팩을 표시한다(`tasty_i18n::available_languages`). 내장 `lang/{en,ko,ja}.toml`에도 meta name을 추가했다.
- **언어팩 폰트를 실제 UI에 적용한다.** 부팅 때 `[font]`의 file/family/candidates/builtin을 파일로 해석해 셸 UI 두 경로와 클립보드·Git·이미지·마크다운 플러그인의 글꼴 대체 목록 끝에 추가한다. 기본 라틴·시스템 CJK 글꼴에 없는 문자는 팩 글꼴을 사용한다. 호스트와 플러그인이 같은 `ab_glyph` 검증기를 사용하며, file이 절대 경로나 `..`로 팩 디렉터리 밖을 가리키면 거절한다.

  파일 해석이나 검증 실패 시 다른 팩 폰트를 임의로 선택하지 않고 경고 토스트를 한 번 표시한다. 번역 문자열은 유지하며 지원하지 않는 문자는 □로 보일 수 있다. egui의 양방향 텍스트 미지원으로 아랍어·히브리어 등 RTL 어순은 여전히 지원하지 않는다. [ADR-0040](docs/adr/0040-locale-catalogs-and-display-text.md).
- **워크스페이스 전체를 닫는 `workspace.close`를 추가했다.** CLI `tasty close workspace --id <W>`로 내부 페인·탭·서피스를 함께 닫는다. 이전 release API에는 생성만 있어 서피스를 하나씩 닫아야 했다. id 또는 index로 대상을 지정하며 활성 워크스페이스가 닫힐 때만 이웃으로 전환한다. 에이전트가 닫은 터미널은 복원 목록이나 스크롤백에 남지 않아 되돌릴 수 없다.

  호출자 자신의 서피스를 포함한 워크스페이스, 마지막 워크스페이스, 원격 attach가 hard 점유한 서피스를 포함한 워크스페이스, 원격 mirror 워크스페이스는 거절한다. 창 자체의 닫기는 `window.close`로 지정한다. 권한은 `SurfaceWrite`다. 근거 ADR-0017.
- `tasty close window --id <N>`을 기존 `window.close` IPC에 연결했다. 마지막 main 창을 거절하는 기존 규칙은 유지한다.
- IPC에만 있던 세션 토큰 발급·폐기·조회에 `tasty session issue|revoke|list` CLI를 추가했다.
- `tasty surface cursor-position|foreground-process|locate|respawn-terminal|fire-hook --surface <id>`을 추가했다. 각각 커서 위치, 전경 프로세스, 소속 페인·생존 여부, 셸 재시작, 훅 실행에 대응하는 기존 IPC를 호출한다.
- `tasty send text --wait-idle`은 사용자 입력 중이면 `"sent": false`, `"reason": "typing"`을 반환하고 보내지 않는다. 입력 여부 확인과 전송을 한 요청에서 처리해 `is-typing` 조회 뒤 전송까지의 틈을 줄였다.
- **attention 표시를 지우는 `surface.attention.clear`를 추가했다.** CLI는 `tasty surface attention clear --surface <id> [--kind completion|needs_input]`이다. kind를 지정하면 현재 기록과 일치할 때만 지우며, 생략하면 종류와 관계없이 지운다. 알 수 없는 값은 거절한다. 표시가 없어도 성공하며 `cleared`와 `previous_kind`로 실제 결과를 반환한다. 이전에는 화면의 실제 포커스나 알림 읽음 같은 GUI 동작으로만 지울 수 있었다.

  없는 서피스, 원격 attach의 hard 점유 서피스, mirror 서피스는 거절한다. 점유·mirror의 표시는 다른 인스턴스가 소유하기 때문이다(ADR-0021·ADR-0024). mirror 사용자가 실제로 보고 확인한 해제는 계속 소유 인스턴스로 전달한다. 권한은 `Notification`이다.
- `surface.attention.get`(CLI `tasty surface attention get --surface <id>`)으로 `completion`, `needs_input`, null 중 현재 표시 종류를 조회한다. 읽기 전용이므로 mirror·점유 중에도 허용하며 권한은 `Notification`이다.
- **플러그인·에이전트에 `remote.workspaces` 조회를 허용했다.** 이전에는 권한 표에 없어 `UnknownMethod`로 거절했고 로컬 CLI `tasty remote workspaces`에서만 사용할 수 있었다. 이제 `plugin(&[])`로 등록해 원격 또는 같은 머신의 다른 인스턴스를 조회할 수 있다. 소켓 접근·SSH 연결을 신뢰 기준으로 삼아 `remote.profile.*`처럼 추가 권한을 요구하지 않는다. 로컬 mirror 워크스페이스를 생성하는 `remote.attach`는 사용자 상태를 바꾸므로 계속 Local 호출자만 허용한다.

### Changed

- **웹훅 도움말에 네트워크·인증·응답 조건을 명시했다.** `tasty webhook --help`는 등록 출력의 `http://127.0.0.1:<port>/<id>`가 표시용이며 실제 리스너는 `0.0.0.0`에 바인딩한다고 설명한다. 서명 검증은 없고 선택적인 고정 공유 토큰을 상수시간 비교한다. `--auth-*`를 생략하면 포트에 접근한 누구나 시퀀스를 요청할 수 있다. persistent 토큰은 `~/.tasty/webhooks.toml`에 평문으로 저장한다.

  `200`은 실행 완료가 아닌 접수 응답이며 시퀀스보다 먼저 확정한다. 한 단계가 실패해도 다음 단계를 실행하므로 부분 적용될 수 있고 결과는 로그로 확인한다. 만료 전용 타이머는 없으며 만료 후 첫 호출은 `410`, 이후는 `404`다. sweep의 정리 목적도 설명한다. 긴 도움말만 확장하고 `-h` 목록은 유지한다.
- **승인의 실행 중 상태와 이력을 도움말에서 구분했다.** `tasty approval --help`는 request/respond/await/cancel/list/get이 부팅마다 비어 있는 메모리 저장소를 사용한다고 설명한다. 재시작 전 pending 요청은 list에 없고 같은 ID의 await도 not found다. 저장된 기록은 memory store의 history로 조회한다. 긴 도움말을 보완하고 짧은 `-h` 목록은 유지한다.
- **승인 요청의 만료 시점을 도움말에 명시했다.** `tasty approval request --help`는 timeout이 자동으로 상태를 바꾸지 않고 `approval await`에서 처리된다고 설명한다. 대기 호출이 없으면 timeout 이후에도 pending 목록에 남는다. default-choice는 responded 선택이 아니라 해당 키를 담은 시스템의 timed_out 결과이며 `approval history --decision`으로 조회하는 응답 결정과 다르다. 긴 도움말과 옵션 설명을 보완하고 `-h` 목록은 유지한다.
- **세션 목표의 수명과 상속 여부를 도움말에 명시했다.** `tasty memory goal --help`는 자식 서피스가 목표를 상속하지 않고 별도 TTL도 없다고 설명한다. 서피스를 닫거나 부팅 때 복원되지 않은 서피스를 정리하면 메모리 스코프와 목표도 삭제된다. 긴 도움말만 확장하고 `-h` 목록은 유지한다.
- **blackboard 복원이 전체 필드 교체임을 명시했다.** `tasty memory bb snapshot-restore --help`는 현재 필드를 모두 지운 뒤 스냅샷을 복원하므로 나중에 추가한 필드도 사라진다고 설명한다. `_meta`는 있으면 유지하고 없을 때만 스냅샷에서 복원해 이전 schema로 반드시 돌아가는 것은 아니다. 긴 도움말만 확장하고 `-h` 목록은 유지한다.
- **blackboard 삭제가 스냅샷도 지운다고 명시했다.** `tasty memory bb delete --help`는 같은 키 접두어에 저장된 스냅샷이 함께 삭제되며 보드 삭제 후 복구용 백업이 아니라고 설명한다. 긴 도움말만 확장하고 `-h` 목록은 유지한다.
- **secret 저장값은 디스크에서 암호화하지 않는다고 명시했다.** `tasty memory secret --help`는 소유자 분리가 IPC에만 적용되고 같은 `memory.db`에 평문 바이트를 저장한다고 설명한다. 파일 접근자는 읽을 수 있으며 secret의 export 제외가 암호화를 뜻하지는 않는다. 민감한 값은 OS 키체인이나 별도 접근 권한이 있는 파일에 보관하고 여기에는 경로를 두도록 안내한다. 긴 도움말만 확장하고 `-h` 목록은 유지한다.
- **실제 호출 제한에 쓰는 metric을 도움말에 명시했다.** `tasty agent rate-limit-set --help`와 metric 설명은 IPC 미들웨어가 `ipc_calls`만 소비한다고 밝힌다. 다른 이름의 버킷도 저장·조회되지만 호출을 제한하지 않는다. 초과는 `-32010 throttled`와 감사 로그의 Deny로 남는다. 토큰 없는 로컬 호출, 호스트 호출, `telemetry.*`, `agent.rate_limit_*`, `system.info`는 제외하며 마지막 두 종류는 제한된 에이전트가 상태를 확인하고 한도를 조정할 수 있게 한다. 짧은 `-h` 목록은 유지한다.
- **작업 생성과 러너 시작을 도움말에서 구분했다.** `tasty agent task-create`·`task-run --help`는 러너를 켜기 전에는 작업을 전달하지 않으며 부팅도 상태 복구만 한다고 설명한다. 따라서 생성 후 바로 await하면 기본 10분 동안 작업이 진행되지 않을 수 있다. 러너 stop은 이미 실행한 프로세스를 종료하지 않고 완료 감지를 멈춘다. 다시 start하면 기존 handle의 생존 상태와 watcher의 종료 코드를 확인해 작업을 마감한다. 긴 도움말을 보완하고 `-h` 목록은 유지한다.
- **barrier 만료가 조회·신호 시 처리된다고 명시했다.** `tasty agent barrier-create --help`는 타이머 대신 다음 barrier-signal/state/list에서 TimedOut으로 바뀐다고 설명한다. 이후 signal은 실패하며 기록은 delete 전까지 남는다. `--timeout-ms`를 생략하면 만료하지 않는다. 긴 도움말만 확장하고 `-h` 목록은 유지한다.
- **세마포어 점유 실패의 응답을 도움말에 명시했다.** `tasty agent semaphore-acquire --help`는 여유가 없어도 호출 자체는 성공하고 본문에 `acquired: false`와 현재 holder 목록을 반환한다고 설명한다. 대기 기능은 없으며 호출자가 재시도해야 한다. lease의 block 모드와 같은 방식이고 세마포어는 다른 모드를 선택할 수 없다. 긴 도움말만 확장하고 `-h` 목록은 유지한다.
- **lease의 block 모드가 대기하지 않음을 명시했다.** `tasty agent lease-acquire --help`는 충돌 시 `acquired: false`와 현재 holder를 즉시 반환하고 종료 코드 0으로 끝난다고 설명한다. fail 모드는 같은 충돌을 오류로 반환한다. 재시도는 호출자가 결정해야 한다. TTL을 생략하면 만료하지 않아 같은 holder ID로 release해야 하며, 재시작 정리는 Running 작업이 자기 task ID로 점유한 lease만 회수한다. 사용자 지정 holder는 보존한다. 지정한 기한도 다음 acquire/list에서 확인한다. 긴 도움말과 두 옵션을 보완하고 `-h` 목록은 유지한다.
- **플러그인 비활성화의 유지 범위를 도움말에 명시했다.** `tasty plugin disable --help`는 비활성 상태가 재시작·재설치 후에도 남고 파일 판별기·핸들러, 훅, 완료 전략, 설정 페이지를 해제한다고 설명한다. 해당 파일이 다른 핸들러로 열릴 수 있다. 설치된 매니페스트의 네임스페이스 소유권은 유지하므로 호출은 없는 메서드가 아니라 `plugin '…' is not running`으로 거절한다(ADR-0026). 긴 도움말만 확장하고 `-h` 목록은 유지한다.
- **기본 플러그인 제거를 되돌리는 명령을 안내한다.** `tasty plugin remove --help`에 제거 기록 때문에 다음 시작에서 재설치하지 않는다는 점, 플러그인 디렉터리 전체를 삭제한다는 점, `plugin upgrade-builtins --restore-removed <id>`로 복구할 수 있다는 점을 적었다. 동작과 짧은 `-h` 목록은 유지한다.
- **플러그인 설치의 권한·기동 동작을 안내한다.** `tasty plugin install --help`에 파일 복사 외에 선언 권한을 별도 확인 없이 부여하고 비활성 기록이 없으면 바로 활성화·시작한다고 명시했다. `plugin permissions <id>`와 `plugin revoke <id> <permission>`로 확인·회수하는 방법도 추가했다. 짧은 `-h` 목록은 유지한다.
- **텔레메트리의 자동 기록과 보존 범위를 명시했다.** `tasty telemetry --help`는 호스트가 에이전트 IPC 호출에 method 태그의 `ipc_calls`를 기록하며 호스트 호출·`telemetry.*`·throttled 호출은 제외한다고 설명한다. 조회는 최근 2만 개 raw 이벤트를 사용하고 영속 요약은 없으므로 더 오래된 since를 지정해도 보존된 부분만 반환한다. anomaly는 최근 5천 개·최대 50시간을 보관한다. 긴 도움말만 확장하고 `-h` 목록은 유지한다.
- **SSH 프로필과 attach 프로필을 도움말에서 구분했다.** `tasty tool remote-profile add-ssh --help`는 ssh가 연결 정보만 저장하고 attach는 tasty-attach 종류를 요구한다고 설명한다. tasty-attach가 `--ssh-ref`로 SSH 프로필을 참조한다. 긴 도움말만 보완하고 `-h` 목록은 유지한다.
- **번들 플러그인은 내용이 달라진 파일만 다시 설치한다.** 부팅마다 전체를 복사하던 동작을 바꿨다. 같은 번들이면 해당 파일을 다시 쓰지 않으며 당시 debug 측정의 45파일·약 1.1 GB 반복 쓰기가 줄었다. mtime에 의존하지 않아 `cp -p`나 아카이브 해제로 시각이 유지돼도 내용 차이를 반영한다. `tasty plugin upgrade-builtins`도 같은 비교를 사용해 같은 버전의 변경 파일을 복사하고 처리 이유를 알린다.
- **부팅 시 더 높은 버전의 설치 플러그인을 유지한다.** 이전에는 번들로 덮어써 버전이 내려갈 수 있었다. 이제 높은 설치 버전은 건너뛰며 의도적으로 낮추려면 `tasty plugin upgrade-builtins --force`를 사용한다.
- **비활성 플러그인의 메서드는 실행 중이 아님을 알린다.** 네임스페이스 소유권을 실행 프로세스 대신 설치 매니페스트에서 결정해, `-32601 Method not found` 대신 `-32002 plugin '<id>' is not running`을 반환한다. 이전에는 같은 이름의 호스트 구현으로 넘어가 결과도 달라졌다. 예를 들어 마크다운을 끄면 `markdown.navigate`가 호스트의 `-32602`로 바뀌었다. 이제 둘 다 -32002로 처리하며 설치되지 않은 이름은 계속 -32601이다.
- **headless에서 잘못된 메서드 이름만으로 플러그인을 시작하지 않는다.** 디스크의 설치 정보를 먼저 확인한다. 이전에는 알 수 없는 이름도 번들 전체를 시작해 측정에서 응답 1272 ms, 프로세스 9개, 자식 RSS 약 47 MB를 사용했다. 수정 후 같은 오타는 98 ms, 프로세스 0개였다. 실제 플러그인 메서드를 호출할 때 시작하는 지연 기동은 유지한다.
- **headless 부팅 시 번들 플러그인을 설치한다.** 이전에는 알 수 없는 메서드 호출 경로에서 설치해 새 홈의 `plugin.list`가 그전까지 0개를 반환했다. 이제 부팅 직후 설치 목록을 조회할 수 있다. 설치만 수행하며 부팅 시 플러그인 프로세스는 시작하지 않는다.

- **플러그인을 거친 실패도 호스트의 오류 코드를 유지한다.** 이전에는 인자 오류 `-32602`까지 `-32000`으로 바꿔 재시도 판단에 필요한 분류를 잃었다. Claude·Codex에 없는 서피스를 지정해 26건을 요청한 측정에서 잘못 감싼 -32000이 11건에서 0건으로 줄었다. `host call '<call#N>' failed: <호스트 사유>` 문구는 유지한다. 같은 대상 오류가 플러그인 기동 여부에 따라 달라지던 문제도 해결했다. `ipc.result`에 `error_code`를 추가했으며 기존 SDK 플러그인과 양방향 호환한다. [ADR-0004](docs/adr/0004-ipc-discovery-and-errors.md).

- **등록된 메서드를 지원하지 않는 빌드와 이름 오타를 구분한다.** 이전에는 headless의 `window.creat`와 `window.create` 모두 `-32601`이었다. 이제 등록된 이름에 실행 경로가 없는 경우 `-32017 method '<name>' is registered but this binary has no dispatch arm for it: it is gated out of this build combination (headless / release)`를 반환한다. 미등록 이름은 -32601을 유지한다. 플랫폼 제한 -32015, 호출자 제한 -32016과 함께 원인을 구분할 수 있다. 당시 GUI의 등록 메서드 326개 호출에서는 -32017이 0건이었으며 이 수정의 대상은 headless였다. [ADR-0004](docs/adr/0004-ipc-discovery-and-errors.md).

- `tasty debug raw-key`와 `tasty debug switch-input-source` 도움말에 macOS GUI 전용임을 명시했다. 다른 플랫폼에도 명령 이름을 표시하되 호출하면 `-32015`와 지원하지 않는 이유를 즉시 반환한다.
- **설치된 마크다운 플러그인을 통해서도 `markdown.navigate`를 호출할 수 있다.** 플러그인이 점유한 네임스페이스로 전달된 뒤 처리 메서드가 없어 -32601이 되던 문제를 고쳤다. `image.open`·`image.list`처럼 플러그인이 호스트로 다시 전달한다. 이 수정 당시 오류는 `-32000 host call '…' failed: <host 의 사유>` 형식으로 한 번 감싸 반환했다.
- **창 안의 휠 스크롤 거리를 통일했다.** 플러그인 서피스·팝업·배너·attach mirror의 50pt와 호스트 ScrollArea·modifier hint의 40pt가 25% 차이나던 문제를 고쳤다. 호스트 UI는 40에서 50으로 빨라졌으며 이전 거리를 원하면 새 `general.wheel_line_scroll`을 40으로 설정할 수 있다. [ADR-0015](docs/adr/0015-terminal-user-input-routing.md).
- **팝업과 모달의 그림자를 구분하고 모달 셸에도 적용했다.** 기존에 그림자가 없던 호스트·플러그인 팝업 셸과 부팅 셸 설정 창이 어두운 배경에서 잘 구분되지 않던 문제를 보완했다. 트리거 옆에 표시하는 배너·툴팁·자동완성·modifier hint·튜토리얼 안내·도구 메뉴·검색 바·카테고리·배너 더보기와 화면을 가리는 모달은 서로 다른 그림자를 사용한다. 이동 가능한 창처럼 동작하는 알림 패널에는 그림자를 적용하지 않는다. popover 그림자도 디자인에 맞춰 번짐을 좁히고 가장자리를 조금 짙게 했다.
- 첫 실행 설정 카드의 그림자가 앱 배경색으로 번지던 문제를 고쳤다. 리팩터링 중 잘못 적용한 배경색 토큰 대신 공용 popover의 검정 반투명 그림자를 사용한다.
- **내장 언어 오버라이드의 빈 값을 무시한다.** `~/.tasty/lang/<code>.toml`에서 빈 문자열이나 공백만 지정하면 원래 내장 문구를 표시한다. 기존 언어팩과 같은 규칙이다. 의도적으로 빈 표시를 원하면 U+200B 또는 U+2060을 사용할 수 있다. NBSP(U+00A0)는 `str::trim`의 Unicode White_Space 처리로 제거되므로 대안이 아니다. [ADR-0040](docs/adr/0040-locale-catalogs-and-display-text.md).
- (BREAK) **세마포어 holder 응답에 점유 시각을 추가했다.** `agent.semaphore_*`의 `holders`가 문자열 배열에서 `{ id, acquired_at?, expires_at? }` 객체 배열로 바뀌었다. semaphore-list로 점유 시작과 만료를 조회할 수 있다. 시각 기록 도입 전의 holder에는 acquired_at을 생략하며 0으로 대신하지 않는다. 기존 저장 형식도 계속 읽는다.
- **부팅 중 터미널 엔진 생성 실패를 창에 표시한다.** 셸 경로 오류 등으로 실패하면 제목·본문·원인·해결 안내를 보여 주고 종료 버튼이나 Esc/Enter까지 유지한다. 이전에는 창이 사라지고 stderr·로그에만 남아 런처 사용자가 오류를 보기 어려웠다. GPU·창이 이미 생성된 경우에만 가능하며 GPU 드라이버 부재나 첫 창 생성 실패는 계속 로그를 남기고 종료한다. 진단은 `~/.tasty/debug.log`에도 기록한다. ADR-0016 갱신.
- (BREAK) **창 생성은 성공·실패가 결정된 뒤 응답한다.** `window.create`/`view.create`(CLI `tasty new window`)는 기존 `{"scheduled": true}` 대신 성공 시 `{"created": true, "window_id": <u64>}`, 실패 시 원인을 포함한 JSON-RPC 오류를 반환한다. 에이전트 요청 실패를 사용자 토스트로 알리지 않으며 사용자 메뉴·트레이 실패의 InfoModal은 유지한다. scheduled를 읽던 호출자는 created/window_id를 사용해야 한다. ADR-0007·ADR-0016 갱신.
- `surface.raw_key`는 macOS 손쉬운 사용 권한이 없으면 `-32001 permission_denied`로 거절한다. 이전에는 CGEventPost가 무시돼도 성공으로 응답했다. 권한은 부팅값을 저장해 쓰지 않고 호출할 때마다 확인한다.
- **macOS release에서 손쉬운 사용 권한을 요청하지 않는다.** 이 권한을 사용하는 OS 전역 키 주입 `surface.raw_key`가 debug 전용이 되어 release의 첫 실행 권한 요청은 화면 기록에서 끝난다. 설정 > 일반 > 권한에서도 손쉬운 사용 행을 제거하고 전체 디스크 접근·화면 기록만 표시한다. debug는 기존 세 행과 요청을 유지하며, 호출 시 미승인 상태면 `-32001 permission_denied`로 거절한다.
- Claude 플러그인에 `memory.read` 권한과 stop-gate의 `--surface` 인자를 추가했다. `memory.goal_*`로 세션 목표를 읽어 계속 작업할지 판단한다. 권한 추가이므로 업그레이드 후 재승인이 한 번 필요하다.
- CLI stderr, 플러그인 보고서, 호스트 알림 제목, OS 메뉴·트레이·Jump List 문구에 `general.language`를 적용했다. 기존 영어 문자열을 파싱하던 스크립트는 로케일 차이를 처리해야 한다. 종료 코드와 stdout JSON은 유지한다.
- `~/.tasty/lang/<code>.toml`은 내장 en/ko/ja의 오버라이드로만 사용한다. 다른 코드의 단일 파일은 경고 후 무시하므로 `<code>/pack.toml` 형식으로 옮겨야 한다.
- **선택한 언어팩이 없거나 잘못되면 영어로 대체하고 알린다.** GUI는 요청 코드·기대 경로를 경고 토스트로 한 번 표시하고 headless·CLI는 tracing 경고를 남긴다. 설정값은 유지하되 `current_language()`와 플러그인의 `TASTY_LOCALE`에는 실제 적용값 en을 전달한다. `tasty_i18n::init`은 새 `LoadReport`를 반환한다.
- 삭제된 언어팩 등 현재 코드가 목록에 없으면 설정 콤보에 `<code> (not found)`를 남긴다. 콤보를 열고 닫는 것만으로 설정값을 바꾸지 않는다.
- **스크린샷에서 모달·프리셋 창을 명시할 수 있다.** `ui.screenshot`(CLI `tasty screenshot`)의 `--window <id>`가 설정·플러그인·종료 확인 창 등을 허용한다. 생략 시에는 계속 main 창이 정확히 하나일 때만 자동 선택하며 포커스에 의존하지 않는다. `window.list`와 `window.close`의 대상은 main 창으로 유지한다. main 창이 없을 때는 Multiple windows open 대신 No main window open으로 구분한다. 모달 ID는 OS 창 목록에서 얻으며 X11에서는 winit ID와 X11 window ID가 같다.
- **사용자 번역 파일에 2 MiB 크기 상한을 추가했다.** 언어팩 `~/.tasty/lang/<code>/pack.toml`과 내장 오버라이드 `<builtin>.toml` 모두 넘으면 파싱 전 거절하고 경고 후 언어 목록에서 제외한다. 선택된 코드라도 영어로 대체하며 설정값은 유지한다. 도입 당시 가장 큰 내장 ja.toml은 89 KiB·약 1,300키로 상한은 약 23배였다. 이전에는 200 MB 파일도 제한 없이 읽고 파싱했다.
- **플러그인 전용 메서드를 외부 호출 제한으로 구분한다.** `banner.open`, `banner.close`, `popup.close`, `host.shared_buffer.create`를 CLI·네트워크 IPC에서 호출하면 기존 -32601 대신 -32016과 사유를 반환한다. 구현은 있지만 호출자 종류가 제한된 경우이며 플러그인의 정상 호출과 미등록 이름의 -32601은 유지한다. 메서드 표에 local_only와 구분되는 plugin_only를 추가했다. 당시 GUI의 플러그인 설치·미설치, headless, release 조합에서 확인했다. 근거 ADR-0004.
- **Codex의 완료 알림·샌드박스 안내·재시작 안내를 번역한다.** 기존 한국어 고정 문구를 `codex.notify.done_message`, `codex.notify.sandbox_hint`, `codex.reboot.notice`와 `crates/tasty-plugin-codex/lang/{en,ko,ja}.toml`로 옮겼다. 완료 로그의 문장을 직접 비교하는 소비자는 서피스 번호 등 언어와 무관한 정보를 사용해야 한다. reboot 안내는 화면 판독에 필요한 `tasty codex reboot` 접두어를 모든 언어에서 유지한다.
- **Codex IPC 오류 24문장을 설정 언어로 표시한다.** 인자·훅·설치·재시작 오류를 `codex.params.*`, `codex.hook.*`, `codex.launch.*`, `codex.spawn.*`, `codex.respawn.*`, `codex.install.*`, `codex.reboot.*` 번역 키로 옮겼다. 스크립트는 로케일별 문장 대신 `-32602` 같은 JSON-RPC code로 구분해야 한다. 종료 코드와 stdout JSON은 유지한다.
- `tasty codex reboot` 오류의 `host call '…' failed:` 접두어가 두 번 붙던 문제를 고쳤다. 플러그인이 이미 감싼 호스트 오류를 다시 감싸지 않는다.

### Removed

- (BREAK) **OS 전역 키·입력 소스 조작을 debug 전용으로 제한했다.** `surface.raw_key`, `surface.switch_input_source`는 `local_only()`이며 release에서 `method_not_found`를 반환한다. macOS의 CGEventPost·TISSelectInputSource는 대상 서피스 ID 없이 OS 포커스를 조작해 다른 앱에도 영향을 줄 수 있었다. 사용자 입력 재현을 release API로 제공하지 않는 원칙과 보안 예외에 따라 폐기 유예 없이 제거했다. 기존 `tasty debug raw-key`·`switch-input-source`는 그대로다. 특정 서피스에 키를 보내려면 ID를 지정하는 `surface.send_key`로 해당 PTY에 기록한다. 근거 ADR-0012 및 API 안정성 정책.
- (BREAK) **IME 시뮬레이션도 debug 전용으로 제한했다.** `surface.ime_enable`, `surface.ime_disable`, `surface.ime_preedit`, `surface.ime_commit`, `surface.ime_status`는 포커스된 창의 입력 상태를 조작하고 대상을 ID로 지정하지 못해 release에서 제외했다. 원래 local-only이고 CLI도 `tasty debug ime-*`였으므로 영향은 release 로컬 IPC 직접 호출에 한정된다. debug의 한글·CJK 검증 절차는 유지한다(`docs/ai-verification/ime-testing.md`).
- (BREAK) **동기 네이티브 파일 선택 IPC `fs.pick_file`을 제거했다.** 호출하면 method_not_found를 반환한다. 포털 없는 Linux에서 메인 스레드의 다이얼로그 호출이 끝나지 않아 system.info·surface.list·window.list도 멈췄다. 평시 0.01–0.28초이던 요청이 15/30/45/60/90초에도 회복되지 않았고 120초 대기에서도 응답하지 않았다. 사용자·에이전트·원격 attach를 함께 멈추는 문제라 폐기 유예 없이 제거했다. 저장소 내 호출자는 이미 즉시 접수 후 event.dispatch로 알리는 `file_picker.trigger`를 사용했다(ADR-0036). 설정 scripts·remote-transfer, 프리셋 필드, 플러그인 추가에서 사용자가 여는 네이티브 다이얼로그는 유지한다. 근거 ADR-0031 및 API 안정성 정책의 예외.

### Fixed

- **Claude·Codex의 서피스 인자 오류에 실제 키 이름을 표시한다.** surface와 surface_id를 같은 필드로 처리하면서 오류를 모호하게 표시하거나 이미 보낸 값을 다시 요구하던 문제를 고쳤다. 이제 `'surface_id' = "x"`처럼 잘못된 키를 명시하고 두 키가 다른 대상을 가리키면 거절한다. 일반 Codex 인자 검사는 원래 키를 표시했지만 codex.hook은 오류를 `hook requires --surface to identify the child`로 덮어썼다. 기존 `codex.params.not_a_number`·`surface_conflict` 번역도 훅에서 사용한다.

  값이 하나도 없을 때만 surface를 요구한다. claude.hook은 이때만 `TASTY_SURFACE_ID`로 대체하며 잘못된 값이 있으면 대체하지 않는다. codex.hook에는 원래 환경값 대체가 없고 CLI가 surface를 채운다. 플러그인별 문구·placeholder 형식의 차이는 유지한다.
- **이미지 탐색 목록을 실제 디코더 지원과 맞췄다.** .gif 디코더를 활성화하고 래스터 디코더로 열 수 없는 .svg는 목록에서 제외했다. 이전에는 next/prev가 해당 파일로 이동하면 로그 없이 빈 화면을 표시했다. 이제 디코드 실패를 기록하며 지원 목록은 빌드에 포함한 디코더에서 구한다. SVG를 열 수 있게 되면 목록에도 다시 포함한다.
- **웹훅 본문에 요청당 크기 제한을 추가했다.** 이전에는 경로·인증보다 먼저 본문을 제한 없이 읽어 등록된 웹훅이 없어도 메모리를 소모했다. 300 MB 요청의 RSS는 27 MB에서 352 MB로, 길이 없는 chunked 전송은 20초에 10.2 GB까지 증가했다. 이제 기본 1 MiB이며 `TASTY_WEBHOOK_MAX_BODY_BYTES`로 조정한다. 초과하면 `413 payload too large`를 반환한다.

  선언된 Content-Length가 크면 본문을 읽지 않고 거절한다. 길이 없는 chunked는 상한 + 1바이트에서 멈춘다. 같은 측정에서 RSS는 20초 동안 28 MB 수준을 유지했다. 반복 413은 남용 차단에 포함한다. 더 큰 정상 payload가 필요하면 환경변수를 조정해야 하며 고정 오류 응답에는 상한을 싣지 않는다. 동시 요청 수는 제한하지 않아 전체 본문 메모리는 요청당 상한과 동시 요청 수에 따라 늘 수 있다.

- **웹훅 인증 실패 반복도 출처별 남용 차단에 포함한다.** 이전에는 404·405만 세어 잘못된 토큰으로 65회 요청해도 모두 401을 반환하고 차단하지 않았다. 이제 기본 10초 20회 임계치를 넘으면 해당 출처가 기본 60초 동안 429를 받는다. 인증 실패는 등록별 호출 횟수에서는 차감하지 않고 출처별 실패 수에만 반영한다. 만료 응답 410은 계속 제외한다. 잘못 설정한 정상 발신자도 적용되며 출처가 IP이므로 NAT를 공유하는 다른 발신자까지 차단될 수 있다. 집계 대상 응답 상태를 한 함수에서 빠짐없이 분류하도록 했다.

- **틀린 토큰이 웹훅의 호출 횟수를 소모하지 않게 했다.** 이전에는 인증 전에 `--count N`을 차감해 count 2 등록이 잘못된 토큰 두 번으로 삭제되고 다음 요청은 404가 됐다. 이제 레지스트리 잠금 안에서 인증을 먼저 확인해 401은 차감하지 않는다. 405의 기존 규칙과 일치하며 인증 실패의 출처별 남용 차단은 별도로 적용한다. count 도움말, 사용자 가이드, 기능 문서의 처리 순서·검증 기준도 인증 후 차감으로 맞췄다.
- **CLI의 빈 설명과 잘못 붙은 설명을 고쳤다.** `tasty list queue`, `tasty set global-hook`, `tasty telemetry record`에 설명을 넣고 이웃 명령인 list theme·record-batch에 섞인 문장을 제거했다. 옵션 설명 누락도 당시 204곳에서 0곳으로 줄였다(memory 86, agent 49, telemetry 19, tool 17, debug 13, preset 12, approval 5, plugin 2, output 1). 같은 이름의 옵션이라도 명령별 뜻을 설명하며 이후 누락을 검사한다.

- **번들 업그레이드 도움말을 실제 비교 방식과 맞췄다.** `tasty plugin upgrade-builtins`는 번들이 높으면 전체를, 버전이 같으면 내용이 다른 파일만 복사하며 설치본이 높으면 건너뛴다. force는 높은 설치본을 낮추는 것도 허용한다. 새로 선언한 권한을 같은 호출에서 부여한다는 점도 명시했다. 동작은 바꾸지 않고 ‘높은 버전만 설치’라는 오래된 설명을 고쳤다.

- `tasty set cwd` 도움말에 남아 있던 global-hook 설명을 제거했다. 연속된 rustdoc 문장을 clap이 하나로 합쳐 두 명령의 설명이 함께 나오던 문제다. 설명 존재만 확인하는 기존 검사로는 구분하지 못했다. global-hook 자신의 설명은 유지한다.

- **`image.list`가 모든 창의 이미지를 반환한다.** 기존 포커스 창뿐 아니라 main·parked 창의 항목을 합친다. 플러그인 네임스페이스로 전달된 뒤 호스트로 돌아오는 요청에도 같은 합산을 적용한다. surface_id가 창 사이에서 유일하므로 합친 목록에서도 같은 ID로 대상을 지정한다.

- **CLI에서 제거한 번들 플러그인도 재설치되지 않게 했다.** GUI Uninstall에만 있던 `plugins.toml`의 `removed_builtins` 기록을 CLI·IPC 제거에도 적용했다. 이전에는 다음 실행에서 비활성 상태로 다시 설치됐다. `tasty plugin upgrade-builtins --restore-removed <id>`로 복구하면 비활성 기록도 정리해 켜진 상태로 설치한다.

- **플러그인 제거 후 네임스페이스 소유권을 다시 계산한다.** 설치 목록만 지우고 소유권을 남겨 -32002를 반환하거나 image.list·image.open·markdown.navigate의 호스트 구현에 접근하지 못하던 문제를 고쳤다. 당시 이미지 플러그인 제거 후 plugin.list는 8개인데 image.list는 실행 중이 아니라는 오류를 반환했다. 이제 설치 때와 같은 재발견 절차를 거쳐 호스트 구현이 바로 응답한다.

- **다중 인스턴스의 자식 프롬프트 임시 파일을 분리했다.** Claude·Codex가 `{plugin prefix}{surface_id}.txt`를 공유해 같은 서피스 번호의 프롬프트를 덮어쓰거나 셸 치환 중 빈 값으로 만들던 문제를 고쳤다. 파일 이름에 플러그인 프로세스 PID를 추가한다. 기존 청소 패턴이 이전 이름도 찾도록 prefix 뒤에 PID를 배치했다.

- **split 요청을 명시한 대상의 창으로 전달한다.** `tasty split --target-surface <ID>`의 target_surface·target_pane을 인식하지 않아 포커스 창에서 실행하던 문제를 고쳤다. 없는 대상은 -32602와 `no live surface N (named by 'split')`로 거절한다. CLI의 문자열 ID와 nickname 지정도 같은 경로로 처리한다.

- **Claude·Codex의 kill·respawn이 surface_id를 누락하지 않게 했다.** 기존에는 surface 키만 전달해 surface_id를 지정한 raw IPC가 단일 부모 대체 경로로 넘어가 다른 자식을 조작할 수 있었다. codex.kill이 Claude 자식을 종료하는 경우도 있었다. CLI는 두 키를 모두 보내 영향을 받지 않았다. 이제 tasty-plugin-agent-common에서 두 이름을 같은 필드로 읽고 다르면 -32602로 거절한다. 두 키를 모두 생략한 경우의 기존 대체 규칙은 유지한다.
- codex.parent/state/tell/spawn/respawn/children이 surface_id 인자를 받도록 고쳤다. 이전에는 surface만 읽어 agent-runner 가이드의 `params.surface_id` 예제로 호출할 수 없었다.
- Claude 인자 판독을 `Missing required 'surface' (or 'surface_id') parameter` 안내와 맞췄다. 안내와 달리 surface 키를 읽지 않던 문제를 고쳤다.

- **호스트 명령과 중복되는 플러그인 CLI 이름을 등록 전에 거절한다.** 기존에는 contributes.cli의 terminal 같은 이름이 clap에 중복 등록돼 debug의 도움말과 다른 플러그인 명령도 panic했다. 이제 clap 트리에서 구한 정적 명령 이름·alias와 비교하고 중복은 경고 후 건너뛴다. release에서도 이미 호출할 수 없던 중복 이름만 제외하므로 기존 서드파티 명령의 기능은 유지한다.
- **headless에서도 theme.query가 동작한다.** 전역 Theme·설정만 필요한 조회가 GUI 전용 웹뷰 모듈에 있어 -32601이 되던 문제를 고쳤다. 두 빌드가 같은 함수로 응답한다.
- **호스트 IPC가 u32 범위를 넘는 ID를 거절한다.** terminal·pty·preset의 공통 형태 헬퍼 세 곳, terminal.tell을 포함한 20개 호출에서 값이 잘려 다른 서피스를 가리키던 문제를 고쳤다. 당시 surface.locate에 실제 ID + 2^32를 주면 원래 대상이 존재한다고 응답했다. 이제 invalid_params로 거절하며 누락과 잘못된 값을 구분한다. 선택 인자도 잘못된 값을 버리고 기본 대상이나 단일 부모로 대체하지 않는다.
- **Agent Stream이 잘못된 ID와 없는 서피스를 구분한다.** agent_stream.turn_start·unwatch는 u32 범위 초과를 missing_surface로, 범위 안의 없는 ID를 watch하지 않는 서피스로 잘못 안내했다. 이제 값 오류를 명시하고 없는 대상은 `no live surface <id> (named by '<호출한 메서드>')`로 반환한다. 괄호에는 내부 확인 호출이 아닌 실제 호출 메서드를 적는다. 기존 소비자가 파싱하는 이 프로토콜 문구는 번역하지 않는다. 호스트의 거절을 대상 생존으로 잘못 처리해 사라진 세션의 turn 정리를 건너뛰던 문제도 고쳤다.
- Codex의 surface/surface_id/caller_surface/child/caller/target이 u32 범위를 넘으면 invalid_params로 거절한다. 이전에는 4294967297이 1, 5000000000이 705032704로 잘려 다른 서피스를 조작하고 성공할 수 있었다. u32::MAX까지는 유효하다. 키가 없을 때만 missing으로 안내하고 값이 잘못되면 해당 값을 표시한다.
- Claude의 surface_id·child_index에도 같은 범위 검사를 적용했다. 앞서 수정한 claude.hook 외에 profile·reboot·spawn·kill·respawn의 헬퍼 두 곳에서 잘리던 값을 거절하고 누락과 값 오류를 구분한다.
- **macOS 전용 debug 명령의 다른 플랫폼 오류를 구분한다.** tasty debug raw-key·switch-input-source는 이름을 도움말과 표에 유지하되 CGEventPost·TISSelectInputSource를 지원하지 않는 조합에서 -32015와 사유를 반환한다. 기존 -32601은 이름 오타로 오해할 수 있었다. 근거 ADR-0004.
- **트리 합산 조회 전에 권한·사용량 제한을 검사한다.** GUI의 surface.list·workspace.list·pane.list가 surface.read 검사 전에 조기 응답해 권한 없는 플러그인도 전경 PID·프로세스를 읽고 cap·rate limit·감사를 건너뛰던 문제를 고쳤다. Pause 상태의 플러그인도 통과하던 요청을 포함해 세 검사를 라우팅 전에 수행한다. headless에는 이 문제가 없었다. [ADR-0012](docs/adr/0012-request-admission-and-isolation.md).
- 플러그인 진입부에서 직접 처리하는 popup.close·banner.open·banner.close에도 권한뿐 아니라 cap·rate limit·감사를 먼저 적용한다. 이전에는 거절 기록과 사용량 제한이 누락됐다. 당시 더 안쪽의 `ipc.invoke:<prefix>` 검사에서 거절한 네임스페이스 전달은 여전히 감사에 기록하지 않았다.
- **host.shared_buffer.create를 메서드 표에 등록했다.** 이전에는 권한·cap·rate·감사 경로가 이름을 찾지 못해 권한 없는 플러그인이 호출당 1 GiB 공유 버퍼를 개수 제한 없이 요청할 수 있었다. 회수는 사용하는 UI 종료 시 이루어졌다. 이제 cap·rate limit을 적용하고 감사에 남긴다. 요구 권한 자체는 매니페스트 호환성을 고려한 별도 결정으로 남겨 아직 추가하지 않았다. 현재 범위는 [플러그인 권한](docs/dev-guide/plugin-permissions.md)의 ‘토큰 없이도 열려 있는 메서드’ 표에 설명한다. host 네임스페이스도 플러그인이 점유하지 못하도록 예약했다.
- **raw 메모리 API로 감사 로그에 접근하지 못하게 했다.** `plugin.audit_*`는 플러그인에 닫혀 있었지만 `memory.read`/`memory.write`로 `tasty.audit.` 기록을 읽거나 위조할 수 있었다. 이제 `tasty.` 접두어를 호스트 예약으로 처리해 권한이 제한된 플러그인·에이전트의 raw `memory.*`에서 직접 지정·열거·계수를 막는다. Local CLI의 `tasty memory list --prefix tasty.audit.`은 유지한다. raw KV를 거치지 않는 전용 `memory.bb_*`/`plan_*`/`cache_*`도 그대로다. [ADR-0012](docs/adr/0012-request-admission-and-isolation.md).

- claude.hook의 surface/surface_id가 u32 상한을 넘으면 invalid_params로 거절한다. 이전에는 5000000000이 705032704로 잘려 다른 서피스를 조작할 수 있었다. 음수 거절과 u32::MAX까지의 정상 입력은 유지한다.
- **플러그인 기동 직후 첫 호출의 권한을 등록 후 확인한다.** hello와 첫 호출이 같은 pump 배치에 들어오면 아직 등록되지 않은 상태의 빈 권한을 붙여 permission_denied가 발생하던 문제를 고쳤다. headless의 markdown.recent가 com.tasty.markdown의 surface.read 누락으로 거절된 사례가 있었다. 이제 배치가 같거나 달라도 등록 후 권한을 적용한다. 모르는 플러그인의 호출에는 계속 빈 권한을 사용한다.

- **poison된 락 때문에 처리를 계속 건너뛰던 문제를 고쳤다.** 다른 스레드가 락을 잡은 채 panic한 뒤 오류를 무시하면 상태가 계속 남았다. 그 결과 종료된 자식의 생존 오판, 세션 재시작 중단, SSE 연결 종료, 플러그인 호출 timeout·기동 실패, 드래그 취소 오판, state.db 부재 오판, 이상 감지 누락이 발생할 수 있었다. 자료구조 조작만 하여 남은 상태를 사용할 수 있는 곳은 복구하고 처음 한 번 경고한다. 쓰기 중간 상태 등이 남을 수 있는 락은 복구하지 않는다. 기준은 [오류 처리](docs/dev-guide/error-handling.md)의 ‘락 poison’ 절을 따른다.
- **poison 이후에도 플러그인 네임스페이스 등록·해제를 처리한다.** 등록 실패 시 실행 중인 플러그인의 메서드가 UnknownMethod가 되고, 해제 실패 시 죽은 플러그인의 prefix가 남아 `plugin_callable=true`·필요 권한 없음으로 통과하던 문제를 고쳤다. 맵만 조작하는 다섯 지점에 공통 복구를 적용하고 경고는 한 번 남긴다.
- `tasty read since-mark --strip-ansi`가 private mode 시퀀스를 남기던 문제를 고쳤다. IPC surface.read_since_mark의 정규식에 ?와 더 넓은 CSI 종결 바이트를 반영해 `\x1b[?25l`, `\x1b[?1049h`, `\x1b[?2004h` 등을 제거한다. 같은 처리를 하던 tasty-output과 범위를 맞췄다.
- **ANSI 제거의 CSI 파라미터 범위를 확장했다.** `tasty read since-mark --strip-ansi`와 tasty-output이 ECMA-48의 0x30–0x3F(`0-9 : ; < = > ?`) 전체를 처리한다. private mode `\x1b[?25l`·`\x1b[?1049h`·`\x1b[?2004h`, 서브파라미터 `\x1b[4:3m`·`\x1b[38:2::R:G:Bm`, private prefix `\x1b[>4;1m`·`\x1b[<0;12;3M`·`\x1b[=1c` 등이 해당한다. 종결 바이트의 알파벳 제한도 넓혔다. 이전에는 닫는 `\x1b[0m`만 지워져 남은 escape를 놓치기 쉬웠다. 두 정규식의 일치 여부는 테스트로 확인한다.
- **플러그인 CLI의 잘못된 숫자 인자를 거절한다.** type이 u32/i64인 값을 읽지 못하면 버리고 기본값을 쓰던 문제를 고쳤다. 기존에는 잘못된 surface가 호출자 자신의 TASTY_SURFACE_ID로 바뀌어 자기 터미널에 보내고도 성공할 수 있었다. 당시 여섯 플러그인의 숫자 플래그 56개가 같은 경로였다. 생략한 인자의 기본값은 유지한다.

  claude.hook도 surface가 잘못됐을 때 surface_id나 환경값으로 넘어가지 않는다. null이 아닌 첫 키를 선택하고 숫자가 아니면 invalid_params로 거절한다. null은 계속 건너뛴다. 서로 다른 타입의 두 키를 보내던 외부 호출자는 거절될 수 있으며 저장소 송신자는 같은 값으로 보낸다. claude hook·checklist-hook·codex hook의 stdin JSON에도 숫자 형식 검사를 적용한다. CLI 플래그를 명시했다면 그 값이 우선이다.
- **워크스페이스 재정렬 뒤에도 카테고리의 마지막 사용 대상을 유지한다.** Ctrl+Shift+번호 전환 위치를 전역 인덱스 대신 워크스페이스 ID로 기억한다. 같은 카테고리의 다른 워크스페이스로 이동하던 문제와 headless에서 재정렬 시 활성 대상 보정이 빠진 문제를 고쳤다. 근거 ADR-0017.
- **마지막 터미널 종료로 사라진 워크스페이스도 workspace.closed를 알린다.** GUI·IPC 닫기와 달리 자연 종료에서 이벤트가 빠지던 문제를 고쳤다. 워크스페이스 제거 세 경로가 같은 알림 처리를 사용한다.
- **hook-failures 로그에 JSON-RPC code 필드를 추가했다.** `~/.tasty/hook-failures.log` 형식은 `<UTC> method=… event=… surface=… code=… reason=…`이며 호스트에 도달하기 전 실패는 code=-다. reason은 마지막 필드로 유지하므로 reason= 뒤를 읽는 방식은 같고 열 번호로 읽는 경우는 5번째에서 6번째로 바뀐다. 구조화된 필드는 로케일에 영향을 받지 않으며 reason은 문구를 만든 쪽의 언어를 사용한다. Claude·Codex 훅의 오류 응답은 앱 언어로 기록할 수 있다. 근거 ADR-0043.
- **hook-failures의 CLI 자체 오류 두 종류는 영어로 기록한다.** Tasty 미실행·연결 실패의 reason을 영어로 고정했다. 호스트에 도달한 뒤 받은 오류는 응답한 플러그인의 언어를 유지한다. method/event/surface와 `Error (-32602)` 같은 코드로 로케일과 무관하게 구분할 수 있다. 사용자 stderr는 계속 번역하며 key=value 한 줄 형식도 유지한다. 훅 래퍼가 종료 코드를 버려 이 로그가 전달 실패의 근거가 되는 경우를 고려한 변경이다.
- **poison된 승인 저장소의 변경 요청은 오류로 거절한다.** approval.request/respond/cancel이 메인 스레드까지 panic시켜 모든 창의 터미널을 종료하던 문제를 고쳤다. 기록과 대기자 목록이 어긋난 중간 상태일 수 있어 변경은 `-32014`(store_poisoned)로 거절한다. approval.list/history 같은 읽기는 복구해 계속 제공한다.
- **읽지 못하거나 잘못된 설정 파일을 기본값으로 덮어쓰지 않게 했다.** config.toml과 layouts/NN.json의 없음·읽기 실패·해석 실패를 구분한다. 파싱 실패 파일을 덮어쓰기 전에는 같은 폴더의 config.toml.bak·NN.json.bak으로 옮기며 기존 백업이 있으면 .bak.2부터 .bak.9까지 사용한다. 읽기 실패나 지원하지 않는 미래 version은 내용을 알 수 없어 이동하지 않고 저장도 거절한다.

  백업 직전에 다시 확인해 부팅 뒤 정상화된 파일을 옮길 위험을 줄였다. 확인과 이동 사이의 두 syscall 간 경쟁까지 없앤 것은 아니다. 부팅 경고 토스트에 상황과 필요한 조치를 표시한다.
- **내장 번역 오버라이드도 빈 값을 번역 없음으로 처리한다.** `~/.tasty/lang/<code>.toml`의 `key = ""`가 빈 라벨을 만들던 문제를 고쳤다. 언어팩은 영어 기본값, 오버라이드는 해당 내장 언어의 원문을 표시한다. 무시한 빈 값 수를 tracing 경고로 알린다. 빈 표시가 필요하면 U+200B를 사용할 수 있으며 NBSP는 trim에서 제거된다.
- **화면 조회에서 하단 빈 행 대신 마지막 내용 N줄을 반환한다.** `tasty read screen --lines N`이 빈 화면의 마지막 행만 잘라 프롬프트·공백 위주로 반환하던 문제를 고쳤다. 하단 빈 행을 건너뛰고 부족하면 대체 화면에서도 primary 스크롤백으로 채운다. 같은 N에 이전과 다른 줄이 나올 수 있으며 존재하는 내용보다 많이 반환하지는 않는다.
- **웹뷰에 포커스가 있어도 사용자 단축키를 처리한다.** rendering=webview인 마크다운·HTML의 OS 자식 창이 키를 받아 winit 창에 전달하지 않던 문제를 고쳤다. 세 백엔드가 native 키를 호스트에 전달하고 KeybindingSettings·플러그인 명령 등록부로 호스트가 처리할 조합을 정한다. 수식 없는 키, Shift만 있는 키, 페이지의 find/copy/cut/paste/select_all은 유지한다. 러시아어 등 다른 레이아웃은 물리 키로도 매칭한다. 분할 탭의 비포커스 웹뷰 set_url 실패와 포커스 웹뷰가 이웃 영역을 덮던 문제도 고쳤다. 근거 ADR-0029.
- 창·GPU 초기화 실패를 보고하고 기존 창의 세션은 유지한다. 셸 경로 오류 등으로 새 창을 만들지 못했을 때 프로세스 전체가 panic하던 문제를 고쳤다.
- 팝업·배너 렌더러가 휠 델타 단위를 무시하고 한 칸을 1포인트로 처리하던 문제를 고쳤다.
- **포커스 복귀 시 OS가 합성한 키 이벤트를 무시한다.** X11·Windows에서 winit이 보내는 합성 Pressed/Released를 is_synthetic으로 구분한다. 다른 앱을 Alt+F4로 닫은 뒤 남은 F4가 탭 이름 변경 팝업을 열던 문제를 고쳤다.
- 일반 서피스 ID가 2^31을 넘어 headless PTY의 ID 범위와 겹치던 문제를 고쳤다. 새 충돌을 막고 부팅 때 기존 충돌 범위를 정리한다.
- **홈 경로를 찾지 못하면 셸 통합을 명시적으로 생략한다.** HOME 없는 데몬·컨테이너에서 빈 경로로 대체해 rcfile·ZDOTDIR가 상대 경로가 되고 OSC 7/133이 동작하지 않던 문제를 고쳤다. 이제 경고 후 일반 로그인 셸로 실행하며 설정 UI의 bashrc 저장도 거절하고 오류를 남긴다. 상대 TASTY_HOME은 거절하지 않고 절대 경로로 바꿔 전달한다.
- **작업 저장소 조회 실패를 작업 수 0과 구분한다.** 러너 응답의 ready_count/running_count를 null로 반환하고 store_error에 원인을 표시한다. 러너가 실행 중일 때의 연속 실패는 list_failures로 제공한다. task-list/task-run 텍스트도 ready=?와 사유를 표시한다. task_list/task_graph/task_run의 runner를 읽는 소비자는 두 카운트의 null을 처리해야 한다.
- **작업 목록 조회 실패로 러너가 멈춘 경우 로그를 남긴다.** 실패를 빈 목록으로 처리해 dispatch·poll·permit 회수가 조용히 중단되던 문제를 고쳤다. 첫 실패는 경고, 연속 실패는 주기적 오류로 기록하며 복구도 알린다.
- tracing 진단을 stdout 대신 stderr로 출력한다. `tasty list tree | jq .` 같은 JSON 처리에 경고가 섞이던 문제를 고쳤다. 파일 로그는 유지한다.
- **닫기·이동 후에도 사용자가 보던 대상을 유지한다.** 워크스페이스·탭의 앞쪽 항목을 지우면 인덱스가 다른 대상을 가리키고, 페인은 무조건 첫 항목으로 포커스를 옮기던 문제를 고쳤다. 이제 삭제 위치와 기존 선택을 함께 보정하고 보던 대상 자체가 없어졌을 때만 이동한다. 에이전트 close뿐 아니라 사용자 메뉴, surface.move, 원격 attach 전달에도 적용한다. mirror 정리는 기존에도 보정했으며 공용 함수로 합치면서 카테고리 전환 위치도 보정했다. 근거 ADR-0017.
- **headless에서도 surface.completion을 적용한다.** GUI 전용 Intent 처리에만 의존해 아무 효과가 없던 요청을 IPC 핸들러에서 대상 엔진에 적용한다. 응답 형식은 유지하며 새 surface.attention.get/clear도 headless에서 동작한다.
- **headless가 상태 변경 Intent를 응답 전에 처리한다.** surface.set_mark, notification.create, settings.set_remote_transfer가 ok를 반환하고도 상태를 바꾸지 않던 문제를 고쳤다. --headless와 --no-default-features 모두 적용하며 큐가 요청 수에 비례해 계속 늘지 않게 한다. 근거 ADR-0003.
- **플러그인에 실제 설정 언어를 전달한다.** 호스트가 부팅 때 general.language로 TASTY_LOCALE을 설정해 모든 플러그인이 상속한다. 이전에는 셸에서 직접 설정한 경우 외에는 플러그인 UI가 영어였다. 셸의 기존 환경값보다 앱 설정을 우선하며 프로세스 생성 시 고정되므로 언어 변경은 재시작 후 적용한다.
- **CLI stdout의 닫힌 파이프를 정상 종료로 처리한다.** `tasty list tree | head -1`, `| true` 등의 EPIPE에서 panic·종료 코드 101·가짜 crash report를 만들던 문제를 고쳤다. 세 OS 모두 종료 코드 0을 반환하며 다른 stdout 오류는 계속 오류로 처리한다. 같은 상황의 루트 --help도 기존 Broken pipe·코드 1 대신 이 규칙을 적용한다. 근거 ADR-0043.

## [0.10.2] - 2026-08-29

### Added

- `terminal.state`(CLI `tasty terminal state --surface <child>`)로 자식 하나의 idle/needs_input/active/exited 상태를 조회한다. 등록부에서 제거된 서피스도 실제 트리와 대조해 exited로 구분한다.
- `claude.state`·`codex.state`와 CLI `tasty claude state`·`tasty codex state`를 terminal.state에 연결했다. claude.spawn·codex.spawn에는 `[[contributes.completion_strategy]] default_for_methods`를 지정해 DAG에서 poll을 생략해도 접수만으로 완료 처리하지 않는다. 이 두 메서드는 자식이 실제 idle/exited가 될 때까지 running을 유지한다.
- **DAG 단위 조회 API를 추가했다.** `agent.dag_list`·`agent.dag_get`(CLI `tasty agent dag-list`·`dag-get`)은 기존 작업 목록에서 그룹을 계산한다. `task.metadata.dag`가 문자열이면 이를 명시적 그룹 키로 사용하고 ID는 `d:<값>`이다. 그 외에는 depends_on, Fallback.task, Reduce.inputs, metadata.fallback_of 연결을 방향 없이 따라가 같은 그룹으로 묶으며 ID는 `c:<root task id>`다. 같은 작업 집합은 같은 ID를 갖는다.

  dag_list에서 workspace_id를 생략하면 살아 있는 모든 워크스페이스를 조회하고 scope=live_workspaces로 반환하며 삭제된 워크스페이스의 고아 작업은 제외한다. 각 항목은 name/source/task_count/state_counts/rollup_state/created_at/updated_at/root_task_ids/has_cycle을 포함하며 include_tasks:true이면 task_ids도 추가한다. dag_get은 해당 그룹만으로 task_graph와 같은 nodes/edges 또는 format:dot을 반환한다. 둘 다 AgentManage 권한을 요구한다.
- agent.task_run의 러너 시작·정지·조회를 플러그인에도 AgentManage 권한으로 허용했다. 호스트 재시작 뒤 러너가 자동 시작하지 않으므로 플러그인이 직접 재개할 수 있게 한다. 당시 task_set_result와 task_await는 계속 로컬 전용이었다.
- agent.task_list/task_graph 응답에 `runner: { running, crashed, ready_count, running_count }`를 추가했다. 러너가 꺼져 있어도 작업 수는 저장소를 조회해 반환하므로 실행할 작업이 남아 있는지 알 수 있다.
- agent.task_get에 조건부 `awaiting_external: { wait_key, deadline_ms }`를 추가했다. AwaitExternal 완료 전략으로 외부 신호를 기다리는 작업을 일반 running 상태와 구분한다.
- agent.task_create에서 on_failure=Fallback과 비어 있지 않은 depends_on을 함께 지정하면 warnings를 반환한다. Fallback은 작업 자체의 Running→Failed에 적용되며 의존성 실패로 Waiting→Skipped가 되는 경우에는 적용되지 않는다. 의존 작업에 Fallback을 설정해야 하는 차이를 알리되 생성을 거절하지는 않는다.
- task-list/task-get/task-run CLI 출력을 JSON 대신 읽기 쉬운 텍스트로 표시한다. `state  id  name` 목록과 `runner: running (ready=N running=M)` 요약, 정지 시 재개 명령을 포함한다. barrier·semaphore·lease·rate-limit·task-graph 등 다른 agent 명령의 JSON은 유지한다.
- **작업 삭제 API를 추가했다.** agent.task_delete(CLI tasty agent task-delete)는 depends_on/Fallback.task/Reduce.inputs로 참조되면 -32010과 error.data.referenced_by로 거절한다. `--cascade`는 연결된 참조자를 따라 함께 삭제하고 `--force`는 참조 검사만 생략한다. running 작업은 두 옵션으로도 삭제하지 못하며 -32011을 반환한다.
- agent.task_purge(CLI tasty agent task-purge)는 `--states`·`--older-than-ms`로 선택한 작업을 일괄 삭제한다. task_delete와 같은 참조 검사를 적용해 후보 밖에서 참조하는 작업은 보존한다. `--dry-run`으로 삭제 없이 계획을 조회할 수 있다.
- 부팅 정리 `purge_stale_agent_state_on_boot`에 오래된 작업 삭제를 추가했다. 상태 필터 없이 7일 이상이라는 잠정 기준을 적용하되 agent.task_purge와 같은 삭제·참조 규칙을 따른다. 메모리 TTL의 PutOpts.expires_at은 사용하지 않는다.
- **작업 결과를 합치기 전에 일부 값만 선택할 수 있다.** agent.task_reduce의 extract_path(`--extract-path`)에 RFC 6901 JSON Pointer(예: /stdout/text)를 지정한다. Run 결과의 `{pid,stdout:{text,...},stderr:{...}}` 전체를 concat_text·merge_json에 넘기면 의도한 텍스트를 합치지 못하거나 뒤 결과가 앞 결과를 덮을 수 있었다. 생략하면 계속 전체 output을 사용한다. 경로가 없는 입력은 null로 대체하고 warnings에 사유를 남기며 다른 입력의 처리는 계속한다.
- **SDK에 HostHandle::self_invoke(method, params)를 추가했다.** 플러그인이 자기 네임스페이스의 메서드를 호스트 왕복 없이 워커 큐로 요청한다. HostHandle::call은 호스트의 self-call 전달 제한 때문에 이 용도로 사용할 수 없었다. 응답 대상이 없는 비동기 요청이며 실패는 tracing 경고로만 남긴다.

### Changed

- TaskCommand::Custom.poll(PollSpec)의 interval_ms를 생략할 수 있다. 기본은 500 ms이며 이전에는 필수 필드라 생략 시 역직렬화에 실패했다.
- (BREAK) **Reduce 입력을 실행 의존성으로 처리한다.** TaskCommand::Reduce.inputs는 depends_on처럼 모든 입력이 성공·실패를 포함한 종료 상태가 될 때까지 waiting을 유지한 뒤 ready가 된다. 이전에는 depends_on이 없으면 즉시 실행해 미완료 입력을 succeeded:false·output:null로 합치고 성공 처리할 수 있었다. Reduce.inputs도 순환 검사에 포함한다.
- **Run 작업의 stdout·stderr를 보관한다.** TaskCommand::Run은 호스트 stdio를 상속하는 대신 각각 마지막 64 KiB와 truncated/dropped_bytes를 수집한다. 성공하면 result.output에, 0이 아닌 종료 코드로 실패하면 같은 정보를 result.error 문자열에 포함한다. 이전의 pid만 있는 결과보다 빌드 등의 실패 원인을 확인하기 쉽다.
- **호스트 재시작 정리를 부팅 때 한 번 수행한다.** 살아 있는 모든 워크스페이스에서 오래된 semaphore·lease 점유 회수, 이전 Running 작업의 Failed("host restart") 처리, 저장된 handle 읽기를 러너 스레드 없이 수행한다. 이전에는 task_run --action start를 호출해야 정리됐다. 자동 실행 재개는 도입하지 않으며 사용자·플러그인이 start해야 한다.
- **AwaitExternal의 완료 기한을 저장한다.** DispatchHandle::AwaitExternal에 실행 전달 시점의 now + timeout_ms인 deadline_ms를 추가했다. hook_task_waits는 저장하지 않아 재시작 후 훅으로 깨울 수 없지만, 저장된 기한으로 만료를 판단할 수 있다. 필드 도입 전 handle은 다음 reload에서 deadline_ms=0으로 읽고 즉시 Failed로 마감한다. 업그레이드 후 첫 reload에서 기존 외부 신호 대기 작업이 실패할 수 있다.
- (BREAK) **agent.task_await를 로컬 전용으로 제한했다.** 플러그인이 호출하면 -32001을 반환한다. 실제 대기가 SDK의 단일 워커를 막아 다른 호스트 요청을 처리하지 못할 수 있어 approval.await와 같은 제한을 적용했다. 플러그인은 contributes.completion_strategy로 러너가 기다리게 하거나 task_get을 반복 조회해야 한다. 로컬 CLI는 영향이 없다.
- (BREAK) **task_await의 생략한 timeout 기본값을 10분으로 바꿨다.** 잠정값은 600,000 ms이며 이전에는 무기한 대기했다. 이제 10분 뒤 `{"outcome":"timed_out"}`을 반환한다. 무기한 대기가 필요하면 timeout_ms:0 또는 `tasty agent task-await --timeout-ms 0`을 명시한다.

### Removed

- (BREAK) **본체의 `tasty design *` 명령과 `design.*` IPC를 제거했다.** login/logout/import-session/status/projects/detect/probe/chat/chat-status/turn-status/protocol 11개 명령을 포함한다. claude-design 플러그인을 별도 프로젝트로 분리했으며 대체 명령·alias는 없다. [ADR-0035](docs/adr/0035-shared-design-and-theme.md)의 디자인 도구 분담을 따른다.

### Fixed

- **마크다운의 백그라운드 reload 호출을 고쳤다.** 자기 네임스페이스를 HostHandle::call로 호출해 호스트 self-call 정책에 따라 -32601 Method not found: markdown.reload가 되던 문제다. 위 Added의 SDK HostHandle::self_invoke로 자체 워커에 요청한다.
- **raw attach가 연결 끊김 후 재연결 경로로 돌아오게 했다.** tasty remote attach --raw와 tasty attach --raw가 종료 이유와 관계없이 process::exit(0)을 호출해 기본으로 켜진 reconnect가 동작하지 않던 문제를 고쳤다. 이제 mirror-dump처럼 종료 이유를 구분해 반환하고 AttachExit::Disconnected의 재연결 판단을 진행한다.
- **완료 전략의 소유 네임스페이스 검사를 고쳤다.** contributes.completion_strategy의 default_for_methods/poll_method를 reverse-DNS ID(com.tasty.claude 등) 대신 실제 ipc_namespace 접두어(claude 등)와 비교한다. 기존에는 서로 다른 형식을 비교해 플러그인 전략을 모두 등록하지 않았다.
- **작업 생성 시 fallback·reduce 대상의 존재를 검사한다.** agent.task_create가 `OnFailure::Fallback{task}`와 TaskCommand::Reduce.inputs의 없는 ID를 -32602로 거절한다. 이전에는 fallback을 무시해 후속 작업이 waiting에 남거나 reduce 실행 때 실패했다. 기존 저장된 잘못된 참조를 마이그레이션하지는 않으며 해당 실패 전이가 발생하면 tracing 경고를 남긴다.
- **여러 main 창에서는 터미널 부모를 명시해야 한다.** terminal.kill/release/respawn/broadcast가 `--surface` 생략 시 포커스 창 안에서만 단일 부모를 찾고 다른 창의 자식을 조작할 수 있던 문제를 고쳤다. main 창이 둘 이상이면 명시 오류로 거절한다. 단일 창에서는 기존처럼 생략할 수 있다.

## [0.9.7] - 2026-07-15

이전 릴리스 이후 누적된 변경을 반영했다.
