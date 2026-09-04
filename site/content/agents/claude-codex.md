# Claude · Codex 와 함께 쓰기

Tasty 안에서 Claude Code 와 Codex CLI 를 띄우고, 한 에이전트가 다른 에이전트를 자식으로 부려 병렬 작업을 시키는 방법을 정리한다. 훅을 한 번 설치하면 자식이 작업을 마쳤을 때 부모가 자동으로 알림을 받는다.

Claude Code 와 Codex CLI 자체는 따로 설치돼 있어야 한다. Tasty 는 실행·배치·부모-자식 관계 관리만 맡는다.

## 1. 훅 설치 (처음 한 번)

에이전트의 상태(작업 중 / 대기 / 입력 필요 / 종료)를 Tasty 가 알려면 각 CLI 의 훅 설정에 Tasty 항목이 들어가야 한다.

```sh
tasty claude install    # ~/.claude/settings.json 의 hooks 에 Tasty 항목 추가
tasty codex install     # ~/.codex/config.toml 의 [hooks] 에 Tasty 항목 추가
```

- 이미 직접 넣어 둔 훅은 그대로 보존된다. 여러 번 실행해도 중복되지 않는다.
- **Tasty 를 업데이트한 뒤에는 다시 실행한다.** 훅 명령 문자열은 설정 파일에 박히므로 새 형식을 반영하려면 재설치가 필요하다.
- Tasty 밖에서 Claude Code 를 실행할 때는 이 훅이 아무것도 하지 않는다 (조용히 통과).
- 제거는 `tasty claude uninstall` / `tasty codex uninstall`.

훅이 설치되면 다음이 자동으로 동작한다.

- 에이전트가 응답을 마치거나 질문을 던지면 그 서피스에 **주의 환기 테두리**가 켜지고 사이드바 워크스페이스에 배지가 붙는다 (질문 대기는 노란색 우선).
- 탭을 닫았다가 복원하거나 Tasty 를 재시작하면 같은 세션으로 다시 이어진다 (`claude -r` / `codex resume`).
- 자식 에이전트의 완료 알림(아래)이 부모에게 간다.

## 2. 실행하기

```sh
tasty claude launch --workspace myproj --directory ~/proj --task "테스트 고치기"
tasty codex launch --workspace review --directory ~/proj
```

새 워크스페이스를 만들고 그 터미널에서 CLI 를 실행한다. `--workspace` 를 생략하면 이름은 `claude` / `codex`.

Codex 는 `--approval untrusted|on-request|never`, `--sandbox read-only|workspace-write|danger-full-access`, `--full-auto` 로 승인·샌드박스 정책을 붙일 수 있다 (아래 "Codex 승인 정책").

## 3. 자식 에이전트 부리기 (spawn / tell)

Claude Code 세션 안에서 다음처럼 자식을 띄운다. 자식은 지정한 워크스페이스의 패인에 새 탭으로 생기고, 부모-자식 관계가 기록된다.

```sh
tasty claude spawn --workspace workers --cwd ~/proj --role tester --nickname t1 \
  --prompt "cargo test 를 돌리고 실패 원인을 보고해"
tasty codex spawn --workspace workers --cwd ~/proj --sandbox read-only \
  --prompt "방금 커밋된 diff 를 리뷰해"
```

| 옵션 | 뜻 |
|---|---|
| `--workspace <ID 또는 이름>` | 필수. 자식이 들어갈 워크스페이스 |
| `--pane <ID>` | 워크스페이스의 특정 패인 (기본: 첫 패인) |
| `--cwd <경로>` | 자식의 작업 디렉토리 |
| `--role <라벨>` | 역할 라벨. `broadcast --role` 로 골라 보낼 때 씀 |
| `--nickname <이름>` | 탭에 표시할 이름 |
| `--prompt <텍스트>` | 띄운 직후 보낼 첫 지시 |
| `--surface <ID>` | 부모 서피스 (기본: 자기 자신) |

`spawn` 은 **즉시 반환**한다. 기다리는 명령은 따로 없고, 자식이 대기 상태가 되면 완료 알림이 온다 (다음 절).

