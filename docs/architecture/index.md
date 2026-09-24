# 아키텍처 개요

Tasty는 본 바이너리(`src/`)와 60개 크레이트(`crates/*`)로 구성된 Cargo workspace다. 도메인 로직은 GUI 없이 동작하고, GUI·IPC·OS 연동은 port와 adapter로 연결한다.

## 기술 스택

| 역할 | 라이브러리 |
|------|-----------|
| 윈도우/입력 | winit |
| GPU 렌더링 | wgpu |
| UI 위젯 | egui + egui-wgpu + egui-winit |
| VTE 파싱 | termwiz |
| PTY | portable-pty (Windows ConPTY / Unix) |
| 폰트 래스터라이징 | cosmic-text + swash |
| IPC | TCP (127.0.0.1, 동적 포트, `~/.tasty/tasty.port`) + JSON-RPC 2.0 (serde_json) |
| CLI | clap |
| 설정 | toml + directories |
| OS 알림 | notify-rust |
| 공유 메모리 | `tasty-shm`(자체) — POSIX shm + SCM_RIGHTS / Windows DuplicateHandle |

## headless 분리 — `gui` feature

본 바이너리는 `gui` feature(`default = ["gui"]`)로 GUI 표면을 켠다. 끄면 **도메인(`core`) + 외부통신(`hub`) 만 빌드**되고 View/GPU(winit·wgpu·egui)는 컴파일에서 빠진다. 이것이 headless 원칙([identity](../identity.md))의 컴파일 차원 강제다 — 에이전트가 GUI 없이 IPC 로 tasty 를 구동할 수 있다.

`App`(winit `ApplicationHandler` 본체)은 세 부분을 합성한다:

| 필드 | 역할 | gui gate |
|------|------|----------|
| `core: Core` | **도메인 본체** — 워크스페이스/탭/페인/서피스 상태, 세션, attach, registries | 항상 |
| `hub: Hub` | **외부 통신** — IPC 서버(`Option<Box<dyn IpcServerPort>>`), 포트 파일 | 항상 |
| `view: ViewRegistry` | **GUI 어댑터** — winit proxy, `views: HashMap<WindowId, Box<dyn View>>`, `active_modal_id`/`focused_view_id` | `#[cfg(feature = "gui")]` |

현재 본체는 `Core`가 상태, `Hub`가 외부 통신, `ViewRegistry`가 GUI를 담당한다. surface_registry·command_index·output_observer·layout_persistence는 `src/core/`에 있다.

### 도메인 경계 — `core` + `ports`

도메인은 src/core와 src/ports에 두고 같은 크레이트 내 모듈 경계로 관리한다.
도메인은 app·adapter·GUI 구현을 직접 참조하지 않는다.
공용 타입은 도메인에 정의하고 창 연산은 도메인이 선언한 `CascadeWindow`·`IdentifySpawner` 등의 trait으로 받는다.
GUI 작업과 응답 전송 worker는 상위 모듈에 둔다.
별도 core 크레이트는 많은 GUI 조건과 형제 모듈 결합을 이동하고 pub(crate)를 공개해야 하는 비용 때문에 현재 채택하지 않는다.
경계 검사는 상위 참조·GUI 조건 개수·직접 GUI 의존을 보며 전이 의존과 상위에서 도메인 내부에 접근하는 것까지 막지는 못한다.
외부 도메인 소비자가 생기거나 분리 빌드의 실측 이득이 커지면 크레이트 분리를 재검토한다.

경계 검사는 기존 gui 조건 뒤의 GUI import도 확인한다.
optional GUI 의존 목록은 manifest에서 읽고 Windows는 창·그리기·창 핸들 경로와 이를 별칭으로 들일 수 있는 상위 import를 검사한다.
비GUI Windows 항목은 직접 경로로 사용할 수 있다.
도메인·IPC port 구현 파일도 에이전트의 활성 상태 읽기 검사에 포함한다.
webhook·hook 실행부는 inbound adapter 대신 HostIpcInjector를 사용하고 별도 검사로 방향을 확인한다.
판정 코드는 공유하지만 도메인과 자동화의 허용 의존은 구분한다.
전이 의존, 다른 workspace crate 내부 feature, 새 port 구현 파일 누락은 여전히 별도 확인 대상이다.
GUI 허용 경로 목록에 항목이 생기면 같은 경로의 추가 사용도 셀 수 있도록 검사 범위를 재검토한다.

