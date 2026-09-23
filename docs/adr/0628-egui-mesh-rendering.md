# ADR-0628: 플러그인이 그린 mesh를 호스트가 합성한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: plugins, runtime
- **Group**: plugins

## Context

플러그인의 화면 내용을 호스트가 직접 그리면 모델과 편집 기능도 호스트에 남는다.
고정 위젯 DSL을 늘리는 방식은 rich text와 다양한 UI를 따라 구현해야 하고,
픽셀 프레임만 보내면 글자 선명도와 확대 처리에 불리하다.

별도 프로세스에서 그리는 UI에는 전송 비용도 있다.
같은 프로세스라면 작은 애니메이션 한 프레임도 여기서는 입력 전달, tessellation, 인코딩과 GPU 갱신을 반복한다.

## Decision

egui-mesh 플러그인은 자기 Context에서 레이아웃·입력 처리·tessellation을 끝낸다.
호스트는 받은 mesh와 texture delta를 surface·popup·banner 영역에 합성한다.
폰트와 콘텐츠 상태는 플러그인이 소유한다. 예전 Host 직접 렌더와 UiNode DSL 경로는 유지하지 않는다.

image 비트맵도 egui texture로 보낸다. 최초와 변경 때 픽셀을 보내고 pan·zoom에서는 캐시된 텍스처를 참조한다.
별도 Canvas 아래에 mesh를 겹치는 합성 경로를 만들지 않는다.
markdown 본문은 문서 표현 요구 때문에 webview를 사용한다.

공유 버퍼에서 프레임을 놓치면 geometry 채택과 texture delta 적용을 따로 판단한다.
기존 텍스처로 안전하게 그릴 수 있는 새 geometry는 사용하되 delta를 적용하지 못했다면 last_seq를 전진시키지 않는다.
전체 텍스처를 받을 때까지 다시 요청하고 범위를 넘는 delta는 적용하지 않는다.

목록은 일정한 행 높이를 전제로 보이는 행만 layout하고 안정적인 행 ID를 쓴다.
가로 스크롤 폭은 전체 내용을 한 번 측정해 캐시한다.
wheel delta는 이동량을 보존한 채 같은 pass에 처리하도록 나누고 불필요한 scroll animation을 끈다.
남은 self-repaint 요청은 프로세스당 타이머 스레드 하나가 처리한다.

IME 캐럿 위치는 실제 위젯을 그린 플러그인이 frame 알림에 포함한다.
호스트가 그 좌표를 창 좌표로 변환해 OS 후보창 위치를 정한다.
입력은 실제 사용자 입력만 전달하며 에이전트의 사용자 입력 재현 API로 쓰지 않는다.

## Consequences

UI 상태와 표현은 플러그인 안에 남고 호스트 합성 경로를 재사용한다.
이미지의 최초 업로드와 편집 texture 갱신, 매 프레임 인코딩 비용은 남는다.
texture 복구 전에는 글리프가 잠깐 어긋날 수 있으며 무응답 플러그인에는 full 요청이 반복될 수 있다.

wire는 epaint 구성과 결합돼 있어 현재 번들 허용 목록과 api_version으로 제한한다.
외부 플러그인 개방은 보류다. IME 위치는 frame dedup에 영향을 받고 attach mesh mirror에는 아직 전달되지 않는다.
banner는 키·IME 입력을 받지 않아 이 위치 알림 대상이 아니다.

## Alternatives Considered

- in-process dylib는 크래시 격리를 잃고 Rust ABI까지 맞춰야 한다.
- UiNode 확장은 egui 기능을 호스트 DSL로 다시 구현하게 한다.
- Canvas 하이브리드는 surface 합성·좌표·입력 경로를 하나 더 만든다.
- 외부 GPU 메모리 공유는 플랫폼별 구현 부담이 커 현재 채택하지 않는다.
- texture epoch를 새로 전달하는 방식보다 기존 full 재전송 복구가 현재 변경 범위에 맞았다.

## Reconsideration Triggers

epaint 변경이나 외부 플러그인 개방 때 codec과 버전 협상을 다시 검토한다.
큰 이미지 업로드·full 재요청·목록 layout이 실제 지연을 만들면 해당 비용을 따로 측정한다.
행 높이가 달라지면 가상화 방식을 바꾸고, IME 위치만 바뀌는 화면에서 문제가 생기면 독립 알림을 검토한다.
attach 헤더가 확장 가능해지면 원격 IME 위치를 추가할 수 있다.

## References

- [egui-mesh 채널 규칙](../dev-guide/egui-mesh-channel.md)
- [이미지 데이터와 편집](../plugins/image/index.md)
- [git-viewer 목록과 조회](../plugins/git-viewer/screens/git-viewer.md)
- [Webview 통합](0629-webview-host-integration.md)
