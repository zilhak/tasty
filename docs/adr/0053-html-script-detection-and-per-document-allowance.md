# ADR-0053: HTML 문서의 스크립트는 원본 파일에서 감지하고 사용자만 문서 단위로 허용한다

- **Status**: Accepted — 결정만 확정했고 구현은 아직 없다. Windows와 macOS의 navigation별 적용 시점은 측정하지 않았다
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

### 감지 대상

로컬 문서에서 다음 셋 중 하나라도 있으면 실행 가능한 스크립트가 있다고 판단한다.

- `<script>` 요소. inline이든 `src`든 같다. `<svg>` 안의 inline `<script>`도 포함한다. 단 `type`이 JavaScript MIME 형식이나 `module`이 아니면 제외한다. `application/json`, `text/template` 같은 데이터 블록이 이 예외에 해당한다.
- 요소의 `on*` 이벤트 핸들러 속성.
- `javascript:` 스킴 URL. `href`, `src`, `action`, `formaction` 속성의 값을 검사한다.

원격 콘텐츠가 차단된 상태에서 `http(s)` `src` 스크립트만 있으면 허용해도 실행되지 않는다. 이때는 배너 본문을 원격 안내 문구(`bodyRemote`)로 바꾼다.

### 감지 위치

감지는 webview가 아니라 호스트가 한다. 호스트가 `file://` 문서의 원본 파일을 읽어 정적으로 스캔하며, 로드 후 DOM을 조사하지 않는다.
DOM을 조사하지 않는 이유는 OS별로 다음과 같다.

- **Linux(WebKitGTK 2.50.4)**: `enable-javascript`가 꺼져 있으면 `webkit_web_view_evaluate_javascript`가 아무 일도 하지 않는다(API 문서).
  - 측정해 보니 "Cannot execute JavaScript in this document" 오류가 돌아왔다.
  - 문서가 권하는 `enable-javascript-markup` 끄기는 파싱 단계에서 스크립트 요소와 속성을 제거한다. 측정 결과 `script`와 `[onclick]`이 모두 0개였다. 우리가 찾으려는 대상 자체가 사라진다.
- **Windows(WebView2)**: `IsScriptEnabled`가 꺼져 있어도 `ExecuteScript`는 동작한다(API 문서). DOM 조사가 가능하지만 이 OS에만 해당한다.
- **macOS(WKWebView)**: `allowsContentJavaScript`가 꺼져 있어도 호스트가 주입하는 스크립트는 실행된다고 문서에 적혀 있다. 확인은 하지 않았다.

세 OS가 같은 규칙으로 판정해야 하므로, 모두에서 동작하는 원본 스캔을 택한다.
스캔하는 크기에는 상한을 둔다. 상한 값은 구현 문서에서 정한다.
상한을 넘은 부분에만 스크립트가 있으면 감지하지 못한다. 이 경우 JS가 꺼진 채 배너가 뜨지 않는다. 즉 실패해도 JS가 켜지지는 않는다.

### 범위

- html surface가 연 `file://` 문서만 대상으로 한다.
- 원격 `http(s)` 문서에는 배너를 띄우지 않고 기존 원격 차단 설정을 따른다. 감지하려면 호스트가 문서를 따로 가져와야 하고, 원격 콘텐츠는 기본적으로 차단되어 있기 때문이다.
- markdown surface는 제외한다. 사용자 HTML은 ammonia 허용 목록으로 정리되고(`crates/tasty-plugin-markdown/src/render.rs`), 렌더러가 넣는 스크립트는 신뢰 대상이다([ADR-0029](0029-webview-host-integration.md)).
- 전역 "Sandbox scripts"를 꺼서 이미 JS가 실행되는 상태라면 배너를 띄우지 않는다.

### 허용의 수명

허용은 surface, main frame 문서의 URL, 내용 지문에 묶인다. URL은 fragment를 뺀 값이다. 내용 지문은 스캔한 원본에서 구한다.
사용자에게 보이는 문안("until Tasty restarts", 갤러리의 "cleared by: navigation to another document · app restart")과 같은 규칙이다.

- **같은 문서의 재로드**: 허용을 유지한다. 사용자의 재로드(F5 등)와 허용 직후 호스트가 하는 재로드를 구분하지 않는다.
- **다른 문서로 이동**: 허용을 푼다. 그 뒤 같은 URL로 돌아와도 다시 허용해야 한다. 뒤로 가기와 bfcache 복원도 여기에 포함된다.
- **내용이 바뀐 뒤의 재로드**: 다른 문서로 본다. 다시 스캔한 결과나 내용 지문이 허용 당시와 다르면 허용을 푼다.
- **세션 한정**: 허용은 세션 안에서만 유지한다. 레이아웃에 저장하지 않으므로 재시작하면 다시 차단 상태가 된다.

