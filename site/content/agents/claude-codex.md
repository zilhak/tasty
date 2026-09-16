# Claude · Codex 와 함께 쓰기

Claude Code와 Codex CLI를 연결해 여러 에이전트에게 일을 나눠 맡겨보세요. 한 에이전트가 다른 에이전트를 실행하고 결과를 받는 방식으로 구현, 테스트, 검토를 함께 진행할 수 있습니다.

Claude Code와 Codex CLI는 별도로 설치하세요. Tasty는 에이전트 실행과 배치, 작업을 맡긴 에이전트와의 연결을 관리합니다. 아래 훅 설정을 마치면 작업 완료 알림을 받을 수 있습니다.

## 1. 훅 설치 (처음 한 번)

에이전트의 상태(작업 중 / 대기 / 입력 필요 / 종료)를 Tasty 가 알려면 각 CLI 의 훅 설정에 Tasty 항목이 들어가야 합니다.

```sh
tasty claude install    # ~/.claude/settings.json 의 hooks 에 Tasty 항목 추가
tasty codex install     # ~/.codex/config.toml 의 [hooks] 에 Tasty 항목 추가
```

- 이미 직접 넣어 둔 훅은 그대로 보존됩니다. 여러 번 실행해도 중복되지 않습니다.
- **Tasty 를 업데이트한 뒤에는 다시 실행합니다.** 훅 명령 문자열은 설정 파일에 저장되므로 새 형식을 반영하려면 재설치가 필요합니다.
- Codex에서 `hook returned invalid ... JSON output` 오류가 뜨면 업데이트 후 `tasty codex install`을 다시 실행하세요. Tasty의 상태 전달 결과가 Codex의 훅 응답에 섞이지 않도록 설정됩니다.
- Tasty 밖에서 Claude Code를 실행하면 이 훅은 동작하지 않습니다.
- 제거는 `tasty claude uninstall` / `tasty codex uninstall`.

훅이 설치되면 다음이 자동으로 동작합니다.

- 에이전트가 응답을 마치거나 질문을 던지면 그 서피스에 **주의 환기 테두리**가 켜지고 사이드바 워크스페이스에 배지가 붙습니다 (질문 대기는 노란색 우선).
- 탭을 닫았다가 복원하거나 Tasty 를 재시작하면 같은 세션으로 다시 이어집니다 (`claude -r` / `codex resume`).
- 자식 에이전트의 완료 알림(아래)이 부모에게 갑니다.

## 2. 실행하기

```sh
tasty claude launch --workspace myproj --directory ~/proj --task "테스트 고치기"
tasty codex launch --workspace review --directory ~/proj
```

새 워크스페이스를 만들고 그 터미널에서 CLI 를 실행합니다. `--workspace` 를 생략하면 이름은 `claude` / `codex`.

Codex 는 `--approval untrusted|on-request|never`, `--sandbox read-only|workspace-write|danger-full-access`, `--full-auto` 로 승인·샌드박스 정책을 붙일 수 있습니다 (아래 "Codex 승인 정책").

Claude Code 는 `--permission-mode` 로 권한 모드를 지정할 수 있습니다 (아래 "Claude 권한 모드").

<a id="3-자식-에이전트-부리기-spawn--tell"></a>

## 3. 다른 에이전트에게 작업 맡기기 (spawn / tell)

Claude Code 세션에서 다음 명령으로 다른 에이전트를 실행해 보세요. 작업을 맡긴 쪽을 부모, 새로 실행한 쪽을 자식 에이전트라고 부릅니다. 새 에이전트는 지정한 워크스페이스의 페인에 탭으로 열리고, 이 관계가 기록됩니다.

```sh
tasty claude spawn --workspace workers --cwd ~/proj --role tester --nickname t1 \
  --prompt "cargo test 를 돌리고 실패 원인을 보고해"
tasty codex spawn --workspace workers --cwd ~/proj --sandbox read-only \
  --prompt "방금 커밋된 diff 를 리뷰해"
```

