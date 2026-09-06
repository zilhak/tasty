# 아키텍처 개요

tasty 는 Cargo 워크스페이스 기반 크로스 플랫폼 GPU 가속 터미널 에뮬레이터다. **본 바이너리(`src/`) + 52 개 크레이트(`crates/*`)** 로 구성되며, **ports-and-adapters(헥사고날) + headless core** 로 layering 된다 — 도메인 로직은 GUI 없이도 동작하고, GUI/IPC/OS 연동은 교체 가능한 adapter 뒤에 있다.

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

## 워크스페이스 크레이트 (52)

의존은 아래 계층 순서로만 흐른다(상위 → 하위). 순환 없음. **그 순서를 `crates/tasty-doc-guards/tests/architecture_layer_order_holds.rs` 가 매니페스트 의존과 대조한다** — 절 소속은 각 절 **첫 문단의 열거**에서 읽고(항목마다 `` `이름` `` 으로 시작), 순서를 거스르는 간선은 문서 본문이 이유를 적은 것만 허용한다. 지금 그런 예외는 `tasty-remote` → `tasty-ipc` 하나다(아래 도메인-IO 절). **순서를 거스르는 간선을 보면 그 간선보다 절 순서를 먼저 의심해라** — 실측에서 그런 넷은 전부 정상 의존이었고 절 넷이 잘못된 자리에 있었다. 이 절은 `crates/*/` 전체를 빠짐없이 열거한다 — `crates/tasty-doc-guards/tests/architecture_crate_list_complete.rs` 가 각 디렉토리명의 등장과 위 괄호 수의 일치를 강제한다 — `doc-guards.yml` 이 main push · PR 마다 자동으로 돌린다([ci-gates](../dev-guide/ci-gates.md)). 크레이트를 추가했으면 push 전에 직접 돌려라.

### type-\* / primitive (leaf)
`tasty-type-geometry`(길이·도형: `LogicalPx`/`PhysicalPx`/`Rect`, 의존 0) · `tasty-type-appearance`(색·테마 schema, → type-geometry) · `tasty-design-tokens`(vendored DTCG 디자인 토큰 + codegen, → type-geometry 만) · `tasty-utils`(path helper, leaf) · `tasty-ansi`(ANSI escape 제거 — CSI/OSC 정규식 하나. `tasty-terminal`(IPC `--strip-ansi`)과 `tasty-output`(파서의 plain text 매칭)이 공유한다. 두 크레이트가 서로를 흡수하면 상대가 몰라도 되는 의존(serde / termwiz)을 들이므로 크기가 아니라 의존 방향으로 분리했다, → regex 만, ADR-0089) · `tasty-timer`(중앙 타이머 허브 — 메인 루프의 주기 작업을 키로 등록하고 매 프레임 `drain_due` 로 소비, 고정 주기 ticker 스레드 대신 다음 데드라인까지만 자는 waker 스레드 1개, 의존 0) · `tasty-shm`(공유 메모리 + FD/HANDLE 전달 primitive — POSIX shm + SCM_RIGHTS / Windows DuplicateHandle. 호스트와 plugin 이 대용량 데이터를 주고받는 **선(wire)** 이라 `tasty-plugin-protocol` 과 같은 역할이고, tasty 도메인 개념을 담지 않는다. 워크스페이스 내부 의존 0, 의존 0)

이 절 안에서만 의존 가능("type-\*" 는 절 이름이고 규칙의 단위는 **절 소속**이다 — `tasty-utils`·`tasty-ansi`·`tasty-timer`·`tasty-design-tokens` 처럼 이름이 `tasty-type-` 으로 시작하지 않는 것도 이 절이다). 도메인/IO crate 의존 금지(그룹 내 순환도 금지). — [typed-length](../concepts/typed-length.md)

