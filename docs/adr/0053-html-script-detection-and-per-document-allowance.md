# ADR-0053: HTML 문서의 스크립트는 원본 파일에서 감지하고 사용자만 문서 단위로 허용한다

- **Status**: Accepted — 감지·문서 단위 JS 게이트·배너 발화 판정·조회 명령·inset 배너·탭 표지가 구현됐다. Windows와 macOS에서 navigation별 적용 시점과 로드 종료 순서, macOS 서브프레임의 JS 적용은 측정하지 않았다. 두 OS의 로드 세대 구분·process 종료 처리와 macOS 서브프레임 preferences는 실기 측정 없이 구현했다
- **Date**: 2026-09-29
- **Tags**: plugins, webview, html, javascript, sandbox, banner, ipc, security
- **Group**: plugins

## Context

html surface의 JavaScript는 plugin 설정 "Sandbox scripts"를 따르며 기본값은 꺼짐이다(`src/view/main/redraw.rs`의 `resolve_webview_settings`).
스크립트가 있는 문서를 열면 사용자는 결과가 왜 비어 있는지 알 수 없다. 한 문서만 실행하려고 전역 설정을 끄면 이후 연 모든 문서에도 적용된다.
그래서 스크립트가 있는 문서에 배너를 띄우고, 사용자가 그 문서만 허용하는 기능이 필요하다.
이 기능을 만들려면 다음 다섯 가지를 정해야 한다.

- 무엇을 실행 가능한 스크립트로 셀지 정한다.
- 어디서 감지할지 정한다. JS가 꺼진 상태에서 DOM을 조사할 수 있는지는 OS마다 다르다.
- 원격 문서와 markdown을 범위에 넣을지 정한다.
- 허용을 언제 풀지 정한다. 새 문서의 스크립트가 실행되기 전에 풀려야 한다.
- 에이전트가 연 문서에 배너를 언제 띄울지 정한다.

에이전트는 CLI로 html surface를 열 수 있다. 에이전트가 허용 명령까지 쓸 수 있다면, 자기가 연 문서의 JS를 사용자 확인 없이 켤 수 있다.

## Decision

### 감지 대상과 위치

로컬 문서의 `<script>` 요소(데이터 블록 `type` 제외), `on*` 이벤트 핸들러 속성, `javascript:` 스킴 URL을 실행 가능한 스크립트로 센다.
감지는 webview가 아니라 호스트가 한다. 호스트가 `file://` 문서의 원본 파일을 읽어 정적으로 스캔하며, 로드 후 DOM을 조사하지 않는다.
DOM을 조사하지 않는 이유는 OS별로 다음과 같다.

- **Linux(WebKitGTK 2.50.4)**: `enable-javascript`가 꺼져 있으면 `webkit_web_view_evaluate_javascript`가 아무 일도 하지 않는다(API 문서).
  - 측정해 보니 "Cannot execute JavaScript in this document" 오류가 돌아왔다.
  - 문서가 권하는 `enable-javascript-markup` 끄기는 파싱 단계에서 스크립트 요소와 속성을 제거한다. 측정 결과 `script`와 `[onclick]`이 모두 0개였다. 우리가 찾으려는 대상 자체가 사라진다.
- **Windows(WebView2)**: `IsScriptEnabled`가 꺼져 있어도 `ExecuteScript`는 동작한다(API 문서). DOM 조사가 가능하지만 이 OS에만 해당한다.
- **macOS(WKWebView)**: `allowsContentJavaScript`가 꺼져 있어도 호스트가 주입하는 스크립트는 실행된다고 문서에 적혀 있다. 확인은 하지 않았다.

세 OS가 같은 규칙으로 판정해야 하므로, 모두에서 동작하는 원본 스캔을 택한다.