구조 실행과 IPC 핸들러의 구체적인 port 사용은 [AppState 소유권](../dev-guide/app-state-ownership.md)을 따른다.

### 크레이트를 나누는 기준

크레이트를 나눌 때는 의존 방향을 먼저 확인한다. 재사용하는 코드가 작더라도 기존 크레이트에 불필요한 의존성을 들여오면 분리한다. `tasty-remote`가 SSH와 IPC를 함께 사용하되 `tasty-ssh`는 IPC를 모르도록 한 것이 이 기준의 예다. 줄 수는 빌드 성능을 개선할 후보를 찾는 참고값이며, 분리를 금지하는 기준은 아니다. 소비자가 하나로 줄거나 크레이트 관리·링크 비용이 분리 이득보다 커지면 다시 검토한다.

파일 형식·핸들러 레지스트리의 plugin port 구현은 타입을 소유한 크레이트에 둔다. wire 계약인 trait을 도메인으로 옮기면 protocol이 도메인 구현을 의존하게 된다. Rust 고아 규칙 때문에 제3의 adapter에 두기도 어렵다. `tasty-file-format → tasty-plugin-protocol`을 명시적 계층 예외로 관리하고, 예외 목록과 아키텍처 설명을 함께 고친다. protocol 사용이 port 구현을 넘어 커지면 경계를 재검토한다.

파일 핸들러 정책과 레지스트리는 `tasty-file-handler`에, 실제 파일 실행은 호스트에 둔다. 번들 기본값은 `HOST_DEFAULTS_TOML`로 한 번 포함하고 소비자는 상수를 사용한다. `tasty-file-handler → tasty-plugin-protocol`도 같은 port 예외다. protocol을 낮은 계층으로 옮기면 의존 예외는 줄지만 plugin sandbox 경계 설명이 흐려지므로 현재 소속을 유지한다. 도메인-IO 예외가 넷째로 늘거나 새로운 예외가 기존 이유로 설명되지 않으면 trait 배치를 다시 검토한다.

선택 계산은 `tasty-selection`에 두고 GUI와 헤드리스 모두 `tasty-cell-width`의 Unicode 폭을 사용한다. GUI가 없다는 이유로 비ASCII 문자를 전부 2칸으로 세지 않는다. 크레이트 이동 때 이전 feature 조건을 복사하지 말고 그 조건이 막던 의존성이 아직 필요한지 확인한다. 문자 폭 계산이 GUI 의존을 갖게 되거나 헤드리스 소비자에서 폭 차이가 관측되면 계산 공유 범위를 재검토한다.

링크 검출과 결과 타입은 `tasty-terminal-link`에서 GUI 없이 계산하고, 브라우저를 여는 `open_uri`는 UI adapter에 둔다. 타입 복제나 무의미한 gui feature 대신 계산과 부수효과를 분리한다. GUI 결합은 파일 내부의 cfg나 직접 의존만으로 판단하지 않는다. 상위 모듈 선언과 전이 의존성까지 확인한다. 링크 크레이트에 OS·프로세스 부수효과가 들어가거나 adapter의 역할이 커지면 분리 기준을 다시 확인한다.

루트 패키지는 라이브러리와 얇은 실행 파일로 나눈다. 모듈 트리는 `src/lib.rs`가 소유하고 실행 파일은 시작 함수와 실행 파일 전용 설정을 둔다. 이 분리는 외부 소비자가 사용할 타깃을 만들지만 도메인 API 공개를 자동으로 허용하지 않는다. 기존 pub 재수출도 실제 공개 API가 되므로 소스 선언을 함께 점검한다. 루트 단위 테스트는 lib 타깃에 있으며 bin만 검사하면 0건일 수 있다. 실행 파일 테스트를 라이브러리 테스트로 바꾸면 검증 범위도 달라진다.

공용 타입 크레이트의 기본 feature는 GUI 의존을 켜지 않는다. egui 변환이 필요한 소비자가 `egui-compat`을 명시하고 루트의 GUI 전용 크레이트와 아이콘 feature는 `gui`에서만 켠다. 모든 소비자에게 default-features 비활성화를 요구하면 새 소비자가 놓치기 쉬워 기본값 자체를 비운다. 빌드 성공과 GUI 의존 제거는 별도로 확인하며, feature를 변경할 때 전이 의존 그래프를 비교한다.

