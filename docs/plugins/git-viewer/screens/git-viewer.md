# Git Viewer popup 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: `design-system/` 의 git viewer popup 디자인(있으면), vendor 예정.

도구 메뉴/IPC 로 뜨는 git status/log/diff 읽기 전용 popup.

## 트리거

[도구 메뉴](../../../features/tools-menu/screens/tools-menu.md) git viewer 항목 또는 IPC.

## 레이아웃

`vbox[ header, (error), splitter(Horizontal, ~0.25, worktree_rail | right_column) ]`
- **worktree_rail** (좌, ratio ≈ 0.25 ≈ 960px 의 240px) — drag 가능 divider.
- **right_column** = `splitter(Vertical, 0.5, status | log·diff)` — 기존 2-pane 구조 유지, drag 가능.

## UI 요소 인벤토리

- **worktree rail** — main + 모든 linked worktree 행 목록(`selectable_row`).
  각 행: 이름 + HEAD(브랜치/short oid) + 타입 배지(`main`/`linked`) + 상태 배지(`current`/`locked`/`invalid`).
  선택 시 우측이 그 worktree 기준으로 rebind. invalid 행은 전환 불가.
- **status** — 변경된 파일 목록(활성 worktree 기준).
- **log** — 커밋 히스토리(활성 worktree 기준).
- **diff** — 선택 항목의 변경 내용.
- 전부 **읽기 전용**(액션 버튼 없음 — worktree 조작 버튼도 없음).

## 상태별 시각

- repo / 비-repo(빈·에러) / detached.
- worktree: current(녹색 마커) / linked / locked(노랑 배지, 사유) / invalid(빨강 배지) / worktree 0개(main 단일 행).

## 시각 소스

popup 치수·섹션 배치는 design-system(vendor 후 링크). popup 구현은 `PopupDef` 시스템(dev-guide).
</content>
