# Codex (`com.tasty.codex`)

- **Status**: Implemented (bundled plugin)
- **주체**: AI Agent / 로컬 사용자 (`tasty codex` CLI · IPC)
- **배포/통합**: bundled · cli + ipc_namespace + 멀티에이전트 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-codex/`
- **권한**: `terminal.spawn` 등 (매니페스트 `permissions`)
- **화면**: 없음 — CLI/IPC 실행 관리 플러그인 (headless).

> CLI·IPC로 자식 프로세스를 실행하고 관리하는 예제다. [플러그인 개발](../../dev-guide/plugin-development.md#cli--ipc-namespace)을 참고한다.

## 부모의 완료 수신 채널

상태 변경은 caller surface의 `<parent_home>/notify/<caller_surface>.log`에 한 줄로 기록한다. 자식 CLI 종류로 경로를 바꾸지 않는다. 훅 설치와 부모의 수신 준비는 별개이며 idle·needs_input·interrupt·exit는 작업 성공을 뜻하지 않는다. [완료 알림 로그](../../dev-guide/external-interaction.md#child-완료-알림--completion-log)를 따른다.

## 목적

**Codex CLI를 tasty 안에서 실행하고 관리**하는 통합. [claude](../claude/index.md) 플러그인과 동형이며, 주로 작성한 코드/판단을 Codex 에게 교차 검증시키는 용도.

## 내부 동작

짝 핸들러의 서로 다른 공개 응답과 번역 형식은 기존 호출자 호환을 위해 유지한다.
`children`/`kill`의 응답 형식과 상태 훅의 공통점·차이점은
[짝 핸들러 호환 경계](../../dev-guide/paired-agent-handlers.md)를 따른다. 완료 알림은
[completion-log](../../dev-guide/external-interaction.md#child-완료-알림--completion-log)에 기록하며 caller PTY에 메시지를 입력하지 않는다.

- **cli `codex`** (`tasty codex …`) — 서브커맨드: `launch` · `spawn`(자식, 페인 분할) · `children`/`parent` · `tell`(메시지 전송, 줄바꿈 보존·자동 제출) · `notify-caller`(내부용, 아래) · `broadcast` · `kill`/`respawn` · `reboot`(같은 세션 resume 재시작, 아래) · `hook`(stop/prompt-submit/session-start/permission-request/post-tool-use/interrupt/session-end). `install`/`uninstall`(Tasty 훅을 Codex CLI 설정에 설치).
- **ipc_namespace `codex`** — 위 동작에 대응하는 IPC API.

### 훅 설치와 실행

install은 Windows에 PowerShell, 다른 OS에 POSIX sh 명령을 설치한다.
PowerShell은 `$env:TASTY_SURFACE_ID`를 확인하고 `$input`을 hook 명령에 전달한다.
POSIX는 `if [ -n "$TASTY_SURFACE_ID" ]; then … || true; fi`로 Tasty 안에서만 호출한다.
실패 진단은 [CLI 훅 실패 기록](../../dev-guide/cli-structure.md#에이전트-훅-전달-실패-기록)을 따른다.

업데이트로 설치 명령이 바뀌면 `tasty codex install`을 다시 실행한다.
`tasty codex hook` marker로 Tasty 항목만 교체하며 같은 matcher의 사용자 핸들러와 설정은 보존한다.
훅 명령의 변경은 Codex의 trust 상태에도 영향을 줄 수 있다. 설치된 trust 메타데이터만으로
실제 훅 실행을 보장하지 않으며, 0.154.0 remote resume에서는 검토 화면이 나타날 수 있다.

훅 stdin의 session_id는 `codex-session-id`와 `restore.command` meta에 기록하며 reboot와
세션 복원이 사용한다. 설치 이벤트는 아래 일곱 개다. 승인이 어느 도구에서나 발생할 수 있어
도구 이름 matcher를 두지 않으며 PostToolUse도 매 호출마다 프로세스를 실행한다.

  | Codex config table 키 | tasty hook token | trust state 키 | 산출 상태 | 추가 이벤트·동작 |
  |---|---|---|---|---|
  | `Stop` | `stop` | `stop` | `idle` | `codex-idle` |
  | `UserPromptSubmit` | `prompt-submit` | `user_prompt_submit` | `active` | — |
  | `SessionStart` | `session-start` | `session_start` | `active` | — |
  | `SessionEnd` | `session-end` | `session_end` | 변경 없음 | `codex-session-id` meta 삭제 |
  | `PermissionRequest` | `permission-request` | `permission_request` | `needs_input` | `needs-input` · `surface.completion`(kind=needs_input) |
  | `PostToolUse` | `post-tool-use` | `post_tool_use` | `active` | — |
  | `Interrupt` | `interrupt` | `interrupt` | `idle` | `codex-idle` |

  `~/.codex/config.toml`의 `[hooks]` 섹션에 심는다(`settings.json`이 아니다 — Codex 의 hook dispatch 경로가 아니다). Codex 는 새 hook entry를 *trust*하기 전엔 fire하지 않는다 — `--dangerously-bypass-hook-trust`(아래)를 전달하지만 remote resume의 검토 화면이 사라진다고 보장하지 않는다.
- **도구 실행 승인 대기**: `PermissionRequest`는 상태를 `needs_input`으로 바꾸고 부모 알림과
  공용 attention 표시를 요청한다. 승인이 필요 없는 실행에는 이 훅이 발생하지 않는다.
  `PostToolUse`는 active로, 거절·Esc·Ctrl-C의 `Interrupt`는 idle로 바꾼다.
  호스트의 idle 전이는 `terminal.state`에 남아 있던 `needs_input` 상태를 해제한다.

  승인 직후 이벤트가 없으므로 승인해도 도구가 끝날 때까지 `terminal.state`는 `needs_input`일 수 있다.
  화면의 attention 알림은 별도 상태이며 [사용자 확인·clear 규칙](../../adr/0024-attention-ownership-and-clear.md)을 따른다.
  `PermissionRequest`에 tool ID가 없어 한 서피스의 연속 승인을 도구별로 대응시키지 못한다.
  이 제한은 codex-cli 0.154.0 관측에 근거하며 다른 버전에서는 실제 훅을 확인한다.
  일반 질문(`request_user_input` 등)의 입력 대기는 이 훅의 대상이 아니다.
  Codex의 작업 완료 전략은 `needs_input`을 성공 종료 상태로 사용하지 않는다.

### 자식 상태 알림

spawn과 tell은 필요한 호스트 호출의 응답을 받은 뒤 반환하며 자식 작업 완료까지 기다리지 않는다. `codex-idle`, `needs-input`, `process-exit` once 훅을 등록한다. 하나가 실행되면 notify-caller가 상태 변경 로그를 쓰고 같은 command의 나머지 훅을 정리한다.

명령 문자열은 caller·target·kind로 구분한다. 이 조합이 같으면 반복 요청도 같은 훅 그룹이므로 요청마다 독립된 구독은 아니다. 이후 `surface.locate`가 성공하면 세 훅을 다시 등록한다. Surface 존재는 프로세스 생존과 다르며, 훅 등록과 로그 쓰기 실패나 재등록 사이의 이벤트 전달까지 보장하지 않는다.

notify-caller는 최근 화면 800줄에서 RTM_NEWADDR를 찾으면 샌드박스 초기화 실패 가능성을
힌트로 덧붙인다. 조회 실패나 패턴 미검출은 기존 알림을 막지 않는다.
힌트는 진단용이며 Tasty가 실행 정책을 자동으로 바꾸거나 재시작한다는 뜻은 아니다.

- **훅 호출 실패 집계**: `terminal.set_state` 실패는 오류 응답으로 반환한다. 나머지 정리·알림 호출은 실패해도 계속하고 `host_call_failures`로 실패 수를 알린다. 0이 아니면 `<tasty_home>/hook-failures.log`에도 기록을 시도한다. `ok`만으로 모든 호출이 성공했다고 판단하지 않는다([오류 처리](../../dev-guide/error-handling.md)).
- **훅 출력 계약** — `hook`의 IPC/직접 CLI 응답은 진단용 `host_call_failures`를 포함한다. 설치된 일곱 셸 래퍼는 CLI stdout을 버리고 Codex에는 빈 JSON 객체 `{}`만 반환한다 — `PermissionRequest` 에서 그 값은 "결정 없음"이라 승인 흐름을 바꾸지 않는다. 실패 기록(`hook-failures.log`)과 stderr는 유지한다. [Codex 훅 출력 규약](https://learn.chatgpt.com/docs/hooks)에 없는 내부 필드를 전달하지 않으며, 기존 설치에는 `tasty codex install`을 다시 실행해 래퍼를 갱신한다.
- **모든 codex 기동 명령에 `--dangerously-bypass-hook-trust`** — `spawn`/`launch`/`reboot`이 이 옵션을 전달한다. 그러나 0.154.0 remote resume의 훅 검토 화면은 별도로 나타날 수 있다. `install`의 trust 표시는 선택한 파일의 metadata이며 실제 훅 실행이나 원격 daemon 신뢰의 증명이 아니다. 검토가 필요한 훅은 사용자가 Codex에서 확인하며 Tasty가 대신 승인하지 않는다.
- **POSIX 셸의 외부 Codex 실행** — `launch`/`spawn`/`respawn` 의 공통 빌더는 모든 OS 에서 기존 POSIX 환경변수·프롬프트 구문과 함께 `command codex` 를 사용해 셸의 `codex` alias/function 을 우회하고 현재 `PATH` 의 실행 파일을 찾는다. Windows 에서 탐지하는 Git Bash 도 이 경로를 사용한다. `reboot` 는 Linux/macOS 에서 `command codex`, Windows 에서는 기존 `codex` 실행어를 유지한다. Windows 의 수신 셸 종류를 모르는 상태에서 cmd 를 중첩하지 않으므로 Git Bash 의 MSYS 인자 변환을 새로 유발하지 않는다. **Windows reboot 의 alias/function 우회와 native cmd/PowerShell 전용 새 세션 전달 개선은 후속 범위**다. 기존 정책 우선순위·기본값·명시 옵션과 Windows 지원 범위는 바꾸지 않는다. alias 에 넣어 둔 옵션을 적용하려면 tasty 의 정책 플래그나 전역 설정에 명시한다.

### 승인과 샌드박스 설정

launch·spawn·respawn·reboot는 다음 값을 Codex에 전달한다.

| Tasty 옵션 | 전달 내용 |
|---|---|
| `--approval untrusted\|on-request\|never` | `-a` 승인 정책 |
| `--sandbox read-only\|workspace-write\|danger-full-access` | `-s` 샌드박스 모드 |
| `--full-auto` | `--dangerously-bypass-approvals-and-sandbox` |

full-auto와 approval 또는 sandbox를 함께 지정하면 invalid_params로 거절한다.
우선순위는 호출별 옵션 → 플러그인 설정 → 기본값이다.
설정 키는 `default_approval_policy`와 `default_sandbox_mode`다.
호출별 approval이 없으면 플러그인 설정을 사용한다. 이 설정도 없거나 inherit이면 never를 사용한다.
호출별 sandbox와 유효한 플러그인 기본값이 모두 없으면 해당 옵션을 붙이지 않아 Codex 자체 설정을 따른다.

승인을 직접 받으려면 untrusted 또는 on-request를 명시한다.
PermissionRequest 훅은 그 대기를 알려주며 대신 승인하지 않는다.
중첩 샌드박스를 지원하지 않는 환경에서는 read-only나 workspace-write가
RTM_NEWADDR 오류로 시작에 실패할 수 있다. 실행 환경과 선택한 정책을 함께 확인한다.

- **`reboot`** (`tasty codex reboot [--surface <id>] [--delay <초>] [--prompt <추가문구>] [--approval <값>] [--sandbox <값>] [--full-auto]`) — surface 안의 Codex 를 종료하고 **같은 세션으로 재시작**한다([claude reboot](../claude/index.md) 와 동형).
  요청 시 `codex-session-id`와 화면의 종료 안내 횟수를 읽고 재시작 작업을 예약한다. delay(기본 5s) 후 Ctrl+C를 4회 보낸 뒤 화면에서 `run codex resume`의 횟수가 늘었는지 확인한다. 확인되면 `<플랫폼별 Codex 실행어> resume --dangerously-bypass-hook-trust [정책 플래그] -c check_for_update_on_startup=false <session_id>`를 전송한다. 이후 복귀 화면을 확인하고 안내 문구와 별도 Enter를 보낸다.
  정책 플래그는 위 승인/샌드박스 규칙과 동일하게 해석된다.
  같은 surface의 중복 reboot는 거절한다. 종료·복귀 확인은 화면 문자열을 사용하므로 Claude 플러그인의 전경 이름 확인과 다르며, 임의 출력이나 화면 이동으로 판정이 틀릴 수 있다.
  **턴의 마지막 행동으로 호출할 것.** SessionEnd는 현재 세션 meta를 정리한다.
  `Disconnected from this task.`나 원격 연결 문맥은 이 재시작 판정에서 사용하지 않는다.

### 자식 수 경고

spawn 뒤 살아 있는 자식 수가 설정의 Spawn child warning threshold(기본 6)를 넘으면
응답에 warning을 넣되 생성은 막지 않는다. 재사용 후보는 idle과 확정 stale을 나눠 보여 준다.
idle은 자식의 보고이고, 확정 stale은 전경이 셸로 돌아온 관측이다.
관측만 받은 자식이 작업 성공을 보고했다고 설명하지 않는다.
문구는 `codex.spawn_warning.{total,idle,stale}`을 현재 언어로 번역한다.

`confidence: heuristic`인 stale은 재사용 후보 수에 넣지 않는다. 긴 추론·SIGSTOP·무출력 작업을 종료한 자식으로 잘못 권할 수 있기 때문이다. 판정 기준은 [자식 터미널](../../features/child-terminal/index.md)의 판정 응답 필드를 참고한다.

## 훅 설치 대상 선택

`tasty codex install --codex-home <절대 디렉터리>` 는 그 Codex home 의 hook 을 설치한다.
`--config-file <절대 파일>` 은 설정 파일을 직접 고르며 `--codex-home` 과 함께 쓸 수 없다.
기존 설정을 읽거나 파싱할 수 없으면 덮어쓰지 않고 실패한다. 둘 다 생략하면 호스트 환경의
`CODEX_HOME` 을, 그것도 없으면 `HOME`/`USERPROFILE` 아래 `.codex` 를 쓴다. 원격 서버의
설정은 로컬 설치가 성공했다는 것만으로 갱신되지 않는다.

## 인터페이스

- **AI Agent / 사용자**: `tasty codex launch|spawn|tell|broadcast|kill|respawn|children|parent|hook|install …`.
- 일반 흐름: `spawn --prompt "…"` → (선택) `tell` → 완료 알림 대기(caller의 completion-log 조회) → 출력 확인.

## 비-목표

- Codex 자체 기능 — 외부 CLI. 이 플러그인은 *실행·배치·관계 관리*.
- 터미널/PTY 내부 — host.

## Acceptance Criteria

- Given Linux/macOS 의 sh/bash/zsh 에 `codex` alias 또는 function 이 우회 옵션을 추가한다 When `launch`/`spawn`/`respawn`/`reboot` 명령을 생성해 실행한다 Then 외부 Codex 에는 tasty 가 결정한 정책 인자만 전달되고 환경변수와 프롬프트 전달 방식이 유지된다.

- Given 플러그인 활성 When `tasty codex spawn --prompt "…"` Then 자식 Codex 가 페인 분할로 생성되고 CLI는 호스트 응답 후 반환하며 자식 작업 완료는 기다리지 않는다.
- Given 자식 When `tasty codex tell <msg>` Then 줄바꿈 보존하며 메시지가 전송·제출되고 CLI는 호스트 응답 후 반환하며 자식 작업 완료는 기다리지 않는다.
- Given 상태 훅과 로그 기록이 정상 동작하는 대상 When 등록된 idle·needs_input·process-exit 훅이 실행 Then caller의 상태 로그를 쓰고 형제 훅을 정리한다. `surface.locate`가 성공하면 다시 등록한다.
- Given 훅이 설치된 Codex When 도구 실행 승인 프롬프트가 뜬다 Then 그 surface 의 상태가 `needs_input` 으로 조회되고 비포커스 대상의 탭·워크스페이스에 기존 노란 표시가 난다.
- Given 승인 대기 When 사용자가 승인하고 그 도구가 끝난다(`PostToolUse`) 또는 거절·Esc·Ctrl-C 로 중단한다(`Interrupt`) Then 상태가 각각 `active` · `idle` 로 돌아오고 `needs_input` 이 잔류하지 않는다.
- Given 승인이 필요 없는 실행 When 도구가 오래 걸리거나 출력이 없다 Then `needs_input` 으로 오판하지 않는다.
