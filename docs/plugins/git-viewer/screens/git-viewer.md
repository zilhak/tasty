# Git Viewer popup 화면

- **부모 기획**: [../index.md](../index.md)
- **시각 소스**: 디자인 `ui_kits/terminal/overlays/git_viewer.jsx` (changelog `2026-06-30-git-viewer.md`).

도구 메뉴/IPC 로 뜨는 git status/log/diff 읽기 전용 popup. **egui-mesh** 로 그린다 — plugin 이
자기 프로세스 egui Context 에서 콘텐츠를 직접 페인트하고, host 는 셸(scrim/border/Esc/outside-click)만
소유한다(ADR-0028 / B3). Theme 은 `popup.set_context` 의 `ThemeWire` 로 매 frame 전달돼 host 와
동일 `Theme` 으로 재구성된다.

## 트리거

[도구 메뉴](../../../features/tools-menu/screens/tools-menu.md) git viewer 항목 또는 IPC. 단일
인스턴스 — 두 번째 open 은 "이미 열림" 안내를 보인다.

## 레이아웃

960×640. `header + context strip + body`.
- **header** — `Git` 타이틀 + `Refresh`(secondary). 하단 `separator`.
- **context strip** — `bg-sidebar` 밴드: 활성 worktree · branch · HEAD oid pill · repo path(우측, ellipsis).
- **body** — 좌 **worktree rail**(고정 232px) | 우 **right column**. right column 은 상 **Changes** /
  하 **Commits↔Diff** 로 50/50 분할.

## UI 요소 인벤토리

- **worktree rail** — main + 모든 linked worktree 를 2줄 행으로. line1 = 이름(mono) + 타입 pill
  (`main`=sky / `linked`=neutral), line2 = short oid(sky) + 상태 pill(`current`/`locked`/`invalid`, dot).
  선택 행 = `surface-active` + 좌측 2px inset accent bar. 선택 시 우측이 그 worktree 기준으로 rebind.
  invalid 행은 흐림 + 전환 불가.
- **Changes** — 변경된 파일 목록. 고정폭 상태 pill(`M/A/D/R/?/U`) + 경로(dir `text-muted` / file
  `text-primary`). 행 선택 시 하단 pane 이 diff 로 교체(선택 행에 inset bar).
- **Commits** — 커밋 히스토리. oid(sky) + refs pills(sky, 있을 때만) + summary(flex) + author + time.
- **Diff** — 선택 파일의 unified hunk. 툴바(`Back` ghost + 파일 경로) + recessed `bg-app` well:
  old/new 라인번호 거터 + `+`/`-`/context 부호 컬럼 + ±-라인 배경 tint + hunk header 밴드(sky).
- 전부 **읽기 전용**(액션 버튼 없음). 상호작용 = Refresh · worktree 선택(rebind) · 파일 선택(→diff) ·
  diff Back. 입력은 host 가 forward 한 실제 사용자 입력으로 plugin egui 안에서 처리된다.

## 상태별 시각

- repo / 비-repo(중앙 안내) / detached(branch 자리에 `detached`).
- worktree: current(녹색 dot pill) / linked / locked(노랑 dot pill, 사유 hover) / invalid(빨강 dot pill) /
  worktree 0개(`no_worktrees`).
- 빈/없음 안내(Changes `no_changes` · Commits `no_commits` · rail `no_worktrees`)는 해당 pane 중앙 한 줄.
- error(`accent-danger` 라인, tinted 밴드) 는 header 아래·body 위에 표시.
- already-open(단일 인스턴스 두 번째 인스턴스) 중앙 안내.

## 디자인 토큰 매핑

색·폰트·간격은 전부 `Theme` 토큰(host catppuccin → 의미 토큰). UI 인벤토리 ↔ 토큰:

| UI 요소 | 토큰 | 비고 |
|---|---|---|
| popup 프레임(host 셸) | `bg-panel` · `border-default` | 960×640 |
| context / 섹션 strip | `bg-sidebar` · `separator` | 상단 밴드 |
| 섹션 제목 | `text-muted` · `font-size-micro` mono uppercase | count 포함 |
| 선택 행 | `surface-active` + `accent-primary` inset bar | worktree / change |
| HEAD·commit oid · refs · `main` · hunk | `accent-info` | sky (Tag `Info` 톤) |
| `current` · added(`A`) · diff `+` | `accent-success` | `green` |
| `locked` · modified(`M`) | `accent-warning` | `yellow` |
| `invalid` · deleted(`D`) · unmerged(`U`) · diff `-` · error | `accent-danger` | `red` |
| `linked` · untracked(`?`) | neutral(`text-secondary`/`border-default`) | Tag `Default` |
| dir 경로 · author · time · 거터 | `text-muted` / `text-disabled` | |
| diff well | `bg-app` | recessed |

## 갤러리 specimen

`crates/tasty-gallery/src/catalog/components/git_viewer.rs` — Overlays › `Git worktree viewer
popup`. context strip · 섹션 strip · 2줄 worktree 행 · Changes · Commits · diff well 을 토큰·구조
정합으로 전사(픽셀 동일성 비목표 — ADR-0020 완전성). 3자 매핑:
[design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#git-viewer-plugins).

## 시각 소스

디자인 `ui_kits/terminal/overlays/git_viewer.jsx`(+`.html` preview). popup 구현은 egui-mesh
채널(ADR-0028) + `EguiMeshPopup` SDK 헬퍼.
