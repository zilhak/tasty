# 모듈별 상세

91개 .rs 파일을 디렉토리 모듈 단위로 묶어 설명한다. 각 모듈의 책임, 설계 목적, 한계를 기술한다.

---

## model/ — 데이터 모델

**책임:** Workspace → PaneNode → Pane → Tab → Panel → SurfaceNode/SurfaceGroupLayout의 계층 데이터 구조 정의. 레이아웃 계산(Rect 분할, 디바이더 탐색), 터미널 순회, 리사이즈.

**설계 목적:** 렌더링이나 UI에 의존하지 않는 순수 데이터 계층. `tasty-terminal` 크레이트만 참조한다.

| 파일 | 역할 |
|------|------|
| `mod.rs` | Rect, SplitDirection, DividerInfo 등 공통 타입. compute_terminal_rect() |
| `workspace.rs` | Workspace 구조체. PaneNode 트리를 소유하며 take/put 패턴으로 구조 변경 |
| `pane_tree.rs` | PaneNode 이진 트리 (상위 분할). split/close/rect계산/디바이더탐색/방향포커스 |
| `pane.rs` | Pane 구조체. 탭 관리 (생성/닫기/전환), Terminal 생성, Surface 분할 |
| `tab.rs` | Tab 구조체. Panel lazy init (deferred PTY spawn), take/put 패턴 |
| `panel.rs` | Panel enum (Terminal/SurfaceGroup/Markdown/Explorer). 터미널 탐색, 분할 연산 |
| `surface_group.rs` | SurfaceGroupNode wrapper. SurfaceGroupLayout의 포커스/리사이즈 위임 |
| `surface_layout.rs` | SurfaceGroupLayout 이진 트리 (하위 분할). pane_tree.rs와 동일한 패턴 |
| `markdown_panel.rs` | 마크다운 파일 경로 + 파싱 캐시 |
| `explorer_panel.rs` | 파일 탐색기 트리 상태 |
| `tests.rs` | Rect/PaneNode/SurfaceGroupLayout 유닛 테스트 |

**한계:** pane_tree.rs(456줄)와 surface_layout.rs(380줄)는 재귀 트리의 본질적 크기로, 모든 메서드가 `match self { Leaf/Split }` 패턴이라 더 분리하면 응집성이 깨진다.

---

## state/ — 애플리케이션 상태

**책임:** 윈도우당 1개의 AppState. model/ 위에서 "어떤 워크스페이스가 활성인가", "어떤 서피스에 포커스가 있는가" 등의 런타임 상태를 관리한다.

**설계 목적:** God Object였던 state.rs(1812줄)를 도메인별 impl 분산으로 분리. AppState 구조체는 mod.rs에서 정의하고, 각 서브모듈이 `impl AppState` 블록으로 메서드를 추가한다.

| 파일 | 역할 |
|------|------|
| `mod.rs` | AppState 구조체 + SurfaceMessage/ClaudeChildEntry/PaneContextMenu. 기본 접근자 |
| `workspace.rs` | 워크스페이스 생성/전환/닫기 |
| `tab.rs` | 탭 생성/이동/마크다운탭/탐색기탭 |
| `pane.rs` | 패인 분할/닫기, close_surface_by_id (5-case 계단식 닫기) |
| `focus.rs` | 포커스 이동 (순차, 방향별, 마우스 위치 기반) |
| `claude.rs` | Claude 부모-자식 관계 등록/해제/상태 관리 |
| `message.rs` | Surface 간 메시지 송수신 (큐 기반) |
| `layout.rs` | resize_all, render_regions, process_all, update_grid_size |
| `mouse.rs` | 디바이더 탐색/드래그, cursor_style_at |
| `mark.rs` | Read mark, 타이핑 감지 |
| `tests.rs` | 유닛 테스트 |

**한계:** state/pane.rs의 close_surface_by_id(~110줄)는 SurfaceGroup→탭→패인→워크스페이스 순서로 계단식으로 닫는 로직이 한 함수에 있다. 본질적으로 5-case 처리라 분리하면 오히려 흐름 파악이 어려워진다.

---

## gpu/ — GPU 상태 관리

**책임:** wgpu 디바이스/서피스, egui 통합, 렌더 오케스트레이션.

**설계 목적:** GpuState의 메서드를 역할별로 분산. mod.rs에 구조체와 진입점을 두고, 서브모듈이 impl 블록을 추가한다.

