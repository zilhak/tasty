# Tasty - 구현된 기능

## 터미널 엔진

### PTY 기반 셸 실행
- ConPTY(Windows) / Unix PTY를 통한 네이티브 셸 실행
- `TERM=xterm-256color` 환경 설정
- PTY 리사이즈 전파: 윈도우 크기 변경 시 자식 프로세스에 새 크기 통보
- 자식 프로세스 핸들 관리: 생존 여부 확인 가능
- PTY 채널 백프레셔: `sync_channel(32)`으로 버퍼 크기 제한 (32 * 8KB = 256KB), 버퍼 가득 차면 PTY 리더 스레드 블로킹

### VTE 파싱 및 터미널 에뮬레이션
- termwiz `Parser`를 통한 VT 이스케이프 시퀀스 파싱
- termwiz `Surface`를 통한 셀 그리드 상태 관리
- 지원하는 시퀀스:
  - **텍스트 출력**: Print, PrintString
  - **제어 코드**: LF, CR, BS, HT, Bell
  - **SGR (텍스트 속성)**: Reset, Intensity(Bold/Dim), Underline, Italic, Blink, Inverse, Invisible, StrikeThrough, Foreground/Background 색상
  - **커서 이동**: Up/Down/Left/Right, Position(CUP), CharacterAbsolute(CHA), LinePositionAbsolute(VPA), NextLine(CNL), PrecedingLine(CPL), Save/Restore
  - **화면 편집**: EraseInDisplay(ED 0/2/3), EraseInLine(EL 0/1/2), ScrollUp(SU), ScrollDown(SD), ClearScreen, ClearToEndOfLine, ClearToEndOfScreen, EraseToStartOfDisplay, EraseToStartOfLine, DeleteCharacter(DCH), InsertCharacter(ICH), DeleteLine(DL), InsertLine(IL), EraseCharacter(ECH). DCH/ICH는 전각 문자(CJK)의 2셀 너비를 올바르게 처리
  - **ESC 시퀀스**: DECSC/DECRC(커서 저장/복원), IND(인덱스, ESC D), RI(역방향 인덱스, ESC M), RIS(전체 리셋). IND/RI는 스크롤 리전 경계에서 ScrollRegionUp/Down을 수행
  - **DECSET/DECRST (CSI ? Pm h/l)**: 터미널 모드 전환
    - DECCKM (모드 1): 애플리케이션 커서 키 — 방향키가 `\x1bO{A..D}` 시퀀스를 전송
    - DECTCEM (모드 25): 커서 가시성 제어
    - 대체 화면 버퍼 (모드 47/1047/1049): vim, htop, less, nano 등 TUI 앱 지원. 모드 1049는 커서 저장/복원 및 화면 클리어 포함
    - 마우스 트래킹 (모드 1000/1002/1003): 클릭/셀 모션/전체 모션 추적
    - SGR 마우스 (모드 1006): 확장 마우스 좌표 인코딩
    - 포커스 트래킹 (모드 1004): FocusIn/FocusOut 이벤트
    - 브래킷 붙여넣기 (모드 2004): 붙여넣기 텍스트를 브래킷으로 감쌈
    - 커서 저장/복원 (모드 1048)
  - **스크롤 리전 (DECSTBM)**: `CSI Pt;Pb r`로 스크롤 영역 설정. InsertLine/DeleteLine/LineFeed/Index/ReverseIndex가 스크롤 리전 내에서 동작

### 키보드 입력
- winit `KeyEvent.text`를 활용한 수정자 키 반영 (Ctrl+C 등 제어 문자 자동 처리)
- 특수 키 매핑: Enter, Backspace, Tab, Escape, 방향키, Home/End, PageUp/PageDown, Insert/Delete, F1~F12
- DECCKM 모드에 따른 방향키 시퀀스 자동 전환: 일반 모드 `\x1b[{A..D}` / 애플리케이션 모드 `\x1bO{A..D}`

### 스크롤백 버퍼
- 화면 위로 스크롤된 줄을 `VecDeque`에 보관하여 이전 출력을 다시 볼 수 있음
- 기본 10,000줄, 설정에서 0~100,000줄까지 조절 가능 (`scrollback_lines`)
- 마우스 휠로 스크롤백 탐색 (일반 모드), PageUp/PageDown으로 페이지 단위 이동
- 대체 화면(vim, less, htop 등)에서는 스크롤백 비활성 — 모든 입력이 PTY로 전달됨
- 키보드 입력(타이핑) 시 자동으로 최하단(라이브 뷰)으로 복귀
- 스크롤백 중에는 새 PTY 출력이 도착해도 스크롤 위치를 유지 — 새 라인이 추가되면 scroll_offset을 자동 보정하여 동일한 위치를 표시
- 스크롤 시 GPU 렌더러가 스크롤백 라인과 현재 화면 라인을 혼합하여 표시. 전각 문자(CJK, 한글 등)의 2셀 너비를 올바르게 반영하여 배치
- `ScrollRegionUp`(전체 화면 스크롤)과 `\n`(커서가 하단에 있을 때) 발생 시 최상단 줄 캡처

### GPU 가속 렌더링
- wgpu 기반 크로스 플랫폼 GPU 렌더링
- `Arc<Window>` 기반 안전한 surface 생명주기 관리 (unsafe transmute 제거)
- 구조체 드롭 순서 보장: GPU 리소스가 윈도우보다 먼저 해제
- 인스턴스 렌더링 기반 2-pass 파이프라인:
  - Pass 1: 셀 배경색 쿼드
  - Pass 2: 알파 블렌딩 글리프 쿼드
- WGSL 셰이더: NDC 변환, 텍스처 샘플링

### 폰트 래스터라이징
- cosmic-text FontSystem/SwashCache를 이용한 글리프 래스터라이징
- 2048x2048 R8 텍스처 아틀라스에 선반(shelf) 기반 글리프 패킹
- 아틀라스 가득 찰 때 자동 리셋 및 재구축 (캐시 초기화 + 텍스처 클리어)
- 베이스라인 기반 글리프 오프셋 계산
- Bold/Italic 변형 지원
- Mask/Color/SubpixelMask 콘텐츠 타입별 그레이스케일 변환 (`chunks_exact` 사용)

### 색상 지원
- xterm-256color 팔레트: ANSI 16색, 216색 큐브, 24단계 그레이스케일
- TrueColor (24-bit RGB) 지원
- SGR을 통한 전경색/배경색 개별 설정

### 윈도우 관리
- winit 기반 크로스 플랫폼 윈도우 생성
- 리사이즈 시 뷰포트 유니폼 자동 갱신 및 터미널 그리드 재조정
- DPI 변경 감지: `ScaleFactorChanged` 이벤트 처리로 모니터 간 이동 시 스케일 팩터 자동 갱신
- 모노스페이스 폰트 기반 셀 그리드 레이아웃 (기본 14pt)

