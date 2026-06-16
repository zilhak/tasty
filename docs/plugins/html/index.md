# HTML Viewer (`com.tasty.html`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (GUI surface) · AI Agent (`tasty html` CLI)
- **배포/통합**: bundled · surface_kind(webview) · 파일 핸들러 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-html/`, host WebView 오버레이
- **권한**: 매니페스트 `permissions`
- **화면**: [screens/html.md](screens/html.md)

> **예제로서**: `rendering = "webview"` surface 의 예제 → [plugin-development](../../dev-guide/plugin-development.md#surface-kind--rendering-3-종).

## 목적

HTML / 웹 콘텐츠를 보는 **`html` surface 종류**를 제공한다. `rendering = "webview"` — tasty 의 **네이티브 WebView 오버레이**로 그린다(host 가 surface 별 URL 을 동기화).

## 내부 동작

- **surface_kind `html` (webview)** — host 트리엔 `RemoteSurface` marker, 실제 콘텐츠는 네이티브 WebView 오버레이. surface 의 `webview_url()` 로 URL 식별.
- **파일 핸들러** — `handler` `open_surface{surface_kind:"html"}`. `detector "html"` 은 **host 가 유지**(`default-file-format.toml`) — 플러그인 disable 시에도 확장자 인식이 남도록. HTML 파일 열기 시 이 surface.
- **cli** — `tasty html open …`. `html.*` IPC(URL 설정 등 — `webview.set_url`).

## 인터페이스

- **사용자**: HTML 파일 열기 → html surface(WebView).
- **AI Agent**: `tasty html …` CLI / `html.*` IPC. surface 생성은 [work-area](../../features/work-area/index.md) (`--type html --url …`).

## 비-목표

- WebView 오버레이 동기화 메커니즘 — host(gpu/webview) 구현.
- surface 배치/생성 도메인 — [work-area](../../features/work-area/index.md).

## Acceptance Criteria

- [ ] Given html 플러그인 활성 When `tasty new tab --type html --url <u>` Then WebView surface 가 그 URL 을 띄운다.
- [ ] Given HTML 파일 열기 Then html surface 로 뜬다.
- [ ] Given 플러그인 disable Then `html` 확장자 detector 는 host 가 유지한다.

## 화면

- [screens/html.md](screens/html.md) — WebView surface.
</content>
