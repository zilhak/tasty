# Git Viewer (`com.tasty.git-viewer`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (도구 메뉴 → popup) · AI Agent (IPC trigger)
- **배포/통합**: bundled · 도구 메뉴 항목 + popup — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-git-viewer/`
- **권한**: `ui.popup` · `ui.tool_item` · `fs.read`
- **화면**: [screens/git-viewer.md](screens/git-viewer.md)

> **예제로서**: **도구 메뉴 항목 + popup** 예제 — main/git/view 모듈을 분리한 깔끔한 구조의 레퍼런스 → [plugin-development](../../dev-guide/plugin-development.md#도구-메뉴-항목--popup).

## 목적

git **status / log / diff 를 읽기 전용**으로 보여주는 popup 을 제공한다.
좌측 **worktree rail** 로 main + 모든 linked worktree 를 한눈에 보고, worktree 를 골라
그 worktree 기준 status/log/diff 로 전환한다(읽기 전용).

## 내부 동작

- **tool** `open-viewer` — [도구 메뉴](../../features/tools-menu/index.md)에 항목 추가(`ui.tool_item`), action `open_popup{com.tasty.git-viewer/viewer}`.
- **popup** `viewer` — trigger `ipc`(IPC 로도 열림). status/log/diff 를 `fs.read` 로 읽어 표시. **읽기 전용**(커밋/스테이징 등 변경 없음).
- **worktree 종합 목록** — libgit2 `worktrees()` 는 linked 만 주므로 main working tree 는
  `commondir` 파일(폴백: 경로 추론)로 직접 식별해 목록 선두에 합성한다(`git worktree list` 와 동등).
  popup 이 받은 cwd 가 속한 worktree 에 `current` 마커, `locked`/`invalid` 상태 배지를 표시한다.
- **worktree 전환** — rail 행 선택 시 그 worktree 의 workdir 로 다시 열어 status/log/diff 를
  재바인딩한다. **실제 checkout/working dir/HEAD 변경은 없다**(플러그인 popup 내부 상태만 변경).
- **fs 접근** — git2 가 파일을 직접 읽어(host fs 포트 우회) worktree 가 cwd 밖에 있어도 읽는다.
  권한 선언은 `fs.read` 유지.

## 인터페이스

- **사용자**: 도구 메뉴 `Git viewer` → popup.
- **AI Agent**: popup trigger 가 ipc — IPC 로 열 수 있다.

## 비-목표

- git *쓰기*(커밋/스테이징/브랜치 조작) — 읽기 전용.
- **worktree 조작**(add/remove/prune/lock 토글) — 읽기 전용 정체성 유지. 목록·전환만 제공.
- 상태바의 브랜치 표시 — [workspace-status-bar](../../features/workspace-status-bar/index.md)(별개).

## Acceptance Criteria

- [ ] Given 플러그인 활성 Then 도구 메뉴에 git viewer 항목이 보인다.
- [ ] Given repo 안에서 popup 열기 Then status/log/diff 가 표시된다.
- [ ] Given 비-repo Then 적절한 빈/에러 표시.
- [ ] Given worktree 가 여러 개인 repo Then rail 에 main + 모든 linked worktree 가 나열되고
      cwd 가 속한 worktree 에 `current` 마커가 표시된다.
- [ ] Given worktree 행 선택 Then status/log/diff 가 그 worktree 기준으로 전환되고
      실제 working dir/checkout 은 변하지 않는다.
- [ ] Given worktree 가 없는 일반 repo Then rail 에 main 한 항목만 표시된다(하위 호환).

## 화면

- [screens/git-viewer.md](screens/git-viewer.md) — git status/log/diff popup.
</content>
