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
스캔하는 크기에는 상한을 둔다. 상한 값은 구현 문서에서 정한다. 상한은 감지 스캔에만 적용하며, 아래 내용 지문은 상한과 관계없이 파일 전체를 덮는다.
상한을 넘은 부분에만 스크립트가 있으면 감지하지 못한다. 이 경우 JS가 꺼진 채 배너가 뜨지 않는다. 즉 실패해도 JS가 켜지지는 않는다.
정규 파일만 스캔한다. FIFO·장치 파일은 거절하므로 지문이 없고, JS가 꺼진 채 허용할 수 없다.
스캔은 응답 단계의 UI 스레드에서 한다. FIFO는 여는 순간 쓰는 쪽을 기다리며 막혔고 `/dev/zero`는 읽기가 끝나지 않았다(측정). 그래서 막히지 않게 연 핸들로 종류를 먼저 확인한다.

### 범위

- html surface가 연 `file://` 문서만 대상으로 한다.
- 원격 `http(s)` 문서에는 배너를 띄우지 않고 기존 원격 차단 설정을 따른다. 감지하려면 호스트가 문서를 따로 가져와야 하고, 원격 콘텐츠는 기본적으로 차단되어 있기 때문이다.
- markdown surface는 제외한다. 사용자 HTML은 ammonia 허용 목록으로 정리되고(`crates/tasty-plugin-markdown/src/render.rs`), 렌더러가 넣는 스크립트는 신뢰 대상이다([ADR-0029](0029-webview-host-integration.md)).
- 전역 "Sandbox scripts"를 꺼서 이미 JS가 실행되는 상태라면 배너를 띄우지 않는다.

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
      - Windows·macOS 백엔드도 같은 방식으로 로드를 끝낸다. 구현은 했지만 실기에서 측정하지 않았다. 같은 증상이 나는지와 종료 뒤 어떤 navigation 신호가 오는지는 모른다.
        - macOS는 `webViewWebContentProcessDidTerminate:`에서 로드를 끝낸다.
        - Windows는 `ProcessFailed` 중 main frame 문서를 그리던 process가 끝난 종류(`RENDER_PROCESS_EXITED`·`BROWSER_PROCESS_EXITED`)에서만 끝낸다. iframe·GPU·utility process 종료와 `RENDER_PROCESS_UNRESPONSIVE`는 main frame 로드를 끝내지 않으므로 건너뛴다.
    - 측정 결과 허용 문서의 timer가 다시 돌았다(tick 14→19, JS True).
  - `stop_loading`이나 기존 정책이 무시하는 `http(s)` navigation에는 `STARTED`가 오지 않았다. 따라서 JS와 문서가 그대로다(리뷰 측정).
- **Windows(미측정, API 문서 근거)**: `NavigationStarting`은 main frame navigation에서만 발생한다. 서브프레임은 `FrameNavigationStarting`, 새 창은 `NewWindowRequested`로 따로 온다. 이 핸들러 안에서 `IsScriptEnabled`를 정한다. API 문서의 예제도 이 핸들러에서 해당 navigation에 적용되도록 설정을 바꾼다. `NavigationStarting` 이후에 바꾸면 다음 top-level navigation부터 적용된다.
- **macOS(미측정, API 문서 근거)**: `webView:decidePolicyForNavigationAction:preferences:decisionHandler:`에서 navigation별 `WKWebpagePreferences.allowsContentJavaScript`를 정한다. `targetFrame.isMainFrame`이 참일 때만 로드를 시작하고 판단한다. 서브프레임 navigation에는 그 시점 main frame 문서에 대한 게이트의 판단(`ScriptGate::effective_js`)을 넣는다. Linux에서 서브프레임이 webview 전체 JS 설정을 따르는 것과 같은 결과다. 새 창 요청은 `targetFrame`이 nil이며 preferences를 바꾸지 않는다.
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
    - 실기 측정 없이 구현했다. 이 머신에는 macOS용 C 컴파일러가 없어 macOS 코드를 컴파일하지 못했다.

