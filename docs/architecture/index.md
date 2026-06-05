# 아키텍처 개요

Tasty는 Cargo 워크스페이스 기반 크로스 플랫폼 GPU 가속 터미널 에뮬레이터다.
본 바이너리(`src/`)와 33개의 라이브러리 크레이트(`crates/*`)로 구성된다.

## 기술 스택

| 역할 | 라이브러리 |
|------|-----------|
| 윈도우/입력 | winit |
| GPU 렌더링 | wgpu |
| UI 위젯 | egui + egui-wgpu + egui-winit |
| VTE 파싱 | termwiz |
| PTY | portable-pty (ConPTY/Unix) |
| 폰트 래스터라이징 | cosmic-text + swash |
| IPC 프로토콜 | serde_json (JSON-RPC 2.0) |
| CLI | clap |
| 설정 파일 | toml + directories |
| OS 알림 | notify-rust |
| 공유 메모리 | tasty-shm (자체) — POSIX shm + SCM_RIGHTS / Windows DuplicateHandle |

## 워크스페이스 크레이트

크레이트 분류는 4 계층 + 테스트 도구 계층:

### type-\* layer (leaf, primitive/schema)

| 크레이트 | 책임 |
|----------|------|
| `tasty-type-geometry` | 길이/도형 primitive (LogicalPx, PhysicalPx, Rect 등). 의존 0 (serde 만) |
| `tasty-type-appearance` | appearance schema/primitive: HexColor/GpuRgba/GpuRgb, ThemeColors/PartialColors/ThemeSizing/Theme + impl, SurfaceTheme/PartialSurfaceTheme + FALLBACK_SURFACE, derive_overlays |
| `tasty-utils` | 공용 path helper (directories). leaf |

type-\* 끼리만 의존 가능 (`type-appearance → type-geometry`). 도메인/IO crate 의존 금지.

### 도메인-IO layer

| 크레이트 | 책임 |
|----------|------|
| `tasty-themes` | 빌트인 mocha fallback const, 전역 `RwLock<Theme>` + `theme()/set_theme()`, `ThemeApplyContext` trait, TOML 로딩/스캔/저장 |
| `tasty-settings` | 설정 스키마/직렬화 (appearance/keybindings/general/...) |
| `tasty-font` | 폰트 atlas, 글리프 래스터라이징, 내장 D2Coding (cosmic-text + wgpu) |
| `tasty-terminal` | PTY + termwiz VTE 래퍼 (cross-platform pty) |
| `tasty-hooks` | Surface Hook 매니저 (process-exit, output-match, idle-timeout 등) |
| `tasty-memory` | 에이전트 영속 키-값 저장소 (`~/.tasty/memory.db`, SQLite WAL) |
| `tasty-telemetry` | 메트릭/이벤트 추적 (memory 의존) |
| `tasty-output` | 출력 파서 카탈로그 (10 종 빌트인) |
| `tasty-approval` | Approval 게이트 도메인 |
| `tasty-agent` | 에이전트 세션/lifecycle |
| `tasty-presets` | preset 정의/저장 |
| `tasty-shm` | 크로스 플랫폼 공유 메모리 + 핸들 전달 |
| `tasty-portscan` | 포트 스캐너 (포트 사용 감지) |
| `tasty-update` | 업데이트 체커 (ureq + semver) |
| `tasty-lua` | Lua sandbox (file_format detector, mlua) |
| `tasty-model` | workspace / pane / pane_tree / surface_layout / tab / surface_trait / terminal_surface / popup_kind / toast_kind / closed_item / empty_surface / image_panel / markdown_panel 도메인 (G.E, 16 파일 / 3,719 LOC). headless / GUI 양쪽 빌드 정상 동작 |
| `tasty-ipc` | JSON-RPC 2.0 envelope + alias + caller + audit + method_meta + port_file + session. plugin host facade trait (`IpcHostFacade`) 외부화 (Phase F.B) |
| `tasty-plugin-manifest` | manifest 스키마 / 파서 — `SurfaceKindDecl` (+ `default_colors`), `[[contributes.*]]`, permissions, `CliSubcommandDecl` (+ `[polling]`) (Phase F.B + F.H) |
| `tasty-host-plugin` | 호스트의 plugin 매니저 / process / handle_channel / discovery / event_bus / registry / remote_kind 등 17 모듈. `plugin-protocol` + `plugin-manifest` + `terminal` + `shm` 의존 (Phase F.B) |

### Plugin layer

| 크레이트 | 책임 |
|----------|------|
| `tasty-plugin-protocol` | 호스트↔plugin 와이어 프로토콜 (envelope, 메서드 enum) |
| `tasty-plugin-sdk` | 외부 plugin 제작용 SDK (Plugin trait, transport, snapshot 헬퍼). `plugin-protocol + shm` 의존 |

