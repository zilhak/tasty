# ADR-0568: 엔진이 사용자 제스처로 보고한 plugin webview 의 navigation 에서 온 파일 열기는 사용자 행동이다 — ADR-0526 의 "세 조건이 모두 맞을 때만" 조항 개정

- **Status**: Accepted
- **Date**: 2026-09-23
- **Tags**: file-handler, focus, tab, plugin, webview, markdown, user-agent-separation, identity, identity-principle-1, user-activation, adr-0526, adr-0302

## Context

[ADR-0526](0526-a-plugin-popup-the-user-touched-makes-its-file-dispatch-a-user-action.md) 은 plugin 이
자기 popup 안의 사용자 조작으로 `file_handler.dispatch` 를 부를 때 그 호출을 사용자 행동으로 칠 채널을
만들었다. 근거는 요청 값이 아니라 host 가 popup 입력을 forward 하며 직접 관측한 확정형 입력이고, host 는
"세 조건이 모두 맞을 때만" 사용자로 친다.

그 채널은 webview 를 덮지 않는다. markdown 문서 안의 파일 링크를 사용자가 누르면 WebKit 이 navigation
을 시도하고, host 가 그 시도를 `webview.navigation_attempt` 로 소유 plugin 에 통지하고, plugin 이
`file_handler.dispatch`(`origin_surface_id` 동봉)를 부른다. popup 이 아니니 근거가 없고, 호출은
에이전트로 분류돼 결과 탭이 선택되지 않았다. ADR-0526 은 이것을 "잃은 것" 과 재검토 조건(원리적으로 안
붙는 것 — webview 입력에도 같은 활성화 기록이 필요)으로 남겼고,
[ADR-0302](0302-a-user-file-open-selects-its-result-tab.md) 의 재검토 조건도 같은 조작을 재는 법으로
적었다.

webview 는 OS 자식 창이라 host 가 그 입력을 forward 하지 않는다 — popup 처럼 입력 자리에서 기록을
세울 수 없다. 대신 native 엔진이 navigation 시도마다 **그것이 사용자 제스처에서 났는가** 를 보고한다.
Linux(WebKitGTK)는 `webkit_navigation_action_is_user_gesture`, Windows(WebView2)는
`NavigationStarting` 의 `IsUserInitiated` 다. 두 엔진 문서의 정의로 이 값은 클릭 같은 사용자
제스처가 낸 navigation 에만 참이고, 사람의 제스처 없이 스크립트가 낸 navigation 에는 참이 아니다 —
사람의 입력 없이 스크립트만으로는 이 값을 만들 수 없다(사람의 제스처 **안에서** 스크립트가 낸 것은 참이다
— 아래 "잃은 것"). Linux 에서 사람의 입력 없는 스크립트 쪽은 아래 실측에서 거짓으로 나왔다.

`webview.set_url` 은 소유 plugin 전용이 아니다 — 외부 호출자(에이전트)도 그 surface 에 페이지를 쓸 수
있다(CLI `tasty set url`). 그래서 **그 페이지를 누가 썼는가** 도 근거의 일부가 된다: 에이전트가 쓴 페이지
위에서는 사람이 누른 클릭이라도 무엇을 열지는 에이전트가 정했고, 에이전트가 쓴 스크립트는 plugin 이
되대는 URL 을 제스처 없이 낼 수 있다.

이 경계는 [정체성 원칙](../identity.md) 1 과 맞물린다. plugin 의 자기 신고("이것은 사용자 클릭이다")를
그대로 믿으면 에이전트가 plugin 을 거쳐, 또는 plugin 이 스스로, 사용자 행동을 사칭해 포커스와 선택을
가져간다.

## Decision

**host 는 navigation 시도마다 그 surface 의 기록을 다시 정한다. 시도가 근거가 되면 — native 엔진이
사용자 제스처로 보고했고, 그 surface 의 지금 페이지를 쓴 `webview.set_url` 호출자가 소유 plugin 이다 —
그 시도를 통지한 plugin 에 묶어 기록하고, 근거가 못 되는 시도면 그 surface 의 기록을 지운다. plugin 이
`file_handler.dispatch` 에 그 시도의 URL 을 `user_navigation_url` 로 되대면, host 는 아래 넷이 모두 맞을
때 그 호출 한 번을 사용자 행동(`FileDispatchOrigin::User`)으로 치고 기록을 지운다.**

1. 호출자가 plugin 이다(`CallerContext::Plugin`). 외부 IPC 호출자는 같은 키를 실어도 사용자가 될 수
   없다.