### 배너 표시 시점

에이전트가 IPC·CLI로 연 문서, 세션 복원으로 열린 문서, 백그라운드 탭의 문서는 감지 결과만 기록한다.
배너는 사용자가 그 문서를 볼 때 띄운다. 사용자가 직접 문서를 열었거나 그 surface를 선택한 경우다.
배너를 띄우려고 포커스나 활성 탭을 바꾸지 않는다(원칙 1).

"사용자가 봤다"는 다음 신호로 판정한다.

- 포커스된 surface가 바뀐 순간을 사용자 선택으로 본다. release의 에이전트 경로는 탭과 surface 선택을 바꾸지 않기 때문이다. 창을 처음 그리는 프레임의 포커스는 선택으로 세지 않는다. 그래서 세션 복원 문서는 기록만 한다.
- 문서가 없거나 로드 중일 때 선택되면 다음에 commit되는 문서를 본 것으로 한다. 사용자가 연 파일은 새 탭이 선택된 뒤 첫 문서가 오기 때문이다.
- 사용자가 보던 문서에서 링크나 스크립트로 바뀐 문서는 본 상태를 이어받는다. 호스트가 URL을 넣은 로드(plugin·IPC·복원)는 이어받지 않는다. 그래서 에이전트가 사용자가 보던 surface에 넣은 문서도 다음 선택 때까지 기록만 한다.
- 한계: 에이전트가 포커스된 탭을 닫아 포커스가 다른 surface로 옮겨 가는 경우도 선택으로 센다.
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
- 해제 시점을 navigation 콜백에 두므로 세 백엔드의 콜백 코드를 수정해야 한다.
  - Linux는 `ResponsePolicyDecision::is_main_frame_main_resource()`가 필요하다. 이 API는 WebKitGTK 2.40부터 있어 바인딩 feature를 `v2_40`으로 둔다(`Cargo.toml`). 최소 런타임도 WebKitGTK 2.40이 된다.
  - Windows는 `NavigationStarting` 안에서 정한 `IsScriptEnabled`가 같은 navigation에 적용되는지 측정하지 않았다. API 문서가 근거다.
  - macOS는 delegate 시그니처가 바뀌고, 실제 Mac에서 확인하기 전까지 미검증이다.
  - 앞 로드의 종료가 새 로드 시작 뒤에 오면 새 문서가 지문 없이 기록될 수 있다. 그 문서는 스크립트가 있어도 배너가 뜨지 않고 허용할 수 없으며, 허용된 문서의 재로드였다면 허용이 풀린다. Windows는 `NavigationId`, macOS는 `WKNavigation` 세대로 이 경우를 막는다. macOS는 정책 결정과 provisional 시작 사이에 온 종료를 막지 못한다. 두 OS 모두 순서를 측정하지 않았다.
  - commit 신호(Windows `ContentLoading`, macOS `didCommitNavigation`)는 세대를 보지 않는다. 종료 신호와 달리 commit은 어느 로드의 것이든 화면 문서를 기록한다. 앞 로드의 늦은 commit이 새 로드의 응답 단계 결과를 소비할 수 있는지는 측정하지 않았다.
    - 고치지 않는 이유: commit은 그 문서가 실제로 화면에 올라왔다는 신호다. 늦은 commit을 무시하면 화면에 있는 문서가 현재 문서로 기록되지 않는다. 또 Windows와 macOS의 세대 규칙을 함께 정해야 해서 한 OS만 먼저 바꾸지 않는다.
  - macOS의 `decidePolicyForNavigationAction`은 서브프레임 navigation에 main frame 문서에 대한 판단을 preferences로 돌려준다. WebKit이 서브프레임에서 이 값을 쓰는지는 확인하지 않았다(미측정). 쓰지 않는다면 차단 문서의 iframe 스크립트가 실행될 수 있다.
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
