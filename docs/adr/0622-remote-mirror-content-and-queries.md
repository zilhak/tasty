# ADR-0622: 원격 화면은 서버 상태를 확인한 뒤 표시한다

- **Status**: Accepted
- **Date**: 2026-09-24
- **Tags**: attach, mirror, content
- **Group**: terminal

## Context

원격 mirror는 실제 PTY와 파일을 소유하지 않는다. 화면 크기나 파일 내용을 로컬에서 임의로 확정하면 서버와 화면이 달라진다.

## Decision

mirror 크기는 client pane에서 요청하고 서버가 실제 PTY를 resize한 뒤 보낸 Resize로 확정한다. client는 미리 grid를 바꾸지 않는다. 서버의 일반 창 resize는 hard 점유 surface를 건너뛴다. resize 규칙은 Core::resize_all_terminals 한 곳에서 처리한다.

파일 피커는 로컬·원격을 같은 UI로 제공하되 원격 디렉토리는 attach의 request_id 기반 요청·응답으로 읽는다. 원격 조회 권한은 해당 client의 workspace 점유로 판단한다. local host UI의 디렉토리 조회에 plugin 권한을 요구하지 않는다.

파일 바이트는 같은 SSH 터널의 별도 bulk 연결에서 raw binary Data 프레임으로 보낸다. 시작·완료·경로 회신은 작은 JSON Control 메시지를 사용한다. 대화형 attach 소켓과 분리해 큰 파일이 키 입력을 지연시키지 않게 한다. 저장 경로는 서버가 정하고 업로드 시점과 경로 삽입은 소비자가 결정한다.

원격 Git 조회는 tasty-git-core의 데이터 타입과 조회 로직을 host와 plugin이 공유한다. git_viewer.query는 request_id를 먼저 반환하고 응답은 해당 plugin에 event로 전달한 뒤 repaint를 요청한다. 서버는 surface ID로 실제 원격 cwd를 찾는다. 원격 경로를 로컬 파일 경로로 해석하지 않는다.

원격 explorer는 파일 피커의 list_dir 요청을 재사용한다. 디렉토리 탐색만 지원하고 파일 변경·더블클릭 내용 열기는 제공하지 않는다. 소비자 view가 경로별 pending 요청과 cache를 소유한다. 파일 피커의 단일 요청 상태를 여러 explorer가 공유하거나 host에 중복 registry를 만들지 않는다.

markdown mirror는 HTML·픽셀 대신 원문 문자열을 전달하고 client plugin이 자신의 테마로 렌더한다. handshake에는 전용 markdown role과 표시용 원격 경로만 넣고 원문은 surface ID로 따로 요청한다. 모델은 후보를 모으고 host가 kind/plugin 허용 목록을 검증한다. html은 임의 URL일 수 있어 같은 원문 조회에 포함하지 않는다.

원문은 server host가 직접 읽는다. 콘텐츠 조회는 이 engine의 workspace를 하나라도 점유한 client를 허용하며, 변경 신호도 같은 client 집합에 보낸다. 입력·resize·구조 변경의 특정 workspace holder 검증과는 범위가 다르다. 원문 응답의 실패는 ok/reason 하나로 표현하고 파일 없는 문서는 빈 성공으로 반환한다.

JSON으로 직렬화한 원문은 700KiB 예산에서 UTF-8 문자 경계를 지켜 자른다. 원문 바이트 수만 재지 않는다. 잘린 문서는 toast로 알리고 본문을 수정하지 않는다. plugin 대용량 렌더 확인은 읽기 권한이 아니므로 host는 그 확인 대기 중인 파일도 전송 예산 안에서 읽을 수 있다.

서버 markdown의 webview.set_url을 변경 신호로 사용하며 client는 stale 표시만 하고 사용자가 새로고침할 때 읽는다. 테마 변경처럼 원문이 같아도 신호가 올 수 있다. 상대경로 자산은 base_dir=None으로 두어 client 로컬 파일을 잘못 읽지 않게 하고 주소창 탐색은 비활성화한다.

