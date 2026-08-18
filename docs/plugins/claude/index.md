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

- **cli `claude`** (`tasty claude …`) — 서브커맨드: `launch`(새 워크스페이스에서 실행) · `spawn`(자식 인스턴스, 패인 분할) · `children`/`parent`(관계 조회) · `tell`/`broadcast`(메시지 전송) · `kill`/`respawn` · `reboot`(같은 세션 resume 재시작, 아래) · `hook`(Claude Code 훅 통합, 아래 "Claude Code 훅 통합" 절) · `checklist-hook`(`continue-checklist` 세션 프로필 전용 `Stop` 훅, 아래 "continue-checklist 세션 프로필" 절) · `checklist-enable`/`checklist-disable`/`checklist-status`(그 게이트 마커 파일을 켜고 끄고 조회, 같은 절) · `notify-done`(내부용: spawn/tell 상태 전환 시 caller 에게 알림 전달 + 형제 hook 정리·재무장, 아래) · `profile-register`/`profile-unregister`/`profile-list`/`profile-show`/`profile-current`(Claude 세션 프로필 레지스트리, 아래 "Claude 세션 프로필 레지스트리" 절).
- `spawn`/`tell`은 **동기 블록 없이 즉시 반환**한다. 대상(child 또는 tell 대상 surface)이 idle/needs_input 에 도달할 때마다, 그리고 최종적으로 exited 에 도달했을 때 caller surface(spawn/tell을 호출한 surface)에 완료 메시지가 자동으로 주입된다 — `claude-idle`/`needs-input`/`process-exit` 3개의 once(1회성) surface hook을 등록해 구현하며, 그중 하나가 fire되면 `notify-done`이 알림 전송 + 나머지 형제 hook 정리 후, target surface 가 아직 살아있으면(=이번 fire 가 process-exit 가 아니었으면) `surface.locate` 로 확인해 3개 hook 을 다시 등록한다(자기재무장). 이 덕분에 needs-input(되묻기) 같은 일시적 상태 전환을 거쳐도 그 뒤 진짜 완료 시 알림을 놓치지 않는다 — "spawn/tell 당 알림 1회"가 아니라 "child 가 살아있는 동안 상태 전환마다 알림"이다.
- **ipc_namespace `claude`** — 위 동작의 IPC 표면.
- **event_subscribe** `surface.closed` — surface 종료를 받아 인스턴스 상태 정리.
- 실제 Claude 프로세스는 터미널 surface 안에서 돌고(`terminal.spawn`), 플러그인은 그 생명주기·관계를 관리한다.
- **`reboot`** (`tasty claude reboot [--surface <id>] [--delay <초>] [--prompt <추가문구>] [--profile-file <경로> | --profile <이름[,이름2,...]>] [--clear-profile]`) — surface 안의 Claude 를 종료하고 **같은 세션으로 재시작**한다. Claude 는 스스로 자기 TUI 를 껐다 켤 수 없으므로 에이전트가 이 명령을 자기 surface 에 호출한다(설정/훅/버전 변경 반영용). 동작: 즉시 응답 반환 → `--delay`(기본 5s) 후 Ctrl+C ×4(0.5s 간격) → 전경 프로세스가 Claude 에서 이탈했는지 확인 후 셸에 `claude -r <session_id>`(프로필이 해석되면 뒤에 `--settings "<경로>"` 추가) 전송(session id 는 요청 시점에 surface meta `claude-session-id` 에서 캡처) → Claude 복귀 확인 후 재시작 안내 프롬프트를 `terminal.tell` 로 제출(화면 검증·재시도 + 별도 Enter 로 결정적 제출). 안전 가드: 전경이 여전히 Claude 면 텍스트 미전송·중단, resume 후 미복귀면 안내 미전송(셸 오염 방지), 같은 surface 중복 reboot 거부. **턴의 마지막 행동으로 호출할 것** — delay 이후 진행 중이던 턴은 잘린다.
  - **`claude-session-id` meta 가 비어 reboot 가 실패하는 경우**: `no active claude session on surface {id} (claude-session-id meta not set …)` 에러는 hook 미설치가 아니어도 발생할 수 있다 — session-start hook 이 이 meta 를 못 심은 것이 원인. 조용히 실패할 수 있는 지점이 최소 3곳: ① `install.rs`의 등록 커맨드가 `[ -n "$TASTY_SURFACE_ID" ] && tasty claude hook … || true` 라 `TASTY_SURFACE_ID` 미설정 시 tasty 바이너리 자체가 실행되지 않음(로그 불가), ② `hook.rs`의 `apply_hook` session-start 분기가 stdin JSON 에 `session_id` 가 없으면 meta 기록을 건너뜀(`tracing::warn!`으로 로그, `tasty plugin logs com.tasty.claude --follow` 또는 `~/.tasty/plugins-logs/com.tasty.claude.log` 에서 확인), ③ `dynamic.rs`의 `read_stdin_json` 이 TTY/파싱 실패로 `None` 을 반환(release stderr + debug 빌드는 `~/.tasty/debug-dev.log`). 수동 복구: `tasty surface-meta set --key claude-session-id --value <세션ID>`.
