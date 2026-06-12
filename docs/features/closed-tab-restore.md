# 닫힌 항목 복원 (Closed Item Restore)

- **Status**: Implemented
- **Surface**: 사용자 전용 (단축키 `Ctrl+Shift+T` — 전 프리셋 공통 기본값)
- **Related ADR**: ADR 후보 (adr-candidates.md #0002 user-vs-agent-action-separation)
- **Related design**: [`../concepts/ubiquitous-language.md`](../concepts/ubiquitous-language.md) (사용자/에이전트 행동 분리)

## 목적

사용자가 실수로 닫은 surface / tab / workspace 를 즉시 되돌릴 수 있게 한다. 닫기 시점의 스냅샷 (`ClosedItem`, 인메모리 LIFO 스택) 을 보관했다가 단축키 한 번으로 복원한다. 디스크에 저장하지 않는 휘발성 안전망이며, 반복 사용을 의도한 영구 저장은 레이아웃 프리셋이 담당한다.

## 사용자 행동 (UX)

- 트리거: `restore_closed` 단축키 (기본 `Ctrl+Shift+T`, 4 개 프리셋 모두 동일).
- 결과:
  - 스택 top 의 항목 (Surface / Tab / Workspace) 이 복원된다. Surface/Tab 은 호출 시점의 focused pane 에 복원된다.
  - scrollback 은 **`general.restore_terminal_content` 옵션과 무관하게 항상** 복원된다 (메모리 보관분 즉시 재사용, 디스크 미경유).
  - 닫기 시점에 surface 메타데이터 `restore.command` 가 있었으면 (예: Claude plugin 의 `claude -r <session-id>`) 셸 시작 직후 자동 실행되어 TUI 세션이 재개된다.
  - workspace 가 하나도 없는 상태에서 Surface/Tab 을 복원하면 default workspace 를 먼저 생성한 뒤 복원한다.
- 예외: 복원할 항목이 없으면 (스택 비어 있음) 아무 일도 일어나지 않는다 (no-op — 토스트/알림 없음).

## 에이전트 행동 (CLI / IPC)

**없음 (비노출).** 복원 스택은 사용자의 시점 상태이므로 에이전트 표면에 존재하지 않는다.

- 복원을 트리거하는 CLI/IPC 메서드가 없다 (`RestoreClosedItem` intent 는 사용자 단축키 전용 — `src/intent.rs` "사용자 단축키 전용" 명시).
- **에이전트가 닫은 항목은 복원 스택에 들어가지 않는다**:
  - IPC `surface.close` 는 `save_snapshot=false` 로 닫는다 (`src/adapters/ipc/handler/surface/close.rs` — "save_snapshot=false (Agent)").
  - IPC `tab.close` (`DomainIntent::CloseTab`) 는 스냅샷 경로 자체가 없다 (`src/core/mod.rs::apply_close_tab`).
  - 스냅샷 push 는 사용자 단축키/마우스 닫기 경로 (`src/state/{tab,pane,workspace}.rs`) 에서만 일어난다.

## 비-목표 (Out of Scope)

- 에이전트용 복원 API 제공 — 에이전트는 자기가 만든 리소스를 ID 로 재생성하면 되므로 복원 스택이 필요 없다.
- 앱 재시작을 넘는 영속화 — 그것은 레이아웃 영속화 (`layout.json`) 와 레이아웃 프리셋의 역할.
- PTY 프로세스 상태 복원 — 셸은 새로 시작되며 scrollback 과 `restore.command` 만 이어진다.

## Acceptance Criteria

- [ ] Given 사용자가 단축키 (`Ctrl+W` 등) 로 탭을 닫은 후 When `Ctrl+Shift+T` Then 직전 닫은 탭이 focused pane 에 복원된다 (scrollback 포함).
- [ ] Given 에이전트가 IPC `tab.close` / `surface.close` 로 항목을 닫음 When 사용자가 `Ctrl+Shift+T` Then **그 항목은 복원 대상이 아니다** (사용자/에이전트 분리 — 에이전트 닫기는 스택에 push 되지 않음).
- [ ] Given 복원 가능한 항목이 없음 When `Ctrl+Shift+T` Then no-op (에러/토스트 없음).
- [ ] Given 닫힌 터미널에 `restore.command` 메타키가 있었음 When 복원 Then 셸 시작 직후 해당 명령이 자동 실행된다.
- [ ] Given `general.restore_terminal_content` 가 off When 닫은 탭을 복원 Then scrollback 은 그래도 복원된다 (옵션은 앱 재시작 경로에만 적용).
- [ ] Given workspace 가 0 개 When Surface/Tab 항목 복원 Then default workspace 생성 후 그 안에 복원된다.

## 관련 문서

- [`../features.md`](../features.md) "TUI 세션 복원" / "터미널 내용 복원 (scrollback)" 섹션
- `CLAUDE.md` "# 핵심 원칙 §1" — 닫힌 항목 히스토리는 사용자 상태
- `.claude-workspace/todo/adr-candidates.md` #0002 (ADR 작성 대기)