surface cwd는 서버가 1Hz로 확인해 바뀐 값만 holder에 push한다. diff cache는 holder와 값을 함께 기억해 새 holder가 초기값을 받는다. 알 수 없어진 cwd는 null로 보내 client cache를 지운다.

CoreState::surface_cwd는 Local(PathBuf)과 Remote(RemoteCwd)를 구분한다. RemoteCwd는 Path로 자동 변환하지 않는다. mirror의 push·OSC 7·explorer root에서 온 값은 모두 Remote다. 로컬 PTY 생성·preset 저장·로컬 Git 조회는 Local만 사용한다. plugin JSON도 remote_cwd와 mirror:true로 구분한다.

inherit_cwd는 실행할 때만 적용한다. cwd push 자체를 끄지 않으며 원격 구조 변경은 client의 캐시를 그대로 보내지 않고 서버가 현재 값을 찾는다. 명시 cwd만 요청에 담는다.

markdown의 abandon(request_id=0)은 대기 요청 유무와 관계없이 연결 끊김을 뜻한다. 끊긴 문서는 옛 원문 대신 끊김 안내를 표시한다. 재연결 직후 survivor 문서마다 기존 changed 이벤트를 한 번 보낸다. 원문을 표시 중인 문서는 stale만 표시하고, 끊김·오류·미로딩 문서는 pending 요청이 없을 때 다시 요청한다. 성공 응답이 끊김 상태를 해제한다.

client에 markdown kind가 아직 없으면 DeferredPlugin placeholder로 기다리고 등록 뒤 remote 복원 데이터를 사용해 생성한다. 같은 kind를 다른 plugin이 이미 등록했다면 잘못된 plugin으로 복원하지 않고 빈 surface로 남긴다.

## Consequences

크기 변경은 왕복 지연이 있고 처음 attach할 때 화면이 바뀔 수 있다. 중복 크기 요청은 client와 server에서 억제한다. 파일 피커의 원격 디렉터리 조회는 8초 응답 제한과 mirror workspace 소멸 감지로 연결 오류를 표시한다.

native bulk는 SSH 프로필이 없는 수동·loopback attach에서도 동작하지만 청크 순서·완료·무결성·회수 처리를 직접 유지해야 한다. bulk 연결은 점유한 attach 세션과 연결되어야 한다.

list_dir는 점유한 client에 임의 경로 조회를 허용하며 explorer root를 보안 경계로 강제하지 않는다. 같은 client가 파일 피커로 조회할 수 있으므로 explorer만 좁혀도 접근 제한 효과가 없다. 파일시스템 내용은 필요할 때 조회하고 workspace 구조 delta에 넣지 않는다.

Git 응답은 크기를 제한하며 잘림 여부를 전달하지만 plugin은 현재 별도 잘림 안내를 표시하지 않는다. 세션이 끊기면 진행 중 요청을 취소한다. 연결이 살아 있으면서 응답만 없는 경우의 Git soft timeout은 아직 없다.

handshake 직후에는 plugin snapshot이 아직 없어 파일 경로가 비어 있을 수 있다. 경로는 힌트이며 내용 조회 응답을 기준으로 표시한다. 문자열 이스케이프가 많으면 원문 700KiB보다 훨씬 전에 잘리고 코드펜스가 중간에서 끊길 수도 있다. host와 plugin 사이 markdown_mirror 메서드·이벤트는 attach wire와 끝점·권한이 달라 별도 이름을 사용한다. plugin 요청은 FsRead를 요구하고 응답은 로컬 surface ID로 변환한다.

cwd는 terminal만의 값이 아니므로 kind 전환과 모든 mirror 정리에서 cache를 제거한다. 구 server가 push하지 않아도 서버측 cwd 해소는 유지한다. 원격 cwd로 로컬 디스크를 검색하던 status bar 값은 표시하지 않는다. macOS cwd 조회의 실측 비용은 기존 자료에서 확인되지 않았다.