- **Claude 세션 프로필**(용어 정의: [ubiquitous-language.md](../../concepts/ubiquitous-language.md)) — Claude Code 는 훅을 프로세스 기동 시 한 번만 읽으므로, 살아있는 세션에 훅을 추가하는 유일한 창구가 `reboot`/`spawn`/`respawn`/`launch` 4개 기동 경로다. 프로필을 붙이는 방법은 두 가지고 **상호 배타적**이다(둘 다 주면 즉시 에러):
  - `--profile-file <경로>`(`path_kind = "file"`, CLI 가 호출자 cwd 기준 절대경로로 정규화, **반복 지정 거부** — 아래 "왜 반복 지정을 CLI 가 거부하는가") — 파일 경로를 그대로 쓴다(TODO 31).
  - `--profile <이름[,이름2,...]>` — 아래 "Claude 세션 프로필 레지스트리"에 등록해 둔 이름으로 부착한다. 이름을 둘 이상 쉼표로 주면 레지스트리가 머지해 만든 파일 하나를 쓴다.

  어느 쪽이든 최종적으로 기동 명령에 `--settings "<경로>"` 가 붙는다 — Claude Code 의 `--settings` 는 `~/.claude/settings.json` 의 기존 훅을 **대체가 아니라 병합**하므로 tasty 내장 훅(`claude hook` 경유)도 그대로 발화한다. `reboot` 만 부착 상태를 surface meta 에 기록해 **다음 무인자 reboot 가 기본값으로 승계**한다 — 경로로 부착하면 `claude-session-profile`(경로 그대로), 이름으로 부착하면 `claude-session-profile-names`(이름 문자열)에 기록되고 두 meta 는 상호 배타적으로 관리된다(한쪽을 새로 쓰면 다른 쪽은 지운다). 이름-meta 는 **승계 시점마다 레지스트리에서 다시 해석**한다 — 경로를 캐시하지 않으므로 그 사이 `profile-register` 로 내용이 갱신됐다면 다음 reboot 에 최신 내용이 반영된다. 두 meta 모두 파일 존재 + JSON 파싱을 매 reboot 마다 동기 재검증한다(승계된 프로필이 깨져 있으면 kill 시퀀스를 시작하지 않고 에러 반환). `--clear-profile` 로 둘 다 뗀다. `spawn`/`respawn`/`launch` 는 그 호출 1회의 기동 명령에만 반영하고 meta 를 건드리지 않는다(반복 재기동은 `reboot` 만의 개념).
  - **왜 반복 지정을 CLI 가 거부하는가**: Claude Code 의 `--settings` 는 반복 지정 시 **마지막 값만 남고 앞선 값이 조용히 사라진다**(실측, TODO 31/32). tasty CLI 인자 자체(`--profile-file`)를 실수로 두 번 주는 경우도 같은 함정에 빠질 수 있어, 매니페스트 `CliArg.reject_repeat = true`(`crates/tasty-cli/src/dynamic.rs`)로 clap 을 `ArgAction::Append` 로 등록해 두 번째 값이 들어오면 조용히 버리지 않고 에러로 거부한다.

### Claude 세션 프로필 레지스트리

프로필 파일을 매번 손으로 만들고 경로를 외우는 대신, **이름으로 등록해 두고** 위 `--profile <이름>` 으로 부착하는 계층. `src/hook_handler/registry.rs` 의 형태(patch semantics · `<owner>/<short>` id)를 미러링하되 타입은 공유하지 않는다 — 소비자가 이 플러그인 하나뿐이라 호스트 레지스트리를 신설하지 않고 plugin 내부(`crates/tasty-plugin-claude/src/profile.rs`)에 둔다.