플랫폼 코드는 OS 호출과 신호 수신을 담당한다. 신호를 `AppEvent`나 사용자 동작으로 해석하는 코드는 호출자가 전달한 callback에 둔다. OS를 호출하지 않는 앱 상태 진단은 platform 밖에 둔다. 단순 callback 알림에 별도 채널을 추가하면 버퍼와 수명 관리만 늘어나므로 사용하지 않는다. 플랫폼 동작을 바꿀 때는 해당 OS의 컴파일과 실제 resume·dock·종료 동작을 따로 확인한다.

`tasty-font`는 GPU device를 요구하는 atlas 모듈만 기본 비활성 `gpu` feature 뒤에 둔다. 폰트 설정, metrics, 패킹·LRU·프레임 시계는 device 없이 사용·테스트할 수 있다. 헤드리스의 폰트 이름 해석은 `FontConfig`를 사용하므로 cosmic-text는 남는다. 폰트 해석 수단이 바뀌면 이 의존도 검토한다. 재수출을 지워서는 의존 그래프가 바뀌지 않으므로 manifest의 feature와 전이 의존을 확인한다.

셀 렌더러는 선택·링크·폭·폰트·모델 타입을 소유한 크레이트를 직접 참조한다. 호스트의 재수출 경로에 기대면 실제 소속과 feature 경계를 알아보기 어렵다. 호스트 접착 코드인 gpu 모듈은 앱 상태 참조를 유지하고, 공용 셀 색 계산은 본체 모듈을 함께 사용할 수 있다. 텍스트 검색은 별칭·상대 경로 의존을 놓칠 수 있어 의존 경계의 완전한 증명으로 사용하지 않는다.

OS 호출은 `tasty-platform` 크레이트에 둬 본체 타입에 직접 의존하지 못하게 한다. 본체의 별칭은 기존 호출 경로를 유지할 수 있지만 크레이트 자체는 공용 하위 라이브러리만 사용한다. 창·메뉴·트레이 기능은 gui feature 뒤에, crash report는 그 밖에 둔다. OS 경계는 egui 위젯 계층과 구분한다. 공개 API를 넓힐 때 실제 외부 호출자를 확인하고 테스트용 구현은 private로 둔다. 교차 타깃 check는 링크와 OS callback 실행을 검증하지 않는다.

`StreamHub`는 bounded sink, 프레임 분류, 손실 집계를 제공하는 공용 전송 계약이므로 `tasty-ipc`에 둔다. TCP 소켓 accept와 읽기·쓰기는 production adapter에 둔다. core는 허브를 직접 참조하고 adapter 재수출을 거치지 않는다. 파일 조립까지 확인하는 테스트는 도메인 쪽에, 분류 순서 테스트는 IPC 크레이트에 둔다. 다른 전송 수단이 추가돼 허브 자체 동작이 달라져야 한다면 sink 추상을 검토한다.

셀 렌더링에서 반복 호출하는 tasty-cell-width, tasty-terminal-link, tasty-selection은 dev에서도 opt-level 3으로 빌드한다. workspace 멤버는 외부 의존용 별표 설정에 포함되지 않아 개별 등록이 필요하다. 선택 기준은 실행 시간 감소와 수정 후 재컴파일 시간 증가를 각각 비교하는 것이다. 폭 계산·링크 검출의 반복 비용 감소가 작은 추가 컴파일 비용보다 커 채택했다. 새 크레이트를 무조건 같은 수준으로 최적화하지는 않는다. 호출이 캐시되거나 해당 크레이트 수정 빈도가 늘면 번갈아 빌드·실행해 다시 비교한다.

## 워크스페이스 크레이트 (60)

아래 목록은 낮은 계층부터 나열한다. 의존은 상위에서 하위로 향하며 순환을 허용하지 않는다. `architecture_layer_order_holds`가 매니페스트 의존과 순서를 대조한다. 크레이트 소속은 각 절 첫 문단에서 백틱 이름으로 시작하는 항목을 읽는다. 순서와 다른 의존을 발견하면 실제 의존과 문서의 계층 순서를 함께 확인한다.

계층 예외는 도메인-IO의 세 의존이다: `tasty-remote → tasty-ipc`, `tasty-file-format → tasty-plugin-protocol`, `tasty-file-handler → tasty-plugin-protocol`. 이유는 해당 절에서 설명한다.

