# 모듈별 상세

디렉토리 모듈 단위로 각 모듈의 책임과 구조를 기술한다.

---

## model/ — 데이터 모델 (`tasty-model`, G.E)

**책임:** Workspace → PaneNode → Pane → Tab → SurfaceLayout의 계층 데이터 구조 정의. 레이아웃 계산(Rect 분할, 디바이더 탐색), 터미널 순회, 리사이즈.

**GUI-free.** `tasty-model` 크레이트 (G.E, `crates/tasty-model/` 16 파일 / 3,719 LOC) 는 egui/wgpu/image 등 GUI 라이브러리는 물론, 색/시각 schema (`tasty-type-appearance`) 도 의존하지 않는다. `tasty-terminal` + `tasty-type-geometry` (LogicalPx/PhysicalPx) + `tasty-utils` + `serde_json` + `termwiz` 정도만 참조한다. 본 바이너리 `src/model.rs` 는 `pub use tasty_model::*;` 단 두 줄로 축약 — 옛 callsite 회귀 0. 시각 표현은 `tasty-type-appearance::theme` (schema) + `tasty-themes` (전역 인스턴스 + IO) 가 책임지고, model 은 그 결과를 모른다.

| 파일 | 역할 |
|------|------|
| `mod.rs` | Rect, SplitDirection, DividerInfo 등 공통 타입. compute_terminal_rect() |
| `workspace.rs` | Workspace 구조체. PaneNode 트리를 소유하며 take/put 패턴으로 구조 변경 |
| `pane_tree.rs` | PaneNode 이진 트리 (상위 분할). split/close/rect계산/디바이더탐색/방향포커스 |
| `pane.rs` | Pane 구조체. 탭 관리 (생성/닫기/전환), Terminal 생성, Surface 분할 |
| `tab.rs` | Tab 구조체. `SurfaceLayout`을 직접 소유. 분할/포커스/복원 로직 |
| `surface_trait.rs` | `Surface` trait 정의. `kind()`(소문자 식별자) + `type_name()`(표시용) 분리, `html_url()` 안정 메서드, downcast 메서드 포함. GUI 의존 없음 |
| `terminal_surface.rs` | TerminalSurface (단일 터미널). Surface impl |
| `surface_layout.rs` | SurfaceLayout 이진 트리 (하위 분할) + SurfaceRegion. pane_tree.rs와 동일한 패턴 |
| `markdown_panel.rs` | MarkdownPanel. Surface impl. **`file_path` + mtime poll만 보유** — 콘텐츠/스크롤/CommonMark 캐시는 호스트 `MarkdownView`로 분리 |
| `html_panel.rs` | HtmlPanel. Surface impl. URL만 보유. `Surface::html_url()` 노출로 native WebView 동기화 가능 |
| `image_panel.rs` | ImagePanel. Surface impl. **`file_path`, `dir_images`, `current_index`만 보유** — 픽셀/텍스처/편집 상태는 호스트 `ImageView`로 분리 |
| `empty_surface.rs` | EmptySurface. Surface impl. 변환 버튼 표시 |
| `tests.rs` | Rect/PaneNode/SurfaceLayout 유닛 테스트 |

`Surface::kind()`는 호스트 빌트인으로 `"terminal"`, `"html"`, `"empty"` 3종을 반환하고, `"image"`는 빌트인 `com.tasty.image` plugin이, `"markdown"`은 빌트인 `com.tasty.markdown` plugin이 (둘 다 `rendering = "host"` whitelist 경유 등록), `"explorer"`/클립보드 viewer popup 등은 다른 plugin이 hello 시점에 추가한다. plugin이 등록하는 `RemoteSurface`는 plugin이 선언한 kind를 그대로 노출한다. IPC/registry/플러그인이 이 값으로 surface 타입을 식별하며, `type_name()`은 표시 전용이라 식별 비교에 쓰면 안 된다.

### Model + Host View 분리 패턴

