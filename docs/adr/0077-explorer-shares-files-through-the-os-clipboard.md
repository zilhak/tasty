# ADR-0077: 탐색기의 복사·잘라내기는 OS 파일 클립보드와 공유하고 나중에 한 복사가 이긴다

- **Status**: Accepted — Windows·macOS 구현은 실 기기에서 확인하지 않았다.
- **Date**: 2026-10-11
- **Tags**: explorer, files, clipboard, platform, linux, wayland
- **Group**: plugins

## Context

탐색기의 복사·잘라내기는 창마다 하나인 내부 목록(`MainViewState::explorer_clipboard`)에만 담겼다. 다른 파일 관리자에서 복사한 파일을 탐색기에 붙여넣거나, 탐색기에서 복사한 파일을 다른 파일 관리자·다른 Tasty 창에 붙여넣을 수 없었다. 다른 앱이 클립보드를 바꿔도 오래된 내부 잘라내기 목록이 그대로 남았다.

조건:

- Windows·macOS·Linux 에서 각 OS 파일 관리자의 형식을 따라야 한다. 잘라내기는 OS 마다 표지가 다르고 macOS Finder 에는 없다.
- 원격 mirror 탐색기의 경로는 같은 문자열의 로컬 파일로 읽히면 안 된다([ADR-0022](0022-remote-mirror-content-and-queries.md)).
- 호스트가 쓰는 `arboard` 3.6 은 파일 목록을 읽고 쓰지만 Linux 에서는 `text/uri-list` 만 다루고, 다른 형식을 더하는 API 가 없다.
- 호스트 텍스트 클립보드는 `arboard` 의 X11 경로라, Wayland 세션에서는 XWayland 를 거친다.

## Decision

탐색기의 로컬 경로 복사·잘라내기는 창의 목록과 함께 OS 파일 클립보드에도 올리고, 붙여넣기는 OS 파일 클립보드의 파일도 받는다. 원격 mirror 에서 복사한 경로는 OS 클립보드에 올리지 않는다.

창의 목록과 OS 클립보드가 다르면 나중에 한 복사가 이긴다. 목록을 정한 직후의 OS 클립보드와 붙여넣을 때의 OS 클립보드가 다르면 OS 쪽이 나중이므로, 그것이 파일이면 그 목록을 쓰고 파일이 아니면 창의 목록을 버린다. 다른 프로그램이 클립보드를 가졌는데 내용을 주지 않을 때도 그쪽이 나중이므로 붙여넣지 않는다. OS 클립보드에 아예 접근하지 못할 때만 창의 목록을 쓴다. 로컬 파일이 아닌 항목이 하나라도 섞인 OS 목록은 OS 와 관계없이 전체를 붙여넣지 않는다.

Linux 는 `arboard` 대신 `x11rb` 로 X11 `CLIPBOARD` 선택을 직접 소유하고 읽어 GNOME·KDE 잘라내기 표지까지 다룬다. Wayland 세션은 텍스트 클립보드와 같이 XWayland 를 거치며, Wayland 네이티브 클립보드는 텍스트와 파일을 함께 후속으로 다룬다. Windows 는 `CF_HDROP` 과 `Preferred DropEffect`, macOS 는 파일 URL 을 쓴다.

