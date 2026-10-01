# 아키텍처 개요

Tasty는 본 바이너리(`src/`)와 62 개 크레이트(`crates/*`)로 구성된 Cargo workspace다. 도메인 로직은 GUI 없이 동작하고, GUI·IPC·OS 연동은 port와 adapter로 연결한다.

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
| 공유 메모리 | `tasty-shm`(자체) — POSIX shm + SCM_RIGHTS / Windows DuplicateHandle |

## headless 분리 — `gui` feature

본 바이너리는 `gui` feature(`default = ["gui"]`)로 GUI 표면을 켠다. 끄면 **도메인(`core`) + 외부통신(`hub`) 만 빌드**되고 View/GPU(winit·wgpu·egui)는 컴파일에서 빠진다. 이것이 headless 원칙([identity](../identity.md))의 컴파일 차원 강제다 — 에이전트가 GUI 없이 IPC 로 tasty 를 구동할 수 있다.

`App`(winit `ApplicationHandler` 본체)은 세 부분을 합성한다:

| 필드 | 역할 | gui gate |
|------|------|----------|
| `core: Core` | **도메인 본체** — 워크스페이스/탭/페인/서피스 상태, 세션, attach, registries | 항상 |
| `hub: Hub` | **외부 통신** — IPC 서버(`Option<Box<dyn IpcServerPort>>`), 포트 파일 | 항상 |
| `view: ViewRegistry` | **GUI 어댑터** — winit proxy, `views: HashMap<WindowId, Box<dyn View>>`, `active_modal`(ID·종류)/`focused_view_id` | `#[cfg(feature = "gui")]` |

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

구조 실행과 IPC 핸들러의 구체적인 port 사용은 [MainViewState 소유권](../dev-guide/app-state-ownership.md)을 따른다.

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

## 워크스페이스 크레이트 (62)

아래 목록은 낮은 계층부터 나열한다. 의존은 상위에서 하위로 향하며 순환을 허용하지 않는다. `architecture_layer_order_holds`가 매니페스트 의존과 순서를 대조한다. 크레이트 소속은 각 절 첫 문단에서 백틱 이름으로 시작하는 항목을 읽는다. 순서와 다른 의존을 발견하면 실제 의존과 문서의 계층 순서를 함께 확인한다.

계층 예외는 도메인-IO의 세 의존이다: `tasty-remote → tasty-ipc`, `tasty-file-format → tasty-plugin-protocol`, `tasty-file-handler → tasty-plugin-protocol`. 이유는 해당 절에서 설명한다.

`architecture_crate_list_complete`가 `crates/*/`의 전체 이름과 개수를 비교한다. `doc-guards.yml`이 경로 필터 없이 main push·PR에서 두 검사를 실행하며, 크레이트를 추가할 때는 push 전에 직접 확인한다([CI 가이드](../dev-guide/ci-gates.md)).

### type-\* / primitive (leaf)
`tasty-type-geometry`(길이·도형 타입: LogicalPx·PhysicalPx·Rect, 의존 없음) · `tasty-type-appearance`(색·테마 스키마·ToastKind. 그리기와 이벤트 처리에서 같은 타입 사용. egui 변환은 egui-compat, → type-geometry) · `tasty-design-tokens`(DTCG 디자인 토큰 사본·코드 생성, → type-geometry) · `tasty-utils`(경로 등 공용 함수) · `tasty-cell-width`(렌더러·선택·링크가 공유하는 코드포인트별 셀 폭, 의존 없음) · `tasty-ansi`(CSI·OSC 제거 정규식. terminal의 strip-ansi와 output 파서가 공유, → regex, ADR-0001) · `tasty-timer`(주기 작업을 키로 등록하고 drain_due로 실행. 다음 기한까지 기다리는 waker 스레드 1개, → utils) · `tasty-shm`(공유 메모리와 FD/HANDLE 전달. POSIX shm·SCM_RIGHTS·Windows DuplicateHandle, workspace 및 외부 의존 없음)

이 절 안에서만 의존 가능("type-\*" 는 절 이름이고 규칙의 단위는 **절 소속**이다 — `tasty-utils`·`tasty-ansi`·`tasty-timer`·`tasty-design-tokens` 처럼 이름이 `tasty-type-` 으로 시작하지 않는 것도 이 절이다). 도메인/IO crate 의존 금지(그룹 내 순환도 금지). — [typed-length](../concepts/typed-length.md)

