# 아키텍처 개요

Tasty는 Cargo 워크스페이스 기반 크로스 플랫폼 GPU 가속 터미널 에뮬레이터다. 2개의 독립 크레이트(tasty-hooks, tasty-terminal)와 메인 바이너리로 구성된다.

## 기술 스택

| 역할 | 라이브러리 | 버전 |
|------|-----------|------|
| 윈도우/입력 | winit | 0.30 |
| GPU 렌더링 | wgpu | 24 |
| UI 위젯 | egui + egui-wgpu + egui-winit | 0.31 |
| VTE 파싱 | termwiz | 0.22 |
| PTY | portable-pty (ConPTY/Unix) | 0.8 |
| 폰트 래스터라이징 | cosmic-text + swash | - |
| IPC 프로토콜 | serde_json (JSON-RPC 2.0) | - |
| CLI | clap | - |
| 설정 파일 | toml + directories | - |
| OS 알림 | notify-rust | - |

## 프로젝트 구조

```
tasty/
├── Cargo.toml                  # 워크스페이스 루트
├── crates/
│   ├── tasty-hooks/            # 이벤트 훅 시스템 (독립 크레이트)
│   └── tasty-terminal/         # PTY + VTE 터미널 에뮬레이터 (독립 크레이트)
└── src/
    ├── main.rs                 # 진입점, App 구조체, 윈도우 생성
    ├── event_handler.rs        # winit ApplicationHandler impl
    ├── engine.rs               # Engine (IPC 서버, 윈도우 ID 관리)
    ├── engine_state.rs         # EngineState (워크스페이스, 설정, 훅, 알림 공유 상태)
    │
    ├── state/                  # 애플리케이션 상태 (윈도우당 1개)
    │   ├── mod.rs              # AppState 구조체 + 접근자
    │   ├── workspace.rs        # 워크스페이스 생성/전환/닫기
    │   ├── tab.rs              # 탭 생성/이동
    │   ├── pane.rs             # 패인 분할/닫기, close_surface_by_id
    │   ├── focus.rs            # 포커스 이동 (방향, 순차)
    │   ├── claude.rs           # Claude 자식 프로세스 관리
    │   ├── message.rs          # Surface 간 메시징
    │   ├── layout.rs           # 리사이즈, 렌더 리전 계산
    │   ├── mouse.rs            # 디바이더 드래그, 커서 스타일
    │   ├── mark.rs             # Read mark, 타이핑 감지
    │   └── tests.rs            # 유닛 테스트
    │
    ├── model/                  # 데이터 모델 (Workspace/Pane/Tab/Panel/Surface)
    │   ├── mod.rs              # Rect, SplitDirection, DividerInfo, 공통 타입
    │   ├── workspace.rs        # Workspace 구조체
    │   ├── pane_tree.rs        # PaneNode 이진 트리 (상위 분할)
    │   ├── pane.rs             # Pane 구조체 + 탭 관리
    │   ├── tab.rs              # Tab 구조체 + Panel lazy init
    │   ├── panel.rs            # Panel enum (Terminal/SurfaceGroup/Markdown/Explorer)
    │   ├── surface_group.rs    # SurfaceGroupNode wrapper + DeferredSpawn
    │   ├── surface_layout.rs   # SurfaceGroupLayout 이진 트리 (하위 분할)
    │   ├── markdown_panel.rs   # 마크다운 뷰어 데이터
    │   ├── explorer_panel.rs   # 파일 탐색기 데이터
    │   └── tests.rs            # 모델 유닛 테스트
    │
    ├── gpu/                    # GPU 상태 관리
    │   ├── mod.rs              # GpuState 구조체, new(), render() 오케스트레이션
    │   ├── render_pass.rs      # clear/terminal/egui 렌더 패스
    │   ├── egui_bridge.rs      # egui 프레임 실행, IME preedit, 테마 적용
    │   ├── fonts.rs            # CJK 폰트 로딩
    │   ├── screenshot.rs       # 프레임 캡처 → PNG
    │   └── shell_setup.rs      # 셸 경로 확인 UI
    │
    ├── renderer/               # wgpu 기반 셀 렌더러
    │   ├── mod.rs              # CellRenderer, prepare, render_scissored
    │   ├── pipeline.rs         # wgpu 파이프라인/바인드그룹 초기화
    │   ├── line_render.rs      # scrollback/surface 라인 렌더 통합
    │   ├── shaders.rs          # WGSL 셰이더 소스
    │   ├── palette.rs          # ANSI 컬러 팔레트
    │   └── types.rs            # GPU 데이터 타입 (Uniforms, BgInstance, GlyphInstance)
    │
    ├── ui/                     # egui UI 컴포넌트
    │   ├── mod.rs              # draw_ui() 진입점
    │   ├── sidebar.rs          # 사이드바 (축소/전체)
    │   ├── tab_bar.rs          # 패인별 탭 바
    │   ├── notification.rs     # 알림 패널
    │   ├── context_menu.rs     # 우클릭 컨텍스트 메뉴
    │   ├── dialog.rs           # 이름변경/마크다운 다이얼로그
    │   ├── divider.rs          # 분할선/서피스 하이라이트
    │   └── non_terminal.rs     # 마크다운/탐색기 패널 렌더
    │
    ├── tasty_window/           # 윈도우 이벤트 처리
    │   ├── mod.rs              # TastyWindow, handle_window_event dispatch
    │   ├── keyboard.rs         # 키보드 입력, IME
    │   ├── mouse.rs            # 마우스 클릭/이동/휠
    │   ├── selection.rs        # 텍스트 선택 (Normal/Word/Line)
    │   ├── redraw.rs           # 프레임 렌더, 터미널 이벤트 처리
    │   └── clipboard.rs        # 붙여넣기, 이미지 저장
    │
    ├── cli/                    # CLI 클라이언트
    │   ├── mod.rs              # Cli/Commands enum, run_client()
    │   ├── request.rs          # Commands → JSON-RPC 변환
    │   ├── format.rs           # 출력 포맷팅
    │   ├── claude.rs           # claude-hook, claude-wait 처리
    │   └── transport.rs        # TCP 통신
    │
    ├── ipc/                    # IPC 서버
    │   ├── mod.rs
    │   ├── protocol.rs         # JSON-RPC 2.0 프로토콜
    │   ├── server.rs           # TCP 서버
    │   └── handler/            # 요청 핸들러
    │       ├── mod.rs          # 라우터 dispatch
    │       ├── workspace.rs    # workspace.* 핸들러
    │       ├── pane.rs         # pane.*, split, focus.direction
    │       ├── tab.rs          # tab.*, open_markdown/explorer
    │       ├── surface.rs      # surface.* 핸들러
    │       ├── claude.rs       # claude.* 핸들러 (spawn/kill/broadcast 등)
    │       ├── hooks.rs        # hook.*, global_hook.*, surface.fire_hook
    │       ├── notification.rs # notification.*
    │       ├── message.rs      # message.*
    │       └── meta.rs         # surface.meta_*
    │
    ├── settings/               # 설정 시스템
    │   ├── mod.rs              # Settings 구조체, load()/save()
    │   ├── general.rs          # GeneralSettings + Shell 감지/검증
    │   ├── appearance.rs       # AppearanceSettings + hex 색상 파싱
    │   ├── keybindings.rs      # KeybindingSettings + preset
    │   └── types.rs            # Clipboard/Zoom/Performance/Notification 설정
    │
    ├── settings_ui/            # 설정 UI
    │   ├── mod.rs              # draw_settings_panel()
    │   ├── keybindings_tab.rs  # 키바인딩 탭 (캡처/프리셋)
    │   └── tabs.rs             # General/Appearance/Clipboard/Notification/Language/Performance 탭
    │
    ├── shortcuts.rs            # 키보드 단축키 매칭 + 실행
    ├── font.rs                 # FontConfig + GlyphAtlas (래스터라이징)
    ├── theme.rs                # Catppuccin Mocha 테마
    ├── selection.rs            # 텍스트 선택 좌표 정규화
    ├── click_cursor.rs         # 클릭→커서 이동
    ├── notification.rs         # NotificationStore + OS 알림
    ├── global_hooks.rs         # GlobalHookManager (타이머/파일 감시)
    ├── surface_meta.rs         # Surface별 메타데이터 저장소
    ├── modal_window.rs         # 설정 모달 윈도우
    ├── i18n.rs                 # 국제화 (TOML 번역)
    ├── crash_report.rs         # 크래시 리포트 수집
    ├── markdown_ui.rs          # 마크다운 렌더링 (egui)
    └── explorer_ui.rs          # 파일 탐색기 렌더링 (egui)
```