끊긴 동안은 예전 원문도 볼 수 없다. anchor 없는 세션은 workspace 자체를 정리하므로 이 끊김 문서 상태는 재연결을 기다리는 세션에서만 보인다. host는 plugin의 pending ID를 대신 관리하지 않고 changed만 전달한다.

## Alternatives Considered

로컬 grid를 먼저 바꾸면 서버의 이전 크기 출력이 잘못 줄바꿈된다. 서버 창 크기로 고정하면 client pane에 맞지 않는다. 배타 holder가 하나이므로 여러 client의 geometry 합의는 현재 필요 없다. 원격 파일 선택을 동기 host-call에 넣으면 이벤트 루프를 막으므로 비동기 요청을 사용한다.

sftp/scp는 외부 subsystem과 별도 SSH 정보가 필요하고 수동 attach를 그대로 지원하지 못한다. SMB/NFS는 추가 서버·인증·포트가 필요하다. 이런 프로토콜의 별도 기능을 금지하는 결정은 아니다. Control+base64로 일반 파일을 보내면 전송량과 대화형 소켓 대기가 늘어난다.

Git live handle을 직렬화할 수 없으므로 plain data를 공유한다. popup.set_context에는 임의 결과 payload가 없어 비동기 조회 응답을 대신할 수 없다. 최초 snapshot만 원격으로 읽으면 refresh·worktree 전환·diff가 동작하지 않는다. 파일 내용 fetch는 목록 조회와 달리 임시파일 수명·크기·MIME 정책이 필요하다.

서버 HTML은 크기가 크고 client 테마와 recent 상태를 반영하지 않는다. markdown을 mesh로 되돌리면 현재 WebView 렌더 구조를 다시 바꿔야 한다. 임시 로컬 파일로 받으면 원격 경로·감시·recent를 잘못 해석하고 파일 수명 정책도 필요하다. 자동 원문 갱신은 사용자의 읽던 위치를 바꿀 수 있다.

mirror Terminal의 cached_cwd에 원격 경로를 넣으면 출처를 잃고 비terminal에는 저장할 수 없다. PathBuf와 bool을 나누면 bool을 무시한 사용이 가능하다. 소비자별 질문이나 OSC 7만 사용하면 같은 비동기 조회를 반복하거나 OSC 7 없는 셸을 놓친다.

## Reconsideration Triggers

고지연 환경에서 resize 반응이 문제가 되거나 여러 holder를 허용하면 geometry 협상을 다시 정한다. 초기 크기 협상과 원격 파일 내용 fetch 요구, 점유별 읽기·쓰기 권한 분리가 생겨도 해당 프로토콜을 검토한다.

비-Tasty 호스트나 매우 큰 파일에서 native 전송이 불리하면 다른 프로토콜을 비교한다. bulk 인가·수명이 attach와 어긋나면 결속 방식을 검토한다. Git soft timeout·잘림 안내·동시 popup 격리 요구가 생기면 요청 상태를 보강한다. list_dir 소비자가 늘거나 파일 변경·내용 열기 요구가 생기면 라우팅과 권한을 함께 검토한다.

다른 kind에 원문 전달을 확장하거나 markdown이 mesh 후보로도 들어오면 역할 분류를 검토한다. 원격 상대 자산·편집 요구는 별도 전송·쓰기 정책이 필요하다. 예산에 자주 걸리면 프레임 상한만 높이지 말고 청크 전달을 검토한다.

RemoteCwd의 로컬 Path 변환, 서버측 cwd fallback 제거, 1Hz 조회 비용 증가가 생기면 타입·호환·주기를 다시 검토한다.

재연결이 survivor plugin을 재사용하지 않게 되거나 abandon 표현·서버 재연결 유예가 바뀌면 재요청 신호의 필요성을 검토한다. 연결이 끊긴 동안 예전 문서를 읽어야 한다는 요구가 반복되면 끊김 화면 정책을 재검토한다.

## References

- [attach 구현](../dev-guide/attach-behavior.md)
- [원격 attach](../features/remote-attach/index.md)