### 이벤트 드리븐 렌더 루프
- `EventLoopProxy<AppEvent>` 기반 PTY 웨이크업: PTY 리더 스레드에서 데이터 수신 시 `AppEvent::TerminalOutput` 이벤트를 메인 이벤트 루프로 전송
- 무조건적 `request_redraw()` 제거: 이전에는 매 프레임 끝에 `request_redraw()`를 호출하여 VSync 기반 busy-loop을 실행했으나, 이제는 실제 변경이 있을 때만 redraw 요청
- 웨이크업 소스:
  - PTY 출력 → `AppEvent::TerminalOutput` → `user_event()` → `request_redraw()`
  - 키보드/마우스 입력 → `window_event()` → dirty 플래그 설정 → `request_redraw()`
  - 윈도우 리사이즈/포커스 → `window_event()` → dirty 플래그 설정 → `request_redraw()`
  - IPC 명령 → `process_ipc()` → dirty 플래그 설정 → `request_redraw()`
- `Waker` 타입 (`Arc<dyn Fn() + Send + Sync>`): Terminal 생성 시 전달되어 PTY 리더 스레드가 이벤트 루프를 깨울 수 있게 함
- Waker 전파 경로: `App` → `AppState` → `Workspace` → `Pane` → `Tab` → `Terminal`
- CPU 유휴 시 0% 사용: 터미널 출력이 없고 사용자 입력이 없으면 이벤트 루프가 대기 상태로 진입

## 워크스페이스 & 탭

### 데이터 모델

용어 정의는 `docs/design/ubiquitous-language.md` 참조.

- Workspace: 최상위 컨테이너. 상위 레이아웃(PaneNode 이진 트리)을 소유
- PaneNode: Pane Group의 상위 레이아웃 트리. Leaf(Pane) 또는 Split. 탭 전환과 무관하게 고정
- Pane (= Pane Group): **독립적인 탭 바**를 가진 화면 영역. 여러 Tab을 포함
- Tab (= Pane): 탭 하나. Panel에 매핑
- Panel: 콘텐츠 타입 enum. Terminal(단일), SurfaceGroup(하위 레이아웃), Markdown, Explorer
- SurfaceGroupNode: 하위 레이아웃 트리. 탭 전환 시 함께 전환
- Surface: 실제 터미널 인스턴스 (PTY + termwiz Surface)
- AppState: 전체 워크스페이스 목록과 활성 상태를 관리하는 중앙 상태 (IdGenerator 포함)

### egui UI 오버레이
- egui-winit + egui-wgpu를 이용한 wgpu 위 egui 렌더링
- 좌측 SidePanel: 워크스페이스 목록, 활성 표시, 추가 버튼
- Pane별 탭 바: 각 Pane의 rect 상단에 egui Area로 렌더링 (탭 2개 이상일 때만 표시)
- 글로벌 상단 탭 바 제거, Pane별 독립 탭 바로 전환
- 다크 테마 적용 (패널 배경색 커스터마이징)
- 사이드바에 키보드 단축키 안내 표시

### 두 가지 분할 유형
- **Pane 분할** (Ctrl+Shift+E/O): 물리적 화면 분할. PaneNode 이진 트리 기반. 각 영역이 독립 탭 바를 가진다
- **SurfaceGroup 분할** (Ctrl+D / Ctrl+Shift+D): 탭 내부 분할. SurfaceGroupLayout 이진 트리 기반. 탭 바에서는 하나의 탭으로 표시된다
- Panel::Terminal이 분할 시 자동으로 Panel::SurfaceGroup으로 변환
- **패닉 없는 분할 구현**: PTY/Terminal을 구조적 변경 이전에 선행 생성 — 리소스 생성 실패 시 레이아웃이 변경되지 않음
- `PaneNode::split_pane_in_place`: `std::mem::replace` 2-step 패턴으로 소유권 이동 없이 트리 내부 노드를 in-place 변경
- `SurfaceGroupLayout::split_with_node`: 소유권 기반 infallible 분할 — 사전 생성된 SurfaceNode를 받아 구조적 변경 중 패닉 경로 없음
- Workspace/Tab/SurfaceGroupNode 내부 Option 래핑 + take/put 패턴: split 함수가 infallible이므로 take 이후 put이 항상 실행됨 보장
- 각 Surface를 scissor rect로 독립 렌더링
- 뷰포트별 유니폼 갱신 (grid_offset을 각 Surface rect에 맞게 조정)

### 키보드 단축키

#### 플랫폼별 수정자 키 매핑

바인딩 문자열에서 `"alt"`는 macOS에서 Cmd(⌘) 키에 매핑된다. macOS 키보드의 Cmd 위치가 Windows/Linux의 Alt 위치와 물리적으로 일치하기 때문이다. 예를 들어 `"alt+n"` 바인딩은:
- **Windows/Linux**: Alt+N
- **macOS**: Cmd+N (⌘N)

| 바인딩 토큰 | Windows/Linux | macOS |
|-------------|---------------|-------|
| `ctrl` | Ctrl | Ctrl |
| `alt` | Alt | Cmd (⌘) |
| `shift` | Shift | Shift |

#### 기본 단축키 (Tasty 프리셋)

