# Clipboard Viewer (`com.tasty.clipboard-viewer`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (도구 메뉴 / 단축키 → popup)
- **배포/통합**: bundled · 도구 메뉴 항목 + popup — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-clipboard-viewer/`
- **권한**: `ui.tool_item` · `ui.popup` · `clipboard.read`
- **화면**: [screens/clipboard-viewer.md](screens/clipboard-viewer.md)

> **예제로서**: **도구 메뉴 항목 + popup**(master-detail) 예제. 클립보드를 host 백엔드 없이 **plugin 프로세스가 `arboard` 로 직접 read** 하는 [ADR-0009](../../adr/0009-plugin-sandbox-deferred.md) 비-샌드박스 모델의 레퍼런스 → [plugin-development](../../dev-guide/plugin-development.md#도구-메뉴-항목--popup).

## 목적

현재 시스템 클립보드의 내용을 **타입별로 분류해 미리보기**하는 read-only popup 을 제공한다. 히스토리(과거 항목 누적)는 다루지 않는다 — 지금 클립보드에 무엇이 들어 있는지 보여줄 뿐이다. 타입에 따라 미리보기 방식이 다르다 — Text 는 본문을 그대로 보여주지만, Image 는 **인라인 렌더링을 하지 않는다**(design-system 이 명시적으로 내린 결정, TODO48): 아이콘 + 치수/용량 메타 + "인라인 미리보기 없음" 안내 문구만 표시한다.

## 내부 동작

- **tool** `open-viewer` — [도구 메뉴](../../features/tools-menu/index.md)에 항목 추가(`ui.tool_item`), action `open_popup{com.tasty.clipboard-viewer/viewer}`.
- **command** `open_viewer` — 단축키로도 뷰어를 연다(`scope = "global"`, 기본값 `ctrl+shift+h` — 설정 > 단축키 > 플러그인에서 변경 가능). action 은 tool 항목과 동일한 `open_popup{com.tasty.clipboard-viewer/viewer}`(TODO43 — 구 호스트 하드코딩 `toggle_clipboard_viewer` 전용 필드에서 [git-viewer](../git-viewer/index.md)와 동일한 플러그인 커맨드 레지스트리로 마이그레이션).
- **popup** `viewer` — trigger `ipc`. header(아이콘+타이틀+snapshot 뱃지+close) → type-bar(타입 1개면 아이콘+뱃지, 2개 이상이면 가로 세그먼트 스위치) → body(well 스크롤 미리보기) → footer(mime+Close) 4단 구조(TODO51 — design-system 구조 전사, 이전의 좌측 rail master-detail 레이아웃은 폐기).
- popup open 시 `arboard::Clipboard` 로 클립보드를 1회 읽어 사용 가능한 타입 목록을 만든다. 현재 Text/Files/Image 타입이 채워진다(TODO52 — Files 는 `arboard::Get::file_list()` · TODO48 — Image 는 `arboard::Clipboard::get_image()` 로 치수(width/height)와 바이트 수만 보존하고 픽셀 데이터 자체는 들고 있지 않는다, 렌더링을 안 하므로 필요 없다) — HTML/기타 포맷은 자매 작업이 추가한다. 타입 선택 → body 미리보기 갱신.
- **단일 인스턴스**: 이미 열려 있으면 재호출은 무시(`already_open`).

## 인터페이스

- **사용자**: 도구 메뉴 `Clipboard Viewer` 또는 설정 > 단축키 > 플러그인에서 지정한 단축키 → popup. type-bar 에서 타입 선택 → body 에서 내용 확인.
- **AI Agent**: 단발 클립보드 읽기/쓰기는 host 가 아닌 각 에이전트 프로세스의 직접 접근 영역이다(ADR-0009). 이 plugin 은 IPC 네임스페이스를 노출하지 않는 순수 뷰어다.

## 비-목표

- 클립보드 **히스토리** 수집·재복사 — 제거됨(host `ClipboardHistory` 백엔드 폐기). 이 plugin 은 *현재* 클립보드 표시만 한다.
- 클립보드 **쓰기/편집** — read-only.
- 도구 메뉴 자체 — [tools-menu](../../features/tools-menu/index.md).

## Acceptance Criteria

- [ ] Given 플러그인 활성 Then 도구 메뉴에 `Clipboard Viewer` 항목이 보인다.
- [ ] Given 단축키(플러그인 커맨드 `open_viewer`) Then 뷰어 popup 이 열린다.
- [ ] Given 클립보드에 텍스트가 있음 Then type-bar 에 text 타입 뱃지가 보이고 body 에 내용이 미리보기된다.
- [ ] Given 클립보드에 파일(경로 목록)이 있음 Then type-bar 에 files 타입이 보이고 선택 시 body 에 아이콘+경로 목록이 한 줄씩 표시된다.
- [ ] Given 클립보드에 이미지가 있음 Then type-bar 에 image 타입이 노출되고, 선택 시 body 에 아이콘 + 치수/용량 메타 + "인라인 미리보기 없음" 안내 문구가 보인다(실제 그림은 렌더링하지 않음).
- [ ] Given 클립보드가 비어 있음 Then 빈 상태 메시지가 보인다.

## 화면

- [screens/clipboard-viewer.md](screens/clipboard-viewer.md) — master-detail 뷰어 popup.
