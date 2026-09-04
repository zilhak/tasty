# tasty CLI 로 터미널 조작하기

`tasty` 명령으로 실행 중인 Tasty 의 터미널을 밖에서 조작합니다. 서피스 목록을 보고, 명령을 보내고, 그 결과만 잘라 읽는 기본 패턴을 익힙니다.

AI 코딩 에이전트(Claude Code, Codex 등)가 Tasty 안의 터미널에서 돌 때 자기 옆 터미널을 다루는 도구가 바로 이 CLI 입니다. 사람이 스크립트에서 써도 똑같이 동작합니다.

## 준비

- Tasty 가 실행 중이어야 합니다. CLI 는 `~/.tasty/tasty.port` 에 적힌 포트로 실행 중인 인스턴스에 접속합니다.
- Tasty 가 띄운 터미널 안에서는 `tasty` 가 이미 PATH 에 있습니다. 밖(다른 터미널 앱)에서 쓰려면 Tasty 실행 파일이 있는 경로를 PATH 에 넣습니다. 설치 방식별 경로는 [설치](../getting-started/install.md#설치-위치) 에 있습니다.
- Tasty 가 띄운 셸에는 `TASTY_SURFACE_ID` 환경변수가 들어 있습니다. `--surface` 를 생략한 명령은 이 값을 쓰므로, 자기 터미널을 조작할 때는 ID 를 적지 않아도 됩니다.

```sh
echo $TASTY_SURFACE_ID     # 예: 42
tasty list info            # 버전·워크스페이스 수 등 — 접속 확인용
```

## 용어와 ID

Tasty 의 화면은 **워크스페이스 > 패인 > 탭 > 서피스** 순으로 겹쳐 있습니다. 서피스(surface)가 터미널 하나입니다. CLI 의 모든 대상은 이 ID 로 직접 지정합니다 — 어느 창이 포커스돼 있든 결과가 같습니다.

```sh
tasty list tree            # 전체 계층을 트리로
tasty list workspaces      # 워크스페이스 목록
tasty list surfaces        # 서피스(터미널) 목록 — 전 워크스페이스
tasty list panes           # 패인 목록
tasty list tabs --pane 3   # 특정 패인의 탭
```

`list tree` 는 분할 구조까지 보여줍니다. 포커스된 서피스에는 `*focus` 가 붙습니다.

```
└─ vertical (L|R) 60:40
   ├─ surface:396 (terminal)
   └─ horizontal (T|B) 50:50
      ├─ surface:417 (terminal) *focus
      └─ surface:418 (markdown)
```

`list workspaces` 의 행은 `이름 (id:N) (패인 수)` 형식입니다. 활성 워크스페이스에는 `*`, 원격 mirror 에는 `[mirror]` 가 붙습니다 ([원격 attach](../remote/attach.md)).

## 기본 패턴: 마크 → 보내기 → 마크 이후 읽기

명령 하나의 결과만 깔끔히 꺼내는 표준 절차입니다.

1. `tasty set mark` — 지금 출력 위치에 표시를 남깁니다.
2. `tasty send text "명령\r"` — 텍스트를 보냅니다. `\r` 이 Enter 입니다.
3. 잠시 기다린 뒤 `tasty read since-mark --strip-ansi` — 표시 이후에 나온 출력만 읽습니다.

```sh
tasty set mark --surface 42
tasty send text "cargo test 2>&1 | tail -20\r" --surface 42
sleep 5
tasty read since-mark --surface 42 --strip-ansi
```

- `send text` 는 `\r` `\n` `\t` `\\` `\0` 이스케이프를 해석합니다. 셸 따옴표 안에 그대로 적으면 됩니다.
- `--strip-ansi` 를 붙이면 색상 등 제어 시퀀스를 걷어낸 순수 텍스트가 옵니다. 파싱할 때는 항상 붙입니다.
- 마크는 `set mark` 를 다시 부를 때까지 유지됩니다. `read since-mark` 는 마크를 옮기지 않으므로 여러 번 읽어도 같은 구간이 나옵니다.

명령이 끝났는지 모를 때는 화면을 읽어 프롬프트가 돌아왔는지 봅니다.

```sh
tasty read screen --surface 42 --lines 5     # 화면 하단 5줄 (부족하면 스크롤백까지)
tasty is-typing --surface 42                  # 최근 5초 내 사람이 키를 눌렀는지
```

`read screen` 은 기본적으로 흐리게 표시되는 자동완성 제안(예: Claude Code 의 회색 제안 텍스트)을 제외합니다. 포함하려면 `--show-dim`.

`--lines N` 을 줬는데 N 줄보다 적게 왔다면, 응답의 `scrollback_len` 이 이유를 알려줍니다. `0` 이면 그 화면이 가진 전부라 더 줄 것이 없습니다 — 전체 화면 앱(TUI)이 뜨자마자 화면을 차지하면 이렇게 됩니다. `0` 이 아닌데도 적게 왔다면 그때는 이상한 것입니다. `alt_screen` 은 지금 전체 화면 앱이 떠 있는지를 알려줍니다.

## 키 보내기

Enter 외의 키는 `send key` 로 보냅니다.

```sh
tasty send key enter --surface 42
tasty send key ctrl+c --surface 42
tasty send key escape --surface 42
tasty send key up --surface 42
```

키 이름: `enter` `tab` `escape`(또는 `esc`) `backspace` `delete` `insert` `up` `down` `left` `right` `home` `end` `pageup` `pagedown` `f1`~`f12`. 조합은 `ctrl+c` `alt+x` `ctrl+shift+c` 처럼 `+` 로 잇습니다.

## 셸 통합이 있으면: 명령 단위로 읽기

bash / zsh 는 Tasty 가 셸 통합을 자동으로 넣어 주므로, 실행한 명령과 종료 코드를 명령 단위로 조회할 수 있습니다.

```sh
tasty read commands --surface 42       # 기록된 명령 목록
tasty read last-command --surface 42   # 마지막 명령 (명령 문자열·종료 코드)
tasty read command-at --surface 42 --index -1   # 뒤에서 첫 번째 (0부터, 음수는 끝에서)
```

fish 등 다른 셸은 직접 셸 통합을 설치하지 않으면 빈 목록이 나옵니다.

## 새 터미널 만들기·닫기

```sh
tasty new workspace --name build --cwd ~/proj          # 새 워크스페이스
tasty split --level surface --target-surface this --direction vertical   # 내 서피스를 좌우 분할
tasty split --level pane --target-pane 3 --direction horizontal          # 패인 분할
tasty new tab --pane 3 --cwd ~/proj                     # 패인에 새 탭
tasty close surface --surface 99                        # 서피스 닫기
tasty close tab --tab 12
tasty close workspace --id 3                            # 워크스페이스를 통째로 (안의 탭·서피스까지)
tasty close window --id 1                               # 창 닫기
tasty close self                                        # 지금 이 서피스 닫기
```

`--target-surface this` 는 자기 자신(`TASTY_SURFACE_ID`)입니다. `--type markdown --file README.md` 처럼 터미널이 아닌 표면도 만들 수 있습니다 ([파일 열기](../using/files.md)).

워크스페이스와 창은 마지막 하나를 닫지 못합니다. 워크스페이스를 닫으면 창까지 사라지는 것이 아니라
거절되므로, 창을 없앨 생각이면 `tasty close window` 를 따로 씁니다. 자기 터미널이 들어 있는 대상은
닫히지 않습니다 — 그때는 `tasty close self` 입니다. 원격에 접속해 미러로 띄운 워크스페이스도 닫지 못합니다 —
그건 접속을 끊어서 정리합니다. 반대로 **누군가 원격에서 접속해 지금 쓰고 있는 터미널**이 들어 있는
워크스페이스도 닫지 못합니다 — 그 사람이 접속을 놓은 뒤에야 닫힙니다. 보고 있지 않은 워크스페이스를 닫아도 화면에 떠 있는 워크스페이스는
그대로 남습니다.

**워크스페이스 닫기는 되돌릴 수 없습니다.** 안에서 돌던 터미널이 전부 종료되고, "닫은 항목" 으로
되살릴 수 없으며, 스크롤백도 남지 않습니다. 되살릴 수 있는 건 사람이 자기 손으로 닫은 것뿐입니다.
지울 게 맞는지는 `tasty list workspaces` 로 먼저 확인합니다.

## 서피스 들여다보기

```sh
tasty surface cursor-position --surface 42     # 커서가 몇 행 몇 열에 있나
tasty surface foreground-process --surface 42  # 지금 앞에서 도는 프로그램 (셸이면 유휴)
tasty surface locate --surface 42              # 이 서피스가 속한 패인, 그리고 아직 살아 있는지
tasty surface respawn-terminal --surface 42    # 자리를 유지한 채 셸만 다시 띄우기
tasty surface fire-hook --surface 42 --event process-exit    # 훅을 직접 발화
tasty surface fire-hook --surface 42 --event idle-timeout:300 # 초 단위가 붙는 이벤트도 있음
```

## 사람이 타이핑 중이면 보내지 않기

```sh
tasty send text "make test\r" --surface 42 --wait-idle
```

`--wait-idle` 은 판정과 전송을 한 번에 합니다. `tasty is-typing` 으로 먼저 확인하고 보내면 그 사이에
사람이 타이핑을 시작할 수 있는데, 이 플래그는 그 틈을 없앱니다. 타이핑 중이면 보내지 않고
`"sent": false` 와 이유를 돌려줍니다.

## 자식 에이전트에게 권한 주기

```sh
tasty session issue --agent-id build-bot --permission surface.read --permission terminal.write
tasty session list
tasty session revoke --token <토큰>
```

발급한 토큰을 자식이 `TASTY_SESSION_TOKEN` 으로 들고 있으면 거기 적힌 권한만 쓸 수 있습니다.

## 알림 보내기

긴 작업이 끝났을 때 사람에게 알립니다. 알림 패널과 OS 알림으로 나갑니다.

```sh
tasty notify "빌드 완료" --title "cargo"
tasty list notifications
```

자세한 알림 동작과 자동 실행(훅)은 [훅 · 알림 · 웹훅](hooks-notifications.md).

## 서피스에 메모 남기기 (메타데이터)

서피스마다 키-값을 붙여 둘 수 있습니다. 여러 에이전트가 역할을 표시하거나 상태를 주고받을 때 씁니다.

```sh
tasty surface-meta set --key role --value builder --surface 42
tasty surface-meta get --key role --surface 42
tasty surface-meta list --surface 42
tasty surface-meta unset --key role --surface 42
```

## 서피스끼리 메시지 주고받기 (큐)

터미널 입력을 건드리지 않고 서피스 간에 메시지를 전달하는 큐입니다.

```sh
tasty send queue --to 42 "테스트 끝났음, 결과 확인 바람"
tasty list queue --surface 42            # 대기 건수·미리보기
tasty read queue --surface 42            # 가장 오래된 메시지 하나 꺼냄
tasty read queue --surface 42 --peek     # 꺼내지 않고 보기
tasty read queue --surface 42 --clear    # 전부 비움
```

## 자주 쓰는 명령 표

| 하고 싶은 것 | 명령 |
|---|---|
| 계층 구조 보기 | `tasty list tree` |
| 서피스 목록 | `tasty list surfaces` |
| 텍스트 보내기 (Enter 포함) | `tasty send text "ls\r" --surface ID` |
| 키 보내기 | `tasty send key enter --surface ID` |
| 마크 찍기 | `tasty set mark --surface ID` |
| 마크 이후 읽기 | `tasty read since-mark --surface ID --strip-ansi` |
| 화면 읽기 | `tasty read screen --surface ID --lines N` |
| 알림 | `tasty notify "본문" --title "제목"` |
| 스크린샷 | `tasty screenshot --path out.png [--surface ID] [--window ID]` |
| 도움말 | `tasty --help`, `tasty <명령> --help`, `tasty -a -h` (전체 트리) |

## 문제 해결

- **연결이 안 됩니다** — Tasty 가 실행 중인지, `~/.tasty/tasty.port` 파일이 있는지 확인합니다. 파일이 남아 있는데 접속이 안 되면 이전 인스턴스가 비정상 종료된 것입니다 ([문제 해결](../help/troubleshooting.md)).
- **`--surface` 없이 부르면 거부됩니다** — `TASTY_SURFACE_ID` 가 없는 셸(Tasty 밖)에서는 대상 서피스를 알 수 없어 명령이 오류로 끝납니다. Tasty 는 포커스된 서피스로 추측하지 않습니다 — 어느 창이 앞에 나와 있든 같은 명령은 같은 결과를 냅니다. 스크립트에서는 항상 `--surface` 를 적습니다.
- **`read since-mark` 가 비어 있습니다** — 마크를 찍기 전에 출력이 끝났거나, 명령이 아직 안 끝난 것입니다. `read screen` 으로 현재 상태를 봅니다.
- **`screenshot` 이 어느 창을 찍는지 모르겠습니다** — 자동 선택은 **메인 창(터미널 창)만** 셉니다. 메인 창이 하나면 `--window` 없이 그 창을 찍고, 메인 창이 여럿이면 `--window` 가 필수입니다(포커스된 창을 임의로 고르지 않습니다). 설정 창처럼 `list windows` 에 안 나오는 창은 이 계산에 들어가지 않습니다 — 설정 창이 떠 있어도 `--window` 없이 메인 창이 찍히며, 설정 창 자체를 찍으려면 `--window` 로 그 ID 를 직접 적습니다.

## 다음 읽을 것

- [Claude · Codex 와 함께 쓰기](claude-codex.md) — 자식 에이전트를 띄우고 완료를 통지받기.
- [작업 DAG](tasks.md) — 여러 작업을 의존 관계로 묶어 순서대로 실행하기.
- [훅 · 알림 · 웹훅](hooks-notifications.md) — 이벤트로 명령을 자동 실행하기.