- Alt+N (macOS: ⌘N): 새 워크스페이스
- Alt+T (macOS: ⌘T): 포커스된 Pane에 새 탭
- Alt+E (macOS: ⌘E): Pane 수직 분할
- Alt+Shift+E (macOS: ⌘⇧E): Pane 수평 분할
- Alt+D (macOS: ⌘D): SurfaceGroup 수직 분할 (탭 내부)
- Alt+Shift+D (macOS: ⌘⇧D): SurfaceGroup 수평 분할 (탭 내부)
- Alt+] / Alt+[ (macOS: ⌘] / ⌘[): Surface 포커스 다음/이전
- Ctrl+] / Ctrl+[: Pane 포커스 다음/이전
- Alt+1~9 (macOS: ⌘1~9): 워크스페이스 전환
- Ctrl+1~0: 탭 전환
- Ctrl+Tab / Ctrl+Shift+Tab: 다음/이전 탭
- Ctrl+Shift+B: 사이드바 토글 (숨김/표시)
- Ctrl+B: 사이드바 접기/펼치기

### 방향성 포커스 이동 (IPC/CLI)
- `focus.direction` IPC 메서드: `direction` 파라미터 (`"left"`, `"right"`, `"up"`, `"down"`)로 분할 트리 구조 기반 방향성 포커스 이동
- `tasty focus-direction <방향>` CLI 커맨드로도 동일하게 사용 가능
- 알고리즘: SplitDirection 트리를 역방향으로 탐색하여 이동 방향에 맞는 분할을 찾고, 시블링 서브트리의 엣지 리프로 포커스 이동
  - `SplitDirection::Vertical`(좌우 경계) → Left/Right 방향에 대응
  - `SplitDirection::Horizontal`(상하 경계) → Up/Down 방향에 대응
  - Left/Up: 시블링의 rightmost/bottommost 리프로 이동 (인접한 엣지)
  - Right/Down: 시블링의 leftmost/topmost 리프로 이동 (인접한 엣지)
- SurfaceGroup 내부 서피스 간 이동 우선, 이동 불가 시 Pane 간 이동
- close_surface 단축키: 포커스된 서피스 닫기. 서피스가 하나뿐이면 상위 패인 닫기로 fallback (패인이 2개 이상일 때)
- Ctrl+W: 활성 탭 닫기 (탭이 2개 이상일 때)
- Ctrl+Shift+W: 포커스된 패인 닫기 (unsplit, 패인이 2개 이상일 때)
- Ctrl+Shift+I: 알림 패널 토글
- Ctrl+,: 설정 모달 윈도우 열기 (독립 OS 윈도우, 모달 활성 시 다른 윈도우 입력 차단)
- Ctrl+D: 터미널에 전달 (EOF). 이전에는 Surface 수직 분할이었으나, Ctrl+Shift+D로 변경
- winit ModifiersState를 이용한 수정자 키 추적

### 마우스 인터랙션
- **클릭으로 Pane 포커스**: 터미널 영역 좌클릭 시 해당 Pane이 포커스됨. `cursor_position` 추적 + `focus_pane_at_position()`으로 어떤 Pane인지 판별
- **클릭으로 Surface 포커스**: SurfaceGroup 내에서 특정 터미널을 클릭하면 해당 Surface가 포커스됨. `focus_surface_at_position()`으로 클릭 좌표에서 Surface ID를 찾아 전환
- **디바이더 드래그로 분할 비율 조절**: Pane 또는 SurfaceGroup 분할 경계선을 마우스 드래그하여 비율 조정 (0.1~0.9 범위 클램프). `DividerDrag` 상태 머신으로 드래그 시작/이동/종료를 추적. 드래그 중 실시간 리사이즈 적용
- **디바이더 호버 시 커서 변경**: 분할 경계선에 4px 이내로 마우스를 가져가면 커서가 리사이즈 아이콘으로 변경 (수직 분할: ColResize, 수평 분할: RowResize). 벗어나면 Default로 복귀
- **마우스 스크롤**: 일반 모드에서 마우스 휠은 스크롤백 버퍼를 탐색함. 대체 화면(vim, less 등)에서는 방향키 시퀀스(`\x1b[A`/`\x1b[B`)를 PTY에 전달. LineDelta와 PixelDelta 모두 지원
- **egui와의 이벤트 충돌 방지**: egui가 이벤트를 소비한 경우 (사이드바, 설정 윈도우 등) 터미널에는 전달하지 않음
- 관련 모델 메서드: `Rect::contains()`, `PaneNode::find_divider_at()`, `PaneNode::update_ratio_for_rect()`, `SurfaceGroupLayout::find_divider_at()`, `SurfaceGroupLayout::update_ratio_for_rect()`, `SurfaceGroupLayout::find_surface_at()`

### 비터미널 패널 (Markdown Viewer / Explorer)
- Panel enum에 `Markdown(MarkdownPanel)`과 `Explorer(ExplorerPanel)` 변형 추가
- PTY가 없는 순수 egui 렌더링 패널. 터미널 관련 메서드(focused_terminal, render_regions 등)는 None/empty 반환
- egui Area로 해당 패인 rect에 오버레이 렌더링

#### Markdown Viewer
- 마크다운 파일을 egui로 렌더링하는 읽기 전용 뷰어
- 지원 문법: 제목(#, ##, ###), 목록(-, *), 인용(>), 수평선(---), 코드 블록(```), 테이블(|), 인라인 서식(**볼드**, *이탤릭*, \`코드\`)
- 파일 경로를 지정하여 열기 (IPC/CLI/우클릭 메뉴)
- 탭으로 열리며 파일명이 탭 이름이 됨

#### Explorer
- 디렉토리 트리와 파일 미리보기를 제공하는 파일 탐색기
- 왼쪽 트리 + 오른쪽 뷰어의 2-컬럼 레이아웃
- 디렉토리 확장/축소, 파일 클릭 시 내용 미리보기
- .md 파일 선택 시 마크다운 렌더링, 기타 파일은 모노스페이스 텍스트 표시
- 숨김 파일 기본 제외 (.env, .gitignore, .claude는 표시)
- 디렉토리 우선, 대소문자 무시 이름순 정렬

#### 패인 우클릭 컨텍스트 메뉴
- 터미널 영역에서 마우스 우클릭 시 컨텍스트 메뉴 표시
- "Open Markdown..." → 파일 경로 입력 다이얼로그 → 마크다운 탭 열기
- "Open Explorer" → 홈 디렉토리를 루트로 하는 탐색기 탭 열기
- 좌클릭 또는 Cancel로 메뉴 닫기

#### IPC/CLI 지원
- `tab.open_markdown`: `file_path` 파라미터로 마크다운 탭 열기 (`pane_id` 옵션)
- `tab.open_explorer`: `path` 파라미터로 탐색기 탭 열기 (생략 시 홈 디렉토리, `pane_id` 옵션)
- CLI: `tasty open-markdown <path>`, `tasty open-explorer [--path <dir>]`

## 알림 시스템

### OSC 시퀀스 감지
- termwiz Parser에서 파싱된 OSC 액션을 인터셉트하여 알림 이벤트 생성
- 지원하는 시퀀스:
  - **OSC 9**: iTerm2/ConEmu 알림 (`\e]9;message\e\\`)
  - **OSC 99**: Kitty 알림 (`\e]99;key=value;...\e\\`), Unspecified로 파싱된 것을 수동 처리
  - **OSC 777**: rxvt-unicode 알림 (`\e]777;notify;title;body\e\\`)
  - **OSC 7**: 현재 작업 디렉토리 변경 (`\e]7;file://host/path\e\\`)
  - **OSC 0/2**: 윈도우 타이틀 변경
  - **BEL** (`\x07`): 벨 알림
- TerminalEvent / TerminalEventKind enum을 통한 이벤트 전달
- `take_events()` 메서드로 축적된 이벤트를 소비

### NotificationStore (notification.rs)
- VecDeque 기반 FIFO 알림 저장소 (최대 100개, 초과 시 `pop_front()`로 O(1) 삭제)
- 알림 병합(coalescing): 같은 소스에서 설정 가능한 간격(기본 500ms) 이내 연속 알림이 오면 기존 알림에 합침
- `with_coalesce_ms()`: 커스텀 병합 간격으로 생성
- 워크스페이스별 읽지 않은 알림 카운트 제공
- 개별 알림 또는 전체 읽음 처리

### 시스템 알림 (notify-rust)
- 윈도우가 비활성 상태일 때 OS 네이티브 알림 전송
- 초당 1회 제한(rate limiting)으로 알림 폭주 방지
- Windows/macOS/Linux 크로스 플랫폼 지원

### 사이드바 알림 배지
- 워크스페이스 이름 옆에 읽지 않은 알림 수 표시 (`[N]`)
- 읽지 않은 알림이 있는 워크스페이스는 파란색으로 강조
- 사이드바 헤더에 전체 읽지 않은 알림 수 배지

### 알림 패널 (Ctrl+I)
- egui Window 오버레이로 구현된 알림 목록
- 스크롤 가능한 최신순 정렬 알림 표시
- 각 알림에 워크스페이스 이름, 제목, 본문, 경과 시간 표시
- "Jump" 버튼으로 해당 워크스페이스로 즉시 전환
- 패널 열 때 자동으로 전체 읽음 처리
- "Mark all read" 버튼 제공

### 이벤트 수집 파이프라인
- AppState.collect_events()가 모든 워크스페이스의 모든 터미널에서 이벤트 수집
- AppState.process_all()이 모든 워크스페이스의 PTY 채널을 처리 (비활성 워크스페이스 메모리 누수 방지)
- main.rs 이벤트 루프에서 process_all() 후 이벤트 수집 및 알림 처리
- 윈도우 포커스 상태 추적으로 시스템 알림 발송 조건 판단

### 터미널 뷰포트 관리
- egui 사이드바를 제외한 전체 영역에 상위 레이아웃(PaneNode 트리) 렌더링
- PaneNode에서 각 Pane의 rect를 계산, 탭 바 높이를 뺀 영역에 터미널 렌더링
- 탭 바 높이는 egui 렌더링 시 실측된 값을 사용 (하드코딩 아님)
- 리사이즈 시 모든 Pane, 모든 Tab, 모든 Surface의 행/열 재계산
- wgpu RenderPass의 forget_lifetime()을 이용한 egui-wgpu 호환

## 설정 시스템

### TOML 기반 설정 파일
- 설정 파일 경로: `~/.tasty/config.toml` (전 플랫폼 통일)
- `directories` 크레이트로 플랫폼별 홈 디렉토리 추상화
- `toml` + `serde` 기반 직렬화/역직렬화
- 설정 파일이 없거나 파싱 실패 시 기본값으로 폴백

### 설정 카테고리
- **General**: 셸 경로 (OS별 자동 감지: COMSPEC/SHELL), 시작 명령, 스크롤백 줄 수 (기본 10,000)
- **Appearance**: 폰트 패밀리 (기본값: 시스템 모노스페이스), 폰트 크기, 테마 (dark/light), 배경 투명도, 사이드바 너비
- **Clipboard**: OS별 기본 활성화 (macOS: Alt+C/V, Linux: Ctrl+Shift+C/V, Windows: Ctrl+C/V)
- **Notifications**: 알림 활성화, 시스템 알림, 사운드, 병합 간격(ms)
- **Keybindings**: 워크스페이스/탭/패인/서피스 분할 단축키

### GUI 설정 윈도우
- Ctrl+, 단축키로 설정 윈도우 토글
- egui Window 기반 탭 인터페이스 (General / Appearance / Clipboard / Notifications / Keybindings / Language)
- egui에 시스템 CJK 폰트 로드: Windows(맑은 고딕), macOS(AppleSDGothicNeo), Linux(Noto Sans CJK)
- 편집 중 원본 설정을 보존하는 드래프트 패턴
- Save 버튼: 디스크에 저장 후 즉시 적용
- Cancel 버튼: 변경 사항 폐기

### 설정 로드/저장
- `Settings::load()`: 설정 파일 로드, 없으면 기본값 반환
- `Settings::save()`: 설정 디렉토리 자동 생성 후 TOML 형식으로 저장
- `Settings::config_path()`: 플랫폼 독립적 설정 파일 경로 반환
- 앱 시작 시 자동 로드, AppState에 통합

### 설정 연동
- `settings.general.shell`: Terminal 생성 시 커스텀 셸 경로 사용 (비어있으면 OS 기본 셸)
- `settings.general.startup_command`: 첫 터미널 생성 후 자동 실행할 명령. 비어있으면 무시
- `settings.appearance.font_family`: GPU 렌더러의 cosmic-text FontSystem에 전달. `FamilyOwned::Name`으로 해석되며, 빈 문자열이나 "monospace"이면 시스템 기본 모노스페이스 사용
- `settings.appearance.font_size`: GpuState 생성 시 CellRenderer에 전달. 기본값 14.0
- `settings.appearance.theme`: egui Visuals 설정에 반영. "dark" → `Visuals::dark()`, "light" → `Visuals::light()`. wgpu clear color도 테마에 따라 변경. 설정 저장 후 실시간 반영
- `settings.appearance.background_opacity`: wgpu clear color의 알파 값으로 적용. 0.0(투명)~1.0(불투명)
- `settings.appearance.sidebar_width`: 사이드바 너비가 UI, GPU 렌더러, 터미널 rect 계산에 반영. 렌더 루프에서 설정값과 자동 동기화
- `settings.clipboard.windows_style`: Ctrl+V 붙여넣기 활성화
- `settings.clipboard.linux_style`: Ctrl+Shift+V 붙여넣기 활성화
- `settings.clipboard.macos_style`: Alt+V 붙여넣기 활성화
- `settings.notification.enabled`: 알림 활성화/비활성화. 비활성 시 알림 수집 및 시스템 알림 모두 차단
- `settings.notification.system_notification`: OS 네이티브 알림 개별 제어
- `settings.notification.coalesce_ms`: NotificationStore 생성 시 병합 간격 전달
- `settings.notification.sound`: UI 체크박스만 존재. 사운드 재생 미구현 (TODO)
- `settings.keybindings.*`: UI에 미노출. 현재 main.rs에서 하드코딩된 단축키 사용 (TODO: 파싱 및 적용)

## 클립보드

### arboard 기반 크로스 플랫폼 클립보드
- `arboard` 크레이트를 사용한 시스템 클립보드 읽기/쓰기
- 앱 시작 시 `Clipboard` 인스턴스를 생성하여 App 구조체에 보관

### 텍스트 선택 (Text Selection)
- 마우스 드래그로 터미널 텍스트 선택
- 선택 모드:
  - **Normal**: 문자 단위 드래그 선택
  - **Word**: 더블클릭으로 단어 선택
  - **Line**: 트리플클릭으로 줄 전체 선택
- 선택 영역 시각적 하이라이트 (배경색 오버라이드, Catppuccin Surface2 기반)
- 스크롤백 영역과 화면 영역을 넘나드는 선택 지원
- 전각 문자(CJK, 한글) 2셀 너비 올바르게 처리
- 마우스 트래킹 모드(1000/1002/1003) 활성 시 Shift+드래그로 강제 선택
- 터미널 텍스트 영역 위에서 마우스 커서가 I-beam으로 변경
- 마우스 클릭으로 커서 위치 이동 (`click_cursor` 모듈, `general.click_to_move_cursor` 설정으로 on/off):
  - `EditableRegion`: 현재 셸 입력의 편집 가능 영역을 계산 (커서 위치 + 소프트 랩 연속 행)
  - 클릭 위치를 편집 가능 영역으로 클램핑한 뒤, 화살표 키를 전송하여 셸 커서 이동
  - 전각 문자(CJK, 한글) 2셀 너비를 고려하여 정확한 화살표 횟수 계산
  - 소프트 랩(긴 명령어 줄바꿈) 시 여러 줄에 걸친 이동 지원
  - 편집 불가 영역 클릭 방지:
    - 커서 행 아래(빈 영역) 클릭 시 이동하지 않음
    - 이전 명령어 출력 행 클릭 시 이동하지 않음 (소프트 랩 연속 행만 허용)
    - 커서 행에서 커서 오른쪽(빈 공간) 클릭 시 커서 위치로 클램핑
  - 스크롤백 중, alternate screen(vim 등), 마우스 트래킹 모드에서는 비활성

### 복사 (Copy)
- 설정에 따라 세 가지 단축키 지원:
  - **Windows**: Ctrl+C (`clipboard.windows_style`) — 선택 있으면 복사, 없으면 SIGINT 전달
  - **Linux**: Ctrl+Shift+C (`clipboard.linux_style`)
  - **macOS**: Alt+C (`clipboard.macos_style`)
- 선택 텍스트를 시스템 클립보드에 복사 후 선택 해제
- 키보드 입력 시 선택 자동 해제

### 붙여넣기 (Paste)
- 설정에 따라 세 가지 단축키 지원:
  - **Windows**: Ctrl+V (`clipboard.windows_style`)
  - **Linux**: Ctrl+Shift+V (`clipboard.linux_style`)
  - **macOS**: Alt+V (`clipboard.macos_style`)
- 브래킷 붙여넣기 모드(DECSET 2004) 지원: 활성화 시 `\x1b[200~` ... `\x1b[201~`로 감싸서 전송
- 포커스된 터미널의 PTY에 직접 전송

### OSC 52 클립보드 설정
- 터미널 프로그램이 OSC 52 시퀀스로 시스템 클립보드에 텍스트를 설정할 수 있음
- termwiz의 `SetSelection` 파싱을 활용하여 이벤트 발생 → main.rs에서 arboard로 클립보드에 반영

## CLI 도구 & 소켓 API

### JSON-RPC IPC 서버 (ipc/)
- GUI 모드 시작 시 `127.0.0.1`의 랜덤 포트에 TCP 서버 자동 기동
- 포트 번호를 `~/.tasty/tasty.port` 파일에 기록하여 CLI 클라이언트가 접속 가능
- `--port-file` 옵션으로 커스텀 포트 파일 경로 지정 가능 (테스트 격리용)
- 앱 종료 시 포트 파일 자동 삭제 (Drop trait)
- JSON-RPC 2.0 프로토콜: 줄 단위 JSON 요청/응답
- 멀티클라이언트: 각 TCP 연결을 별도 스레드에서 처리
- 메인 스레드 채널 통신: IPC 스레드 -> mpsc 채널 -> 이벤트 루프에서 처리 -> oneshot 응답

### 지원 메서드

모든 서피스 관련 메서드는 optional `surface_id` 파라미터를 지원한다. 지정하면 해당 서피스에 직접 접근하고, 생략하면 현재 포커스된 서피스에 작용한다.

#### 시스템
- `system.info`: 버전, 워크스페이스 수, 활성 워크스페이스 인덱스
- `system.shutdown`: 헤드리스 모드에서 프로세스를 정상 종료
- `ui.state`: GUI 오버레이 상태 조회 (settings_open, notification_panel_open, active_workspace, workspace_count, pane_count, tab_count)

#### 워크스페이스
- `workspace.list`: 전체 워크스페이스 목록 (이름, 활성 여부, 패인 수)
- `workspace.create`: 새 워크스페이스 생성 (선택적 이름 지정)
- `workspace.select`: 인덱스로 워크스페이스 전환

#### 패인
- `pane.list`: 활성 워크스페이스의 패인 목록 (포커스 여부, 탭 수)
- `pane.split`: 포커스된 패인 분할 (vertical/horizontal)
- `pane.close`: 포커스된 패인 닫기 (unsplit)
- `pane.focus`: **pane_id로 특정 패인을 직접 포커스** — 멀티패인 환경에서 원하는 패인으로 전환

#### 탭
- `tab.list`: 포커스된 패인의 탭 목록
- `tab.create`: 포커스된 패인에 새 탭 추가
- `tab.close`: 포커스된 패인의 활성 탭 닫기

#### 서피스 (터미널)
- `surface.list`: 활성 워크스페이스의 전체 서피스 목록 (id, pane_id, tab_index, cols, rows)
- `surface.focus`: **surface_id로 특정 서피스를 직접 포커스** — 해당 서피스가 속한 패인까지 자동 포커스
- `surface.close`: SurfaceGroup 내 포커스된 서피스 닫기

#### 입력
- `surface.send`: 텍스트 전송 (optional surface_id)
- `surface.send_key`: 특수키 전송 — enter, tab, escape, backspace, 방향키, home/end, pageup/pagedown, delete/insert, f1~f12 (optional surface_id)
- `surface.send_combo`: **키 조합 전송** — Ctrl+C (0x03), Ctrl+Z (0x1A), Ctrl+D (0x04), Alt+키 (ESC prefix) 등. 파라미터: `{key, modifiers: ["ctrl"|"shift"|"alt"], surface_id?}`
- `surface.send_to`: 특정 surface_id에 텍스트 직접 전송 (포커스 변경 없이)

#### 출력 읽기
- `surface.screen_text`: 화면 텍스트 조회 (optional surface_id)
- `surface.cursor_position`: 커서 위치 (x, y) 조회 (optional surface_id)
- `surface.set_mark`: 출력 읽기 마크 설정 (optional surface_id)
- `surface.read_since_mark`: 마크 이후 출력 텍스트 조회, ANSI 제거 옵션 (optional surface_id)

#### 타이핑 감지
- `surface.is_typing`: 서피스가 최근 5초 내 키 입력을 받았는지 조회. 반환: `{ typing: bool, idle_seconds: f64 }` (idle_seconds가 -1이면 입력 기록 없음). optional surface_id
- `surface.send_wait_idle`: 서피스가 유휴 상태일 때만 텍스트 전송. 타이핑 중이면 `{ sent: false, reason: "typing" }` 반환, 유휴면 전송 후 `{ sent: true }` 반환. CLI에서 폴링하여 대기 구현 가능. optional surface_id, 필수 text

#### 알림
- `notification.list`: 최근 50개 알림 목록
- `notification.create`: 알림 생성

#### 트리
- `tree`: 전체 워크스페이스/패인/탭 트리 구조 조회

#### 훅
- `hook.set`: 서피스 훅 등록 (event, command, once)
- `hook.list`: 등록된 훅 목록 조회 (서피스별 필터 가능)
- `hook.unset`: 훅 삭제

#### 글로벌 훅
- `global_hook.set`: 글로벌 훅 등록. 파라미터: `condition` (타입별 포맷), `command`, `label?`. 반환: `{ hook_id: N }`
  - `interval:SECS` — 매 N초마다 반복 실행
  - `once:SECS` — N초 후 1회 실행 후 자동 삭제
  - `file:/path` — 파일 수정 감지 시 실행
- `global_hook.list`: 등록된 글로벌 훅 전체 목록. 각 항목: `{ id, condition, command, label }`
- `global_hook.unset`: `hook_id`로 글로벌 훅 삭제. 반환: `{ removed: bool }`

#### 메시지 패싱
- `message.send`: `to_surface_id`, `content`, `from_surface_id?` — 다른 서피스의 메시지 큐에 메시지 추가. 응답: `{ id: N }`
- `message.read`: `surface_id?`, `from_surface_id?`, `peek?` — 메시지 큐 읽기. 기본적으로 소비(consume), `peek: true`이면 읽기만 하고 큐에서 제거하지 않음. `from_surface_id`로 발신자 필터 가능
- `message.count`: `surface_id?` — 대기 중인 메시지 수. 응답: `{ count: N }`
- `message.clear`: `surface_id?` — 메시지 큐 전체 삭제. 응답: `{ cleared: true }`

#### Surface 메타데이터
- `surface.meta_set`: `surface_id?`, `key`, `value` — 서피스별 메타데이터 키-값 설정. 응답: `{ ok: true }`
- `surface.meta_get`: `surface_id?`, `key` — 메타데이터 값 조회. 응답: `{ value: "..." }` 또는 `{ value: null }`
- `surface.meta_unset`: `surface_id?`, `key` — 메타데이터 키 삭제. 응답: `{ ok: true }`
- `surface.meta_list`: `surface_id?` — 전체 메타데이터 객체 반환

#### 에이전트 전용
- `claude.launch`: Claude Code 전용 워크스페이스 생성 및 실행

### 멀티 윈도우
- `window.create` IPC 또는 `tasty new-window` CLI로 새 독립 윈도우 생성
- 각 윈도우는 자체 GPU 서피스, egui 컨텍스트, 터미널 세트를 보유
- `window.list`: 전체 윈도우 목록 (id, focused, title)
- `window.close`: 포커스된 윈도우 닫기
- `window.focus`: 특정 윈도우에 포커스
- 윈도우 닫기 시 HashMap에서 제거, 마지막 윈도우면 앱 종료
- 모달 활성 시 다른 윈도우 입력 차단

### GUI 통합 테스트 프레임워크
- `tests/gui_common/mod.rs`의 `GuiTestInstance` 헬퍼: 실제 GUI 모드로 프로세스 스폰
- `enigo` 크레이트로 키보드/마우스 입력 시뮬레이션 (Windows SendInput API)
- `windows` 크레이트로 창 탐색(FindWindowW) 및 포커스 전환(SetForegroundWindow)
- IPC `ui.state` 메서드로 GUI 오버레이 상태 검증
- `wait_for_ui()`: 조건 기반 UI 상태 폴링 (타임아웃 포함)
- `measure_ui_latency()`: UI 동작별 응답 속도 측정
- IPC Waker: IPC 명령 도착 시 `EventLoopProxy`를 통해 이벤트 루프 즉시 깨움
- 24개 GUI 테스트:
  - 설정창 열기/닫기 (Ctrl+,, Escape)
  - 알림 패널 토글 (Ctrl+Shift+I, Escape)
  - 워크스페이스 생성/전환 (Ctrl+Shift+N, Alt+1~9)
  - 탭 생성/닫기 (Ctrl+Shift+T, Ctrl+W)
  - 패인 분할/닫기 (Ctrl+Shift+E/O/W)
  - 키보드 라우팅: 오버레이 열림 시 터미널 입력 차단 검증
  - 키보드 라우팅: 오버레이 없을 때 터미널 입력 전달 검증
  - 전체 워크플로우: 워크스페이스→패인→탭 CRUD 통합 시나리오
  - 속도 테스트: 설정 토글, 워크스페이스 전환, 탭 전환 반복 측정 (1초 이내 응답 보장)

### CLI 클라이언트 (cli.rs)
- `tasty` 명령에 서브커맨드가 있으면 CLI 모드, 없으면 GUI 모드로 동작
- clap 기반 서브커맨드: `list`, `new-workspace`, `select-workspace`, `send`, `send-key`, `notify`, `notifications`, `tree`, `split`, `new-tab`, `close-tab`, `close-pane`, `close-surface`, `surfaces`, `panes`, `info`, `set-hook`, `list-hooks`, `unset-hook`, `set-mark`, `read-since-mark`, `claude`, `message-send`, `message-read`, `message-count`, `message-clear`, `claude-broadcast`, `claude-wait`
- 포트 파일에서 포트 번호를 읽어 TCP 연결 후 JSON-RPC 요청/응답
- `tree` 커맨드: 워크스페이스/패인/탭 계층을 트리 형태로 표시
- 에러 시 종료 코드 1 반환

## 에이전트 자동화

tasty의 핵심 차별점으로, "에이전트가 에이전트를 제어하는 자동화"를 위한 세 가지 기능을 제공한다.

### Surface Hook 시스템 (crates/tasty-hooks)

Surface별 이벤트 훅을 등록하여 특정 이벤트 발생 시 셸 명령을 자동 실행한다.

- **HookManager**: 훅 등록/삭제/조회/실행을 관리하는 중앙 매니저
- **HookEvent 타입**:
  - `ProcessExit`: 셸 프로세스 종료 시
  - `OutputMatch(pattern)`: PTY 출력이 정규식 패턴에 매칭될 때
  - `Bell`: BEL 문자 수신 시
  - `Notification`: OSC 알림 수신 시
  - `IdleTimeout(secs)`: N초간 PTY 출력 없을 때
  - `ClaudeIdle`: Claude Code 작업 완료 시
  - `NeedsInput`: Claude Code 사용자 입력 필요 시
- **ProcessExit 구현**: 터미널 프로세스 종료 시 ProcessExited 이벤트 자동 발생 및 훅 실행
- **정규식 캐싱**: OutputMatch 훅 등록 시 정규식을 사전 컴파일하여 매칭 시 재컴파일 방지
- **once 옵션**: true로 설정하면 한 번 실행 후 자동 삭제
- **비동기 실행**: 훅 명령은 백그라운드 스레드에서 실행 (메인 루프 블로킹 없음)
- **이벤트 루프 통합**: main.rs에서 TerminalEvent 수집 후 Bell/Notification/ProcessExit 이벤트에 대해 자동으로 훅 체크 및 실행
- **Surface ID 추적**: 각 이벤트가 발생한 Surface ID를 추적하여 훅이 올바른 Surface에서 실행
- CLI: `tasty set-hook --event bell --command "notify-send 'bell'" --once`
- IPC: `hook.set`, `hook.list`, `hook.unset` 메서드

### Read Mark API (crates/tasty-terminal)

터미널 출력에 마크를 설정하고, 마크 이후의 새 출력만 효율적으로 읽는 델타 트래킹 API.

- **output_buffer**: PTY에서 수신한 원시 바이트를 최대 1MB까지 순환 버퍼에 저장
- **read_mark**: 바이트 오프셋 기반 마크 위치 추적
- **버퍼 관리**: 1MB 초과 시 오래된 데이터 자동 삭제, 마크가 잘린 영역에 있으면 무효화(None), 아니면 오프셋 조정
- **set_mark()**: 현재 버퍼 끝 위치에 마크 설정
- **read_since_mark(strip_ansi)**: 마크 이후 출력 텍스트 반환. `strip_ansi=true`이면 ANSI 이스케이프 시퀀스 제거
- **strip_ansi_escapes()**: `LazyLock<Regex>`으로 초기화 시점 한 번만 컴파일하는 정규식으로 ANSI CSI, OSC BEL, OSC ST 시퀀스 제거 (반복 호출 시 regex 재컴파일 없음)
- **Surface ID로 조회**: AppState에서 전체 워크스페이스/패인/탭/서피스 트리를 재귀 탐색하여 특정 Surface의 마크 설정/읽기 지원
- CLI: `tasty set-mark`, `tasty read-since-mark --strip-ansi`
- IPC: `surface.set_mark`, `surface.read_since_mark` 메서드

### Claude Code 런처 (claude.launch)

Claude Code를 새 워크스페이스에서 자동으로 실행하는 전용 런처.

- 새 워크스페이스 자동 생성 및 이름 설정
- 지정된 디렉토리로 이동 후 `claude` 명령 실행 (shell-escape로 인젝션 방지)
- `--task` 옵션으로 작업 설명 전달 가능 (shell-escape 적용)
- CLI: `tasty claude --workspace "my-project" --directory "/path/to/project" --task "Fix the bug"`
- IPC: `claude.launch` 메서드 (workspace, directory, task 파라미터)

### Claude Parent-Child 관계 관리

부모 Claude 인스턴스가 자식 Claude 인스턴스를 생성하고 관리하는 시스템. AI 에이전트가 멀티 에이전트 워크플로우를 구성할 때 사용한다.

- **ClaudeChildEntry**: 자식 surface ID, 인덱스, cwd, role, nickname을 추적하는 데이터 구조
- **부모-자식 매핑**: `HashMap<u32, Vec<ClaudeChildEntry>>`로 부모별 자식 목록 관리, `HashMap<u32, u32>`로 자식에서 부모 역참조
- **자동 정리**: 부모 또는 자식 surface가 닫힐 때 관계를 자동으로 정리. 부모가 먼저 닫혀도 자식이 살아있는 동안 관계 유지 (ghost cleanup)
- **claude.spawn**: 부모 pane을 분할하여 새 터미널 생성 후 `claude` 명령 자동 실행. cwd, role, nickname, prompt 파라미터 지원
- **claude.children**: 부모 surface의 자식 목록 조회. 각 자식의 surface ID, 인덱스, 메타데이터 반환
- **claude.parent**: 자식 surface의 부모 조회. 부모의 surface ID와 상태(active/closed) 반환
- **claude.kill**: 자식 surface를 종료하고 관계를 정리
- **claude.respawn**: 기존 자식을 종료하고 같은 인덱스로 새 자식을 생성. cwd, role, nickname, prompt 재설정 가능
- **claude.broadcast**: 부모의 모든 자식에게 텍스트를 동시에 전송. `role` 파라미터로 특정 역할의 자식에만 필터링 가능. 반환: `{ sent_count, children }`
- **claude.wait**: 자식 surface의 현재 상태를 조회. surface가 존재하지 않으면 "exited" 반환. 반환: `{ state: "idle"|"needs_input"|"active"|"exited" }`. CLI에서 폴링 루프로 대기 구현
- CLI: `tasty claude-spawn --direction vertical --cwd /path --role worker --nickname "agent-1" --prompt "Fix bugs"`
- CLI: `tasty claude-children`, `tasty claude-parent`, `tasty claude-kill --child 5`, `tasty claude-respawn --child 5`
- CLI: `tasty claude-broadcast "text\r" [--role ROLE]`, `tasty claude-wait --child ID [--timeout SECS]`
- IPC: `claude.spawn`, `claude.children`, `claude.parent`, `claude.kill`, `claude.respawn`, `claude.broadcast`, `claude.wait` 메서드

### Claude Hook 통합

Claude Code의 훅 시스템과 연동하여 Claude의 활동 상태를 추적하고, 상태 변화 시 등록된 훅을 실행하는 시스템.

- **상태 추적**: surface별로 idle/needs_input 상태를 HashMap으로 관리
- **claude.set_idle_state**: surface의 idle 상태 설정. idle=false 시 needs_input 상태도 자동 해제
- **claude.set_needs_input**: surface의 needs_input 상태 설정
- **claude_state_of()**: surface의 현재 상태를 "needs_input", "idle", "active" 중 하나로 반환
- **claude.children 상태 반영**: 자식 목록 조회 시 각 자식의 실제 Claude 상태가 state 필드에 반영됨
- **surface.fire_hook**: 특정 이벤트의 등록된 훅을 수동으로 실행 (hook_manager.check_and_fire 호출)
- **HookEvent 확장**: ClaudeIdle, NeedsInput 이벤트 타입 추가 ("claude-idle", "needs-input"으로 등록)
- **자동 정리**: surface가 닫힐 때 (unregister_child, mark_parent_closed) idle/needs_input 상태 자동 제거
- CLI: `tasty claude-hook stop|notification|prompt-submit|session-start [--surface ID]`
- IPC: `claude.set_idle_state`, `claude.set_needs_input`, `surface.fire_hook` 메서드

### Surface Metadata Store (surface_meta.rs)

파일 기반 키-값 스토어로, 어떤 프로세스(Claude Code 포함)든 서피스별 임의 메타데이터를 읽고 쓸 수 있다.

- **저장 위치**: Windows — `%TEMP%\tasty-surfaces\<surface_id>\meta.json`
- **SurfaceMetaStore**: 정적 메서드만 가지는 유틸리티 구조체 (상태 없음)
- **ensure_created(surface_id)**: 서피스 생성 시 메타 디렉토리와 빈 JSON 파일 생성. `send_fast_init()` 내부에서 자동 호출됨
- **remove(surface_id)**: 서피스 닫힐 때 메타 디렉토리 전체 삭제. 탭/패인/서피스 닫기 시 자동 호출됨
- **set/get/unset/list**: 파일을 읽어 HashMap으로 역직렬화, 수정 후 pretty JSON으로 재직렬화
- **범용 키-값**: 역할(role), 닉네임, 상태 등 에이전트가 필요한 임의 데이터를 저장 가능
- CLI: `tasty surface-meta set|get|unset|list [--surface ID] [--key KEY] [--value VALUE]`
- IPC: `surface.meta_set`, `surface.meta_get`, `surface.meta_unset`, `surface.meta_list` 메서드

## Crash Report & 진단

### Panic Hook (Release + Debug)
- `std::panic::set_hook`으로 커스텀 panic handler 등록
- panic 발생 시 `~/.tasty/crash-reports/crash-YYYY-MM-DDTHH-MM-SS.log` 파일에 자동 저장
- 리포트 내용: 타임스탬프, 버전, OS/아키텍처, panic 메시지 및 위치, 전체 스택트레이스
- stderr에도 동일 내용 출력 (fallback)
- 정상 동작 중 성능 영향 없음

### Debug 전용: 상세 파일 로깅
- debug 빌드에서 `~/.tasty/debug.log`에 모든 tracing 이벤트 기록
- 로그 레벨: `debug` (wgpu 관련은 `warn`)
- 매 실행 시 파일을 초기화하여 무한 증가 방지
- `#[cfg(debug_assertions)]`으로 release 빌드에서 완전히 제거

### Debug 전용: 에러 루프 감지 (ErrorLoopDetector)
- 동일 에러가 1초 내 100회 이상 반복되면 panic을 발생시켜 crash report로 기록
- 무한루프/데드락 상황에서 자동으로 진단 정보 수집
- `#[cfg(debug_assertions)]`으로 release 빌드에서 완전히 제거

## 단위 테스트

각 모듈에 `#[cfg(test)] mod tests` 블록으로 인라인 단위 테스트를 포함한다.

### tasty-terminal 테스트
- DECSET/DECRST 모드 토글: 애플리케이션 커서 키(모드 1), 커서 가시성(모드 25), 브래킷 붙여넣기(모드 2004), 마우스 트래킹(모드 1000/1003)
- 대체 화면 전환: 모드 1049 진입/퇴장, 모드 47 진입/퇴장, 대체 화면 리사이즈
- 방향키 모드 전환: 일반/애플리케이션 커서 키 모드 확인
- 전체 리셋(RIS): 모든 모드가 기본값으로 복원

### model.rs 테스트
- `Rect::contains`: 내부/외부/경계 포인트 판정
- `Rect::split`: 수직/수평/불균등 비율 분할
- `Rect::approx_eq`: 근사 비교 (1px 허용)
- `PaneNode::compute_rects`: 단일 및 분할 레이아웃
- `PaneNode::find_pane`: ID 기반 탐색
- `PaneNode::all_pane_ids`: 순서 보장 ID 수집
- `PaneNode::next_pane_id` / `prev_pane_id`: 순환 포커스 이동
- `AppState::move_focus_forward` / `move_focus_backward`: SurfaceGroup 내 Surface 우선 이동, 단일이면 Pane 간 이동
- `PaneNode::find_divider_at`: 분할 경계선 히트 테스트
- `PaneNode::split_pane_in_place`: 트리 내부 분할 (성공/실패 케이스)
- `PaneNode::close_pane`: 단일 리프 닫기 실패, 분할에서 형제 승격, 중첩 분할에서 닫기, 미발견 대상
- `Pane::close_tab`: 탭 닫기 성공, 마지막 탭 닫기 실패

### notification.rs 테스트
- 알림 추가 및 개수 확인
- 개별 및 전체 읽음 처리
- 워크스페이스별 필터 카운트
- 동일 소스 병합(coalescing)
- 다른 소스 비병합
- FIFO 최대 100개 제한

### tasty-hooks 테스트
- `HookEvent::parse` 전체 이벤트 타입
- 디스플레이 문자열 라운드트립
- 이벤트 매칭 (같은 타입, 다른 타입, 정규식)
- HookManager: 등록, 삭제, 조회
- once 훅 실행 후 자동 삭제
- persistent 훅 실행 후 유지

### settings.rs 테스트
- 기본 설정 유효성
- TOML 직렬화/역직렬화 라운드트립
- 부분 TOML 기본값 폴백
- 빈 TOML 전체 기본값

### model.rs Visitor 패턴 테스트
- for_each_terminal: 단일 Pane 순회, 분할된 Pane 순회
- for_each_terminal_mut: mutable 접근 및 수정
- compute_terminal_rect: 기본 계산, 스케일 팩터, 사이드바 클램핑, 사이드바 없음

### ipc/protocol.rs 테스트
- 요청 직렬화/역직렬화
- 성공/에러 응답 생성
- method_not_found 응답
- 응답 라운드트립

## 국제화 (i18n)

### 번역 시스템
- TOML 기반 번역 파일: 중첩 테이블을 점(dot) 구분 평면 키로 변환
- 내장 언어: 영어(en), 한국어(ko), 일본어(ja)
- `include_str!`로 바이너리에 번역 파일 임베드
- 영어를 기본 베이스로 로드 후, 선택된 언어를 오버레이하는 계층 구조
- 사용자 커스텀 번역: `~/.tasty/lang/{code}.toml` 파일로 개별 키 오버라이드 가능
- `OnceLock` 기반 글로벌 번역 스토어, 앱 시작 시 1회 초기화
- `t(key)`: 키로 번역 문자열 조회 (미등록 키는 키 자체를 반환)
- `t_fmt(key, arg)`: `{}` 플레이스홀더를 인자로 치환
- `current_language()`: 현재 언어 코드 조회
- 설정 파일(`config.toml`)의 `general.language` 필드로 언어 지정
- 언어 변경 시 재시작 필요

### 번역 키 구조
- `app.*`: 앱 이름
- `button.*`: 버튼 레이블 (취소, 저장, 새 워크스페이스 등)
- `tooltip.*`: 툴팁 텍스트
- `badge.*`: 배지 텍스트
- `settings.*`: 설정 UI (탭, 일반, 외관, 클립보드, 알림, 언어)
- `sidebar.*`: 사이드바 헤딩
- `shortcut.key.*` / `shortcut.desc.*`: 단축키 키/설명
- `notification_panel.*`: 알림 패널
