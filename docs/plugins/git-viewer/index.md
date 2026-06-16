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

## 내부 동작

- **tool** `open-viewer` — [도구 메뉴](../../features/tools-menu/index.md)에 항목 추가(`ui.tool_item`), action `open_popup{com.tasty.git-viewer/viewer}`.
- **popup** `viewer` — trigger `ipc`(IPC 로도 열림). status/log/diff 를 `fs.read` 로 읽어 표시. **읽기 전용**(커밋/스테이징 등 변경 없음).

## 인터페이스

- **사용자**: 도구 메뉴 `Git viewer` → popup.
- **AI Agent**: popup trigger 가 ipc — IPC 로 열 수 있다.

## 비-목표

- git *쓰기*(커밋/스테이징/브랜치 조작) — 읽기 전용.
- 상태바의 브랜치 표시 — [workspace-status-bar](../../features/workspace-status-bar/index.md)(별개).

## Acceptance Criteria

- [ ] Given 플러그인 활성 Then 도구 메뉴에 git viewer 항목이 보인다.
- [ ] Given repo 안에서 popup 열기 Then status/log/diff 가 표시된다.
- [ ] Given 비-repo Then 적절한 빈/에러 표시.

## 화면

- [screens/git-viewer.md](screens/git-viewer.md) — git status/log/diff popup.
</content>
