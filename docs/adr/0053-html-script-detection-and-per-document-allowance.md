# ADR-0053: HTML 문서의 스크립트는 원본 파일에서 감지하고 사용자만 문서 단위로 허용한다

- **Status**: Accepted — 결정만 확정했고 구현은 아직 없다. macOS의 navigation별 차단은 실제 Mac에서 확인하지 않았다
- **Date**: 2026-09-29
- **Tags**: plugins, webview, html, javascript, sandbox, banner, ipc, security
- **Group**: plugins

## Context

html surface의 JavaScript는 plugin 설정 "Sandbox scripts"를 따르며 기본값은 꺼짐이다(`src/view/main/redraw.rs`의 `resolve_webview_settings`).
스크립트가 있는 문서를 열면 사용자는 결과가 왜 비어 있는지 알 수 없다. 한 문서만 실행하려고 전역 설정을 끄면 이후 연 모든 문서에도 적용된다.
그래서 스크립트가 있는 문서에 배너를 띄우고, 사용자가 그 문서만 허용하는 기능이 필요하다.
이 기능을 만들려면 다음 네 가지를 정해야 한다.

- 무엇을 실행 가능한 스크립트로 셀지 정한다.
- 어디서 감지할지 정한다. JS가 꺼진 상태에서 DOM을 조사할 수 있는지는 OS마다 다르다.
- 원격 문서와 markdown을 범위에 넣을지 정한다.
- 허용이 새 문서로 넘어가지 않도록 언제 해제할지 정한다.

에이전트는 CLI로 html surface를 열 수 있다. 에이전트가 허용 명령까지 쓸 수 있다면, 자기가 연 문서의 JS를 사용자 확인 없이 켤 수 있다.

## Decision

### 감지 대상

로컬 문서에서 다음 셋 중 하나라도 있으면 실행 가능한 스크립트가 있다고 판단한다.

- `<script>` 요소. inline이든 `src`든 같다. 단 `type`이 JavaScript MIME 형식이나 `module`이 아니면 제외한다. `application/json`, `text/template` 같은 데이터 블록이 이 예외에 해당한다.
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

### 범위

- html surface가 연 `file://` 문서만 대상으로 한다.
- 원격 `http(s)` 문서에는 배너를 띄우지 않고 기존 원격 차단 설정을 따른다. 감지하려면 호스트가 문서를 따로 가져와야 하고, 원격 콘텐츠는 기본적으로 차단되어 있기 때문이다.
- markdown surface는 제외한다. 사용자 HTML은 ammonia 허용 목록으로 정리되고(`crates/tasty-plugin-markdown/src/render.rs`), 렌더러가 넣는 스크립트는 신뢰 대상이다([ADR-0029](0029-webview-host-integration.md)).
- 전역 "Sandbox scripts"를 꺼서 이미 JS가 실행되는 상태라면 배너를 띄우지 않는다.

### 허용의 수명

- 허용은 surface와 현재 문서의 URL에 묶인다. URL은 fragment를 뺀 값으로 비교한다.
- 허용은 세션 안에서만 유지한다. 레이아웃에 저장하지 않으므로 재시작하면 다시 차단 상태가 된다.
- 허용하면 JS를 켜고 같은 문서를 다시 로드한다.
- 다른 문서로 이동하면, 새 문서의 스크립트가 실행되기 전에 navigation 결정 콜백에서 JS를 다시 끈다. redraw에서 사후에 되돌리지 않는다.
  - **Linux**: `decide-policy`의 navigation 결정을 진행하기 전에 `set_enable_javascript(false)`를 호출한다.
    - 측정 결과 `load_uri`와 링크 클릭 두 경우 모두 새 문서의 첫 로드에서 스크립트가 실행되지 않았다.
    - fragment만 바뀌어도 `decide-policy`가 호출된다. 그래서 fragment를 뺀 URL로 같은 문서인지 판정한다.
    - 허용 직후의 재로드는 URL이 같다. 호스트가 표시해 둔 재로드로 구분한다.
  - **Windows**: `NavigationStarting` 핸들러에서 `IsScriptEnabled`를 정한다. API 문서의 예제도 이 핸들러에서 해당 navigation에 적용되도록 설정을 바꾼다. `NavigationStarting` 이후에 바꾸면 다음 top-level navigation부터 적용된다.
  - **macOS**: 현재의 전역 `javaScriptEnabled`(deprecated) 대신 `webView:decidePolicyForNavigationAction:preferences:decisionHandler:`에서 navigation별 `WKWebpagePreferences.allowsContentJavaScript`를 정한다.

### 에이전트 경로

- 감지 결과와 허용 상태의 조회는 release IPC와 CLI로 제공한다(원칙 2). 상태를 읽을 뿐 사용자 화면이나 입력을 바꾸지 않는다.
- IPC·CLI로 스크립트를 허용하는 명령은 debug 빌드에서만 제공한다.
  - 핸들러는 모듈 선언에 `#[cfg(debug_assertions)]`가 붙은 별도 파일에 둔다([debug IPC 가이드](../dev-guide/debug-ipc.md)).