- **등록**: `tasty claude profile-register <이름> --file <경로>` — `<경로>`(JSON object) 를 읽어 `TASTY_PLUGIN_DATA_DIR/profiles/registered/<이름>.json` 에 **복사본**으로 저장한다(원본이 나중에 옮겨지거나 지워져도 레지스트리는 영향받지 않는다). 이미 등록된 이름이면 내용을 덮어쓴다. 이름은 소문자/숫자/`-`, 최대 32자.
- **해제**: `tasty claude profile-unregister <이름>`.
- **목록**: `tasty claude profile-list` — 등록된 프로필(`user/<이름>`, attachable) 과 항상 전역 설치돼 있는 내장 훅 8종(`host/<token>`, attachable 아님 — 위 "Claude Code 훅 통합" 절의 `install.rs::MANAGED_HOOKS` 를 그대로 나열, 정의를 복제하지 않는다)을 함께 보여준다.
- **조회**: `tasty claude profile-show <이름>` — 등록된 원본 JSON 그대로. `tasty claude profile-current [--surface <id>]` — 그 surface 에 지금 부착된 프로필(이름 또는 경로)과 내장 훅 목록을 함께 보여준다("지금 이 세션에 무슨 프로필이 걸려 있나").
- **조합 머지**(`crates/tasty-plugin-claude/src/profile_merge.rs`) — `--settings` 는 슬롯이 하나뿐이라(위 실측) 이름을 둘 이상 주면 등록된 각 파일을 순서대로 접어 하나의 JSON 으로 만들고 `TASTY_PLUGIN_DATA_DIR/profiles/generated/<정렬된-이름들>.json` 에 실체화한다(등록 원본과 별도 하위 디렉토리 — 재생성되는 산출물이 원본을 덮어쓰지 않도록). 매 attach 시점마다 다시 만들어 항상 최신 등록 내용을 반영한다. 키 유형별 규칙:

  | 키 유형 | 예 | 규칙 |
  |---|---|---|
  | 훅 이벤트 배열 | `hooks.Stop` | union(중복 command 문자열 제거) — 사실상 concat, 둘 다 발화 |
  | 객체 맵 | `env`, `enabledPlugins` | 키 단위 재귀 병합. 리프 값 충돌은 스칼라 규칙과 동일 |
  | 허용/거부 리스트 | `permissions.allow`/`deny` | union 후 **불변식 강제**: `deny` 에 있는 항목은 `allow` 에서 제거한다 — 프로필 조합 순서와 무관하게 deny 가 항상 이긴다(테스트: `profile_merge::tests::deny_beats_allow_*`) |
  | 스칼라(대부분) | `theme`, `effortLevel` | 값이 다르면 경고 로그 남기고 나중 프로필 값으로 last-wins |
  | 스칼라(보안 민감) | `permissions.defaultMode` | 값이 다르면 **거부**(에러) — 권한 모드가 조합으로 조용히 약해지는 것을 last-wins 보다 우선 차단 |

- **저장 위치** — 전부 `TASTY_PLUGIN_DATA_DIR`(`~/.tasty/plugin-data/com.tasty.claude/`) 하위. 호스트가 이 디렉토리를 미리 만들어 주므로 `fs.write` 권한 없이도 쓸 수 있다. 호스트가 이 env 를 주입하지 않은 비정상 기동(`data_dir = None`)이면 등록/부착 모두 명시적 에러로 거부한다 — `~/.claude/` 나 새 경로를 조용히 쓰지 않는다.
- IPC: `claude.profile_register`/`claude.profile_unregister`/`claude.profile_list`/`claude.profile_show`/`claude.profile_current` — CLI 서브커맨드와 1:1 대응(원칙 2, 에이전트 조작 가능성).
- spawn 시 parent 의 살아있는 child 수가 설정 임계치를 넘으면 응답에 `warning` 필드가 실린다 — Settings › Plugin › Claude Code 에서 임계치 조정.
- **승인 정책 플래그 없음(미확인 상태)** — [codex](../codex/index.md) 플러그인은 `--approval`/`--sandbox`/`--full-auto` 로 자식의 승인/샌드박스 정책을 지정할 수 있지만(비대화형 자동화 흐름에서 승인 프롬프트가 자식을 영구히 멈추는 문제의 해결책), 이 플러그인의 `build_launch_command`(`crates/tasty-plugin-claude/src/handlers.rs`)에는 대응하는 플래그가 없다. Claude Code 는 codex 처럼 기동 시점 CLI 플래그가 아니라 `settings.json`(`permissions`)/`--permission-mode` 기반 권한 모델을 쓰므로 구조가 다르지만, `permissions.defaultMode` 가 승인이 필요한 값일 때 `spawn`/`launch`/`respawn` 으로 띄운 자식이 codex 와 동형으로 승인 프롬프트에서 영구히 멈추는지는 아직 재현·확인되지 않았다. 재현되면 codex 와 동형의 정책 플래그 노출이 필요하다.