## 모듈 의존성 (DAG)

```
main.rs
├── tasty_window/       ← 윈도우 이벤트 처리
│   ├── gpu/            ← GPU 렌더링 + egui
│   │   ├── renderer/   ← 셀 렌더러
│   │   │   └── font    ← 폰트/아틀라스
│   │   └── ui/         ← egui UI 컴포넌트
│   ├── state/          ← 애플리케이션 상태
│   │   └── engine_state ← 공유 엔진 상태
│   │       └── settings/ ← 설정 (최하위, 외부 의존 없음)
│   └── shortcuts       ← 단축키 처리
├── modal_window        ← 설정 모달
│   └── settings_ui/    ← 설정 UI
└── cli/                ← CLI 클라이언트 (독립)
    └── ipc/            ← IPC 서버 + 핸들러
```

순환 의존 없음. settings가 최하위, main이 최상위.

## 계층 구조

### 기반 계층 (Foundation)

| 모듈 | 역할 | 외부 의존 |
|------|------|-----------|
| `settings/` | TOML 설정 로드/저장 | toml, directories |
| `model/` | 데이터 모델 (Workspace~Surface) | tasty-terminal |
| `ipc/protocol` | JSON-RPC 2.0 타입 | serde_json |
| `notification` | 알림 저장소 + OS 알림 | notify-rust |
| `selection` | 텍스트 선택 좌표 | (없음) |

