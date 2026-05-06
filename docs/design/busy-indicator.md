# Busy Indicator (실행 중 표시)

> **상태: 구현 완료.**

탭과 워크스페이스가 "지금 무언가 실행 중인지"를 시각적으로 알려주는 정책.

## 1. 정의

**Busy 한 surface (terminal)** = 세 조건을 **모두** 만족하는 상태.

1. PTY의 foreground 프로세스가 shell 자신도 아니고 알려진 shell 이름(`bash`/`zsh`/`fish`/`sh`/`pwsh`/`powershell`/`cmd`)도 아니다.
2. 최근 `BUSY_OUTPUT_WINDOW`(현재 2초) 안에 PTY로부터 출력 바이트를 처리한 적이 있다.
3. 그 출력이 사용자 입력의 에코가 아니다 — 마지막 PTY 출력 시점이 마지막 사용자 입력(`send_key`/`send_bytes`) 시점 + `INPUT_ECHO_WINDOW`(200ms) 이후여야 한다.

쉘이 prompt를 띄우고 입력을 기다리는 상태는 (1) 위반으로 **idle**.
`vim`을 띄워두고 화면이 정적이거나 `claude`가 사용자 입력을 기다리는 상태는 (2) 위반으로 **idle**.
사용자가 `claude` 프롬프트에 타이핑하는 중에는 (3) 위반으로 **idle** — 에코 출력만 발생하는 상태.
`cargo build`처럼 출력이 흘러나오거나 `claude`가 응답 토큰을 흘리는 동안에는 **busy**.

(2)는 tmux/iTerm2/WezTerm의 activity-monitor와 같은 시멘틱이며, foreground 프로세스 검사만으로는 잡히지 않던 "프로세스는 떠 있지만 일하고 있지 않다" 상태를 걸러낸다. (3)은 사용자 타이핑의 에코를 프로그램 활동으로 오인하는 것을 방지한다.

## 2. 집계 정책

| 단위 | Busy 판정 |
|------|----------|
| Surface (terminal) | foreground가 shell이 아니고 + 최근 2초 안에 PTY 출력이 있고 + 그 출력이 사용자 입력 에코가 아니면 busy |
| Tab | 탭이 포함하는 surface 중 **하나라도 busy면 busy** |
| Pane | (집계 표시 없음 — 탭바가 직접 보여줌) |
| Workspace | 워크스페이스가 포함하는 surface 중 **하나라도 busy면 busy** |

집계는 OR. busy인 surface가 몇 개인지(count) 워크스페이스 인디케이터에서 노출하지만, 판정 자체는 "1개 이상" 임계.

비-터미널 surface(Markdown viewer, Explorer, Image 등)는 busy 판정 대상이 아니다 — 항상 idle로 취급.

## 3. 시각 표시

### Tab
탭 라벨 옆에 **작은 점 1개** (직경 6px). 색상은 focused tab 텍스트의 dim 버전. busy 갯수는 표시하지 않음 (한 탭은 사용자가 이미 시야에 두고 있는 단위라 정보량 최소화).

### Workspace (사이드바)
워크스페이스 항목 우측에 **점 + 카운트 텍스트** (예: `● 3`). busy인 surface 갯수를 함께 보여줌. 다른 워크스페이스로 이동했을 때 무엇이 어디서 돌고 있는지 파악할 수 있어야 하므로 정보량을 더 준다.

### Surface 자체
별도 표시하지 않음. focused/unfocused 보더 색이 이미 존재하고, 분할된 surface는 모두 시야에 들어와 있음.

## 4. 폴링 주기

매 frame이 아니라 **약 1초 간격**으로 모든 활성 PTY의 foreground 프로세스를 갱신. tpgid/process-snapshot 조회는 가볍지만 매 frame 호출은 과함.

## 5. 플랫폼별 판정 메커니즘

| 플랫폼 | 메커니즘 | 정확도 |
|--------|---------|--------|
| Linux  | `/proc/<shell_pid>/stat`의 6번째 필드(`tpgid`) → `/proc/<tpgid>/comm` | 정확 (커널이 알려주는 foreground PGID) |
| macOS  | `ps -o tpgid= -p <shell_pid>` → `ps -o comm= -p <tpgid>` | 정확 |
| Windows | `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` → `Process32FirstW`/`NextW`로 트리 enum → shell의 자손 존재 여부 + leaf 자손의 이름 | 근사 (백그라운드 자식이 있으면 오탐 가능) |

Windows에는 Unix의 `tcgetpgrp`에 대응하는 직접 API가 없다. ConPTY는 foreground process group 개념을 노출하지 않으므로 process tree 추적으로 근사한다. 이는 WezTerm·iTerm2(Win)·Windows Terminal이 모두 채택하는 방식이며, 일반적인 사용 패턴(`vim`, `cargo build`, `node`, `python` 등)에서는 정확하다.

쉘 자신을 foreground로 판정하는 기준:

- foreground 프로세스 이름이 `bash`, `zsh`, `fish`, `sh`, `pwsh`, `powershell`, `cmd` 중 하나이고
- 그 PID가 PTY가 spawn한 shell PID와 같거나, 그 shell의 직계 자손이 아닐 때

→ idle로 판정. 그 외에 한해서 (2)의 출력 활동 검사로 한 번 더 거른 뒤 busy로 판정.

## 6. CLI/IPC 노출

- IPC `surface.list` 응답에 이미 `foreground_process: "string"` 필드 존재 (`src/ipc/handler/surface.rs`).
- 추가로 `busy: bool` 필드를 도입하여 에이전트가 직접 판정 결과를 받아볼 수 있게 한다.
- 워크스페이스/탭 단위 집계는 `tree`/`list workspaces`/`list tabs` 응답에 `busy_count: number` 필드로 추가.
- **focus와 무관하게 동작**한다. busy는 surface 고유 속성이며 사용자의 관심 위치와 분리된다 (focus-policy.md §6 참조).

## 7. 비-목표

- 정확한 "지금 실행 중인 명령줄"을 알아내는 것은 목표가 아니다. 그 정보는 OSC 133 등 shell integration이 필요하며 별도 기능.
- busy 시간(elapsed) 추적은 별도 기능으로 분리. 이 문서는 ON/OFF 판정과 표시만 다룸.
- 자식 프로세스의 CPU/메모리 사용량 같은 리소스 정보는 별도 영역.
