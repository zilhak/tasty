# Markdown Viewer (`com.tasty.markdown`)

- **Status**: Implemented (bundled plugin)
- **주체**: 로컬 사용자 (GUI surface) · AI Agent (`tasty markdown` CLI)
- **배포/통합**: bundled · surface_kind(webview) · 파일 핸들러 — [plugins 개념](../../concepts/plugins.md)
- **코드**: `crates/tasty-plugin-markdown/`(`crates/tasty-plugin-markdown/src/render.rs` = `pulldown-cmark` → sanitize(`ammonia`) → CSS 주입 HTML 문서 생성, `crates/tasty-plugin-markdown/src/main.rs` surface 라이프사이클/네비게이션, `crates/tasty-plugin-markdown/src/popup.rs` egui-mesh 팝업(file-open/large-file-confirm),
  idle 자동 리로드는 SDK 공용 `crates/tasty-plugin-sdk/src/file_watch.rs`) — [ADR-0029](../../adr/0029-webview-host-integration.md)(EguiMesh→Webview 전환 결정) · [ADR-0028](../../adr/0028-egui-mesh-rendering.md)(egui-mesh 채널, 팝업만 계속 사용)
- **권한**: `surface.read/write`, `fs.read`(파일 읽기 + 링크 dispatch), `file_handler.*`, `ui.settings_page`, `ui.popup`(file-open/large-file-confirm 만 자가 렌더 — 본문은 아님) (매니페스트 `permissions`)
- **화면**: [screens/markdown.md](screens/markdown.md)

