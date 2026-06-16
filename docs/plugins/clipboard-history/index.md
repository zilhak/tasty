# Clipboard History (`com.tasty.clipboard-history`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (도구 메뉴 / 단축키 → popup)
- **배포/통합**: bundled · 도구 메뉴 항목 + popup — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-clipboard-history/`
- **권한**: `ui.tool_item` · `ui.popup` · `clipboard.read`
- **화면**: [screens/clipboard-history.md](screens/clipboard-history.md)

> **예제로서**: **도구 메뉴 항목 + popup** 예제이자 유일한 **wasm 양빌드**(`--features wasm`, `wasm32-wasip2` component) 레퍼런스 → [plugin-development](../../dev-guide/plugin-development.md#도구-메뉴-항목--popup).

## 목적

최근 클립보드 항목을 목록으로 보여주고 다시 복사하게 하는 **클립보드 히스토리 popup** 을 제공한다.

## 내부 동작

- **tool** `open-viewer` — [도구 메뉴](../../features/tools-menu/index.md)에 항목 추가(`ui.tool_item`), action `open_popup{com.tasty.clipboard-history/viewer}`.
- **popup** `viewer` — trigger `event shortcut.toggle_clipboard_viewer`(단축키)로도 열림. 클립보드 항목 목록 표시(`clipboard.read`).
- 항목 선택 → 다시 클립보드로 복사.

## 인터페이스

- **사용자**: 도구 메뉴 `Clipboard history` 또는 토글 단축키 → popup. 항목 클릭 → 복사.
- **AI Agent**: `clipboard_history.*` IPC 네임스페이스. (단발 클립보드 읽기/쓰기는 host `tasty clipboard` 별도.)

## 비-목표

- 시스템 클립보드 히스토리 *수집* 메커니즘 — host `ClipboardHistory`(메모리). 이 플러그인은 *표시/재복사* UI.
- 도구 메뉴 자체 — [tools-menu](../../features/tools-menu/index.md).

## Acceptance Criteria

- [ ] Given 플러그인 활성 Then 도구 메뉴에 `Clipboard history` 항목이 보인다.
- [ ] Given 토글 단축키 Then 히스토리 popup 이 열린다.
- [ ] Given 항목 클릭 Then 그 내용이 클립보드로 복사된다.

## 화면

- [screens/clipboard-history.md](screens/clipboard-history.md) — 히스토리 목록 popup.
</content>
