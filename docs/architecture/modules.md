# 모듈별 상세

17개 소스 파일 각각의 역할, 공개 API, 의존 관계, 테스트 현황을 분석한다.

---

## 1. main.rs (762줄)

### 역할
애플리케이션 진입점. winit 이벤트 루프를 운용하고 모든 모듈을 통합한다.

### 주요 타입 및 구조체
- `AppEvent` (33행): PTY 리더 스레드에서 이벤트 루프로 전달하는 커스텀 이벤트. `TerminalOutput` 한 가지.
- `DividerDragKind` (40행): 디바이더 드래그 종류 (Pane / Surface).
- `DividerDrag` (48행): 진행 중인 디바이더 드래그 상태.
- `App` (53행): 메인 애플리케이션 구조체. GpuState, AppState, Window, IpcServer를 소유.

### 공개 API
- 없음 (바이너리 진입점이므로 `pub` API 불필요).

### 주요 메서드
| 메서드 | 줄 | 역할 |
|--------|-----|------|
| `App::new()` | 73 | App 초기 구성 |
| `App::compute_terminal_rect_with_sidebar()` | 89 | 사이드바 제외 터미널 영역 계산 |
| `App::handle_shortcut()` | 102 | 키보드 단축키 처리 (Ctrl+Shift 조합, Alt+숫자 등) |
| `App::process_ipc()` | 244 | IPC 명령 큐 처리 |
| `ApplicationHandler::resumed()` | 277 | 윈도우/GPU/상태 초기화 |
| `ApplicationHandler::window_event()` | 339 | 이벤트 디스패치 (키보드, 마우스, 리사이즈, 리드로우) |
| `main()` | 739 | CLI 파싱 → CLI 모드 또는 GUI 모드 분기 |

### 의존 관계
`gpu`, `ipc::server`, `ipc::handler`, `model`, `state`, `cli`, `terminal`, `hooks`, `notification`, `settings`, `settings_ui`

### 테스트
없음.

---

## 2. model.rs (1,370줄)

### 역할
전체 데이터 모델을 정의한다. Workspace → PaneNode → Pane → Tab → Panel → SurfaceNode/SurfaceGroupNode 계층 구조, 바이너리 트리 레이아웃, Rect 연산.

### 주요 타입
| 타입 | 줄 | 설명 |
|------|-----|------|
| `WorkspaceId`, `PaneId`, `TabId`, `SurfaceId` | 3-6 | u32 타입 별칭 |
| `Rect` | 9 | 픽셀 사각형 (x, y, width, height) |
| `Workspace` | 74 | 워크스페이스. PaneNode 트리 소유 |
| `PaneNode` | 148 | 바이너리 트리 enum (Leaf / Split) |
| `Pane` | 394 | 화면 영역. Tab 목록 소유 |
| `Tab` | 569 | 탭. Panel 소유 |
| `Panel` | 605 | 콘텐츠 enum (Terminal / SurfaceGroup) |
| `SurfaceNode` | 707 | 단일 터미널 (id + Terminal) |
| `SurfaceGroupNode` | 713 | 탭 내 분할 그룹 |
| `SurfaceGroupLayout` | 833 | 서피스 바이너리 트리 (Single / Split) |
| `DividerInfo` | 1116 | 디바이더 위치 정보 |
| `SplitDirection` | 1123 | Horizontal / Vertical |

### 공개 API 목록

**Rect:**
- `contains(x, y)` (19행), `approx_eq(other)` (24행), `split(direction, ratio)` (31행)

**Workspace:**
- `new()` (84행), `new_with_shell()` (98행), `pane_layout()` (122행), `pane_layout_mut()` (129행), `take_pane_layout()` (136행), `put_pane_layout()` (141행)

**PaneNode:**
- `split_pane_in_place()`, `close_pane()`, `first_pane()`, `compute_rects()`, `find_pane()`, `find_pane_mut()`, `all_terminals()`, `all_terminals_mut()`, `process_all()`, `all_pane_ids()`, `next_pane_id()`, `prev_pane_id()`, `find_divider_at()`, `update_ratio_for_rect()`

