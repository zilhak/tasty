# 원격 attach

다른 머신에서 실행 중인 Tasty 의 워크스페이스를 내 Tasty 안에 **mirror** 로 띄워 보고 조작합니다. 연결·인증은 전부 SSH 에 맡기므로, 그 머신에 SSH 로 들어갈 수 있으면 attach 도 됩니다.

## 개념

- attach 는 **이미 떠 있는 원격 Tasty** 의 워크스페이스를 가져와 보는 것입니다. 새 원격 셸을 여는 게 아닙니다. 원격 머신에 Tasty 가 실행 중이어야 합니다 (GUI 든 `tasty --headless` 든).
- 원격 워크스페이스는 mirror 로 가져온 쪽이 **독점**합니다. 그동안 원격 쪽 사용자는 그 터미널을 볼 수는 있지만 입력할 수 없습니다 (읽기 전용). 연결을 끊으면 돌아옵니다.
- mirror 워크스페이스는 사이드바에 하늘색 **원격** <!-- en: REMOTE --> 표시가 붙습니다. 재시작하면 사라지며 저장되지 않습니다.
- 원격 Tasty 는 PATH 에 없어도 됩니다. 포트 파일(`~/.tasty/tasty.port`)만 읽을 수 있으면 됩니다.

## 1. 프로필 만들기

프로필은 두 종류입니다. 둘 다 `~/.tasty/remote-profiles.toml` 에 저장됩니다.

| 종류 | 담는 것 | 쓰임 |
|---|---|---|
| `ssh` | 호스트·사용자·포트·identity 같은 **접속 정보만** | `tasty tool ssh` 접속, attach 프로필이 참조 |
| `tasty-attach` | attach 스펙 — 연결(ssh 프로필 참조 또는 직접 입력) + 원격 tasty 경로·포트 발견 방식 | attach·원격 조회는 **이 종류만** 씁니다 |

`ssh` 프로필로 직접 attach 하려 하면 거부됩니다. 반드시 `tasty-attach` 프로필을 하나 만듭니다.

### GUI 에서

1. 사이드바의 도구 버튼 → **도구** <!-- en: Tools --> 메뉴 → **원격 접속…** <!-- en: Remote connections… -->.
2. **원격 접속 프로필** <!-- en: Remote profiles --> 탭 → **+ 프로필 추가** <!-- en: + Add profile -->. 이름·호스트·사용자·포트·셸을 적고 저장합니다. 셸을 `auto` 로 두면 저장 시 SSH 로 한 번 접속해 셸을 감지합니다.
   - `~/.ssh/config` 에 이미 있는 호스트는 탭 아래 **로컬 SSH config** <!-- en: Local SSH config --> 섹션에서 **tasty 프로필로 가져오기** <!-- en: Import as tasty profile --> 로 바로 만듭니다. 별칭만 저장되므로 ssh config 를 고쳐도 그대로 따라갑니다.
   - 키 파일은 **Passkey** 탭에 먼저 등록하고 프로필의 Passkey 드롭다운에서 고릅니다. 비밀 값은 `~/.tasty/passkeys.toml` 에 경로로만 참조됩니다.
3. **Attach** 탭 → **+ Attach 추가** <!-- en: + Add attach -->. 이름을 적고 **연결** <!-- en: Connection --> 에서 **SSH 프로필** (위에서 만든 것 참조) 또는 **직접 입력(인라인)** 을 고릅니다.
   - **원격 TASTY** <!-- en: Remote tasty --> 그룹: **실행 파일** (원격 tasty 경로, 기본 `tasty`) · **포트 모드** · **포트 파일**. 원격 PATH 에 tasty 가 없으면 실행 파일에 전체 경로를 적거나 포트 모드를 `file-unix` 로 둡니다.

### CLI 에서

```sh
# ssh 프로필
tasty tool remote-profile add-ssh --name gx10 --host 10.0.0.5 --user me --port 22 --identity ~/.ssh/id_ed25519
tasty tool remote-profile list-local                      # ~/.ssh/config 별칭 목록
tasty tool remote-profile import --from devbox --name devbox   # 별칭을 ssh 프로필로

# tasty-attach 프로필 (ssh 프로필 참조)
tasty tool remote-profile add-attach --name gx10-attach --ssh-ref gx10 \
  --remote-tasty /home/me/tasty/target/release/tasty --port-mode auto

# 확인·수정·삭제
tasty tool remote-profile list [--kind ssh|tasty-attach]
tasty tool remote-profile show --name gx10-attach
tasty tool remote-profile edit --name gx10-attach --port-mode file-unix
tasty tool remote-profile detect --name gx10-attach       # 원격에 실제로 접속해 포트 검증
tasty tool remote-profile remove --name gx10-attach
```