### continue-checklist 세션 프로필

위 레지스트리가 host 출처로 미리 등록해 두는 첫 attachable 기본 프로필. `--profile continue-checklist` 로 부착하면(사용자가 같은 이름으로 직접 등록한 파일이 있으면 그쪽이 우선한다) `Stop` 훅으로 `tasty claude checklist-hook` 을 심는다 — 전역 `install`(위 "Claude Code 훅 통합" 절의 8종)에는 포함되지 않으며, 이 프로필이 부착된 세션에서만 발화한다.

- **동작**: 매 `Stop` 훅 발화마다 stdin JSON(`session_id`/`prompt_id`/`stop_hook_active`/`last_assistant_message`)을 읽어 4분기로 판단한다: ① 저장된 `prompt_id` 와 다르면(또는 저장 상태 없음) 라운드 0 으로 취급 ② `last_assistant_message` 에 센티넬 문자열 `[[TASTY-CHECKLIST-DONE]]` 이 포함되면 통과 ③ 라운드 수가 상한에 도달했으면 통과(백스톱) ④ 그 외엔 `{"decision":"block","reason":"<체크리스트 본문>"}` 을 반환하고 라운드 +1. `reason` 본문은 `t("claude.checklist.body")`(lang 파일, 3개 언어)로 활성 locale 로 해석된 문자열이며 3개 범용 항목(결과가 요청을 충족했는지 재검토 / 코드·설정 변경을 실제로 검증했는지 / 후속 작업 유무 명시)과 센티넬 포함 지시로 구성된다.
- **라운드 상한 백스톱이 필요한 이유**: Claude Code 자체엔 `Stop` 훅의 block 을 무한 반복해도 막아주는 host 측 안전장치가 없다(실측 확인 — 모델이 루프에 갇혔음을 스스로 인지해도 탈출하지 못했다). 상한은 Settings › Plugin › Claude Code 의 `continue-checklist round limit`(기본 3)로 노출된다.
- **라운드 상태 저장**: `TASTY_PLUGIN_DATA_DIR/checklist/rounds/<session_id>.json` — Claude Code `session_id` 로 키잉해 동시에 여러 세션이 이 프로필을 부착해도 라운드 카운터가 섞이지 않는다(전역 단일 파일 아님). 해당 세션의 `SessionEnd` 훅(위 "Claude Code 훅 통합" 절의 8종 중 하나, 이 프로필과 무관하게 항상 발화)이 오면 상태 파일을 정리한다.
- **마커 파일 게이트**: `TASTY_PLUGIN_DATA_DIR/checklist/enabled.marker` — 존재 여부로 발동을 켜고 끈다. 프로필 attach(=훅 등록)는 Claude Code 프로세스 기동 시점에 고정되지만, 마커는 매 훅 발화마다 파일 존재를 새로 확인하므로 재기동 없이 즉시 토글된다. `checklist-enable`/`checklist-disable`/`checklist-status` CLI(및 대응 IPC)가 이 마커 파일을 만들고 지우고 조회하는 제어된 진입점이다 — raw `touch`/`rm` 로 직접 조작할 필요가 없다.
- **안전한 통과 원칙**: 마커 부재, `session_id`/`prompt_id` 누락, stdin 파싱 실패 등 불확실한 조건은 전부 block 하지 않고 조용히 통과시킨다 — 판단 불가 상태에서 세션을 가두지 않는 것을 우선한다.
- IPC: `claude.checklist_hook` — `hook_args` 와 별개 파라미터 스키마(`checklist_hook_args`, 전부 stdin 자동 채움). `stop_hook_active` 는 `bool` 이 아니라 `string` 타입으로 선언돼 있다 — `CliArgType::Bool` 은 부재를 표현하지 못해(`extract_value` 가 항상 `Some(false)` 를 반환) `stdin_field` 매핑과 함께 쓰면 stdin 값이 절대 반영되지 않는다(핸들러가 `"true"`/`"false"` 문자열을 직접 비교). `claude.checklist_enable`/`claude.checklist_disable`/`claude.checklist_status` — 인자 없음(`no_args`), 마커 파일 생성/삭제/조회. `data_dir` 이 없는 비정상 기동이면 enable/disable 은 명시적 에러로 거부하고(profile.rs 결정 3과 동일 방침), status 는 `marker_present` 와 동일하게 `enabled: false` 로 안전 폴백한다(에러 아님 — 조회는 항상 응답 가능해야 한다).

