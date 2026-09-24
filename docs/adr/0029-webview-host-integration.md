# ADR-0029: Webview 콘텐츠와 호스트 창의 책임을 나눈다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: plugins, runtime
- **Group**: plugins

## Context

markdown 문서에는 CSS로 조절하는 타이포그래피와 HTML 기반 확장이 필요하다.
이를 egui 문서 렌더러에 계속 추가하는 대신 native webview를 사용한다.
플러그인이 문서를 만들고 호스트는 별도 OS 자식 창의 위치·수명·입력을 관리한다.

세 OS는 같은 동작을 서로 다른 API로 구현한다.
메서드 이름이 같아도 크기 전파나 키 입력, 자원 정리가 실제로 같다는 뜻은 아니다.

## Decision

markdown과 html은 webview 채널을 사용한다. markdown은 정리한 본문 HTML에 신뢰하는 CSS와 스크립트를 추가한다.
주소창도 HTML로 만들고 파일 이동은 navigation 알림으로 호스트에 전달한다.
현재 렌더러에는 Mermaid 블록이 있을 때 번들 스크립트를 넣고 실행하는 코드가 있다.
문서 채널 전환 자체만으로 모든 웹 확장 기능이 제공되는 것은 아니므로 지원 범위는 플러그인 가이드에 기록한다.

백엔드는 공통 호출부가 사용하는 PlatformWebView 메서드를 제공하고 cfg가 하나를 선택한다.
이를 위한 별도 backend trait은 두지 않는다.
반대 방향의 입력은 capture_key·note_focus만 제공하는 WebViewKeySink로 좁혀 주입한다.
NavState는 도메인 모델에 두고 키 정책은 설정 객체 대신 단축키 목록을 받는다.

호스트 단축키 소비 여부는 공통 브리지가 동기 결정하고 실제 액션은 다음 프레임에 실행한다.
페이지 입력과 예약 액션을 보호하며 설정·활성 플러그인 명령에서 조합을 도출한다.
클릭은 모델 포커스를 맞추지만 키 도착만으로 포커스를 옮기지 않는다.
가려진 webview의 포커스 회수도 활성 창과 해당 자식의 실제 포커스를 모두 확인한다.

Linux의 foreign X11 창은 생성·조회 사이 XSync, NULL 오류 처리, 실패 시 정리와 제한된 재시도를 사용한다.
종료는 GDK의 hide·close 처리를 먼저 끝내고 X 창을 파괴한다.
크기 변경은 native container와 실제 렌더 target 모두에 전달한다.
원격 콘텐츠 차단은 navigation과 하위 리소스 요청을 함께 처리한다.

## Consequences

문서 레이아웃과 확장을 웹 렌더러로 처리하면서 호스트의 창·입력 정책을 유지한다.
그 대신 webview 인스턴스별 자원과 세 백엔드의 수명 관리 비용을 부담한다.
컴파일은 호출 가능한 형태를 검사할 뿐 실제 focus·키 반복·렌더 크기·종료 순서를 증명하지 않는다.

Linux 원격 필터 컴파일에는 비동기 공백이 남는다.
일부 단축키는 호스트가 소비한 뒤 현재 focus 조건 때문에 실행되지 않을 수 있다.
플랫폼 실행 검증의 범위와 webview 파일 열기의 사용자 제스처 제한은 가이드에서 따로 표시한다.

## Alternatives Considered

- egui 문서 렌더러 포크는 타이포그래피와 확장 기능을 계속 별도로 유지해야 한다.
- JavaScript로 host 단축키를 중계하면 페이지 신뢰와 비동기 입력 순서 문제가 추가된다.
- backend trait이나 메서드 이름 검사만 추가해도 플랫폼 행동 차이는 검사되지 않는다.
- X 오류를 trap으로만 숨기면 잘못된 종료 순서가 남는다.
- 모든 webview가 쓰는 proxy를 막으면 원격 접근을 허용한 다른 뷰까지 영향을 받는다.

## Reconsideration Triggers

GTK·WebKit·WebView2·AppKit 변경 때 크기·focus·종료·원격 리소스 차단을 실제 플랫폼에서 비교한다.
Wayland나 복수 backend 선택을 지원하면 현재 창 소유와 정적 선택을 다시 정한다.
상류가 안전한 FFI 또는 공식 크기 갱신 방법을 제공하면 직접 호출을 줄인다.
다중 문서의 자원 비용이 문제가 되면 같은 문서를 열어 webview와 대안 렌더러의 비용을 측정한다.

## References

- [Webview 호스트 계약](../design/systems/webview.md)
- [Markdown 문서와 링크](../plugins/markdown/index.md)
- [사용자 파일 열기의 출처](../features/file-handler/index.md)