`tasty-shm`은 Tasty의 도메인 상태를 모르고 공유 메모리 전송만 담당하며 workspace 의존도 없다. syscall 사용 여부가 아니라 역할과 의존 방향으로 이 계층에 둔다. SDK가 shm을 사용하는 것을 도메인 의존 예외로 만들 필요도 없다.

### 도메인-IO
`tasty-themes`(전역 Theme·TOML IO) · `tasty-settings`(설정 스키마·직렬화) · `tasty-font`(글리프 atlas) · `tasty-terminal`(PTY·termwiz) · `tasty-hooks`(Surface Hook) · `tasty-memory`(memory.db) · `tasty-event-store`(SQLite event journal 저장 계약. bootstrap 제품 연결, workspace 의존 없음) · `tasty-telemetry`(사용량·진단, → memory) · `tasty-output`(출력 파서) · `tasty-approval`(승인) · `tasty-agent`(세션·수명, → memory) · `tasty-presets`(레이아웃 프리셋) · `tasty-portscan`(포트 조회) · `tasty-reaper`(Windows Job Object로 자식 수명 관리. 다른 OS에서는 동작 없음) · `tasty-lua`(격리 워커·고정 host API, ADR-0027) · `tasty-i18n`(번역) · `tasty-remote-profiles`(연결 프로필·passkey. attach·explorer·plugin이 공유, ADR-0020) · `tasty-ssh`(시스템 ssh 실행·터널·포트 탐색·재시도·취소. SSH 프로토콜은 구현하지 않음, → remote-profiles/i18n/utils) · `tasty-remote`(원격 workspace 조회·생성. CLI·GUI·IPC 공용, → ssh/ipc/remote-profiles, ADR-0001) · `tasty-model`(workspace·pane·tab·surface 모델, GUI·PTY 의존 없음, → type-appearance/type-geometry/utils) · `tasty-domain`(구조 저널의 도메인 이벤트·저널 전용 모델·이벤트 codec·evolve·decide 계약. bootstrap 제품 연결, 이벤트 저장소 의존 없음, → model) · `tasty-dag-layout`(Sugiyama 방식의 DAG 좌표 계산. 본체·갤러리 공용, egui·Theme 의존 없음, → type-geometry. [설명](../dev-guide/dag-layout.md)) · `tasty-git-core`(git2 읽기 전용 래퍼. repo·status·log·diff·worktrees, 원격 host와 git-viewer 공용, → utils, ADR-0022) · `tasty-file-format`(detector·규칙·Lua·구조 평가. 호스트·GUI·IPC 구현 의존 없음, → utils/plugin-protocol) · `tasty-file-handler`(detector와 handler 연결·사용자 설정·plugin 등록·최근 선택. 호스트·GUI·IPC 구현 의존 없음, → utils/file-format/plugin-protocol) · `tasty-selection`(격자 선택·픽셀 변환·적중 판정·텍스트 추출. 렌더러·View 공용, → terminal/type-geometry/cell-width) · `tasty-terminal-link`(URL·파일 경로·여러 줄 링크와 강조 범위 계산. 실제 열기는 호스트 GUI에서 처리, → terminal/type-appearance/cell-width)

이 절 + type-\*/primitive 절만 의존 가능(위와 같이 판정 단위는 절 소속이다). **예외 셋** — 첫째, `tasty-remote` 는 plugin host 의 `tasty-ipc` 에 의존한다: 원격 client 능력이 IPC 호출이고, 합칠 후보 둘(`tasty-ssh` 와 `tasty-ipc`)이 각각 더 나쁜 의존을 들여 기각됐다.
그 방향은 [ADR-0001](../adr/0001-crate-dependency-boundaries.md) 의 결정이다.
둘째, `tasty-file-format` 은 plugin 경계의 `tasty-plugin-protocol` 에 의존한다: plugin 이 형식 레지스트리를 조회하는 port trait 이 그 wire 크레이트에 살고, trait 소유 크레이트가 구체 레지스트리에 역의존하지 않도록 impl은 타입 소유 쪽에 둔다.
의존은 trait 정의 한 개뿐이고 역방향 호출은 없다.
그 방향은 [ADR-0001](../adr/0001-crate-dependency-boundaries.md) 의 결정이다.
셋째, `tasty-file-handler` 도 같은 이유로 `tasty-plugin-protocol` 에 의존한다: plugin 이 handler 레지스트리를 조회하는 port trait 이 그 wire 크레이트에 있고, 같은 역의존을 피하려고 impl을 타입 소유자 쪽에 둔다.
둘째와 같은 형태이고 같은 결정([ADR-0001](../adr/0001-crate-dependency-boundaries.md))의 적용이다.
이 절의 다른 크레이트에는 예외가 없다.