| 옵션 | 뜻 |
|---|---|
| `--workspace <ID 또는 이름>` | 필수. 자식이 들어갈 워크스페이스 |
| `--pane <ID>` | 워크스페이스의 특정 페인 (기본: 첫 페인) |
| `--cwd <경로>` | 자식의 작업 디렉토리 |
| `--role <라벨>` | 역할 라벨. `broadcast --role` 로 골라 보낼 때 씀 |
| `--nickname <이름>` | 탭에 표시할 이름 |
| `--prompt <텍스트>` | 띄운 직후 보낼 첫 지시 |
| `--surface <ID>` | 부모 서피스 (기본: 자기 자신) |

`spawn` 은 **즉시 반환**합니다. 기다리는 명령은 따로 없고, 자식이 대기 상태가 되면 완료 알림이 옵니다 (다음 절).

부모는 자기 워크스페이스가 아닌 **다른 워크스페이스**에 자식을 두는 편이 안전합니다. 원격 mirror 워크스페이스에는 spawn 할 수 없습니다.

이후 자식에게 추가 지시를 보내거나 상태를 봅니다.

```sh
tasty claude tell "이번엔 clippy 도 돌려" --surface 57     # 여러 줄 가능, 자동 제출
tasty claude children                                       # 자식 목록 (index · surface · 상태)
tasty claude state --surface 57                             # idle / needs_input / active / exited
tasty claude broadcast "진행 상황 보고해\r" --role tester   # 역할별로 일괄 전송 (\r 로 제출)
tasty claude kill --child 0                                 # index 로 종료
tasty claude respawn --child 0 --prompt "다시 시작"          # 같은 자리에서 재시작
tasty claude parent --surface 57                            # 이 자식의 부모
```

`tasty codex …` 도 같은 서브커맨드(`tell` / `children` / `state` / `broadcast` / `kill` / `respawn` / `parent`)를 가집니다.

자식이 너무 많아지면 spawn 응답에 경고가 붙습니다. 임계치는 **설정** <!-- en: Settings --> › **플러그인** <!-- en: Plugin --> › **Claude Code** / **Codex** 의 **Spawn child 경고 임계치** <!-- en: Spawn child warning threshold --> 에서 바꿉니다 (Codex 기본 6).

## 4. 완료 알림 받기

받는 방법은 **부모 에이전트**로 정합니다. 자식이 Claude인지 Codex인지는 관계없습니다.
입력 요청·중단·오류·프로세스 종료도 상태 알림이므로, 알림을 작업 성공으로 단정하지 마세요.

### 부모가 Codex일 때

Codex 0.154.0의 **같은 서버에서 실행 중인 부모 대화**에 도구 결과로 전달합니다.
일반 `codex` TUI가 별도로 켠 서버에 자동 연결되지는 않습니다. 지원되는 기존 서버에
`codex --remote <주소>`로 TUI를 명시 연결하세요. 현재 대화를 다른 서버에 복제해서
여는 것으로 연결을 대신하지 않습니다. 이 연결은 Tasty의 SSH 워크스페이스 연결과 별개입니다.

훅 설정과 부모 연결은 별도 단계입니다. 다른 Codex home이나 profile을 쓰면 해당 위치를 지정하세요.
두 위치 옵션은 함께 사용하지 않습니다. 기존의 다른 훅과 모델 설정은 보존합니다.

```sh
tasty codex install --codex-home /absolute/codex-home
# profile 파일을 직접 선택하는 경우:
tasty codex install --config-file /absolute/codex-home/work.config.toml
```

SessionStart 훅을 받은 뒤 상태를 확인하고 연결합니다. 아래 ID 자리는 부모의 실제 값으로
바꾸세요. `hook_session`은 진단의 세션 식별값이고, `thread.id`와 `sessionId`는 부모 서버의
응답값입니다. 값이 같은 경우에도 서로 다른 식별 항목으로 검증합니다.

```sh
tasty codex completion diagnose
tasty codex completion bind --endpoint unix:///absolute/app-server.sock \
  --thread-id 'THREAD_ID' --session-id 'SESSION_ID' --hook-session 'HOOK_SESSION_ID'
tasty codex completion status
```