**Pane:**
- `new()`, `new_with_shell()`, `add_tab()`, `add_tab_with_shell()`, `close_tab()`, `close_active_tab()`, `active_panel()`, `active_panel_mut()`, `split_active_surface()`, `split_active_surface_with_shell()`, `active_terminal()`, `active_terminal_mut()`, `next_tab()`, `prev_tab()`, `all_terminals()`, `all_terminals_mut()`

**Panel:**
- `focused_terminal()` (614행), `focused_terminal_mut()` (622행), `collect_terminals()` (630행), `collect_terminals_mut()` (638행), `render_regions()` (646행), `resize_all()` (654행), `split_surface_with_terminal()` (667행)

**SurfaceGroupNode:**
- `layout()`, `layout_mut()`, `close_surface()`, `split_surface()`, `compute_rects()`, `focused_terminal()`, `focused_terminal_mut()`, `resize_all()`, `move_focus_forward()`, `move_focus_backward()`

**SurfaceGroupLayout:**
- `split_with_node()`, `close_surface()`, `first_terminal()`, `first_surface_id()`, `find_terminal()`, `find_terminal_mut()`, `render_regions()`, `resize_all()`, `all_surface_ids()`, `collect_terminals()`, `collect_terminals_mut()`, `process_all()`, `find_divider_at()`, `update_ratio_for_rect()`, `find_surface_at()`

### 의존 관계
`terminal::Terminal`, `terminal::Waker`

### 테스트
19개 테스트: Rect 연산, PaneNode 트리 조작, 디바이더 탐색, 분할 in-place, 패인 닫기(close_pane), 탭 닫기(close_tab), SurfaceGroupLayout.

---

## 3. state.rs (733줄)

### 역할
앱 전체 상태를 관리한다. 워크스페이스 목록, 포커스 추적, 설정, 알림, 훅 매니저, ID 생성기를 통합 보유.

### 주요 타입
- `IdGenerator` (8행): workspace/pane/tab/surface ID 자동 증가 생성기.
- `AppState` (50행): 전체 상태. pub 필드 12개.

### 공개 API 목록

| 메서드 | 줄 | 역할 |
|--------|-----|------|
| `new()` | 75 | 초기 상태 생성 (1 workspace, 1 pane, 1 tab, 1 terminal) |
| `active_workspace()` | 102 | 활성 워크스페이스 참조 |
| `active_workspace_mut()` | 107 | 활성 워크스페이스 가변 참조 |
| `focused_pane()` | 113 | 포커스된 패인 참조 |
| `focused_pane_mut()` | 122 | 포커스된 패인 가변 참조 (fallback 포함) |
| `focused_terminal()` | 137 | 포커스된 터미널 참조 |
| `focused_terminal_mut()` | 142 | 포커스된 터미널 가변 참조 |
| `add_workspace()` | 147 | 새 워크스페이스 추가 |
| `add_tab()` | 172 | 포커스 패인에 탭 추가 |
| `split_pane()` | 187 | 패인 분할 |
| `split_surface()` | 211 | 서피스 분할 (탭 내부) |
| `close_active_tab()` | - | 활성 탭 닫기 |
| `close_active_pane()` | - | 포커스된 패인 닫기 (unsplit) |
| `close_active_surface()` | - | SurfaceGroup 내 포커스된 서피스 닫기 |
| `switch_workspace()` | 225 | 워크스페이스 전환 |
| `next_tab_in_pane()` | 232 | 다음 탭 전환 |
| `prev_tab_in_pane()` | 239 | 이전 탭 전환 |
| `move_focus_next_pane()` | 246 | 다음 패인 포커스 |
| `move_focus_prev_pane()` | 252 | 이전 패인 포커스 |
| `process_all()` | 259 | 모든 터미널 PTY 처리 |
| `render_regions()` | 273 | 렌더 영역 계산 |
| `update_grid_size()` | 302 | 그리드 크기 갱신 |
| `resize_all()` | 308 | 모든 터미널 리사이즈 |
| `focused_pane_id()` | 328 | 포커스 패인 ID |
| `collect_events()` | 334 | 모든 터미널 이벤트 수집 |
| `set_mark()` | 390 | 읽기 마크 설정 |
| `read_since_mark()` | 400 | 마크 이후 출력 읽기 |
| `next_surface_id()` | 555 | 다음 서피스 ID |
| `focus_pane_at_position()` | 576 | 위치 기반 패인 포커스 |
| `focus_surface_at_position()` | 595 | 위치 기반 서피스 포커스 |
| `find_pane_divider_at()` | 646 | 패인 디바이더 탐색 |
| `find_surface_divider_at()` | 652 | 서피스 디바이더 탐색 |
| `update_pane_divider()` | 682 | 패인 디바이더 비율 갱신 |
| `update_surface_divider()` | 692 | 서피스 디바이더 비율 갱신 |