`architecture_crate_list_complete`가 `crates/*/`의 전체 이름과 개수를 비교한다. `doc-guards.yml`이 경로 필터 없이 main push·PR에서 두 검사를 실행하며, 크레이트를 추가할 때는 push 전에 직접 확인한다([CI 가이드](../dev-guide/ci-gates.md)).

### type-\* / primitive (leaf)
`tasty-type-geometry`(길이·도형 타입: LogicalPx·PhysicalPx·Rect, 의존 없음) · `tasty-type-appearance`(색·테마 스키마·ToastKind. 그리기와 이벤트 처리에서 같은 타입 사용. egui 변환은 egui-compat, → type-geometry) · `tasty-design-tokens`(DTCG 디자인 토큰 사본·코드 생성, → type-geometry) · `tasty-utils`(경로 등 공용 함수) · `tasty-cell-width`(렌더러·선택·링크가 공유하는 코드포인트별 셀 폭, 의존 없음) · `tasty-ansi`(CSI·OSC 제거 정규식. terminal의 strip-ansi와 output 파서가 공유, → regex, ADR-0001) · `tasty-timer`(주기 작업을 키로 등록하고 drain_due로 실행. 다음 기한까지 기다리는 waker 스레드 1개, → utils) · `tasty-shm`(공유 메모리와 FD/HANDLE 전달. POSIX shm·SCM_RIGHTS·Windows DuplicateHandle, workspace 및 외부 의존 없음)

이 절 안에서만 의존 가능("type-\*" 는 절 이름이고 규칙의 단위는 **절 소속**이다 — `tasty-utils`·`tasty-ansi`·`tasty-timer`·`tasty-design-tokens` 처럼 이름이 `tasty-type-` 으로 시작하지 않는 것도 이 절이다). 도메인/IO crate 의존 금지(그룹 내 순환도 금지). — [typed-length](../concepts/typed-length.md)

`tasty-shm`은 Tasty의 도메인 상태를 모르고 공유 메모리 전송만 담당하며 workspace 의존도 없다. syscall 사용 여부가 아니라 역할과 의존 방향으로 이 계층에 둔다. SDK가 shm을 사용하는 것을 도메인 의존 예외로 만들 필요도 없다.

### 도메인-IO
`tasty-themes`(전역 Theme·TOML IO) · `tasty-settings`(설정 스키마·직렬화) · `tasty-font`(글리프 atlas) · `tasty-terminal`(PTY·termwiz) · `tasty-hooks`(Surface Hook) · `tasty-memory`(memory.db) · `tasty-telemetry`(사용량·진단, → memory) · `tasty-output`(출력 파서) · `tasty-approval`(승인) · `tasty-agent`(세션·수명, → memory) · `tasty-presets`(레이아웃 프리셋) · `tasty-portscan`(포트 조회) · `tasty-reaper`(Windows Job Object로 자식 수명 관리. 다른 OS에서는 동작 없음) · `tasty-lua`(격리 워커·고정 host API, ADR-0027) · `tasty-i18n`(번역) · `tasty-remote-profiles`(연결 프로필·passkey. attach·explorer·plugin이 공유, ADR-0020) · `tasty-ssh`(시스템 ssh 실행·터널·포트 탐색·재시도·취소. SSH 프로토콜은 구현하지 않음, → remote-profiles/i18n/utils) · `tasty-remote`(원격 workspace 조회·생성. CLI·GUI·IPC 공용, → ssh/ipc/remote-profiles, ADR-0001) · `tasty-model`(workspace·pane·tab·surface 모델, GUI 의존 없음, → terminal/type-appearance/type-geometry/utils) · `tasty-dag-layout`(Sugiyama 방식의 DAG 좌표 계산. 본체·갤러리 공용, egui·Theme 의존 없음, → type-geometry. [설명](../dev-guide/dag-layout.md)) · `tasty-git-core`(git2 읽기 전용 래퍼. repo·status·log·diff·worktrees, 원격 host와 git-viewer 공용, → utils, ADR-0022) · `tasty-file-format`(detector·규칙·Lua·구조 평가. 호스트·GUI·IPC 구현 의존 없음, → utils/plugin-protocol) · `tasty-file-handler`(detector와 handler 연결·사용자 설정·plugin 등록·최근 선택. 호스트·GUI·IPC 구현 의존 없음, → utils/file-format/plugin-protocol) · `tasty-selection`(격자 선택·픽셀 변환·적중 판정·텍스트 추출. 렌더러·View 공용, → terminal/type-geometry/cell-width) · `tasty-terminal-link`(URL·파일 경로·여러 줄 링크와 강조 범위 계산. 실제 열기는 호스트 GUI에서 처리, → terminal/type-appearance/cell-width)