### Claude Code 훅 통합

`tasty claude install`이 `~/.claude/settings.json`의 `hooks`에 아래 8개 이벤트를 심는다. 모든 이벤트가 같은 형태의 명령 문자열을 쓴다:

```
[ -n "$TASTY_SURFACE_ID" ] && tasty claude hook <token> || true
```

`session_id`/`message`/`notification_type` 같은 이벤트별 가변 데이터는 명령 인자가 아니라 **stdin JSON**으로 들어온다 — 매니페스트 `hook` cli 항목이 `stdin_json = true`를 선언하고, `--session`/`--message`/`--notification-type` 플래그가 각각 `stdin_field`로 stdin JSON에서 자동 채워진다(Claude Code가 hook 실행 시 stdin으로 JSON payload를 준다). POSIX 셸 구문 1종만 발행한다 — [codex](../codex/index.md)처럼 Windows PowerShell 분기는 없다.

| Claude Code 이벤트 | matcher | tasty hook token | `terminal.set_state` | `surface.fire_hook` | surface meta | `surface.completion` kind |
|---|---|---|---|---|---|---|
| `Stop` / `SubagentStop` | `""`(전체) | `stop` / `subagent-stop` | `idle` | `claude-idle` | — | `completion` |
| `SessionEnd` | `""`(전체) | `session-end` | `idle` | `claude-idle` | `claude-session-id`·`restore.command` **unset** | `completion` |
| `Notification` | `""`(전체) | `notification` | `needs_input`(단 `notification_type`이 `idle_prompt`면 건너뜀 — 무입력 대기 오탐이라 실제 질문 없음) | `needs-input`(동일 조건) | — | `needs_input`(동일 조건) |
| `UserPromptSubmit` | `""`(전체) | `prompt-submit` | `active` | — | — | — |
| `SessionStart` | `""`(전체) | `session-start` | `active` | — | `claude-session-id` = 세션 ID, `restore.command` = `claude -r <id>` **set**(stdin JSON에 `session_id`가 없으면 건너뜀) | — |
| `PreToolUse` | `AskUserQuestion` | `pre-tool-use` | `needs_input` | `needs-input` | — | `needs_input` |
| `PostToolUse` | `AskUserQuestion` | `post-tool-use` | `active` | — | — | — |

`surface.completion` 은 `{ surface_id, kind }` 로 호출되며(`HostCall::SurfaceCompletion`,
`hook.rs`), `kind` 는 위 표의 값을 그대로 싣는다 — 호스트의 `AttentionStore` 가
`NeedsInput` 을 `Completion` 보다 높은 우선순위로 표시한다(surface 테두리 노랑, 탭
제목·워크스페이스 배지도 동일 우선순위, [surface-highlight](../../features/surface-highlight/index.md)
참고).

