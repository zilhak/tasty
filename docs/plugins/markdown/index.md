# Markdown Viewer (`com.tasty.markdown`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (GUI surface) · AI Agent (`tasty markdown` CLI)
- **배포/통합**: bundled · surface_kind(webview) · 파일 핸들러 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-markdown/`(`crates/tasty-plugin-markdown/src/render.rs` = `pulldown-cmark` → sanitize(`ammonia`) → CSS 주입 HTML 문서 생성, `crates/tasty-plugin-markdown/src/main.rs` surface 라이프사이클/네비게이션/팝업,
  `crates/tasty-plugin-markdown/src/watch.rs` idle 자동 리로드) — [ADR-0065](../../adr/0065-markdown-webview-render-channel.md)(EguiMesh→Webview 전환 결정) · [ADR-0028](../../adr/0028-plugin-egui-mesh-render-channel.md)(egui-mesh 채널, 팝업만 계속 사용)
- **권한**: `surface.read/write`, `fs.read`(파일 읽기 + 링크 dispatch), `file_handler.*`, `ui.settings_page`, `ui.popup`(file-open/large-file-confirm 만 자가 렌더 — 본문은 아님) (매니페스트 `permissions`)
- **화면**: [screens/markdown.md](screens/markdown.md)

> **예제로서**: webview surface(plugin 이 sanitize 된 HTML 문서를 생성, host native WebView 가 렌더) + **파일 detector/handler** + cli + settings_page 를 한 플러그인에 모은 예제 → [plugin-development](../../dev-guide/plugin-development.md#파일-핸들러-detector--handler).

## 목적

마크다운 파일을 렌더해 보는 **`markdown` surface 종류**를 제공한다. `rendering = "webview"`([ADR-0065](../../adr/0065-markdown-webview-render-channel.md)) — 플러그인이 `pulldown-cmark` 로 markdown 을 HTML 로 변환하고 `ammonia` 로 sanitize 한 뒤, Theme 토큰을 CSS custom property 로 주입한 `<style>` 을 문서에 인라인해 host 의 native OS WebView(WebKitGTK/WKWebView/WebView2) overlay 에 올린다(`webview.set_url`). host 는 그 문서의 픽셀에 관여하지 않는다 — egui-mesh 시절과 달리 host 가 mesh 를 합성하지 않는다.

## 내부 동작

- **surface_kind `markdown` (webview)** — plugin 이 `MdDoc`(파일 경로·내용·base_dir·load_error)를 소유하고, `render::render_document` 가 완전한 `<!doctype html>` 문서 하나를 생성한다: `<base href="file:///<base_dir>/">`(상대 경로 이미지/링크 앵커) + `<style>`(CSS custom property, Theme→토큰) + 주소창 `<input>`+`<button>` + 본문(sanitize 된 HTML) + 신뢰된 인라인 `<script>`(주소창 Go/Enter + 스크롤 복원). display_name 은 파일명.
  - **헤딩 사다리**: `render::heading_sizes_px` 가 `font-size-prose-h1`(h1)↔`font-size-body`(h6) 사이를 5단계 **선형보간**한다 — CSS 라 라이브러리 제약 없이 이 보간 자체가 plugin 의 디자인 선택이다(원하면 per-level 값을 자유롭게 override 가능).
  - **표(GFM)**: 실제 `<table>`/`<th>`/`<td>` — header 밴드·zebra(`tr:nth-child(even)`)·셀 패딩 전부 CSS 로 직접 달성(egui `Grid` 우회 불필요).
  - **코드블록**: 펜스드 언어 태그(` ```rust `)는 `class="language-rust"` 로 `<code>` 에 살아남는다(`render::sanitize_fence_lang` 이 `[A-Za-z0-9_+-]` 로 정규화 후 ammonia 화이트리스트에 `code`/`class` 를 명시 허용) — 향후 mermaid fenced block(`language-mermaid`) 식별의 전제. 실제 syntax highlighting/mermaid 렌더는 아직 미배선(후속).