GUI 휘발성 상태(콘텐츠 캐시, 텍스처, 편집 세션, 스크롤 오프셋, 팝업 버퍼 등)는 모델에 두지 않고 호스트 측 **View** 구조체에 둔다. 각 View는 surface ID를 키로 하는 **Store** (`HashMap<SurfaceId, View>`)에 보관되며, surface가 닫힐 때 `AppState::cleanup_surface(sid)`가 모든 store에서 `drop_view(sid)`를 호출한다.

| Model | View | Store 위치 |
|-------|------|-----------|
| `MarkdownPanel` (file_path + mtime) | `MarkdownView` (content, scroll_offset, commonmark_cache) | `AppState::markdown_views` |
| `ImagePanel` (file_path, dir_images) | `ImageView` (original_image, texture, edit_state, undo history, brush, popup buffers) | `AppState::image_views` |
| `HtmlPanel` (url) | (없음 — 모델이 충분히 슬림. native WebView는 `MainView::webviews`에서 직접 관리) | — |

이 분리로 모델은 직렬화 친화적인 식별 정보만 남고, GUI 라이브러리(egui)에 묶이지 않는다 → 향후 플러그인 프로세스에서 동일한 모델을 그대로 사용 가능.

pane_tree.rs와 surface_layout.rs는 재귀 이진 트리 구조로, `match self { Leaf/Split }` 패턴의 반복이 본질적이다.

---

## state/ — 애플리케이션 상태

**책임:** 윈도우당 1개의 AppState. model/ 위에서 "어떤 워크스페이스가 활성인가", "어떤 서피스에 포커스가 있는가" 등의 런타임 상태를 관리한다.

AppState 구조체를 mod.rs에서 정의하고, 각 서브모듈이 도메인별 `impl AppState` 블록으로 메서드를 추가한다.

| 파일 | 역할 |
|------|------|
| `mod.rs` | AppState 구조체 + SurfaceMessage/ClaudeChildEntry/PaneContextMenu. 기본 접근자 |
| `workspace.rs` | 워크스페이스 생성/전환/닫기 |
| `tab.rs` | 탭 생성/이동/마크다운탭/탐색기탭 |
| `pane.rs` | 패인 분할/닫기, close_surface_by_id (5-case 계단식 닫기) |
| `focus.rs` | 포커스 이동 (순차, 방향별, 마우스 위치 기반) |
| `claude.rs` | Claude 부모-자식 관계 등록/해제/상태 관리 |
| `message.rs` | Surface 간 메시지 송수신 (큐 기반) |
| `layout.rs` | resize_all, surface_regions, update_grid_size |
| `mouse.rs` | 디바이더 탐색/드래그, `winit_cursor_icon_at` (디바이더 위 ResizeHorizontal/Vertical, terminal kind는 Text, 그 외 None) |
| `mark.rs` | Read mark, 타이핑 감지 |
| `tests.rs` | 유닛 테스트 |

state/pane.rs의 close_surface_by_id는 탭 내부 분할→탭→패인→워크스페이스 순서로 계단식 닫기를 수행하는 로직이다.

---

## gpu/ — GPU 상태 관리

**책임:** wgpu 디바이스/서피스, egui 통합, 렌더 오케스트레이션.

mod.rs에 GpuState 구조체와 진입점을 두고, 서브모듈이 역할별 impl 블록을 추가한다.

| 파일 | 역할 |
|------|------|
| `mod.rs` | GpuState 구조체, new(), resize(), render() 오케스트레이션, 접근자 |
| `render_pass.rs` | clear/terminal/egui 3-pass 렌더링 |
| `egui_bridge.rs` | egui 프레임 실행, IME preedit 오버레이, 테마/폰트 변경 후처리 |
| `fonts.rs` | egui CJK 폰트 로딩 (플랫폼별 시스템 폰트 탐색) |
| `screenshot.rs` | wgpu 프레임 캡처 → PNG 저장 |
| `shell_setup.rs` | 셸 경로 확인 다이얼로그 (첫 실행 시) |

