# 도구 메뉴 (Tools menu)

- **Status**: Implemented
- **주체**: 로컬 사용자 (원격 접속 사용자는 mirror 로 봄)
- **ADR**: 없음
- **코드**: `src/adapters/ui/tools_menu.rs`, `src/adapters/ui/sidebar/tools.rs`
- **화면**: [screens/tools-menu.md](screens/tools-menu.md)

## 목적

[사이드바](../sidebar/index.md) 의 **도구 버튼**이 여는 메뉴. 빌트인 + 플러그인 기여 도구 항목을 한곳에 모아 실행 진입점을 제공한다. 메뉴 자체는 *진입점* 일 뿐 — 각 항목의 내용은 그 기능 문서가 가진다 (연결 개념).

## 내부 동작

### 두 출처

- **빌트인 항목** (`BUILTIN_TOOLS`, 플러그인 무관 — 현재 3개):
  - Command palette → 명령 팔레트 popup
  - Listening ports → 리스닝 포트 popup
  - SSH profiles → SSH 도구 popup
- **플러그인 기여 항목**: 활성 + `ui.tool_item` 권한을 grant 받은 plugin 이 `[[contributes.tool]]` 로 선언한 항목. `AppState::tool_registry` 에 동기화된다. (과거 호스트 빌트인이던 Clipboard History 등은 builtin-plugin 항목으로 이전됨.)

### 레이아웃 / 크기

빌트인 섹션 + (둘 다 있으면 구분선) + 플러그인 섹션. 메뉴 높이는 **현재 등록된 항목 수로 매 프레임 동적 계산**(`tools_menu_sizer`).

### 항목 실행 (`invoke_tool`)

- **빌트인**: 해당 popup 을 연다 (`BuiltinAction::OpenPopup`).
- **플러그인**: `ToolAction` 종류별 — event 발화 또는 `<plugin_id>/<popup_id>` 형식 popup open (활성 surface 의 상속 cwd 를 실어 전달).

## 인터페이스

- **사용자**: 사이드바 도구 버튼 클릭 → 메뉴 표시, 항목 클릭 → 실행.
- **각 항목은 그 기능으로 연결** (연결 개념):
  - Command palette → [`features/command-palette/`](../command-palette/index.md)
  - Listening ports → [`features/listening-ports/`](../listening-ports/index.md)
  - SSH profiles → [`features/remote-profiles/`](../remote-profiles/index.md)
  - 플러그인 기여 도구 → **[번들 플러그인 문서](../../plugins/index.md)** (예: [clipboard-history](../../plugins/clipboard-history/index.md) · [git-viewer](../../plugins/git-viewer/index.md)). 이 메뉴 문서에는 항목을 나열하지 않는다 — 공식 플러그인 메뉴는 해당 플러그인 쪽에서 다룬다.

## 비-목표

- 각 도구 항목의 *내용/동작* — 메뉴는 목록 + 실행 진입점만. 내용은 각 기능 문서.
- 플러그인 도구의 기여/권한 메커니즘 — `features/plugin-system/` 영역.

## Acceptance Criteria

- [ ] 사이드바 도구 버튼 클릭 시 빌트인 3개(Command palette / Listening ports / SSH profiles)가 표시된다.
- [ ] `ui.tool_item` 권한을 가진 활성 플러그인의 기여 항목이 빌트인 아래 구분선과 함께 추가된다.
- [ ] 항목 클릭 시 해당 popup(빌트인) 또는 plugin action(플러그인)이 실행된다.
- [ ] 등록 항목 수에 따라 메뉴 높이가 달라진다.

> GUI 메뉴라 시각 검증은 스크린샷, 항목 등록/실행은 `debug.tool.list`/`debug.tool.invoke`(debug IPC)로 검증 가능.

## 구현

- `src/adapters/ui/tools_menu.rs` — `BUILTIN_TOOLS`, `draw_tools_menu`, `invoke_tool`, `tools_menu_sizer`.
- `src/adapters/ui/sidebar/tools.rs` — `open_tools_menu` (도구 버튼 → 메뉴 popup).
- 플러그인 항목: `AppState::tool_registry` (plugin `[[contributes.tool]]` 동기화).

## 화면

- [screens/tools-menu.md](screens/tools-menu.md) — 메뉴 레이아웃과 각 항목의 연결.