2. 요청이 `origin_surface_id` 를 싣고, 그 surface 에 기록이 있다.
3. 그 기록을 통지받은 plugin 이 호출 plugin 이다.
4. 기록의 URL 이 `user_navigation_url` 과 같다.

기록은 host 가 시도를 plugin 에 통지하는 **같은 자리**(`sync_webviews`)에서 정한다 — plugin 이 시도를
받기 전에 기록이 서 있어야 그 응답 호출이 근거를 찾는다. **사용자 제스처가 아닌 시도는 그 surface 의
기록을 지운다.** 쓰이지 않은 클릭 기록(링크 대상 파일이 없었다 · mirror 문서의 파일 링크 · 입구에서
거절된 dispatch)이 남아 있으면, 뒤에 에이전트가 `webview.set_url` 로 쓴 스크립트가 같은 URL 의 navigation
을 제스처 없이 내고 plugin 이 그것을 되대는 것으로 그 기록이 쓰인다 — 그 순서가 사용자 입력 없이 새
탭을 선택하는 것이 라이브로 재현됐다. 소유 plugin 이 아닌 호출자가 쓴 페이지 위의 시도와 통지할 소유자가
없는 시도도 근거가 못 되므로 같이 지운다. 그래서 기록은 **그 surface 의 가장 최근 시도가 근거일 때만**
있다. webview 가 사라진 surface 의 기록은 같은 자리에서 걷힌다. 한 번 쓰인 기록은 사라지고, 어긋난
주장은 기록을 소비하지 않는다.

페이지를 쓴 호출자는 host 가 `webview.set_url` 을 처리할 때 surface 에 페이지와 함께 적는다(호출자가 그
surface 의 소유 plugin 인가 하나). `webview.set_url` 자체는 에이전트에게 계속 열어 둔다(원칙 2) — 에이전트는
페이지를 쓸 수 있지만, 자기가 쓴 페이지 위의 사용자 클릭을 파일 열기의 사용자 행동으로 바꾸지는 못한다.
소유 plugin 이 페이지를 다시 쓰면 그때부터 그 페이지 위의 클릭은 다시 근거가 된다.

그래서 **ADR-0526 의 "세 조건이 모두 맞을 때만 사용자로 친다" 조항을 개정한다** — 사용자로 치는 근거는
이제 둘이다: 사용자가 만진 자기 popup(ADR-0526 의 세 조건 그대로)과 엔진이 사용자 제스처로 보고한 자기
webview 의 navigation(위 네 조건). 두 근거 모두 **host 가 직접 관측한 입력**이고 요청 값은 그 관측을
가리키는 열쇠일 뿐이다. 어느 것도 안 맞으면 종전대로 조용히 `Agent` 로 떨어진다(debug 로그 한 줄).

macOS(WKWebView)는 공개 API 에 같은 뜻의 값이 없어 시도를 늘 사용자 제스처가 아닌 것으로 싣는다 —
그 플랫폼의 webview 링크 클릭은 종전대로 에이전트로 도착한다. `WKNavigationType::LinkActivated` 는 쓰지
않는다. 스크립트의 `a.click()` 과 사람의 클릭을 가르는지 재지 않았고, 못 가르면 plugin 이 제 스크립트로
근거를 만든다.

markdown plugin 의 문서 안 파일 링크는 통지받은 URL 을 그대로 `user_navigation_url` 로 되댄다. plugin 은
판정하지 않는다.

**개정하지 않는 것**

- ADR-0526 의 나머지: popup 근거의 세 조건과 그 기록 자리(`plugin_bridge::popup_render`) · popup 근거를
  조회만 하고 소비하지 않는 것 · `Intent::NewTab` 의 `activate: origin.is_user()` · 발화 intent 의
  `IntentOrigin` 이 `FileDispatchOrigin` 에서 나오는 것 · ADR-0302 · 0502 · 0503 에 대한 개정.
- ADR-0302: `FileDispatchOrigin` 이라는 값과 그것이 identify 왕복을 건너는 방식 · 명시 origin 갈래가
  `selects_result()` 로 선택을 정하는 것.
- ADR-0503: 에이전트 발화의 적용 실패는 로그, 사용자 발화는 toast 라는 결정. 이 결정으로 사용자로
  도착하게 된 링크 클릭은 그 결정대로 toast 쪽으로 간다.
- `webview.navigation_attempt` 의 wire 모양(`surface_id` · `url`). host→plugin 통지는 바뀌지 않는다.
- `file_handler.dispatch` 의 응답 모양과 오류 코드. 새 키는 선택 사항이고 없으면 종전과 같다.