이 절 + type-\*/primitive 절만 의존 가능(위와 같이 판정 단위는 절 소속이다). **예외 셋** — 첫째, `tasty-remote` 는 plugin host 의 `tasty-ipc` 에 의존한다: 원격 client 능력이 IPC 호출이고, 합칠 후보 둘(`tasty-ssh` 와 `tasty-ipc`)이 각각 더 나쁜 의존을 들여 기각됐다.
그 방향은 [ADR-0001](../adr/0001-crate-dependency-boundaries.md) 의 결정이다.
둘째, `tasty-file-format` 은 plugin 경계의 `tasty-plugin-protocol` 에 의존한다: plugin 이 형식 레지스트리를 조회하는 port trait 이 그 wire 크레이트에 살고, trait 소유 크레이트가 구체 레지스트리에 역의존하지 않도록 impl은 타입 소유 쪽에 둔다.
의존은 trait 정의 한 개뿐이고 역방향 호출은 없다.
그 방향은 [ADR-0001](../adr/0001-crate-dependency-boundaries.md) 의 결정이다.
셋째, `tasty-file-handler` 도 같은 이유로 `tasty-plugin-protocol` 에 의존한다: plugin 이 handler 레지스트리를 조회하는 port trait 이 그 wire 크레이트에 있고, 같은 역의존을 피하려고 impl을 타입 소유자 쪽에 둔다.
둘째와 같은 형태이고 같은 결정([ADR-0001](../adr/0001-crate-dependency-boundaries.md))의 적용이다.
이 절의 다른 크레이트에는 예외가 없다.

### UI primitive
`tasty-egui-theme`(Theme를 egui Visuals/Style로 변환) · `tasty-ui-widgets`(본체·갤러리 공용 egui 위젯·배치 함수. [설명](ui-widgets-crate.md)) · `tasty-icons`(line/fill SVG. 본체·갤러리와 plugin 빌드가 공유) · `tasty-key-match`(바인딩과 키 이벤트 대조. 단축키·webview 공용, egui 입력은 egui-input feature, → settings/winit)

### OS 경계
`tasty-platform`(panic·hang 진단, 호스트 로그, 네이티브 메뉴, 트레이, jump list, 전원 재개, 창 크롬, OS 화면 캡처, macOS 권한·Dock·메뉴바, 이벤트 루프 watchdog. App 해석은 호출자가 callback으로 전달. 창·GTK·AppKit·트레이는 gui feature 뒤이며 headless에는 crash 진단이 포함됨, → i18n/settings/utils, [ADR-0001](../adr/0001-crate-dependency-boundaries.md))

`tasty-platform`은 본 바이너리가 사용하고 도메인-IO와 primitive 계층에만 의존한다. 네이티브 메뉴·트레이는 egui 위젯이 아니므로 본체·갤러리 공용 UI 계층과 구분한다.

### plugin protocol / SDK (sandbox 경계)
`tasty-plugin-protocol`(호스트·plugin 전송 타입, → type-appearance) · `tasty-plugin-sdk`(외부 plugin SDK, → protocol/shm/utils/i18n) · `tasty-plugin-sdk-wasm`(WASM 타깃 SDK) · `tasty-plugin-agent-common`(Claude·Codex 공용 prompt 임시파일·훅 정리·children 응답·reboot 인자. 매니페스트가 없어 번들 plugin은 아님, → sdk)

이 계층은 protocol·SDK를 통해 호스트와 통신하며 도메인-IO에 직접 의존하지 않는다. 이 경계가 OS 샌드박스를 뜻하지는 않는다. **예외 하나** — `tasty-plugin-sdk` → `tasty-i18n`: 호스트와 SDK 가 설치·사용자 plugin 카탈로그 로딩을 공유한다.

### plugin host (IPC 인프라)
`tasty-plugin-manifest`(매니페스트 스키마·파서) · `tasty-ipc`(JSON-RPC 메시지·caller·audit·method_meta·port trait·클라이언트 연결·HostIpcInjector·StreamHub. TCP 소켓 처리는 본체 tcp_ipc_server adapter, [ADR-0001](../adr/0001-crate-dependency-boundaries.md)) · `tasty-host-plugin`(호스트의 plugin manager·process·event bus·registry)