GpuState가 egui_ctx, egui_state, wgpu device/queue, CellRenderer를 모두 소유한다.

---

## renderer/ — 셀 렌더러

**책임:** 터미널 셀을 wgpu 인스턴스 데이터로 변환하고 GPU에서 렌더링.

CellRenderer가 GpuState와 분리되어 독립적으로 동작한다.

| 파일 | 역할 |
|------|------|
| `mod.rs` | CellRenderer 구조체, prepare_with_bg, prepare_terminal_viewport, render_scissored |
| `pipeline.rs` | new() + update_font(): wgpu 파이프라인/바인드그룹/버퍼 초기화 |
| `line_render.rs` | render_cell() 공통 로직으로 scrollback/surface 라인 렌더 통합 |
| `shaders.rs` | WGSL 셰이더 소스 (배경 + 글리프) |
| `palette.rs` | ANSI 256색 + TrueColor 팔레트 변환 |
| `types.rs` | Uniforms, BgInstance, GlyphInstance (bytemuck 호환) |

pipeline.rs는 wgpu RenderPipelineDescriptor의 장황한 선언 코드가 대부분이다.

---

## ui/ — egui UI 컴포넌트

**책임:** egui로 그리는 모든 UI. 사이드바, 탭바, 알림 패널, 다이얼로그, egui 기반 Surface 패널(Markdown/Html/Empty/Image + plugin RemoteSurface).

각 파일이 하나의 독립 UI 컴포넌트를 담당한다.

| 파일 | 역할 |
|------|------|
| `mod.rs` | draw_ui() 진입점 — 사이드바 모드 분기 + 터미널 영역 계산 |
| `sidebar.rs` | 축소/전체 사이드바 렌더링 |
| `tab_bar.rs` | 패인별 탭 바 (액션 큐 패턴) |
| `notification.rs` | 알림 패널 (스크롤 목록 + 워크스페이스 점프) |
| `dialog.rs` | 워크스페이스 이름변경 + 마크다운 경로 다이얼로그 |
| `divider.rs` | 분할선 + 서피스 하이라이트 |
| `egui_panels.rs` | egui 기반 Surface 패널 렌더링 (Markdown/Html/Empty/Image/RemoteSurface). `mem::take` 패턴으로 view store를 임시 추출해 모델과 view 동시 mutable 접근 |
| `markdown_view.rs` | `MarkdownView`/`MarkdownViewStore` — content, scroll_offset, commonmark_cache 보관. `get_or_init(panel)`이 mtime poll까지 자동 처리 |
| `image_view.rs` | `ImageView`/`ImageViewStore` — 픽셀, 텍스처, 편집 상태(EditState/DragState/ResizeHandle/FloatingSelection/ActionHistory/StrokeBuilder), brush, popup 버퍼 보관 |
| `popup.rs` / `popup_defs.rs` | `PopupManager` + `PopupDef` 정의. egui::Window 직접 사용 금지, 모든 내부 팝업은 이 시스템으로 |
| `approval_popup.rs` | approval.request 응답 UI (capability elevation 등) |
| `convert_popup.rs` | EmptySurface 변환 메뉴 |
| `drop_overlay.rs` | drag&drop hover overlay |
| `file_handler_picker_popup.rs` | 파일 핸들러 선택 popup (후보/최근 두 열) |
| `file_open_popup.rs` | 파일 열기 popup |
| `font_registry.rs` | 폰트 등록 / 프리뷰 |
| `info_modal.rs` | 정보성 모달 (about 등) |
| `layout_context.rs` | UI 그리기 context 헬퍼 |
| `notification_popup.rs` | 알림 토스트/팝업 변환 |
| `command_palette_popup.rs` | VS Code 스타일 명령 팔레트 — 쿼리 입력 + 후보 리스트 + Enter 실행 |
| `port_scanner_popup.rs` | 활성 surface의 listening TCP 포트 뷰어 (`tasty-portscan` 호출) |
| `search_bar.rs` | 검색 바 UI |
| `toast.rs` | toast 큐 + 렌더 |
| `tools_menu.rs` | 사이드바 도구 메뉴. plugin `[[contributes.tool]]` + 빌트인(`BUILTIN_TOOLS`) prepend |
| `update_popup.rs` | 새 버전 알림 popup (`tasty-update` GitHub Releases 폴러와 연동) |