### 번들 Plugin layer (모두 `tasty-plugin-sdk` 의존)

| 크레이트 | 책임 |
|----------|------|
| `tasty-plugin-claude` | Claude Code (claude.* IPC/CLI, hook 4종 설치) |
| `tasty-plugin-codex` | Codex CLI (codex.* IPC/CLI) |
| `tasty-plugin-image` | 이미지 뷰어 surface kind (`rendering = "host"`) + image.* IPC |
| `tasty-plugin-html` | HTML 뷰어 surface kind (얇은 wrapper) |
| `tasty-plugin-explorer` | 파일 탐색기 surface kind |
| `tasty-plugin-clipboard-history` | 클립보드 히스토리 (tool.clipboard.*) |
| `tasty-plugin-git-viewer` | git diff/log 뷰어 |

### CLI client layer

| 크레이트 | 책임 |
|----------|------|
| `tasty-cli` | clap 기반 CLI 클라이언트 (request / format / transport / dynamic plugin subcommand / polling loop). 호스트 IPC port 파일 (`~/.tasty/tasty.port`) 만 의존 (Phase F.B) |

### 테스트/dev 도구

| 크레이트 | 책임 |
|----------|------|
| `tasty-tui-simulator` | E2E TUI 테스트용 시뮬레이터 (crossterm + clap, binary 산출) |

### 본 바이너리 (`src/`)

`tasty` 본 바이너리는 위 33 크레이트를 직접 의존하며 윈도우/Engine/Window 계층, UI/GPU,
IPC 라우터, CLI 를 제공한다. F.B 후 `src/cli/` / `src/ipc/` / `src/host_api/plugin_manifest/`
가 별도 crate 로 빠졌고 G.E 후 `src/model/` 도 `tasty-model` 로 분리됐다. 본 바이너리 잔존
모듈 (`src/i18n.rs`, `src/waker.rs`, `src/core/agent/`, `src/file/paths.rs` 등 *옛 분리
계획에서 "GUI-free 공용 도메인" crate 후보* 였던 모듈) 은 별도 crate 로 분리되지 않고
*본 바이너리 안에 그대로 존재* 한다 (옛 계획의 가공 crate 명은 [`library-separation/index.md`](library-separation/index.md) 참조).
`src/plugin_bridge/` 는 호스트 측 plugin 라우팅 facade 로 잔존.

## 본 바이너리 모듈 (`src/`)

> **참고**: 아래 트리는 부분적으로 옛 디렉토리 구조 기반이다. 현재는
> `src/app/`, `src/core/`, `src/adapters/`, `src/view/`, `src/gfx/`,
> `src/host_api/`, `src/engine/`, `src/store/`, `src/intent/` 등으로 layering 됨.
> 정확한 디렉토리별 책임은 [`modules.md`](modules.md) 참조 (모듈별 상세는 별도 갱신 작업 대상).

