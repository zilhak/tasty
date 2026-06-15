# Git 뷰어 (builtin plugin)

- **Status**: Implemented

### 개요
**read-only MVP**: 활성 surface 의 cwd 에서 git repo 를 찾아 상단에는 working tree 변경 목록을, 하단에는 커밋 평면 리스트(또는 선택된 파일의 diff)를 표시하는 popup. 모든 동작은 **read-only** — stage/commit/checkout 등 mutate 작업은 없다 (그건 터미널에서 직접 하라는 정책).

본바이너리에 박혀 있던 in-tree popup을 외부 plugin `com.tasty.git-viewer`로 분리해 release/dist 빌드 시 `plugins/` 디렉터리에 함께 배포된다. plugin 비활성 시 사이드바 도구 메뉴에서 항목이 사라지고 호스트는 git 관련 코드 0.

### 트리거
- 사이드바 도구 메뉴의 `Git` 항목 — plugin의 `[[contributes.tool]]` 로 노출.
- 클릭 시 호스트 `tools_menu::invoke_tool::OpenPopup` 분기가 활성 surface의 상속 cwd를 `context.cwd` 페이로드에 실어 `popup.open` IPC로 plugin에 전달.

### 동작
- 첫 진입 시 plugin process가 `git2::Repository::discover(cwd)` 로 repo 탐색.
- 상단(Changes): `M / A / D / R / ? / U` 아이콘 + 색상 (yellow/green/red/blue/overlay0/red), 파일 클릭 → 하단을 diff 패널로 전환. SelectableRow 위젯으로 행 강조.
- 하단 기본(Commits): 최근 200개 커밋, `[oid] (refs?) summary  author  time` 평면 리스트 — **그래프 없음**.
- 하단 diff: working tree vs HEAD 통합 (staged/unstaged 분리 없음). hunk 헤더(blue), `+`(green) / `-`(red) / context(text), 좌측 줄번호. `Back` 버튼으로 log 복귀.
- `Refresh` 버튼으로 status/log/diff 일괄 재수집.
- 단일 인스턴스 — 이미 열린 상태에서 다시 메뉴 클릭 시 "already open" placeholder만 표시.
- repo 없음/에러 시 안내 메시지.

### IPC 노출 없음
사용자 UI 편의 기능. 에이전트는 터미널에서 `git status`/`git log`/`git diff` 직접 호출하면 충분하므로 IPC 표면에 노출하지 않는다 (popup은 `trigger = "ipc"`이지만 호스트 내부 tool-action 경로로만 호출 가능).

### 구현
- crate: `crates/tasty-plugin-git-viewer/`
  - `src/git.rs` — git2 래핑 (discover/status/log/diff), 모두 read-only.
  - `src/view.rs` — UiNode tree 빌더. `SelectableRow` + `Label{ style: Mono, color }` 조합.
  - `src/main.rs` — Plugin impl, 단일 인스턴스 가드, popup event dispatch.
- manifest: popup 720×540, anchor=screen-center, dismiss_on_outside_click. permissions `ui.popup`, `ui.tool_item`, `fs.read`.
- 의존성: `git2 = "0.19"` (`default-features = false` — HTTPS/SSH 불필요, libgit2 vendored C 빌드). 본바이너리에는 더 이상 git2 의존 없음.
- i18n: plugin 자체 `lang/{en,ko,ja}.toml`. `tasty-plugin-sdk::i18n::Translator`가 `TASTY_LOCALE` 환경변수로 활성 언어 결정.

### 추후 항목
커밋 그래프, staged/unstaged 분리, 커밋 클릭 → 해당 커밋 diff, 브랜치/태그 목록, 자동 새로고침, 백그라운드 스레드 수집, 리사이즈 디바이더.