### 의존 관계
`model`, `terminal`, `settings`, `notification`, `hooks`, `settings_ui`

### 테스트
없음.

---

## 4. terminal.rs (~1,100줄)

### 역할
PTY 기반 터미널 에뮬레이터. portable-pty로 셸 프로세스를 실행하고, termwiz로 VTE 시퀀스를 파싱하여 Surface에 반영한다. DECSET/DECRST 모드 관리, 대체 화면 버퍼, 스크롤 리전을 지원한다.

### 주요 타입
- `Waker` (16행): `Arc<dyn Fn() + Send + Sync>` 타입 별칭. PTY 데이터 수신 시 이벤트 루프를 깨운다.
- `TerminalEvent` (19행): 터미널이 생성하는 이벤트 (surface_id + kind).
- `TerminalEventKind` (26행): Notification / BellRing / TitleChanged / CwdChanged / ProcessExited / ClipboardSet.
- `MouseTrackingMode`: None / Click(1000) / CellMotion(1002) / AllMotion(1003).
- `Terminal`: PTY + 파서 + 기본/대체 Surface + 출력 버퍼 + DECSET 상태를 소유하는 핵심 구조체.

### 공개 API 목록

| 메서드 | 역할 |
|--------|------|
| `new()` | 기본 셸로 터미널 생성 |
| `new_with_shell()` | 커스텀 셸로 터미널 생성 |
| `process()` | PTY 출력 처리, Surface 갱신 (DECSET/DECRST 인터셉트) |
| `send_key()` | 키보드 텍스트를 PTY에 전송 |
| `send_bytes()` | 원시 바이트를 PTY에 전송 |
| `resize()` | 기본/대체 Surface + PTY 리사이즈 |
| `surface()` | 활성(기본 또는 대체) termwiz Surface 참조 |
| `cols()` / `rows()` | 열/행 수 |
| `is_alive()` / `check_process_alive()` | 프로세스 생존 여부 |
| `take_events()` | 누적 이벤트 추출 |
| `set_mark()` / `read_since_mark()` | 읽기 마크 설정/조회 |
| `application_cursor_keys()` | DECCKM 모드 조회 |
| `cursor_visible()` | DECTCEM 모드 조회 |
| `bracketed_paste()` | 브래킷 붙여넣기 모드 조회 |
| `mouse_tracking()` | 마우스 트래킹 모드 조회 |
| `sgr_mouse()` | SGR 마우스 인코딩 조회 |
| `focus_tracking()` | 포커스 트래킹 조회 |
| `is_alternate_screen()` | 대체 화면 활성 여부 조회 |

### 내부 메서드
| 메서드 | 역할 |
|--------|------|
| `action_to_changes()` | VT 액션 → Surface Change 변환 |
| `handle_mode()` | DECSET/DECRST CSI::Mode 처리 |
| `set_dec_mode()` | 개별 DEC 사적 모드 설정/해제 |
| `surface_mut()` | 활성 Surface 가변 참조 |
| `scroll_region_params()` | 스크롤 리전 파라미터 계산 |
| `read_line_from_surface()` | Surface에서 특정 행의 텍스트 읽기 |
| `map_control()` | 제어 문자 매핑 (LF, CR, BS, Tab, Bell) |
| `map_csi()` | CSI 시퀀스 매핑 (SGR, Cursor, Edit) |
| `map_sgr()` | SGR 색상/스타일 매핑 |
| `map_cursor()` | 커서 이동 매핑 + DECSTBM 스크롤 리전 |
| `map_edit()` | 화면 편집 매핑 (ED, EL, SU, SD, DCH, ICH, DL, IL, ECH) |
| `map_esc()` | ESC 시퀀스 매핑 (커서 저장/복원, ReverseIndex, FullReset) |
| `map_osc()` | OSC 매핑 (타이틀, CWD, 알림) |
| `default_shell()` | 플랫폼별 기본 셸 경로 |
| `strip_ansi_escapes()` | ANSI 이스케이프 시퀀스 제거 |