다른 surface에서 실행하면 `--surface <부모 ID>`를 지정합니다. 연결 검증은 비동기이며
`verifying` 뒤 `verified`인지 확인하세요. 버전·실행 대화·서버 홈이 맞지 않으면 원인이
출력되고 결과는 보존됩니다. `--codex-home`으로 기대하는 서버 home도 대조할 수 있습니다.
서버를 자동 시작하거나 현재 TUI를 몰래 재시작하지 않습니다.

Unix는 절대 소켓 경로를 사용합니다. TCP는 `ws://127.0.0.1:<port>` 또는
`ws://localhost:<port>`, TLS 연결은 `wss://<host>`를 지정합니다. Windows에서는 TCP/TLS를
사용하세요. 인증이 필요하면 `--auth-env TOKEN_ENV_NAME`으로 환경변수 **이름**을 지정합니다.
Tasty 프로세스에서 그 변수를 읽을 수 있어야 하며, TUI도 같은 인증 문맥으로 연결해야 합니다.
토큰 값을 명령 인자나 결과 본문에 넣지 마세요. 버전 기준과 실제 연결 진단을 따르며,
Linux의 Unix/TCP 검증을 다른 OS의 실행 검증으로 간주하지 않습니다.

대기 중인 부모는 새 턴으로 재개하고 일반 작업 중에는 결과가 현재 턴에 큐잉됩니다.
리뷰 중 거부 등 특수 상태는 진단을 확인하세요. 완료 알림을 `tell`이나 키 입력, 사용자
메시지로 대신 보내지 않습니다. 직접 작업 지시를 보내는 기존 `tell` 기능은 유지됩니다.

원격 daemon의 훅이 현재 Tasty 인스턴스에 SessionStart를 보낼 수 없다면, 부모 surface와 실제 hook 식별값을 직접 확인하고 bind에 `--register`를 추가합니다. 진단에는 훅 관측이 아닌 명시 등록으로 표시됩니다. 이 경우 Tasty 재시작 후에도 다시 명시 등록·바인딩해야 하며, endpoint의 실행 대화 검증은 생략하지 않습니다.

### 부모가 Claude Code일 때

기존 로그/Monitor 방식을 사용합니다.

```text
Monitor({ command: "tail -n0 -F \"$TASTY_PARENT_HOME/notify/$TASTY_SURFACE_ID.log\"", persistent: true })
```

로그 문구는 앱 언어를 따르고, 256 KiB 이상이면 비워집니다. Monitor 없이 직접 읽을 수는
있지만 자동 재개나 영구 보관을 보장하지 않습니다. 이 로그는 Codex 연결의 대체 경로가 아닙니다.

### 전달이 확인되지 않거나 구독을 끝낼 때

`status`는 이벤트별 원인·다음 재시도 시각과 구독을 보여줍니다.

| 상태 | 의미와 조치 |
|---|---|
| `pending` / `blocked` | 확정 미송신 또는 명시 거부. 원인을 고친 뒤 `tasty codex completion retry --event <ID>`로 재시도합니다. 자동 연결 재시도는 최대 8회입니다. |
| `accepted` | 서버가 수락했으며 영속·처리 완료를 보장하지 않습니다. 반복 제출하지 마세요. |
| `unknown` | 수락 여부가 불명입니다. 원본을 보존하고 같은 서버의 이력과 자동 대조합니다. 무조건 재송신하지 않습니다. |
| `recorded` | 같은 도구 결과가 저장 이력에서 확인됐습니다. 모델이 처리했다는 별도 증거는 아닙니다. |
| `unbound` | 검증된 연결이 없습니다. 바인딩과 SessionStart 관측을 확인하세요. |
| `cancelled` | 미수락 상태에서 구독이 종료돼 재시도하지 않습니다. |

불완전한 이력·알림 부재만으로 미수신을 확정하지 않습니다. 서버의 busy 큐는 중단이나
재시작 때 결과를 잃을 수 있어 수락을 완료로 간주하지 않습니다. 호스트/서버 재시작 뒤에도
같은 부모 대화로 재검증됐는지 확인하세요.

