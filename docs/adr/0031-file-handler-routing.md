# ADR-0031: 파일 열기는 대상과 사용자 조작 여부를 끝까지 보존한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: plugins, runtime
- **Group**: plugins

## Context

파일 식별과 핸들러 선택은 비동기로 끝날 수 있다.
완료할 때 현재 포커스를 다시 보면 요청한 창과 다른 곳에 탭이 만들어진다.
반면 origin surface가 있다는 이유만으로 결과 탭을 선택하지 않으면 Explorer의 사용자 더블클릭도 제대로 반응하지 않는다.

경로와 URL, 사용자 클릭과 자동화, 설정을 읽은 순서도 각기 다른 판단이다.
이들을 문자열이나 현재 화면 상태 하나로 추측하면 기능 경계가 흐려진다.

## Decision

origin surface는 요청 대상이며 FileDispatchOrigin은 사용자 조작 여부를 나타낸다. 두 값을 식별 worker와 picker 완료까지 전달한다.
명시한 surface의 소유 engine과 pane에서만 실행하고 대상이 사라지면 취소한다.
사용자 파일 열기는 결과 탭을 선택하며 Agent 요청은 기존 선택을 유지한다.
접수 응답은 완료를 뜻하지 않고 이후 실패는 기존 비동기 보고로 알린다.

IPC는 기본적으로 Agent 요청이다. 플러그인이 사용자 조작을 중계했다고 선언하는 것만으로 분류를 바꾸지 않는다.
호스트가 확인한 자기 popup의 확정 입력, 또는 소유 플러그인이 작성한 webview 페이지의 native 사용자 navigation 기록을 근거로 한다.
webview 기록은 같은 origin·소유자·URL에 한 번만 쓰며 부적격 navigation이 오면 이전 기록을 지운다.
근거가 부족하면 요청을 Agent로 처리한다.

파일과 HTTP URL은 DispatchTarget에서 구별한다.
URL은 파일 detector를 거치지 않고 사용자의 연결 동작 picker로 보낸다.
System과 url 파라미터를 선언한 OpenSurface만 URL을 받으며 파일 전용 IPC payload에는 넣지 않는다.
공개 file_handler.dispatch는 파일 경로만 받는다.

handler와 detector는 Host→Plugin→User 순서로 병합한다.
등록 시점과 무관하게 user patch를 마지막에 적용하고 reload는 적용하지 않은 항목의 사유를 알린다.

host를 무기한 막는 native fs.pick_file은 제공하지 않는다.
file_picker.trigger는 Plugin caller만 접수해 request_id를 즉답하고 결과를 그 plugin에 보낸다.
headless file dispatch는 실행할 수 없으므로 빌드 미지원 오류로 거절한다.

## Consequences

비동기 작업 중 포커스가 바뀌어도 대상은 유지되고 사용자 행동과 Agent 요청의 선택 효과가 구별된다.
원래 대상이 사라지면 대체 탭을 만들지 않으므로 호출자는 경고나 후속 상태로 결과를 확인해야 한다.

popup 입력 근거는 열린 수명 동안 재사용할 수 있다. webview는 최신 한 건만 보존해 정당한 클릭도 재로드와 겹치면 Agent로 처리될 수 있다.
macOS의 webview 클릭은 같은 native gesture 판정이 없어 Agent로 남는다.
페이지 작성자가 바뀐 뒤 늦게 도착한 navigation까지 완전히 구별하는 것은 아직 보장하지 않는다.

reload의 항목별 보고는 파일 읽기·파싱 전체 실패를 충분히 설명하지 못한다.
URL handler의 url 파라미터 이름은 현재의 제한된 선언 규약이다.

## Alternatives Considered

- 완료 시 focused pane을 사용하면 원래 요청 대상을 잃는다.
- 모든 plugin 요청을 User로 분류하면 백그라운드 작업이 사용자 선택을 바꾼다.
- origin:user 자기 신고는 호스트가 확인한 입력 근거를 대신할 수 없다.
- URL을 PathBuf로 보내면 확장자 식별·file URI 생성·IPC path 해석이 틀린다.
- 설정을 plugin 뒤에 읽는 방식만으로는 나중에 재기동하는 plugin patch를 보존하지 못한다.

## Reconsideration Triggers

surface ID 재사용, 새로운 비동기 완료 프로토콜, 파일 이외의 사용자 조작 중계가 생기면 보존할 대상과 근거를 다시 정한다.
플랫폼별 링크 클릭과 script navigation을 비교해 gesture 판정이 유지되는지 확인한다.
URL을 처리하는 IPC handler나 다른 파라미터 이름이 필요하면 권한과 선언 규약부터 확장한다.
headless에서 파일을 열려면 창 없는 실행 대상과 선택 정책을 먼저 정한다.

## References

- [파일 대상·병합·비동기 완료](../features/file-handler/index.md)
- [파일 피커](../features/native-file-picker/index.md)
- [Webview 입력과 페이지 작성자](../design/systems/webview.md)
- [Surface kind 입력 팝업](../dev-guide/plugin-development.md)