---

## view/ — View 트레잇 계층과 구현체

**책임:** 모든 View 타입의 trait 계층 정의 및 구현체 관리.

`View` sealed trait을 최상위로 두고, `ModalView`/`TerminalHostView`/`EditorView`
supertrait 으로 계열을 분리한다. 모든 구현체는 `ViewBase` 구조체를 composition 하여
공통 필드를 공유한다. Window 어휘는 winit OS-level concept 에만 사용.

| 파일/디렉토리 | 역할 |
|---------------|------|
| `mod.rs` | `Modality`, `ViewAction`, `ViewCtx`, `ViewRegistry` (proxy + 모든 view HashMap + focused/active_modal id), `unbox_main()` |
| `ui.rs` | `View` sealed trait, `sealed::Sealed` |
| `base.rs` | `ViewBase`: gpu, winit, dirty, modifiers, focused, close_requested 공통 필드 |
| `modal.rs` | `ModalView` supertrait (reveal_after_first_render, on_escape default method) |
| `terminal_host.rs` | `TerminalHostView` supertrait (has_sidebar default method) |
| `editor.rs` | `EditorView` supertrait (modeless 에디터 marker) |
| `settings.rs` | `SettingsView` (impl View + ModalView) |
| `quit.rs` | `QuitView` (impl View + ModalView) |
| `plugins.rs` | `PluginsView` (impl View + ModalView) — plugin manager 모달 |
| `preset.rs` | `PresetView` (impl View + EditorView) — workspace/tab/pane preset 편집기 |
| `main.rs` (+ `main/`) | `MainView` 구조체, new(), `handle_event` dispatch (impl View + TerminalHostView) |
| `main/keyboard.rs` | handle_keyboard_input(), send_key_to_terminal(), handle_ime() |
| `main/mouse.rs` | handle_cursor_moved(), handle_mouse_input(), handle_mouse_wheel() |
| `main/selection.rs` | 텍스트 선택 (다중 클릭 감지, 단어/줄 경계, 그리드 변환) |
| `main/redraw.rs` | handle_redraw(): arrow queue + 터미널 이벤트 + 훅 실행 + 렌더 |
| `main/clipboard.rs` | paste_to_terminal(), 이미지 저장 |
| `main/ime.rs` | IME 핸들러 (preedit, commit, anchor) |
| `main/file_drop.rs` | winit DroppedFile 라우팅 |
| `main/divider_drag.rs` | divider 드래그 상태 |
| `main/preset_actions.rs` | preset 트리거 처리 |

---

## cli/ — CLI 클라이언트

**책임:** `tasty <subcommand>` 실행 시 GUI 앱의 IPC 서버에 연결하여 명령을 보내고 결과를 표시.

GUI 앱과 공유하는 타입은 `JsonRpcRequest/Response`뿐인 독립 클라이언트.

| 파일 | 역할 |
|------|------|
| `mod.rs` | Cli/Commands enum (35+ variant), run_client() |
| `request.rs` | Commands → JSON-RPC 변환 (command_to_request) |
| `format.rs` | 응답 포맷팅 (tree/list/pane/notification) |
| `claude.rs` | claude-hook, claude-wait (다중 요청/폴링) |
| `transport.rs` | TCP send_request() |

cli/mod.rs는 clap `#[derive(Subcommand)]` enum이 35+ variant를 가진다.