스캔 크기에는 상한을 두고, 내용 지문은 상한과 관계없이 파일 전체를 덮는다. 상한을 넘은 부분에만 스크립트가 있으면 감지하지 못하지만 JS가 꺼진 채 배너가 뜨지 않을 뿐이다. 실패해도 JS가 켜지지 않는 쪽으로 둔다.
감지 대상의 세부 규칙, 상한 값, 정규 파일 확인은 [HTML Viewer 문서](../plugins/html/index.md#스크립트-감지와-허용-규칙)에 있다.

### 범위

- html surface가 연 `file://` 문서만 대상으로 한다.
- 원격 `http(s)` 문서에는 배너를 띄우지 않고 기존 원격 차단 설정을 따른다. 감지하려면 호스트가 문서를 따로 가져와야 하고, 원격 콘텐츠는 기본적으로 차단되어 있기 때문이다.
- markdown surface는 제외한다. 사용자 HTML은 ammonia 허용 목록으로 정리되고(`crates/tasty-plugin-markdown/src/render.rs`), 렌더러가 넣는 스크립트는 신뢰 대상이다([ADR-0029](0029-webview-host-integration.md)).
- 전역 "Sandbox scripts"를 꺼서 이미 JS가 실행되는 상태라면 배너를 띄우지 않는다.

### 허용의 수명

- 허용은 surface, main frame 문서의 URL(fragment 제외), 파일 전체의 내용 지문에 묶인다. 지문은 허용 클릭 때가 아니라 로드할 때 감지와 같은 읽기로 구하고, 클릭은 그 시점의 현재 문서를 기록한다.
  허용 대상이 사용자가 보고 배너 감지 결과가 나온 그 내용이어야 하기 때문이다. 로드 시작 뒤 main frame commit 전에는 허용을 받지 않는다.
- 같은 문서의 재로드에서는 허용을 유지한다. 다른 문서로 이동하거나(뒤로 가기·bfcache 복원 포함) 내용이 바뀐 뒤 재로드하면 푼다. 사용자에게 보이는 문안("until Tasty restarts")과 같은 규칙이다.
- 허용 기록은 다른 main frame 문서가 commit될 때 해제한다. 문서가 바뀌지 않고 끝나는 로드(실패·중단·다운로드·web process 종료)는 화면 문서의 허용 상태로 JS를 되돌리고 기록을 그대로 둔다.
- 허용은 세션 안에서만 유지하고 레이아웃에 저장하지 않는다.
- 허용 판단은 main frame 문서가 바뀔 때에만 적용한다. 서브프레임 navigation, 새 창 요청, fragment 이동은 허용을 바꾸지 않는다.
- 새 문서의 JS는 그 문서의 스크립트가 실행되기 전에 backend의 navigation 콜백이 정한다. redraw에서 사후에 적용하지 않는다. 사후 적용은 그 사이 시작된 다른 문서의 로드에 이전 문서의 허용이 새게 한다.
  설정 경로(`resolve_webview_settings`)는 전역 "Sandbox scripts" 값만 backend에 넘긴다.
- Linux html surface는 page cache를 끈다. 캐시 복원은 응답 단계 없이 commit돼 지문이 없는 문서를 만들기 때문이다.

허용 수명의 세부 규칙, OS별 콜백과 측정, 로드 세대 판정은 [HTML Viewer 문서](../plugins/html/index.md#스크립트-감지와-허용-규칙)에 있다.

### 배너 표시 시점

에이전트가 IPC·CLI로 연 문서, 세션 복원으로 열린 문서, 백그라운드 탭의 문서는 감지 결과만 기록한다.
배너는 사용자가 그 문서를 볼 때 띄운다. 사용자가 직접 문서를 열었거나 그 surface를 선택한 경우다.
배너를 띄우려고 포커스나 활성 탭을 바꾸지 않는다(원칙 1).
판정 신호는 [HTML Viewer 문서](../plugins/html/index.md#사용자가-봤다는-판정)에 있다.
에이전트 동작이 배너로 이어지는 경우에 대한 [ADR-0036](0036-overlay-scope-and-lifetime.md)의 재검토 조건을 이 규칙으로 다룬다. 에이전트 동작은 상태만 만들고, 배너는 사용자 화면에서 사용자가 볼 때만 나타난다.

### 에이전트 경로

- 감지 결과와 허용 상태의 조회는 release IPC와 CLI로 제공한다(원칙 2). 상태를 읽을 뿐 사용자 화면이나 입력을 바꾸지 않는다.
- IPC·CLI로 스크립트를 허용하는 명령은 debug 빌드에서만 제공한다.
  - 핸들러는 모듈 선언에 `#[cfg(debug_assertions)]`가 붙은 별도 파일에 둔다([debug IPC 가이드](../dev-guide/debug-ipc.md)).
- release에서는 사용자가 GUI 배너나 탭 마커로만 허용한다.
- 이유는 보안이다. 에이전트는 문서를 열 수 있다. release 명령으로 허용까지 할 수 있으면, 에이전트가 연 문서의 JS를 에이전트 스스로 켜는 경로가 생긴다. 사용자의 클릭을 거치게 해야 이 경로가 사라진다.
  - 이 절이 없애는 것은 허용 명령 경로다. 지문 계산과 webview 읽기 사이의 경합은 Consequences에 적은 대로 수용한다.

배너는 surface가 소유하는 배너다. 키보드 포커스를 가져가지 않는다는 규칙은 [ADR-0036](0036-overlay-scope-and-lifetime.md)을 따른다. 위치·문안·마커는 디자인 결정을 따른다.

## Consequences

- 사용자는 전역 설정을 바꾸지 않고 한 문서만 실행할 수 있다. 허용은 다른 문서로 넘어가지 않고, 같은 문서의 재로드에서는 유지된다.
- 세 OS가 같은 판정을 쓰므로 조회 결과도 OS와 관계없이 같다.
- 허용은 webview 전체의 JS를 켠다. 허용한 문서가 `<iframe>`, `<object>`, `<embed>`로 불러오는 다른 로컬 파일의 스크립트도 함께 실행된다.
  - 감지는 main frame 문서의 원본만 보므로, 그 파일들의 스크립트는 배너 판단에 들어가지 않는다. 외부 파일로 불러오는 SVG의 스크립트도 같다.
  - 따라서 "이 문서만 허용"은 main frame 문서 단위라는 뜻이다. 그 문서가 불러오는 로컬 파일까지 허용 범위에 들어간다.
- 정적 스캔은 실제 실행 여부와 다를 수 있다.
  - 조건부 주석이나 문자열 안의 태그 모양 때문에 오탐이 생길 수 있다. 오탐이 생겨도 배너가 뜨는 것뿐이고 JS가 켜지지는 않는다.
  - 크기 상한을 넘은 부분만의 스크립트는 놓친다. 이 경우 JS가 꺼진 채 배너가 없다.
- 모든 main frame `file://` 로드에서 파일 전체를 읽어 해시한다. 허용 여부와 관계없다. 스캔 상한은 이 비용을 제한하지 않는다.
  - 큰 파일에서는 응답 결정이 해시가 끝날 때까지 늦어진다.
  - Linux와 macOS에서 응답 결정은 main thread 콜백이다. 읽기와 해시를 이 콜백과 분리해 main thread 밖에서 할 수 있는지는 검토하지 않았다. Linux는 문서상 결정을 보류할 수 있다.
- Linux html surface는 page cache를 쓰지 않는다. 뒤로·앞으로 가기가 캐시 복원 대신 파일을 다시 읽는 새 로드가 된다.
  - page cache가 주던 다른 동작도 달라질 수 있다. 뒤로·앞으로 가기에서 스크롤 위치와 폼 입력이 보존되는지가 해당하며, 측정하지 않았다.
- 로드 때 지문을 구한 뒤 webview가 파일 본문을 읽기 전에 내용이 바뀌면, 바뀐 내용이 JS가 켜진 채 실행된다. 남는 창은 이 구간뿐이다. 지문을 로드 때 구하므로, 사용자가 본 뒤 클릭 전에 바꾸는 경로는 없다.
  - 리뷰 측정에서 응답 핸들러 안에서 파일을 바꾸자 바뀐 스크립트가 실행됐다. 실제 창의 길이와 적중 확률은 측정하지 않았다.
  - 파일을 쓸 수 있는 에이전트는 내용을 번갈아 쓰면서 release IPC로 같은 URL 로드를 반복해 이 창을 노릴 수 있다. 전제는 두 가지다. 에이전트에게 그 파일의 쓰기 권한이 있고, 사용자가 그 문서를 한 번 허용했어야 한다.
  - 이 위험은 수용한다. 파일을 쓸 수 있는 에이전트는 이미 셸로 임의 코드를 실행할 수 있다. 따라서 이 창이 새 권한을 주지 않는다. 스크립트 차단이 막는 대상은 신뢰하지 않는 HTML이며, 로컬 에이전트가 아니다.
- 해제 시점을 navigation 콜백에 두므로 세 백엔드의 콜백 코드를 유지해야 한다.
  - Linux는 `ResponsePolicyDecision::is_main_frame_main_resource()`가 필요해 바인딩 feature를 `v2_40`으로 둔다(`Cargo.toml`). 최소 런타임이 WebKitGTK 2.40이 된다.
  - Windows·macOS는 navigation별 적용 시점, 앞 로드의 늦은 종료·commit 신호 순서, bfcache 복원, macOS 서브프레임 preferences를 측정하지 않았다. 앞 로드의 종료가 새 로드 시작 뒤에 오면 새 문서가 지문 없이 기록될 수 있어 두 OS는 로드 세대로 막는다. 남은 한계는 [HTML Viewer 문서](../plugins/html/index.md#os별-적용)에 있다.
- 에이전트는 release에서 스크립트 문서를 자동으로 실행할 수 없다. 자동화에는 debug 빌드나 전역 설정이 필요하다.
- 에이전트가 연 문서는 사용자가 그 문서를 보기 전까지 배너 없이 차단 상태로 남는다.

## Alternatives Considered

- **로드 후 DOM 조사**: Linux에서는 JS가 꺼져 있으면 호출이 동작하지 않는다(측정). Windows에서만 가능하다.
- **`enable-javascript-markup` 끄기 후 조사**: 파싱 단계에서 찾을 대상이 제거된다(측정).
- **감지 전용으로 JS를 잠깐 켜기**: 조사하는 동안 문서의 스크립트가 실행될 수 있다. 차단하려는 동작이 그대로 일어난다.
- **원격 문서를 따로 가져와 스캔**: 원격 차단 설정을 우회하는 추가 요청이 생긴다. 이 요청과 webview의 요청이 서로 다른 응답을 받을 수 있다.
- **release IPC·CLI 허용 명령**: 에이전트가 자기가 연 문서의 JS를 사용자 확인 없이 켤 수 있다.
- **redraw에서 사후 해제**: 새 문서의 첫 스크립트가 해제 전에 실행될 수 있다.
- **Linux에서 navigation action의 URL 비교로 해제**: 서브프레임과 main frame을 구분할 수 없다. iframe이 있는 허용 문서에서 JS가 곧바로 다시 꺼졌다(측정).
- **Linux에서 back-forward type의 navigation action에서 끄기**: page cache가 켜진 상태의 bfcache 누수는 막았다(측정). 하지만 서브프레임의 back-forward에도 걸린다. page cache를 끈 상태의 측정에서 iframe의 뒤로 가기도 type이 `back-forward`였다. 그래서 main frame 전용인 `load-changed`를 택했다.
- **Linux `STARTED` 시점의 `get_uri()`로 허용 판정**: 링크 이동에서 이전 문서 URI를 돌려준다(측정).
- **허용 직후 재로드만 표시해 구분하고 사용자 재로드에서 풀기**: 문안의 "until Tasty restarts"와 다르다. 같은 내용의 문서를 다시 여는 데 매번 확인을 요구할 이유도 없다.
- **허용을 URL에만 묶기**: 같은 경로의 파일 내용이 바뀌어도 허용이 남는다.
- **허용 기록을 응답 단계에서 해제**: 다운로드 응답 한 번으로 화면의 허용 문서가 허용을 잃는다. 없는 파일 실패에서는 해제가 일어나지 않는 등 경로마다 결과가 달라진다(리뷰 측정). 그래서 commit에서 해제한다.
- **commit 없이 끝난 로드 뒤 JS를 꺼 둔 채 두기**: 허용 문서가 멈춘 채 남는다. 사용자가 재로드해야 복구된다.
- **허용 클릭 때 지문 구하기**: 허용이 없는 문서는 해시하지 않아도 된다. 하지만 사용자가 본 뒤 클릭 전에 파일을 바꾸면 그 내용이 경합 없이 실행된다. 리뷰 측정에서 제목이 `EVIL-ran`이 됐다. 허용 대상이 사용자가 본 내용이 아니게 된다.
- **로드마다 대기 값을 비우지 않기**: 허용 재로드의 "일치"가 이어지는 bfcache commit에 남는다(page cache를 켠 상태). 앞으로 가기로 돌아온 문서에서 commit 없는 로드 뒤 JS가 복원됐다(리뷰 측정).
- **webview의 현재 URI로 화면 문서 판정**: 취소된 로드와 `stop_loading` 뒤에는 화면에 없는 URL을 돌려준다(리뷰 측정).
- **Linux page cache 유지**: 캐시 복원에는 응답 단계가 없어 지문 없는 문서가 생긴다. 로컬 파일의 재로드 비용보다 이 예외를 없애는 쪽을 택했다.
- **지문을 스캔 상한까지만 구하기**: 상한 뒤에 덧붙인 스크립트가 허용된 채 실행된다(리뷰 측정).
- **상한을 넘는 파일은 허용할 수 없게 하기**: 큰 문서는 전역 설정 없이 실행할 방법이 없어진다. 전체 해시 비용을 감수하는 쪽을 택했다.
- **호스트가 스캔·해시한 바이트를 webview에 그대로 공급**(custom scheme 등): 경합은 없어진다. 대신 `file://` origin과 상대 경로 해석, 하위 리소스 접근이 달라져 문서 동작이 바뀐다. 이 비용에 비해 막는 위협이 새 권한이 아니다.
- **로드 후 재해시로 불일치 탐지**: 불일치를 알았을 때는 스크립트가 이미 실행된 뒤라 사후 대응일 뿐이다.
- **허용을 레이아웃에 저장**: 재시작 후 사용자가 다시 확인하지 않은 문서가 실행된다. 같은 경로에 다른 내용이 놓일 수도 있다.
- **에이전트가 연 문서에 즉시 배너 표시**: 에이전트 동작이 사용자 화면에 끼어든다. 백그라운드 탭에서는 사용자가 볼 수도 없다.

## Reconsideration Triggers

코드와 설정에서 확인한다.

- 원격 문서의 원격 콘텐츠 기본값이 허용으로 바뀌면 원격 문서도 범위에 넣을지 다시 정한다.
- markdown의 ammonia 허용 목록에 스크립트 관련 요소나 속성이 추가되면 markdown 범위를 다시 정한다.
- WebKitGTK가 JS 비활성 상태에서도 호스트 전용 스크립트 실행 경로를 제공하면 DOM 조사를 다시 비교한다.
  - 예를 들어 script world 기준의 실행 경로가 해당된다.
- WebKitGTK의 `NavigationAction`이 main frame 여부를 제공하면 `load-changed`와 `RESPONSE`의 조합을 navigation action 하나로 줄일 수 있는지 비교한다.
- 서브프레임별로 JS를 켜고 끌 수 있는 API가 생기면 허용 범위를 main frame 문서로 좁힐지 다시 정한다.
- 지원하는 Linux 배포판의 WebKitGTK가 2.40보다 낮으면 main frame 판별 방법을 다시 정한다.
- 에이전트가 파일 쓰기 없이 로드만 할 수 있는 실행 형태가 생기면 지문과 읽기 사이의 경합을 다시 정한다. 원격 에이전트나 쓰기 권한이 없는 샌드박스 에이전트가 예다. 이때는 이 창이 새 권한이 되므로, 호스트가 읽은 바이트를 공급하는 방식을 다시 비교한다.
- Windows나 macOS에서 응답 단계 없이 commit되는 문서가 생기고 bfcache를 끌 수 없으면, 지문 없는 문서의 허용 방법과 배너 동작을 디자인과 함께 다시 정한다.
- `load-failed` 핸들러가 WebKit 기본 오류 페이지를 쓰도록 바뀌면 commit 없는 로드의 복원 규칙을 다시 확인한다.
- WebKitGTK가 web process 종료 뒤 `FINISHED`를 보내도록 바뀌면 종료 핸들러의 로드 종료 처리를 뺄 수 있는지 확인한다.

실행 결과로 확인한다.

- Windows에서 `NavigationStarting` 안의 설정이 같은 navigation에 적용되지 않으면 해제 시점을 다시 정한다.
- macOS에서 앞 로드의 종료가 정책 결정과 provisional 시작 사이에 오는 것이 측정되면 세대를 기록하는 시점을 다시 정한다.
- Windows나 macOS에서 앞 로드의 commit 신호가 새 로드 시작 뒤에 오는 것이 측정되면 commit 신호에도 세대 판정을 둘지 두 OS를 함께 다시 정한다.
- macOS에서 서브프레임 navigation의 preferences를 main frame 문서의 판단에 맞췄는데도 차단 문서의 iframe 스크립트가 실행되면 서브프레임의 차단 방법을 다시 정한다.
- macOS에서 `allowsContentJavaScript`를 설정한 navigation의 첫 로드에 스크립트가 실행되면 해제 시점을 다시 정한다.
- Windows나 macOS의 bfcache 복원에서 허용되지 않은 문서의 스크립트가 다시 실행되면 복원 경로의 차단을 추가한다.
- 두 OS 모두 Linux와 같은 방법으로 확인한다.
  - 스크립트가 제목을 바꾸는 문서 두 개를 만들고, 링크로 이동한 뒤 새 문서의 제목을 읽는다.
  - iframe이 있는 허용 문서를 연다. 뒤로 가기로 timer가 있는 문서를 복원한다.
  - macOS에서는 iframe이 있는 차단 문서를 열고 iframe 스크립트가 실행되는지 확인한다.
  - 로드 중 링크를 눌러 앞 로드를 취소하고, 앞 로드의 종료 신호와 새 로드의 시작 순서를 기록한다.
- 사용자가 오탐이나 미탐을 보고하면 스캔 규칙과 실제 실행 여부를 같은 문서로 비교한다.

## References

- [ADR-0029](0029-webview-host-integration.md): webview 채널, 원격 콘텐츠 차단.
- [ADR-0036](0036-overlay-scope-and-lifetime.md): 배너의 입력과 수명. 에이전트 동작과 배너에 관한 재검토 조건.
- [debug IPC 가이드](../dev-guide/debug-ipc.md): debug 전용 핸들러의 격리 조건.
- [Webview 호스트 계약](../design/systems/webview.md).
- `src/view/main/redraw.rs`의 `resolve_webview_settings`: 전역 sandbox 값 전달.
- `src/host_api/webview/script_gate.rs`: navigation 콜백과 허용 상태의 연결.
- 현재 navigation 콜백:
  - `src/host_api/webview/linux.rs`의 `connect_decide_policy`, `connect_load_failed`, `connect_web_process_terminated`
  - `src/host_api/webview/macos.rs`의 navigation delegate
  - `src/host_api/webview/windows.rs`의 `add_NavigationStarting`
- `Cargo.toml`의 `webkit2gtk` feature.
- 플랫폼 문서:
  - WebKitGTK `WebKitSettings:enable-javascript`, `enable-javascript-markup`, `webkit_web_view_evaluate_javascript`, `webkit_response_policy_decision_is_main_frame_main_resource`, `WebKitWebView::load-changed`
  - WebView2 `ICoreWebView2Settings::IsScriptEnabled`, `ICoreWebView2::NavigationStarting`, `FrameNavigationStarting`, `NewWindowRequested`
  - Apple `WKWebpagePreferences.allowsContentJavaScript`, `WKNavigationAction.targetFrame`, `WKFrameInfo.isMainFrame`