Codex 부모의 spawn 관계를 release하면 새 결과와 확정 미수락 재시도가 중단되지만 자식
터미널은 남습니다. 이미 수락했거나 수락 여부가 불명인 결과를 회수하는 동작은 아닙니다.
명시 tell 구독은 관계와 별개이며 `tasty codex completion unsubscribe --subscription <ID>`로
종료합니다. 같은 surface의 새 작업에 이전 구독을 옮기지 않습니다.

## 5. Codex 승인 정책

`tasty codex spawn/launch/respawn/reboot` 는 Codex 의 승인·샌드박스 정책을 플래그로 받습니다.

`launch`/`spawn`/`respawn` 는 sh/bash/zsh 및 Windows Git Bash에서 셸의 `codex` alias나 function을 건너뛰고 `PATH`에 설치된 Codex를 실행합니다. alias에 넣어 둔 승인·샌드박스 옵션은 아래 플래그나 전역 설정으로 지정하세요. `reboot`의 alias/function 우회는 Linux/macOS에 적용됩니다. Windows의 `reboot`는 기존 실행 방식을 유지하며 alias/function 우회는 아직 지원하지 않습니다.

- **승인**: `--approval untrusted|on-request|never`. 아무것도 안 주면 **`never`** 로 실행됩니다 — 자동화 중 승인 프롬프트에 걸려 영원히 멈추는 것을 막기 위해서입니다. 사람이 옆에서 승인해 줄 때만 `untrusted` / `on-request` 를 명시합니다.
- **샌드박스**: `--sandbox read-only|workspace-write|danger-full-access`. 안 주면 Codex 기본값. 리뷰·교차검증용 자식은 `read-only` 가 적당합니다.
- `--full-auto`: 승인과 샌드박스를 모두 우회. `--approval`/`--sandbox` 와 함께 쓸 수 없습니다.
- 전역 기본값은 **설정** › **플러그인** › **Codex** 의 **기본 승인 정책** <!-- en: Default approval policy --> / **기본 샌드박스 모드** <!-- en: Default sandbox mode -->. 호출별 플래그가 우선합니다.

컨테이너 등 중첩 샌드박스가 안 되는 환경에서 `--sandbox` 지정이 `RTM_NEWADDR: Operation not permitted` 류로 실패하면 `--full-auto` 를 씁니다. 완료 알림에도 이 힌트가 붙습니다.

## 6. Claude 권한 모드

`tasty claude launch/spawn/respawn/reboot/child-profile` 는 자식 Claude 의 권한 모드를 플래그로 받습니다.

- `--permission-mode acceptEdits|auto|bypassPermissions|manual|dontAsk|plan` — Claude Code 에 그대로 전달됩니다.
- **아무것도 안 주면 플래그가 붙지 않습니다.** 자식은 여러분이 쓰던 Claude Code 설정 그대로 뜹니다. Codex 와 달리 자동으로 "묻지 않음" 이 되지 않습니다 — Claude Code 에는 샌드박스 축이 따로 없어서, 안 묻게 만드는 순간 그것이 곧 제한 없는 실행이 되기 때문입니다.
- 자동화 중에 자식이 승인 대기로 멈추는 것이 곤란하면 그 호출에만 원하는 모드를 명시하세요. 멈춘 자식은 부모에게 알림이 가므로 눈치채지 못한 채 방치되지는 않습니다.
- 전역 기본값은 **설정** › **플러그인** › **Claude Code** 의 **자식 세션 기본 권한 모드** <!-- en: Default permission mode for child sessions -->. 기본값은 **물려받음**(플래그 미부착)이고, 호출별 플래그가 우선합니다.
- `--profile` / `--profile-file` 로 붙이는 설정 JSON 이 `permissions.defaultMode` 를 정하고 있으면 `--permission-mode` 와 함께 쓸 수 없습니다 — 둘이 같은 것을 정하므로 하나만 고르라는 에러가 납니다.
- `reboot` / `child-profile` 에서 지정한 모드는 **그 재시작에만** 적용됩니다. 탭 복원으로 다시 뜰 때는 따라가지 않습니다.

<a id="6-세션-재시작-reboot"></a>

## 7. 세션 재시작 (reboot)

훅이나 설정을 바꾼 뒤 에이전트를 같은 세션으로 다시 띄웁니다.