허용 판단은 **main frame의 문서가 바뀔 때에만** 적용한다. 다음 경우에는 허용을 바꾸지 않는다.

- 같은 문서 안의 서브프레임(`<iframe>`, `<object>`, `<embed>`) navigation.
- 새 창 요청(`target=_blank` 등). 현재 문서를 바꾸지 않는다.
- fragment만 바뀌는 이동.

문서가 바뀌면 새 문서의 스크립트가 실행되기 전에 JS를 끈다. redraw에서 사후에 되돌리지 않는다. OS별 구현은 다음과 같다.

- **Linux(측정함, WebKitGTK 2.50.4)**: `NavigationAction`에는 main frame과 서브프레임을 구분할 정보가 없다. iframe 문서의 navigation도 `get_frame_name()`이 None이고 type이 `other`로 같았다. 그래서 navigation action에서는 판단하지 않고, 다음 두 신호를 쓴다.
  - `load-changed` `STARTED`: main frame 로드에서만 온다(측정 결과 iframe 로드에서는 오지 않았다). 여기서 JS를 무조건 끈다.
    - 이 시점의 `get_uri()`는 링크 이동에서 이전 문서 URI를 돌려준다(측정). 그래서 URL을 판정에 쓰지 않는다.
  - `decide-policy`의 `RESPONSE` 결정: `ResponsePolicyDecision::is_main_frame_main_resource()`(2.40부터)가 참일 때만 응답 URL과 내용 지문으로 허용 여부를 정해 JS를 켜거나 끈다.
    - 측정에서 iframe 응답은 거짓이었다.
    - 여기서 켜고 끈 설정은 그 문서의 첫 스크립트부터 적용됐다.
  - `NEW_WINDOW_ACTION`은 별도 결정 type으로 오며 허용에 영향을 주지 않는다.
  - fragment 이동에는 `load-changed`가 오지 않는다. JS 상태가 유지됐다.
  - bfcache 복원에는 `RESPONSE` 결정이 오지 않는다. `STARTED`에서 끈 상태가 유지되어, 복원된 문서의 timer가 멈춘 채로 남았다.
    - `STARTED`에서 끄지 않으면 허용된 문서에서 뒤로 가기로 복원한 허용되지 않은 문서의 timer가 다시 실행됐다(측정).
  - main frame 로드가 실패하면 WebKit이 오류 페이지로 문서를 교체한다(측정: 없는 파일). 따라서 꺼진 상태가 맞다.
- **Windows(미측정, API 문서 근거)**: `NavigationStarting`은 main frame navigation에서만 발생한다. 서브프레임은 `FrameNavigationStarting`, 새 창은 `NewWindowRequested`로 따로 온다. 이 핸들러 안에서 `IsScriptEnabled`를 정한다. API 문서의 예제도 이 핸들러에서 해당 navigation에 적용되도록 설정을 바꾼다. `NavigationStarting` 이후에 바꾸면 다음 top-level navigation부터 적용된다.
- **macOS(미측정, API 문서 근거)**: 현재의 전역 `javaScriptEnabled`(deprecated) 대신 `webView:decidePolicyForNavigationAction:preferences:decisionHandler:`에서 navigation별 `WKWebpagePreferences.allowsContentJavaScript`를 정한다. `targetFrame.isMainFrame`이 참일 때만 판단한다. 새 창 요청은 `targetFrame`이 nil이다.
- Windows와 macOS의 bfcache 복원에 navigation별 설정이 적용되는지는 측정하지 않았다.

### 배너 표시 시점

에이전트가 IPC·CLI로 연 문서, 세션 복원으로 열린 문서, 백그라운드 탭의 문서는 감지 결과만 기록한다.
배너는 사용자가 그 문서를 볼 때 띄운다. 사용자가 직접 문서를 열었거나 그 surface를 선택한 경우다.
배너를 띄우려고 포커스나 활성 탭을 바꾸지 않는다(원칙 1).
에이전트 동작이 배너로 이어지는 경우에 대한 [ADR-0036](0036-overlay-scope-and-lifetime.md)의 재검토 조건을 이 규칙으로 다룬다. 에이전트 동작은 상태만 만들고, 배너는 사용자 화면에서 사용자가 볼 때만 나타난다.

### 에이전트 경로

- 감지 결과와 허용 상태의 조회는 release IPC와 CLI로 제공한다(원칙 2). 상태를 읽을 뿐 사용자 화면이나 입력을 바꾸지 않는다.
- IPC·CLI로 스크립트를 허용하는 명령은 debug 빌드에서만 제공한다.
  - 핸들러는 모듈 선언에 `#[cfg(debug_assertions)]`가 붙은 별도 파일에 둔다([debug IPC 가이드](../dev-guide/debug-ipc.md)).