### 의존 관계
portable-pty, termwiz, regex

### 테스트
11개: DECSET/DECRST 모드 토글, 대체 화면 전환/리사이즈, 방향키 모드, 전체 리셋.

---

## 5. renderer.rs (699줄)

### 역할
wgpu 기반 터미널 셀 렌더러. 배경색과 글리프를 각각 인스턴스 렌더링으로 그린다. WGSL 셰이더 포함.

### 주요 타입
- `Uniforms` (12행): cell_size, grid_offset, viewport_size를 GPU에 전달하는 유니폼 버퍼.
- `BgInstance` (21행): 배경 인스턴스 데이터 (pos, bg_color).
- `GlyphInstance` (28행): 글리프 인스턴스 데이터 (pos, uv, fg_color, offset, size).
- `CellRenderer` (196행): 배경/글리프 파이프라인, 유니폼, 인스턴스 버퍼, 폰트/아틀라스를 소유.

### 공개 API 목록

| 메서드 | 줄 | 역할 |
|--------|-----|------|
| `new(device, queue, format, font_size, font_family)` | 218 | 파이프라인, 버퍼, 바인드 그룹 초기화. font_size와 font_family를 FontConfig에 전달 |
| `resize()` | 500 | 뷰포트 크기 갱신 |
| `prepare()` | 514 | Surface → 인스턴스 데이터 빌드 |
| `render()` | 592 | 2-pass 렌더 (배경 → 글리프) |
| `grid_size()` | 611 | 픽셀 → 그리드 크기 계산 |
| `grid_size_for_rect()` | 621 | Rect → 그리드 크기 계산 |
| `prepare_viewport()` | 632 | 뷰포트별 유니폼 갱신 + 인스턴스 빌드 |
| `render_scissored()` | 657 | 시저 렉트 적용 렌더 |
| `cell_width()` | 691 | 셀 너비 (px) |
| `cell_height()` | 696 | 셀 높이 (px) |

### 내장 셰이더
- `BG_SHADER` (39행): 배경 쿼드 WGSL. 셀 위치 → NDC 변환, 단색 렌더.
- `GLYPH_SHADER` (82행): 글리프 쿼드 WGSL. 아틀라스 텍스처 샘플링, 알파 블렌딩.

### 색상 팔레트
- `ANSI_COLORS` (141행): 16색 ANSI 팔레트.
- `palette_index_to_rgb()` (160행): 0-255 인덱스 → RGB 변환 (16색 + 216큐브 + 24그레이).
- `color_attr_to_rgba()` (178행): termwiz ColorAttribute → RGBA 변환.

### 의존 관계
`font.rs`, `model::Rect`, wgpu, bytemuck, termwiz

### 테스트
없음.

---

## 6. gpu.rs (370줄)

### 역할
wgpu + egui 통합 GPU 상태 관리. Surface 생성, 디바이스/큐 초기화, 프레임 렌더링 오케스트레이션.

### 주요 타입
- `GpuState` (13행): wgpu Surface, Device, Queue, Config, CellRenderer, egui Context/State/Renderer를 소유.

### 공개 API 목록

| 메서드 | 줄 | 역할 |
|--------|-----|------|
| `new(window, appearance)` | 27 | GPU 어댑터/디바이스/Surface/렌더러/egui 초기화. AppearanceSettings에서 font_size/font_family/theme을 읽어 적용 |
| `resize()` | 127 | Surface 재구성 + 렌더러 리사이즈 |
| `handle_egui_event()` | 140 | egui에 winit 이벤트 전달 |
| `render()` | 146 | 전체 프레임 렌더링 (egui UI + 터미널) |
| `grid_size()` | 327 | 윈도우 크기 → 그리드 크기 |
| `grid_size_for_rect()` | 332 | Rect → 그리드 크기 |
| `cell_width()` | 336 | 셀 너비 위임 |
| `cell_height()` | 340 | 셀 높이 위임 |
| `device()` | 344 | Device 참조 |
| `queue()` | 348 | Queue 참조 |
| `config()` | 352 | SurfaceConfiguration 참조 |
| `size()` | 356 | 물리 크기 |
| `scale_factor()` | 360 | DPI 스케일 팩터 |
| `update_scale_factor()` | 365 | 스케일 팩터 갱신 |
| `refresh_theme()` | - | 설정 변경 후 egui 테마 재적용 |