- **sanitize (XSS 방어의 1차 관문)** — `ammonia::Builder` 최소 화이트리스트: `<script>`/이벤트 핸들러 속성(`onerror=` 등)/`javascript:` scheme href 전부 stripped. `classify_link` 도 별도로 `javascript:` 를 판정 불가(`None`)로 취급해 이중 방어. GFM 렌더에 필요한 태그(`table`/체크박스 `input[type=checkbox]`/`del`/footnote `sup`/`div`/이미지/링크)만 허용 — 코드는 `crates/tasty-plugin-markdown/src/render.rs::sanitize_html` 이 정본.
- **리로드·삭제 처리** — `markdown.reload` IPC(명시 호출)와 idle-watch 감지 리로드가 **동일한 단일 함수**(`MarkdownPlugin::markdown_reload`)로 수렴한다 — 실제 read(`MdDoc::force_reload`)와 문서 재생성(`reload_webview`)이 항상 이 한 경로만 타므로, 빠른 연속 편집이 와도 "stale read 가 최신 값을 덮어쓰는" 레이스가 애초에 생기지 않는다(plugin runtime 은 단일 스레드 dispatch 루프). 파일이 외부 삭제로 사라지면 error 상태(`markdown.state.failed`)로 표시하고, 다시 생기면 자동 복구한다.
- **idle auto-reload(입력 없이도 갱신)** — webview-kind surface 는 `surface.set_context`/`paint`(=egui-mesh 전용 forward)를 **아예 받지 않는다** — 그래서 idle watch(`crates/tasty-plugin-markdown/src/watch.rs`)가 사실상 유일한 자동 갱신 경로다. `on_start` 에서 별도 스레드를 띄워 `RELOAD_CHECK_INTERVAL_SECS`(1초) 주기로 열려 있는 모든 markdown surface 의 파일 mtime 을 stat 하고, 변경을 감지하면 host 를 왕복해 **plugin 자신의** `markdown.reload` IPC 를 호출한다(`host.call`) — CLI/사용자가 같은 메서드를 부르는 것과 동일 요청이 단일 dispatch 루프에 직렬 도착해 위 레이스-없음 보장을 그대로 공유한다.
- **콘텐츠 전달** — surface 생성 시 host 가 `surface.create{file}` 를 plugin 에 보낸다. plugin 이 파일을 직접 읽는다(`fs.read`).
- **Theme parity** — webview-kind surface 는 Theme 이 자동으로 push 되지 않으므로(egui-mesh 의 `set_context.theme` 와 달리), plugin 이 문서를 (재)생성할 때마다 host 의 read-only **`theme.query`** IPC 로 현재 색+`is_light`+UI zoom 을 직접 조회한다. 이후 색이 바뀌면 host 가 발행하는 **`theme.changed`** 이벤트(매니페스트 `event_subscribe`)를 구독해 열려 있는 모든 markdown 문서를 재생성한다.
- **JS 는 기본 허용** — host 의 webview 설정 기본값은 plugin 마다 다르다: html 플러그인은 JS 기본 차단(임의 원격 콘텐츠를 열 수 있어서), markdown 은 **기본 허용**(`resolve_webview_settings` 의 per-plugin override, `src/view/main/redraw.rs`) — 신뢰된 주소창/네비게이션 스크립트가 항상 실행돼야 하고, 실제 markdown 콘텐츠 자체는 이미 별도로 sanitize 되므로 JS 허용이 추가 공격면을 만들지 않는다.
- **인라인 이미지** — 문서 `<head>` 의 `<base href="file:///<base_dir>/">` 가 상대 경로 image `src`/링크 `href` 를 앵커하고, 나머지는 평범한 `<img>` 태그로 브라우저 엔진이 직접 로드한다(host/plugin 관여 없음 — egui 텍스처 파이프라인 완전히 벗어남). `<base>` 는 이미 스킴이 있는 절대경로/원격 URL 은 건드리지 않으므로, 옛 `image_uri_prefix` 가 갖고 있던 "절대경로 이미지가 상대경로처럼 잘못 앵커되는" 버그 자체가 없다. 지원 포맷은 브라우저 엔진이 지원하는 전부(PNG/JPEG/GIF/SVG/WebP 등 — `image` crate 의 PNG/JPEG 제한이 사라짐).
- **파일 핸들러** — `detector "markdown"`(확장자 매핑) + `handler` action `open_surface{surface_kind:"markdown"}`. 마크다운 파일 열기 시 이 surface 로 뜬다.
- **파일열기/대용량 확인 팝업** — plugin 매니페스트 `[[contributes.popup]] id="file-open"`/`id="large-file-confirm"` — **egui-mesh 로 남아 있다**(Stage B 는 본문 렌더만 webview 로 전환, 확인 팝업 두 개는 그대로). 경로 입력 필드 + **찾아보기** + 열기/취소로 구성한다. **두 경로로 열린다**: (1) surface_kind 매니페스트 `convert_input_popup = "file-open"` capability — host 가 convert 팝업/`open_markdown`·`convert_to_markdown` 단축키/context menu 진입점에서 이 팝업을 `open_popup_instance` 로 직접 연다([ADR-0043](../../adr/0043-convert-input-popup-capability.md)), (2) event trigger `com.tasty.markdown.file_open`. **찾아보기**는 host generic **`file_picker.trigger {filters,owner_popup_instance?} → {request_id}`** IPC 로 host 소유 in-app `file_picker` 팝업을 연다([ADR-0058](../../adr/0058-plugin-triggered-host-popup-async-ack-push.md) — 옛 native rfd 다이얼로그 위임 [ADR-0042](../../adr/0042-fs-pick-file-native-dialog-host-delegation.md) 를 대체). 호출은 `request_id` 만 즉시 받고 선택 결과는 `file_picker.result` 이벤트로 비동기 도착한다. 이때 `owner_popup_instance` 로 **file-open 자신의 popup instance** 를 신고해 두 팝업이 부모-자식 스택을 이룬다([ADR-0084](../../adr/0084-plugin-triggered-host-popup-ownership.md)) — 피커가 떠 있는 동안 file-open 이 바깥 클릭으로 닫히지 않고, Esc 는 위쪽 피커부터 한 단계씩 닫으며, file-open 이 먼저 닫히면 피커도 함께 정리되어 고른 파일이 유실되지 않는다. **열기 확정 시 open context 의 `surface_id` 유무로 분기**한다: 있으면 그 surface 를 제자리 변환(host `markdown.navigate {surface_id,path}` — convert-to-markdown), 없으면 새 탭으로 연다(host `file_handler.dispatch {path,depth:"deep"}` — open-markdown). `file_picker.trigger` 는 **`fs.read`** 권한으로 게이트.
- **링크 클릭 라우팅** — 문서 안의 모든 non-anchor 링크 destination 은 HTML 생성 시점에 내부 nav-fragment 스킴(`#tasty-nav:link:<percent-encoded-dest>`)으로 rewrite 된다(`render::rewrite_link_dest`) — 실제 `href` 를 그대로 두면 native WebView 가 진짜 파일/미지 스킴으로 navigate 해버려(host 는 *원격* http(s) 만 차단) 렌더된 문서가 그 자리에서 깨진다. fragment 만 바뀌는 same-document navigation 은 (a) WebKitGTK 의 `decide-policy` 로는 여전히 캡처되지만(→ host 가 `webview.navigation_attempt` 이벤트로 forward) (b) 실제 페이지 리로드는 일으키지 않는다(실측 검증됨) — 이 성질로 "클릭을 가로채되 화면은 안 깨지는" 신호 채널을 만든다. plugin 의 `on_webview_navigation_attempt` 핸들러가 그 이벤트를 받아 `render::parse_nav_fragment`+`classify_link` 로 판정한다:
  - **상대 경로**(`docs/index.md`, `../sibling.md`)는 **현재 마크다운 파일의 폴더(base_dir) 기준**으로 절대화(프로세스 cwd 아님). 절대 경로는 그대로. `javascript:` 는 무조건 무시(sanitize 와 별개의 2차 방어).
  - **외부 URL**(`http(s)://`·`mailto:`·`data:`)만 plugin 이 OS 핸들러(`webbrowser`)로 위임.
  - 파일 링크는 host `file_handler.dispatch` 로 보낸다(Explorer "파일 열기" 와 동일한 `DispatchFile` 경로 — 그 surface 가 속한 **Pane 의 새 탭**, 포커스 전환 없음).