형식 목록과 우선순위·소비·수명의 현재 명세는 [OS 파일 클립보드](../surfaces/explorer/file-operations.md#os-파일-클립보드)에 있다. OS 파일 클립보드 연동과 Wayland 를 XWayland 경유로 두는 범위는 사용자가 정했다(2026-10-10).

## Consequences

- 다른 파일 관리자·다른 Tasty 창과 파일을 주고받는다. 창 사이 공유를 위한 별도 프로세스 내부 저장소가 필요 없다.
- 탐색기에서 파일을 복사하면 OS 클립보드의 이전 글자가 사라진다. Linux 에서는 경로 글자도 함께 내주므로 터미널에 붙여넣으면 경로가 들어간다.
- 붙여넣기와 탐색기 메뉴를 열 때 UI 스레드에서 OS 클립보드를 읽는다. Linux 는 읽기 한 번 전체를 짧은 한도 안에 끝내고, 넘기면 응답하지 않는 다른 소유자가 나중에 복사한 것으로 보아 붙여넣지 않는다.
- 나중 복사 판정은 내용 비교라, 원격 출처처럼 OS 에 올리지 않은 목록 뒤에 다른 앱이 같은 비-파일 내용을 다시 복사하면 바뀐 것을 알 수 없다.
- Linux 에서 올린 목록은 Tasty 가 끝나면 사라진다. X11 선택 소유 코드(요청 응답·INCR 읽기)를 직접 유지한다.
- Windows·macOS 코드는 Linux 에서 Windows 대상 컴파일·lint 만 확인했고 macOS 는 컴파일도 확인하지 못했다.

## Alternatives Considered

- **창 사이 내부 공유만**: 같은 Tasty 의 창끼리는 되지만 다른 파일 관리자와는 주고받지 못한다. 사용자가 OS 파일 관리자와의 양방향을 정했다.
- **창의 목록을 늘 우선**: OS 클립보드가 바뀌어도 오래된 잘라내기 목록이 붙여넣어진다. 사용자가 다른 앱에서 새로 복사한 것을 무시하게 된다.
- **OS 클립보드를 늘 우선하고 창의 목록을 버림**: 원격 출처 목록처럼 OS 에 올리지 않은 목록과, OS 클립보드에 접근하지 못하는 환경에서 붙여넣기가 사라진다.
- **응답하지 않는 소유자를 접근 실패와 같게 보고 창의 목록을 씀**: 다른 앱이 나중에 복사했는데도 오래된 잘라내기 목록이 붙여넣어진다.
- **Linux 도 `arboard` 파일 목록**: 의존 코드가 줄지만 `x-special/gnome-copied-files` 를 쓰지도 읽지도 못한다. GNOME Files 는 이 형식이 있어야 붙여넣고, 잘라내기를 구별할 수 없다.
- **`arboard` 의 `wayland-data-control`**: Wayland 네이티브 클립보드를 쓰지만 GNOME Mutter 가 지원하지 않는 data-control 프로토콜이 필요하고 새 의존성이 붙는다. 텍스트 클립보드와 경로가 갈라진다.

## Reconsideration Triggers

코드와 설정에서 확인:

- Wayland 네이티브 클립보드를 후속으로 다룰 때 텍스트(`src/clipboard/context.rs`)와 파일(`src/clipboard/files/x11.rs`)을 함께 옮긴다. GNOME 을 포함한 주요 컴포지터가 data-control 계열 프로토콜을 지원하게 되면 이 후속을 앞당긴다.
- `arboard` 가 사용자 정의 형식을 쓰고 읽게 되면 Linux 의 직접 선택 소유를 걷어낼지 검토한다.

실행 결과로 확인:

- Windows·macOS 실 기기에서 Explorer·Finder 와 주고받지 못하는 사례가 나오면 그 OS 형식 처리를 고친다.
- XWayland 가 파일 형식을 Wayland 앱에 넘기지 않는 컴포지터가 보고되면 Wayland 네이티브 후속을 앞당긴다.
- 클립보드 읽기가 UI 를 멈추게 하는 사례가 보이면 읽기를 비동기로 옮기거나 메뉴를 열 때의 읽기를 없앤다.

## References

- [OS 파일 클립보드](../surfaces/explorer/file-operations.md#os-파일-클립보드) — 형식·우선순위·소비·수명.
- [ADR-0022](0022-remote-mirror-content-and-queries.md) — mirror 탐색기 경로의 출처.
- `src/clipboard/files.rs`, `src/clipboard/files/format.rs`, `src/clipboard/files/x11.rs`, `src/clipboard/files/windows.rs`, `src/clipboard/files/macos.rs`, `src/state/explorer_os_clipboard.rs`.
