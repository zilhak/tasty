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
  - `url` 인자는 URL(`http://`·`https://`·`file://`) 또는 로컬 파일 경로다. CLI 매니페스트가 `path_kind = "url_or_file"`이라 상대 경로는 CLI를 호출한 디렉터리 기준 절대 경로로 바뀌고, 파일이 없으면 요청을 보내지 않고 실패한다.
  - `html.open`은 생성 경로와 같은 `local_path_to_file_uri`로 절대 경로를 `file://` URI로 바꿔 `webview.set_url`에 넘긴다. IPC로 직접 온 상대 경로는 기준 디렉터리가 없어 `invalid_params`로 거절한다.
  - `html.open`으로 바꾼 URL은 스냅샷에 남지 않는다. 스냅샷은 생성·복원 때의 URL이며 호스트가 `surface.snapshot`을 다시 묻지 않으므로, 재시작하거나 플러그인 프로세스가 바뀌면 생성 URL로 돌아간다.
- **스크립트 감지와 문서 단위 허용** — 전역 설정 sandbox scripts가 켜져 있으면 host가 main frame 문서마다 JS를 끈다. `file://` 문서는 응답 단계에서 원본 파일을 읽어 스크립트를 감지하고 파일 전체 지문을 구한다. 허용은 URL(fragment 제외)과 지문에 묶이며 다른 문서가 commit되면 풀린다. 규칙은 아래 [스크립트 감지와 허용 규칙](#스크립트-감지와-허용-규칙) 절, 결정은 [ADR-0053](../../adr/0053-html-script-detection-and-per-document-allowance.md).
  - 스캔 상한: 감지는 파일 앞 4 MiB(`SCAN_LIMIT_BYTES`)까지만 읽는다. 지문은 상한과 관계없이 파일 전체를 해시한다.
  - 정규 파일만 스캔한다. FIFO·장치 같은 파일은 읽지 않고 지문 없음으로 두므로 그 문서는 허용할 수 없다.
  - 파일 열기와 정규 파일 확인은 host(`src/host_api/webview/script_gate.rs`)가 한다. `tasty_model::html_script::scan`은 받은 바이트로 감지와 지문만 계산하며 파일 시스템에 접근하지 않는다.
  - Linux html surface는 WebKit page cache를 끈다. 뒤로·앞으로 가기도 캐시 복원이 아니라 파일을 다시 읽는 새 로드가 되어 같은 게이트를 거친다.
  - 배너 발화 판정은 `tasty_model::html_script::banner`에 있다. 사용자가 그 문서를 봤을 때만 배너 단계가 `blocked`가 되고, 에이전트가 연 문서·세션 복원·보이지 않는 탭의 문서는 `pending_view`만 기록한다. 판정은 포커스와 활성 탭을 바꾸지 않는다.
  - 배너 그리기: `src/adapters/ui/surface/html_script_banner.rs`가 webview chrome 위에 inset 배너를 그린다([배너 시스템 §inset 배치](../../design/systems/banner.md#inset-배치)). 그린 카드 아래 `banner_inset_gap`까지의 높이를 MainViewState에 남기면 같은 프레임의 WebView 동기화가 그만큼 WebView를 내리고 줄인다. surface 폭이 `banner_narrow_below` 미만이면 narrow 배치(액션이 다음 줄)로 그린다. surface가 새 문서를 로드하는 동안(로드 시작부터 main frame commit 전까지) 단계는 `loading`이고 허용 버튼은 비활성이며 hover하면 위쪽 툴팁으로 이유를 보인다. 이 동안 모델도 허용을 받지 않는다(`AllowError::Loading`). commit 뒤 새 문서에 스크립트가 있으면 `blocked`로 돌아오고 없으면 배너가 사라진다. 허용을 누르면 재로드 중 배너가 액션 자리에 스피너를 보이고, 새 문서가 commit되어 단계가 `hidden`이 되면 `banner_fade` 동안 흐려지며 사라진다. 닫기(×)와 문서 교체는 즉시 사라진다. 배너는 키보드 포커스를 가져가지 않는다.
  - 로드 실패: backend가 main frame 로드 실패를 알리면(WebKitGTK `load-failed`·web process 종료, WKWebView `didFail`·`didFailProvisional`·web content process 종료, WebView2 `NavigationCompleted` 실패·렌더러/브라우저 프로세스 종료) `HtmlScriptState::on_load_failed`가 실패 표지를 세운다. 표지가 있는 동안 배너 단계는 `hidden`이고 탭 표지는 없다. 재로드 중 배너도 페이드 없이 바로 내린다. 둘 다 이전 문서를 가리키고 허용할 문서가 없기 때문이다. 다음 main frame commit이 표지를 지우고 배너·표지를 새 문서로 다시 정한다. commit 전의 재로드 시작만으로는 지우지 않는다.
    - chrome의 Failed와 이 실패 표지는 같은 로드의 실패에만 선다. Windows·macOS는 새 로드가 앞 로드를 취소해 앞 로드의 실패가 늦게 오면 둘 다 건드리지 않는다. 앞 로드의 늦은 성공도 새 로드의 Loading을 Done으로 바꾸지 않는다(로드 세대 판정, [OS별 적용](#os별-적용)). web process 종료는 세대와 관계없이 둘 다 세운다. 화면 문서를 잃었기 때문이다. Linux는 취소된 앞 로드의 `load-failed`가 새 로드의 시작보다 먼저 와서 세대 판정 없이 같은 결과가 된다(측정).
  - 탭 표지: `HtmlScriptState::marker`가 표지를 정한다. 차단된 배너를 닫았으면 lock(`Blocked`), 현재 문서를 허용했으면 scriptFile(`Allowed`)이고 sandbox가 꺼져 있으면 없다. 탭 바는 이동 글리프 왼쪽에 `html_script_marker_hit` 칸과 `tab_status_gap`을 잡아 표지를 그린다. 탭에 html surface가 여럿이면 `Blocked`를 먼저 보인다. lock 클릭은 배너만 다시 보이며 탭 전환·포커스 변경은 하지 않는다. scriptFile은 툴팁만 있다. 표지 툴팁은 [탭 스트립 툴팁 규칙](../../design/systems/banner.md#탭-스트립-툴팁)을 따라 탭 위쪽에 뜨므로 html 탭이 활성이어도 WebView에 가려지지 않는다.
  - 조회: `tasty surface html-script --surface <id>`(IPC `surface.html_script`)가 현재 문서의 URL·감지 결과(`none`/`scripts`/`scripts_remote_only`)·지문, 허용 기록, 현재 JS 적용 값, 로드 진행 여부(`loading`, 로드 시작 뒤 commit 전이면 `true`), 배너 단계(`hidden`/`blocked`/`loading`/`reloading`)와 표지(`viewed`·`pending_view`·`shown`·`dismissed`)를 돌려준다. 읽기만 하므로 배너를 띄우거나 사용자가 본 것으로 기록하지 않는다. html이 아닌 surface는 거절하고 헤드리스 빌드는 빌드 미지원 오류(`-32017`)로 답한다.
  - 허용: release에서는 사용자만 GUI로 허용한다. 에이전트용 허용 재현은 debug 전용 `debug.html_script.allow`다([debug IPC](../../dev-guide/debug-ipc.md)).

## 스크립트 감지와 허용 규칙

결정과 기각한 대안은 [ADR-0053](../../adr/0053-html-script-detection-and-per-document-allowance.md)에 있다. 이 절은 현재 규칙과 OS별 구현·측정이다.

### 감지 대상

로컬 문서에서 다음 셋 중 하나라도 있으면 실행 가능한 스크립트가 있다고 판단한다.

- `<script>` 요소. inline이든 `src`든 같다. `<svg>` 안의 inline `<script>`도 포함한다. 단 `type`이 JavaScript MIME 형식이나 `module`이 아니면 제외한다. `application/json`, `text/template` 같은 데이터 블록이 이 예외에 해당한다.
- 요소의 `on*` 이벤트 핸들러 속성.
- `javascript:` 스킴 URL. `href`, `src`, `action`, `formaction` 속성의 값을 검사한다.

원격 콘텐츠가 차단된 상태에서 `http(s)` `src` 스크립트만 있으면 허용해도 실행되지 않는다. 이때는 배너 본문을 원격 안내 문구(`bodyRemote`)로 바꾼다.

### 스캔

스캔하는 크기에는 상한을 둔다. 상한 값은 구현 문서에서 정한다. 상한은 감지 스캔에만 적용하며, 아래 내용 지문은 상한과 관계없이 파일 전체를 덮는다.
상한을 넘은 부분에만 스크립트가 있으면 감지하지 못한다. 이 경우 JS가 꺼진 채 배너가 뜨지 않는다. 즉 실패해도 JS가 켜지지는 않는다.
정규 파일만 스캔한다. FIFO·장치 파일은 거절하므로 지문이 없고, JS가 꺼진 채 허용할 수 없다.
스캔은 응답 단계의 UI 스레드에서 한다. FIFO는 여는 순간 쓰는 쪽을 기다리며 막혔고 `/dev/zero`는 읽기가 끝나지 않았다(측정). 그래서 막히지 않게 연 핸들로 종류를 먼저 확인한다.

### 허용의 수명

허용은 surface, main frame 문서의 URL, 내용 지문에 묶인다. URL은 fragment를 뺀 값이다. 내용 지문은 파일 전체를 스트리밍 해시로 구한다.
지문을 스캔 상한까지만 구하면 상한 뒤에 덧붙인 스크립트가 허용된 채 실행된다(리뷰 측정). 그래서 지문 범위를 스캔 범위와 분리한다.

지문은 로드할 때 구한다.

- html surface에서 main frame `file://` 문서를 로드할 때마다 감지 스캔과 같은 읽기로 파일 전체 지문을 구한다. 허용 여부와 관계없이 모든 로드에서 구한다.
- 구한 지문과 감지 결과는 그 문서가 commit될 때 surface의 "현재 문서" 기록이 된다. 기록 항목은 URL, 지문, 감지 결과다.
- 허용 클릭은 그 시점의 현재 문서 `{URL, 지문}`을 허용으로 기록한다. 클릭할 때 파일을 다시 읽지 않는다.
- 이유: 허용 대상은 사용자가 보고 배너 감지 결과가 나온 그 내용이어야 한다. 클릭할 때 지문을 구하면, 사용자가 본 뒤 파일을 바꿔 둔 내용이 경합 없이 실행된다(리뷰 측정).
- 로드가 시작된 뒤 main frame commit 전에는 허용을 받지 않는다(`AllowError::Loading`). 배너는 이 동안 허용 버튼을 비활성으로 그린다.
  - 이유: 이 사이에 누른 허용은 이전 문서를 기록한다. 재로드 도중 파일 내용이 바뀌었다면 commit 때 지문이 달라 허용이 풀리고, 사용자는 "허용했는데 다시 막혔다"를 겪는다. commit 뒤 재평가로 허용을 옮기는 안은 사용자가 보지 않은 내용을 허용하게 되어 택하지 않았다.
- 화면 문서는 마지막 main frame commit에서 호스트가 기록한 현재 문서다. webview의 현재 URI 조회로 판정하지 않는다.

허용의 수명은 다음 규칙을 따른다. 사용자에게 보이는 문안("until Tasty restarts", 갤러리의 "cleared by: navigation to another document · app restart")과 같은 규칙이다.

- **같은 문서의 재로드**: 허용을 유지한다. 사용자의 재로드(F5 등)와 허용 직후 호스트가 하는 재로드를 구분하지 않는다.
- **다른 문서로 이동**: 허용을 푼다. 그 뒤 같은 URL로 돌아와도 다시 허용해야 한다. 뒤로 가기와 bfcache 복원도 여기에 포함된다.
- **내용이 바뀐 뒤의 재로드**: 다른 문서로 본다. 다시 스캔한 결과나 내용 지문이 허용 당시와 다르면 허용을 푼다.
- **허용 기록의 갱신 시점**: 허용 기록은 다른 main frame 문서가 commit될 때 해제한다.
  - 새 문서의 JS를 켤지 끌지는 commit 전의 응답 단계에서 정한다. 이 판단은 허용 기록을 바꾸지 않는다.
  - 응답 단계에서 허용과 일치한 문서가 commit되면 허용을 유지한다. 그 밖의 문서가 commit되면 허용을 해제한다. 응답 단계 없이 commit되는 bfcache 복원도 해제에 포함된다.
  - 응답 단계의 결과는 로드마다 새로 둔다. 로드가 시작될 때 이전 로드의 결과를 비우고, commit에서 소비한다. 비우지 않으면 앞선 재로드의 "일치"가 뒤이은 bfcache commit에 남아 허용이 유지된다(page cache를 켠 상태의 리뷰 측정).
  - 응답 단계 없이 commit된 문서는 지문이 없다. 이 문서는 허용할 수 없다. Linux는 아래처럼 page cache를 꺼서 이 경우를 없앤다.
- **문서가 바뀌지 않고 끝나는 로드**: 로드 실패, 중단, 다운로드, web process 종료처럼 main frame 문서가 commit되지 않고 로드가 끝나는 경우다.
  - 화면에 남은 문서가 허용된 문서이면 JS를 허용 상태로 되돌린다. 멈춘 채 두지 않는다.
  - 허용 기록은 그대로 둔다.
- **세션 한정**: 허용은 세션 안에서만 유지한다. 레이아웃에 저장하지 않으므로 재시작하면 다시 차단 상태가 된다.

허용 판단은 **main frame의 문서가 바뀔 때에만** 적용한다. 다음 경우에는 허용을 바꾸지 않는다.

- 같은 문서 안의 서브프레임(`<iframe>`, `<object>`, `<embed>`) navigation.
- 새 창 요청(`target=_blank` 등). 현재 문서를 바꾸지 않는다.
- fragment만 바뀌는 이동.

문서가 바뀌면 새 문서의 스크립트가 실행되기 전에 JS를 끈다. redraw에서 사후에 되돌리지 않는다.

- 설정 경로(`resolve_webview_settings`)는 전역 "Sandbox scripts" 값만 backend에 넘긴다. 문서 단위 JS는 backend의 navigation 콜백이 surface의 허용 상태로 정해 그 자리에서 적용한다.
- 이유: redraw에서 문서 단위 값을 사후에 적용하면 그 사이 시작된 다른 문서의 로드에 이전 문서의 허용이 샌다. 콜백은 로드 단계와 같은 순서로 불리므로 이 경합이 없다.
- 전역 값이 바뀌면 backend는 그 값을 허용 상태에 반영하고, 진행 중인 로드가 있으면 그 로드의 응답 단계 판단을, 없으면 화면 문서의 허용 여부를 적용한다.

### OS별 적용

OS별 구현은 다음과 같다.

- **Linux(측정함, WebKitGTK 2.50.4)**: `NavigationAction`에는 main frame과 서브프레임을 구분할 정보가 없다. iframe 문서의 navigation도 `get_frame_name()`이 None이고 type이 `other`로 같았다. 그래서 navigation action에서는 판단하지 않고, 다음 두 신호를 쓴다.
  - `load-changed` `STARTED`: main frame 로드에서만 온다(측정 결과 iframe 로드에서는 오지 않았다). 여기서 JS를 무조건 끈다.
    - 끈 설정은 화면에 남아 있는 현재 문서에도 적용된다. 허용된 문서에서 응답이 오지 않는 로드(FIFO)를 시작하자 1초 간격 timer가 5.3초 동안 멈췄고, 중단 뒤 복원하자 다시 돌았다(측정). 끈 직후 같은 이벤트 처리 턴에서 이미 예약된 timer가 한 번 실행된 경우는 있었다(측정 1회).
    - 이 시점의 `get_uri()`는 링크 이동에서 이전 문서 URI를 돌려준다(측정). 그래서 URL을 판정에 쓰지 않는다.
    - 응답 단계의 결과(대기 값)를 여기서 비운다.
  - `decide-policy`의 `RESPONSE` 결정: `ResponsePolicyDecision::is_main_frame_main_resource()`(2.40부터)가 참일 때만 동작한다.
    - `file://`이면 원본을 읽어 감지 스캔과 전체 지문을 구한다. 응답 URL과 지문을 허용과 비교해 JS를 켜거나 끈다. 결과(URL, 지문, 감지 결과, 일치 여부)는 대기 값으로 둔다.
    - 문서상으로는 결정 객체를 잡아 두고 나중에 결정할 수 있다. 따라서 읽기와 해시를 main thread 밖에서 하고 끝난 뒤 결정을 넘기는 분리가 가능해 보인다. 구현에서의 가능 여부는 검토하지 않았다.
    - 측정에서 iframe 응답은 거짓이었다.
    - 여기서 켜고 끈 설정은 그 문서의 첫 스크립트부터 적용됐다.
    - 이 단계에서는 JS만 정한다. 허용 기록은 `load-changed` `COMMITTED`에서 갱신한다.
    - page cache를 끈 상태에서는 뒤로·앞으로 가기를 포함한 모든 main frame 로드의 순서가 `STARTED` → `RESPONSE` → `COMMITTED`였다(측정). 리뷰 측정의 일반 로드와 재로드도 같은 순서였다.
  - `NEW_WINDOW_ACTION`은 별도 결정 type으로 오며 허용에 영향을 주지 않는다.
  - fragment 이동에는 `load-changed`가 오지 않는다. JS 상태가 유지됐다.
  - page cache가 켜져 있으면 bfcache 복원에 `RESPONSE` 결정이 오지 않는다. `STARTED`에서 끈 상태가 유지되어, 복원된 문서의 timer가 멈춘 채로 남았다.
    - page cache가 켜진 상태에서 `STARTED`에서 끄지 않으면, 허용된 문서에서 뒤로 가기로 복원한 허용되지 않은 문서의 timer가 다시 실행됐다(측정).
    - 이렇게 응답 단계 없이 commit된 문서는 지문이 없다. 그래서 아래처럼 page cache를 끈다.
  - html surface는 page cache(`enable-page-cache`)를 끈다.
    - 끄면 뒤로·앞으로 가기도 `STARTED` → main `RESPONSE` → `COMMITTED` 순서의 새 로드가 됐다(측정). 모든 main frame commit이 지문을 가진다.
    - `STARTED`에서 끄는 규칙은 그대로 둔다.
  - `COMMITTED`에서 대기 값을 소비해 현재 문서를 기록한다.
  - Tasty는 `load-failed` 핸들러가 `true`를 돌려 WebKit 기본 오류 페이지를 띄우지 않는다(`src/host_api/webview/linux.rs`의 `connect_load_failed`).
    - 이 설정에서 main frame 로드가 실패하면 문서가 바뀌지 않는다. 대상은 없는 파일, 그리고 "Frame load interrupted"로 끝나는 다운로드 응답이다.
    - 두 경우 모두 `STARTED`(끔) 뒤 `COMMITTED` 없이 `load-failed`와 `FINISHED`가 왔다. 화면에는 이전 문서가 남았다. 복원하지 않으면 화면의 허용 문서가 JS 꺼진 채 멈췄다(리뷰 측정).
  - 복원은 `load-changed` `FINISHED`에서 한다. 직전 `STARTED` 뒤 `COMMITTED`가 없었으면, 화면 문서의 허용 여부대로 JS를 되돌린다.
    - 화면 문서는 마지막 `COMMITTED`에서 호스트가 기록한 현재 문서다. `FINISHED` 시점이나 `stop_loading` 뒤의 `get_uri()`는 쓰지 않는다.
      - 취소된 로드의 `FINISHED` 시점 `get_uri()`는 아직 시작하지 않은 다음 URL을 돌려줬다(리뷰 측정).
      - `load_uri` 직후 `stop_loading`하면 이벤트가 없는데도 `get_uri()`가 새 URL로 남았다. 화면은 이전 문서였다(리뷰 측정).
    - `load-failed`는 기록만 한다. 측정한 두 경우 모두 `load-failed` 뒤에 `FINISHED`가 왔다. `FINISHED`에서 처리하면 commit 없이 끝나는 다른 경로도 함께 다룬다. 예외는 web process 종료다.
    - commit 전에 web process가 종료되면 `web-process-terminated`만 오고 `load-failed`와 `FINISHED`는 오지 않았다. 느린 http 응답을 기다리는 로드에서 web process를 강제 종료해 측정했고, 3분 뒤에도 신호가 없었다. 복원하지 않으면 로드 중 상태가 남아 배너가 허용 버튼을 비활성으로 두고 허용 요청이 `AllowError::Loading`으로 거절됐다(측정).
      - 그래서 `web-process-terminated`에서도 `FINISHED`와 같은 방식으로 로드를 끝내고 JS를 되돌린다. 로드가 이미 끝났으면 아무것도 바꾸지 않으므로, 뒤에 `FINISHED`가 와도 결과가 같다.
      - Windows·macOS 백엔드도 같은 방식으로 로드를 끝낸다. 구현은 했지만 실 기기에서 측정하지 않았다. 같은 증상이 나는지와 종료 뒤 어떤 navigation 신호가 오는지는 모른다.
        - macOS는 `webViewWebContentProcessDidTerminate:`에서 로드를 끝낸다.
        - Windows는 `ProcessFailed` 중 main frame 문서를 그리던 process가 끝난 종류(`RENDER_PROCESS_EXITED`·`BROWSER_PROCESS_EXITED`)에서만 끝낸다. iframe·GPU·utility process 종료와 `RENDER_PROCESS_UNRESPONSIVE`는 main frame 로드를 끝내지 않으므로 건너뛴다.
    - 측정 결과 허용 문서의 timer가 다시 돌았다(tick 14→19, JS True).
  - `stop_loading`이나 기존 정책이 무시하는 `http(s)` navigation에는 `STARTED`가 오지 않았다. 따라서 JS와 문서가 그대로다(리뷰 측정).
- **Windows(미측정, API 문서 근거)**: `NavigationStarting`은 main frame navigation에서만 발생한다. 서브프레임은 `FrameNavigationStarting`, 새 창은 `NewWindowRequested`로 따로 온다. 이 핸들러 안에서 `IsScriptEnabled`를 정한다. API 문서의 예제도 이 핸들러에서 해당 navigation에 적용되도록 설정을 바꾼다. `NavigationStarting` 이후에 바꾸면 다음 top-level navigation부터 적용된다.
- **macOS(문서 단위 적용과 서브프레임 측정)**: `webView:decidePolicyForNavigationAction:preferences:decisionHandler:`에서 navigation별 `WKWebpagePreferences.allowsContentJavaScript`를 정한다. `targetFrame.isMainFrame`이 참일 때만 로드를 시작하고 판단한다. 서브프레임 navigation에는 그 시점 main frame 문서에 대한 게이트의 판단(`ScriptGate::effective_js`)을 넣는다. Linux에서 서브프레임이 webview 전체 JS 설정을 따르는 것과 같은 결과다. 새 창 요청은 `targetFrame`이 nil이며 preferences를 바꾸지 않는다.
  - macOS 27.0.1 실 기기에서 debug 빌드로 실제 실행해 측정했다. 스크립트와 스크립트가 있는 iframe을 가진 `file://` 문서는 main frame과 서브프레임 모두 JS가 꺼진 채 그려졌고, 사용자가 본 뒤 배너가 `blocked`로 떴다. 허용하면 재로드 뒤 두 frame 모두 JS가 돌았다. 다른 문서로 이동하면 그 문서의 JS가 꺼졌고, 허용했던 문서로 돌아와도 허용이 풀려 다시 꺼졌다.
  - 허용 판단이 붙은 webview에서는 전역 `javaScriptEnabled`(deprecated)를 켜 둔다. 전역 값이 꺼져 있으면 navigation별 `allowsContentJavaScript`를 켜도 스크립트가 실행되지 않기 때문이다. 문서의 JS는 navigation별 값으로만 정한다.
  - 그래서 전역 "Sandbox scripts" 변경은 화면 문서에 바로 적용되지 않고 다음 navigation부터 적용된다(미측정).
- Windows와 macOS의 bfcache 복원에 navigation별 설정이 적용되는지는 측정하지 않았다.
- 두 OS에서 bfcache를 끌 수 있는지는 구현할 때 확인한다. 응답 단계 없이 commit된 문서는 지문이 없어 허용할 수 없다.
- 두 OS에서도 허용 기록은 새 main frame 문서가 commit될 때 갱신한다. commit 없이 끝난 로드에서는 화면 문서의 허용 상태로 JS를 되돌린다. 이 시점에 대응하는 이벤트는 구현에서 정하며 측정하지 않았다.
  - 두 OS는 로드 시작과 응답 결정을 navigation 시작 콜백에서 함께 한다. 새 로드가 앞 로드를 취소하면 앞 로드의 종료 신호가 새 로드의 시작 뒤에 올 수 있다. 이 신호가 새 로드의 대기 값을 지우면 새 문서가 지문 없이 기록된다.
  - Windows는 `NavigationStarting`의 `NavigationId`를 로드 세대로 기록하고, `NavigationCompleted`는 자기 `NavigationId`가 현재 세대일 때만 복원한다. 앞 로드의 늦은 종료는 무시한다(순서 자체는 미측정).
  - macOS는 `WKNavigation`을 로드 세대로 쓴다. 정책 결정(`decidePolicyForNavigationAction`)에는 `WKNavigation`이 없으므로, 게이트가 로드를 시작한 정책 결정 뒤 첫 `didStartProvisionalNavigation`의 `WKNavigation`을 세대로 기록한다. `didFinishNavigation`·`didFailNavigation`·`didFailProvisionalNavigation`은 자기 `WKNavigation`이 현재 세대일 때만 복원한다. 판정은 Windows와 같은 함수(`load_generation::is_current_load`)이며, 어느 쪽이든 `WKNavigation`을 알 수 없으면 현재 로드로 본다.
    - 정책 결정부터 provisional 시작까지는 세대를 비워 둔다. 이 사이에 온 종료는 모두 현재 로드로 본다. 앞 로드의 종료가 이 구간에 오면 새 로드의 대기 값을 지울 수 있다. 이 순서가 실제로 생기는지는 측정하지 않았다.
    - 게이트 로드가 끝나면(`gate_finished`) provisional 시작 대기 표시도 내린다. provisional 시작 없이 끝난 로드 뒤에 오는 다른 provisional 시작을 세대로 기록하지 않기 위해서다. 그래서 위 구간에 앞 로드의 종료가 오면 새 로드는 세대 없이 진행하고, 그 로드의 종료는 모두 현재 로드로 본다.
    - 기록한 `WKNavigation`은 다음 로드가 시작할 때까지 붙잡아 둔다. 같은 주소가 다른 navigation에 다시 쓰여 앞 로드의 종료가 현재 세대로 보이는 일을 막는다.
    - 실 기기 측정 없이 구현했다. 이 머신에는 macOS용 C 컴파일러가 없어 macOS 코드를 컴파일하지 못했다.
  - chrome의 `NavState` 종료 전이(Done·Failed)도 같은 판정 함수(`load_generation::nav_state_after_end`)를 따른다. 앞 로드의 늦은 실패가 chrome만 Failed로 두거나, 늦은 성공이 새 로드의 Loading을 Done으로 덮어 새 로드가 끝나기 전에 overlay를 보이는 일을 막는다. Windows는 `NavigationCompleted`의 성공·실패, macOS는 `didFinishNavigation`·`didFailNavigation`·`didFailProvisionalNavigation`이 대상이다. 실 기기 측정은 하지 않았다.
    - chrome 세대는 게이트 세대와 따로 둔다. 마지막에 시작한 main frame navigation(Windows `NavigationStarting`의 `NavigationId`, macOS `didStartProvisionalNavigation`의 `WKNavigation`)을 모두 기록한다. 게이트 세대는 fragment 이동과 게이트가 없는 surface의 탐색을 기록하지 않으므로, 이를 chrome에 쓰면 그 탐색의 종료가 앞 로드로 보여 Loading이 남는다.
    - `load_url`·`load_html`은 navigation 시작 신호 전에 Loading을 두고 chrome 세대를 비운다(모름 = 현재). 게이트가 정책 결정에서 세대를 비우는 것과 같은 장치다. 요청한 로드가 시작 신호 없이 끝나도 그 종료를 현재 로드로 보아 Loading에 남지 않는다. 그런 경로가 실제로 있는지는 측정하지 않았다. 후보는 macOS의 provisional 시작 전 정책·스킴 단계 실패와 Windows `NavigationStarting`의 args가 없는 경우다.
      - 대가로 그 사이에 오는 앞 로드의 종료도 현재 로드로 보아 상태를 바꾼다. 뒤이은 시작 신호가 다시 Loading으로 둔다.
    - web process 종료(Windows `ProcessFailed`의 렌더러·브라우저 종료, macOS `webViewWebContentProcessDidTerminate:`)는 세대와 관계없이 Failed로 둔다. 종료 신호에는 navigation이 없고 화면 문서를 잃었기 때문이다.
    - Linux도 같은 함수를 따른다. WebKitGTK의 종료 신호에는 navigation 식별자가 없어 신호 순서로 세대를 매긴다(`load_generation::SignalOrderLoads`).
      - `load_url`·`load_html`이 새 세대를 현재 로드로 둔다. 요청 없이 온 main frame `STARTED`(링크·뒤로 가기)도 새 세대다.
      - `load-failed`와 `FINISHED`는 마지막 `STARTED`를 받은 로드의 것으로 본다. `FINISHED`가 그 로드를 비우고, web process 종료도 비운다.
      - `load_uri`로 앞 로드를 취소하면 앞 로드의 `load-failed`("Load request cancelled")와 `FINISHED`가 새 로드의 navigation 결정과 `STARTED`보다 먼저 온다. 이 두 신호는 앞 로드의 늦은 종료가 되어 chrome은 Loading에 머문다. 판정 전에는 새 `STARTED`가 올 때까지 Failed였다(측정: FIFO `file://` 로드를 걸어 둔 채 다른 파일로 바꾸기, 판정 전 3회·판정 후 4회, WebKitGTK 2.50.4). 없는 파일처럼 현재 로드가 실패하면 Failed가 되고, 뒤따르는 `FINISHED`가 Done으로 덮지 않는다(측정 1회).
      - 앞 로드가 끝나기 전에 요청한 로드가 `STARTED` 없이 실패하면 그 실패도 앞 로드의 것으로 보아 Loading에 남는다. 측정한 순서에서는 앞 로드의 종료가 먼저 와서 생기지 않았다.

### 사용자가 봤다는 판정

배너는 사용자가 그 문서를 볼 때만 띄운다. "사용자가 봤다"는 다음 신호로 판정한다.

"사용자가 봤다"는 다음 신호로 판정한다.

- 포커스된 surface가 바뀐 순간을 사용자 선택으로 본다. release의 에이전트 경로는 탭과 surface 선택을 바꾸지 않기 때문이다. 창을 처음 그리는 프레임의 포커스는 선택으로 세지 않는다. 그래서 세션 복원 문서는 기록만 한다.
- 문서가 없거나 로드 중일 때 선택되면 다음에 commit되는 문서를 본 것으로 한다. 사용자가 연 파일은 새 탭이 선택된 뒤 첫 문서가 오기 때문이다.
- 사용자가 보던 문서에서 링크나 스크립트로 바뀐 문서는 본 상태를 이어받는다. 호스트가 URL을 넣은 로드(plugin·IPC·복원)는 이어받지 않는다. 그래서 에이전트가 사용자가 보던 surface에 넣은 문서도 다음 선택 때까지 기록만 한다.
- 한계: 에이전트가 포커스된 탭을 닫아 포커스가 다른 surface로 옮겨 가는 경우도 선택으로 센다.

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
- Given 사용자가 보고 있는 surface의 배너가 `blocked` When 배너를 그린다 Then WebView는 배너 카드 아래 `banner_inset_gap` 뒤에서 시작하고, 배너가 사라지면 콘텐츠 위 끝으로 돌아간다.
- Given surface 폭이 `banner_narrow_below` 미만 When 배너를 그린다 Then 허용 버튼이 본문 아래 줄에 놓인다.
- Given 사용자가 배너의 ×를 눌렀다 When 탭 바를 그린다 Then 그 탭에 lock 표지가 보이고 조회의 `banner.dismissed`는 `true`다.
- Given lock 표지가 있는 탭이 비활성 When 사용자가 lock을 누른다 Then 조회의 `banner.phase`는 `blocked`이고 활성 탭은 바뀌지 않는다.
- Given 사용자가 [이 문서에서 허용]을 눌렀다 When 재로드가 끝난다 Then 조회의 `allowed`·`javascript`는 `true`, `banner.phase`는 `hidden`이고 탭에 scriptFile 표지가 보인다.
- Given 사용자가 보고 있는 surface의 배너가 `blocked` When 그 surface가 새 문서를 로드하기 시작했고 아직 commit 전이다 Then 조회의 `loading`은 `true`, `banner.phase`는 `loading`이고 허용 버튼은 비활성이며 ×는 그대로 누를 수 있다.
- Given 배너 단계가 `loading`이고 그 로드는 사용자가 연 이동(링크·뒤로 가기·재로드처럼 호스트가 URL을 넣지 않은 이동)이다 When 새 문서가 commit되고 그 문서에 스크립트가 있다 Then 조회의 `loading`은 `false`, `banner.phase`는 `blocked`이고 허용 버튼이 다시 활성이다.
- Given 배너가 `blocked`이거나 탭에 lock·scriptFile 표지가 있다 When 다음 로드가 문서를 commit하지 못하고 실패한다 Then host chrome이 "Failed to load"를 보이는 동안 조회의 `banner.phase`는 `hidden`이고 탭 표지가 없다. 그 뒤 재로드가 commit되면 배너와 표지를 새 문서로 다시 정한다.
- Given 배너 단계가 `loading`이고 그 로드는 에이전트가 넣은 이동(`tasty html open` 등)이다 When 스크립트가 있는 새 문서가 commit된다 Then [사용자가 봤다는 판정](#사용자가-봤다는-판정)대로 `banner.phase`는 `hidden`, `banner.pending_view`는 `true`이고 사용자가 그 surface를 선택할 때 배너가 `blocked`로 뜬다.
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
- **Done** — 성공. WebView overlay 가 페이지를 그린다. 메뉴/팝업으로 overlay 가 일시
  숨겨지거나, 토스트 카드가 이 surface 와 겹쳐 이 WebView 만 숨겨지면(지금 키를 받는 WebView 는 숨기지 않는다) 그 동안 boundary 타일이 보인다. boundary 는 `bg-panel` 배경과 1px `border-default`
  테두리만 있는 빈 타일이며 글리프·라벨·URL 을 그리지 않는다.
- **Failed** — 실패. overlay 를 숨긴 채 `ALERT_CIRCLE`(`accent-danger`) + "Failed to load"
  + URL chrome. 실패 사유는 화면 대신 `tracing::warn!` 로그로만 남긴다. 이 상태에서는 스크립트
  차단 배너와 탭 표지를 그리지 않는다(아래 "스크립트 허용" 절).

Failed chrome 의 URL 줄은 `webview.set_url` 의 선택 인자 `label` 을 먼저 보이고, label 이 없으면
탐색 가능한 URL(`http(s)://`·`file://`)만 보인다. raw HTML 을 url 로 싣는 surface(markdown)는 label 로
문서 경로를 보내므로 chrome 에 HTML 원문이 나오지 않는다. 줄은 mono `font_size_caption`(11) ·
`text_disabled` 한 줄이며 넘치면 말줄임한다. chrome 내용은 좌우에 `space-lg`(16) 여백을 둬(시안 타일의 내용 padding) 긴 줄이 패널 가장자리에 닿지 않는다. 판정은 `webview_chrome.rs::chrome_caption` 이며, 주소로 볼지는 로드 분기(주소로 열기 / raw HTML 싣기)와 같은 `webview::is_navigable_url` 이 정한다.

### 디자인 토큰 매핑

페이지 콘텐츠는 해당 페이지의 스타일을 따른다. Tasty가 그리는 로딩·오류·빈 상태에는 공용 Theme 토큰을 사용한다.

### 갤러리 specimen

`crates/tasty-gallery/src/catalog/components/html_chrome.rs` — Layouts › `Content viewers` ›
`HTML (webview) chrome`. boundary / placeholder / loading / error 4 chrome 상태만 전사(콘텐츠는
overlay). 3자 매핑: [design-gallery-mapping.md](../../design/systems/design-gallery-mapping.md#surface-viewers-plugins).

### 시각 소스

콘텐츠는 OS 네이티브 WebView 가 렌더하므로 design-system 토큰이 적용되지 않는다(웹 페이지 자체 스타일). 타일 정렬/경계만 작업영역 레이아웃을 따른다.