### render() 렌더링 파이프라인 (146-324행)
1. 터미널 영역 계산 (사이드바 제외)
2. 패인 Rect 계산
3. egui 프레임 시작 (`egui_ctx.run`)
4. UI 그리기 (`ui::draw_ui`, `ui::draw_pane_tab_bars`, `ui::draw_notification_panel`, `settings_ui::draw_settings_window`)
5. egui 테셀레이트
6. wgpu 출력 텍스처 획득
7. Clear 패스
8. 터미널 패스 (각 서피스별 prepare_viewport + render_scissored)
9. egui 패스 (텍스처 업로드, 버퍼 업데이트, 렌더)
10. Present

### 의존 관계
`renderer::CellRenderer`, `state::AppState`, `model::Rect`, `ui`, `settings_ui`, wgpu, egui, egui_wgpu, egui_winit

### 테스트
없음.

---

## 7. font.rs (358줄)

### 역할
cosmic-text 기반 폰트 설정과 GPU 글리프 아틀라스 관리.

### 주요 타입
- `FontMetrics` (8행): cell_width, cell_height, font_size, baseline.
- `FontConfig` (17행): FontSystem + SwashCache + FontMetrics + FamilyOwned.
- `GlyphKey` (76행): (char, bold, italic) 해시 키.
- `AtlasEntry` (84행): UV 좌표, 오프셋, 크기.
- `GlyphAtlas` (100행): 2048x2048 R8 텍스처, 선반 패커, HashMap 캐시.

### 공개 API 목록

| 메서드 | 줄 | 역할 |
|--------|-----|------|
| `FontConfig::new(font_size, font_family)` | 24 | FontSystem 초기화 + 폰트 패밀리 설정 + 셀 크기 측정. font_family가 빈 문자열이나 "monospace"이면 FamilyOwned::Monospace 사용 |
| `GlyphAtlas::new()` | 115 | 텍스처/뷰/샘플러 생성 |
| `GlyphAtlas::get_or_insert()` | 157 | 캐시 히트 또는 래스터라이즈 |

### 내부 메서드
- `FontConfig::measure_cell()` (37행): 'M' 문자로 모노스페이스 셀 크기 측정.
- `GlyphAtlas::rasterize_glyph()` (170행): cosmic-text 래스터라이즈 → 그레이스케일 변환 → 선반 패킹 → GPU 업로드.

### 아틀라스 전략
- 고정 크기 2048x2048 (`ATLAS_SIZE`, 113행).
- 선반(shelf) 기반 행 패킹.
- 아틀라스 가득 차면 캐시 전체 초기화 (273행).
- R8Unorm 포맷, 알파 채널만 저장.

### 의존 관계
cosmic-text, wgpu

### 테스트
없음.

---

## 8. ui.rs (401줄)

### 역할
egui 기반 사이드바, 패인별 탭 바, 알림 패널 UI.

### 공개 API 목록

| 함수 | 줄 | 역할 |
|------|-----|------|
| `draw_ui()` | 10 | 좌측 사이드바 (워크스페이스 목록, 알림 배지, 단축키 도움말) |
| `draw_pane_tab_bars()` | 139 | 패인별 독립 탭 바 (egui Area 오버레이) |
| `draw_notification_panel()` | 242 | 알림 윈도우 (스크롤, 워크스페이스 점프, 읽음 처리) |

### 내부 타입
- `PaneTabInfo` (148행): 읽기 전용 탭 정보 구조체.
- `PaneTabAction` (398행): SwitchTab / AddTab 액션 enum.

### UI 패턴
3-pass 패턴 사용 (draw_pane_tab_bars):
1. 읽기 패스: 상태에서 표시 정보 수집
2. 렌더 패스: egui 위젯 렌더 + 액션 수집
3. 적용 패스: 수집된 액션으로 상태 변경

### 의존 관계
`state::AppState`, `model::Rect`, egui