포트 모드:

| 값 | 동작 |
|---|---|
| `auto` (기본) | `subcommand` → `file-unix` → `file-windows` 순서로 시도 |
| `subcommand` | 원격에서 `<실행 파일> port` 를 실행해 포트를 얻습니다 |
| `file-unix` | 원격의 `~/.tasty/tasty.port` 를 읽습니다 (tasty 실행 없음) |
| `file-windows` | Windows 원격의 포트 파일을 읽습니다 |

`--port-file <경로>` 를 주면 그 파일을 최우선으로 읽습니다.

원격 머신에서 지금 떠 있는 인스턴스의 IPC 포트를 직접 확인하려면 그 머신에서 `tasty port` 를 실행합니다 — 포트 모드 `subcommand` 가 원격에서 부르는 것이 이것입니다.

## 2. 원격이 살아 있는지 확인

```sh
tasty remote check --profile gx10-attach       # alive: gx10 (port 41234, version …, N workspaces)
tasty remote workspaces --profile gx10-attach  # 원격 워크스페이스 목록 (id·이름·점유 여부)
```

`remote check` 는 포트를 찾은 뒤 실제로 응답까지 받아야 alive 로 판정합니다. 포트 파일만 남은 죽은 인스턴스는 dead 로 나옵니다. 실패 원인은 SSH 연결 실패 / 원격 인스턴스 미실행 / 응답 해석 실패 / 타임아웃 네 가지로 구분해 알려 줍니다.

연결 시도는 무한정 기다리지 않습니다 — SSH 접속 10초, 단계당 20초, 전체 45초 상한. 느린 회선이면 ssh 프로필의 `--option ConnectTimeout=30` 처럼 직접 늘립니다.

## 3. GUI 로 attach 하기

1. 사이드바에서 **새 워크스페이스(+) 버튼을 우클릭**하거나 빈 배경을 우클릭 → **원격 워크스페이스 추가** <!-- en: Add remote workspace -->. (카테고리를 켰다면 카테고리 헤더 우클릭에도 있습니다.)
2. 왼쪽 **Attach 프로필** <!-- en: Attach profiles --> 에서 프로필을 고릅니다. 오른쪽에 원격 워크스페이스 목록이 뜹니다 (20초 안에 응답이 없으면 중단하고 **다시 시도** 를 보여 줍니다).
3. 워크스페이스를 고르고 **연결** <!-- en: Connect -->. 다른 사람이 이미 붙어 있는 것은 **사용 중** <!-- en: in use --> 으로 표시되고 고를 수 없습니다.
   - 목록 첫 행 **새 워크스페이스** <!-- en: New workspace --> 를 고르면 원격에 기본 이름으로 워크스페이스를 만든 뒤 붙습니다 (**만들고 연결** <!-- en: Create & connect -->). 이렇게 만든 워크스페이스는 원격에 남습니다.
4. 사이드바에 **원격** 표시가 붙은 mirror 워크스페이스가 생기고 포커스가 옮겨갑니다.

## 4. CLI 로 attach 하기

```sh
tasty tool attach --list                                   # tasty-attach 프로필 목록
tasty tool attach gx10-attach --workspace 3                # 워크스페이스째 mirror (터미널에서 실행)
tasty tool attach gx10-attach 57                           # 서피스 하나만
tasty tool attach gx10-attach 57 --raw                     # 내 터미널을 원격 서피스에 직결 (나가기 Ctrl+\)
tasty remote attach --profile gx10-attach --workspace 3    # tool attach 와 같은 일의 긴 형태
tasty remote attach --ssh me@10.0.0.5 --workspace 3        # 프로필 없이 1회성
tasty remote new-workspace --profile gx10-attach --name build --cwd /home/me/proj   # 원격에 워크스페이스 생성
```

- 워크스페이스 attach 는 그 안의 터미널을 분할 구조까지 그대로 mirror 합니다. 마크다운·HTML 같은 비터미널 표면은 자리만 잡고 내용은 보이지 않습니다 (탐색기는 둘러보기만 가능).
- `--raw` 는 서피스 단위에서만 됩니다.
- `--no-reconnect` 를 주지 않으면 SSH 가 끊겼을 때 자동으로 재연결을 시도합니다.

실행 중인 Tasty 창 안에 mirror 워크스페이스로 띄우려면 아래 "워크스페이스에 자동 attach 걸기" 를 씁니다.