`tasty-event-store`는 데이터 홈 하나의 SQLite 구조 journal 저장 계약을 구현한다. App 초기 engine 생성·선택 슬롯 import·복원·슬롯 폐기는 저장 worker에 연결돼 있다. 일반 구조 명령 전체의 journal 전환은 아직 완료되지 않았다. 한 journal 파일 안에서 다음을 제공한다.

- stream(엔진)별 revision과 expected revision 검사, 여러 stream을 한 batch로 묶는 원자 commit. batch는 번호와 stream별 revision vector로 식별한다.
- 이벤트·명령 기록(재시도 키·요청 digest·해소한 대상·진행 상태·응답)·effect 의무를 한 transaction으로 확정한다. 하나라도 실패하면 아무것도 남지 않는다. 재시도 키로 저장된 대상·결과를 조회할 수 있고, 같은 키·같은 요청의 재제출은 새로 쓰지 않고 기존 기록을 돌려준다. 같은 키의 다른 요청은 충돌로 거절한다.
- writer 독점 잠금과 세대(epoch) fencing. 쓰기는 journal 옆 잠금 파일(`<journal>.writer-lock`)의 OS 독점 잠금을 얻은 저장소만 하며, 잠금을 얻지 못하거나 잠금을 쓸 수 없는 환경이면 writer가 되지 않는다. 자식 프로세스는 생성부터 exec까지 방금 놓은 잠금의 파일 설명을 복제해 가질 수 있으므로, 잠금이 잡혀 있으면 최대 2초 동안 다시 시도한 뒤 실패로 돌려준다. 잠금 없이 연 저장소는 읽기만 한다. 같은 저장소가 새 세대를 등록하면 이전 세대의 쓰기를 거절한다.
- effect 상태(Pending·Running·Deferred·Succeeded·Failed·Cancelled·Superseded·Uncertain)의 허용 전이, activation claim과 attempt 기록, 이전 attempt·generation의 늦은 결과 거절.
- domain snapshot(파생 cache)과 snapshot+tail 읽기, consumer checkpoint, 불변 payload와 참조 기반 GC. 검증에 실패한 snapshot은 건너뛰고 이전 snapshot이나 전체 로그로 재구성한다.
- projection 출력 행(consumer·projection version별 key→바이트)과 consumer 위치를 한 transaction으로 확정한다. 위치가 뒤로 가거나 batch가 없으면 행 변경도 반영하지 않는다. 행을 가진 consumer는 위치만 저장하는 API로 위치를 옮길 수 없다.
- kind별 영속 ID 예약. 예약한 범위는 재오픈 뒤에도 다시 내주지 않으며, 예약 뒤 commit이 실패해 쓰지 않은 구간은 빈 채로 남는다. 상한을 넘는 예약은 되감지 않고 거절한다.

이 크레이트는 도메인 타입을 모른다. 이벤트·effect·snapshot 내용은 type tag·schema version·바이트로 저장하고 해석은 호출자의 codec이 맡는다. WAL과 `synchronous=FULL`이 실제로 적용되지 않거나 journal의 스키마 버전이 이 빌드보다 새로우면 열지 않는다. 비어 있지 않은데 journal 버전 표가 없는 SQLite 파일은 설정을 바꾸기 전에 거절하며 파일을 변경하지 않는다. memory.db·state.db와 독립된 저장소이며 그 DB들과의 원자성은 없다.