---

## ipc/handler/ — IPC 요청 핸들러

**책임:** JSON-RPC 메서드를 AppState 조작으로 변환.

도메인별로 핸들러 파일을 분리한다. 모든 핸들러가 `(state, id, params) → JsonRpcResponse` 시그니처를 따른다.

| 파일 | 역할 |
|------|------|
| `mod.rs` | handle_with_caller() dispatch + 권한 게이트 + audit hook + 유틸 |
| `workspace.rs` | workspace.list/create/update |
| `pane.rs` | pane.list/close, split |
| `tab.rs` | tab.list/create/close (type 파라미터로 markdown/html/image + plugin kind 통합) |
| `surface.rs` | surface.send/send_key/send_combo/send_to/close/close_self + mark/screen_text/cursor_position |
| `hooks.rs` | hook.set/list/unset, global_hook.set/list/unset, surface.fire_hook |
| `notification.rs` | notification.list/create |
| `message.rs` | message.send/read/count/clear |
| `meta.rs` | surface.meta_set/get/unset/list |
| `clipboard.rs` | clipboard.read/write |
| `image.rs` | image.* (host 이미지 viewer API) |
| `ime.rs` | ime 관련 IPC |
| `input_source.rs` | input source 전환/조회 |
| `memory.rs` | memory.* (agent memory CRUD) |
| `output.rs` | output.* (출력 파서 결과 조회) |
| `tool.rs` | tool.* (도구 메뉴 invoke / list) |
| `popup.rs` | popup.show/dismiss/list |
| `agent.rs` | agent.* (Task/Barrier/Semaphore/Lease/Reducer/RateLimit) |
| `approval.rs` | approval.request/respond/list/get |
| `audit.rs` | plugin.audit_query/summary/follow/clear |
| `session.rs` | session.issue/revoke + capability_elevation 게이트 + claude.* delegation |
| `telemetry.rs` | telemetry.* (metric record / cap / anomaly) |
| `file_handler.rs` | file_handler.dispatch / reload |
| `plugin.rs` | plugin.list/grant/revoke/list_agent_permission/request_permission/audit_* |

> `claude.*` 메서드는 호스트 핸들러가 아니라 `crates/tasty-plugin-claude/` plugin 이 contribute 하는 IPC namespace 다. spawn 시 호스트는 `session.issue` 로 토큰을 발급하고 plugin 이 자식 프로세스를 띄운다.

---

## settings/ — 설정 시스템

**책임:** TOML 설정 파일 로드/저장, 플랫폼별 셸 감지, 키바인딩 프리셋.

외부 `use crate::` 없이 독립적이며, 다른 모든 모듈에서 참조되는 최하위 계층.

| 파일 | 역할 |
|------|------|
| `mod.rs` | Settings 구조체, config_path(), load(), save() |
| `general.rs` | GeneralSettings + Shell 감지/검증 + bashrc 관리 |
| `appearance.rs` | AppearanceSettings + hex 색상 파싱 + UI 스케일 |
| `keybindings.rs` | KeybindingSettings + format_display + preset |
| `types.rs` | Clipboard/Zoom/Performance/Notification 설정 (작은 구조체 모음) |

---

## settings_ui/ — 설정 UI

**책임:** egui 모달 윈도우에서 Settings를 편집하는 UI.

| 파일 | 역할 |
|------|------|
| `mod.rs` | SettingsUiState, draw_settings_panel() (탭 바 + Save/Cancel) — Extension Mapping draft commit + save_combined_user_config 호출 포함 |
| `keybindings_tab.rs` | 키바인딩 캡처 UI (서브탭 5개, key combo 캡처, egui_key_to_string) |
| `tabs.rs` | General/Appearance/Clipboard/Notification/Language/Performance 탭 렌더링 |
| `file_handler_tab.rs` | FileHandler 탭 — Detectors / Handlers / Extension Mapping / Recent picks 4 sub-tab |