## 5. 워크스페이스에 자동 attach 걸기

로컬 워크스페이스에 원격 대상을 매핑해 두면, 그 워크스페이스로 전환할 때마다 Tasty 가 알아서 SSH 터널을 세우고 mirror 를 붙입니다.

```sh
tasty new workspace --name gx10-dev --ssh-profile gx10-attach --remote-workspace 3
tasty set workspace --id 5 --ssh-profile gx10-attach --remote-workspace 3
tasty set workspace --id 5 --ssh me@10.0.0.5 --remote-workspace 3      # 프로필 없이
tasty set workspace --id 5 --clear-mapping                              # 해제
```

- `--remote-workspace` 는 원격 워크스페이스 **ID** 입니다. `tasty remote workspaces` 로 확인합니다.
- 매핑된 mirror 는 연결이 끊겨도 워크스페이스와 스크롤백을 그대로 둔 채 백그라운드에서 재연결합니다 (0.5초에서 30초까지 간격을 늘리며 시도, 원격이 다른 사람에게 점유돼 있으면 30초 간격). 20회 실패하면 멈추고 토스트로 알립니다 — 그 워크스페이스를 나갔다 다시 들어오면 즉시 한 번 더 시도합니다.

## mirror 안에서 할 수 있는 것

- 키 입력·마우스는 원격 터미널로 그대로 갑니다. 크기는 내 페인 크기에 맞춰 원격이 다시 배치합니다.
- 분할, 새 탭, 탭 닫기·이동, 표면 변환은 **원격에서 실행**되고 결과가 mirror 에 반영됩니다. 원격에 없는 종류의 표면을 만들면 토스트로 실패를 알립니다.
- 원격 터미널의 완료·입력 필요 표시(테두리·배지)도 mirror 에 그대로 옵니다.
- 클립보드 이미지를 붙여넣으면 원격으로 업로드되고 **원격 경로**가 입력됩니다. 텍스트 붙여넣기는 그대로.
- mirror 워크스페이스에 `tasty claude spawn` 등으로 자식 에이전트를 만들 수는 없습니다. 원격 인스턴스에서 직접 띄웁니다.
- mirror 의 마지막 터미널을 닫으면 원격 워크스페이스 자체가 사라지고 연결이 끊깁니다.

## 점유 풀기 (원격 쪽에서)

내 Tasty 의 워크스페이스가 다른 곳에서 attach 돼 읽기 전용이 됐을 때, 서피스에 **강제 끊기** <!-- en: Force detach --> 안내가 뜹니다. CLI 로도 끊을 수 있습니다.

```sh
tasty remote attach --force-detach --workspace 3    # 이 인스턴스의 워크스페이스 3 점유 해제
tasty remote attach --force-detach 57               # 서피스 57
```

원격 쪽 사용자가 붙어 있는 동안 그 워크스페이스에 로컬에서 분할·탭 생성·`spawn` 을 하려 하면 "점유 중" 오류로 거부됩니다. 다른 워크스페이스를 쓰거나 강제 끊기 후 진행합니다.

## 원격에서 받은 파일 저장 위치

attach 채널로 전송된 파일은 `~/.tasty/transfers/` 에 저장되며 폴더 상한은 500 MiB 입니다. 설정 창에는 아직 항목이 없고 CLI 로 바꿉니다.

```sh
tasty settings get-remote-transfer
tasty settings set-remote-transfer --dir ~/Downloads/tasty --max-mb 2000
```

## 문제 해결

| 증상 | 확인할 것 |
|---|---|
| `kind='ssh'` 거부 | `--profile` 에 ssh 프로필을 줬습니다. `tasty-attach` 프로필을 만들어 지정합니다 |
| 원격 tasty 미발견 | 원격에 Tasty 가 실행 중인가. `tasty tool remote-profile detect --name <n>` 으로 포트 검증. PATH 에 없으면 실행 파일 경로 지정 또는 `--port-mode file-unix` |
| 타임아웃 | 호스트 도달성·방화벽. ssh 프로필 `--option ConnectTimeout=<초>` |
| SSH 연결 실패 | 인증·호스트 키. `tasty tool ssh <ssh 프로필>` 로 먼저 접속되는지 봅니다 |
| 워크스페이스 attach 거부 | 그 안의 터미널 하나가 이미 다른 client 에 점유돼 있습니다. 원격에서 강제 끊기 |
| 처음 붙을 때 화면이 잠깐 깜빡임 | 원격이 내 페인 크기로 다시 배치하는 동안의 정상 동작 |