`UserPromptSubmit`은 child가 2번째 이후 prompt를 받을 때 직전 `Stop` hook이 남긴 `idle=true` 잔재를 지우는 데 필수다 — 미등록 시 실제로는 active인 child를 idle로 오보고하는 상태 버그가 생긴다. `PreToolUse`/`PostToolUse`만 matcher `AskUserQuestion`으로 좁혀 등록돼 그 툴 호출에만 발화한다(나머지 6개는 matcher `""`로 이벤트 전체를 받는다) — 실측(실제 Claude Code를 띄워 hook stdin payload를 덤프해 확인) 결과 `AskUserQuestion` 답변은 `UserPromptSubmit`을 발생시키지 않으므로(질문/답변이 같은 prompt turn 안의 tool 상호작용이라 새 프롬프트로 집계되지 않음), 기존 `UserPromptSubmit`(→active)만으로는 이 케이스의 needs_input 해제 시점을 잡을 수 없다. `PreToolUse`가 질문 UI가 뜨기 **전에** 발화해(`tool_input.questions` 포함) needs_input을 켜고, `PostToolUse`가 답변 즉시(관찰상 `duration_ms: 0`) 그 짝으로 active로 되돌린다 — `needs_input`은 이제 `Notification`과 `PreToolUse` 두 경로에서 나온다.

install이 심는 훅은 이 8개뿐이다 — matcher가 지정되지 않은 `PreToolUse`/`PostToolUse` 호출 전체나 `PreCompact` 등 다른 Claude Code 이벤트는 걸지 않는다. `install.rs`의 `install_preserves_other_hooks` 테스트가 사용자가 직접 추가한(matcher가 다른) `PreToolUse` entry를 tasty의 `AskUserQuestion`-matcher entry와 분리해 그대로 보존함을 검증한다.

install은 marker substring(`tasty claude hook <token>`)으로 자기 entry를 식별해 멱등하게 동작한다 — marker가 일치하는 기존 entry는 명령 문자열만 최신 형태로 덮어쓰고(옛 버전이 심은 잘못된 명령이 남는 회귀 방지), 사용자가 직접 추가한 다른 entry는 건드리지 않는다. `PreToolUse`/`PostToolUse`처럼 matcher가 있는 이벤트는 marker 일치만으로는 matcher 값까지 보증되지 않으므로, install이 matcher도 canonical 값(`AskUserQuestion`)으로 함께 갱신한다.

이 플러그인이 fire하는 surface hook 이벤트는 `claude-idle`/`needs-input`/`claude-error` 3개이며, 매니페스트 `contributes.hook_events`로 선언한다 — host가 (내장 ∪ 활성 plugin 선언) 집합으로 `hook.set` 등록을 검증하므로([hooks](../../features/hooks/index.md)), 이 플러그인이 비활성이면 저 3개 키로의 hook 등록도 거부된다. **이 3개가 전부 위 8개 설치 훅에서 나오는 건 아니다** — `claude-idle`은 위 `apply_hook`(Stop/SubagentStop/SessionEnd)에서, `needs-input`은 `Notification`(idle_prompt 제외)과 `PreToolUse`(matcher `AskUserQuestion`) 두 경로에서 나오지만, `claude-error`는 이 훅 메커니즘과 무관한 별도 producer다: `error_scan.rs`가 surface 출력 텍스트를 패턴 매칭해 매치 시 직접 `surface.fire_hook`으로 발사한다. `claude-idle`/`needs-input`은 [surface-highlight](../../features/surface-highlight/index.md)(Stop hook → highlight)와 [telemetry](../../features/telemetry/index.md)(`session-start`→`stop`의 `wall_time_ms`, `notification`의 `input_tokens`)가 소비하고, `SessionStart`/`SessionEnd`의 meta set/unset은 [layout-persistence](../../features/layout-persistence/index.md)의 `restore.command` 복원이 소비한다.

### PTY 에러 스캔 (`claude-error`) 범위

`error_scan.rs`는 800ms 주기 폴링으로 추적 대상 surface 마다 `surface.read_since_mark`(strip-ansi)를 읽어 알려진 네트워크/API 에러 패턴(`API Error` / `Output blocked by content filtering policy` / `overloaded_error` / `rate_limit_error` / `Internal Server Error` / `network error` / `Bad Request`)을 매칭하고, 매치 시 그 surface 에 `claude-error` 를 fire 한다. 같은 텍스트가 연속 폴링에서 다시 잡히면 발화하지 않으며(dedupe), 새 턴 시작 신호(`prompt-submit`/`session-start`/`active`)에 dedupe 가 풀린다.