부모는 자기 워크스페이스가 아닌 **다른 워크스페이스**에 자식을 두는 편이 안전하다. 원격 mirror 워크스페이스에는 spawn 할 수 없다.

이후 자식에게 추가 지시를 보내거나 상태를 본다.

```sh
tasty claude tell "이번엔 clippy 도 돌려" --surface 57     # 여러 줄 가능, 자동 제출
tasty claude children                                       # 자식 목록 (index · surface · 상태)
tasty claude state --surface 57                             # idle / needs_input / active / exited
tasty claude broadcast "진행 상황 보고해\r" --role tester   # 역할별로 일괄 전송 (\r 로 제출)
tasty claude kill --child 0                                 # index 로 종료
tasty claude respawn --child 0 --prompt "다시 시작"          # 같은 자리에서 재시작
tasty claude parent --surface 57                            # 이 자식의 부모
```

`tasty codex …` 도 같은 서브커맨드(`tell` / `children` / `state` / `broadcast` / `kill` / `respawn` / `parent`)를 가진다.

자식이 너무 많아지면 spawn 응답에 경고가 붙는다. 임계치는 **설정** <!-- en: Settings --> › **플러그인** <!-- en: Plugin --> › **Claude Code** / **Codex** 의 **Spawn child 경고 임계치** <!-- en: Spawn child warning threshold --> 에서 바꾼다 (Codex 기본 6).

## 4. 완료 알림 받기

자식이 대기(idle) 또는 입력 필요(needs_input) 상태가 되거나 종료되면, `spawn`/`tell` 을 호출한 서피스의 **알림 로그 파일**에 한 줄이 추가된다.

```
$TASTY_PARENT_HOME/notify/$TASTY_SURFACE_ID.log
```

- 두 환경변수는 Tasty 가 띄운 셸에 이미 들어 있다. 경로를 직접 조립하지 않는다.
- 한 줄 예: `surface 57 작업 완료 (호출 방식: spawn)`.
- 자식이 살아 있는 동안 상태가 바뀔 때마다 계속 온다 — 한 번만 오는 것이 아니다.
- 파일이 256 KiB 를 넘으면 비우고 새로 쓴다.
- Claude 자식이 API 오류 뒤 30초 넘게 멈춰 있으면 같은 파일에 "멈춤" 줄이 따로 온다.

Claude Code 세션이 부모일 때는 이 파일을 Monitor 도구로 한 번만 걸어 두면 이후 모든 자식의 완료가 알림으로 도착한다.

```
Monitor({ command: "tail -n0 -F \"$TASTY_PARENT_HOME/notify/$TASTY_SURFACE_ID.log\"", persistent: true })
```

Monitor 를 쓸 수 없는 환경에서는 파일을 직접 읽는다 (`tail -f`). 전달이 수십 초 늦어질 수는 있어도 사라지지는 않는다.

Codex 자식은 `needs_input` 알림이 없다 (Codex CLI 에 해당 이벤트가 없다). 승인 프롬프트에서 멈추면 아무도 모르므로 아래 승인 정책이 중요하다.

## 5. Codex 승인 정책

`tasty codex spawn/launch/respawn/reboot` 는 Codex 의 승인·샌드박스 정책을 플래그로 받는다.

- **승인**: `--approval untrusted|on-request|never`. 아무것도 안 주면 **`never`** 로 실행된다 — 자동화 중 승인 프롬프트에 걸려 영원히 멈추는 것을 막기 위해서다. 사람이 옆에서 승인해 줄 때만 `untrusted` / `on-request` 를 명시한다.
- **샌드박스**: `--sandbox read-only|workspace-write|danger-full-access`. 안 주면 Codex 기본값. 리뷰·교차검증용 자식은 `read-only` 가 적당하다.
- `--full-auto`: 승인과 샌드박스를 모두 우회. `--approval`/`--sandbox` 와 함께 쓸 수 없다.
- 전역 기본값은 **설정** › **플러그인** › **Codex** 의 **기본 승인 정책** <!-- en: Default approval policy --> / **기본 샌드박스 모드** <!-- en: Default sandbox mode -->. 호출별 플래그가 우선한다.