- **주소창** — 더 이상 host egui 위젯(옛 `PathField`)이 아니다. `render::addr_bar_html` 이 문서 자체에 `<input list="tasty-addr-recent">`+native `<datalist>`(최근 경로, 브라우저 내장 autocomplete — 커스텀 드롭다운 JS 불필요)+Go `<button>` 을 굽는다. 문서 생성 시점의 *현재* 경로/최근목록으로 baked 되며, webview 에는 JS↔plugin 실시간 메시지 브리지가 없어(module doc 참고) 옛 PathField 처럼 포커스 시 다시 fetch 하는 반응형 동작은 없다 — Go 클릭/Enter 는 신뢰된 인라인 `<script>`(`render::nav_script`)가 `location.hash = 'tasty-nav:addr:' + encodeURIComponent(v)` 로 위 nav-fragment 채널에 태워 보낸다.
- **최근목록 조회** — host 의 generic **`recent.query {kind}`** IPC 가 그 kind 의 최근 연 파일을 **최신순 최대 10개** 반환한다(`{recent:[{path,file_name}]}`). markdown plugin 은 `kind:"markdown"` 을 채워 호출해 위 주소창 `<datalist>` 를 채운다. **읽기 전용** — host `AppState.recent_files` 캐시를 필터 없이 조회할 뿐 사용자 상태를 바꾸지 않는다(불가침 원칙). **`surface.read`** 권한. recent 기록 대상 여부는 매니페스트 `records_recent` capability 로 판정.
- **스크롤 위치 보존(best-effort)** — idle-watch 자동 리로드와 `markdown.reload` 는 전체 문서를 `load_html` 로 통째로 교체한다(부분 DOM patch 없음) — 이대로면 native WebView 의 스크롤이 매 리로드마다 0 으로 리셋된다. `render::nav_script` 가 `sessionStorage`(파일 경로로 키잉, `scroll` 이벤트 150ms 디바운스 저장 + 로드시 복원)로 이를 완화한다. same-origin 스코프 의존이라 `load_html` 반복 호출이 origin identity 를 유지하는지는 webview 엔진(WebKitGTK/WKWebView/WebView2)마다 다를 수 있어 3종 전체 검증되지 않았다 — 실패해도 "복원 안 됨"으로 우아하게 저하될 뿐 에러는 없다.
- **cli** — `tasty markdown recent`(최근목록 조회 — plugin CLI 서브커맨드가 `recent.query` 로 trampoline). reload 등도 plugin CLI.
- **settings_page** — `markdown` 페이지.

## 인터페이스

- **사용자**: 마크다운 파일 열기 → markdown surface, 또는 surface 종류 전환.
- **AI Agent**: `tasty markdown …` CLI / `markdown.*` IPC. surface 생성은 [work-area](../../features/work-area/index.md) (`--type markdown`).

## 비-목표

- surface 배치/생성 도메인 — [work-area](../../features/work-area/index.md).
- 픽셀/타이포 — design-system.

## Acceptance Criteria

- Given markdown 플러그인 활성 When 마크다운 파일 열기 Then markdown surface 로 렌더된다.
- Given `tasty new tab --type markdown --file <f>` Then 그 파일이 렌더된다.
- Given `tasty list surfaces` Then 해당 surface 가 `kind:"markdown"` 으로 보고된다.

## 화면

- [screens/markdown.md](screens/markdown.md) — 마크다운 렌더 surface.
</content>