```sh
tasty claude reboot --surface 57 --delay 5
tasty codex reboot --surface 58
```

지정한 시간 뒤 프로세스를 끊고 같은 세션을 이어서 시작합니다. 에이전트가 **자기 자신**에게 호출할 때는 턴의 마지막 행동으로 부릅니다 — 이후 응답은 프로세스 종료로 중단됩니다. 자식 에이전트만 재시작할 때는 작업을 요청한 에이전트의 응답이 중단되지 않습니다.

<a id="7-claude-세션-프로필과-stop-게이트"></a>

## 8. Claude 세션 프로필과 Stop 게이트

Claude Code 는 훅을 시작할 때 한 번만 읽습니다. 특정 세션에만 추가 훅·권한을 붙이려면 프로필을 등록하고 실행 시 `--profile` 로 지정합니다.

```sh
tasty claude profile-register strict --file ./strict.json   # settings JSON 을 이름으로 등록
tasty claude profile-list
tasty claude spawn --workspace w --profile strict           # 이 자식에게만 적용
tasty claude reboot --profile strict                        # 이후 재시작에도 승계
tasty claude child-profile --child 0 --profile strict       # 자식에게 지속 부착
```

**Stop 게이트**를 사용하면 에이전트가 응답을 마치기 전에 체크리스트로 작업을 다시 확인하도록 할 수 있습니다. 기본 제공 게이트인 `continue-checklist`를 켜고 세션에 연결하세요.

```sh
tasty claude checklist-enable                               # 게이트 켜기 (checklist-disable 로 끔)
tasty claude spawn --workspace w --profile continue-checklist
```

- 에이전트가 응답 끝에 `[[TASTY-CHECKLIST-DONE]]` 을 넣으면 통과, 아니면 체크리스트를 다시 받습니다. 라운드 상한(기본 3)에 닿으면 자동 통과합니다.
- 상한은 **설정** › **플러그인** › **Claude Code** 의 **게이트 기본 라운드 상한** <!-- en: Default gate round limit -->.
- 자기 게이트를 만들려면 `tasty claude gate-register <이름> --body-file <파일> [--sentinel <문자열>] [--rounds N]`. 본문에 센티넬 문자열이 들어 있어야 합니다. `gate-list` / `gate-show` 로 확인.

## 문제 해결

- **완료 알림이 안 옵니다** — `tasty claude install` 을 다시 실행했는지 확인합니다. 훅 전달 실패는 `~/.tasty/hook-failures.log` 에 남습니다. 플러그인 로그는 `tasty plugin logs com.tasty.claude --follow`.
- **`reboot` 가 "claude-session-id meta not set" 으로 실패합니다** — 세션 시작 훅이 세션 ID 를 못 남긴 것입니다. `tasty surface-meta set --key claude-session-id --value <세션ID>` 로 직접 넣습니다.
- **자식이 spawn 되지 않고 "occupied" 오류** — 대상 워크스페이스가 원격에서 attach 중이거나 mirror 입니다. 다른 워크스페이스를 씁니다.
- **macOS 에서 앱 아이콘으로 실행하면 알림이 안 옵니다** — Tasty 가 알림을 쓸 때 `tasty` 를 다시 호출하는데, Tasty 는 자기 실행 파일 경로를 자동으로 PATH 에 넣으므로 보통은 문제없습니다. 그래도 안 되면 `hook-failures.log` 를 봅니다.

<a id="다음-읽을-것"></a>

## 함께 살펴보기

- [작업 순서 관리](tasks.md) — spawn 과 tell 을 의존 관계로 묶어 한 그래프로 돌리기.
- [훅 · 알림 · 웹훅](hooks-notifications.md) — 완료 통지와 승인 게이트.

Codex 연동을 다시 설치하면 SessionEnd 훅도 등록됩니다. 현재 실행 세션이 종료되면 해당 작업 구독을 끝내며, 이미 수락한 결과를 회수한 것으로 표시하지 않습니다.

부모 터미널을 닫은 뒤의 미전달·취소 기록은 `tasty codex completion status --all`로 조회할 수 있습니다.