### 중간 계층 (Services)

| 모듈 | 의존 대상 |
|------|-----------|
| `font` | cosmic-text, wgpu |
| `renderer/` | font, model, selection, wgpu |
| `ipc/server` | ipc/protocol |
| `ipc/handler/` | state, model, ipc/protocol |
| `engine_state` | model, settings, notification |

### 상위 계층 (Composition)

| 모듈 | 의존 대상 |
|------|-----------|
| `state/` | engine_state, model, settings_ui |
| `ui/` | state, model, theme |
| `settings_ui/` | settings, theme |
| `gpu/` | renderer, ui, state, settings |
| `tasty_window/` | gpu, state, shortcuts, selection |

### 최상위 계층 (Integration)

| 모듈 | 역할 |
|------|------|
| `main.rs` | App 구조체, winit 이벤트 루프, 윈도우 관리, IPC 디스패치 |
| `cli/` | CLI 클라이언트 (GUI와 독립) |

## 데이터 흐름

1. **키보드 입력 → 화면**: winit KeyEvent → TastyWindow → shortcuts/send_key → Terminal → PTY → 리더 스레드 → EventLoopProxy → process → CellRenderer → wgpu
2. **PTY 출력 → 렌더링**: PTY 리더 → Terminal::process → termwiz Parser → Surface → CellRenderer::prepare → 2-pass 렌더
3. **IPC 요청 → 응답**: TCP → IpcServer → mpsc → main process_ipc → handler::handle → AppState → JsonRpcResponse → TCP
4. **알림**: Terminal 이벤트 → NotificationStore → 사이드바 배지 + 알림 패널

## 모듈 크기 분포

91개 .rs 파일, 합계 16,635줄, 평균 182줄.

300줄 이상 파일(16개)은 모두 본질적으로 큰 코드:
- 재귀 바이너리 트리: pane_tree(456), surface_layout(380)
- 테스트: tests(551, 278)
- 매핑 테이블/enum: keybindings_tab(405), cli/mod(381)
- wgpu 선언: pipeline(349)
- 래스터라이저: font(408)
- IPC 핸들러 집합: claude(425), surface(330)

## 하위 문서

| 문서 | 설명 |
|------|------|
| [모듈별 상세](modules.md) | 디렉토리 모듈별 책임, 설계 목적, 한계 |
| [데이터 흐름](data-flows.md) | 5가지 주요 데이터 흐름 단계별 설명 |
| [라이브러리 분리 분석](library-separation/index.md) | 분리 후보 다관점 분석 |
| [의존성 분리 (파일 분할)](dependency-separation/index.md) | 대형 파일 모듈 분할 계획 |
| [리팩토링 분석](refactoring.md) | 코드 개선점, 리팩토링 로드맵 |