이 크레이트의 강제 종료 시험(`crates/tasty-event-store/tests/crash.rs`)은 시험 바이너리를 자식 프로세스로 다시 실행해 지정 지점에서 `abort`시키고, 부모가 journal을 다시 열어 판정한다. 판정 지점은 commit 직후(batch·명령·effect가 모두 남는다), 다음 commit 준비 뒤·확정 전(확정한 batch만 남는다), effect를 Running으로 기록한 직후(새 writer 세대가 그 attempt를 Running으로 보고 대조 전이로 닫을 수 있다), 두 프로세스의 동시 첫 열기(파일이 손상되지 않고 모든 migration이 한 번씩 적용된다)다. 프로세스 종료만 재현하며 전원 차단이나 OS 충돌에 의한 쓰기 유실은 재현하지 않는다.

CoreState의 로컬 트리는 `local_workspaces`, 원격 mirror 트리는 `mirror_workspaces`에 별도로 있다. 렌더·입력·IPC는 빌린 합성 목록을 조회하고 로컬 저장·digest는 로컬 목록만 읽는다. 혼합 표시 순서는 비영속 projection으로 관리한다([ADR-0061](../adr/0061-external-remote-module-and-attach-sync.md)).

앱 진행 값은 `app::state::AppState`, 공유 저장소·OS port·작업 실행 서비스는 `app::services::AppServices`가 소유한다. 부팅 phase와 복원 요청값은 `BootProgress`, OS 창·GPU·engine worker receiver는 `BootResources`로 분리한다. 열린 View와 engine 연결 관계는 기존 registry가 원본이며 AppState에 ID 목록을 복제하지 않는다.

`SurfaceLayout::Leaf`는 ID·kind·확정 activation을 가진 `SurfaceDescriptor`다. 실행 kind 객체는 `EngineRuntime.surfaces` 한 컬렉션에 있고 Terminal/Pty 원본은 같은 engine의 TerminalStore에 있다. runtime kind registry·waker·파일 처리기 등록부와 생성자 파일 I/O도 EngineRuntime에 속한다. `LiveDomainState`는 점유·busy·attention·입력/출력 관측을 담으며, 점유 값 변경 자체는 전송하지 않는다. Remote 모듈이 허브와 전송 큐를 맡고 로컬 readonly Terminal 표시는 EngineRuntime에 남는다. 외부 client session은 Remote가 소유하며 session 값과 sender/outbox/SSH tunnel 자원을 구분한다. socket handshake·reader/writer·heartbeat는 Remote transport에 있고 App을 참조하지 않는 wake 콜백을 받는다. 연결 sender는 교체 슬롯이 아니라 고정 epoch에 묶이며 재연결 전 입력·응답 큐를 새 연결로 재해소하지 않는다. 전체 producer/소비자·복구 이행은 아직 완료되지 않았다.

공통 `ViewState`는 focus·modifiers·dirty/repaint·닫기 요청과 View 수명 식별자를 가지며 OS 창·GPU는 ViewBase 자원이다. 구조 생성의 사용자 선택 continuation은 원래 View 수명과 선택 세대가 유지될 때만 적용하고 저장/replay 대상에는 넣지 않는다. View 이벤트 문맥에는 engine 수명 owner 전체 대신 실행에 필요한 대여를 전달한다. View의 구조 후처리와 기존 직접 writer 제거는 이어지는 전환 범위다.

`tasty-domain`은 순수 구조 판단과 재구성 모델을 구현한다. 연결된 bootstrap·복원 경로의 원본은 worker의 JournalModel이며, CoreState에는 확정 batch의 live projection을 적용한다. 일반 구조 writer의 전환은 진행 중이므로 전체 engine 활성화 완료를 뜻하지 않는다. 의존 방향은 `tasty-domain` → `tasty-model`이고 `tasty-event-store`에는 의존하지 않는다. 두 크레이트를 연결하는 것은 본 바이너리의 runtime 모듈이다.

