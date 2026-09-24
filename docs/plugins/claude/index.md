# Claude Code (`com.tasty.claude`)

- **Status**: Implemented (bundled plugin)
- **주체**: AI Agent / 로컬 사용자 (`tasty claude` CLI · IPC)
- **배포/통합**: bundled · cli + ipc_namespace + 멀티에이전트 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-claude/`
- **권한**: `terminal.spawn` 등 (매니페스트 `permissions`) · `memory.read`(Stop 훅이 실행된 surface의 goal을 읽는다 — 읽기 전용)
- **화면**: 없음 — CLI/IPC 로 터미널 surface 를 조작하는 실행 관리 플러그인 (headless).

> CLI·IPC, 부모·자식 관계, 훅 설치를 함께 제공하는 예제다. 구현 구조는 [플러그인 개발](../../dev-guide/plugin-development.md#cli--ipc-namespace)을 참고한다.

## 부모의 완료 수신 채널

자식의 상태 변경은 caller surface의 `<parent_home>/notify/<caller_surface>.log`에 한 줄로 기록한다. 자식 CLI 종류에 따라 수신 경로를 바꾸지 않는다. 훅 설치와 부모의 로그 수신 준비는 별개이며 idle·needs_input·interrupt·exit는 작업 성공을 뜻하지 않는다. [완료 알림 로그](../../dev-guide/external-interaction.md#child-완료-알림--completion-log)를 따른다.

## 목적

**Claude Code CLI를 tasty 안에서 실행하고 관리**하는 통합. 새 워크스페이스/페인에 Claude 인스턴스를 띄우고, 부모-자식 관계로 여러 인스턴스를 spawn·제어한다 (멀티에이전트).

## 내부 동작

- **입력 기본값**: 설치 후 최초 활성화에서 `settings.initialize_input_rule`로 `claude`의 Shift+Enter → LF 규칙을 등록한다. 기존 설치본도 업데이트 후 최초 활성화에서 등록한다. 사용자가 이미 지정한 값은 보존하며, 이후 수정·삭제한 규칙은 재시작·재활성화로 덮어쓰지 않는다. 등록 이력은 호스트 설정에 영속화한다. 프로세스 이름의 정확한 매칭을 사용하므로 `node` 등 런처 이름만 감지되는 환경은 해당 실행 파일명으로 사용자가 별도 규칙을 지정해야 한다.

짝 핸들러의 서로 다른 공개 응답과 번역 형식은 기존 호출자 호환을 위해 유지한다.
`children`/`kill`의 응답 형식과 상태 훅의 공통점·차이점은
[짝 핸들러 호환 경계](../../dev-guide/paired-agent-handlers.md)를 따른다. 완료 알림은
[completion-log](../../dev-guide/external-interaction.md#child-완료-알림--completion-log)에 기록하며 caller PTY에 메시지를 입력하지 않는다.

- **cli `claude`** (`tasty claude …`) — 서브커맨드: `launch`(새 워크스페이스에서 실행) · `spawn`(자식 인스턴스, 페인 분할) · `children`/`parent`(관계 조회) · `tell`/`broadcast`(메시지 전송) · `kill`/`respawn` · `reboot`(같은 세션 resume 재시작, 아래) · `child-profile`(자식에게 지속 프로필 부착, 아래) · `hook`(Claude Code 훅 통합, 아래 "Claude Code 훅 통합" 절) · `checklist-hook`(`continue-checklist` 세션 프로필 전용 `Stop` 훅, 아래 "continue-checklist 세션 프로필" 절) · `checklist-enable`/`checklist-disable`/`checklist-status`(게이트별 마커 파일을 켜고 끄고 조회 — `--gate` 생략 시 `continue-checklist`, 같은 절) · `notify-done`(내부용: spawn/tell 상태 전환 시 caller 에게 알림 전달 + 형제 hook 정리·재등록, 아래) · `profile-register`/`profile-unregister`/`profile-list`/`profile-show`/`profile-current`(Claude 세션 프로필 레지스트리, 아래 "Claude 세션 프로필 레지스트리" 절).
- `spawn`/`tell`은 필요한 호스트 호출의 응답을 받은 뒤 반환하며 자식 작업 완료까지 기다리지는 않는다. `claude-idle`/`needs-input`/`process-exit` once 훅을 등록한다. 하나가 실행되면 `notify-done`이 상태 변경 로그를 쓰고 같은 명령의 형제 훅을 정리한다. 이후 `surface.locate`가 성공하면 훅을 다시 등록한다. Surface 존재는 프로세스 생존과 같지 않으며, 등록·호출·로그 기록 실패나 재등록 사이의 이벤트까지 전달한다고 보장하지 않는다.
- **ipc_namespace `claude`** — 위 동작에 대응하는 IPC API.
- 실제 Claude 프로세스는 터미널 surface 안에서 돌고(`terminal.spawn`), 플러그인은 그 생명주기·관계를 관리한다.
- **`reboot`** (`tasty claude reboot [--surface <id>] [--delay <초>] [--prompt <추가문구>] [--profile-file <경로> | --profile <이름[,이름2,...]>] [--clear-profile] [--permission-mode <모드>]`) — surface 안의 Claude 를 종료하고 **같은 세션으로 재시작**한다.
  Claude 는 스스로 자기 TUI 를 껐다 켤 수 없으므로 에이전트가 이 명령을 자기 surface 에 호출한다(설정/훅/버전 변경 반영용).
  요청 처리 중 세션·프로필 정보를 확인한 뒤 재시작 작업을 예약하고 응답한다. `--delay`(기본 5s) 후 Ctrl+C를 0.5s 간격으로 4회 보내고 전경 이름이 Claude에서 바뀌었는지 확인한다. 바뀌면 `claude -r <session_id>`를 전송하며 프로필이 있으면 `--settings "<경로>"`를 붙인다. Claude 복귀를 확인한 뒤 안내 문구와 별도 Enter를 보낸다. 전경 이름과 화면 확인은 휴리스틱이며, 조회와 입력 사이의 상태 변화까지 막는 원자적 제출은 아니다.
  전경이 계속 Claude로 보이면 resume 명령을 보내지 않고, 복귀를 확인하지 못하면 안내 문구를 보내지 않는다. 같은 surface의 중복 reboot는 거절한다.
  **턴의 마지막 행동으로 호출할 것** — delay 이후 진행 중이던 턴은 잘린다.
  - **`claude-session-id` meta 가 비어 reboot 가 실패하는 경우**: `no active claude session on surface {id} (claude-session-id meta not set …)` 에러는 hook 미설치가 아니어도 발생할 수 있다 — session-start hook 이 이 meta 를 못 심은 것이 원인.
    조용히 실패할 수 있는 지점이 최소 3곳: ① `install.rs`의 등록 커맨드가 `if [ -n "$TASTY_SURFACE_ID" ]; then tasty claude hook … || true; fi` 라 `TASTY_SURFACE_ID` 미설정 시 tasty 바이너리 자체가 실행되지 않음(로그 불가), ② `hook.rs`의 `apply_hook` session-start 분기가 stdin JSON 에 `session_id` 가 없으면 meta 기록을 건너뜀(`tracing::warn!`으로 로그, `tasty plugin logs com.tasty.claude --follow` 또는 `~/.tasty/plugins-logs/com.tasty.claude.log` 에서 확인), ③ `dynamic/stdin.rs`의 `read_stdin_json` 이 TTY/파싱 실패로 `None` 을 반환(hook 은 CLI 프로세스라 tracing 이 **stderr 로만** 나간다 — 공유 로그 파일에는 남지 않는다, [ADR-0043](../../adr/0043-cli-errors-and-diagnostic-logs.md).
    전달 실패 자체는 `$TASTY_HOME/hook-failures.log` 에 기록된다).
    수동 복구: `tasty surface-meta set --key claude-session-id --value <세션ID>`.
- **`child-profile`** (`tasty claude child-profile [--surface <부모>] --child <index> [--delay <초>] [--prompt <추가문구>] [--profile-file <경로> | --profile <이름[,이름2,...]>] [--clear-profile] [--permission-mode <모드>]`) — **부모가 자식에게 지속 세션 프로필을 부착**한다.
  `--child <index>`(`claude children` 이 보여주는 index)를 `terminal.children` 으로 자식 surface id 로 해석한 뒤, 그 surface 에 대해 위 `reboot`과 같은 처리 경로를 사용한다(프로필 검증 → surface meta 부착 → Ctrl+C 시퀀스 → `claude -r <sid> --settings "<경로>"` → 안내 프롬프트).
  별도 부착 메커니즘이 아니라 reboot 진입점의 재사용이므로, 부착 상태는 자식의 이후 **무인자 `reboot` 에 그대로 승계**된다.
  중복 가드도 reboot 과 같은 set 을 쓴다 — 같은 자식에 `reboot` 과 이 명령이 겹치면 뒤엣것이 "이미 진행 중" 으로 거부된다.
  - **`reboot` 과 다른 점 1 — 턴이 잘리지 않는다.** `reboot` 의 "턴의 마지막 행동으로 호출할 것" 경고는 **호출자 자신이 재기동될 때**의 제약이다. 이 명령은 자식만 재기동시키므로 **부모의 턴은 잘리지 않는다** — 호출 후 계속 작업해도 된다.
  - **상태 알림**: `spawn`/`tell`과 같은 상태 변경 훅을 caller에 등록한다. `reboot` 자체는 이 알림을 등록하지 않는다. 실제 전달은 훅 실행과 로그 기록의 성공 여부에 달려 있다.
  - **`--child` 는 필수다.** 자기 자신에게 붙이는 것은 `reboot --profile` 의 몫이라 창구를 겹치지 않게 한다. 없는 index 를 주면 사용 가능한 index 목록과 함께 즉시 에러이며, **아무 자식도 죽지 않는다** — 프로필 인자 검증(상호배타 · 미등록 이름 · JSON 파싱)도 전부 Ctrl+C 시퀀스 시작 **이전**에 끝난다.
  - **`spawn`/`respawn`/`launch` 의 `--profile` 과의 차이**: 그쪽은 그 기동 명령 **1회에만** `--settings` 를 싣고 meta 를 건드리지 않는다 — 자식이 한 번이라도 `reboot` 하면 프로필이 빠진다. 지속 부착이 필요하면 이 명령을 쓴다.

- **Claude 세션 프로필**([용어 정의](../../concepts/ubiquitous-language.md)): 이 플러그인은 `reboot`/`spawn`/`respawn`/`launch`의 기동 명령에 프로필을 전달한다. 아래 두 지정 방법은 함께 사용할 수 없다.
  - `--profile-file <경로>`(`path_kind = "file"`, CLI 가 호출자 cwd 기준 절대경로로 정규화, **반복 지정 거부** — 아래 "왜 반복 지정을 CLI 가 거부하는가") — 파일 경로를 그대로 쓴다.
  - `--profile <이름[,이름2,...]>` — 아래 "Claude 세션 프로필 레지스트리"에 등록해 둔 프로필 이름, 또는 "Stop-훅 게이트 레지스트리"에 등록해 둔 **게이트 이름**으로 부착한다(두 레지스트리는 이름 공간을 공유한다). 이름을 둘 이상 쉼표로 주면 레지스트리가 머지해 만든 파일 하나를 쓴다.

  어느 쪽이든 최종적으로 기동 명령에 `--settings "<경로>"` 가 붙는다 — Claude Code 의 `--settings` 는 `~/.claude/settings.json` 의 기존 훅을 **대체가 아니라 병합**하므로 tasty 내장 훅(`claude hook` 경유)도 그대로 발생한다.
`reboot`(및 자식을 대상으로 같은 경로를 타는 `child-profile`) 만 부착 상태를 surface meta 에 기록해 **다음 무인자 reboot 가 기본값으로 승계**한다 — 경로로 부착하면 `claude-session-profile`(경로 그대로), 이름으로 부착하면 `claude-session-profile-names`(이름 문자열)에 기록되고 두 meta 는 상호 배타적으로 관리된다(한쪽을 새로 쓰면 다른 쪽은 지운다).
이름-meta 는 **승계 시점마다 레지스트리에서 다시 해석**한다 — 경로를 캐시하지 않으므로 그 사이 `profile-register` 로 내용이 갱신됐다면 다음 reboot 에 최신 내용이 반영된다.
두 meta 모두 파일 존재 + JSON 파싱을 매 reboot 마다 동기 재검증한다(승계된 프로필이 깨져 있으면 kill 시퀀스를 시작하지 않고 에러 반환).
`--clear-profile` 로 둘 다 뗀다.
`spawn`/`respawn`/`launch` 는 그 호출 1회의 기동 명령에만 반영하고 meta 를 건드리지 않는다(반복 재기동은 `reboot` 계열만의 개념) — 그렇게 띄운 자식에 지속 부착이 필요하면 `child-profile` 을 쓴다.
  - **복원을 건너 프로필이 유지된다** — 아래 "복원을 건너 프로필이 유지되는 방식".
  - **왜 반복 지정을 CLI 가 거부하는가**: Claude Code 의 `--settings` 는 반복 지정 시 **마지막 값만 남고 앞선 값이 조용히 사라진다**(실측). tasty CLI 인자 자체(`--profile-file`)를 실수로 두 번 주는 경우도 같은 함정에 빠질 수 있어, 매니페스트 `CliArg.reject_repeat = true`(`crates/tasty-cli/src/dynamic/build.rs`)로 clap 을 `ArgAction::Append` 로 등록해 두 번째 값이 들어오면 조용히 버리지 않고 에러로 거부한다.

### 복원을 건너 프로필이 유지되는 방식

앱 재시작이나 닫은 탭 복원에서는 새 surface ID를 사용하므로 이전 surface meta를 그대로 넘기지 않는다. 플러그인은 세션 ID별 부착 기록으로 프로필 정보를 복구한다. 호스트의 [레이아웃 복원](../../features/layout-persistence/index.md)은 에이전트 종류와 무관하게 `restore.command`를 사용한다.

- **기록 위치**: `TASTY_PLUGIN_DATA_DIR/profiles/attachments/<session_id>.json`. 내용은 `{"kind": "names"|"path", "value": …}` — **이름으로 부착한 것은 이름을** 남긴다(복원 시 재해석 대상). data dir 은 설치 디렉터리와 분리돼 있어 `upgrade-builtins`/재설치를 건너 보존된다([plugin-development](../../dev-guide/plugin-development.md) §6 "data dir 수명 계약").
- **쓰기**: `reboot` 이 프로필을 부착/해제할 때 surface meta 갱신과 같은 지점에서 함께 갱신한다(`--clear-profile` 은 meta 2키와 기록을 함께 지운다).
- **복구**: `session-start` 훅이 프로필 meta 를 먼저 보고, 비어 있으면(=복원을 건너온 세션) 기록에서 meta 를 되살린다. 그다음 이름을 **매번 다시 해석**해(레지스트리 최신 내용 반영) `restore.command` 를 `claude -r <id> --settings "<경로>"` 로 쓴다. 세션 id 는 `claude -r <id>` 를 건너 보존되므로 기록의 키로 쓸 수 있다 — 이 전제가 깨지면(Claude Code 가 resume 시 새 id 를 발급하면) 프로필만 조용히 빠지므로, 재시작 후 세션 생존 확인을 릴리스 점검에 유지한다.
- **재기록(re-stamp)**: session-start 때 프로필 기록을 다시 쓰고 `ended_at` 종료 표시를 지운다.
  reboot 중 SessionEnd가 남긴 종료 표시도 이때 제거한다. 기록의 수정 시각도 갱신되어
  실행 중인 세션이 종료 유예나 오래된 기록 정리에 걸리지 않게 한다.
- **복구 실패**: 저장한 이름이 해제됐거나 경로를 사용할 수 없으면 경고를 남기고 프로필 없이 복원한다. 반면 명시적 `reboot` 요청에서는 프로필 검증에 실패하면 종료 시퀀스를 시작하지 않는다.
- **수명**: `session-end`는 `ended_at`만 기록하고, 24시간 유예가 지난 뒤 sweep에서 삭제를 시도한다. 닫은 탭을 다시 열 때 프로필 조회와 다음 reboot의 승계 정보가 필요하기 때문이다. 종료 훅이 오지 않은 기록은 90일 TTL로 정리하며 session-start에서 기록 시각을 갱신한다. `--clear-profile`은 유예 없이 삭제를 시도한다.

기록 파일은 세션 ID로 찾는다. ID의 파일명 형식을 검증하지만, 이것이 다른 세션의 접근을 차단하는 권한 검사나 `claude -r`만 같은 ID를 사용할 수 있다는 보장은 아니다. 파일 작업 실패도 경고로 남으므로 모든 meta·기록 갱신을 하나의 원자적 작업으로 보지 않는다.
- **게이트도 같은 경로**: 프로필과 게이트는 같은 이름 공간을 사용하므로 게이트 이름으로 부착한 것도 그대로 복원된다. 같은 이름이 프로필↔게이트로 재등록됐으면 다음 복원은 **새 정의**로 해석한다(경로를 캐시하지 않는 것의 귀결).
- **범위 밖**: 레이아웃 프리셋은 세션 복원이 아니라 구조 템플릿이라 `restore_command` 를 저장하지 않는다 — 프리셋 적용으로는 프로필이 붙지 않는다. `spawn`/`launch`/`respawn --profile` 은 부착 기록을 만들지 않는다(반복 재기동은 `reboot` 만의 개념).

### Claude 세션 프로필 레지스트리

프로필 JSON을 이름으로 등록하고 `--profile <이름>`으로 부착한다. 레지스트리는 이 플러그인의 `profile.rs`가 관리한다.

- **등록**: `tasty claude profile-register <이름> --file <경로>` — `<경로>`(JSON object) 를 읽어 `TASTY_PLUGIN_DATA_DIR/profiles/registered/<이름>.json` 에 **복사본**으로 저장한다(원본이 나중에 옮겨지거나 지워져도 레지스트리는 영향받지 않는다). 이미 등록된 이름이면 내용을 덮어쓴다. 이름은 소문자/숫자/`-`, 최대 32자.
- **해제**: `tasty claude profile-unregister <이름>`.
- **목록**: `tasty claude profile-list` — **이름으로 부착 가능한 것 전부**를 보여준다: 등록 프로필(`user/<이름>`, `description` 없음) · 등록 게이트(`user/<이름>`, 게이트임을 알리는 `description`) · host 기본 게이트(`host/continue-checklist`). 여기에 항상 전역 설치돼 있는 내장 훅 9종(`host/<token>`, attachable 아님 — 위 "Claude Code 훅 통합" 절의 `install.rs::MANAGED_HOOKS` 를 그대로 나열, 정의를 복제하지 않는다)이 더해진다. `profile-list` 와 `gate-list` 가 둘 다 게이트를 보여주는 것은 의도된 중복이다 — 전자는 "부착 가능한 것들" 관점, 후자는 "게이트 정의"(본문·센티넬·상한·on/off) 관점.
- **조회**: `tasty claude profile-show <이름>` — 등록 프로필이면 원본 JSON 그대로, 게이트면 그 게이트를 발동시키는 **생성된 Stop 훅 조각**. `owner` 는 실제 출처를 그대로 반영한다(`user` 등록 프로필/등록 게이트, `host` 기본 게이트). `tasty claude profile-current [--surface <id>]` — 그 surface 에 지금 부착된 것(이름 또는 경로)과 내장 훅 목록을 함께 보여준다("지금 이 세션에 무슨 프로필/게이트가 걸려 있나").
- **이름 해석 순서(부착 시)**: `--profile <이름>` 으로 **부착할 때**의 순서다 — ① `profiles/registered/<이름>.json` → ② 등록 게이트 → ③ host 기본 게이트 → ④ 내장 훅 토큰이면 "attach 불가" 에러 → ⑤ 그 외 미등록 에러. ①과 ②는 등록 시점에 상호 배제되지만(아래 "Stop-훅 게이트 레지스트리" 의 이름 충돌 거부) 순서는 방어적으로 고정돼 있다. 위 `profile-show` 는 이 경로를 쓰지 않으므로 ④가 적용되지 않는다 — 내장 훅 토큰을 주면 "attach 불가" 가 아니라 미등록 에러(`no registered profile named 'user/stop'`)가 난다.
- **프로필 병합**(`crates/tasty-plugin-claude/src/profile_merge.rs`): 여러 이름을 지정하면 순서대로 읽어 JSON 하나로 만들고 `TASTY_PLUGIN_DATA_DIR/profiles/generated/<정렬된-이름들>.json`에 저장한다. 부착할 때마다 다시 만들며 등록 원본은 덮어쓰지 않는다. 병합 규칙은 다음과 같다.

  | 키 유형 | 예 | 규칙 |
  |---|---|---|
  | 훅 이벤트 배열 | `hooks.Stop` | JSON 값 전체가 같은 항목만 중복 제거하고 나머지는 이어 붙임 |
  | 객체 맵 | `env`, `enabledPlugins` | 키 단위 재귀 병합. 리프 값 충돌은 스칼라 규칙과 동일 |
  | 허용/거부 리스트 | `permissions.allow`/`deny` | union 후 **불변식 강제**: `deny` 에 있는 항목은 `allow` 에서 제거한다 — 프로필 조합 순서와 무관하게 deny를 항상 우선한다(테스트: `profile_merge::tests::deny_beats_allow_*`) |
  | 스칼라(대부분) | `theme`, `effortLevel` | 값이 다르면 경고 로그 남기고 나중 프로필 값으로 last-wins |
  | 스칼라(보안 민감) | `permissions.defaultMode` | 값이 다르면 **거부**(에러) — 권한 모드가 조합으로 조용히 약해지는 것을 last-wins 보다 우선 차단 |

- **저장 위치** — 전부 `TASTY_PLUGIN_DATA_DIR`(`~/.tasty/plugin-data/com.tasty.claude/`) 하위. 호스트가 이 디렉토리를 미리 만들어 주므로 `fs.write` 권한 없이도 쓸 수 있다. 호스트가 이 env 를 주입하지 않은 비정상 기동(`data_dir = None`)이면 등록/부착 모두 명시적 에러로 거부한다 — `~/.claude/` 나 새 경로를 조용히 쓰지 않는다.
- IPC: `claude.profile_register`/`claude.profile_unregister`/`claude.profile_list`/`claude.profile_show`/`claude.profile_current` — CLI 서브커맨드와 1:1 대응(원칙 2, 에이전트 조작 가능성).
- spawn 시 parent 의 살아있는 child 수가 설정 임계치를 넘으면 응답에 `warning` 필드가 실린다 — Settings › Plugin › Claude Code 에서 임계치 조정. 재사용 후보는 근거가 다른 두 목록으로 나뉜다: **`idle`**(자식이 hook 으로 완료를 직접 보고) 과 **확정 `stale`**(`confidence: confirmed` — 보고는 없었지만 전경이 셸로 복귀해 에이전트 프로세스 종료가 관측됨, 완료 훅 보고와 별도로 확인된 종료). `confidence: heuristic` 인 `stale` 은 SIGSTOP·긴 추론과 구별되지 않아 세지 않는다 — 판정 근거는 [child-terminal](../../features/child-terminal/index.md) "판정 응답 필드" 참조.

### 승인 정책 (`--permission-mode`)

`launch`, `spawn`, `respawn`, `reboot`, `child-profile`은 `permission_mode`를 받아
Claude 기동 명령의 `--permission-mode`로 전달한다. 허용 값은 `acceptEdits`, `auto`,
`bypassPermissions`, `manual`, `dontAsk`, `plan`이며 다른 값은 거절한다.
이 플러그인은 값의 의미를 다시 정의하지 않는다.

우선순위는 호출별 인자 → `default_permission_mode` 설정 → 플래그 미부착이다.
설정 기본값 `inherit`도 미부착을 뜻한다. 이 경우 사용자 자신의 Claude 설정이 적용된다.
모드를 지정하지 않은 무인 자식은 사용자 설정에 따라 승인을 기다릴 수 있다.
관측과 알림은 대기를 알려줄 뿐, 승인을 대신하지 않는다.

프로필의 `permissions.defaultMode`와 별도 permission mode가 함께 지정되면 거절한다.
`permissions.allow`와 `deny` 목록은 모드와 함께 사용할 수 있다.
`reboot`와 `child-profile`의 모드는 해당 재시작에만 적용하고 `restore.command`에 넣지 않는다.
Codex와 옵션을 억지로 맞추지 않으며, 각각의 기본값은 해당 플러그인 문서를 따른다.

### Stop-훅 게이트 레지스트리

Stop 게이트는 턴을 끝내기 전에 확인할 본문, 종료 표시 문자열(sentinel), 반복 상한을
이름으로 등록한 것이다. 선택 이유는 [에이전트 작업 조율](../../adr/0042-agent-coordination-and-task-views.md)을 따른다.

| 명령 | 동작 |
|---|---|
| `gate-register <이름> --body-file <경로> [--sentinel <문자열>] [--rounds <n>]` | 본문을 복사해 저장. 같은 이름은 정의와 본문을 갱신 |
| `gate-unregister <이름>` | 정의·본문·마커·반복 상태 제거. 실행 상태 정리 실패는 경고 |
| `gate-list` | 사용자 및 내장 게이트의 실효 sentinel·상한·상한 출처·enabled 표시 |
| `gate-show <이름>` | 정의와 본문 조회. 사용자 정의가 없으면 내장 게이트 조회 |

각 명령은 `tasty claude` 뒤에 붙이며 대응 IPC는 `claude.gate_*`다. 전용 GUI는 없다.
이름은 소문자·숫자·`-`로 최대 32자다. 등록 본문에는 비어 있지 않은 실효 sentinel이
있어야 하며 `--rounds`는 1 이상이다. 프로필과 게이트는 같은 이름을 사용할 수 없으며
양쪽 등록 경로에서 충돌을 검사한다. 사용자 정의가 같은 이름의 내장 게이트를 대체하면
목록에는 실효 사용자 항목만 표시한다.

데이터는 `TASTY_PLUGIN_DATA_DIR` 아래에 둔다.

| 경로 | 내용 |
|---|---|
| `gates/registered/<이름>.json` | 실효 sentinel, 선택한 round_limit |
| `gates/bodies/<이름>.md` | 등록한 본문의 복사본 |
| `checklist/gates/<이름>/enabled.marker` | 재시작 없이 바꾸는 활성 상태 |
| `checklist/gates/<이름>/rounds/<session_id>.json` | 게이트·세션별 반복 수 |

내장 `continue-checklist`는 파일로 만들지 않고 코드와 번역 카탈로그에서 제공한다.
데이터 디렉터리가 없으면 등록·해제는 실패하고 조회는 내장 게이트만 반환한다.

`launch`, `spawn`, `respawn`, `reboot`의 `--profile <게이트이름>`으로 부착한다.
`--profile gate-a,gate-b`처럼 여러 게이트나 일반 프로필을 함께 사용할 수 있다.
등록 프로필 → 등록 게이트 → 내장 게이트 → 내장 훅 토큰 순서로 이름을 해석한다.

부착 명령에는 `checklist-hook --gate <이름>`만 넣고 본문·상한은 넣지 않는다.
훅이 실행될 때 다시 읽으므로 재등록한 내용이 재부착 없이 반영된다.
이름이 셸 명령에 들어가므로 허용 문자를 넓히면 인용·이스케이프도 함께 바꿔야 한다.
명령 문자열은 `install.rs::tasty_guarded_command`를 공유한다.

### Stop-훅 게이트 판정

`checklist-hook`은 선택한 게이트의 sentinel·본문·상한을 읽고 다음 순서로 판단한다.

1. 저장된 prompt ID가 다르거나 상태가 없으면 반복 수를 0으로 본다.
2. 마지막 응답에 sentinel이 있으면 통과한다.
3. 반복 상한에 도달했으면 통과한다.
4. 나머지는 `decision: block`과 본문을 reason으로 반환하고 반복 수를 1 늘린다.

상한은 게이트의 `--rounds` → `continue_checklist_round_limit` 설정 → 3 순서다.
설정 키에 continue_checklist라는 이름이 남아 있지만, 상한을 지정하지 않은 사용자
게이트에도 같은 기본값을 적용한다. `round_limit_source`는 gate 또는 settings로 출처를 표시한다.
등록 본문은 매번 파일에서 읽고 내장 본문만 플러그인 기동 시 읽은 번역을 사용한다.

`--gate`를 생략하면 continue-checklist를 사용한다. 미등록 게이트, 꺼진 마커,
세션·prompt ID 부재, stdin 파싱 실패에서는 차단하지 않는다.
미등록 훅은 상태 파일을 만들거나 설정 조회를 보내지도 않는다.
Stop payload에는 게이트 이름이 없으므로 이름은 명령 인자로 전달한다.
`stop_hook_active`는 입력 부재와 false를 구분하기 위해 CLI 스키마에서 string으로 받고
핸들러에서 해석한다. 판정 자체는 위 반복 수와 sentinel을 사용한다.

게이트 여러 개가 붙으면 각각의 Stop 훅이 실행되고 각각의 reason이 전달될 수 있다.
하나라도 block이면 계속되므로 한 게이트가 상한에 도달해도 다른 게이트는 계속 막을 수 있다.
반복 수를 게이트·세션별로 분리하는 이유다.

#### 본문에 goal 넣기

본문에 `{{goal}}`이 있고 block을 반환할 때만 훅이 `memory.goal_get`을 호출한다.
goal이 있으면 현재 언어의 goal 절로 바꾸고, 없거나 조회에 실패하면 토큰이 있는 줄을
제거한다. 토큰이 없는 본문은 바꾸지 않는다. 번역에서도 토큰과 삽입 자리를 유지해야 한다.

목표는 서피스 범위에 둔다. CLI가 쓰고 플러그인은 읽기만 하며 부모의 목표를 자동 상속하지 않는다.
`surface` 인자를 생략하면 CLI가 `TASTY_SURFACE_ID`를 사용하므로 옛 훅 명령도 동작한다.
이 지시는 목표 범위에서 사용자 판단 없이 진행할 수 있는 일을 계속하도록 안내한다.
목표 충족 자체를 계산하거나 작업 성공을 보증하는 검사는 아니다.

### continue-checklist 세션 프로필

내장 게이트 `host/continue-checklist`를 `--profile continue-checklist`로 붙인다.
전역 install에는 포함되지 않으며 부착한 세션에서만 동작한다.
기본 sentinel은 `[[TASTY-CHECKLIST-DONE]]`이다.
본문은 요청 충족 여부, 실제 검증 여부, 남은 작업을 확인하고 선택적으로 goal을 덧붙인다.
goal이 없으면 범위를 넓혀 계속 진행하라는 지시는 하지 않는다.

`checklist-enable`, `checklist-disable`, `checklist-status`에 `--gate <이름>`을 붙여
마커를 제어한다. 생략하면 continue-checklist다. enable/disable은 미등록 이름을 거절하고,
status는 `{ enabled: false, registered: false }`로 답한다. 데이터 디렉터리가 없을 때
변경은 실패하고 status는 enabled false로 응답한다.

마커는 매 훅마다 확인하므로 이미 실행 중인 세션에도 즉시 적용된다.
옛 `checklist/enabled.marker`는 내장 게이트 위치로 한 번 옮긴다.
SessionEnd는 모든 게이트에서 해당 세션의 반복 파일을 지우고 옛 `checklist/rounds/`의
같은 세션 파일도 정리한다. 옛 반복 수는 읽거나 이전하지 않는다.

### Claude Code 훅 통합

훅은 호스트 호출에 실패해도 뒤의 로컬 정리를 계속한다. 응답의 `host_call_failures`에 실패한 호출 수를 넣고, 0이 아니면 `<tasty_home>/hook-failures.log`에도 기록을 시도한다. 따라서 응답의 `ok`만으로 모든 호출이 성공했다고 판단하지 않는다. [오류 처리](../../dev-guide/error-handling.md)와 [훅 실행 설계](../../adr/0027-lua-and-hook-execution.md)를 참고한다.

`tasty claude install`이 `~/.claude/settings.json`의 `hooks`에 아래 9개 이벤트를 심는다. 모든 이벤트가 같은 형태의 명령 문자열을 쓴다:

```
if [ -n "$TASTY_SURFACE_ID" ]; then tasty claude hook <token> || true; fi
```

`TASTY_SURFACE_ID`가 없으면 Tasty 밖에서 실행한 것으로 보고 훅 명령을 호출하지 않는다. 값이 있으면 훅을 실행하며, `|| true`는 훅 실패가 에이전트 턴을 막지 않게 한다. 훅 전달 실패는 `<tasty_home>/hook-failures.log`에 기록하는 경로를 사용한다([CLI 진단 로그](../../adr/0043-cli-errors-and-diagnostic-logs.md)).

명령 문자열 생성은 `install.rs::tasty_guarded_command` 한 곳뿐이다 — 세션 프로필(`continue-checklist`)의 hook 명령도 같은 함수를 쓴다.

> **기존 사용자는 `tasty claude install` 재실행이 필요하다.** 명령 문자열은 사용자의 `settings.json` 에 이미 기록돼 있어, plugin 을 업데이트해도 옛 문자열 그대로다. 재실행하면 marker(`tasty claude hook <token>`) 가 일치하는 기존 entry 를 찾아 **제자리 갱신**하므로 entry 가 중복되지 않는다.

`session_id`/`message`/`notification_type`/`error`/`agent_id` 같은 이벤트별 가변 데이터는 명령 인자가 아니라 **stdin JSON**으로 들어온다 — 매니페스트 `hook` cli 항목이 `stdin_json = true`를 선언하고, `--session`/`--message`/`--notification-type`/`--error`/`--agent-id` 플래그가 각각 `stdin_field`로 stdin JSON에서 자동 채워진다(Claude Code가 hook 실행 시 stdin으로 JSON payload를 준다). POSIX 셸 구문 1종만 발행한다 — [codex](../codex/index.md)처럼 Windows PowerShell 분기는 없다.

| Claude Code 이벤트 | matcher | tasty hook token | `terminal.set_state` | `surface.fire_hook` | surface meta | `surface.completion` kind |
|---|---|---|---|---|---|---|
| `Stop` / `SubagentStop` | `""`(전체) | `stop` / `subagent-stop` | `idle` | `claude-idle` | — | `completion` |
| `StopFailure` | `""`(전체) | `stop-failure` | `idle` | `claude-idle` + `claude-stop-failure` | `claude-last-stop-failure` = stdin `error`(없으면 `unknown`) **set** | `completion` |
| `SessionEnd` | `""`(전체) | `session-end` | `idle` | `claude-idle` | `claude-session-id`·`restore.command`·`claude-last-stop-failure` **unset** (프로필 meta 2키는 건드리지 않는다. 프로필 **부착 기록**에는 종료 표시만 하고 유예 뒤 회수 — 아래 "복원을 건너 프로필이 유지되는 방식") | `completion` |
| `Notification` | `""`(전체) | `notification` | `needs_input`(단 `notification_type`이 `idle_prompt`면 건너뜀 — 무입력 대기 오탐이라 실제 질문 없음) | `needs-input`(동일 조건) | — | `needs_input`(동일 조건) |
| `UserPromptSubmit` | `""`(전체) | `prompt-submit` | `active` | — | `claude-last-stop-failure` **unset** | — |
| `SessionStart` | `""`(전체) | `session-start` | `active` | — | `claude-last-stop-failure` **unset**. `claude-session-id` = 세션 ID, `restore.command` = `claude -r <id>` **set**(stdin JSON에 `session_id`가 없으면 건너뜀). 프로필이 부착돼 있으면 `claude -r <id> --settings "<경로>"` 로 쓰고, 복원으로 프로필 meta 가 사라졌으면 부착 기록에서 **복구**한다(아래 "복원을 건너 프로필이 유지되는 방식") | — |
| `PreToolUse` | `AskUserQuestion` | `pre-tool-use` | `needs_input` | `needs-input` | — | `needs_input` |
| `PostToolUse` | `AskUserQuestion` | `post-tool-use` | `active` | — | — | — |

`surface.completion` 은 `{ surface_id, kind }` 로 호출되며(`HostCall::SurfaceCompletion`,
`hook.rs`), `kind` 는 위 표의 값을 그대로 싣는다 — 호스트의 `AttentionStore` 가
`NeedsInput` 을 `Completion` 보다 높은 우선순위로 표시한다(surface 테두리 노랑, 탭
제목·워크스페이스 배지도 동일 우선순위, [surface-highlight](../../features/surface-highlight/index.md)
참고).

`UserPromptSubmit`은 child가 2번째 이후 prompt를 받을 때 직전 `Stop` hook이 남긴 `idle=true` 잔재를 지우는 데 필수다 — 미등록 시 실제로는 active인 child를 idle로 오보고하는 상태 버그가 생긴다.
`PreToolUse`/`PostToolUse`만 matcher `AskUserQuestion`으로 좁혀 등록돼 그 툴 호출에만 발생한다(나머지 7개는 matcher `""`로 이벤트 전체를 받는다) — 실측(실제 Claude Code를 띄워 hook stdin payload를 덤프해 확인) 결과 `AskUserQuestion` 답변은 `UserPromptSubmit`을 발생시키지 않으므로(질문/답변이 같은 prompt turn 안의 tool 상호작용이라 새 프롬프트로 집계되지 않음), 기존 `UserPromptSubmit`(→active)만으로는 이 케이스의 needs_input 해제 시점을 잡을 수 없다.
`PreToolUse`가 질문 UI가 뜨기 **전에** 발생해(`tool_input.questions` 포함) needs_input을 켜고, `PostToolUse`가 답변 즉시(관찰상 `duration_ms: 0`) 그 짝으로 active로 되돌린다 — `needs_input`은 이제 `Notification`과 `PreToolUse` 두 경로에서 나온다.

`StopFailure`는 API 오류로 끝난 턴을 알린다. `529 Overloaded`, rate limit, 인증 실패 등의
오류가 여기에 해당한다. 이 훅에서는 상태를 idle로 바꾸고 `claude-idle`도 보내 부모의
상태 알림이 실행될 수 있게 한다. 별도로 `claude-stop-failure`를 발생시킨다.

오류 종류는 `claude-last-stop-failure` meta에 기록한다. stdin의 `error` 값은
`rate_limit`·`overloaded`·`authentication_failed`·`billing_error`·`invalid_request`·
`server_error`·`max_output_tokens`·`unknown` 등이다. Surface의 Custom 훅은 값 없이
이벤트 키만 전달하므로 이 meta를 사용한다([훅](../../features/hooks/index.md)).
Meta를 먼저 쓴 뒤 이벤트를 보내며 새 턴(`prompt-submit`/`session-start`/`active`)과
`session-end`에서는 지운다. `notify-done`은 meta가 있으면 상태 알림 뒤에 오류 종류를 붙인다.

서브에이전트의 `stop-failure`는 메인 턴의 상태·알림·meta를 바꾸지 않고 로그만 남긴다.
Payload에 `agent_id`가 있는지로 구분한다. 메인 턴은 서브에이전트 실패를 도구 결과로 받고
계속 진행할 수 있기 때문이다. 이 필드 구분은 Claude Code 2.1.280의 payload 조립부를
확인한 근거이며 모든 버전의 외부 동작을 보장한다는 뜻은 아니다.

install이 심는 훅은 이 9개뿐이다 — matcher가 지정되지 않은 `PreToolUse`/`PostToolUse` 호출 전체나 `PreCompact` 등 다른 Claude Code 이벤트는 걸지 않는다. `install.rs`의 `install_preserves_other_hooks` 테스트가 사용자가 직접 추가한(matcher가 다른) `PreToolUse` entry를 tasty의 `AskUserQuestion`-matcher entry와 분리해 그대로 보존함을 검증한다.

install은 marker substring(`tasty claude hook <token>`)으로 자기 entry를 식별해 멱등하게 동작한다 — marker가 일치하는 기존 entry는 명령 문자열만 최신 형태로 덮어쓰고(옛 버전이 심은 잘못된 명령이 남는 회귀 방지), 사용자가 직접 추가한 다른 entry는 건드리지 않는다. `PreToolUse`/`PostToolUse`처럼 matcher가 있는 이벤트는 marker 일치만으로는 matcher 값까지 보증되지 않으므로, install이 matcher도 canonical 값(`AskUserQuestion`)으로 함께 갱신한다.

이 플러그인이 fire하는 surface hook 이벤트는 `claude-idle`/`needs-input`/`claude-stop-failure`/`claude-error`/`claude-error-stalled` 5개이며, 매니페스트 `contributes.hook_events`로 선언한다 — host가 (내장 ∪ 활성 plugin 선언) 집합으로 `hook.set` 등록을 검증하므로([hooks](../../features/hooks/index.md)), 이 플러그인이 비활성이면 저 5개 키로의 hook 등록도 거부된다.
**이 5개가 전부 위 9개 설치 훅에서 나오는 건 아니다** — `claude-idle`은 위 `apply_hook`(Stop/SubagentStop/StopFailure/SessionEnd)에서, `claude-stop-failure`는 `StopFailure`에서, `needs-input`은 `Notification`(idle_prompt 제외)과 `PreToolUse`(matcher `AskUserQuestion`) 두 경로에서 나오지만, `claude-error`/`claude-error-stalled`는 이 훅 메커니즘과 무관한 별도 producer다: `error_scan.rs`가 surface 출력 텍스트를 패턴 매칭해 매치 시 직접 `surface.fire_hook`으로 보낸다(정지 판정은 아래 절).
API 에러로 턴이 **끝나면** `StopFailure`가 구조적 신호를 주지만, 요청이 응답 없이 매달리면 턴이 끝나지 않아 `Stop`도 `StopFailure`도 **발생하지 않는다** — 그때는 PTY에 찍히는 에러 문자열과 출력 정적이 얻을 수 있는 유일한 신호다.
`claude-idle`/`needs-input`은 [surface-highlight](../../features/surface-highlight/index.md)(Stop hook → highlight)와 [telemetry](../../features/telemetry/index.md)(`session-start`→`stop`의 `wall_time_ms`, `notification`의 `input_tokens`)가 소비하고, `SessionStart`/`SessionEnd`의 meta set/unset은 [layout-persistence](../../features/layout-persistence/index.md)의 `restore.command` 복원이 소비한다.

### API 에러 뒤 자동 재개 (`auto_resume.rs`)

`StopFailure`로 끝난 턴에 일정 시간 뒤 재개 문구를 보낸다. 기본은 꺼짐이다.
선택 이유는 [에이전트 상태와 완료 전달](../../adr/0041-agent-state-and-completion.md)을 따른다.

| 설정 키 | 기본값 | 범위와 의미 |
|---|---|---|
| `auto_resume_enabled` | `false` | 자동 재개 사용 여부 |
| `auto_resume_delay_secs` | 10 | 1~86400초. UI와 코드 모두 범위를 제한 |
| `auto_resume_max_attempts` | 5 | 연속 재개 횟수, 최소 1 |

`overloaded`와 `server_error`만 예약한다. rate limit·인증·과금·잘못된 요청·출력 한도
오류는 자동 재개하지 않는다. `agent_id`가 있는 서브에이전트 실패도 예약하지 않는다.
이 분기의 외부 payload 근거는 Claude Code 2.1.280 조립부의 정적 확인이며, 서브에이전트
API 오류에서 실제 이벤트를 받은 실험까지 완료한 것은 아니다.

같은 서피스의 새 예약은 이전 예약을 덮는다. 새 턴, 성공 Stop, 세션 종료는 예약을 지운다.
전용 스레드가 500ms마다 만기를 확인하므로 훅 핸들러는 지연 시간 동안 기다리지 않는다.
시계 범위를 넘는 예약은 만들지 않는다.

보내기 직전에 다음을 다시 확인한다.

1. 설정이 켜져 있고 `terminal.state`가 idle이다.
2. 전경 이름이 Claude이고, 예약 때 PID를 얻었다면 지금도 같은 PID다.
   이름은 대소문자를 무시하고 `.exe` 접미사를 제거한다.
3. 실패한 턴 시작 이후 사용자 입력이 없었다. 턴 시작을 모르면 예약 시각부터 확인한다.
4. 연속 시도 수가 상한보다 작고, 예약 후 새 턴으로 바뀌지 않았다.

입력 확인은 `surface.is_typing`의 `idle_seconds`를 사용한다. 키보드·IME·붙여넣기
입력을 포함하고 에이전트의 send/tell은 포함하지 않는다. 초안을 썼다가 지운 경우도
취소한다. 조회에 실패해도 보내지 않는다. 이 조건을 통과했지만 지금 `typing`이면
5초 미루며 시도 수를 쓰지 않는다.

문구는 현재 언어의 `claude.auto_resume.message`로 고정한다. `terminal.tell`로 본문 쓰기를
확인한 뒤 잠시 기다리고 Enter를 보낸다. attach 점유 등으로 거절되면 다시 예약하지 않는다.
본문과 Enter 사이에 사용자가 입력할 가능성까지 없애는 원자적 제출은 아니다.

상한에 닿으면 보내지 않고 `notification.create`로 한 번 알린다.
성공 턴과 새 세션은 계수를 초기화하지만 자동 재개 자신의 `prompt-submit`은 초기화하지 않는다.
전송 수는 `claude-auto-resume-count` meta와 로그에 남기며 성공·새 세션·세션 종료 때 meta를 지운다.
부모에게는 실패 턴의 완료 로그가 이미 전달되므로 별도 자동 재개 로그를 더 쓰지 않는다.
그 완료 문구의 '입력 대기' 표현은 재개가 예약됐어도 같으며 실제 성공을 뜻하지 않는다.

### PTY 에러 스캔 (`claude-error`) 범위

`error_scan.rs`는 추적 중인 surface의 `surface.read_since_scan_mark` 결과를 읽고 알려진 오류 패턴을 찾는다. 패턴은 `API Error`, `Output blocked by content filtering policy`, `overloaded_error`, `rate_limit_error`, `Internal Server Error`, `network error`, `Bad Request`다. 매칭되면 `claude-error`를 발생시킨다. 같은 오류 텍스트의 반복 알림은 생략하며 새 턴 신호(`prompt-submit`/`session-start`/`active`)에서 이 기록을 초기화한다.

회차 사이에는 800ms를 기다린다. 파일·IPC·잠금 대기 시간은 별도이므로 800ms 안에 감지한다는 보장은 아니다.

`claude launch`의 top-level surface와 `claude spawn`/`claude respawn`의 자식 surface를 모두 추적한다.

추적을 계속할지는 등록 경로에 따라 아래와 같이 판단한다.

| 대상 | 등록 | 추적 유지 판단 |
|------|------|-----------|
| top-level (`launch`) | child registry에 없음 | `surface.locate`를 조회한다. 대상 부재를 포함해 조회 오류가 나도 추적을 유지한다 |
| 자식 (`spawn`/`respawn`) | 호스트 child registry | `terminal.parent`로 부모·자식 관계를 확인한다. 관계 없음은 추적을 해제하고 그 밖의 조회 오류는 유지한다 |

`terminal.release`는 surface를 남기고 부모·자식 관계와 soft 점유만 해제하므로 자식은 surface 존재만으로 판단하지 않는다. `claude kill`이 성공 응답의 `killed_surface_id`를 받으면 해당 추적을 즉시 해제한다. 일반 조회 오류로 추적을 지우면 다시 등록할 기회가 없어 오류 시에는 유지하는 쪽을 택한다.

`surface.read_since_scan_mark`는 에이전트의 `tasty set mark`·`read since-mark`·`parse-since-mark`와 별도의 커서를 사용한다. 읽을 때마다 해당 커서만 전진하며, 다음 호출에는 그 이후 출력이 온다.

패턴 비교를 위해 플러그인이 읽은 출력을 크기 제한 안에서 누적한다. 이 커서는 스캐너 하나만 소비한다는 전제다. 다른 소비자가 같은 API를 호출하면 한쪽이 읽은 출력을 다른 쪽이 놓칠 수 있어 CLI로 제공하지 않는다. [터미널 입출력 설계](../../adr/0013-terminal-io-and-process-lifetime.md)를 참고한다.

### 정지 알림 (`claude-error-stalled` → 부모 completion-log)

`claude-error`는 부모 알림에 직접 연결하지 않는다. 자동 재시도 중에도 오류 문구가 나올 수 있기 때문이다. 일정 기간 누적 출력이 바뀌지 않으면 정지를 의심해 `claude-error-stalled`를 발생시키며, 부모 알림은 이 이벤트를 구독한다. 재시도와 실제 멈춤을 확정적으로 구분하는 검사는 아니다.

이벤트 이름에 `error`가 있지만 오류 문구가 없는 장시간 정적 상태와 호스트가 `stale`로 분류한 자식도 대상이다. 기존 등록과 호환되도록 키를 유지한다. 알림 직전 화면에서 오류 문구를 찾으면 이를 힌트로 붙이고, 없으면 출력과 완료 신호가 없다는 안내를 보낸다([에이전트 상태 설계](../../adr/0041-agent-state-and-completion.md)).

판정 기준은 두 조건의 **동시** 충족이다(`error_scan.rs`):

| 조건 | 왜 |
|---|---|
| 누적 출력의 해시가 기준 시간 동안 같음 | 오류 문구가 있으면 30초, 없으면 120초. 비교 대상은 스캐너가 보관한 크기 제한 안의 누적 텍스트이며 화면 전체나 프로세스 진행 상태가 아니다. 오류 없는 120초 기준은 호스트의 `CHILD_OUTPUT_SILENCE`와 맞춘다 |
| `terminal.state`가 `active` 또는 `stale` | `idle`·`needs_input`·`exited`는 별도 상태 훅이 다루므로 제외한다. 그 훅이나 알림이 실제 전달됐음을 확인하는 조건은 아니다. `stale`은 `confidence`와 무관하게 포함한다 |

같은 정적 구간에서는 한 번만 알리고 surface별로 최소 5분 간격을 둔다. 출력이 바뀌면 정적 구간을 다시 측정한다. 새 턴 신호도 중복 기록과 정적 측정을 초기화하지만 5분 쿨다운은 유지한다. 긴 추론이나 입력 대기와 실제 멈춤은 구분하지 못할 수 있다.

`StopFailure`가 상태를 idle로 바꾸면 이 정지 알림의 조건에서 빠진다. 대신 `claude-idle` 훅으로 실행된 `notify-done`이 API 오류 종류를 붙여 로그를 쓰는 경로를 사용한다. 훅 호출과 로그 쓰기가 실패하지 않았다는 보장까지 뜻하지는 않는다.

정지 스캐너는 `terminal.set_state`를 호출하지 않는다. 관측 결과로 알림만 만들며 `claude children`의 상태를 직접 바꾸지 않는다.

`register_notify_hooks`는 상태 변경용 once 훅 세 개와 별도로 `claude-error-stalled` 상시 훅을 등록한다. 명령은 `tasty claude notify-error --caller-surface … --target-surface …`이며, `notify-done` 형제 훅과 명령 문자열이 달라 그 정리 대상에 포함되지 않는다.

같은 caller·target의 정지 알림을 다시 등록할 때는 같은 명령 문자열로 등록된 기존 훅을 정리한다.

`notify-error`는 `surface.screen_text`에서 오류 문구를 찾아 안내를 조립한 뒤 `<parent_home>/notify/<caller_surface>.log`에 직접 추가한다. 다른 완료·상태 알림과 같은 [로그 경로](../../dev-guide/external-interaction.md#child-완료-알림--completion-log)를 사용하며 화면 조회나 기록은 실패할 수 있다.

## 인터페이스

- **AI Agent / 사용자**: `tasty claude launch|spawn|tell|broadcast|kill|respawn|children|parent|hook|checklist-hook|checklist-enable|checklist-disable|checklist-status|profile-register|profile-unregister|profile-list|profile-show|profile-current|gate-register|gate-unregister|gate-list|gate-show …`.
- surface/페인 생성 자체는 [work-area](../../features/work-area/index.md) 도메인을 사용.

## 비-목표

- Claude Code 자체 기능 — 외부 CLI. 이 플러그인은 *실행·배치·관계 관리*.
- 터미널/PTY 내부 — host.

## Acceptance Criteria

- Given 플러그인 활성 When `tasty claude launch` Then 새 워크스페이스에서 Claude 가 실행된다.
- Given 부모 인스턴스 When `tasty claude spawn` Then 자식 인스턴스가 페인 분할로 생성되고 `children` 에 보인다.
- Given 상태 훅과 로그 기록이 정상 동작하는 자식 When `tasty claude spawn` 또는 `tell` 뒤 idle/needs_input/process-exit 훅이 발생 Then caller의 completion-log에 상태 변경을 기록하고 같은 명령의 형제 훅을 정리한다. `surface.locate`가 성공하면 세 훅을 다시 등록한다.
- Given `~/.claude/settings.json`에 사용자가 직접 추가한 hook entry가 있음 When `tasty claude install` 실행 Then 9개 tasty hook entry가 추가/갱신되고 사용자 entry는 그대로 보존된다.
- Given 유효한 프로필 JSON When `tasty claude reboot --profile-file <경로>` Then 재시작된 Claude 에서 프로필 훅과 tasty 내장 훅이 함께 실행되고, 무인자로 다시 reboot 해도 프로필이 승계된다. `--clear-profile` 후 reboot 하면 프로필 훅이 더 이상 발생하지 않는다. 존재하지 않는 경로/깨진 JSON 은 kill 시퀀스를 시작하지 않고 즉시 에러를 반환한다.
- Given 등록된 프로필 둘(각각 다른 마커를 남기는 `SessionStart` 훅) When 이름 둘을 쉼표로 `--profile` 에 함께 부착해 spawn Then **둘 다** 발생한다(머지가 last-wins 로 떨어지지 않는다). `permissions.deny`를 담은 프로필을 부착하면 그 자식에게서 해당 도구가 사라지고(거부 프롬프트가 아니라 툴셋에서 빠짐), `deny` 프로필과 그 도구를 `allow` 하는 프로필을 함께 부착해도 도구는 여전히 없다(deny를 allow보다 우선한다). `--profile-file` 과 `--profile` 을 함께 주면 즉시 에러.
- Given `--profile continue-checklist` 로 부착한 세션 + 마커 파일 존재 When Claude 가 센티넬 없이 응답을 끝내려 함 Then block 되고 체크리스트 본문이 주입되며, 센티넬을 포함해 응답하거나 라운드 상한에 도달하면 정상 종료된다. 프로필을 부착하지 않은 세션은 이 동작에 전혀 영향받지 않는다. 같은 프로필을 부착한 세션 둘을 동시에 진행해도 라운드 카운터가 서로 섞이지 않는다.
- Given 마커 파일 부재 When `tasty claude checklist-enable` Then `continue-checklist` 게이트의 마커 파일이 생성되고 `checklist-status` 가 `enabled: true` 를 보고한다. When `tasty claude checklist-disable` Then 마커 파일이 삭제되고 `checklist-status` 가 `enabled: false` 를 보고한다 — 마커가 이미 없는 상태에서 다시 `checklist-disable` 을 호출해도 에러 없이 `enabled: false` 를 반환한다(멱등).
- Given 등록 게이트 둘(`gate-a`/`gate-b`) When `checklist-enable --gate gate-a` Then `checklist-status --gate gate-a` 는 `enabled: true`, `--gate gate-b` 는 `enabled: false` 이고 `gate-list` 가 두 상태를 함께 보여준다. 그 세션에서 `gate-a` 훅만 block 을 걸고 `gate-b` 훅은 조용히 통과한다. 미등록 이름으로 `checklist-enable --gate nope` 를 호출하면 에러이고 마커 디렉토리도 생기지 않으며, `checklist-status --gate nope` 는 에러 없이 `enabled: false, registered: false` 를 돌려준다.
- Given 켜 두고 라운드가 쌓인 게이트 When `gate-unregister <이름>` 후 같은 이름으로 다시 `gate-register` Then `checklist-status` 가 `enabled: false` 이고 라운드 카운터도 1 부터 시작한다(이전 인스턴스의 상태를 물려받지 않는다).
- Given 게이트별로 나누기 전 경로(`checklist/enabled.marker`)에만 마커가 있는 인스턴스 When 아무 진입점(`checklist-status` 또는 훅 실행) Then 마커가 `gates/continue-checklist/enabled.marker` 로 옮겨지고 legacy 파일은 사라지며, 켜져 있던 상태가 그대로 보존된다.
- Given `gate-register mygate --sentinel '[[MY-DONE]]' --rounds 2` 로 등록하고 `checklist-enable --gate mygate` 로 켜 둔 게이트 When `spawn --profile mygate` Then 그 자식의 `Stop` 훅으로 `checklist-hook --gate mygate` 가 걸리고, 센티넬 없이 턴을 끝내려 하면 **mygate 의 본문**(host 기본 체크리스트 본문이 아니라)이 주입되며 block 된다. `[[MY-DONE]]` 을 포함해 답하면 통과하고, 포함하지 않은 채 계속하면 **mygate 의 상한 2** 에서 백스톱으로 통과한다. 라운드는 `checklist/gates/mygate/rounds/<session>.json` 에 쌓이고 그 세션이 끝나면 정리된다.
- Given 등록 게이트 `mygate` 와 그것을 `reboot --profile mygate` 로 부착해 둔 자식(이름 meta 를 기록하는 기동 경로는 `reboot` 뿐이라 `spawn` 으로 띄운 자식은 훅이 걸려 있어도 `attached_names` 가 `null` 이다) When `profile-list` / `profile-show mygate` / `profile-current --surface <자식>` Then 목록에 `user/mygate`(attachable, 게이트 설명)와 `host/continue-checklist` 가 함께 보이고, show 는 owner `user` 와 생성된 Stop 훅 JSON 을 돌려주며, current 의 `attached_names` 에 `mygate` 가 있다.
- Given 게이트 둘 When `--profile mygate,continue-checklist` 로 부착 Then `generated/<정렬된이름>.json` 의 `hooks.Stop` 항목이 2개이고 각각 `--gate mygate` / `--gate continue-checklist` 를 지목한다.