```
src/
├── main.rs                 # 진입점, App 구조체, 일부 IPC dispatch (window.*, system.shutdown,
│                           #   debug.info, ui.screenshot, surface.ime_* 등)
├── event_handler.rs        # winit ApplicationHandler impl
├── engine.rs               # Engine (IPC 서버, 윈도우 ID 관리)
├── engine_state.rs         # EngineState (공유 상태: 워크스페이스/설정/훅/알림)
├── waker_factory_winit.rs  # winit EventLoopProxy 기반 Waker 팩토리
│
├── state/                  # AppState (MainView 당 1개) — workspace/tab/pane/focus/layout/mouse/mark/restore/message
├── view/                   # View sealed trait + 구현체
│   ├── mod.rs              # Modality/ViewAction/ViewCtx/ViewRegistry/unbox_main
│   ├── ui.rs               # View sealed trait
│   ├── base.rs             # ViewBase 공통 필드
│   ├── modal.rs / terminal_host.rs / editor.rs   # supertrait
│   ├── settings.rs / quit.rs / plugins.rs  # ModalView 구현체
│   ├── preset.rs           # EditorView 구현체 (PresetView)
│   └── main.rs (+ main/)   # MainView (TerminalHostView): keyboard/mouse/ime/selection/redraw/clipboard
│
├── gpu/                    # GPU 상태 (GpuState/render_pass/egui_bridge/fonts/screenshot/shell_setup/
│                           #   canvas_prepare/canvas_texture)
├── renderer/               # wgpu 셀 렌더러 (pipeline/line_render/shaders/palette/types)
├── ui/                     # egui UI 컴포넌트 (sidebar/tab_bar/notification/dialog/divider/popup/
│                           #   toast/search_bar/tools_menu/file_open_popup/image_view/markdown_view/
│                           #   notification_popup/info_modal/font_registry/popup_defs/convert_popup/
│                           #   layout_context/egui_panels)
├── settings_ui/            # 설정 윈도우 UI (mod/tabs/keybindings_tab)
├── plugins_ui/             # plugin 관리 윈도우 UI
│
├── plugin/                 # plugin 호스트 — manifest/discovery/manager/process/protocol/listener/
│                           #   command_registry/extension_registry/tool_registry/event_bus/event_throttle/
│                           #   host_actions/host_cmd/host_rendered_kind/ipc_namespace/key_dispatch/
│                           #   popup_render/registry_state/remote_kind/remote_surface/ui_tree(_render)/
│                           #   builtin/handle_channel
│
├── cli/                    # CLI 클라이언트 (mod/request/format/transport/dynamic/plugin)
├── ipc/                    # IPC 서버 + 핸들러
│   ├── mod.rs / server.rs / protocol.rs / method_meta.rs / caller.rs / alias.rs
│   └── handler/
│       ├── mod.rs          # 라우터 (route_engine / route_gui / route_debug)
│       ├── workspace.rs / pane.rs / tab.rs / surface.rs
│       ├── hooks.rs / notification.rs / message.rs / meta.rs
│       ├── plugin.rs / popup.rs / image.rs / ime.rs / tool.rs
│       ├── clipboard.rs / input_source.rs
│
├── storage/                # 영구 저장소 (mod/migrations)
├── surface_registry/       # Surface kind 레지스트리 (mod/builtins)
├── native_menu/            # 네이티브 컨텍스트 메뉴 (linux/macos/windows)
├── webview/                # WebView surface 래퍼 (linux/macos/windows)
├── file_drag/              # 파일 드래그 (linux/macos/windows)
│
├── shortcuts.rs            # 키보드 단축키 매칭 + 실행
├── theme.rs / theme_bridge.rs  # Catppuccin Mocha 테마 / egui 어댑터
├── selection.rs            # 텍스트 선택 좌표 정규화
├── click_cursor.rs         # 클릭→커서 이동
├── double_tap.rs           # 더블탭 수식키
│                           # notification: `src/store/notification.rs` (NotificationStore) +
│                           #   `src/adapters/ui/notification.rs` (OS 알림) +
│                           #   `src/adapters/ipc/handler/notification.rs` (IPC) +
│                           #   `src/view/settings/ui/tabs/notifications.rs` (UI) — 분산
├── global_hooks.rs         # GlobalHookManager (타이머/파일 감시)
├── surface_meta.rs         # Surface별 메타데이터 저장소
├── crash_report.rs         # 크래시 리포트 수집
├── debug_info.rs           # debug.info IPC 응답
├── clipboard_history.rs    # 클립보드 히스토리 백엔드
├── recent_files.rs / jump_list.rs / search_state.rs / layout_persistence.rs
├── markdown_ui.rs / html_ui.rs / image_ui.rs / empty_ui.rs / terminal_link.rs
├── app_icon.rs / macos_delegate.rs / system_tray.rs
└── ...
```

## 모듈 의존성 (DAG)

```
main.rs
├── engine / engine_state           ← IPC 서버, 공유 상태
├── view/                            ← View 트레잇 + 구현체
│   ├── main/                        ← MainView (TerminalHostView)
│   │   ├── gpu/ → renderer/ → tasty-font
│   │   ├── ui/ → state/
│   │   └── shortcuts
│   ├── settings.rs / quit.rs / plugins.rs   ← ModalView
│   └── modal.rs / terminal_host.rs / base.rs
├── plugin/                          ← plugin 호스트 (manifest/process/manager/registry/...)
├── ipc/                             ← IPC 서버 + handler/
└── cli/                             ← CLI 클라이언트 (GUI와 독립)

workspace 크레이트 (4 계층 + 테스트 도구):

# type-* layer (primitive/schema, leaf)
tasty-type-geometry  ← (외부 deps 만: serde)
tasty-type-appearance ← tasty-type-geometry
tasty-utils          ← (외부 deps 만: directories)
   ※ type-* 끼리만 의존 가능. 도메인/IO crate 의존 금지. 그룹 내 순환 금지.

# 도메인/IO layer
tasty-themes    ← tasty-type-appearance, tasty-utils      (전역 Theme + TOML IO)
tasty-settings  ← tasty-themes, tasty-type-appearance, tasty-type-geometry, tasty-utils
tasty-font      ← (외부 deps 만: cosmic-text, wgpu)
tasty-terminal  ← (외부 deps 만: termwiz, portable-pty, unicode-width + cfg-별 libc/windows)
tasty-hooks     ← (외부 deps 만: regex)
tasty-memory    ← (외부 deps 만: rusqlite, directories)
tasty-telemetry ← tasty-memory
tasty-output    ← (외부 deps 만: regex, serde)
tasty-approval  ← (외부 deps 만)
tasty-agent     ← tasty-memory, tasty-utils
tasty-presets   ← tasty-utils
tasty-shm       ← (cfg-별 libc/windows-sys)
tasty-portscan  ← (cfg-별 windows-sys)
tasty-update    ← (외부 deps 만: ureq, semver)
tasty-lua       ← (외부 deps 만: mlua)
tasty-model     ← tasty-terminal, tasty-type-geometry, tasty-utils   (G.E 신규)
tasty-ipc       ← tasty-plugin-protocol (facade trait), serde_json   (F.B 신규)
tasty-plugin-manifest ← (외부 deps 만: serde, toml)                 (F.B 신규)
tasty-host-plugin     ← tasty-plugin-protocol, tasty-plugin-manifest, tasty-terminal, tasty-shm  (F.B 신규)

# Plugin layer
tasty-plugin-protocol ← (외부 deps 만: serde)
tasty-plugin-sdk      ← tasty-plugin-protocol, tasty-shm

# 번들 Plugin layer (모두 tasty-plugin-sdk 의존)
tasty-plugin-claude  ← tasty-plugin-sdk
tasty-plugin-codex   ← tasty-plugin-sdk
tasty-plugin-image   ← tasty-plugin-sdk
tasty-plugin-html    ← tasty-plugin-sdk
tasty-plugin-explorer ← tasty-plugin-sdk
tasty-plugin-clipboard-history ← tasty-plugin-sdk
tasty-plugin-git-viewer ← tasty-plugin-sdk

# CLI client layer
tasty-cli           ← clap, serde_json (호스트 IPC port 파일 의존)         (F.B 신규)

# 테스트/dev 도구
tasty-tui-simulator  ← (외부 deps 만: crossterm, clap, binary 산출)

tasty (binary) ← 모든 위 크레이트
```

