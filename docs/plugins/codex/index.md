# Codex (`com.tasty.codex`)

- **Status**: Implemented (bundled plugin)
- **주체**: AI Agent / 로컬 사용자 (`tasty codex` CLI · IPC)
- **배포/통합**: bundled · cli + ipc_namespace + 멀티에이전트 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-codex/`
- **권한**: `terminal.spawn` 등 (매니페스트 `permissions`)
- **화면**: 없음 — CLI/IPC 실행 관리 플러그인 (headless).

> **예제로서**: **cli + ipc namespace 멀티에이전트** 예제(claude 의 경량판) — state/handlers 모듈 분리 → [plugin-development](../../dev-guide/plugin-development.md#cli--ipc-namespace).

## 부모의 완료 수신 채널

완료는 부모 종류와 무관하게 caller surface 의 `<parent_home>/notify/<caller_surface>.log`
한 줄로 나간다. child CLI 종류로 채널을 선택하지 않는다. 훅 설치·관측 상태와 부모의 수신
준비는 별개다. idle/needs_input/interrupt/exit는 작업 성공과 같은 뜻이 아니다.
[완료 알림 로그](../../dev-guide/external-interaction.md#child-완료-알림--completion-log)를 따른다.


## 목적

**Codex CLI를 tasty 안에서 실행하고 관리**하는 통합. [claude](../claude/index.md) 플러그인과 동형이며, 주로 작성한 코드/판단을 Codex 에게 교차 검증시키는 용도.

## 내부 동작

짝 핸들러의 서로 다른 공개 응답과 번역 형식은 기존 호출자 호환을 위해 유지한다.
`children`/`kill` 형상 및 완료 hook의 대칭·의도된 차이는
[짝 핸들러 호환 경계](../../dev-guide/paired-agent-handlers.md)를 따른다. 완료 알림은
[completion-log](../../dev-guide/external-interaction.md#child-완료-알림--completion-log)에 기록하며 caller PTY에 메시지를 입력하지 않는다.

- **cli `codex`** (`tasty codex …`) — 서브커맨드: `launch` · `spawn`(자식, 페인 분할) · `children`/`parent` · `tell`(메시지 전송, 줄바꿈 보존·자동 제출) · `notify-caller`(내부용, 아래) · `broadcast` · `kill`/`respawn` · `reboot`(같은 세션 resume 재시작, 아래) · `hook`(stop/prompt-submit/session-start/permission-request/post-tool-use/interrupt/session-end). `install`/`uninstall`(Tasty 훅을 Codex CLI 설정에 설치).
- **ipc_namespace `codex`** — 위 동작의 IPC 표면.
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
  | `SessionEnd` | `session-end` | `session_end` | 실행 종료 | 현재 세션 meta 정리, 해당 실행 구독 종료 |
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

spawn과 tell은 즉시 반환한다. `codex-idle`, `needs-input`, `process-exit`에 once 훅을 걸고,
먼저 발생한 이벤트가 notify-caller로 부모 완료 로그에 상태를 남긴다.
대상 surface의 hook.list에서 같은 command를 가진 나머지 훅을 찾아 정리하므로,
같은 서피스에 여러 요청이 있어도 서로 다른 요청의 훅을 지우지 않는다.
대상이 살아 있으면 세 훅을 다시 등록해 다음 상태 변화도 받는다.

notify-caller는 최근 화면 800줄에서 RTM_NEWADDR를 찾으면 샌드박스 초기화 실패 가능성을
힌트로 덧붙인다. 조회 실패나 패턴 미검출은 기존 알림을 막지 않는다.
힌트는 진단용이며 Tasty가 실행 정책을 자동으로 바꾸거나 재시작한다는 뜻은 아니다.

- **훅 응답은 조용히 실패한 host 호출 수를 싣는다** — `host_call_failures`(항상 있고 항상 수). 이 핸들러는 `terminal.set_state` 만 전파하고(그 실패는 오류 응답이라 이미 보인다) `surface.meta.set`·`surface.fire_hook` 은 최선노력이라 실패해도 응답이 `ok` 다 — 세는 것은 그 최선노력 쪽이다. 0 이 아니면 `<tasty_home>/hook-failures.log` 에도 남는다. 규약은 [error-handling](../../dev-guide/error-handling.md) "최선노력의 대가는 치르되 값으로 노출한다".
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
  동작: 즉시 응답 → delay(기본 5s) 후 Ctrl+C ×4 → 일반 종료 안내 또는 remote TUI의 `Disconnected from this task.` 증가 확인 후 `<플랫폼별 Codex 실행어> resume --dangerously-bypass-hook-trust [정책 플래그] -c check_for_update_on_startup=false <session_id>` 전송(업데이트 프롬프트가 기동을 가로채는 것을 방지) → 복귀 확인 후 재시작 안내 프롬프트를 화면 검증·재시도 + 별도 Enter 로 제출.
  정책 플래그는 위 승인/샌드박스 규칙과 동일하게 해석된다.
  안전 가드는 claude 와 동일(전경 불일치 시 미전송·중단, 중복 reboot 거부).
  **턴의 마지막 행동으로 호출할 것.** SessionEnd는 현재 세션 meta를 정리한다.
  reboot는 종료 전에 세션과 검증된 remote 연결 문맥을 캡처한다.
### 자식 수 경고

spawn 뒤 살아 있는 자식 수가 설정의 Spawn child warning threshold(기본 6)를 넘으면
응답에 warning을 넣되 생성은 막지 않는다. 재사용 후보는 idle과 확정 stale을 나눠 보여 준다.
idle은 자식의 보고이고, 확정 stale은 전경이 셸로 돌아온 관측이다.
관측만 받은 자식이 작업 성공을 보고했다고 설명하지 않는다.
문구는 `codex.spawn_warning.{total,idle,stale}`을 현재 언어로 번역한다.


  `confidence: heuristic` 인 `stale` 은 **세지 않는다** — SIGSTOP·긴 추론·무출력 명령과 관측상 구별되지 않아, 그것까지 respawn 후보로 부르면 일하는 자식을 재시작하라고 권하게 된다([api-conventions](../../dev-guide/api-conventions.md) 가 같은 이유로 `stale` 을 기본 terminal state 집합에서 뺀 것과 동일한 판단). 판정 근거는 [child-terminal](../../features/child-terminal/index.md) "판정 응답 필드" 참조.

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

- Given 플러그인 활성 When `tasty codex spawn --prompt "…"` Then 자식 Codex 가 페인 분할로 생성되고 CLI 는 즉시 반환된다.
- Given 자식 When `tasty codex tell <msg>` Then 줄바꿈 보존하며 메시지가 전송·제출되고 CLI 는 즉시 반환된다.
- Given `spawn`/`tell` 로 등록된 완료 대기 When 대상이 idle · needs_input · exited 에 도달 Then caller 의 완료 알림 로그에 상태를 전달하고 형제 hook 이 정리된다. exited 가 아니었다면 형제 hook 이 재등록돼 이후 상태 전환에도 계속 알림이 온다.
- Given 훅이 설치된 Codex When 도구 실행 승인 프롬프트가 뜬다 Then 그 surface 의 상태가 `needs_input` 으로 조회되고 비포커스 대상의 탭·워크스페이스에 기존 노란 표시가 난다.
- Given 승인 대기 When 사용자가 승인하고 그 도구가 끝난다(`PostToolUse`) 또는 거절·Esc·Ctrl-C 로 중단한다(`Interrupt`) Then 상태가 각각 `active` · `idle` 로 돌아오고 `needs_input` 이 잔류하지 않는다.
- Given 승인이 필요 없는 실행 When 도구가 오래 걸리거나 출력이 없다 Then `needs_input` 으로 오판하지 않는다.
