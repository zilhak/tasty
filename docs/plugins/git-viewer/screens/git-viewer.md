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
- 빈/없음 안내(Changes `no_changes` · Commits `no_commits` · `no_repo` · `no_worktrees`)는 `center` 로 해당 pane 가용영역 양축 중앙에 한 줄 배치.

## 디자인 토큰 매핑

plugin 은 `UiNode` DSL(중첩 `splitter` + `selectable_row` + 색 라벨)로 구성만 정하고, host
`ui_tree_render.rs` 가 catppuccin→의미 토큰으로 그린다. UI 인벤토리 ↔ 토큰:

| UI 요소 | 토큰 | 비고 |
|---|---|---|
| popup 프레임 | `bg-panel` | ≈960 wide |
| pane 분할선(H 0.25 / V 0.5) | `separator` · `border-width` | rail \| (status / log·diff) |
| pane 제목(Heading) | `text-primary` · `font-size-term-lg` | Worktrees / Status / Log |
| 선택 행 | `surface-active` | `selectable_row` selected |
| HEAD oid · refs · `main` 배지 | `accent-info` | host `blue`→sky |
| `current` · added(`A`) | `accent-success` | `green` |
| `locked` · modified(`M`) | `accent-warning` | `yellow` |
| `invalid` · deleted(`D`) | `accent-danger` | `red` |
| linked 이름 · author/time | `text-muted` | `subtext0` |
| invalid 이름 | `text-disabled` | `overlay0`(잠정 `overlay1`) |
| diff hunk header | `accent-info` · mono | `@@ … @@` |

## 갤러리 specimen

`crates/tasty-gallery/src/catalog/components/git_viewer.rs` — Overlays › `Git worktree viewer
popup`. worktree rail + (status/log) + diff 를 토큰·구조 정합으로 전사(픽셀 동일성 비목표 —
ADR-0020 완전성에 따라 specimen 포함). 3자 매핑:
[design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#git-viewer-overlays).

## 시각 소스

popup 치수·섹션 배치는 design-system(vendor 후 링크). popup 구현은 `PopupDef` 시스템(dev-guide).
