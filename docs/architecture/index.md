# 아키텍처 개요

tasty 는 Cargo 워크스페이스 기반 크로스 플랫폼 GPU 가속 터미널 에뮬레이터다. **본 바이너리(`src/`) + 60 개 크레이트(`crates/*`)** 로 구성되며, **ports-and-adapters(헥사고날) + headless core** 로 layering 된다 — 도메인 로직은 GUI 없이도 동작하고, GUI/IPC/OS 연동은 교체 가능한 adapter 뒤에 있다.

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

> 옛 `Engine` struct 는 삭제됐고 필드가 Core/Hub/View 로 분산됐다. 전환기 컨테이너로 남아 있던 `engine` 모듈도 사라졌고, 그 sub-module(`surface_registry` / `command_index` / `output_observer` / `layout_persistence`)은 `src/core/` 아래로 옮겨졌다.

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

의존은 아래 계층 순서로만 흐른다(상위 → 하위). 순환 없음. **그 순서를 `crates/tasty-doc-guards/tests/architecture_layer_order_holds.rs` 가 매니페스트 의존과 대조한다** — 절 소속은 각 절 **첫 문단의 열거**에서 읽고(항목마다 `` `이름` `` 으로 시작), 순서를 거스르는 간선은 문서 본문이 이유를 적은 것만 허용한다.
지금 그런 예외는 셋이고 전부 도메인-IO 절이다 — `tasty-remote` → `tasty-ipc`, 그리고 `tasty-file-format`·`tasty-file-handler` → `tasty-plugin-protocol`(아래 도메인-IO 절이 각각 이유를 적는다). **순서를 거스르는 간선을 보면 그 간선보다 절 순서를 먼저 의심해라** — 실측에서 그런 넷은 전부 정상 의존이었고 절 넷이 잘못된 자리에 있었다.
이 절은 `crates/*/` 전체를 빠짐없이 열거한다 — `crates/tasty-doc-guards/tests/architecture_crate_list_complete.rs` 가 각 디렉토리명의 등장과 위 괄호 수의 일치를 강제한다 — `doc-guards.yml` 이 main push · PR 마다 자동으로 돌린다([ci-gates](../dev-guide/ci-gates.md)). 크레이트를 추가했으면 push 전에 직접 돌려라.

### type-\* / primitive (leaf)
`tasty-type-geometry`(길이·도형: `LogicalPx`/`PhysicalPx`/`Rect`, 의존 0) · `tasty-type-appearance`(색·테마 schema + toast 분류 `ToastKind` — 색을 고르는 값이라 여기 있고, egui 는 `egui-compat` feature 뒤라 headless 도 그대로 쓴다. 그리는 쪽과 발화하는 쪽이 서로를 안 보고 같은 열거를 본다, → type-geometry) · `tasty-design-tokens`(vendored DTCG 디자인 토큰 + codegen, → type-geometry 만) · `tasty-utils`(path helper, leaf) · `tasty-cell-width`(문자 하나가 터미널 셀을 몇 칸 차지하는가 — 렌더러·선택 모델·링크 스캐너가 같은 답을 써야 하고, 그 답이 GPU 렌더러 안에 있으면 도메인이 렌더러를 거꾸로 본다. 코드포인트 구간표, 의존 0) · `tasty-ansi`(ANSI escape 제거 — CSI/OSC 정규식 하나. `tasty-terminal`(IPC `--strip-ansi`)과 `tasty-output`(파서의 plain text 매칭)이 공유한다. 두 크레이트가 서로를 흡수하면 상대가 몰라도 되는 의존(serde / termwiz)을 들이므로 크기가 아니라 의존 방향으로 분리했다, → regex 만, ADR-0601) · `tasty-timer`(중앙 타이머 허브 — 메인 루프의 주기 작업을 키로 등록하고 매 프레임 `drain_due` 로 소비, 고정 주기 ticker 스레드 대신 다음 데드라인까지만 자는 waker 스레드 1개, → utils) · `tasty-shm`(공유 메모리 + FD/HANDLE 전달 primitive — POSIX shm + SCM_RIGHTS / Windows DuplicateHandle. 호스트와 plugin 이 대용량 데이터를 주고받는 **선(wire)** 이라 `tasty-plugin-protocol` 과 같은 역할이고, tasty 도메인 개념을 담지 않는다. 워크스페이스 내부 의존 0, 의존 0)