### 번들 plugin (bin 크레이트, 모두 `tasty-plugin-sdk` 의존)
`tasty-plugin-claude`(lib 도 함께 노출) · `tasty-plugin-codex` · `tasty-plugin-git-viewer` · `tasty-plugin-clipboard-viewer` · `tasty-plugin-image` · `tasty-plugin-html` · `tasty-plugin-markdown` · `tasty-plugin-agent-stream` · `tasty-plugin-mesh-demo`(+ manifest). 뒤의 둘은 `bundle = false` 라 배포 패키징에서는 빠지고 dev 번들 sync 로만 붙는다. — [concepts/plugins](../concepts/plugins.md)

### 도구 / standalone
`tasty-tui-simulator`(E2E TUI 시뮬레이터, lib + `tasty-tui-sim` binary — 로직은 lib 공유, debug 빌드에선 `tasty debug sim` 으로도 노출) · `tasty-gallery`(ui-widgets 데모 바이너리, `cargo run -p tasty-gallery` — 본체 빌드와 분리)

### CLI client
`tasty-cli`(clap CLI — request/format/transport/dynamic plugin subcommand. → ipc/host-plugin/terminal/approval/remote-profiles/remote/ssh/i18n/plugin-manifest/plugin-protocol/tui-simulator/utils)

### 테스트·가드 전용 (제품 산출물 밖)
`tasty-latency-control`(CPU 작업 또는 자식 프로세스 시작으로 지연의 대조군 제공. 실패 메시지는 사용한 종류를 표시. 의존 없음, dev-dependencies로만 사용, [ADR-0046](../adr/0046-verification-evidence-and-diagnostics.md)) · `tasty-doc-guards`(문서·소스·workflow를 대조. 의존 없이 빠르게 실행해 문서만 바뀐 main push도 검사, [ADR-0048](../adr/0048-source-guards-and-exemptions.md)) · `tasty-test-support`(테스트 동안 전역 env·TASTY_HOME을 바꾸고 Drop에서 복원하는 RAII 가드, → utils/tempfile. dev-dependencies로만 사용)

### 본 바이너리 (`tasty`)
위 크레이트를 의존하며 App/View/GPU/IPC 라우터/부팅을 제공.

## 본 바이너리 모듈 (`src/`)

ports-and-adapters 배치:

| 모듈 | 역할 |
|------|------|
| `boot/` | `fn main` 부팅 시퀀스(`run()` 진입점) — event_loop, headless_{dispatch,stream,plugins}, cli_routing, wiring, locale, trace(부팅 계측) |
| `app/` | `App`(winit `ApplicationHandler`) — window_lifecycle, boot_machine(첫 윈도우 부팅 상태 머신 — [boot-sequence](boot-sequence.md)), shutdown_cascade(종료 cascade — [shutdown-sequence](shutdown-sequence.md)), modal, ipc dispatch, attach, dispatch_domain(workspace close cascade — [close-sequence](close-sequence.md)) |
| `core/` | **도메인 본체**(`Core`) — state, session, attach, agent, terminal_store, ipc_facade, 구조 실행·cascade, 도메인이 선언한 창 포트(`cascade_window` · `identify_port`). 위 "도메인 경계" 절 |
| `hub.rs` | **외부 통신**(`Hub`) — IPC 서버, 포트 파일 |
| `view/` | **GUI**(gui-gated) — `View` sealed trait 계층 + MainView/SettingsView/QuitView/PluginsView/PresetView. — [multi-window](multi-window.md) |
| `state/` | `AppState` — MainView 당 1개 런타임 상태(focus/layout/mouse/mark/restore). 도메인 포트 `CascadeWindow` 의 구현(`cascade_window.rs`) |
| `gfx/` | GPU — `GpuState`, renderer(셀 렌더), screenshot, perf. — [gpu-rendering](../dev-guide/gpu-rendering.md) |
| `adapters/` | 외부 경계 구현 — `ui`(egui 컴포넌트·popup), `ipc`(handler), `production`/`test`(port 구현체), `cli`, `plugin` |
| `ports/` | **의존성 역전 trait** — ipc_server, clipboard, clock, fs, home, process, notification_sound (production/test adapter 가 구현 → headless·테스트 교체). 도메인의 일부다 |
| `intent/` | **Intent 큐** — 호스트 내부 동작 디스패치. — [action-dispatch](../design/flows/action-dispatch.md) |
| `host_api/` | 호스트가 외부(plugin/agent)에 제공하는 인터페이스 — hooks, webview |
| `plugin_bridge/` | 호스트 측 plugin 라우팅 facade |
| `store/` | 인메모리 스토어 — notification, state.db 수명의 창 간 공유 recent_files |
| `db/` | SQLite `state.db`. — [storage](../design/systems/storage.md) |
| `file/` · `clipboard/` | 파일 핸들러/디스패치(형식 식별 자체는 `tasty-file-format`) · 클립보드. OS 경계(crash_report·native menu 등)는 `src/` 를 떠나 `tasty-platform` 크레이트에 있고, 본체는 `crate::platform::…` 별칭으로 부른다 |