- 엔진별 구조 stream: journal 하나에 엔진마다 구조 stream이 하나 있고 이름은 `structure:` 접두로 시작한다. 이 접두가 없는 stream은 구조 이벤트로 해석하지 않는다.
- 저널 전용 구조 모델(엔진 하나): workspace·category·pane·tab·surface 트리, 이름, 소속, 분할 비율, surface kind와 자료 참조. workspace의 부제·설명·attach 매핑과 tab의 사용자 지정 이름은 typed 필드이고, metadata는 사용자 정의 키만 담는다. 선택·포커스·접힘 같은 View 상태는 담지 않는다. workspace·category·pane·tab·surface ID와 분할 방향은 `tasty-model`의 타입을 쓰고, revision·batch 번호·분할 비율 비트·surface 자료 참조처럼 저널에만 필요한 값은 이 크레이트에 둔다. 새 ID는 worker가 영속 예약한 고정 값으로 받으며 decide 중 저장소를 호출하지 않는다.
- 구조 이벤트(create·split·move·rename·close, workspace 부제·설명과 attach 매핑 설정, tab 사용자 지정 이름 설정, metadata 설정·삭제)와 이벤트 본문 codec. 이벤트마다 type tag와 schema version을 붙이고 모르는 tag·version은 오류로 중단한다. 분할 비율은 f32 비트를 그대로 저장한다. snapshot 본문은 한 batch 위치의 모든 엔진 모델이며 model version과 함께 해석한다. 저장 봉투·저장 형식 버전·migration은 이벤트 저장소가 정한다.
- pure evolve: 저장 batch가 아니라 해석을 마친 도메인 batch(batch 번호와 엔진 stream별로 revision이 붙은 구조 이벤트 목록)를 받는다. batch 하나를 모든 엔진 모델 사본에 적용한 뒤 교체하므로 한 엔진만 반영되는 일이 없으며, batch 순서와 엔진 stream별 revision의 연속성을 검사한다. 이벤트가 없는 엔진 모델도 적용 위치를 옮긴다.
- decide 계약: 상태·명령만 보고 이벤트·effect·응답을 정한다. 새 ID는 예약 완료된 명령의 고정값이며 시각·명령 identity는 결정 문맥으로 받는다.

본 바이너리의 구조 저널 runtime 모듈(`src/runtime/`)은 두 크레이트와 App의 bootstrap·복원 경계를 연결한다.

- 엔진의 구조 stream 이름을 레이아웃 슬롯 번호로 정한다(`structure:slot-<번호>`). 저장 batch에서 엔진 구조 stream 이벤트만 stream별로 해석해 도메인 batch를 만들고, 도메인 이벤트 본문을 저장 봉투에 담는다. 전체 로그 replay와 snapshot+tail 재구성은 모든 엔진에서 같은 모델·ID·revision을 만든다. snapshot 하나가 모든 엔진 모델과 그 surface 자료 참조를 함께 pin한다.
- decide 계약에 대해 generic한 command executor: 재시도 키 조회를 대상 해소보다 먼저 하고, 새 요청만 decide한 뒤 명령·이벤트·effect를 한 transaction으로 확정한다. 확정에 성공한 뒤에만 메모리 상태에 적용하고 응답한다. 확정이 실패하면 상태를 바꾸지 않고 응답하지 않는다. revision 충돌이면 저장소에서 상태를 다시 읽어 정해진 횟수까지 다시 decide한다. 같은 프로세스에서 진행 중인 같은 키는 첫 실행에 합류하고 다른 요청이면 충돌로 거절한다. writer 잠금을 잃거나 fencing되면 이후 쓰기를 멈춘다. 공개 구조 요청의 도메인 거절은 이벤트·효과 없이 Failed 명령과 원래 wire 오류를 확정한다. 공개 응답 계약 없는 내부 판단 API의 거절은 저장하지 않는다.

순수 명령은 metadata·명시 ID 재정렬·대상 leaf를 대조하는 split 비율과 종류별 생성 준비·설치 완료를 결정한다. operation 준비/결과·activation·kind 변환·capture 세대는 별도 확정 사실이며 같은 activation의 늦은 capture도 거절한다. snapshot 모델은 v3이며 v2의 새 필드는 기본값으로 보완한다. 생성 준비는 activation high-water를 소비하고 단일 outbox claim 아래 설치·이전 owner 정리를 계속한다. 준비 성공 batch와 최종 완료 batch를 구분하며, 최종 commit 전에는 새 surface를 트리에 공개하지 않고 convert·restore의 기존 activation을 유지한다. 예약 generation과 claim은 operation에 귀속되며 원 명령은 InProgress다. `live_projection`은 기존 kind 객체와 SplitNodeId를 보존하며 확정 사실을 적용한다. `effect_runner`는 고정된 launch input으로 비공개 후보를 만들고 설치한다. GUI/headless bootstrap과 `workspace.create`는 이 경계를 사용한다. 생성 명령의 고정 입력·공개 응답 틀은 command resolution에, 진행 상태와 먼저 완료한 부분 응답은 InProgress 기록에 보존한다. 최종 wire는 모든 부분이 끝나야 확정되며 재시도에서 live tree로 다시 만들지 않는다. 일반 명령 진입점 전체의 journal 활성화와 durable 효과 복구는 미완이다.