### 테스트
없음.

---

## 9. cli.rs (331줄)

### 역할
CLI 클라이언트. clap으로 서브커맨드를 파싱하고, TCP로 JSON-RPC 요청을 전송하여 실행 중인 tasty 인스턴스를 제어한다.

### 주요 타입
- `Cli` (11행): clap 최상위 파서.
- `Commands` (18행): 21개 서브커맨드 enum (List, NewWorkspace, SelectWorkspace, Send, SendKey, Notify, Notifications, Tree, Split, NewTab, Surfaces, Panes, Info, SetHook, ListHooks, UnsetHook, SetMark, ReadSinceMark, Claude).

### 공개 API 목록

| 함수 | 줄 | 역할 |
|------|-----|------|
| `run_client()` | 125 | TCP 연결 → 요청 전송 → 응답 수신/출력 |

### 내부 함수
- `command_to_request()` (162행): Commands → JsonRpcRequest 변환.
- `format_output()` (247행): 응답 포매팅 디스패치.
- `format_tree()` (260행): 트리 뷰 포매팅.
- `format_workspace_list()` (289행): 워크스페이스 목록 포매팅.
- `format_pane_list()` (301행): 패인 목록 포매팅.
- `format_notification_list()` (313행): 알림 목록 포매팅.

### 의존 관계
`ipc/protocol`, `ipc/server` (포트 파일 경로만), clap, serde_json

### 테스트
없음.

---

## 10. hooks.rs (290줄)

### 역할
Surface별 이벤트 훅 시스템. 특정 이벤트 발생 시 셸 명령을 실행한다.

### 주요 타입
- `HookId` (4행): u64 타입 별칭.
- `SurfaceHook` (7행): 훅 정의 (id, surface_id, event, command, once, compiled_regex).
- `HookEvent` (17행): ProcessExit / OutputMatch(String) / Bell / Notification / IdleTimeout(u64).
- `HookManager` (82행): 훅 목록 + ID 생성기.

### 공개 API 목록

| 메서드/함수 | 줄 | 역할 |
|-------------|-----|------|
| `HookEvent::parse()` | 54 | 문자열 → HookEvent 파싱 |
| `HookEvent::to_display_string()` | 71 | HookEvent → 문자열 직렬화 |
| `HookManager::new()` | 88 | 빈 매니저 생성 |
| `HookManager::add_hook()` | 95 | 훅 등록 (regex 사전 컴파일) |
| `HookManager::remove_hook()` | 121 | 훅 제거 |
| `HookManager::list_hooks()` | 127 | 훅 조회 (surface_id 필터 옵션) |
| `HookManager::check_and_fire()` | 140 | 이벤트 매칭 → 명령 실행 → once 훅 제거 |

### 보안 모델
셸 명령은 백그라운드 스레드에서 실행 (151-156행). IPC가 localhost에서만 수신하므로 원격 공격 벡터는 없다.

### 의존 관계
regex

### 테스트
16개 테스트 (172-290행): 이벤트 파싱, 직렬화 왕복, 매칭, HookManager CRUD, once/persistent 동작.

---

## 11. notification.rs (239줄)

### 역할
터미널 알림 저장소 + OS 네이티브 알림 전송.

### 주요 타입
- `NotificationId` (6행): u64 타입 별칭.
- `Notification` (9행): id, source_workspace, source_surface, title, body, timestamp, read.
- `NotificationStore` (20행): VecDeque 기반 FIFO 저장소 (최대 100개).

### 공개 API 목록

| 메서드/함수 | 줄 | 역할 |
|-------------|-----|------|
| `NotificationStore::new()` | 31 | 기본 500ms 병합 윈도우 |
| `with_coalesce_ms()` | 36 | 커스텀 병합 윈도우 |
| `set_coalesce_ms()` | 47 | 병합 윈도우 변경 |
| `add()` | 52 | 알림 추가 (병합 또는 새 항목) |
| `unread_count()` | 107 | 전체 미읽음 수 |
| `unread_count_for_workspace()` | 112 | 워크스페이스별 미읽음 수 |
| `all()` | 120 | 전체 알림 이터레이터 |
| `mark_read()` | 125 | 특정 알림 읽음 처리 |
| `mark_all_read()` | 132 | 전체 읽음 처리 |
| `should_send_system_notification()` | 139 | 레이트 리미팅 (1초 간격) |
| `has_unread_for_surface()` | 151 | 서피스별 미읽음 여부 |
| `send_system_notification()` | 159 | OS 네이티브 알림 전송 |