| 파일 | 역할 |
|------|------|
| `mod.rs` | GpuState 구조체, new(), resize(), render() 오케스트레이션, 접근자 |
| `render_pass.rs` | clear/terminal/egui 3-pass 렌더링 |
| `egui_bridge.rs` | egui 프레임 실행, IME preedit 오버레이, 테마/폰트 변경 후처리 |
| `fonts.rs` | egui CJK 폰트 로딩 (플랫폼별 시스템 폰트 탐색) |
| `screenshot.rs` | wgpu 프레임 캡처 → PNG 저장 |
| `shell_setup.rs` | 셸 경로 확인 다이얼로그 (첫 실행 시) |

**한계:** GpuState가 egui_ctx, egui_state, wgpu device/queue, CellRenderer를 모두 소유한다. egui와 wgpu를 분리하려면 소유권 재설계가 필요하며 현시점에서는 비용 대비 효과가 작다.

---

## renderer/ — 셀 렌더러

**책임:** 터미널 셀을 wgpu 인스턴스 데이터로 변환하고 GPU에서 렌더링.

**설계 목적:** CellRenderer가 GpuState와 분리되어 독립적으로 동작. 셰이더, 팔레트, 타입을 별도 파일로.

| 파일 | 역할 |
|------|------|
| `mod.rs` | CellRenderer 구조체, prepare_with_bg, prepare_terminal_viewport, render_scissored |
| `pipeline.rs` | new() + update_font(): wgpu 파이프라인/바인드그룹/버퍼 초기화 |
| `line_render.rs` | render_cell() 공통 로직으로 scrollback/surface 라인 렌더 통합 |
| `shaders.rs` | WGSL 셰이더 소스 (배경 + 글리프) |
| `palette.rs` | ANSI 256색 + TrueColor 팔레트 변환 |
| `types.rs` | Uniforms, BgInstance, GlyphInstance (bytemuck 호환) |

**한계:** pipeline.rs(349줄)는 wgpu RenderPipelineDescriptor가 본질적으로 장황한 선언 코드. 줄이기 어렵다.

---

## ui/ — egui UI 컴포넌트

**책임:** egui로 그리는 모든 비터미널 UI. 사이드바, 탭바, 알림 패널, 컨텍스트 메뉴, 다이얼로그.

**설계 목적:** 함수 단위로 이미 분리되어 있던 것을 파일로 옮김. 각 파일이 하나의 독립 UI 컴포넌트.

| 파일 | 역할 |
|------|------|
| `mod.rs` | draw_ui() 진입점 — 사이드바 모드 분기 + 터미널 영역 계산 |
| `sidebar.rs` | 축소/전체 사이드바 렌더링 |
| `tab_bar.rs` | 패인별 탭 바 (액션 큐 패턴) |
| `notification.rs` | 알림 패널 (스크롤 목록 + 워크스페이스 점프) |
| `context_menu.rs` | 우클릭 메뉴 (armed 상태머신) |
| `dialog.rs` | 워크스페이스 이름변경 + 마크다운 경로 다이얼로그 |
| `divider.rs` | 분할선 + 서피스 하이라이트 |
| `non_terminal.rs` | 마크다운/탐색기 패널 (egui 위임) |

---

## tasty_window/ — 윈도우 이벤트 처리

**책임:** 윈도우당 1개의 TastyWindow. winit 이벤트를 받아 GpuState + AppState를 조작한다.

**설계 목적:** handle_window_event() dispatch를 mod.rs에 두고, 입력 유형별(키보드/마우스/선택/리드로우)로 분산.

| 파일 | 역할 |
|------|------|
| `mod.rs` | TastyWindow 구조체, new(), handle_window_event() dispatch |
| `keyboard.rs` | handle_keyboard_input(), send_key_to_terminal(), handle_ime() |
| `mouse.rs` | handle_cursor_moved(), handle_mouse_input(), handle_mouse_wheel() |
| `selection.rs` | 텍스트 선택 (다중 클릭 감지, 단어/줄 경계, 그리드 변환) |
| `redraw.rs` | handle_redraw(): arrow queue + 터미널 이벤트 + 훅 실행 + 렌더 |
| `clipboard.rs` | paste_to_terminal(), 이미지 저장 |

---

## cli/ — CLI 클라이언트

**책임:** `tasty <subcommand>` 실행 시 GUI 앱의 IPC 서버에 연결하여 명령을 보내고 결과를 표시.

**설계 목적:** GUI 앱 내부와 공유하는 타입은 `JsonRpcRequest/Response`뿐. 완전히 독립적인 클라이언트.