## Consequences

- **얻은 것**:
  - Linux 에서 사용자가 markdown 문서 안의 파일 링크를 눌러 연 탭이 선택된다(아래 실측). Windows 는
    컴파일만 재어졌다 — 재는 법은 아래 재검토 조건.
  - plugin 은 사람의 클릭 없이는 사용자 분류를 얻을 수 없다 — 엔진이 사람의 제스처로 보고한 시도가 있어야
    하고, 그 시도는 그 plugin 이 쓴 페이지 위에서 나고 그 plugin 이 통지받은 것이어야 하며, 한 번만
    쓰인다.
  - 외부 호출자가 `user_navigation_url` 을 직접 실은 `file_handler.dispatch` 는 에이전트다. 외부 호출자가
    `webview.set_url` 로 그 surface 에 페이지를 써서 **plugin 이 그 키를 싣게** 만드는 경로는 두 규칙이
    막는다 — 그 페이지 위의 시도는 제스처가 있든 없든 기록되지 않고, 그 시도가 앞선 사용자 클릭의 남은
    기록을 지운다.
  - 채널이 generic 이다 — webview surface 를 가진 어느 plugin 이든 같은 키로 쓴다.
- **잃은 것**:
  - macOS 의 webview 링크 클릭은 여전히 에이전트다(결과 탭이 선택되지 않는다).
  - 기록이 surface 마다 한 건이라, 한 프레임 안에 두 링크를 누르면 앞 클릭은 에이전트로 도착한다. 마지막
    클릭의 탭이 선택되므로 사용자가 보는 결과는 같다.
  - 사람의 제스처 **안에서** 페이지 스크립트가 낸 navigation(클릭 핸들러가 부른 `location.hash = …`)은
    엔진이 사용자 제스처로 본다. 그래서 plugin 이 제 페이지 어디든 눌린 클릭을 파일 열기 시도로 바꿀 수
    있다 — 그래도 사람이 그 surface 를 실제로 누른 뒤이고 한 번이다. markdown 주소창의 이동이 이 형태다.
    이 창은 소유 plugin 이 쓴 페이지에만 있다 — 외부 호출자가 쓴 페이지 위의 클릭은 기록되지 않는다.
  - 사용자가 누른 링크의 시도를 plugin 이 파일 열기가 아닌 다른 일에 쓰고 기록을 남겨 두면, 그 surface 에
    다음 시도가 오기 전까지 그 기록으로 한 번 사용자 행동을 얻는다. 그 기록을 쓸 수 있는 것은 통지받은
    plugin 자신뿐이고(외부 호출자가 낸 시도는 기록을 지운다), 사람이 그 surface 에서 실제로 누른 뒤에만
    생기는 창이며 한 번으로 끝난다 — ADR-0526 이 popup 에 수용한 창(열려 있는 동안 몇 번이든)보다 좁다.
  - 사용자 클릭이 기록된 뒤 plugin 의 dispatch 가 돌아오기 **전에** 같은 surface 에서 근거가 못 되는 시도가
    host 에 도착하면(plugin 의 문서 재로드가 부르는 `load_html` 이 `about:blank` 시도를 낸다) 그 정당한
    클릭은 에이전트로 떨어져 새 탭이 선택되지 않는다. 떨어지는 방향은 안전한 쪽이다. Linux 에서는 그
    순서가 나지 않았다(아래 실측 — 재로드를 클릭에 겹친 70 회 중 0 회). plugin 이 재로드를 먼저 처리하느라
    dispatch 가 그 시도의 drain 보다 늦어지면 난다.
  - 페이지를 쓴 호출자는 `webview.set_url` 한 번 단위로 적는다. 에이전트가 쓴 뒤 소유 plugin 이 다시 쓰기
    전까지 그 surface 의 사용자 클릭은 에이전트로 도착한다. 반대로 에이전트 페이지 위의 클릭과 plugin 의
    재작성이 한 프레임 안에 겹치면, 그 클릭은 host 가 시도를 drain 하는 시점의 작성자(소유 plugin)로
    판정된다 — 그 뒤 재작성의 `load_html` 시도가 기록을 지우기 전에 dispatch 가 돌아오면 쓰인다. 이 겹침은
    재지 않았다.