## 데이터 흐름

주요 흐름 5종(키 입력→렌더, PTY 출력→파싱→렌더, IPC 요청→응답, 알림, 설정 로드→적용)의 단계별 경로는 [data-flows](data-flows.md). 호스트 내부 동작이 Intent 큐로 통일된 디스패치 모델은 [action-dispatch](../design/flows/action-dispatch.md).

## 하위 문서

| 문서 | 설명 |
|------|------|
| [boot-sequence](boot-sequence.md) | 첫 윈도우 부팅 상태 머신(BootPhase) — hidden 생성→로딩 프레임→표시, 프레임 구동 대기, 부팅 계측(T1~T7) |
| [shutdown-sequence](shutdown-sequence.md) | 종료 확정 시 native webview 숨김 + cascade(layout flush→surface close→plugin 종료) + `event_loop.exit()` 이후 Drop tail, 종료 계측(S1~S5) |
| [close-sequence](close-sequence.md) | 워크스페이스 close 경로 3종(gui/inline/cascade) · 자원 회수의 소유(gui/headless 차이 표) · close 계측(C1~C5) 과 실측 기준선 |
| [multi-window](multi-window.md) | App = Core/Hub/ViewRegistry, View trait 계층, 모달 불변식, 단일 프로세스 근거 |
| [input-layer](input-layer.md) | 마우스 입력 z-order 계층 — 소비/버블링 + 커서 결정 |
| [data-flows](data-flows.md) | 주요 데이터 흐름 (파일+함수 기준) |
| [ipc-server](ipc-server.md) | IPC 서버가 요청을 받아들이고 처리하는 쪽의 규칙 — 입장 상한 · dispatch 회차 예산 · 기한 · wake · 요청 압력 게이지 |
| [ui-widgets-crate](ui-widgets-crate.md) | `tasty-ui-widgets` — 본체·갤러리 공유 UI primitive |
| [Invariants](#invariants) | 깨지면 안 되는 시스템 조건 (surface-cwd 등) — 아래 절 |

## Invariants

*깨지면 안 되는 시스템 조건* — 코드 변경 시 가장 먼저 점검할 리스트. 각 invariant 는 가능하면 컴파일/CI 로 강제하고, 그게 불가능하면 review 로 지킨다.

| Invariant | 적용 시점 | 강제 기제 |
|-----------|----------|----------|
| [surface-cwd](../design/policies/cwd.md#surface-cwd-invariant) | surface 생성/변환 | `Surface::source_cwd()` default 없음 — compile-time |
| 포커스 독립성 | 모든 CLI/IPC 명령 | review (전 워크스페이스 순회·ID 직접 지정) — [focus 정책](../design/policies/focus.md) |
| 사용자/에이전트 행동 분리 | release 빌드 IPC 노출 | review + `#[cfg(debug_assertions)]` 격리 — [debug-ipc](../dev-guide/debug-ipc.md) |
| Intent 디스패치 규율 | 호스트 내부 동작 | `check-intent-discipline.sh` 소스 검사 — [action-dispatch](../design/flows/action-dispatch.md) |

> 새 invariant 는 *위반이 조용히 통과하면 큰 회귀* 인 약속만 등재한다. 일반 코딩 규칙은 [CLAUDE.md](../../CLAUDE.md)/dev-guide 로.

결정의 *근거/대안/재검토 조건*(보류 결정 포함)은 [ADR](../adr/index.md).
