# 닫힌 항목 복원 (Closed item restore)

- **Status**: Implemented
- **주체**: 로컬 사용자 전용 (`restore_closed`, 기본 `Ctrl+Shift+T`)
- **ADR**: 없음 (원칙은 [identity](../../identity.md) §1)
- **코드**: `ClosedItem` LIFO (`crates/tasty-model`), snapshot push `src/state/{pane,tab,workspace}.rs`
- **화면**: 없음 (복원은 focused pane 에 즉시 반영)

## 목적

사용자가 실수로 닫은 surface/tab/workspace 를 즉시 되돌린다. 닫기 시점 스냅샷(`ClosedItem`, **인메모리 LIFO 스택**)을 보관했다가 단축키로 복원하는 휘발성 안전망 — 디스크 미저장. 반복 사용 목적의 영구 저장은 [layout-presets](../layout-presets/index.md).

## 내부 동작

- 트리거: `restore_closed` 단축키([keybindings](../keybindings/index.md), 4 프리셋 공통 `Ctrl+Shift+T`).
- 스택 top 의 항목 복원. Surface/Tab 은 호출 시점 focused pane 에. workspace 가 0개일 때 Surface/Tab 복원은 default workspace 를 먼저 만든 뒤.
- **scrollback 은 `general.restore_surface_content` 와 무관하게 항상** 복원(메모리 보관분 즉시 재사용, 디스크 미경유).
- 닫기 시점 surface 메타 `restore.command`(예: claude plugin 의 `claude -r <id>`)가 있었으면 셸 시작 직후 자동 실행 → TUI 세션 재개([layout-persistence](../layout-persistence/index.md) 의 동일 메커니즘).
- 복원할 항목 없으면 no-op (토스트/알림 없음).

## 사용자/에이전트 분리 (핵심)

복원 스택은 **사용자의 시점 상태**라 에이전트 표면에 없다([identity](../../identity.md) §1):

- 복원을 트리거하는 CLI/IPC 없음 (`RestoreClosedItem` intent 는 사용자 단축키 전용).
- **에이전트가 닫은 항목은 스택에 안 들어간다** — IPC `surface.close` 는 `save_snapshot=false`, `tab.close`(DomainIntent)는 스냅샷 경로 자체가 없다. 스냅샷 push 는 사용자 단축키/마우스 닫기 경로에서만.

## 비-목표

- 에이전트용 복원 API — 에이전트는 자기가 만든 리소스를 ID 로 재생성하면 된다.
- 앱 재시작을 넘는 영속 — [layout-persistence](../layout-persistence/index.md) / [layout-presets](../layout-presets/index.md).
- PTY 프로세스 상태 복원 — 셸은 새로 시작, scrollback + `restore.command` 만 이어진다.

## 관련

- [layout-persistence](../layout-persistence/index.md) · [layout-presets](../layout-presets/index.md) · [work-area](../work-area/index.md)
