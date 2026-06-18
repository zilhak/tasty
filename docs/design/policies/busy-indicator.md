# Busy Indicator (실행 중 표시)

탭·워크스페이스가 "지금 무언가 실행 중인지" 를 시각적으로 알리는 정책. focus 와 무관하게 동작한다([focus](focus.md)).

## 판정 — 세 조건 모두 (AND)

**Busy 한 surface(terminal)** = 다음을 *모두* 만족:

1. PTY foreground 프로세스가 shell 자신도, 알려진 shell 이름(`bash`/`zsh`/`fish`/`sh`/`pwsh`/`powershell`/`cmd`)도 아니다.
2. 최근 `BUSY_OUTPUT_WINDOW`(2초) 안에 PTY 출력 바이트를 처리한 적이 있다.
3. 그 출력이 사용자 입력 에코가 아니다 — 마지막 PTY 출력이 마지막 사용자 입력(`send_key`/`send_bytes`) + `INPUT_ECHO_WINDOW`(200ms) 이후.

- 셸이 prompt 대기 → (1) 위반 = **idle**.
- `vim` 띄워두고 정적이거나 `claude` 가 입력 대기 → (2) 위반 = **idle**.
- `claude` 프롬프트에 타이핑 중 → (3) 위반 = **idle**(에코만 발생).
- `cargo build` 출력 흐름·`claude` 응답 토큰 흘림 → **busy**.

(2)는 tmux/iTerm2/WezTerm 의 activity-monitor 와 같은 시멘틱("프로세스는 떠 있지만 일하진 않음" 을 걸러냄), (3)은 타이핑 에코를 활동으로 오인 방지. 비-터미널 surface(Markdown/Explorer/Image)는 판정 대상 아님(항상 idle).

## 집계 — OR

| 단위 | busy 판정 |
|------|-----------|
| Surface(terminal) | 위 3조건 |
| Tab | 포함 surface 중 **하나라도 busy** |
| Workspace | 포함 surface 중 **하나라도 busy** (+ count 노출) |

## 시각 표시

- **Tab**: 라벨 옆 작은 점 1개(focused tab 텍스트 dim). count 미표시(이미 시야에 둔 단위).
- **Workspace(사이드바)**: 점 + count(예: `● 3`) — 다른 워크스페이스에서 뭐가 도는지 파악.
- **Surface 자체**: 별도 표시 없음(focused/unfocused 보더 + 분할이 이미 시야).

## 폴링 / 플랫폼

매 frame 이 아니라 **약 1초 간격**으로 활성 PTY foreground 갱신(`crates/tasty-terminal/src/foreground_process.rs`):

| 플랫폼 | 메커니즘 | 정확도 |
|--------|---------|--------|
| Linux | `/proc/<pid>/stat` 의 tpgid → `/proc/<tpgid>/comm` | 정확 |
| macOS | `proc_pidinfo(PROC_PIDTBSDINFO)` 의 `e_tpgid` → 같은 syscall 로 leader 의 `pbi_name` | 정확 |
| Windows | `CreateToolhelp32Snapshot` 트리 enum → shell 자손 leaf 이름 | 근사(ConPTY 가 foreground PGID 미노출 — WezTerm/Windows Terminal 동일 방식) |

> 모든 플랫폼의 조회는 fork 없는 syscall/파일읽기다. macOS 는 과거 `ps` 를 2회 fork 했으나, live surface 수 × 1Hz 의 `posix_spawn` 이 메인 스레드를 블록해 워크스페이스 전환을 지연시켜(66 live shell 기준 전환 p90 251ms→2.8ms) libproc 으로 교체했다. Windows 의 전체 프로세스 스냅샷은 surface 수 × 전체 프로세스 수 비용이 있으나 fork 는 없다.

## CLI/IPC

`surface.list` 에 `busy: bool`, `tree`/`workspace.list`/`tab.list` 에 `busy_count: number`(`annotate_tree_busy`). **focus 무관** — busy 는 surface 고유 속성이라 사용자 관심 위치와 분리([focus](focus.md)).

## 비-목표

- "지금 실행 중인 명령줄" 알아내기 — OSC 133 shell integration 영역([terminal-output](../../features/terminal-output/index.md)).
- busy elapsed 추적·리소스(CPU/메모리) 사용량 — 별도 영역.

## 관련

- [focus](focus.md) · [notifications](../../features/notifications/index.md)(busy 표시 동반) · [terminal](../../features/terminal/index.md)
