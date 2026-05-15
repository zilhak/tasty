# 아키텍처 개요

Tasty는 Cargo 워크스페이스 기반 크로스 플랫폼 GPU 가속 터미널 에뮬레이터다.
본 바이너리(`src/`)와 14개의 라이브러리 크레이트(`crates/*`)로 구성된다.

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

| 크레이트 | 책임 |
|----------|------|
| `tasty` (본 바이너리, `src/`) | 윈도우/Engine/Window 계층, UI/GPU, IPC 라우터, CLI |
| `tasty-core` | 공용 데이터 모델 (`model`, `theme`, `i18n`, `paths`, `color`) |
| `tasty-settings` | 설정 스키마/직렬화 (appearance/keybindings/general/...) |
| `tasty-font` | 폰트 atlas, 글리프 래스터라이징, 내장 D2Coding |
| `tasty-terminal` | PTY + termwiz VTE 래퍼 |
| `tasty-hooks` | Surface Hook 매니저 (process-exit, output-match, idle-timeout 등) |
| `tasty-shm` | 크로스 플랫폼 공유 메모리 + 핸들 전달 |
| `tasty-plugin-protocol` | 호스트↔plugin 와이어 프로토콜 (envelope, 메서드 enum) |
| `tasty-plugin-sdk` | 외부 plugin 제작용 SDK (Plugin trait, transport, snapshot 헬퍼) |
| `tasty-plugin-claude` | 번들 plugin: Claude Code (claude.* IPC/CLI, hook 4종 설치) |
| `tasty-plugin-codex` | 번들 plugin: Codex CLI (codex.* IPC/CLI) |
| `tasty-plugin-image` | 번들 plugin: 이미지 뷰어 surface kind (`rendering = "host"`) + image.* IPC |
| `tasty-plugin-explorer` | 번들 plugin: 파일 탐색기 surface kind |
| `tasty-plugin-clipboard-history` | 번들 plugin: 클립보드 히스토리 (tool.clipboard.*) |
| `tasty-tui-simulator` | E2E TUI 테스트용 시뮬레이터 |

본 바이너리는 `pub use tasty_core::{model, theme, i18n, paths};`,
`pub use tasty_settings as settings;`, `pub use tasty_font as font;` 식으로
재수출하므로 `crate::model::X` 같은 기존 경로가 그대로 동작한다.

## 본 바이너리 모듈 (`src/`)

```
src/
├── main.rs                 # 진입점, App 구조체, 일부 IPC dispatch (window.*, system.shutdown,
│                           #   debug.info, ui.screenshot, surface.ime_* 등)
├── event_handler.rs        # winit ApplicationHandler impl
├── engine.rs               # Engine (IPC 서버, 윈도우 ID 관리)
├── engine_state.rs         # EngineState (공유 상태: 워크스페이스/설정/훅/알림)
├── waker_factory_winit.rs  # winit EventLoopProxy 기반 Waker 팩토리
│
├── state/                  # AppState (윈도우당 1개) — workspace/tab/pane/focus/layout/mouse/mark/restore/message
├── window/                 # Window sealed trait + 구현체
│   ├── mod.rs              # Window/Modality/WindowAction
│   ├── base.rs             # WindowBase 공통 필드
│   ├── modal.rs / terminal_host.rs   # supertrait
│   ├── settings.rs / quit.rs / plugins.rs  # 모달 윈도우
│   └── main/               # MainWindow (TerminalHostWindow): keyboard/mouse/ime/selection/redraw/clipboard
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
├── notification.rs         # NotificationStore + OS 알림
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
├── window/                          ← Window 트레잇 + 구현체
│   ├── main/                        ← MainWindow (TerminalHostWindow)
│   │   ├── gpu/ → renderer/ → tasty-font
│   │   ├── ui/ → state/
│   │   └── shortcuts
│   ├── settings.rs / quit.rs / plugins.rs   ← ModalWindow
│   └── modal.rs / terminal_host.rs / base.rs
├── plugin/                          ← plugin 호스트 (manifest/process/manager/registry/...)
├── ipc/                             ← IPC 서버 + handler/
└── cli/                             ← CLI 클라이언트 (GUI와 독립)

workspace 크레이트:
tasty-core ← (의존 없음, 다른 거의 모든 크레이트의 base)
tasty-settings ← tasty-core
tasty-font ← tasty-core
tasty-terminal ← tasty-core
tasty-hooks ← tasty-core
tasty-shm ← (OS API만)
tasty-plugin-protocol ← tasty-core, tasty-shm
tasty-plugin-sdk ← tasty-plugin-protocol, tasty-shm
tasty-plugin-{claude,codex,image,explorer,clipboard-history}
              ← tasty-plugin-sdk
tasty (binary) ← 모든 위 크레이트
```

순환 의존 없음. tasty-core가 최하위, 본 바이너리가 최상위.

## 데이터 흐름

1. **키보드 입력 → 화면**: winit KeyEvent → MainWindow → shortcuts/send_key → tasty-terminal → PTY → 리더 스레드 → EventLoopProxy → CellRenderer → wgpu
2. **PTY 출력 → 렌더링**: PTY 리더 → Terminal::process → termwiz Parser → Surface → CellRenderer::prepare → 2-pass 렌더
3. **IPC 요청 → 응답**: TCP → IpcServer → mpsc → main process_ipc → handler::handle (또는 main.rs 직접 dispatch) → AppState → JsonRpcResponse → TCP
4. **Plugin 호출**: handler/plugin.rs → plugin/manager → plugin process (stdio) → tasty-plugin-sdk → 응답
5. **알림**: Terminal 이벤트 → NotificationStore → 사이드바 배지 + 알림 패널

## 코드 규모

본 바이너리 약 160개 `.rs` (~49k 줄) + 워크스페이스 크레이트 78개 `.rs` (~24k 줄).
정확한 수치는 `find src crates -name '*.rs' -not -path '*/target/*' | wc -l` 참조.

## 하위 문서

| 문서 | 설명 |
|------|------|
| [모듈별 상세](modules.md) | 디렉토리 모듈별 책임, 설계 목적, 한계 |
| [데이터 흐름](data-flows.md) | 주요 데이터 흐름 (파일+함수 기준 참조) |
| [리팩토링 분석](refactoring.md) | 남아있는 개선 가능성, 우선순위별 로드맵 |
| [라이브러리 분리 분석](library-separation/index.md) | 크레이트 분리 후보 다관점 분석 |
