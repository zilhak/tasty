# Busy Indicator (실행 중 표시)

탭·워크스페이스가 "지금 무언가 실행 중인지" 를 시각적으로 알리는 정책. focus 와 무관하게 동작한다([focus](focus.md)).

## 판정 — 해제 두 조건 · 진입 조건 하나

busy는 이전에 확인한 상태를 기억한다. 입력 직후 에코만 있으면 idle을 유지하지만, 이미 작업 중인 프로그램은 입력만으로 idle이 되지 않는다([ADR-0615](../../adr/0615-terminal-user-input-routing.md)).

**Busy 한 surface(terminal)** 는 다음 순서로 판정한다 — 셸 복귀와 출력 만료를 먼저 확인한다.

1. PTY foreground 프로세스가 shell 자신이거나 알려진 shell 이름(`is_known_shell_name`)이면 **idle**.
2. 마지막 PTY 출력이 `BUSY_OUTPUT_WINDOW`(2초)보다 오래됐으면 **idle**.
3. 진입 억제 — 직전 판정이 **같은 foreground 프로세스에 대해 busy 가 아니었고**, 마지막 출력이 사용자 입력 에코 구간(`last_input_at <= last_output_at <= last_input_at + INPUT_ECHO_WINDOW`, 200ms) 안이면 **idle**.
4. 나머지는 **busy**.

해제 사유는 (1)·(2) 둘뿐이고, 입력은 해제 사유가 아니다. (3) 의 억제는 방향을 갖는다 — 에코는 입력 **이후**의 출력이므로, 마지막 입력보다 앞선 출력은 억제하지 않는다.

`Terminal`은 마지막으로 **busy로 판단한 foreground PID**를 기억한다. 이 기록으로 직전 판정과 현재 프로세스를 비교한다.

- (1)·(2) 로 idle 이 되면 상태 기록이 지워진다. 같은 프로그램이 다시 foreground 로 와도(중단 후 `fg`) busy 를 물려받지 않는다.
- foreground 가 다른 프로세스로 바뀌면 PID 가 달라 상태 기록이 적용되지 않는다 — 새 프로그램은 이전 프로그램의 busy 를 물려받지 않는다.
- 상태 락이 경합 중이면(파서가 ingest 중) 그 tick 은 busy 로 **추정**하되([ADR-0613](../../adr/0613-terminal-io-and-process-lifetime.md)), 그 추정은 상태 기록을 만들지도 지우지도 않는다.
- 상태 기록은 busy 로 답할 때만 세워지므로 같은 판정을 연달아 물어도 답이 같다.

예시:

- 셸이 prompt 대기 → (1) = **idle**.
- `vim` 띄워두고 정적이거나 `claude` 가 입력 대기 → (2) = **idle**.
- idle 인 `vim`·`claude` 프롬프트에 타이핑을 시작 → (3) = **idle**(에코만 발생).
- `claude` 가 응답을 흘리는 중에 다음 질문을 타이핑 → 직전이 busy 라 (3) 이 적용되지 않음 = **busy**. 응답이 멈춰 2초가 지나면 (2) 로 **idle**.
- `cargo build` 출력 흐름·`claude` 응답 토큰 흘림 → **busy**.
- 직전이 idle 인 마우스 트래킹 TUI 위에서 마우스만 움직이는 중 → 마우스 리포트도 입력이므로 (3) = **idle**. 직전이 busy 였다면 출력이 이어지는 동안 **busy**.
- DSR(`ESC[6n`)·DA·OSC 색상 질의를 반복하는 TUI 가 출력도 내는 중 → 응답은 입력이 아니므로 **busy**.

(2)는 tmux/iTerm2/WezTerm 의 activity-monitor 와 같은 동작("프로세스는 떠 있지만 일하진 않음" 을 걸러냄), (3)은 idle 한 프로그램에 타이핑한 에코를 활동으로 오인하는 것을 막는다. 연속 타이핑 간격이 200ms 보다 짧으면 그동안 시작된 출력도 에코 구간 안에 들어가 진입이 미뤄지고, 타이핑이 멈춘 뒤 다음 폴링에서 진입한다. 비-터미널 surface(Markdown/Explorer/Image)는 판정 대상 아님(항상 idle).

### (3) 의 억제 창을 갱신하는 write / 갱신하지 않는 write

PTY 로 나가는 write 는 모두 같은 채널을 타지만, 억제 창(`last_input_at`)을 갱신하는 것은 **터미널 바깥에서 들어온 입력** 뿐이다.

| write | 억제 창 갱신 | 경로 |
|-------|-------------|------|
| 키 입력 / `send_key` / `send_bytes` (에이전트 IPC 포함) | **갱신** | `TerminalState::write_input` |
| 마우스 리포트(`encode_mouse_report`) | **갱신** | 위와 동일 |
| 터미널 자체 질의 응답 — DSR(커서 위치/상태) · DA/DA2 · DECRPTUI · XTVERSION · OSC 4/10/11 색상 · XTWINOPS 18/19 | **갱신 안 함** | `TerminalState::send_terminal_response` |

터미널 응답은 사용자가 타이핑한 것이 아니므로 에코 억제의 대상이 아니다. 갱신하게 두면 `INPUT_ECHO_WINDOW`(200ms) 보다 짧은 주기로 질의를 쏘는 TUI 가 억제 창을 영구히 열어둬, 출력이 끊임없이 흐르는데도 실행 내내 `busy == false` 가 된다. 두 전송은 입력 시각 갱신 여부만 다르고 실제 전송 경로(입력 채널 send, `enqueued_count`/`WriteAck` 계약)는 동일하다.

## 집계 — OR

| 단위 | busy 판정 |
|------|-----------|
| Surface(terminal) | 위 판정 |
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
| Windows | `CreateToolhelp32Snapshot` 트리 enum → shell 의 **가장 얕은 non-shell 자손**(같은 깊이는 pid 오름차순; non-shell 자손 없으면 deepest-leaf/셸 fallback) | 근사(ConPTY 가 foreground PGID 미노출 — WezTerm/Windows Terminal 동일 방식) |

프로세스 조회는 fork 없이 syscall이나 파일 읽기로 처리한다.
Windows는 한 번의 poll에서 시스템 프로세스 snapshot을 한 번만 만들고 모든 surface가 공유한다.
가장 얕은 non-shell 자식을 골라 단명 helper가 생길 때마다 표시 이름이 바뀌는 것을 줄인다.
같은 깊이에서는 PID 순서로 고른다. 중첩 셸은 건너뛴다.

StatusBar 의 프로세스명 셀도 이 1Hz 캐시(`CoreState::foreground_name`)를 읽는다 — 매 프레임 스냅샷을 다시 뜨지 않는다. 표시는 최대 1초 지연되지만 대화형 라벨엔 무해하다.

## CLI/IPC

`surface.list` 에 `busy: bool`, `tree`/`workspace.list`/`tab.list` 에 `busy_count: number`(`annotate_tree_busy`). **focus 무관** — busy 는 surface 고유 속성이라 사용자 관심 위치와 분리([focus](focus.md)).

## 비-목표

- "지금 실행 중인 명령줄" 알아내기 — OSC 133 shell integration 영역([terminal-output](../../features/terminal-output/index.md)).
- busy elapsed 추적·리소스(CPU/메모리) 사용량 — 별도 영역.

## 관련

- [focus](focus.md) · [notifications](../../features/notifications/index.md)(busy 표시 동반) · [terminal](../../features/terminal/index.md)