순환 의존 없음. type-\* 가 leaf 그룹. 도메인-IO layer 는 type-\* 만 / 다른 도메인-IO 만 의존 가능
(예: `agent → memory`, `telemetry → memory`, `themes → type-appearance`). Plugin layer 는 도메인-IO 와
*직접* 의존하지 않고 `plugin-protocol` / `plugin-sdk` 만 통과 (sandbox 경계). 본 바이너리가 최상위.

## 데이터 흐름

1. **키보드 입력 → 화면**: winit KeyEvent → MainView → shortcuts/send_key → tasty-terminal → PTY → 리더 스레드 → EventLoopProxy → CellRenderer → wgpu
2. **PTY 출력 → 렌더링**: PTY 리더 → Terminal::process → termwiz Parser → Surface → CellRenderer::prepare → 2-pass 렌더
3. **IPC 요청 → 응답**: TCP → IpcServer → mpsc → main process_ipc → handler::handle (또는 main.rs 직접 dispatch) → AppState → JsonRpcResponse → TCP
4. **Plugin 호출**: handler/plugin.rs → plugin/manager → plugin process (stdio) → tasty-plugin-sdk → 응답
5. **알림**: Terminal 이벤트 → NotificationStore → 사이드바 배지 + 알림 패널

## 코드 규모

실측 (2026-06-03, F.B / G.E 후): 본 바이너리 394 `.rs` (~69k 줄) + 워크스페이스 크레이트 270 `.rs` (~63k 줄). F.B (cli / ipc / plugin manifest / host-plugin 4 crate 이동) + G.E (model 분리, 16 파일 / 3,719 LOC) 로 본 바이너리 LOC 가 약 22k 감소, 워크스페이스 crate 가 그만큼 증가.
재측정: `find src -name '*.rs' | wc -l` / `find src -name '*.rs' -print0 | xargs -0 wc -l | tail -1` /
`find crates -name '*.rs' -path '*/src/*' | wc -l` 식으로 산출.

## 하위 문서

| 문서 | 설명 |
|------|------|
| [모듈별 상세](modules.md) | 디렉토리 모듈별 책임, 설계 목적, 한계 |
| [데이터 흐름](data-flows.md) | 주요 데이터 흐름 (파일+함수 기준 참조) |
| [리팩토링 분석](refactoring.md) | 남아있는 개선 가능성, 우선순위별 로드맵 |
| [라이브러리 분리 분석](library-separation/index.md) | 크레이트 분리 후보 다관점 분석 |
| [Plugin categories](plugin-categories.md) | host-native / bundled plugin / user plugin 3 카테고리 분류 정책 + 기존 "builtin" 표현 매핑 |
| [Plugin sandbox 평가](plugin-sandbox-evaluation.md) | WASM / OS-level / 현 상태 비교 — 0.7 보류 근거 + 재검토 trigger |
| [성능 벤치마크](performance-benchmarks.md) | F.G GPU 최적화 실측 — terminals_ms p50/p99/max + draw call 수 + atlas eviction 카운터 (10 surface ASCII / CJK 4 surface, release / dist 프로필) |