`tasty-shm` 이 이 절인 이유는 **의존 방향과 역할**이다. syscall 을 부른다는 사실은 분류 기준이 아니다 — `tasty-utils`(경로 IO)·`tasty-timer`(스레드/슬립)도 부른다. 도메인-IO 절의 크레이트는 전부 tasty 의 도메인 개념(테마·설정·터미널·훅·메모리·에이전트·프리셋·ssh·원격·모델·git)을 담는데 shm 은 담지 않고, 워크스페이스 내부 의존이 0 이라 이 절의 규칙("절 안에서만 의존")을 그대로 만족한다. **이 분류에 걸린 것이 아래 sandbox 경계 문장이다** — shm 이 도메인-IO 라면 `tasty-plugin-sdk → tasty-shm` 이 그 문장의 예외가 되어야 하고, 경계 진술에 안 적힌 예외를 두는 대가가 가장 크다. 실측: 이 절로 옮겨도 새로 빨개지는 간선은 0 이다(shm 의 소비자는 plugin host 와 sandbox 경계 둘, 양쪽 다 이 절보다 상위다).

### 도메인-IO
`tasty-themes`(전역 Theme + TOML IO) · `tasty-settings`(설정 스키마/직렬화) · `tasty-font`(글리프 atlas) · `tasty-terminal`(PTY + termwiz) · `tasty-hooks`(Surface Hook) · `tasty-memory`(에이전트 메모리 `memory.db`) · `tasty-telemetry`(→ memory) · `tasty-output`(출력 파서 카탈로그) · `tasty-approval`(approval 게이트) · `tasty-agent`(세션/lifecycle, → memory) · `tasty-presets`(레이아웃 프리셋) · `tasty-portscan` · `tasty-reaper`(자식 프로세스를 호스트 수명에 결박 — Windows Job Object / 비-Windows no-op) · `tasty-lua`(Lua 스크립트 — 워커 격리 + 고정 host API, ADR-0031) · `tasty-i18n`(번역) · `tasty-remote-profiles`(원격 연결 프로필 + passkey, typed-tagged registry — attach/explorer/plugin 공유, ADR-0015/0032) · `tasty-ssh`(시스템 ssh 위임 — ssh 프로세스 spawn · 터널 수명 · 원격 포트 발견 · 백오프 · 취소. SSH 프로토콜은 구현하지 않는다, → remote-profiles/i18n/utils) · `tasty-remote`(원격 인스턴스 client 능력 — 워크스페이스 조회/생성. CLI·GUI·IPC 3소비자 공유, → ssh/ipc/remote-profiles, ADR-0089) · `tasty-model`(도메인 모델 — workspace/pane/tab/surface, → terminal/type-geometry/utils, GUI-free) · `tasty-dag-layout`(task DAG 레이어 레이아웃 — Sugiyama 계열로 노드 좌표만 계산, egui/Theme 를 모르는 순수 계산이라 본체·갤러리가 같은 코드를 씀, → type-geometry 만. [dag-layout](../dev-guide/dag-layout.md)) · `tasty-git-core`(read-only git2 래퍼 — repo 탐색·status/log/diff/worktrees, mutate 없음. host core(원격 attach git query)와 `tasty-plugin-git-viewer`(로컬)가 공유, → utils, ADR-0056)

이 절 + type-\*/primitive 절만 의존 가능(위와 같이 판정 단위는 절 소속이다). **예외 하나** — `tasty-remote` 는 plugin host 의 `tasty-ipc` 에 의존한다: 원격 client 능력이 IPC 호출이고, 합칠 후보 둘(`tasty-ssh` 와 `tasty-ipc`)이 각각 더 나쁜 의존을 들여 기각됐다. 그 방향은 [ADR-0089](../adr/0089-crate-split-follows-dependency-direction.md) 의 결정이다. 이 절의 다른 크레이트에는 예외가 없다.

### UI primitive
`tasty-egui-theme`(Theme → egui Visuals/Style 어댑터) · `tasty-ui-widgets`(본체·갤러리 공유 egui 위젯/레이아웃 primitive — 시각 동기화 단일 출처) · `tasty-icons`(line/fill 아이콘 SVG 단일 출처 — host/gallery/plugin build-time bake 공유). — [ui-widgets-crate](ui-widgets-crate.md)