keybindings_tab.rs의 egui_key_to_string 매핑 테이블은 키 목록을 1:1 나열하는 구조이다.

---

## 단일 파일 모듈

| 파일 | 줄 | 역할 |
|------|-----|------|
| `main.rs` | 402 | App 구조체, 윈도우 생성/관리, process_ipc, winit 이벤트 루프 |
| `event_handler.rs` | 182 | ApplicationHandler impl (winit 이벤트 → App 메서드 위임) |
| `engine.rs` | ~60 | Engine 구조체 (IPC 서버, View ID, EventLoopProxy) |
| `engine_state.rs` | 270 | EngineState (워크스페이스 Vec, 설정, HookManager, 알림, waker factory) |
| `shortcuts.rs` | 439 | 키보드 단축키: physical→logical 변환, binding 매칭, 카테고리별 핸들러 |
| `font.rs` | 408 | FontConfig (cosmic-text 측정) + GlyphAtlas (shelf packing + 래스터라이징) |
| `theme_bridge.rs` | ~50 | `apply_theme_to_egui()` — `tasty_type_appearance::theme::Theme`을 egui Visuals/Style/TextStyle/Stroke로 변환. 호스트 측에 위치해 GUI 의존 격리. Theme 구조체 자체는 `tasty-type-appearance/src/theme.rs`에 있다 (`HexColor` + `LogicalPx` 필드 사용, GUI-free schema). 전역 인스턴스 IO 는 `tasty-themes` 가 담당 |
| `selection.rs` | 220 | NormalizedSelection, 좌표 정규화, is_selected() |
| `click_cursor.rs` | 217 | 클릭 좌표 → 터미널 그리드 → 커서 이동 명령 생성 |
| `notification.rs` | 248 | NotificationStore (FIFO, 병합, 읽음 추적) + OS 네이티브 알림 |
| `global_hooks.rs` | 209 | GlobalHookManager (interval/once/file 조건 기반 훅) |
| `surface_meta.rs` | ~90 | Surface별 key-value 메타데이터 (OnceLock + Mutex HashMap) |
| `view/mod.rs` | ~110 | Modality, ViewAction, ViewCtx, ViewRegistry, unbox_main |
| `view/ui.rs` | ~70 | View sealed trait + sealed::Sealed |
| `view/base.rs` | ~30 | ViewBase (모든 View 공통 필드 composition struct) |
| `view/modal.rs` | ~30 | ModalView supertrait (모달 계열 공통 default method) |
| `view/terminal_host.rs` | ~25 | TerminalHostView supertrait (터미널 호스트 계열) |
| `view/editor.rs` | ~20 | EditorView supertrait (modeless 에디터 marker) |
| `view/settings.rs` | ~160 | SettingsView (설정 모달, impl View + ModalView) |
| `view/quit.rs` | ~170 | QuitView (종료 확인 모달, impl View + ModalView) |
| `view/plugins.rs` | ~110 | PluginsView (plugin 매니저 모달, impl View + ModalView) |
| `view/preset.rs` | ~140 | PresetView (preset 편집기, impl View + EditorView) |
| `i18n.rs` | ~100 | TOML 기반 번역 (en/ko/ja 내장 + 사용자 오버라이드) |
| `crash_report.rs` | 243 | panic hook + 크래시 로그 수집 |
| `markdown_ui.rs` | ~100 | egui 마크다운 렌더링 (제목/목록/코드블록/테이블). 시그니처: `(ui, &mut MarkdownView, scroll_delta, id, font)` |
| `image_ui.rs` | ~600 | 이미지 뷰어/에디터 렌더링. 시그니처: `(ui, &mut ImagePanel, &mut ImageView)` — 모델은 디렉터리 탐색용, view는 픽셀/편집 상태용 |
| `html_ui.rs` | ~20 | HTML placeholder (URL 라벨만 표시; 실제 콘텐츠는 native WebView가 오버레이) |
