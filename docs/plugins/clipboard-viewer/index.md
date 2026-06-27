# Clipboard Viewer (`com.tasty.clipboard-viewer`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (도구 메뉴 / 단축키 → popup)
- **배포/통합**: bundled · 도구 메뉴 항목 + popup — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-clipboard-viewer/`
- **권한**: `ui.tool_item` · `ui.popup` · `clipboard.read`
- **화면**: [screens/clipboard-viewer.md](screens/clipboard-viewer.md)

> **예제로서**: **도구 메뉴 항목 + popup**(master-detail) 예제. 클립보드를 host 백엔드 없이 **plugin 프로세스가 `arboard` 로 직접 read** 하는 [ADR-0009](../../adr/0009-plugin-sandbox-deferred.md) 비-샌드박스 모델의 레퍼런스 → [plugin-development](../../dev-guide/plugin-development.md#도구-메뉴-항목--popup).

## 목적

현재 시스템 클립보드의 내용을 **타입별로 분류해 미리보기**하는 read-only popup 을 제공한다. 히스토리(과거 항목 누적)는 다루지 않는다 — 지금 클립보드에 무엇이 들어 있는지 보여줄 뿐이다.

## 내부 동작

- **tool** `open-viewer` — [도구 메뉴](../../features/tools-menu/index.md)에 항목 추가(`ui.tool_item`), action `open_popup{com.tasty.clipboard-viewer/viewer}`.
- **popup** `viewer` — trigger `event shortcut.toggle_clipboard_viewer`(단축키)로도 열림. master-detail 레이아웃: 좌측 타입 목록(Button) + 우측 내용 미리보기(`scroll_v(text_preview)`).
- popup open 시 `arboard::Clipboard` 로 클립보드를 1회 읽어 사용 가능한 타입 목록을 만든다. 좌측 타입 클릭 → 우측 미리보기 갱신.
- **단일 인스턴스**: 이미 열려 있으면 재호출은 무시(`already_open`).

## 인터페이스

- **사용자**: 도구 메뉴 `Clipboard Viewer` 또는 토글 단축키 → popup. 좌측 타입 선택 → 우측에서 내용 확인.
- **AI Agent**: 단발 클립보드 읽기/쓰기는 host 가 아닌 각 에이전트 프로세스의 직접 접근 영역이다(ADR-0009). 이 plugin 은 IPC 네임스페이스를 노출하지 않는 순수 뷰어다.

## 비-목표

- 클립보드 **히스토리** 수집·재복사 — 제거됨(host `ClipboardHistory` 백엔드 폐기). 이 plugin 은 *현재* 클립보드 표시만 한다.
- 클립보드 **쓰기/편집** — read-only.
- 도구 메뉴 자체 — [tools-menu](../../features/tools-menu/index.md).

## Acceptance Criteria

- [ ] Given 플러그인 활성 Then 도구 메뉴에 `Clipboard Viewer` 항목이 보인다.
- [ ] Given 토글 단축키 Then 뷰어 popup 이 열린다.
- [ ] Given 클립보드에 텍스트가 있음 Then 좌측에 text 타입이 보이고 우측에 내용이 미리보기된다.
- [ ] Given 클립보드가 비어 있음 Then 빈 상태 메시지가 보인다.

## 화면

- [screens/clipboard-viewer.md](screens/clipboard-viewer.md) — master-detail 뷰어 popup.