> **예제로서**: webview surface(plugin 이 sanitize 된 HTML 문서를 생성, host native WebView 가 렌더) + **파일 detector/handler** + cli + settings_page 를 한 플러그인에 모은 예제 → [plugin-development](../../dev-guide/plugin-development.md#파일-핸들러-detector--handler).

## 목적

마크다운 파일을 렌더해 보는 **`markdown` surface 종류**를 제공한다. `rendering = "webview"`([ADR-0029](../../adr/0029-webview-host-integration.md)) — 플러그인이 `pulldown-cmark` 로 markdown 을 HTML 로 변환하고 `ammonia` 로 sanitize 한 뒤, Theme 토큰을 CSS custom property 로 주입한 `<style>` 을 문서에 인라인해 host 의 native OS WebView(WebKitGTK/WKWebView/WebView2) overlay 에 올린다(`webview.set_url`). 문서의 픽셀은 WebView가 렌더하며 host는 mesh를 합성하지 않는다.

## 내부 동작

- **surface_kind `markdown` (webview)** — plugin 이 `MdDoc`(파일 경로·내용·base_dir·load_error)를 소유하고, `render::render_document` 가 완전한 `<!doctype html>` 문서 하나를 생성한다: `<style>`(CSS custom property, Theme→토큰) + 주소창 `<input>`+`<button>` + 목차 `<nav>` + 본문(sanitize 된 HTML) + 신뢰된 인라인 `<script>`(주소창 Go/Enter + 문서 안 앵커 스크롤 + 스크롤 복원).
  **`<base href>` 는 없다** — 이 파이프라인이 내보내는 것 중 상대 URL 을 푸는 자리가 없고(이미지는 인라인, **마크다운 링크** destination 은 anchor-only 가 아닌 한 전부 nav fragment), base 가 있으면 fragment-only `href` 가 base URL 기준으로 풀려 목차 클릭이 다른 문서로 나가 버린다([ADR-0030](../../adr/0030-bundled-plugin-data.md)).
  저자가 직접 쓴 raw HTML `<a href="x.md">` 는 rewrite 대상이 아니라 상대 href 로 남지만, base 가 없으면 그것은 **파일로 풀리지 않는다**(클릭했을 때 엔진이 무엇을 하는지는 세 백엔드 어디에서도 측정되지 않았다).
  display_name 은 파일명.
  - **제목 크기**: `render::heading_sizes_px` 가 `font-size-prose-h1`(h1)↔`font-size-body`(h6) 사이를 5단계 **선형보간**한다 — CSS 라 라이브러리 제약 없이 이 보간 자체가 plugin 의 디자인 선택이다(원하면 per-level 값을 자유롭게 override 가능).
  - **표(GFM)**: 실제 `<table>`/`<th>`/`<td>` — header 밴드·zebra(`tr:nth-child(even)`)·셀 패딩 전부 CSS 로 직접 달성(egui `Grid` 우회 불필요).
  - **코드블록**: `sanitize_fence_lang`은 언어 태그를 `[A-Za-z0-9_+-]`로 정리하고 `code`의 class를 유지한다. `render_document`는 생성한 본문 HTML에 `class="language-`가 있으면 highlight.js를, `language-mermaid`가 있으면 mermaid.js를 넣는다. 문자열 포함 검사이므로 실제 코드 블록 외의 본문도 이 조건에 걸릴 수 있다. 실행 스크립트가 코드 요소를 선택해 처리하며, Mermaid는 `code.language-mermaid`를 대상으로 한다. 실패는 console error로 기록한다. 모든 OS·오프라인 환경에서 실행을 확인한 것은 아니다.
- **sanitize (XSS 방어의 1차 관문)** — `ammonia::Builder` 최소 화이트리스트: `<script>`/이벤트 핸들러 속성(`onerror=` 등)/`javascript:` scheme href 전부 stripped. `classify_link` 도 별도로 `javascript:` 를 판정 불가(`None`)로 취급해 이중 방어. GFM 렌더에 필요한 태그(`table`/체크박스 `input[type=checkbox]`/`del`/footnote `sup`/`div`/이미지/링크)만 허용 — 코드는 `crates/tasty-plugin-markdown/src/render.rs::sanitize_html` 이 정본.
- **리로드·삭제 처리**: 명시적 `markdown.reload`와 파일 감시 요청은 같은 플러그인 워커에서 처리한다. 로컬 파일을 다시 읽고 HTML을 만들며 읽기에 실패하면 `markdown.state.failed`를 표시한다. 파일이 다시 생긴 것을 감지하면 다시 읽는다. 워커의 직렬 처리가 파일 시스템의 동시 변경까지 막는 것은 아니다.
- **입력 없는 자동 갱신**: 별도 스레드가 SDK의 `file_watch`를 사용한다([플러그인 개발 가이드](../../dev-guide/plugin-development.md)). 검사 사이의 대기는 `RELOAD_CHECK_INTERVAL_SECS`(1초)이며 조회·읽기 시간을 포함한 전체 갱신 상한은 아니다. 감시에서도 파일을 읽어 내용 해시를 비교하고, 바뀌면 `self_invoke`로 워커의 `markdown.reload`를 요청한다. 자기 namespace를 `host.call`로 부르면 호스트 구현으로 전달되므로 플러그인 내부 호출에는 사용하지 않는다.
- **콘텐츠 전달** — surface 생성 시 host 가 `surface.create{file}` 를 plugin 에 보낸다. plugin 이 파일을 직접 읽는다(`fs.read`).
- **Theme parity** — webview-kind surface 는 Theme 이 자동으로 push 되지 않으므로(egui-mesh 의 `set_context.theme` 와 달리), plugin 이 문서를 (재)생성할 때마다 host 의 read-only **`theme.query`** IPC 로 현재 색+`is_light`+UI zoom 을 직접 조회한다. 이후 색이 바뀌면 host 가 발행하는 **`theme.changed`** 이벤트(매니페스트 `event_subscribe`)를 구독해 열려 있는 모든 markdown 문서를 재생성한다.
- **JS 는 기본 허용** — host 의 webview 설정 기본값은 plugin 마다 다르다: html 플러그인은 JS 기본 차단(임의 원격 콘텐츠를 열 수 있어서), markdown 은 **기본 허용**(`resolve_webview_settings` 의 per-plugin override, `src/view/main/redraw.rs`) — 신뢰된 주소창/네비게이션 스크립트가 항상 실행돼야 하고, 실제 markdown 콘텐츠 자체는 이미 별도로 sanitize 되므로 플러그인이 추가하는 신뢰 스크립트와 저자가 작성한 본문을 구분한다. sanitize가 모든 스크립트 위험을 없앤다고 보장하지 않는다.
- **인라인 이미지**: 정리한 본문의 로컬 `<img src>`를 플러그인이 읽어 `data:` URI로 넣는다. 상대 경로, scheme 없는 절대 경로와 raw HTML 이미지에 같은 처리를 적용한다. WebView가 로컬 파일 경로를 직접 열지 않아도 되게 하기 위한 방식이다.
  `canonicalize`한 경로가 문서 디렉터리 안에 있고 확장자가 PNG/APNG/JPEG/GIF/WebP/AVIF/BMP/ICO/SVG 목록에 있어야 한다. 파일을 읽기 전 metadata 길이로 한 장 4 MiB·문서 총합 16 MiB 기준을 확인한다. 경로 확인·metadata 조회·읽기는 별도 작업이므로 그 사이 파일이나 심볼릭 링크가 바뀌는 경우까지 막는 보장은 아니다. 읽지 못한 이미지의 `src`는 제거해 실패 표시 대상으로 둔다.
  원격 `http(s)` 이미지는 그대로 두며 호스트의 `allow_remote_content` 설정을 따른다. 기본값은 꺼짐이고 Settings › Appearance › Markdown에서 바꾼다. 세 백엔드에 차단 구현이 있으며 실제 차단·허용 비교는 Linux/WebKitGTK에서 확인했다. 다른 OS 실행은 확인하지 않았다. 선택 이유는 [ADR-0030](../../adr/0030-bundled-plugin-data.md)에 있다.
- **파일 핸들러** — `detector "markdown"`(확장자 매핑) + `handler` action `open_surface{surface_kind:"markdown"}`. 마크다운 파일 열기 시 이 surface 로 뜬다.
- **파일 열기와 대용량 확인 팝업** — 매니페스트에 `file-open`과 `large-file-confirm`을 등록하며 egui-mesh로 그린다. 파일 열기 팝업은 경로 입력, 찾아보기, 열기/취소로 구성한다.

  `file-open`은 `scope = "surface"`다. host가 연결한 대상 surface가 보일 때 그 영역 가운데에 뜬다. 제자리 변환은 그 surface를, 새 탭 열기는 당시 포커스된 surface를 대상으로 삼는다. 다른 workspace나 탭으로 이동하면 숨고 돌아오면 복원된다. event trigger로 열 때는 창 범위를 사용한다([팝업 범위](../../design/systems/popup.md#plugin-popup-의-스코프)).

  | 열기 경로 | 동작 |
  |---|---|
  | `convert_input_popup = "file-open"` | host가 convert 팝업, `open_markdown`·`convert_to_markdown` 단축키, context menu에서 `open_popup_instance`로 연다. |
  | `com.tasty.markdown.file_open` | event trigger로 연다. |

  **찾아보기**는 `file_picker.trigger {filters,owner_popup_instance?,start_dir?,origin_surface_id?} → {request_id}`로 host의 파일 피커를 연다. `fs.read` 권한이 필요하며 선택 결과는 `file_picker.result` 이벤트로 비동기 도착한다.

  시작 폴더는 open context의 로컬 `observed_cwd`, mirror이면 `remote_cwd`를 `start_dir`에 담는다. `origin_surface_id`도 그대로 전달한다. `inherit_cwd` 설정의 영향을 받는 `cwd` 키와는 다르다([파일 피커의 시작 위치](../../features/native-file-picker/index.md)).

  `owner_popup_instance`에 file-open 자신의 인스턴스를 넣어 두 팝업을 부모·자식으로 연결한다. 피커가 떠 있는 동안 부모는 바깥 클릭으로 닫히지 않는다. Esc는 위쪽 피커부터 닫으며, 부모가 먼저 닫히면 피커도 함께 정리한다([ADR-0036](../../adr/0036-overlay-scope-and-lifetime.md)).

  **열기를 확정하면** open context의 `surface_id` 유무에 따라 처리한다.

  | 조건 | 호출 |
  |---|---|
  | `surface_id` 있음 | `markdown.navigate {surface_id,path}`로 해당 surface를 제자리 변환 |
  | `surface_id` 없음 | `file_handler.dispatch {path,depth:"deep",owner_popup_instance}`로 새 탭 열기 |

  확정 요청의 `owner_popup_instance`도 file-open 자신의 인스턴스다. host는 이를 통해 사용자가 조작한 팝업의 요청임을 확인하고 새 탭을 선택한다. 빠지면 에이전트 요청으로 처리해 새 탭을 선택하지 않는다([ADR-0031](../../adr/0031-file-handler-routing.md)).

- **링크 클릭 라우팅** — 문서 안의 모든 non-anchor 링크 destination 은 HTML 생성 시점에 내부 nav-fragment 스킴(`#tasty-nav:link:<percent-encoded-dest>`)으로 rewrite 된다(`render::rewrite_link_dest`) — 실제 `href` 를 그대로 두면 native WebView 가 진짜 파일/미지 스킴으로 navigate 해버려(host 는 *원격* http(s) 만 차단) 렌더된 문서가 그 자리에서 깨진다.
  fragment 만 바뀌는 same-document navigation 은 (a) WebKitGTK 의 `decide-policy` 로는 여전히 캡처되지만(→ host 가 `webview.navigation_attempt` 이벤트로 forward) (b) 실제 페이지 리로드는 일으키지 않는다(실측 검증됨) — 이 성질로 "클릭을 가로채되 화면은 안 깨지는" 신호 채널을 만든다.
  plugin 의 `on_webview_navigation_attempt` 핸들러가 그 이벤트를 받아 `render::parse_nav_fragment`+`classify_link` 로 판정한다:
  - **상대 경로**(`docs/index.md`, `../sibling.md`)는 **현재 마크다운 파일의 폴더(base_dir) 기준**으로 절대화(프로세스 cwd 아님). 절대 경로는 그대로. `javascript:` 는 무조건 무시(sanitize 와 별개의 2차 방어).
  - **외부 URL**(`http(s)://`·`mailto:`·`data:`)은 host `webview.open_external {surface_id,url}` 로 보내고, host 가 OS 기본 핸들러로 연다. **이 plugin 은 OS 열기를 직접 하지 않는다**(OS 열기 크레이트를 링크하지 않는다) — host 한 자리로 모아야 debug 스위치(`TASTY_DEBUG_OS_OPEN_LOG`, [ADR-0045](../../adr/0045-test-isolation-and-harness.md))가 이 열기도 기록한다. host 는 호출 plugin 이 `surface_id` 의 소유자이고 URL 에 스킴이 있으며 `javascript:` 가 아닐 때만 연다. 클릭하면 확인 없이 기본 브라우저가 열리는 사용자 동작은 그대로다([ADR-0030](../../adr/0030-bundled-plugin-data.md)).
  - 파일 링크는 host `file_handler.dispatch` 로 보낸다(Explorer "파일 열기" 와 동일한 `DispatchFile` 경로 — 그 surface 가 속한 **Pane 의 새 탭**). 통지받은 navigation 의 URL 을 `user_navigation_url` 로 그대로 되대고(`file_link_params`), host 가 직접 본 두 사실(엔진의 사용자 제스처 보고 · 그 문서 페이지를 이 plugin 이 썼다)로 그 호출을 사용자 행동으로 판정하면 새 탭이 선택된다 — Linux 는 실측, Windows 는 컴파일만 재어졌다(재는 법: Windows 에서 문서 안의 파일 링크를 누르고 새 탭이 선택되는지 `tab.list` 로 본다). macOS 는 엔진이 그 값을 주지 않아 새 탭이 뒤에 붙기만 한다. plugin 은 판정하지 않는다([ADR-0031](../../adr/0031-file-handler-routing.md)).
- **주소창** — 문서 안의 HTML 입력 요소로 만든다. `render::addr_bar_html` 이 문서 자체에 `<input list="tasty-addr-recent">`+native `<datalist>`(최근 경로, 브라우저 내장 autocomplete — 커스텀 드롭다운 JS 불필요)+Go `<button>`을 생성한다. 문서 생성 시점의 *현재* 경로/최근목록으로 만들어지며, webview 에는 JS↔plugin 실시간 메시지 브리지가 없어(module doc 참고) 옛 PathField 처럼 포커스 시 다시 fetch 하는 반응형 동작은 없다 — Go 클릭/Enter 는 신뢰된 인라인 `<script>`(`render::nav_script`)가 `location.hash = 'tasty-nav:addr:' + encodeURIComponent(v)` 로 위 nav-fragment 채널에 태워 보낸다.
- **attach mirror 문서** — 원격 tasty 의 워크스페이스를 attach 하면 그 안의 markdown surface 는 이 plugin 이 **원격에서 가져온 원문**으로 다시 그린다([ADR-0022](../../adr/0022-remote-mirror-content-and-queries.md)).
  host 가 surface 를 만들 때 params 의 `remote.file` 에 원격 경로를 싣고(최상위 `file` 이 아니다), plugin 은 그것을 보면 `MdDoc::new_remote` 로 문서를 연다 — **파일을 읽지 않고, 감시에 등록하지 않고, `base_dir` 를 두지 않고, snapshot 을 내지 않는다.** 원문은 `markdown_mirror.content_request {surface_id}` 로 요청해(바깥 호출자의 `markdown.reload` 가 건 요청만 `agent_origin: true` 를 더 싣는다 — host 가 그 회신의 잘림 toast 를 사용자에게 띄우지 않는다, [ADR-0036](../../adr/0036-overlay-scope-and-lifetime.md)) `request_id` 를 받아 두고, host 가 unicast 하는 `markdown_mirror.content_result` 이벤트 중 **그 id 의 것만** 반영한다(새로고침을 연달아 누르면 옛 회신은 버려진다).
  회신의 `reason` 은 `load_error` 로 가 실패 상태로 그려지고, 원문을 한 번도 못 받은 동안은 로딩 상태(`markdown.remote.loading`)가 그려진다.
  `request_id: 0`은 연결 끊김을 알리는 신호다.
  대기 중인 요청을 끝내고, 이미 원문을 받은 문서도 끊김 화면으로 바꾼다.
  재연결 뒤 변경 신호를 받으면 원문을 다시 요청한다.
  - **주소창·링크**: 주소창은 읽기 전용이고 Go 와 최근목록 대신 우측 끝에 **새로고침 버튼**(`#tasty-refresh`)이 붙는다. 버튼은 `#tasty-nav:refresh:<nonce>` 로 nav-fragment 채널을 타고, plugin 은 그것을 원문 재요청으로 옮긴다(nonce 는 같은 hash 재대입이 navigation 을 안 일으켜 두 번째 클릭이 사라지는 것을 막는다). `markdown.reload` 도 mirror 문서에는 같은 재요청이다. 파일 링크와 주소창 경로는 원격 호스트의 것이라 무시하고, 외부 URL 만 연다.
  - **변경 신호**: `markdown_mirror.changed {surface_id}` 이벤트를 받으면 원문을 다시 받지 않고 버튼의 `data-stale` 만 `true` 로 바꿔 색을 accent 로 바꾼다. 최신 원문을 받으면 풀린다. 이미 stale 이면 문서를 다시 싣지 않는다.
  - **테마 변경**은 받아 둔 원문으로 다시 그릴 뿐 요청하지 않는다. mirror 문서는 host 의 파일 핸들러를 거치지 않으므로 최근목록에도 기록되지 않는다.
- **최근목록 조회** — host 의 generic **`recent.query {kind}`** IPC 가 그 kind 의 최근 연 파일을 **최신순 최대 10개** 반환한다(`{recent:[{path,file_name}]}`). markdown plugin 은 `kind:"markdown"` 을 채워 호출해 위 주소창 `<datalist>` 를 채운다. **읽기 전용** — host의 모든 창이 공유하는 `AppState.recent_files` 캐시를 필터 없이 조회할 뿐 사용자 상태를 바꾸지 않는다(불가침 원칙). **`surface.read`** 권한. recent 기록 대상 여부는 매니페스트 `records_recent` capability 로 판정.
- **스크롤 위치 보존(best-effort)** — idle-watch 자동 리로드와 `markdown.reload` 는 전체 문서를 `load_html` 로 통째로 교체한다(부분 DOM patch 없음) — 이대로면 native WebView 의 스크롤이 매 리로드마다 0 으로 리셋된다. `render::nav_script` 가 `sessionStorage`(파일 경로로 키잉, `scroll` 이벤트 150ms 디바운스 저장 + 로드시 복원)로 이를 완화한다. same-origin 스코프 의존이라 `load_html` 반복 호출이 origin identity 를 유지하는지는 webview 엔진(WebKitGTK/WKWebView/WebView2)마다 다를 수 있어 3 종 전체 검증되지 않았다 — 실패하면 스크롤 위치가 복원되지 않으며 별도 오류는 표시하지 않는다.
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