`src/runtime/journal_product`의 저장 worker는 bounded 요청/완료 채널과 별도 publication ACK를 사용한다. 원본 key 조회에서 같은 진행 중 요청을 합치고, ID 예약은 decide 밖에서 수행한다. 저장소의 canonical 적용 뒤에도 App의 전체 batch 적용 ACK 전에는 성공 응답과 다음 변경을 내보내지 않는다. 부팅 replay cut도 초기 projection ACK가 필요하다. App이 worker를 소유하고 각 EngineSession에 명시 binding을 둔다. 추가 창은 초기 구조와 선택 자원이 준비된 뒤 성공 응답을 내며, 전체 batch 공개 실패는 App의 조회·명령·렌더를 멈춘다. 승인된 외부 설치와 최종 구조 공개 사이에도 App이 관측을 보류한다. IPC·plugin·remote의 기존 수신 큐와 별도로 OS 입력만 수신 당시 View/대상 binding을 가진 한시적 bounded 큐에 보관하며 raw 입력을 journal에 저장하지 않는다. geometry 입력은 구조 revision, surface 키 입력은 확정 activation·물리 generation·kind binding, 호스트 popup 입력은 View의 입력 수신자를 대조한다. 이 공개 경계의 종료·복구 연결과 모든 producer 이행은 계속 진행 중이다.

공개 카테고리 변경과 workspace metadata·순서 요청은 GUI/headless·plugin 공통 journal admission을 사용한다. 현재 권한 검사를 먼저 하고 원래 요청의 key를 조회한 뒤 miss만 대상을 고정한다. 동일 요청은 진행 중에도 합류하며, 공개 응답과 거절은 commit에 고정한다. byte pressure와 요청 크기 거절은 해당 요청만 끝내고, canonical 적용/공개 실패는 typed halt로 App까지 전달한다. 설정의 category reset은 모든 engine의 한 batch이며 늦은 완료가 더 최신 설정을 덮지 않도록 continuation 세대를 대조한다.

mirror 이름·부제·설명·분류와 혼합 표시 순서는 비영속 App continuation이다. 로컬 상대 순서는 확정 모델을 따르며, continuation은 local tree를 재정렬하지 않는다. mirror 교체 때 발급한 token으로 delta·재연결 이후의 오래된 표시 변경을 버리고, 저장 응답 재시도에는 표시 변경과 host 알림을 재발행하지 않는다. 이 token은 transport reconnect epoch와 별개다.

직접 이름 변경 팝업은 확정 후 origin에 맞는 host 알림을 보낸다. 탭 이름 해제 알림은 그 시점의 View 선택 surface 제목을 읽는다. 분할선 드래그는 View 미리보기이며 시작 revision과 명시 split 대상을 고정한 순수 명령으로 끝낸다. `CoreState.committed_structure_revision`은 검증된 live projection의 읽기용 적용 위치이고 명령 원본 모델은 아니다. 로컬 탭 이동 Domain intent도 고정 TabId를 journal로 넘기며, 공개 tab.move와 원격 forward 수신을 포함한 나머지 producer 전환은 아직 진행 중이다.

정상 slot resume는 구조 ID와 incarnation을 유지한다. 복원을 끄고 버리는 engine은 retirement fact를 확정한 뒤 정확한 실행 owner를 회수한다. 폐기된 slot 재사용은 새 incarnation이며 과거 ID·명령·미완 효과는 되감지 않는다. 최초 창은 worker의 journal 슬롯 목록을 받은 뒤 활성 저장 슬롯을 고르고, View 선택은 해당 binding과 공개 cut을 가진 별도 checkpoint로 저장한다([ADR-0063](../adr/0063-event-store-storage-fencing-and-effect-states.md)).

