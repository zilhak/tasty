# HTML Viewer (`com.tasty.html`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (GUI surface) · AI Agent (`tasty html` CLI)
- **배포/통합**: bundled · surface_kind(webview) · 파일 핸들러 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-html/`, host WebView 오버레이
- **권한**: 매니페스트 `permissions`
- **화면**: [아래 절](#화면)

> **예제로서**: `rendering = "webview"` surface 의 예제 → [plugin-development](../../dev-guide/plugin-development.md#surface-kind--rendering-3-종).

## 목적

HTML / 웹 콘텐츠를 보는 **`html` surface 종류**를 제공한다. `rendering = "webview"` — tasty 의 **네이티브 WebView 오버레이**로 그린다(host 가 surface 별 URL 을 동기화).

## 내부 동작

- **surface_kind `html` (webview)** — host 트리엔 `RemoteSurface` marker, 실제 콘텐츠는 네이티브 WebView 오버레이. surface 의 `webview_url()` 로 URL 식별.
- **파일 핸들러** — `handler` 둘: `viewer`(detector `html`)와 `svg-viewer`(detector `svg`), 둘 다 `open_surface{surface_kind:"html", param_key:"url"}`. `detector "html"`·`detector "svg"` 는 **host 가 유지**(`default-file-format.toml`) — 플러그인 disable 시에도 확장자 인식이 남도록. HTML·SVG 파일 열기 시 이 surface. SVG 는 image 플러그인이 디코드하지 못하므로 WebView 가 렌더하며, `svg` detector 에 붙는 기본 핸들러가 이것 하나라 picker 없이 열린다.
- **cli** — `tasty html open …`. `html.*` IPC(URL 설정 등 — `webview.set_url`).
- **스크립트 감지와 문서 단위 허용** — 전역 설정 sandbox scripts가 켜져 있으면 host가 main frame 문서마다 JS를 끈다. `file://` 문서는 응답 단계에서 원본 파일을 읽어 스크립트를 감지하고 파일 전체 지문을 구한다. 허용은 URL(fragment 제외)과 지문에 묶이며 다른 문서가 commit되면 풀린다. 규칙과 근거는 [ADR-0053](../../adr/0053-html-script-detection-and-per-document-allowance.md).
  - 배너 발화 판정은 `tasty_model::html_script::banner`에 있다. 사용자가 그 문서를 봤을 때만 배너 단계가 `blocked`가 되고, 에이전트가 연 문서·세션 복원·보이지 않는 탭의 문서는 `pending_view`만 기록한다. 판정은 포커스와 활성 탭을 바꾸지 않는다. 배너와 탭 마커를 그리는 부분은 아직 없다.
  - 조회: `tasty surface html-script --surface <id>`(IPC `surface.html_script`)가 현재 문서의 URL·감지 결과(`none`/`scripts`/`scripts_remote_only`)·지문, 허용 기록, 현재 JS 적용 값, 배너 단계(`hidden`/`blocked`/`reloading`)와 표지(`viewed`·`pending_view`·`shown`·`dismissed`)를 돌려준다. 읽기만 하므로 배너를 띄우거나 사용자가 본 것으로 기록하지 않는다. html이 아닌 surface는 거절하고 헤드리스 빌드는 빌드 미지원 오류(`-32017`)로 답한다.
  - 허용: release에서는 사용자만 GUI로 허용한다. 에이전트용 허용 재현은 debug 전용 `debug.html_script.allow`다([debug IPC](../../dev-guide/debug-ipc.md)).

## 인터페이스

- **사용자**: HTML·SVG 파일 열기 → html surface(WebView).
- **AI Agent**: `tasty html …` CLI / `html.*` IPC. surface 생성은 [work-area](../../features/work-area/index.md) (`--type html --url …`).
- **AI Agent**: 스크립트 감지·허용·배너 상태 조회는 `tasty surface html-script --surface <id>`(IPC `surface.html_script`).

## 비-목표