- release에서는 사용자가 GUI 배너나 탭 마커로만 허용한다.
- 이유는 보안이다. 에이전트는 문서를 열 수 있다. release 명령으로 허용까지 할 수 있으면, 에이전트가 연 문서의 JS를 에이전트 스스로 켜는 경로가 생긴다. 사용자의 클릭을 거치게 해야 이 경로가 사라진다.

배너는 surface가 소유하는 배너다. 키보드 포커스를 가져가지 않는다는 규칙은 [ADR-0036](0036-overlay-scope-and-lifetime.md)을 따른다. 위치·문안·마커는 디자인 결정을 따른다.

## Consequences

- 사용자는 전역 설정을 바꾸지 않고 한 문서만 실행할 수 있다. 허용은 다른 문서로 넘어가지 않는다.
- 세 OS가 같은 판정을 쓰므로 조회 결과도 OS와 관계없이 같다.
- 정적 스캔은 실제 실행 여부와 다를 수 있다.
  - 조건부 주석이나 문자열 안의 태그 모양 때문에 오탐이 생길 수 있다.
  - `<iframe>`이 불러오는 다른 로컬 파일의 스크립트는 놓칠 수 있다.
  - 오탐이 생겨도 배너가 뜨는 것뿐이고 JS가 켜지지는 않는다.
- 파일을 읽는 동안 문서가 바뀌면 감지 결과가 오래된 상태일 수 있다. 결과를 해당 URL에 묶어 두고, 문서가 바뀌면 버린다.
- 해제 시점을 navigation 콜백에 두므로 세 백엔드의 콜백 코드를 수정해야 한다.
  - macOS는 delegate 시그니처가 바뀌고, 실제 Mac에서 확인하기 전까지 미검증이다.
- 에이전트는 release에서 스크립트 문서를 자동으로 실행할 수 없다. 자동화에는 debug 빌드나 전역 설정이 필요하다.

## Alternatives Considered

- **로드 후 DOM 조사**: Linux에서는 JS가 꺼져 있으면 호출이 동작하지 않는다(측정). Windows에서만 가능하다.
- **`enable-javascript-markup` 끄기 후 조사**: 파싱 단계에서 찾을 대상이 제거된다(측정).
- **감지 전용으로 JS를 잠깐 켜기**: 조사하는 동안 문서의 스크립트가 실행될 수 있다. 차단하려는 동작이 그대로 일어난다.
- **원격 문서를 따로 가져와 스캔**: 원격 차단 설정을 우회하는 추가 요청이 생긴다. 이 요청과 webview의 요청이 서로 다른 응답을 받을 수 있다.
- **release IPC·CLI 허용 명령**: 에이전트가 자기가 연 문서의 JS를 사용자 확인 없이 켤 수 있다.
- **redraw에서 사후 해제**: 새 문서의 첫 스크립트가 해제 전에 실행될 수 있다.
- **허용을 레이아웃에 저장**: 재시작 후 사용자가 다시 확인하지 않은 문서가 실행된다. 같은 경로에 다른 내용이 놓일 수도 있다.

## Reconsideration Triggers

코드와 설정에서 확인한다.

- 원격 문서의 원격 콘텐츠 기본값이 허용으로 바뀌면 원격 문서도 범위에 넣을지 다시 정한다.
- markdown의 ammonia 허용 목록에 스크립트 관련 요소나 속성이 추가되면 markdown 범위를 다시 정한다.
- WebKitGTK가 JS 비활성 상태에서도 호스트 전용 스크립트 실행 경로를 제공하면 DOM 조사를 다시 비교한다.
  - 예를 들어 script world 기준의 실행 경로가 해당된다.

실행 결과로 확인한다.

- macOS에서 `allowsContentJavaScript`를 설정한 navigation의 첫 로드에 스크립트가 실행되면 해제 시점을 다시 정한다.
  - 확인 방법은 Linux 측정과 같다. 스크립트가 제목을 바꾸는 두 문서를 만들고, 링크로 이동한 뒤 새 문서의 제목을 읽는다.
- WebView2에서 `NavigationStarting` 안의 설정이 같은 navigation에 적용되지 않으면 해제 시점을 다시 정한다.
- 사용자가 오탐이나 미탐을 보고하면 스캔 규칙과 실제 실행 여부를 같은 문서로 비교한다.

## References

- [ADR-0029](0029-webview-host-integration.md): webview 채널, 원격 콘텐츠 차단.
- [ADR-0036](0036-overlay-scope-and-lifetime.md): 배너의 입력과 수명.
- [debug IPC 가이드](../dev-guide/debug-ipc.md): debug 전용 핸들러의 격리 조건.
- [Webview 호스트 계약](../design/systems/webview.md).
- `src/view/main/redraw.rs`의 `resolve_webview_settings`: 현재 JS 설정 결정.
- 현재 navigation 콜백:
  - `src/host_api/webview/linux.rs`의 `connect_decide_policy`
  - `src/host_api/webview/macos.rs`의 navigation delegate
  - `src/host_api/webview/windows.rs`의 `add_NavigationStarting`
- 플랫폼 문서:
  - WebKitGTK `WebKitSettings:enable-javascript`, `enable-javascript-markup`, `webkit_web_view_evaluate_javascript`
  - WebView2 `ICoreWebView2Settings::IsScriptEnabled`
  - Apple `WKWebpagePreferences.allowsContentJavaScript`