추적 대상은 **`claude launch` 로 만든 top-level surface 와 `claude spawn`/`claude respawn` 으로 만든 자식 surface 전부**다. 사람이 화면을 보고 있지 않은 자식이야말로 감지가 가장 필요한 대상이므로 자식을 제외하지 않는다.

정리(추적 해제)는 별도 구독 없이 같은 폴링 주기에 편승하되, **등록 경로에 따라 생존 판정 기준이 다르다**:

| 대상 | 등록 | 생존 판정 |
|------|------|-----------|
| top-level (`launch`) | child registry 에 없음 | `surface.locate` 로 surface 존재 확인 |
| 자식 (`spawn`/`respawn`) | 호스트 child registry | `terminal.parent` 로 **부모-자식 관계** 존재 확인 |

자식을 관계로 판정하는 이유는 [`terminal.release`](../../features/child-terminal/index.md)가 surface 를 닫지 않고 관계·soft 점유만 해제하기 때문이다 — surface 존재만 봤다면 release 후에도 영원히 폴링되며, 더 이상 자식이 아닌 사용자 터미널에 `claude-error` 를 계속 발화한다. 호스트가 관계 조회 전 `reconcile_child_terminals()` 를 돌리므로 이 한 번의 조회가 kill/close 실패로 surface 가 살아남은 케이스까지 함께 걷어낸다. `claude kill` 은 성공 응답의 `killed_surface_id` 로 즉시 `disable` 해 최대 800ms 의 잔여 발화 창까지 없앤다. 조회 자체가 실패(IPC 오류)하면 "죽었다"로 단정하지 않고 추적을 유지한다 — 재활성화 경로가 없어 오탐 정리가 오탐 유지보다 위험하다.

**mark 는 공유 자원이라 감지 사각이 있다.** 스캐너의 `surface.read_since_mark` 는 mark 를 전진시키지 않으므로(`Terminal::read_since_mark` 가 `&self`) 에이전트의 `tasty read since-mark` 결과를 소비하지 않지만, 반대로 에이전트가 `tasty set mark` 로 mark 를 앞으로 옮기면 스캐너가 보는 창도 함께 옮겨간다. 그 이전에 지나간 에러는 스캐너 시야에서 사라진다. 이 결합은 수용한다 — mark 를 스캐너 전용으로 따로 두면 mark 자원이 이중화돼 에이전트의 `set mark` 의미가 흐려진다.

## 인터페이스

- **AI Agent / 사용자**: `tasty claude launch|spawn|tell|broadcast|kill|respawn|children|parent|hook|checklist-hook|checklist-enable|checklist-disable|checklist-status|profile-register|profile-unregister|profile-list|profile-show|profile-current …`.
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
- [ ] Given 등록된 프로필 둘(각각 다른 마커를 남기는 `SessionStart` 훅) When 이름 둘을 쉼표로 `--profile` 에 함께 부착해 spawn Then **둘 다** 발화한다(머지가 last-wins 로 떨어지지 않는다). `permissions.deny`를 담은 프로필을 부착하면 그 자식에게서 해당 도구가 사라지고(거부 프롬프트가 아니라 툴셋에서 빠짐), `deny` 프로필과 그 도구를 `allow` 하는 프로필을 함께 부착해도 도구는 여전히 없다(deny 가 allow 를 이긴다). `--profile-file` 과 `--profile` 을 함께 주면 즉시 에러.
- [ ] Given `--profile continue-checklist` 로 부착한 세션 + 마커 파일 존재 When Claude 가 센티넬 없이 응답을 끝내려 함 Then block 되고 체크리스트 본문이 주입되며, 센티넬을 포함해 응답하거나 라운드 상한에 도달하면 정상 종료된다. 프로필을 부착하지 않은 세션은 이 동작에 전혀 영향받지 않는다. 같은 프로필을 부착한 세션 둘을 동시에 진행해도 라운드 카운터가 서로 섞이지 않는다.
- [ ] Given 마커 파일 부재 When `tasty claude checklist-enable` Then 마커 파일이 생성되고 `checklist-status` 가 `enabled: true` 를 보고한다. When `tasty claude checklist-disable` Then 마커 파일이 삭제되고 `checklist-status` 가 `enabled: false` 를 보고한다 — 마커가 이미 없는 상태에서 다시 `checklist-disable` 을 호출해도 에러 없이 `enabled: false` 를 반환한다(멱등).
</content>