- WebView 오버레이 동기화 메커니즘 — host(gpu/webview) 구현.
- surface 배치/생성 도메인 — [work-area](../../features/work-area/index.md).

## Acceptance Criteria

- Given html 플러그인 활성 When `tasty new tab --type html --url <u>` Then WebView surface 가 그 URL 을 띄운다.
- Given HTML 파일 열기 Then html surface 로 뜬다.
- Given 플러그인 disable Then `html` 확장자 detector 는 host 가 유지한다.
- Given html 플러그인 활성 When 탐색기에서 `.svg` 파일을 더블클릭 Then 핸들러 선택 창 없이 html surface 새 탭이 열려 SVG 를 렌더한다.
- Given sandbox scripts가 켜져 있고 에이전트가 스크립트가 있는 `file://` 문서를 html 탭으로 열었다 When `tasty surface html-script --surface <id>` Then `document.detection`은 `scripts`, `javascript`는 `false`, `banner.phase`는 `hidden`, `banner.pending_view`는 `true`이고 조회 전후 상태가 같다.
- Given 같은 문서 When 사용자가 그 surface를 선택한다 Then 다음 조회의 `banner.phase`는 `blocked`다.
- Given 터미널이나 markdown surface When `surface.html_script` Then invalid params로 거절한다.
- Given 플러그인 disable When `.svg` 파일을 열기 Then `svg` detector 는 남고 핸들러가 없어 선택 창이 뜨며, 헤더 형식 표시는 `svg` 다.

## 화면

화면정의서 — **HTML surface 화면**.

- **시각 소스**: 네이티브 WebView 오버레이 — 콘텐츠는 OS WebView 가 그린다(디자인 토큰 무관).

[작업 영역](../../features/work-area/index.md#화면) 타일 위치에 WebView 오버레이로 그려지는 HTML surface.

### 트리거

HTML·SVG 파일 열기 또는 `html` surface 생성(`--url`).

### UI 요소 인벤토리

- **WebView 콘텐츠** — URL/파일의 웹 렌더. 타일 rect 에 오버레이로 정렬.
- 탭 표시명은 파일명/URL.

### 상태별 시각

surface 는 트리에선 `RemoteSurface` marker. 네이티브 WebView 의 navigation 생명주기
(start/finish/fail)를 3 backend(WebView2 / WKNavigationDelegate / WebKitGTK)가 `NavState`
(Idle/Loading/Done/Failed)로 host 에 전달하고, host 가 그 상태에 따라 chrome 을 그린다:

- **Idle** — URL 미지정. placeholder(`GLOBE` · "No page loaded").
- **Loading** — 탐색 중. WebView overlay 를 숨기고 `Spinner` + "Loading…" chrome.
- **Done** — 성공. WebView overlay 가 페이지를 그린다(메뉴/팝업으로 overlay 가 일시
  숨겨질 때만 boundary chrome backdrop 노출).
- **Failed** — 실패. overlay 를 숨긴 채 `ALERT_CIRCLE`(`accent-danger`) + "Failed to load"
  + URL chrome. 실패 사유는 화면 대신 `tracing::warn!` 로그로만 남긴다.

### 디자인 토큰 매핑

페이지 콘텐츠는 해당 페이지의 스타일을 따른다. Tasty가 그리는 로딩·오류·빈 상태에는 공용 Theme 토큰을 사용한다.

### 갤러리 specimen

`crates/tasty-gallery/src/catalog/components/html_chrome.rs` — Layouts › `Content viewers` ›
`HTML (webview) chrome`. boundary / placeholder / loading / error 4 chrome 상태만 전사(콘텐츠는
overlay). 3자 매핑: [design-gallery-mapping.md](../../design/systems/design-gallery-mapping.md#surface-viewers-plugins).

### 시각 소스

콘텐츠는 OS 네이티브 WebView 가 렌더하므로 design-system 토큰이 적용되지 않는다(웹 페이지 자체 스타일). 타일 정렬/경계만 작업영역 레이아웃을 따른다.