| 파일 | 역할 |
|------|------|
| `mod.rs` | Cli/Commands enum (35+ variant), run_client() |
| `request.rs` | Commands → JSON-RPC 변환 (command_to_request) |
| `format.rs` | 응답 포맷팅 (tree/list/pane/notification) |
| `claude.rs` | claude-hook, claude-wait (다중 요청/폴링) |
| `transport.rs` | TCP send_request() |

**한계:** cli/mod.rs(381줄)는 clap `#[derive(Subcommand)]` enum이 35+ variant라 줄일 수 없다.

---

## ipc/handler/ — IPC 요청 핸들러

**책임:** JSON-RPC 메서드를 AppState 조작으로 변환.

**설계 목적:** 도메인별로 핸들러 파일 분리. 모든 핸들러가 `(state, id, params) → JsonRpcResponse` 동일 시그니처.

| 파일 | 역할 |
|------|------|
| `mod.rs` | handle() dispatch match + 유틸 (apply_meta, resolve_target_param) |
| `workspace.rs` | workspace.list/create/update/select |
| `pane.rs` | pane.list/close, split, focus.direction |
| `tab.rs` | tab.list/create/close, open_markdown/explorer |
| `surface.rs` | surface.send/send_key/close/focus + mark/screen_text/cursor_position |
| `claude.rs` | claude.launch/spawn/children/parent/kill/respawn/set_idle/set_needs_input/broadcast/wait |
| `hooks.rs` | hook.set/list/unset, global_hook.set/list/unset, surface.fire_hook |
| `notification.rs` | notification.list/create |
| `message.rs` | message.send/read/count/clear |
| `meta.rs` | surface.meta_set/get/unset/list |

---

## settings/ — 설정 시스템

**책임:** TOML 설정 파일 로드/저장, 플랫폼별 셸 감지, 키바인딩 프리셋.

**설계 목적:** 외부 `use crate::` 없이 독립. 다른 모든 모듈에서 참조되는 최하위 계층.

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
| `mod.rs` | SettingsUiState, draw_settings_panel() (탭 바 + Save/Cancel) |
| `keybindings_tab.rs` | 키바인딩 캡처 UI (서브탭 5개, key combo 캡처, egui_key_to_string) |
| `tabs.rs` | General/Appearance/Clipboard/Notification/Language/Performance 탭 렌더링 |

**한계:** keybindings_tab.rs(405줄)는 egui_key_to_string 매핑 테이블(70줄)이 본질적으로 장황.

---

## 단일 파일 모듈

| 파일 | 줄 | 역할 |
|------|-----|------|
| `main.rs` | 402 | App 구조체, 윈도우 생성/관리, process_ipc, winit 이벤트 루프 |
| `event_handler.rs` | 182 | ApplicationHandler impl (winit 이벤트 → App 메서드 위임) |
| `engine.rs` | ~60 | Engine 구조체 (IPC 서버, 윈도우 ID, EventLoopProxy) |
| `engine_state.rs` | 270 | EngineState (워크스페이스 Vec, 설정, HookManager, 알림, waker factory) |
| `shortcuts.rs` | 439 | 키보드 단축키: physical→logical 변환, binding 매칭, 카테고리별 핸들러 |
| `font.rs` | 408 | FontConfig (cosmic-text 측정) + GlyphAtlas (shelf packing + 래스터라이징) |
| `theme.rs` | 223 | Catppuccin Mocha 테마 구조체, egui 스타일 적용 |
| `selection.rs` | 220 | NormalizedSelection, 좌표 정규화, is_selected() |
| `click_cursor.rs` | 217 | 클릭 좌표 → 터미널 그리드 → 커서 이동 명령 생성 |
| `notification.rs` | 248 | NotificationStore (FIFO, 병합, 읽음 추적) + OS 네이티브 알림 |
| `global_hooks.rs` | 209 | GlobalHookManager (interval/once/file 조건 기반 훅) |
| `surface_meta.rs` | ~90 | Surface별 key-value 메타데이터 (OnceLock + Mutex HashMap) |
| `modal_window.rs` | ~120 | 설정 모달 윈도우 (독립 GPU + egui 인스턴스) |
| `i18n.rs` | ~100 | TOML 기반 번역 (en/ko/ja 내장 + 사용자 오버라이드) |
| `crash_report.rs` | 243 | panic hook + 크래시 로그 수집 |
| `markdown_ui.rs` | 259 | egui 마크다운 렌더링 (제목/목록/코드블록/테이블) |
| `explorer_ui.rs` | 171 | egui 파일 탐색기 렌더링 (트리 + 미리보기) |