데이터 홈의 명시 binding은 `structure/journal.json`, 저장소는 `structure/journal.db`다. 초기화 잠금 아래 Preparing→Ready로 확정한다. Ready binding에서 DB가 사라지면 빈 journal을 만들지 않는다. 파일을 flush한 뒤 Unix에서는 상위 디렉터리를 sync하고 Windows에서는 write-through rename을 사용한다. 이 절은 전원 차단이나 미실행 OS에서의 내구성을 실측했다는 뜻이 아니다.

기존 layout 슬롯 importer(`src/core/layout_persistence/import.rs`)는 선택한 slot stream이 없는 첫 bootstrap에서만 호출한다. 이미 journal이 있는 정상 resume는 남은 legacy 파일을 다시 읽지 않는다. 슬롯 JSON 하나를 기존 슬롯 판정(높은 version·해석 실패 거절)으로 읽고, 그 슬롯 엔진의 구조 stream에 이벤트 batch 하나와 명령 기록으로 확정한다. 새 ID는 journal의 ID 예약에서 받으므로 여러 슬롯을 가져와도 엔진 사이에서 겹치지 않으며, surface ID는 standalone PTY ID 기준값 아래에서만 받는다. 슬롯 안의 위치(workspace 순서, 깊이 우선 leaf pane 순서, tab 순서, 깊이 우선 surface 순서)와 새 ID의 대응은 결과와 명령 기록에 남기며, 같은 슬롯을 같은 내용으로 다시 가져오면 저장된 대응을 돌려주고 다른 내용이면 거절한다. workspace 부제·설명·attach 매핑과 tab의 사용자 지정 이름은 전용 이벤트로 기록한다. terminal의 cwd·복원 명령·scrollback과 plugin surface 자료는 이벤트가 아니라 surface 저장 자료 payload 하나로 저장하고 이벤트에서 pin하며, terminal에 저장할 값이 없으면 자료 참조를 만들지 않는다. Generic의 null도 restore 호출을 유지하도록 명시 payload로 남긴다. scrollback 파일이 없으면 저장 자료에 참조만 남긴다. 선택 workspace·focus pane·선택 tab·카테고리 접힘은 이벤트로 만들지 않고 결과로만 돌려준다.

구조 digest(`src/runtime/shadow_digest.rs`, CoreState 쪽은 `src/core/layout_persistence/import/shadow.rs`)는 CoreState와 엔진 구조 stream 하나의 journal 모델이 같은 구조를 나타내는지 비교하는 도구다. 제품 incremental projection의 전후 구조 확인에도 쓰며, [ADR-0065](../adr/0065-journal-source-and-core-state-projection.md)의 전체 writer shadow 대조·활성화 완료와는 구별한다.