이 절 안에서만 의존 가능("type-\*" 는 절 이름이고 규칙의 단위는 **절 소속**이다 — `tasty-utils`·`tasty-ansi`·`tasty-timer`·`tasty-design-tokens` 처럼 이름이 `tasty-type-` 으로 시작하지 않는 것도 이 절이다). 도메인/IO crate 의존 금지(그룹 내 순환도 금지). — [typed-length](../concepts/typed-length.md)

`tasty-shm` 이 이 절인 이유는 **의존 방향과 역할**이다. syscall 을 부른다는 사실은 분류 기준이 아니다 — `tasty-utils`(경로 IO)·`tasty-timer`(스레드/슬립)도 부른다. 도메인-IO 절의 크레이트는 전부 tasty 의 도메인 개념(테마·설정·터미널·훅·메모리·에이전트·프리셋·ssh·원격·모델·git)을 담는데 shm 은 담지 않고, 워크스페이스 내부 의존이 0 이라 이 절의 규칙("절 안에서만 의존")을 그대로 만족한다. **이 분류에 걸린 것이 아래 sandbox 경계 문장이다** — shm 이 도메인-IO 라면 `tasty-plugin-sdk → tasty-shm` 이 그 문장의 예외가 되어야 하고, 경계 진술에 안 적힌 예외를 두는 대가가 가장 크다. 실측: 이 절로 옮겨도 새로 빨개지는 간선은 0 이다(shm 의 소비자는 plugin host 와 sandbox 경계 둘, 양쪽 다 이 절보다 상위다).

### 도메인-IO
`tasty-themes`(전역 Theme + TOML IO) · `tasty-settings`(설정 스키마/직렬화) · `tasty-font`(글리프 atlas) · `tasty-terminal`(PTY + termwiz) · `tasty-hooks`(Surface Hook) · `tasty-memory`(에이전트 메모리 `memory.db`) · `tasty-telemetry`(→ memory) · `tasty-output`(출력 파서 카탈로그) · `tasty-approval`(approval 게이트) · `tasty-agent`(세션/lifecycle, → memory) · `tasty-presets`(레이아웃 프리셋) · `tasty-portscan` · `tasty-reaper`(자식 프로세스를 호스트 수명에 결박 — Windows Job Object / 비-Windows no-op) · `tasty-lua`(Lua 스크립트 — 워커 격리 + 고정 host API, ADR-0031) · `tasty-i18n`(번역) · `tasty-remote-profiles`(원격 연결 프로필 + passkey, typed-tagged registry — attach/explorer/plugin 공유, ADR-0015/0032) · `tasty-ssh`(시스템 ssh 위임 — ssh 프로세스 spawn · 터널 수명 · 원격 포트 발견 · 백오프 · 취소. SSH 프로토콜은 구현하지 않는다, → remote-profiles/i18n/utils) · `tasty-remote`(원격 인스턴스 client 능력 — 워크스페이스 조회/생성. CLI·GUI·IPC 3소비자 공유, → ssh/ipc/remote-profiles, ADR-0601) · `tasty-model`(도메인 모델 — workspace/pane/tab/surface, → terminal/type-appearance/type-geometry/utils, GUI-free) · `tasty-dag-layout`(task DAG 레이어 레이아웃 — Sugiyama 계열로 노드 좌표만 계산, egui/Theme 를 모르는 순수 계산이라 본체·갤러리가 같은 코드를 씀, → type-geometry 만. [dag-layout](../dev-guide/dag-layout.md)) · `tasty-git-core`(read-only git2 래퍼 — repo 탐색·status/log/diff/worktrees, mutate 없음. host core(원격 attach git query)와 `tasty-plugin-git-viewer`(로컬)가 공유, → utils, ADR-0056) · `tasty-file-format`(파일 형식 식별 — detector 레지스트리 + 규칙/Lua/구조 평가기. 호스트·GUI·IPC 결합 0, → utils/plugin-protocol) · `tasty-file-handler`(파일 핸들러 정책·레지스트리 — detector→handler 매핑, 사용자 오버라이드, plugin 기여, 최근 선택. 호스트·GUI·IPC 결합 0, → utils/file-format/plugin-protocol) · `tasty-selection`(터미널 텍스트 선택 — 격자 좌표·선택 모드·픽셀→격자 사상·적중 판정·선택 텍스트 추출. 렌더러와 view 가 같은 타입을 써야 해서 타입 소속만 내렸다, → terminal/type-geometry/cell-width) · `tasty-terminal-link`(터미널 화면 텍스트의 링크 검출 — URL·경로 스캔, 줄바꿈을 가로지르는 세그먼트, 렌더러가 색을 덮는 하이라이트 범위. 링크를 **여는** 일은 OS 부수효과라 본체의 `gui` 뒤에 남는다, → terminal/type-appearance/cell-width)