- **운영 비용 / 유지 부담**: native backend 가 새로 생기면 그 엔진의 사용자 제스처 값을
  `PendingNavigation::user_gesture` 에 옮겨야 한다. 옮기지 않으면(늘 거짓) 그 플랫폼은 macOS 와 같이
  에이전트로 떨어질 뿐 사칭 창은 생기지 않는다.

## Alternatives Considered

- **plugin 이 `user_click: true` 같은 선언을 싣는다**: 자기 신고만으로 분류가 바뀐다 — plugin 이
  백그라운드 작업에서 사용자 포커스를 가져가고, 외부 호출자가 plugin 을 거쳐 같은 일을 할 수 있다.
- **plugin 이 보내는 모든 `origin_surface_id` 동봉 호출을 사용자로 친다**: 위와 같다. 근거가 없다.
- **surface 가 최근 사용자 입력(포커스 · 키 입력)을 받았는가로 판정한다**: webview 입력은 OS 자식 창으로
  가 host 가 보지 못한다. host 가 보는 winit 입력은 webview 밖의 것이고, 시간 창을 두면 그 값을 정할
  근거가 없다(ADR-0526 의 같은 기각).
- **host 가 토큰을 발급해 `webview.navigation_attempt` 에 싣는다**: 한 번 쓰는 열쇠라는 점은 같지만
  host↔plugin 프로토콜 구조체가 바뀌어 그것을 링크하는 번들 plugin 전부가 바뀐다. host 가 이미 plugin 에
  건넨 URL 이 같은 역할을 한다 — plugin 이 모르는 값이 아니라 host 가 기록한 값과 맞는지를 보므로 강도가
  같다.
- **macOS 에서 `LinkActivated` 를 사용자 제스처로 친다**: 위 Decision 의 이유로 기각.
- **`webview.set_url` 을 소유 plugin 전용으로 막는다**: 에이전트가 그 surface 에 페이지를 쓸 수 없게 되어
  원칙 2(에이전트 조작 가능성)와 부딪힌다 — CLI `tasty set url` 이 사라진다. 그리고 그것만으로는 plugin
  자신의 비-제스처 시도가 남은 기록을 쓰는 창이 그대로다. 대신 페이지 작성자를 기록하고 비-제스처 시도가
  기록을 지운다.
- **기록에 시간 한도를 둔다**: 창 길이를 정할 근거가 없다(위 "최근 사용자 입력" 과 같은 기각). 시간은 남은
  기록을 줄일 뿐 없애지 못한다 — 한도 안에서는 에이전트의 스크립트가 그대로 쓴다.
- **사용자 제스처가 아닌 시도는 기록을 그대로 둔다(이 결정의 첫 형태)**: 쓰이지 않은 클릭 기록이 남아
  에이전트의 `webview.set_url` 스크립트가 그것을 쓴다 — 사용자 입력 없이 새 탭이 선택된 것이 라이브로
  재현됐다(아래 실측).
- **기록을 소비하지 않고 surface 수명 동안 유효하게 둔다(ADR-0526 의 popup 과 같게)**: popup 은 사용자가
  닫는 경계가 있지만 webview 는 surface 가 사라질 때까지 산다 — 한 번의 클릭이 무기한 근거가 된다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

**채널이 붙는 것** — 판정 시점에 레포가 읽을 수 있는 사실이다.

- release 에 webview 입력을 주입하는 경로가 생기면 엔진의 사용자 제스처 값이 사람의 조작을 뜻하지 않게
  된다. `debug.inject_*` 가 debug 격리 밖으로 나오는지, webview 에 입력을 보내는 release 메서드가
  생겼는지로 잰다(`docs/dev-guide/debug-ipc.md`).
- 파일 열기 말고 다른 host 메서드도 plugin 이 사용자 조작을 중계하게 되면, popup · webview 두 근거를
  메서드마다 되풀이하지 말고 공용 판정으로 올린다(ADR-0526 의 같은 조건). 지금 이 판정을 부르는 자리는
  `adapters::ipc::handler::file_handler::dispatch_origin_of` 하나다.
- macOS 백엔드가 `PendingNavigation::user_gesture` 를 참으로 싣는 자리가 생기면, 그 값이 스크립트 클릭과
  사람의 클릭을 가른다는 실측이 함께 있는지 본다.
- `webview.set_url` 이 소유 plugin 전용이 되면(`tasty-ipc` 의 `method_meta.rs` 등급) 페이지 작성자 기록은
  늘 참이라 필요 없어진다. 반대로 surface 에 페이지를 쓰는 다른 메서드가 생기면 그것도 작성자를 적어야
  한다 — `RemoteSurface::set_webview_url` 을 부르는 자리를 센다.

