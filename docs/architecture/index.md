# 아키텍처 개요

tasty 는 Cargo 워크스페이스 기반 크로스 플랫폼 GPU 가속 터미널 에뮬레이터다. **본 바이너리(`src/`) + 40 개 라이브러리 크레이트(`crates/*`)** 로 구성되며, **ports-and-adapters(헥사고날) + headless core** 로 layering 된다 — 도메인 로직은 GUI 없이도 동작하고, GUI/IPC/OS 연동은 교체 가능한 adapter 뒤에 있다.

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
| `core: Core` | **도메인 본체** — 워크스페이스/탭/패인/서피스 상태, 세션, attach, registries | 항상 |
| `hub: Hub` | **외부 통신** — IPC 서버(`Option<Box<dyn IpcServerPort>>`), 포트 파일 | 항상 |
| `view: ViewRegistry` | **GUI 어댑터** — winit proxy, `views: HashMap<WindowId, Box<dyn View>>`, `active_modal_id`/`focused_view_id` | `#[cfg(feature = "gui")]` |

> 옛 `Engine` struct 는 삭제됐고 필드가 Core/Hub/View 로 분산됐다. `src/engine/` 모듈명은 일부 sub-module(`surface_registry` / `command_index` / `output_observer` / `layout_persistence`)의 전환기 컨테이너로 잠시 남아 있다.

## 워크스페이스 크레이트 (40)

의존은 아래 계층 순서로만 흐른다(상위 → 하위). 순환 없음.

### type-\* / primitive (leaf)
`tasty-type-geometry`(길이·도형: `LogicalPx`/`PhysicalPx`/`Rect`, 의존 0) · `tasty-type-appearance`(색·테마 schema, → type-geometry) · `tasty-utils`(path helper, leaf)

type-\* 끼리만 의존 가능. 도메인/IO crate 의존 금지(그룹 내 순환도 금지). — [typed-length](../concepts/typed-length.md)

### 도메인-IO
`tasty-themes`(전역 Theme + TOML IO) · `tasty-settings`(설정 스키마/직렬화) · `tasty-font`(글리프 atlas) · `tasty-terminal`(PTY + termwiz) · `tasty-hooks`(Surface Hook) · `tasty-memory`(에이전트 메모리 `memory.db`) · `tasty-telemetry`(→ memory) · `tasty-output`(출력 파서 카탈로그) · `tasty-approval`(approval 게이트) · `tasty-agent`(세션/lifecycle, → memory) · `tasty-presets`(레이아웃 프리셋) · `tasty-shm`(공유 메모리) · `tasty-portscan` · `tasty-lua`(Lua 스크립트 — 워커 격리 + 고정 host API, ADR-0031) · `tasty-i18n`(번역) · `tasty-ssh-profiles`(SSH 프로필) · `tasty-model`(도메인 모델 — workspace/pane/tab/surface, → terminal/type-geometry/utils, GUI-free)

type-\* + 다른 도메인-IO 만 의존 가능.

### UI primitive
`tasty-egui-theme`(Theme → egui Visuals/Style 어댑터) · `tasty-ui-widgets`(본체·갤러리 공유 egui 위젯/레이아웃 primitive — 시각 동기화 단일 출처). — [ui-widgets-crate](ui-widgets-crate.md)

### plugin host (IPC 인프라)
`tasty-plugin-manifest`(manifest 스키마/파서) · `tasty-ipc`(JSON-RPC envelope + caller + audit + method_meta + facade trait) · `tasty-host-plugin`(호스트의 plugin 매니저/process/event_bus/registry)

### plugin protocol / SDK (sandbox 경계)
`tasty-plugin-protocol`(호스트↔plugin 와이어, leaf) · `tasty-plugin-sdk`(외부 plugin 제작 SDK, → protocol/shm) · `tasty-plugin-sdk-wasm`(WASM 타깃 SDK)

이 계층은 도메인-IO 에 **직접 의존하지 않는다**(sandbox 경계) — protocol/sdk 만 통과.

### 번들 plugin (모두 `tasty-plugin-sdk` 의존)
`tasty-plugin-claude` · `-codex` · `-explorer` · `-git-viewer` · `-clipboard-viewer` · `-image` · `-html` · `-markdown`(+ manifest). — [concepts/plugins](../concepts/plugins.md)

### CLI client
`tasty-cli`(clap CLI — request/format/transport/dynamic plugin subcommand. → ipc/host-plugin/terminal/approval/ssh-profiles)

### 도구 / standalone
`tasty-tui-simulator`(E2E TUI 시뮬레이터, lib + `tasty-tui-sim` binary — 로직은 lib 공유, debug 빌드에선 `tasty debug sim` 으로도 노출) · `tasty-gallery`(ui-widgets 데모 바이너리, `cargo run -p tasty-gallery` — 본체 빌드와 분리)

### 본 바이너리 (`tasty`)
위 크레이트를 의존하며 App/View/GPU/IPC 라우터/부팅을 제공.

## 본 바이너리 모듈 (`src/`)

ports-and-adapters 배치:

| 모듈 | 역할 |
|------|------|
| `boot/` | `fn main` 부팅 시퀀스(`run()` 진입점) — event_loop, headless_dispatch, cli_routing, wiring, locale |
| `app/` | `App`(winit `ApplicationHandler`) — window_lifecycle, modal, ipc dispatch, attach, dispatch_domain |
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
| [multi-window](multi-window.md) | App = Core/Hub/ViewRegistry, Window trait 계층, 모달 불변식, 단일 프로세스 근거 |
| [input-layer](input-layer.md) | 마우스 입력 z-order 계층 — 소비/버블링 + 커서 결정 |
| [data-flows](data-flows.md) | 주요 데이터 흐름 (파일+함수 기준) |
| [ui-widgets-crate](ui-widgets-crate.md) | `tasty-ui-widgets` — 본체·갤러리 공유 UI primitive |
| [invariants/](invariants/index.md) | 깨지면 안 되는 시스템 약속 (surface-cwd 등) |

결정의 *근거/대안/재검토 조건*(보류 결정 포함)은 [ADR](../adr/index.md).