이 절 + type-\*/primitive 절만 의존 가능(위와 같이 판정 단위는 절 소속이다). **예외 셋** — 첫째, `tasty-remote` 는 plugin host 의 `tasty-ipc` 에 의존한다: 원격 client 능력이 IPC 호출이고, 합칠 후보 둘(`tasty-ssh` 와 `tasty-ipc`)이 각각 더 나쁜 의존을 들여 기각됐다.
그 방향은 [ADR-0601](../adr/0601-crate-dependency-boundaries.md) 의 결정이다.
둘째, `tasty-file-format` 은 plugin 경계의 `tasty-plugin-protocol` 에 의존한다: plugin 이 형식 레지스트리를 조회하는 port trait 이 그 wire 크레이트에 살고, trait 소유 크레이트가 구체 레지스트리에 역의존하지 않도록 impl은 타입 소유 쪽에 둔다.
의존은 trait 정의 한 개뿐이고 역방향 호출은 없다.
그 방향은 [ADR-0601](../adr/0601-crate-dependency-boundaries.md) 의 결정이다.
셋째, `tasty-file-handler` 도 같은 이유로 `tasty-plugin-protocol` 에 의존한다: plugin 이 handler 레지스트리를 조회하는 port trait 이 그 wire 크레이트에 있고, 같은 역의존을 피하려고 impl을 타입 소유자 쪽에 둔다.
둘째와 같은 형태이고 같은 결정([ADR-0601](../adr/0601-crate-dependency-boundaries.md))의 적용이다.
이 절의 다른 크레이트에는 예외가 없다.

### UI primitive
`tasty-egui-theme`(Theme → egui Visuals/Style 어댑터) · `tasty-ui-widgets`(본체·갤러리 공유 egui 위젯/레이아웃 primitive — 시각 동기화 단일 출처) · `tasty-icons`(line/fill 아이콘 SVG 단일 출처 — host/gallery/plugin build-time bake 공유) · `tasty-key-match`(바인딩 문자열을 실제 키 이벤트와 대조 — 단축키 디스패치와 webview 키 브리지가 같은 규칙을 쓰는데, 규칙이 `gui` feature 뒤에 있으면 브리지가 UI 에 묶인다. egui 갈래만 `egui-input` feature 뒤, → settings/winit). — [ui-widgets-crate](ui-widgets-crate.md)