- release에서는 사용자가 GUI 배너나 탭 마커로만 허용한다.
- 이유는 보안이다. 에이전트는 문서를 열 수 있다. release 명령으로 허용까지 할 수 있으면, 에이전트가 연 문서의 JS를 에이전트 스스로 켜는 경로가 생긴다. 사용자의 클릭을 거치게 해야 이 경로가 사라진다.

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
- 내용 지문을 다시 구하려면 main frame 응답마다 원본을 읽어야 한다. 크기 상한이 이 비용도 제한한다. 지문을 구한 뒤 webview가 파일을 읽기 전에 내용이 바뀌는 짧은 경합은 남는다.
- 해제 시점을 navigation 콜백에 두므로 세 백엔드의 콜백 코드를 수정해야 한다.
  - Linux는 `ResponsePolicyDecision::is_main_frame_main_resource()`가 필요하다. 이 API는 WebKitGTK 2.40부터 있어 바인딩 feature를 `v2_40`으로 올려야 한다(현재 `v2_38`).
  - Windows는 `NavigationStarting` 안에서 정한 `IsScriptEnabled`가 같은 navigation에 적용되는지 측정하지 않았다. API 문서가 근거다.
  - macOS는 delegate 시그니처가 바뀌고, 실제 Mac에서 확인하기 전까지 미검증이다.
  - Windows와 macOS의 bfcache 복원 동작도 미측정이다.
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
- **Linux에서 back-forward type의 navigation action에서 끄기**: bfcache 누수는 막았지만(측정) 서브프레임의 back-forward에도 걸릴 수 있다. main frame 전용인 `load-changed`를 택했다.
- **Linux `STARTED` 시점의 `get_uri()`로 허용 판정**: 링크 이동에서 이전 문서 URI를 돌려준다(측정).
- **허용 직후 재로드만 표시해 구분하고 사용자 재로드에서 풀기**: 문안의 "until Tasty restarts"와 다르다. 같은 내용의 문서를 다시 여는 데 매번 확인을 요구할 이유도 없다.
- **허용을 URL에만 묶기**: 같은 경로의 파일 내용이 바뀌어도 허용이 남는다.
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

실행 결과로 확인한다.

- Windows에서 `NavigationStarting` 안의 설정이 같은 navigation에 적용되지 않으면 해제 시점을 다시 정한다.
- macOS에서 `allowsContentJavaScript`를 설정한 navigation의 첫 로드에 스크립트가 실행되면 해제 시점을 다시 정한다.
- Windows나 macOS의 bfcache 복원에서 허용되지 않은 문서의 스크립트가 다시 실행되면 복원 경로의 차단을 추가한다.
- 두 OS 모두 Linux와 같은 방법으로 확인한다.
  - 스크립트가 제목을 바꾸는 문서 두 개를 만들고, 링크로 이동한 뒤 새 문서의 제목을 읽는다.
  - iframe이 있는 허용 문서를 연다. 뒤로 가기로 timer가 있는 문서를 복원한다.
- 사용자가 오탐이나 미탐을 보고하면 스캔 규칙과 실제 실행 여부를 같은 문서로 비교한다.

## References

- [ADR-0029](0029-webview-host-integration.md): webview 채널, 원격 콘텐츠 차단.
- [ADR-0036](0036-overlay-scope-and-lifetime.md): 배너의 입력과 수명. 에이전트 동작과 배너에 관한 재검토 조건.
- [debug IPC 가이드](../dev-guide/debug-ipc.md): debug 전용 핸들러의 격리 조건.
- [Webview 호스트 계약](../design/systems/webview.md).
- `src/view/main/redraw.rs`의 `resolve_webview_settings`: 현재 JS 설정 결정.
- 현재 navigation 콜백:
  - `src/host_api/webview/linux.rs`의 `connect_decide_policy`
  - `src/host_api/webview/macos.rs`의 navigation delegate
  - `src/host_api/webview/windows.rs`의 `add_NavigationStarting`
- `Cargo.toml`의 `webkit2gtk` feature.
- 플랫폼 문서:
  - WebKitGTK `WebKitSettings:enable-javascript`, `enable-javascript-markup`, `webkit_web_view_evaluate_javascript`, `webkit_response_policy_decision_is_main_frame_main_resource`, `WebKitWebView::load-changed`
  - WebView2 `ICoreWebView2Settings::IsScriptEnabled`, `ICoreWebView2::NavigationStarting`, `FrameNavigationStarting`, `NewWindowRequested`
  - Apple `WKWebpagePreferences.allowsContentJavaScript`, `WKNavigationAction.targetFrame`, `WKFrameInfo.isMainFrame`
