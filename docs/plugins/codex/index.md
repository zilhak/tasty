# Codex (`com.tasty.codex`)

- **Status**: Implemented (bundled plugin)
- **주체**: AI Agent / 로컬 사용자 (`tasty codex` CLI · IPC)
- **배포/통합**: bundled · cli + ipc_namespace + 멀티에이전트 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-codex/`
- **권한**: `terminal.spawn` 등 (매니페스트 `permissions`)
- **화면**: 없음 — CLI/IPC 오케스트레이션 플러그인 (headless).
- **플로우**: claude 와 동형인 멀티에이전트 오케스트레이션 다이어그램 (spawn·tell·wait·hook·상태머신) — [Figma · Flows & IA](https://www.figma.com/design/ct3uPefwY2uk6i1i9wYpkU/Untitled?node-id=33-915).

> **예제로서**: **cli + ipc namespace 멀티에이전트** 예제(claude 의 경량판) — state/handlers 모듈 분리 → [plugin-development](../../dev-guide/plugin-development.md#cli--ipc-namespace).

## 목적

**Codex CLI 를 tasty 안에서 실행·오케스트레이션**하는 통합. [claude](../claude/index.md) 플러그인과 동형이며, 주로 작성한 코드/판단을 Codex 에게 교차 검증시키는 용도.

## 내부 동작

- **cli `codex`** (`tasty codex …`) — 서브커맨드: `launch` · `spawn`(자식, 페인 분할) · `children`/`parent` · `tell`(메시지 전송, 줄바꿈 보존·자동 제출) · `notify-caller`(내부용, 아래) · `broadcast` · `kill`/`respawn` · `reboot`(같은 세션 resume 재시작, 아래) · `hook`(stop/prompt-submit/session-start). `install`/`uninstall`(Tasty 훅을 Codex CLI 설정에 설치).
- **ipc_namespace `codex`** — 위 동작의 IPC 표면.
- **event_subscribe** `surface.closed` — 인스턴스 상태 정리.
- **hook 명령은 OS 별 셸 구문으로 설치된다** — Codex 는 hook 명령을 Windows 에선 PowerShell, 그 외에선 POSIX sh 로 실행하므로, `install` 이 Windows 에는 `if ($env:TASTY_SURFACE_ID) { $input | tasty codex hook … }` 형태(PS), 그 외에는 `if [ -n "$TASTY_SURFACE_ID" ]; then tasty codex hook … --surface $TASTY_SURFACE_ID || true; fi` 형태(POSIX)를 발행한다. POSIX 쪽은 가드와 실패 처리가 분리돼 있다 — 바깥 `if` 는 tasty 밖 환경(`$TASTY_SURFACE_ID` 미설정)을 무소음 exit 0 으로 처리하고, 안쪽 `|| true` 는 hook 명령 실패만 담당한다(codex 턴을 막지 않기 위해 exit 0 유지). 실패 자체는 `<tasty_home>/hook-failures.log` 에 기록된다([ADR-0075](../../adr/0075-agent-hook-delivery-failure-record.md)). **명령 문자열이 바뀌었으므로 기존 사용자는 `tasty codex install` 재실행이 필요하다** — 재실행은 marker(`tasty codex hook`) 로 옛 entry 를 걷어내고 새 entry 를 넣으므로 중복되지 않는다. codex 는 hook command 해시가 바뀌면 trust 를 무효화하지만, tasty 는 모든 codex 인스턴스를 `--dangerously-bypass-hook-trust` 로 띄우므로 실제 발화에는 영향이 없다(`install` 응답의 trust 표시만 `needs_review` 로 바뀔 수 있다). hook 은 Codex 가 stdin 으로 주는 JSON payload 의 `session_id` 를 읽어 surface meta(`codex-session-id`, `restore.command`)에 기록한다 — `reboot` 와 세션 복원이 이 meta 를 소비한다. 설치 대상은 아래 3개뿐이다([claude](../claude/index.md)의 6개보다 적음 — Codex 에는 `Notification`/`SessionEnd` 에 대응하는 hook 이벤트가 없다):

  | Codex config table 키 | tasty hook token | trust state 키 |
  |---|---|---|
  | `Stop` | `stop` | `stop` |
  | `UserPromptSubmit` | `prompt-submit` | `user_prompt_submit` |
  | `SessionStart` | `session-start` | `session_start` |

  `~/.codex/config.toml`의 `[hooks]` 섹션에 심는다(`settings.json`이 아니다 — Codex 의 hook dispatch 경로가 아니다). Codex 는 새 hook entry를 *trust*하기 전엔 fire하지 않는다 — `--dangerously-bypass-hook-trust`(아래)가 이 승인 절차를 우회한다.
- **`spawn`/`tell` 은 동기 대기하지 않는다** — 호출 즉시 반환하고, 대상이 idle 이 될 때마다, 그리고 최종적으로 exited 가 되면 caller surface 에 알림이 주입된다. 내부적으로 `codex-idle`(`stop` hook 이 `surface.fire_hook` 으로 쏨) 과 `process-exit`(host 내장) 두 이벤트에 once(1회성) hook 을 등록하고, 먼저 fire 되는 쪽이 `notify-caller` 를 실행해 알림을 보낸 뒤 형제 hook 을 정리한다(등록 순서 무관 — fire 시점에 `hook.list`(대상 surface 필터) 로 자기와 **동일 command** 를 가진 형제를 찾아 `hook.unset`. 상태를 공유하지 않아 같은 surface 에 spawn/tell 이 겹쳐 등록돼도 서로의 형제를 덮어써 좀비로 남기지 않는다). 정리 후 `notify-caller` 는 `surface.locate` 로 target 이 아직 살아있는지(=이번 fire 가 process-exit 가 아니었는지) 확인해, 살아있으면 두 hook 을 다시 등록한다(자기재무장) — codex-idle 이 여러 번 반복돼도(예: 대기 후 재개) exit 할 때까지 계속 알림이 온다. `needs_input` 알림은 없다 — Codex CLI 에는 Claude Code 의 `Notification` hook 에 대응하는 이벤트가 없어 구조적으로 도달 불가능하다. **완료 알림에 샌드박스 초기화 실패 힌트가 자동으로 덧붙는다** — `notify-caller`가 알림을 조립하기 직전 대상 surface 의 최근 화면 출력(`surface.screen_text`, 최근 800줄)을 조회해 `RTM_NEWADDR`(아래 샌드박스 정책 플래그 항목의 실패 시그니처) 이 보이면 "sandbox 초기화 실패로 보임 — `--full-auto`로 재시도해보세요" 류 문구를 알림 본문에 추가한다(best-effort — 조회 실패나 미탐지 시 알림은 기존과 동일).
- **모든 codex 기동 명령에 `--dangerously-bypass-hook-trust`** — `spawn`/`launch`/`reboot` 이 codex 를 띄울 때마다 이 플래그를 붙여, 사용자가 `/hooks` 로 수동 승인하지 않아도 tasty 가 심은 hook 이 항상 fire 되게 한다(tasty 가 자기 hook 을 스스로 심으므로 정당한 사용 대상). `install` 은 여전히 trust 상태를 안내에 실어주지만(수동으로 `codex` 를 직접 실행하는 경우 대비), spawn/reboot 로 띄운 인스턴스의 hook 동작에는 영향 없다.
- **승인/샌드박스 정책 플래그** (`launch`/`spawn`/`respawn`/`reboot` 공통) — `--approval <untrusted|on-request|never>`(codex `-a`), `--sandbox <read-only|workspace-write|danger-full-access>`(codex `-s`), `--full-auto`(codex `--dangerously-bypass-approvals-and-sandbox`, `--approval`/`--sandbox` 와 동시 지정 시 invalid_params 로 거부)를 그대로 기동 명령에 전달한다. 우선순위는 **호출별 플래그 > 전역 설정(Settings › Plugin › Codex 의 "Default approval policy"/"Default sandbox mode", storage key `default_approval_policy`/`default_sandbox_mode`) > 하드코드 기본값**. **`approval` 은 호출별 플래그도 전역 설정도 없으면(또는 전역 설정이 `inherit`) 무조건 `never` 로 해석된다** — 완료 훅이 승인 대기 상태에서는 fire 되지 않아(`needs_input` 알림 부재) 아무도 응답하지 않으면 영구히 멈추므로, "아무것도 안 정하면 codex 자체 인터랙티브 기본값" 이라는 옛 동작은 더 이상 없다. 인터랙티브 승인이 정말 필요하면 `--approval untrusted`/`on-request` 를 명시적으로 넘긴다. **`sandbox` 는 승인과 달리 그 자체로 정지를 유발하지 않으므로 기존대로 미설정 시 플래그를 아예 안 붙여 codex 자체 기본값을 쓴다** — 단 nested sandbox(bubblewrap) 를 지원하지 않는 실행 환경에서는 `--sandbox read-only`/`workspace-write` 지정이 `RTM_NEWADDR: Operation not permitted` 류로 실패할 수 있어, 그런 환경에선 `--full-auto`(샌드박스까지 완전 우회)를 명시적으로 골라야 한다.
- **`reboot`** (`tasty codex reboot [--surface <id>] [--delay <초>] [--prompt <추가문구>] [--approval <값>] [--sandbox <값>] [--full-auto]`) — surface 안의 Codex 를 종료하고 **같은 세션으로 재시작**한다([claude reboot](../claude/index.md) 와 동형). 동작: 즉시 응답 → delay(기본 5s) 후 Ctrl+C ×4 → 전경 이탈 확인 후 `codex resume --dangerously-bypass-hook-trust [정책 플래그] -c check_for_update_on_startup=false <session_id>` 전송(업데이트 프롬프트가 기동을 가로채는 것을 방지) → 복귀 확인 후 재시작 안내 프롬프트를 화면 검증·재시도 + 별도 Enter 로 제출. 정책 플래그는 위 승인/샌드박스 규칙과 동일하게 해석된다. 안전 가드는 claude 와 동일(전경 불일치 시 미전송·중단, 중복 reboot 거부). **턴의 마지막 행동으로 호출할 것.** Codex 에는 SessionEnd hook 이 없어 세션 meta 는 종료 시 지워지지 않고 다음 session-start 가 덮어쓴다.
- **spawn child 개수 경고** — `spawn` 이 성공한 뒤 parent 의 현재 child 수를 재조회해, Settings › Plugin › Codex 의 "Spawn child warning threshold"(기본 6) 를 넘으면 응답에 `warning` 필드를 실어 돌려준다(soft 경고, spawn 자체는 막지 않음). 재사용 후보가 있으면 그 index 목록과 함께 새로 spawn 하는 대신 `respawn` 사용을 권하는데, 근거가 다른 두 목록으로 나뉜다: **`idle`** 은 자식이 hook 으로 완료를 직접 보고한 것이고, **확정 `stale`**(`confidence: confirmed` = 전경이 셸로 복귀)은 보고가 오지 않은 채 호스트 관측이 에이전트 프로세스 종료를 잡아낸 것이다(hook 유실 — [ADR-0072](../../adr/0072-child-state-hook-observation-fusion.md) 가 겨냥한 시나리오). 후자에 "이미 작업을 끝냈다" 는 문구를 쓰면 자식이 그렇게 보고한 적 없는데 보고한 것처럼 읽히므로 문구를 분리한다. 세 문구는 plugin 의 `lang/{en,ko,ja}.toml` 의 `codex.spawn_warning.{total,idle,stale}` 에 있고, 활성 언어(`general.language`)를 따라간다 — plugin process 는 호스트 i18n 카탈로그에 접근할 수 없으므로 SDK `Translator` 로 자기 `lang/` 를 직접 로드한다([i18n](../../dev-guide/i18n.md) "Plugin 네임스페이스").

  `confidence: heuristic` 인 `stale` 은 **세지 않는다** — SIGSTOP·긴 추론·무출력 명령과 관측상 구별되지 않아, 그것까지 respawn 후보로 부르면 일하는 자식을 재시작하라고 권하게 된다([api-conventions](../../dev-guide/api-conventions.md) 가 같은 이유로 `stale` 을 기본 terminal state 집합에서 뺀 것과 동일한 판단). 판정 축 자체는 [child-terminal](../../features/child-terminal/index.md) "판정 응답 필드" 참조.

## 인터페이스

- **AI Agent / 사용자**: `tasty codex launch|spawn|tell|broadcast|kill|respawn|children|parent|hook|install …`.
- 일반 흐름: `spawn --prompt "…"` → (선택) `tell` → 완료 알림 대기(caller surface 에 자동 주입) → 출력 확인.

## 비-목표

- Codex 자체 기능 — 외부 CLI. 이 플러그인은 *실행·배치·관계 관리*.
- 터미널/PTY 내부 — host.

## Acceptance Criteria

- Given 플러그인 활성 When `tasty codex spawn --prompt "…"` Then 자식 Codex 가 페인 분할로 생성되고 CLI 는 즉시 반환된다.
- Given 자식 When `tasty codex tell <msg>` Then 줄바꿈 보존하며 메시지가 전송·제출되고 CLI 는 즉시 반환된다.
- Given `spawn`/`tell` 로 등록된 완료 대기 When 대상이 idle 또는 exited 에 도달 Then caller surface 에 알림이 주입되고 형제 hook 이 정리된다. exited 가 아니었다면(=idle) 형제 hook 이 재등록돼 이후 상태 전환에도 계속 알림이 온다.
</content>
