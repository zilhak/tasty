# 문제 해결

Tasty 를 쓰다 막혔을 때 증상별로 원인과 해결 방법을 찾는 페이지다. 어디에 무엇이 기록되는지부터 알아두면 대부분의 문제는 파일 하나를 열어보는 것으로 좁혀진다.

## 먼저 볼 파일

모두 `~/.tasty/` 아래에 있다 (Windows 는 `%USERPROFILE%\.tasty\`).

| 파일 | 내용 |
|---|---|
| `config.toml` | 설정. 설정 창에서 저장한 값이 여기 들어간다 |
| `tasty.port` | 실행 중인 Tasty 의 IPC 포트 번호. Tasty 가 뜰 때 만들어진다 |
| `debug.log` | 직전 실행의 경고 이상 로그. Tasty 를 다시 켜면 비워진다 |
| `crash-reports/crash-*.log` | 크래시 리포트 — 버전 · OS · 위치 · 메시지 · backtrace |
| `crash-reports/hang-*.log` | 창이 5초 넘게 멈췄을 때 자동으로 남는 기록 |
| `hook-failures.log` | Claude / Codex 훅이 Tasty 에 닿지 못한 기록 |
| `plugins-logs/<플러그인 id>.log` | 플러그인별 로그. `tasty plugin logs <id>` 로도 본다 |
| `state.db` | 최근 파일 등 앱이 자동으로 관리하는 데이터 |

더 자세한 로그가 필요하면 터미널에서 로그 레벨을 올려 실행한다. `RUST_LOG` 가 아니라 `TASTY_LOG` 다.

```sh
TASTY_LOG=debug tasty 2> tasty.log
```

## 설치 · 첫 실행

- **Windows: "Windows 의 PC 보호" 경고가 뜬다** — 코드 서명이 없어서다. **추가 정보 → 실행** 을 누른다.
- **macOS: "확인되지 않은 개발자" 라며 열리지 않는다** — Gatekeeper 다. **시스템 설정 > 개인정보 보호 및 보안** 에서 허용하거나, 터미널에서 `xattr -dr com.apple.quarantine /Applications/Tasty.app` 을 실행한다.
- **Linux: AppImage 가 실행되지 않는다** — 실행 권한이 없거나 FUSE 가 없다. `chmod +x Tasty-*.AppImage` 를 먼저 하고, 그래도 안 되면 `./Tasty-*.AppImage --appimage-extract-and-run` 으로 실행한다.
- **Linux: `GLIBC_2.39 not found` 로 실행되지 않는다** — Ubuntu 20.04 · Debian 11 처럼 빌드 기준(Ubuntu 24.04)보다 오래된 배포판이다. 구배포판용 빌드는 없다.
- **Linux `.tar.gz`: 라이브러리가 없다며 종료된다** — `tasty` 가 빠진 라이브러리를 안내하고 끝난다. 안내된 패키지(`libfreetype6` · `libfontconfig1` · `libgtk-3` · `libwebkit2gtk-4.1` 등)를 설치한다. 자동으로 끌어오게 하려면 `.deb` / `.rpm` 을 쓴다.
- **"GPU 어댑터를 찾을 수 없음" 이 뜨고 종료된다** — GPU 드라이버(Vulkan / DirectX 12 / Metal)가 없다. 드라이버를 설치·업데이트한다. Linux 는 `libvulkan1` / `vulkan-loader` 가 있으면 GPU 가속, 없으면 소프트웨어 렌더링으로 뜬다. GPU 가 아예 없는 서버 · VM 에서는 배포 파일로 실행할 수 없다.
- **Windows: "Git Bash를 찾을 수 없습니다"** — Tasty 는 Windows 에서 Git Bash 를 셸로 쓴다. Git for Windows 를 설치하거나 **설정** <!-- en: Settings --> > **터미널** <!-- en: Terminal --> > **셸** <!-- en: Shell --> 에서 bash 경로를 직접 지정한다.
- **"데이터베이스 초기화 오류" 로 시작하자마자 종료된다** — 본문을 본다. "DB가 잠겨 있습니다" 면 다른 Tasty 가 이미 떠 있다. "손상되었습니다" / "스키마 버전이 맞지 않습니다" 면 `~/.tasty/state.db` 를 백업한 뒤 지우면 새로 시작된다. 최근 파일 목록만 사라진다.
- **Tasty 터미널 안에서 `tasty` 를 쳤는데 새 창이 안 뜬다** — Tasty 안에서 인자 없이 실행하면 새 창 대신 도움말을 보여준다. 새 창은 `tasty new window`, GUI 를 강제로 띄우려면 `tasty --launch`.

설치 절차 자체는 [설치](../getting-started/install.md) 에 있다.

## macOS 권한 프롬프트

**증상** — 첫 실행 직후 권한 프롬프트가 연달아 뜬다. 순서는 다운로드 · 문서 · 데스크탑 폴더 → (연결돼 있으면) 외장 · 네트워크 볼륨 → 화면 기록 → 손쉬운 사용이다. 프롬프트가 떠 있는 동안에도 창은 정상 동작한다.

**원인** — 터미널 안에서 실행한 명령이 파일을 읽으면 macOS 는 그 접근을 Tasty 의 것으로 본다(Terminal.app · iTerm2 도 같다). 그대로 두면 새 폴더를 처음 건드릴 때마다 작업 도중에 프롬프트가 떠서 에이전트가 멈추므로, Tasty 는 시작 직후에 미리 물어본다. 이미 허용 · 거부한 항목은 다시 뜨지 않고, 새로 마운트된 볼륨만 추가로 묻는다. 끄는 설정은 없다 — 꺼도 프롬프트가 사라지는 게 아니라 작업 중에 산발적으로 뜨게 될 뿐이다.

**어떻게 답하나**

| 프롬프트 | 허용하지 않으면 | 나중에 바꾸려면 |
|---|---|---|
| 폴더 접근 (다운로드 · 문서 · 데스크탑 · 볼륨) | 그 폴더를 쓰는 명령에서 그때 다시 프롬프트가 뜬다 | 시스템 설정 > 개인정보 보호 및 보안 > 파일 및 폴더 |
| 화면 기록 | `Ctrl+Alt+S` 스크린샷 → 클립보드 기능이 "화면 기록 권한이 필요합니다" 안내만 띄운다. 한 번 거부하면 다시 묻지 않는다 | 시스템 설정 > 개인정보 보호 및 보안 > 화면 및 시스템 오디오 기록 |
| 손쉬운 사용 | 에이전트가 시스템에 직접 키를 주입하는 기능이 `permission_denied` 로 거부된다 | 시스템 설정 > 개인정보 보호 및 보안 > 손쉬운 사용. 켠 뒤 Tasty 를 다시 시작한다 |

현재 상태는 **설정** > **일반** <!-- en: General --> > **권한** <!-- en: Permissions --> 탭에서 본다 (macOS 에서만 보인다).

- **"Tasty 에 전체 디스크 접근 권한 주기" 안내가 떴다** — 전체 디스크 접근 권한이 없어 보일 때 한 번만 뜬다. 앱이 직접 요청할 수 없는 권한이라 **설정 열기** <!-- en: Open settings --> 로 시스템 설정을 열고 Tasty 를 목록에 직접 추가한다. 이 권한을 주면 파일 접근 프롬프트(다른 앱의 데이터 · 다운로드 · 문서 · 데스크탑 · 볼륨)가 사라진다. 다만 다른 앱 제어(Automation) · 화면 기록 · 손쉬운 사용은 별개 권한이라 그대로 남는다. 다시 보려면 **설정** > **일반** > **권한** 의 **시작할 때 전체 디스크 접근 권한 안내 표시** <!-- en: Show the Full Disk Access notice at startup --> 를 켠다. 같은 탭의 전체 디스크 접근 권한 상태는 추정값이라 틀릴 수 있고, 이 값으로 막히는 기능은 없다.
- **"Tasty이(가) 다른 앱의 데이터에 접근하려고 합니다" 가 앱 폴더마다 계속 뜬다** — `~/Library/Application Support/<앱>` 같은 경로는 앱별로 따로 물어서 미리 물어둘 수 없다. 위의 전체 디스크 접근 권한을 주면 사라진다.
- **`osascript` 를 쓸 때 "다른 앱을 제어하려고 합니다" 가 뜬다** — Automation 권한은 대상 앱마다 승인해야 하며 전체 디스크 접근 권한으로도 덮이지 않는다. Tasty 가 미리 해둘 수 있는 것이 없다.

## 창이 멈추거나 죽는다

- **창이 클릭 · 키 입력 · CLI 에 전혀 반응하지 않는다** — 5초 넘게 멈추면 `~/.tasty/crash-reports/hang-*.log` 가 자동으로 남는다. 파일의 `Render phase` 가 `acquire` / `submit` / `present` 면 GPU 드라이버 쪽 문제다 — 드라이버를 업데이트한다. Tasty 는 스스로 복구하지 않으므로 강제 종료하고 다시 띄운다.
- **갑자기 종료됐다** — `~/.tasty/crash-reports/crash-*.log` 를 본다. 문제를 신고할 때 이 파일을 함께 붙인다.

## `tasty` 명령이 연결되지 않는다

- **`No running tasty instance found (port file not found at …)`** — Tasty 창이 떠 있지 않다. 메시지에 적힌 경로가 `~/.tasty/tasty.port` 가 아니면 다른 홈 디렉토리(`TASTY_HOME`)를 보고 있는 것이다.
- **포트 파일은 있는데 연결이 안 된다** — 이전 Tasty 가 비정상 종료돼 포트 파일만 남았다. Tasty 가 실행 중이 아닌지 확인한 뒤 파일을 지우고 다시 띄운다.

  ```sh
  pgrep -x tasty || rm ~/.tasty/tasty.port
  ```

- **`tasty: command not found`** — Tasty 가 띄운 터미널 안에서는 PATH 에 자동으로 잡히지만, 다른 터미널 앱에서는 직접 등록해야 한다. 설치 방식별 경로는 [설치 위치](../getting-started/install.md#설치-위치).

## 알림이 안 온다 · 너무 많다

- **OS 알림이 안 뜬다** — Tasty 창이 활성일 때는 OS 알림을 보내지 않고 앱 안의 패널 · 테두리 · 배지로만 알린다. 창이 비활성일 때만 OS 알림이 가며, 초당 1회로 제한된다. **설정** > **알림** <!-- en: Notifications --> 의 **알림 활성화** <!-- en: Notifications enabled --> 가 꺼져 있지 않은지 본다. 패널은 `Ctrl+Shift+I` (macOS `Cmd+Shift+I`) 로 연다.
- **벨 소리(`\a`)마다 알림이 떠서 시끄럽다** — **설정** > **터미널** > **벨 알림 표시** <!-- en: Show bell notification --> 를 끈다. `config.toml` 에서는 `[general]` 의 `bell_notification = false`. 벨 훅은 그대로 발화한다.
- **소리가 안 난다** — **설정** > **알림** > **소리** <!-- en: Sound --> 가 기본 꺼짐이다. 켜도 병합 간격 안에 같은 출처에서 연달아 온 알림은 하나로 합쳐져 소리가 한 번만 난다.

설정 항목 전체는 [훅 · 알림 · 웹훅](../agents/hooks-notifications.md#설정).

## Claude · Codex 훅이 동작하지 않는다

**증상** — 자식 에이전트의 완료 알림이 안 온다. 에이전트가 응답을 마쳐도 서피스 테두리 · 사이드바 배지가 켜지지 않는다. 탭을 복원하거나 Tasty 를 재시작해도 같은 세션으로 이어지지 않는다. `tasty claude reboot` 가 `claude-session-id meta not set` 으로 실패한다. `tasty claude children` 이 보여주는 상태가 실제와 다르다.

**원인** — 훅이 설치돼 있지 않거나, Tasty 를 업데이트한 뒤 옛 훅 명령이 설정 파일에 그대로 남아 있다.

**해결** — 훅을 다시 설치한다. 여러 번 실행해도 중복되지 않는다.

```sh
tasty claude install    # ~/.claude/settings.json
tasty codex install     # ~/.codex/config.toml
```

그래도 안 되면 `~/.tasty/hook-failures.log` 와 `tasty plugin logs com.tasty.claude --follow` 를 본다. 세부 항목은 [Claude · Codex 와 함께 쓰기](../agents/claude-codex.md#문제-해결).

## 플러그인이 멈췄다

- **`tasty plugin list` 에서 enabled 인데 running 이 아니다** — 10초 안에 3번 실행에 실패하면 자동으로 정지된다. `tasty plugin logs <id>` 로 원인을 본 뒤 `tasty plugin enable <id>` 로 다시 시작한다.
- **번들 플러그인이 깨졌다** — `tasty plugin upgrade-builtins --force` 로 번들에서 다시 복사한다. 플러그인 데이터(북마크 · 프로필 등)는 유지된다.

## 내 dev 서버가 몇 번 포트에 떴는지 모르겠다

사이드바의 **도구** <!-- en: Tools --> 메뉴에서 **리스닝 포트...** <!-- en: Listening ports... --> 를 연다. Tasty 터미널에서 띄운 프로세스가 열어둔 TCP 포트를 포트 · 프로세스 · 워크스페이스 · 탭과 함께 보여준다.

- 기본은 LISTEN 상태만 보인다. 목록이 비고 "상태 필터에 맞는 포트가 없습니다" 가 떠 있으면 포트가 없는 게 아니다 — 필터 행 우측의 **상태** <!-- en: State --> 버튼으로 다른 상태를 켜고 **적용** <!-- en: Apply --> 한다.
- Tasty 밖 프로세스까지 보려면 **전체 보기** <!-- en: Show all (system-wide) --> 를 켠다.
- 행을 클릭해 선택하고 **주소 복사** <!-- en: Copy address --> 로 `host:port` 를 클립보드에 넣는다.
- 별 아이콘으로 즐겨찾기에 넣으면 상단에 항상 보이고, 재시작해도 남는다 (`~/.tasty/port-favorites.toml`).

## 문제 신고

https://github.com/zilhak/tasty/issues 에 올린다. 다음을 함께 적으면 빨리 해결된다.

- `tasty --version` 출력과 OS · 버전
- 재현 절차
- `~/.tasty/crash-reports/` 의 해당 `crash-*.log` / `hang-*.log`
- 증상 직후의 `~/.tasty/debug.log` (다시 켜면 비워지므로 먼저 복사해 둔다)
