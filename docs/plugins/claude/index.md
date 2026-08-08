# Claude Code (`com.tasty.claude`)

- **Status**: Implemented (bundled plugin)
- **주체**: AI Agent / 로컬 사용자 (`tasty claude` CLI · IPC)
- **배포/통합**: bundled · cli + ipc_namespace + 멀티에이전트 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-claude/`
- **권한**: `terminal.spawn` 등 (매니페스트 `permissions`)
- **화면**: 없음 — CLI/IPC 로 터미널 surface 를 조작하는 오케스트레이션 플러그인 (headless).
- **플로우**: 멀티에이전트 오케스트레이션 다이어그램 (spawn·tell·wait·hook·상태머신) — [Figma · Flows & IA](https://www.figma.com/design/ct3uPefwY2uk6i1i9wYpkU/Untitled?node-id=33-915).

> **예제로서**: **최대 통합 레퍼런스**(~3.5k줄) — cli + ipc namespace + 멀티에이전트 + **훅** + event_subscribe + 외부 설치. state/handlers/install/hook/error_scan 모듈 분리의 본보기 → [plugin-development](../../dev-guide/plugin-development.md#cli--ipc-namespace).

## 목적

**Claude Code CLI 를 tasty 안에서 실행·오케스트레이션**하는 통합. 새 워크스페이스/패인에 Claude 인스턴스를 띄우고, 부모-자식 관계로 여러 인스턴스를 spawn·제어한다 (멀티에이전트).

## 내부 동작

- **cli `claude`** (`tasty claude …`) — 서브커맨드: `launch`(새 워크스페이스에서 실행) · `spawn`(자식 인스턴스, 패인 분할) · `children`/`parent`(관계 조회) · `tell`/`broadcast`(메시지 전송) · `kill`/`respawn` · `reboot`(같은 세션 resume 재시작, 아래) · `hook`(Claude Code 훅 통합, 아래 "Claude Code 훅 통합" 절) · `notify-done`(내부용: spawn/tell 상태 전환 시 caller 에게 알림 전달 + 형제 hook 정리·재무장, 아래).
- `spawn`/`tell`은 **동기 블록 없이 즉시 반환**한다. 대상(child 또는 tell 대상 surface)이 idle/needs_input 에 도달할 때마다, 그리고 최종적으로 exited 에 도달했을 때 caller surface(spawn/tell을 호출한 surface)에 완료 메시지가 자동으로 주입된다 — `claude-idle`/`needs-input`/`process-exit` 3개의 once(1회성) surface hook을 등록해 구현하며, 그중 하나가 fire되면 `notify-done`이 알림 전송 + 나머지 형제 hook 정리 후, target surface 가 아직 살아있으면(=이번 fire 가 process-exit 가 아니었으면) `surface.locate` 로 확인해 3개 hook 을 다시 등록한다(자기재무장). 이 덕분에 needs-input(되묻기) 같은 일시적 상태 전환을 거쳐도 그 뒤 진짜 완료 시 알림을 놓치지 않는다 — "spawn/tell 당 알림 1회"가 아니라 "child 가 살아있는 동안 상태 전환마다 알림"이다.
- **ipc_namespace `claude`** — 위 동작의 IPC 표면.
- **event_subscribe** `surface.closed` — surface 종료를 받아 인스턴스 상태 정리.
- 실제 Claude 프로세스는 터미널 surface 안에서 돌고(`terminal.spawn`), 플러그인은 그 생명주기·관계를 관리한다.
- **`reboot`** (`tasty claude reboot [--surface <id>] [--delay <초>] [--prompt <추가문구>] [--profile-file <경로>] [--clear-profile]`) — surface 안의 Claude 를 종료하고 **같은 세션으로 재시작**한다. Claude 는 스스로 자기 TUI 를 껐다 켤 수 없으므로 에이전트가 이 명령을 자기 surface 에 호출한다(설정/훅/버전 변경 반영용). 동작: 즉시 응답 반환 → `--delay`(기본 5s) 후 Ctrl+C ×4(0.5s 간격) → 전경 프로세스가 Claude 에서 이탈했는지 확인 후 셸에 `claude -r <session_id>`(`--profile-file` 있으면 뒤에 `--settings "<경로>"` 추가) 전송(session id 는 요청 시점에 surface meta `claude-session-id` 에서 캡처) → Claude 복귀 확인 후 재시작 안내 프롬프트를 `terminal.tell` 로 제출(화면 검증·재시도 + 별도 Enter 로 결정적 제출). 안전 가드: 전경이 여전히 Claude 면 텍스트 미전송·중단, resume 후 미복귀면 안내 미전송(셸 오염 방지), 같은 surface 중복 reboot 거부. **턴의 마지막 행동으로 호출할 것** — delay 이후 진행 중이던 턴은 잘린다.
  - **`claude-session-id` meta 가 비어 reboot 가 실패하는 경우**: `no active claude session on surface {id} (claude-session-id meta not set …)` 에러는 hook 미설치가 아니어도 발생할 수 있다 — session-start hook 이 이 meta 를 못 심은 것이 원인. 조용히 실패할 수 있는 지점이 최소 3곳: ① `install.rs`의 등록 커맨드가 `[ -n "$TASTY_SURFACE_ID" ] && tasty claude hook … || true` 라 `TASTY_SURFACE_ID` 미설정 시 tasty 바이너리 자체가 실행되지 않음(로그 불가), ② `hook.rs`의 `apply_hook` session-start 분기가 stdin JSON 에 `session_id` 가 없으면 meta 기록을 건너뜀(`tracing::warn!`으로 로그, `tasty plugin logs com.tasty.claude --follow` 또는 `~/.tasty/plugins-logs/com.tasty.claude.log` 에서 확인), ③ `dynamic.rs`의 `read_stdin_json` 이 TTY/파싱 실패로 `None` 을 반환(release stderr + debug 빌드는 `~/.tasty/debug-dev.log`). 수동 복구: `tasty surface-meta set --key claude-session-id --value <세션ID>`.
- **Claude 세션 프로필**(용어 정의: [ubiquitous-language.md](../../concepts/ubiquitous-language.md)) — Claude Code 는 훅을 프로세스 기동 시 한 번만 읽으므로, 살아있는 세션에 훅을 추가하는 유일한 창구가 `reboot`/`spawn`/`respawn`/`launch` 4개 기동 경로다. `--profile-file <경로>`(`path_kind = "file"`, CLI 가 호출자 cwd 기준 절대경로로 정규화)를 주면 기동 명령에 `--settings "<경로>"` 가 붙는다 — Claude Code 의 `--settings` 는 `~/.claude/settings.json` 의 기존 훅을 **대체가 아니라 병합**하므로 tasty 내장 훅(`claude hook` 경유)도 그대로 발화한다. `reboot` 만 부착한 경로를 surface meta `claude-session-profile` 에 기록해 **다음 무인자 reboot 가 기본값으로 승계**하며(파일 존재 + JSON 파싱을 매 reboot 마다 동기 재검증 — 승계된 경로가 깨져 있으면 kill 시퀀스를 시작하지 않고 에러 반환), `--clear-profile` 로 뗀다. `spawn`/`respawn`/`launch` 는 그 호출 1회의 기동 명령에만 반영하고 meta 를 건드리지 않는다(반복 재기동은 `reboot` 만의 개념).
- spawn 시 parent 의 살아있는 child 수가 설정 임계치를 넘으면 응답에 `warning` 필드가 실린다 — Settings › Plugin › Claude Code 에서 임계치 조정.
- **승인 정책 플래그 없음(미확인 상태)** — [codex](../codex/index.md) 플러그인은 `--approval`/`--sandbox`/`--full-auto` 로 자식의 승인/샌드박스 정책을 지정할 수 있지만(비대화형 자동화 흐름에서 승인 프롬프트가 자식을 영구히 멈추는 문제의 해결책), 이 플러그인의 `build_launch_command`(`crates/tasty-plugin-claude/src/handlers.rs`)에는 대응하는 플래그가 없다. Claude Code 는 codex 처럼 기동 시점 CLI 플래그가 아니라 `settings.json`(`permissions`)/`--permission-mode` 기반 권한 모델을 쓰므로 구조가 다르지만, `permissions.defaultMode` 가 승인이 필요한 값일 때 `spawn`/`launch`/`respawn` 으로 띄운 자식이 codex 와 동형으로 승인 프롬프트에서 영구히 멈추는지는 아직 재현·확인되지 않았다. 재현되면 codex 와 동형의 정책 플래그 노출이 필요하다.

### Claude Code 훅 통합

`tasty claude install`이 `~/.claude/settings.json`의 `hooks`에 아래 6개 이벤트를 심는다. 모든 이벤트가 같은 형태의 명령 문자열을 쓴다:

```
[ -n "$TASTY_SURFACE_ID" ] && tasty claude hook <token> || true
```

`session_id`/`message` 같은 이벤트별 가변 데이터는 명령 인자가 아니라 **stdin JSON**으로 들어온다 — 매니페스트 `hook` cli 항목이 `stdin_json = true`를 선언하고, `--session`/`--message` 플래그가 각각 `stdin_field = "session_id"`/`"message"`로 stdin JSON에서 자동 채워진다(Claude Code가 hook 실행 시 stdin으로 JSON payload를 준다). POSIX 셸 구문 1종만 발행한다 — [codex](../codex/index.md)처럼 Windows PowerShell 분기는 없다.

| Claude Code 이벤트 | tasty hook token | `terminal.set_state` | `surface.fire_hook` | surface meta | 기타 |
|---|---|---|---|---|---|
| `Stop` / `SubagentStop` | `stop` / `subagent-stop` | `idle` | `claude-idle` | — | `surface.completion` |
| `SessionEnd` | `session-end` | `idle` | `claude-idle` | `claude-session-id`·`restore.command` **unset** | `surface.completion` |
| `Notification` | `notification` | `needs_input` | `needs-input` | — | `surface.completion` |
| `UserPromptSubmit` | `prompt-submit` | `active` | — | — | — |
| `SessionStart` | `session-start` | `active` | — | `claude-session-id` = 세션 ID, `restore.command` = `claude -r <id>` **set**(stdin JSON에 `session_id`가 없으면 건너뜀) | — |

`UserPromptSubmit`은 child가 2번째 이후 prompt를 받을 때 직전 `Stop` hook이 남긴 `idle=true` 잔재를 지우는 데 필수다 — 미등록 시 실제로는 active인 child를 idle로 오보고하는 상태 버그가 생긴다. hook은 **event 이름만** 받고 툴 이름/`tool_input`은 보지 않으므로, "어떤 툴 때문에 멈췄는지"는 구분하지 않는다 — `needs_input`은 `Notification` 하나에서만 나온다.

**설치 대상은 이 6개뿐이다** — `PreToolUse`/`PostToolUse` 같은 툴 단위 이벤트는 걸지 않는다(claude install 코드/테스트 어디에도 `PreToolUse`/`PostToolUse`를 걸지 않는다 — `install.rs`의 `install_preserves_other_hooks` 테스트가 사용자가 직접 추가한 `PreToolUse` entry를 건드리지 않고 그대로 보존함을 검증한다).

install은 marker substring(`tasty claude hook <token>`)으로 자기 entry를 식별해 멱등하게 동작한다 — marker가 일치하는 기존 entry는 명령 문자열만 최신 형태로 덮어쓰고(옛 버전이 심은 잘못된 명령이 남는 회귀 방지), 사용자가 직접 추가한 다른 entry는 건드리지 않는다.

이 플러그인이 fire하는 surface hook 이벤트는 `claude-idle`/`needs-input`/`claude-error` 3개이며, 매니페스트 `contributes.hook_events`로 선언한다 — host가 (내장 ∪ 활성 plugin 선언) 집합으로 `hook.set` 등록을 검증하므로([hooks](../../features/hooks/index.md)), 이 플러그인이 비활성이면 저 3개 키로의 hook 등록도 거부된다. **이 3개가 전부 위 6개 설치 훅에서 나오는 건 아니다** — `claude-idle`/`needs-input`은 위 `apply_hook`(Stop/SessionEnd → `claude-idle`, Notification → `needs-input`)에서 나오지만, `claude-error`는 이 훅 메커니즘과 무관한 별도 producer다: `error_scan.rs`가 surface 출력 텍스트를 패턴 매칭해 매치 시 직접 `surface.fire_hook`으로 발사한다. `claude-idle`/`needs-input`은 [surface-highlight](../../features/surface-highlight/index.md)(Stop hook → highlight)와 [telemetry](../../features/telemetry/index.md)(`session-start`→`stop`의 `wall_time_ms`, `notification`의 `input_tokens`)가 소비하고, `SessionStart`/`SessionEnd`의 meta set/unset은 [layout-persistence](../../features/layout-persistence/index.md)의 `restore.command` 복원이 소비한다.

## 인터페이스

- **AI Agent / 사용자**: `tasty claude launch|spawn|tell|broadcast|kill|respawn|children|parent|hook …`.
- surface/패인 생성 자체는 [work-area](../../features/work-area/index.md) 도메인을 사용.

## 비-목표

- Claude Code 자체 기능 — 외부 CLI. 이 플러그인은 *실행·배치·관계 관리*.
- 터미널/PTY 내부 — host.

## Acceptance Criteria

- [ ] Given 플러그인 활성 When `tasty claude launch` Then 새 워크스페이스에서 Claude 가 실행된다.
- [ ] Given 부모 인스턴스 When `tasty claude spawn` Then 자식 인스턴스가 패인 분할로 생성되고 `children` 에 보인다.
- [ ] Given 자식 When `tasty claude spawn`(또는 `tell`) 후 자식이 idle/needs_input/exited 에 도달 Then caller surface 에 완료 알림이 주입되고 형제 hook 이 함께 정리된다. 자식이 exited 가 아닌 상태(idle/needs_input)로 도달한 경우엔 형제 hook 이 재등록돼 그 뒤 상태 전환에도 계속 알림이 온다.
- [ ] Given `~/.claude/settings.json`에 사용자가 직접 추가한 hook entry가 있음 When `tasty claude install` 실행 Then 6개 tasty hook entry가 추가/갱신되고 사용자 entry는 그대로 보존된다.
- [ ] Given 유효한 프로필 JSON When `tasty claude reboot --profile-file <경로>` Then 재시작된 Claude 에서 프로필 훅과 tasty 내장 훅이 함께 발화하고, 무인자로 다시 reboot 해도 프로필이 승계된다. `--clear-profile` 후 reboot 하면 프로필 훅이 더 이상 발화하지 않는다. 존재하지 않는 경로/깨진 JSON 은 kill 시퀀스를 시작하지 않고 즉시 에러를 반환한다.
</content>