### 의존 관계
`model::SurfaceId`, `model::WorkspaceId`, notify-rust

### 테스트
8개 테스트 (167-239행): 추가/카운트, 읽음 처리, 워크스페이스별 카운트, 병합, FIFO 제거.

---

## 12. settings.rs (274줄)

### 역할
TOML 기반 설정 파일 로드/저장. 5개 섹션 구조체.

### 주요 타입
- `Settings` (10행): GeneralSettings + AppearanceSettings + ClipboardSettings + NotificationSettings + KeybindingSettings.
- `GeneralSettings` (19행): shell, startup_command.
- `AppearanceSettings` (27행): font_family, font_size, theme, background_opacity, sidebar_width.
- `ClipboardSettings` (36행): macos_style, linux_style, windows_style.
- `NotificationSettings` (44행): enabled, system_notification, sound, coalesce_ms.
- `KeybindingSettings` (53행): 6개 단축키 문자열.

### 공개 API 목록

| 메서드 | 줄 | 역할 |
|--------|-----|------|
| `Settings::config_path()` | 177 | `~/.config/tasty/config.toml` 경로 |
| `Settings::ensure_config_dir()` | 182 | 설정 디렉토리 생성 |
| `Settings::load()` | 192 | TOML 로드 (실패 시 기본값) |
| `Settings::save()` | 220 | TOML 저장 |

### 의존 관계
toml, directories, serde

### 테스트
5개 테스트 (232-274행): 기본값, 직렬화 왕복, 부분 TOML, 빈 TOML.

---

## 13. settings_ui.rs (208줄)

### 역할
egui 기반 설정 윈도우 UI. 4개 탭 (General, Appearance, Clipboard, Notifications).

### 주요 타입
- `SettingsTab` (5행): 탭 enum.
- `SettingsUiState` (13행): 활성 탭 + 설정 드래프트 복사본.

### 공개 API 목록

| 함수/메서드 | 줄 | 역할 |
|-------------|-----|------|
| `SettingsUiState::new()` | 20 | 초기 상태 |
| `draw_settings_window()` | 29 | 설정 윈도우 전체 렌더 |

### 내부 함수
- `draw_general_tab()` (121행): 셸, 시작 명령 편집.
- `draw_appearance_tab()` (140행): 폰트, 테마, 투명도, 사이드바 너비 편집.
- `draw_clipboard_tab()` (178행): 클립보드 스타일 체크박스.
- `draw_notifications_tab()` (188행): 알림 설정 편집.

### 저장 패턴
드래프트 복사본으로 편집 → Save 시 원본에 반영 + 파일 저장, Cancel 시 드래프트 폐기.

### 의존 관계
`settings::Settings`, egui

### 테스트
없음.

---

## 14. ipc/mod.rs (3줄)

### 역할
IPC 하위 모듈 re-export.

### 공개 모듈
`handler`, `protocol`, `server`

---

## 15. ipc/protocol.rs (131줄)

### 역할
JSON-RPC 2.0 프로토콜 타입 정의.

### 주요 타입
- `JsonRpcRequest` (4행): jsonrpc, method, params, id.
- `JsonRpcResponse` (12행): jsonrpc, result, error, id.
- `JsonRpcError` (22행): code, message, data.

### 공개 API 목록

| 메서드 | 줄 | 역할 |
|--------|-----|------|
| `JsonRpcResponse::success()` | 31 | 성공 응답 생성 |
| `JsonRpcResponse::error()` | 40 | 에러 응답 생성 |
| `JsonRpcResponse::method_not_found()` | 53 | -32601 에러 |
| `JsonRpcResponse::invalid_params()` | 57 | -32602 에러 |
| `JsonRpcResponse::internal_error()` | 61 | -32603 에러 |

### 의존 관계
serde, serde_json

### 테스트
5개 테스트 (66-131행): 요청 직렬화, 성공/에러 응답, method_not_found, 왕복.

---