### OS 경계
`tasty-platform`(OS 를 부르는 코드 전부 — panic/hang 리포트와 호스트 로그 초기화, 네이티브 컨텍스트 메뉴(NSMenu / TrackPopupMenu / GTK), 시스템 트레이, Windows jump list · 전원 재개 후크, CSD 윈도우 크롬, OS 인터랙티브 화면 캡처, macOS 권한 preflight 와 Dock/메뉴바 delegate, 이벤트 루프 stall 워치독. **App 을 안 본다** — OS 신호가 `AppEvent` 로 무엇이 되는지는 부르는 쪽이 콜백으로 정한다([ADR-0601](../adr/0601-crate-dependency-boundaries.md)). 윈도잉·GTK·AppKit·트레이 의존은 전부 `gui` feature 뒤라 headless 빌드가 링크하는 것은 crash 리포트 갈래뿐이다, → i18n/settings/utils, [ADR-0601](../adr/0601-crate-dependency-boundaries.md))

이 절이 UI primitive 뒤에 오는 이유는 의존이 아니라 **소비자**다 — 이 크레이트를 드는 것은 본 바이너리 하나이고, 자기가 드는 것은 위 두 절(도메인-IO · type-\*)뿐이다. 네이티브 메뉴·트레이가 UI 지만 egui 를 한 줄도 안 쓰므로 UI primitive 절에 넣지 않았다: 그 절의 규칙은 "본체·갤러리가 공유하는 egui 위젯" 이고 이 크레이트에는 갤러리 소비자가 없다.

### plugin protocol / SDK (sandbox 경계)
`tasty-plugin-protocol`(호스트↔plugin 와이어, → type-appearance) · `tasty-plugin-sdk`(외부 plugin 제작 SDK, → protocol/shm/utils/i18n) · `tasty-plugin-sdk-wasm`(WASM 타깃 SDK) · `tasty-plugin-agent-common`(AI CLI 자식을 다루는 두 번들 plugin — claude/codex — 이 공유하는 헬퍼: prompt 임시파일·형제 hook 정리·children 응답 읽기·reboot 인자. 이름이 `tasty-plugin-` 으로 시작하지만 매니페스트가 없어 번들 plugin 이 아니다, → sdk)

이 계층은 도메인-IO 에 **직접 의존하지 않는다**(sandbox 경계) — protocol/sdk 만 통과. **예외 하나** — `tasty-plugin-sdk` → `tasty-i18n`: 호스트와 SDK 가 설치·사용자 plugin 카탈로그 로딩을 공유한다.

### plugin host (IPC 인프라)
`tasty-plugin-manifest`(manifest 스키마/파서) · `tasty-ipc`(JSON-RPC envelope + caller + audit + method_meta + facade trait + 클라이언트 연결 `client::{IpcConnection, StreamConnection}` — 서버·프레이밍과 같은 크레이트 · off-main 스레드가 호스트 큐에 요청을 주입하는 `host_call::HostIpcInjector` · attach 스트림 연결마다 bounded push sink 를 드는 서버측 레지스트리 `stream_hub::StreamHub` — 소켓을 읽고 쓰는 accept 스레드는 본체 adapter `tcp_ipc_server.rs` 에 남는다, [ADR-0601](../adr/0601-crate-dependency-boundaries.md)) · `tasty-host-plugin`(호스트의 plugin 매니저/process/event_bus/registry)

### 번들 plugin (bin 크레이트, 모두 `tasty-plugin-sdk` 의존)
`tasty-plugin-claude`(lib 도 함께 노출) · `tasty-plugin-codex` · `tasty-plugin-git-viewer` · `tasty-plugin-clipboard-viewer` · `tasty-plugin-image` · `tasty-plugin-html` · `tasty-plugin-markdown` · `tasty-plugin-agent-stream` · `tasty-plugin-mesh-demo`(+ manifest). 뒤의 둘은 `bundle = false` 라 배포 패키징에서는 빠지고 dev 번들 sync 로만 붙는다. — [concepts/plugins](../concepts/plugins.md)

### 도구 / standalone
`tasty-tui-simulator`(E2E TUI 시뮬레이터, lib + `tasty-tui-sim` binary — 로직은 lib 공유, debug 빌드에선 `tasty debug sim` 으로도 노출) · `tasty-gallery`(ui-widgets 데모 바이너리, `cargo run -p tasty-gallery` — 본체 빌드와 분리)