**원리적으로 안 붙는 것** — 사람이 관측해야 한다. 재는 법을 함께 적는다.

- 엔진의 사용자 제스처 보고가 스크립트 navigation 에도 참을 내는 판(WebKitGTK · WebView2 버전)이
  나오면 근거가 무너진다. 재는 법: 페이지 스크립트로 `location.hash` 를 바꾸거나 `a.click()` 을 불러
  `#tasty-nav:link:` 시도를 내고, 새 탭이 선택되지 않는지 `tab.list` 로 본다.
- Windows 는 컴파일만 재어졌다(`cargo check --target x86_64-pc-windows-gnu`). fragment 만 바뀌는
  navigation 에 `NavigationStarting` 이 서는지, `IsUserInitiated` 가 사람의 클릭에 참을 내는지 재지
  않았다 — 안 서면 markdown 링크 클릭 자체가 plugin 에 안 가고, 이 채널도 쓰이지 않는다. 재는 법:
  Windows 에서 markdown 문서 안의 파일 링크를 누르고 새 탭이 생기는지·선택되는지 `tab.list` 로 본다.
  재어지면 사용자 가이드(`site/content/using/files.md`)에 Windows 를 적는다. `IsUserInitiated` 호출이
  실패하면 그 시도는 제스처 아님으로 통지된다(경고 로그 한 줄) — 통지 자체는 버리지 않는다.

## References

- 개정 대상: [ADR-0526](0526-a-plugin-popup-the-user-touched-makes-its-file-dispatch-a-user-action.md) ("세 조건이 모두 맞을 때만 사용자로 친다" 조항 — 근거를 popup · webview 둘로 넓힌다)
- 개정 패턴 선례: [ADR-0030](0030-image-egui-mesh-bitmap-texture.md)
- 선행 결정: [ADR-0302](0302-a-user-file-open-selects-its-result-tab.md) (재검토 조건 "원리적으로 안 붙는 것" 의 markdown 링크 클릭에 답한다)
- 탐색: `git grep -l 'owner_popup_instance\|FileDispatchOrigin\|navigation_attempt\|user_gesture' -- docs/adr/`
- [identity.md](../identity.md) 원칙 1 · [Focus policy](../design/policies/focus.md) · [webview 백엔드 계약](../design/systems/webview.md) · [File handler](../features/file-handler/index.md) · [markdown plugin](../plugins/markdown/index.md)
- 실측(Linux, 2026-09-23, 격리 홈 · 전용 Xvfb · xdotool 포인터 클릭): 에이전트 CLI 로 연 markdown 문서를
  탭바 클릭으로 보이게 한 뒤 문서 안의 파일 링크를 누르면 새 탭이 **선택됐다**(`list tree` 의 `*`) —
  독립 인스턴스 둘에서 재현. 같은 조작을 `user_navigation_url` 을 싣지 않도록 바꾼 plugin 빌드로 하면
  새 탭이 생기되 선택되지 않았다. 세 인스턴스 중 둘에서는 링크 첫 클릭이 navigation 을 안 냈고 두 번째
  클릭부터 났다(원인은 재지 않았다 — WM 없는 Xvfb 의 포커스 전달로 짐작한다). Windows · macOS 는 미측정.
- 현재 구현(심볼): `plugin_bridge::user_navigation`(`record` · `take`) ·
  `AppState::webview_user_navigations` · `IpcWindow::take_webview_user_navigation` ·
  `adapters::ipc::handler::file_handler::dispatch_origin_of` · `host_api::webview::PendingNavigation` ·
  `adapters::ipc::handler::webview::notify_navigation_attempt`(통지한 plugin 과 그 plugin 이 페이지를
  썼는가를 돌려준다) · `adapters::ipc::handler::webview::handle_set_url`(호출자를 적는다) ·
  `RemoteSurface::webview_page_by_owner` · `MainView::sync_webviews` · markdown plugin 의
  `file_link_params`. 입구 시험은 `adapters/ipc/handler/file_handler_origin_tests.rs` 의 webview 절, 기록
  규칙 시험은 `plugin_bridge/user_navigation.rs`, 작성자 기록 시험은 `adapters/ipc/handler/webview.rs` 의
  `set_url_remembers_whether_the_owning_plugin_wrote_the_page`. native 백엔드가 제스처 값을 옮기는 자리와 `sync_webviews` 의 기록
  배선에는 시험 채널이 없다 — 위 실측이 그 자리를 잰 유일한 측정이다.