## 16. ipc/server.rs (196줄)

### 역할
TCP 기반 JSON-RPC IPC 서버. 127.0.0.1 랜덤 포트에서 수신, 메인 스레드와 mpsc 채널로 통신.

### 주요 타입
- `IpcCommand` (14행): request + response_tx.
- `IpcServer` (20행): command_rx + port + shutdown 플래그.

### 공개 API 목록

| 메서드 | 줄 | 역할 |
|--------|-----|------|
| `IpcServer::start()` | 29 | 서버 시작 (포트 파일 기록, 수신 스레드 시작) |
| `IpcServer::try_recv()` | 75 | 비차단 명령 수신 |
| `IpcServer::port()` | 80 | 리슨 포트 |
| `IpcServer::port_file_path()` | 167 | 포트 파일 경로 |
| `IpcServer::read_port_file()` | 172 | 포트 파일 읽기 (CLI용) |

### 스레드 모델
- 수신 스레드: non-blocking TcpListener + 100ms 폴링 + shutdown 플래그.
- 연결 스레드: 연결 당 하나, 라인 단위 JSON-RPC 처리.
- 메인 스레드: `try_recv()`로 폴링, 동기 응답.

### Drop 구현 (187행)
shutdown 시그널 + 포트 파일 삭제.

### 의존 관계
`ipc/protocol`, directories

### 테스트
없음.

---

## 17. ipc/handler.rs (564줄)

### 역할
JSON-RPC 요청을 AppState에 대해 실행하는 핸들러. 20개 메서드 디스패치.

### 공개 API

| 함수 | 줄 | 역할 |
|------|-----|------|
| `handle()` | 10 | 메서드 문자열 → 핸들러 디스패치 |

### 지원 메서드 (20개)

| 메서드 | 핸들러 함수 | 줄 |
|--------|------------|-----|
| `system.info` | `handle_system_info` | 38 |
| `workspace.list` | `handle_workspace_list` | 49 |
| `workspace.create` | `handle_workspace_create` | 66 |
| `workspace.select` | `handle_workspace_select` | 96 |
| `pane.list` | `handle_pane_list` | 118 |
| `pane.split` | `handle_pane_split` | 141 |
| `tab.list` | `handle_tab_list` | 165 |
| `tab.create` | `handle_tab_create` | 184 |
| `surface.list` | `handle_surface_list` | 203 |
| `surface.send` | `handle_surface_send` | 261 |
| `surface.send_key` | `handle_surface_send_key` | 277 |
| `notification.list` | `handle_notification_list` | 316 |
| `notification.create` | `handle_notification_create` | 336 |
| `tree` | `handle_tree` | 356 |
| `hook.set` | `handle_hook_set` | 397 |
| `hook.list` | `handle_hook_list` | 437 |
| `hook.unset` | `handle_hook_unset` | 465 |
| `surface.set_mark` | `handle_set_mark` | 481 |
| `surface.read_since_mark` | `handle_read_since_mark` | 495 |
| `claude.launch` | `handle_claude_launch` | 516 |

### 의존 관계
`state::AppState`, `model::SplitDirection`, `hooks::HookEvent`, `ipc/protocol`, serde_json

### 테스트
없음.

---

## 테스트 현황 요약

| 파일 | 테스트 수 | 커버리지 영역 |
|------|-----------|--------------|
| `model.rs` | 13 | Rect 연산, PaneNode 트리, 디바이더, 분할 |
| `hooks.rs` | 16 | 이벤트 파싱/매칭, HookManager CRUD, once/persistent |
| `notification.rs` | 8 | 추가/카운트, 읽음, 병합, FIFO |
| `settings.rs` | 5 | 기본값, 직렬화, 부분 파싱 |
| `ipc/protocol.rs` | 5 | 요청/응답 직렬화 |
| **합계** | **47** | |

테스트가 없는 모듈: `main.rs`, `state.rs`, `terminal.rs`, `renderer.rs`, `gpu.rs`, `ui.rs`, `cli.rs`, `settings_ui.rs`, `ipc/server.rs`, `ipc/handler.rs`.
- `font.rs`: FontConfig 생성 테스트 4개 (기본 모노스페이스, 명시적 모노스페이스, 이름 지정 패밀리, 크기별 비교).