### CLI client
`tasty-cli`(clap CLI — request/format/transport/dynamic plugin subcommand. → ipc/host-plugin/terminal/approval/remote-profiles/remote/ssh/i18n/plugin-manifest/plugin-protocol/tui-simulator/utils)

### 테스트·가드 전용 (제품 산출물 밖)
`tasty-latency-control`(지연 단정의 대조군 — 부하가 만든 값과 코드가 만든 값을 가른다. 계열 둘(고정 CPU 일감 · 자식 하나 띄우기)을 주고, 실패 문장이 어느 계열을 썼는지 밝힌다. **의존 0 + 소비처가 전부 `dev-dependencies`** 라 제품 산출물에 안 들어간다 — [ADR-0181](../adr/0181-a-latency-assertion-must-carry-a-control-that-load-moves-and-code-does-not.md)) · `tasty-doc-guards`(문서를 읽는 통합 가드들의 집 — `docs/` · `site/` · `*.md` 를 소스·워크플로 텍스트와 대조한다. **의존이 0 인 것이 존재 이유다**: 잡이 싸야 CI 에서 경로 필터 없이 매 push 돌릴 수 있고, 그래야 문서만 바뀐 push 에서도 돈다 — [ADR-0138](../adr/0138-doc-guards-live-in-a-dependency-free-crate.md)) · `tasty-test-support`(테스트 전용 RAII 가드 — 프로세스 전역 env 와 `TASTY_HOME` 을 시험 동안만 갈아끼우고 `Drop` 에서 되돌린다. 위 둘과 달리 의존이 0 은 아니지만(→ utils/tempfile) **소비처가 전부 `dev-dependencies`** 라 같은 이유로 이 절에 있다)

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
| [multi-window](multi-window.md) | App = Core/Hub/ViewRegistry, Window trait 계층, 모달 불변식, 단일 프로세스 근거 |
| [input-layer](input-layer.md) | 마우스 입력 z-order 계층 — 소비/버블링 + 커서 결정 |
| [data-flows](data-flows.md) | 주요 데이터 흐름 (파일+함수 기준) |
| [ipc-server](ipc-server.md) | IPC 서버가 요청을 받아들이고 처리하는 쪽의 규칙 — 입장 상한 · dispatch 회차 예산 · 기한 · wake · 요청 압력 게이지 |
| [ui-widgets-crate](ui-widgets-crate.md) | `tasty-ui-widgets` — 본체·갤러리 공유 UI primitive |
| [Invariants](#invariants) | 깨지면 안 되는 시스템 약속 (surface-cwd 등) — 아래 절 |

## Invariants

*깨지면 안 되는 시스템 약속* — 코드 변경 시 가장 먼저 점검할 리스트. 각 invariant 는 가능하면 컴파일/CI 로 강제하고, 그게 불가능하면 review 로 지킨다.

| Invariant | 적용 시점 | 강제 기제 |
|-----------|----------|----------|
| [surface-cwd](../design/policies/cwd.md#surface-cwd-invariant) | surface 생성/변환 | `Surface::source_cwd()` default 없음 — compile-time |
| 포커스 독립성 | 모든 CLI/IPC 명령 | review (전 워크스페이스 순회·ID 직접 지정) — [focus 정책](../design/policies/focus.md) |
| 사용자/에이전트 행동 분리 | release 빌드 IPC 노출 | review + `#[cfg(debug_assertions)]` 격리 — [debug-ipc](../dev-guide/debug-ipc.md) |
| Intent 디스패치 규율 | 호스트 내부 동작 | `check-intent-discipline.sh` grep CI — [action-dispatch](../design/flows/action-dispatch.md) |

> 새 invariant 는 *위반이 조용히 통과하면 큰 회귀* 인 약속만 등재한다. 일반 코딩 규칙은 [CLAUDE.md](../../CLAUDE.md)/dev-guide 로.

결정의 *근거/대안/재검토 조건*(보류 결정 포함)은 [ADR](../adr/index.md).