- 비교 범위: 두 쪽을 같은 정규 표현으로 옮긴다. category 순서와 이름, workspace 순서·이름·소속 category·부제·설명·attach 매핑·metadata, pane 분할 트리(방향, 비율의 f32 비트), pane마다 tab 순서·이름·사용자 지정 이름, tab마다 surface 분할 트리, surface kind·metadata·저장 자료를 담는다. digest는 정규 표현 직렬화의 FNV-1a 64비트 해시이고, 차이를 찾을 때는 경로별 차이 목록을 쓴다. journal 쪽은 순서 목록에서 닿지 않는 항목과 부모 역참조가 맞지 않는 항목도 결함으로 담는다.
- ID: 원래 ID로 비교하거나, category·workspace는 표시 순서로, pane·tab·surface는 전체 깊이 우선 순서로 번호를 다시 매겨 비교한다. importer는 새 ID를 받으므로 CoreState와 가져온 모델은 순서 번호로 비교한다.
- 저장 자료: payload 번호가 아니라 내용으로 비교한다. 바이트 길이와 해시로 비교하거나, surface 저장 자료 형식으로 해석해 비교한다(scrollback은 길이와 해시). 비교에서 뺄 수도 있다.
- CoreState 쪽 입력은 두 가지다. 트리를 직접 읽은 정규 표현(저장 자료는 비교하지 않는다)과, capture → importer → journal 재구성을 거친 모델의 정규 표현이다. journal 쪽은 전체 로그 replay와 snapshot+tail 재구성에서 같은 digest를 낸다.
- 비교에서 빼는 값: `SurfaceLayout::Split.node_id`(프로세스 내부 호환 projection 식별자), `Tab.surface_titles`(surface별 Terminal 관측 캐시), `Workspace.mirror`(원격 구조는 로컬 비교에서 통째로 제외), `JournalModel.applied`(journal 위치), `EmptySurface.spawn_attempts`(실행 재시도 횟수). 사용자 선택·접힘·탭바 스크롤은 CoreState 원본에 없으므로 이 제외 명부의 필드가 아니다.
- 알려진 불일치: `unregistered-surface-kind` — layout capture는 등록되지 않은 kind의 surface를 kind `empty`로 저장하므로, CoreState를 직접 읽은 kind와 가져온 모델의 kind가 다르다. capture가 그대로 저장하는 kind(terminal과 surface kind registry에 있는 kind)가 `empty`가 된 차이는 회귀로 보고 이 불일치로 분류하지 않는다. 시험의 세 엔진(다단 분할, 여러 surface kind, typed 필드, 빈 category, mirror workspace 포함)에서 이 불일치 외에는 차이가 없다.
- 한계: CoreState를 직접 읽은 쪽은 저장 자료와 metadata를 비교하지 않는다. terminal cwd·복원 명령·scrollback의 해석자는 capture이고, surface metadata는 CoreState 트리가 아니라 memory DB에 있다. 저장 자료 비교는 journal 모델끼리(가져온 직후와 재구성 뒤, 서로 다른 journal)만 한다. 순서 번호 비교는 같은 구조에 다른 ID를 매긴 결함을 드러내지 않는다. 결함 목록은 원래 ID로 적으므로 순서 번호 비교에서는 ID만 다른 결함도 차이로 보인다. capture는 scrollback 저장이 켜져 있으면 살아 있는 terminal의 scrollback을 디스크에 쓰고, 겹친 대기 terminal의 scrollback 저장 ID를 새로 정해 EngineRuntime의 TerminalStore를 바꾼다. 차이 판정은 정규 표현의 동등 비교와 경로별 차이 목록으로 한다. digest는 비교를 줄인 요약값이며 암호학적 해시가 아니다(FNV-1a 64비트). 재구성 경로를 비교하는 일부 시험(`every_replay_path_gives_the_same_digest`, `replaying_the_imported_journal_gives_the_import_digest`의 저장 자료 해석 경로)은 digest가 같은지만 본다.

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
| `state/` | `MainViewState` — MainView 당 1개 런타임 상태(focus/layout/mouse/mark/restore). 공통 App adapter 포트 `CascadeWindow`의 구현(`cascade_window.rs`). Headless는 별도 `CommandContext` |
| `gfx/` | GPU — `GpuState`, renderer(셀 렌더), screenshot, perf. — [gpu-rendering](../dev-guide/gpu-rendering.md) |
| `adapters/` | 외부 경계 구현 — `ui`(egui 컴포넌트·popup), `ipc`(handler), `production`/`test`(port 구현체), `cli`, `plugin` |
| `ports/` | **의존성 역전 trait** — ipc_server, clipboard, clock, fs, home, process, notification_sound (production/test adapter 가 구현 → headless·테스트 교체). 도메인의 일부다 |
| `intent/` | **Intent 큐** — 호스트 내부 동작 디스패치. — [action-dispatch](../design/flows/action-dispatch.md) |
| `host_api/` | 호스트가 외부(plugin/agent)에 제공하는 인터페이스 — Lua hooks, webview |
| `hook_runtime/` | 엔진별 훅 등록·감시 상태(`HookRuntimeState` — surface 훅·전역 훅)와 발화한 훅의 실행(바인딩 실행 · 전역 훅 셸 실행 · IpcSequence worker). 공유 handler 정의 registry는 `hook_handler/` |
| `runtime/` | 제품 engine의 `EngineSession` 소유·실행 생성자. 구조 저널 runtime(bootstrap·복원 제품 연결, 일반 writer 전환 중) — 저장 batch ↔ 도메인 batch 변환, journal replay·snapshot, decide 계약에 generic한 command executor, CoreState와 journal 모델의 구조 digest. `tasty-domain`과 `tasty-event-store`를 연결한다 |
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