### plugin protocol / SDK (sandbox 경계)
`tasty-plugin-protocol`(호스트↔plugin 와이어, leaf) · `tasty-plugin-sdk`(외부 plugin 제작 SDK, → protocol/shm) · `tasty-plugin-sdk-wasm`(WASM 타깃 SDK) · `tasty-plugin-agent-common`(AI CLI 자식을 다루는 두 번들 plugin — claude/codex — 이 공유하는 헬퍼: prompt 임시파일·형제 hook 정리·children 응답 읽기·reboot 인자. 이름이 `tasty-plugin-` 으로 시작하지만 매니페스트가 없어 번들 plugin 이 아니다, → sdk)

이 계층은 도메인-IO 에 **직접 의존하지 않는다**(sandbox 경계) — protocol/sdk 만 통과.

### plugin host (IPC 인프라)
`tasty-plugin-manifest`(manifest 스키마/파서) · `tasty-ipc`(JSON-RPC envelope + caller + audit + method_meta + facade trait + 클라이언트 연결 `client::{IpcConnection, StreamConnection}` — 서버·프레이밍과 같은 크레이트) · `tasty-host-plugin`(호스트의 plugin 매니저/process/event_bus/registry)

### 번들 plugin (bin 크레이트, 모두 `tasty-plugin-sdk` 의존)
`tasty-plugin-claude`(lib 도 함께 노출) · `tasty-plugin-codex` · `tasty-plugin-git-viewer` · `tasty-plugin-clipboard-viewer` · `tasty-plugin-image` · `tasty-plugin-html` · `tasty-plugin-markdown` · `tasty-plugin-agent-stream` · `tasty-plugin-mesh-demo`(+ manifest). 뒤의 둘은 `bundle = false` 라 배포 패키징에서는 빠지고 dev 번들 sync 로만 붙는다. — [concepts/plugins](../concepts/plugins.md)

### 도구 / standalone
`tasty-tui-simulator`(E2E TUI 시뮬레이터, lib + `tasty-tui-sim` binary — 로직은 lib 공유, debug 빌드에선 `tasty debug sim` 으로도 노출) · `tasty-gallery`(ui-widgets 데모 바이너리, `cargo run -p tasty-gallery` — 본체 빌드와 분리)

### CLI client
`tasty-cli`(clap CLI — request/format/transport/dynamic plugin subcommand. → ipc/host-plugin/terminal/approval/remote-profiles)

### 가드 전용 (의존 0)
`tasty-latency-control`(지연 단정의 대조군 — 부하가 만든 값과 코드가 만든 값을 가른다. 계열 둘(고정 CPU 일감 · 자식 하나 띄우기)을 주고, 실패 문장이 어느 계열을 썼는지 밝힌다. **의존 0 + 소비처가 전부 `dev-dependencies`** 라 제품 산출물에 안 들어간다 — [ADR-0181](../adr/0181-a-latency-assertion-must-carry-a-control-that-load-moves-and-code-does-not.md)) · `tasty-doc-guards`(문서를 읽는 통합 가드들의 집 — `docs/` · `site/` · `*.md` 를 소스·워크플로 텍스트와 대조한다. **의존이 0 인 것이 존재 이유다**: 잡이 싸야 CI 에서 경로 필터 없이 매 push 돌릴 수 있고, 그래야 문서만 바뀐 push 에서도 돈다 — [ADR-0138](../adr/0138-doc-guards-live-in-a-dependency-free-crate.md))

### 본 바이너리 (`tasty`)
위 크레이트를 의존하며 App/View/GPU/IPC 라우터/부팅을 제공.

## 본 바이너리 모듈 (`src/`)

ports-and-adapters 배치:

| 모듈 | 역할 |
|------|------|
| `boot/` | `fn main` 부팅 시퀀스(`run()` 진입점) — event_loop, headless_{dispatch,stream,plugins}, cli_routing, wiring, locale, trace(부팅 계측) |
| `app/` | `App`(winit `ApplicationHandler`) — window_lifecycle, boot_machine(첫 윈도우 부팅 상태 머신 — [boot-sequence](boot-sequence.md)), shutdown_cascade(종료 cascade — [shutdown-sequence](shutdown-sequence.md)), modal, ipc dispatch, attach, dispatch_domain(workspace close cascade — [close-sequence](close-sequence.md)) |
| `core/` | **도메인 본체**(`Core`) — state, session, attach, agent, terminal_store, ipc_facade |
| `hub.rs` | **외부 통신**(`Hub`) — IPC 서버, 포트 파일 |
| `view/` | **GUI**(gui-gated) — `View` sealed trait 계층 + MainView/SettingsView/QuitView/PluginsView/PresetView. — [multi-window](multi-window.md) |
| `state/` | `AppState` — MainView 당 1개 런타임 상태(focus/layout/mouse/mark/restore) |
| `gfx/` | GPU — `GpuState`, renderer(셀 렌더), screenshot, perf. — [gpu-rendering](../dev-guide/gpu-rendering.md) |
| `adapters/` | 외부 경계 구현 — `ui`(egui 컴포넌트·popup), `ipc`(handler), `production`/`test`(port 구현체), `cli`, `plugin` |
| `ports/` | **의존성 역전 trait** — ipc_server, clipboard, clock, fs, home, process, notification_sound (production/test adapter 가 구현 → headless·테스트 교체) |
| `intent/` | **Intent 큐** — 호스트 내부 동작 디스패치. — [action-dispatch](../design/flows/action-dispatch.md) |
| `host_api/` | 호스트가 외부(plugin/agent)에 제공하는 인터페이스 — hooks, webview |
| `plugin_bridge/` | 호스트 측 plugin 라우팅 facade |
| `store/` | 인메모리 스토어 — notification, recent_files |
| `db/` | SQLite `state.db`. — [storage](../design/systems/storage.md) |
| `file/` · `clipboard/` · `platform/` | 파일 핸들러/포맷 · 클립보드 · 플랫폼별(crash_report·native menu 등) |

## 데이터 흐름

주요 흐름 5종(키 입력→렌더, PTY 출력→파싱→렌더, IPC 요청→응답, 알림, 설정 로드→적용)의 단계별 경로는 [data-flows](data-flows.md). 호스트 내부 동작이 Intent 큐로 통일된 디스패치 모델은 [action-dispatch](../design/flows/action-dispatch.md).

## 하위 문서

| 문서 | 설명 |
|------|------|
| [boot-sequence](boot-sequence.md) | 첫 윈도우 부팅 상태 머신(BootPhase) — hidden 생성→로딩 프레임→표시, 프레임 구동 대기, 부팅 계측(T1~T7) |
| [shutdown-sequence](shutdown-sequence.md) | 종료 cascade(layout flush→surface close→plugin 종료) + `event_loop.exit()` 이후 Drop tail, 종료 계측(S1~S5) |
| [close-sequence](close-sequence.md) | 워크스페이스 close 경로 3종(gui/inline/cascade) + close 계측(C1~C5) 과 실측 기준선 |
| [multi-window](multi-window.md) | App = Core/Hub/ViewRegistry, Window trait 계층, 모달 불변식, 단일 프로세스 근거 |
| [input-layer](input-layer.md) | 마우스 입력 z-order 계층 — 소비/버블링 + 커서 결정 |
| [data-flows](data-flows.md) | 주요 데이터 흐름 (파일+함수 기준) |
| [ui-widgets-crate](ui-widgets-crate.md) | `tasty-ui-widgets` — 본체·갤러리 공유 UI primitive |
| [invariants/](invariants/index.md) | 깨지면 안 되는 시스템 약속 (surface-cwd 등) |

결정의 *근거/대안/재검토 조건*(보류 결정 포함)은 [ADR](../adr/index.md).