컨테이너 등 중첩 샌드박스가 안 되는 환경에서 `--sandbox` 지정이 `RTM_NEWADDR: Operation not permitted` 류로 실패하면 `--full-auto` 를 쓴다. 완료 알림에도 이 힌트가 붙는다.

## 6. 세션 재시작 (reboot)

훅이나 설정을 바꾼 뒤 에이전트를 같은 세션으로 다시 띄운다.

```sh
tasty claude reboot --surface 57 --delay 5
tasty codex reboot --surface 58
```

지정한 시간 뒤 프로세스를 끊고 같은 세션을 이어서 시작한다. 에이전트가 **자기 자신**에게 호출할 때는 턴의 마지막 행동으로 부른다 — 그 뒤 내용은 잘린다. 자식에게는 부모 턴이 잘리지 않는다.

## 7. Claude 세션 프로필과 Stop 게이트

Claude Code 는 훅을 시작할 때 한 번만 읽는다. 특정 세션에만 추가 훅·권한을 붙이려면 프로필을 등록하고 실행 시 `--profile` 로 지정한다.

```sh
tasty claude profile-register strict --file ./strict.json   # settings JSON 을 이름으로 등록
tasty claude profile-list
tasty claude spawn --workspace w --profile strict           # 이 자식에게만 적용
tasty claude reboot --profile strict                        # 이후 재시작에도 승계
tasty claude child-profile --child 0 --profile strict       # 자식에게 지속 부착
```

**Stop 게이트**는 에이전트가 턴을 끝내려 할 때 체크리스트를 주입해 스스로 재검토하게 만드는 장치다. 내장 게이트 `continue-checklist` 를 켜고 붙인다.

```sh
tasty claude checklist-enable                               # 게이트 켜기 (checklist-disable 로 끔)
tasty claude spawn --workspace w --profile continue-checklist
```

- 에이전트가 응답 끝에 `[[TASTY-CHECKLIST-DONE]]` 을 넣으면 통과, 아니면 체크리스트를 다시 받는다. 라운드 상한(기본 3)에 닿으면 자동 통과한다.
- 상한은 **설정** › **플러그인** › **Claude Code** 의 **게이트 기본 라운드 상한** <!-- en: Default gate round limit -->.
- 자기 게이트를 만들려면 `tasty claude gate-register <이름> --body-file <파일> [--sentinel <문자열>] [--rounds N]`. 본문에 센티넬 문자열이 들어 있어야 한다. `gate-list` / `gate-show` 로 확인.

## 문제 해결

- **완료 알림이 안 온다** — `tasty claude install` 을 다시 실행했는지 확인한다. 훅 전달 실패는 `~/.tasty/hook-failures.log` 에 남는다. 플러그인 로그는 `tasty plugin logs com.tasty.claude --follow`.
- **`reboot` 가 "claude-session-id meta not set" 으로 실패한다** — 세션 시작 훅이 세션 ID 를 못 남긴 것이다. `tasty surface-meta set --key claude-session-id --value <세션ID>` 로 직접 넣는다.
- **자식이 spawn 되지 않고 "occupied" 오류** — 대상 워크스페이스가 원격에서 attach 중이거나 mirror 다. 다른 워크스페이스를 쓴다.
- **macOS 에서 앱 아이콘으로 실행하면 알림이 안 온다** — Tasty 가 알림을 쓸 때 `tasty` 를 다시 호출하는데, Tasty 는 자기 실행 파일 경로를 자동으로 PATH 에 넣으므로 보통은 문제없다. 그래도 안 되면 `hook-failures.log` 를 본다.

## 다음 읽을 것

- [작업 DAG](tasks.md) — spawn 과 tell 을 의존 관계로 묶어 한 그래프로 돌리기.
- [훅 · 알림 · 웹훅](hooks-notifications.md) — 완료 통지와 승인 게이트.
