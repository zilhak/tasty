# Tasty - 구현된 기능

## 터미널 엔진

### PTY 기반 셸 실행
- ConPTY(Windows) / Unix PTY를 통한 네이티브 셸 실행
- `TERM=xterm-256color` 환경 설정
- PTY 리사이즈 전파: 윈도우 크기 변경 시 자식 프로세스에 새 크기 통보. rows 축소 시 커서 아래 빈 행을 먼저 제거하고 부족하면 위쪽 행을 scrollback으로 캡처하여 커서-콘텐츠 관계를 보존. rows 확대 시 scrollback에서 복원. 모든 워크스페이스/탭의 터미널에 리사이즈 전파
- 자식 프로세스 핸들 관리: 생존 여부 확인 가능
- PTY 채널 백프레셔: `sync_channel(32)`으로 버퍼 크기 제한 (32 * 8KB = 256KB), 버퍼 가득 차면 PTY 리더 스레드 블로킹
- 작업 디렉토리 상속: 새 surface/pane/workspace 생성 시 소스 surface의 현재 작업 디렉토리를 자동 상속. 셸이 내보내는 OSC 7 시퀀스로 CWD를 캐싱 (모든 플랫폼 공통). zsh/fish는 기본 지원, bash는 `PROMPT_COMMAND` 설정 필요. VTE 파서가 `/C:/path` URI 형식을 Windows 경로로 정규화. 설정에서 on/off 가능 (`general.inherit_cwd`, 기본 on). CLI/IPC에서는 `--cwd` 옵션으로 명시적 경로 지정도 가능

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
    - 동기화 출력 (모드 2026): 모드 상태를 추적. 변경사항은 항상 즉시 적용 (process→render 순차 파이프라인이므로 렌더러가 항상 최종 상태만 표시)
  - **디바이스 응답**: Device Status Report (DSR → `\x1b[0n`), Primary Device Attributes (DA → `\x1b[?1;2c`), Cursor Position Report (CPR → `\x1b[row;colR`)
  - **스크롤 리전 (DECSTBM)**: `CSI Pt;Pb r`로 스크롤 영역 설정. InsertLine/DeleteLine/LineFeed/Index/ReverseIndex가 스크롤 리전 내에서 동작

### 키보드 입력
- **중앙 키보드 디스패처**: 키보드 이벤트는 focused surface 타입에 따라 정확히 하나의 대상에만 전달됨
  - Terminal: PTY에 바이트 전송 (기존 경로)
  - Explorer/Markdown: `PendingKeyEvent` 큐에 저장 → 다음 egui 렌더 프레임에서 해당 패널이 소비
  - overlay(설정/다이얼로그/팝업) 열림 시에만 egui에 키보드 이벤트 전달 — 비활성 패널의 `ui.input()` 전역 오염 방지
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
- 텍스트 wrap에 의한 implicit 스크롤(termwiz Surface가 `ScrollRegionUp` Change 없이 내부 처리하는 케이스)도 변경 전후 화면 스냅샷 비교로 감지하여 사라진 행을 scrollback에 기록 — 선택 영역의 `absolute_row`가 콘텐츠를 정확히 따라가도록 보장

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
- **번들 D2Coding ligature 폰트** (NAVER, OFL 1.1): Regular/Bold ttf를 바이너리에 임베드해 사용자의 OS에 D2Coding이 설치되지 않아도 동작. `font_family`가 빈 문자열이거나 `"monospace"`일 때 자동 적용되며, 다른 폰트를 지정해도 D2Coding은 폰트 DB에 남아 fallback face로 동작. `Shaping::Advanced`로 합자(`==`, `!=`, `=>`, `->`, `<=`, `>=` 등) 자동 적용. 한자/가나는 시스템 CJK 폰트로 자동 fallback
- 2048x2048 R8 텍스처 아틀라스에 선반(shelf) 기반 글리프 패킹
- 아틀라스 가득 찰 때 자동 리셋 및 재구축 (캐시 초기화 + 텍스처 클리어)
- 베이스라인 기반 글리프 오프셋 계산
- Bold/Italic 변형 지원 (Bold는 D2Coding ligature Bold face가 직접 매칭, synthetic bold 회피)
- Mask/Color/SubpixelMask 콘텐츠 타입별 그레이스케일 변환 (`chunks_exact` 사용)
- 블록 요소(U+2580–U+259F) 및 박스 드로잉(U+2500–U+257F) 커스텀 렌더링: 셀 경계를 정확히 채우는 픽셀 퍼펙트 비트맵을 프로그래밍 방식으로 생성. 상/하/좌/우 분할 블록, 쉐이드(25%/50%/75%), 사분면, light/heavy/double 선, 코너, T-접합, 크로스, 대각선 지원. Bold/Italic이 아닌 일반 스타일에서만 적용되며, 나머지는 swash 래스터라이저로 폴백

### 색상 지원
- xterm-256color 팔레트: ANSI 16색, 216색 큐브, 24단계 그레이스케일
- TrueColor (24-bit RGB) 지원
- SGR을 통한 전경색/배경색 개별 설정
- SGR 2 (dim, `Intensity::Half`): 전경색을 배경과 50:50 블렌딩하여 흐리게 렌더링 (reverse swap 후 적용). 디스크 스크롤백에서도 보존된다.

### 윈도우 관리
- winit 기반 크로스 플랫폼 윈도우 생성
- 리사이즈 시 뷰포트 유니폼 자동 갱신 및 터미널 그리드 재조정
- DPI 변경 감지: `ScaleFactorChanged` 이벤트 처리로 모니터 간 이동 시 스케일 팩터 자동 갱신
- 모노스페이스 폰트 기반 셀 그리드 레이아웃 (기본 14pt)
- **Windows 릴리스 빌드 서브시스템**: 릴리스 빌드는 `#![windows_subsystem = "windows"]`로 GUI 서브시스템으로 빌드되어 `tasty.exe` 실행 시 빈 콘솔 창이 뜨지 않음. 진입 직후 `AttachConsole(ATTACH_PARENT_PROCESS)`로 부모(ConPTY 포함) 콘솔에 attach하여 내부 pane에서의 CLI 호출 출력은 정상 표시. 디버그 빌드는 기존처럼 콘솔 창을 유지해 `tracing` 로그를 바로 확인 가능

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
- PaneNode: Pane의 상위 레이아웃 트리. Leaf(Pane) 또는 Split. 탭 전환과 무관하게 고정
- Pane: **독립적인 탭 바**를 가진 화면 영역. 여러 Tab을 포함
- Tab: 탭 하나. `SurfaceLayout`을 직접 소유. 단일 Leaf = 분할 안 된 상태, Split = 탭 내부 분할
- Surface trait: 모든 콘텐츠 타입의 공통 인터페이스. 각 타입이 독립 struct로 구현. **`tasty-core`는 GUI-free** — 모델은 식별 정보와 직렬화 가능한 상태만 보유한다 (egui는 optional `egui-compat` feature, 헤드리스 플러그인은 비활성 가능)
  - `kind()`: 소문자 식별자 — 호스트 빌트인 6종(`"terminal"`, `"markdown"`, `"html"`, `"image"`, `"empty"`, `"clipboard_viewer"`) + plugin 등록 kind(예: `"explorer"`). IPC/registry/플러그인이 식별자로 사용
  - `type_name()`: 표시용 라벨. 식별 비교 금지
  - `html_url()`: HtmlPanel만 `Some(&url)` 반환. native WebView 동기화가 다운캐스트 없이 사용
  - TerminalSurface: 단일 PTY 터미널
  - MarkdownPanel, HtmlPanel, ImagePanel, EmptySurface, ClipboardViewerPanel: 호스트 빌트인 비터미널 콘텐츠
  - RemoteSurface: plugin이 IPC로 제공하는 surface (예: 기본 제공 Explorer plugin)
- **Model + Host View 분리**: 휘발성 GUI 상태(콘텐츠 캐시, 텍스처, 편집 세션, 스크롤, 팝업 버퍼)는 호스트 측 View로 분리되어 `AppState::markdown_views` / `image_views` (HashMap<SurfaceId, View>) 에 보관. surface 닫힘 시 `cleanup_surface(sid)`가 모든 store에서 `drop_view(sid)` 호출
- AppState: 전체 워크스페이스 목록과 활성 상태를 관리하는 중앙 상태 (IdGenerator 포함)

### egui UI 오버레이
- egui-winit + egui-wgpu를 이용한 wgpu 위 egui 렌더링
- 좌측 SidePanel: 워크스페이스 목록, 활성 표시, 추가 버튼
- Pane별 탭 바: 각 Pane의 rect 상단에 egui Area로 렌더링
- 탭 UI: 150px 너비 영역 스타일, 1px 세로 구분선(surface1), active 탭 상단 2px 강조선(blue)
- 탭 스크롤: 탭이 영역을 초과하면 좌우 화살표 버튼(< >)으로 스크롤 가능
- 탭 이름 정책:
  - 기본: 포커스된 surface의 현재 작업 디렉토리의 폴더 이름 (시스템/유저 루트는 `/` 또는 `~`)
  - 명시적 설정 시: 설정된 이름 (explicit_name) 우선
- 이름 변경 다이얼로그(탭 rename, 워크스페이스 이름/부제목 rename, 북마크 이름 입력)는 열리는 즉시 기존 텍스트가 전체 선택되어, 곧바로 입력하면 새 값으로 대체된다
- 다크 테마 적용 (패널 배경색 커스터마이징)
- 사이드바에 키보드 단축키 안내 표시

### 두 가지 분할 유형
- **Pane 분할** (Alt+E / Alt+Shift+E, macOS: ⌘E / ⌘⇧E): 물리적 화면 분할. PaneNode 이진 트리 기반. 각 영역이 독립 탭 바를 가진다
- **Surface 분할** (Alt+D / Alt+Shift+D, macOS: ⌘D / ⌘⇧D): 탭 내부 분할. Tab이 직접 소유하는 SurfaceLayout 이진 트리 기반. 탭 바에서는 하나의 탭으로 표시된다
- 단일 Leaf 탭이 분할 시 자동으로 Split 구조로 변환
- **패닉 없는 분할 구현**: PTY/Terminal을 구조적 변경 이전에 선행 생성 — 리소스 생성 실패 시 레이아웃이 변경되지 않음
- `PaneNode::split_pane_in_place`: `std::mem::replace` 2-step 패턴으로 소유권 이동 없이 트리 내부 노드를 in-place 변경
- `SurfaceLayout::split_with_surface`: 소유권 기반 infallible 분할 — 사전 생성된 `Box<dyn Surface>`를 받아 모든 surface 타입(Terminal, Markdown, Html, RemoteSurface 등) 지원. `split_with_node`는 TerminalSurface 전용 편의 래퍼
- Workspace/Tab 내부 Option 래핑 + take/put 패턴: split 함수가 infallible이므로 take 이후 put이 항상 실행됨 보장
- 각 Surface를 scissor rect로 독립 렌더링
- 뷰포트별 유니폼 갱신 (grid_offset을 각 Surface rect에 맞게 조정)

### 새 surface 생성 시 cwd 상속

단축키나 IPC/CLI(cwd 미지정)로 새 surface를 만들 때, 분할 대상 또는 포커스된 source surface의 `Surface::source_cwd()` 값을 새 터미널의 시작 디렉터리로 사용한다 (`docs/design/split-command.md` 참조).

- Terminal: OSC 7로 알린 cwd
- Explorer: `root_path` (주소바 편집 텍스트는 무시)
- Markdown: 열려 있는 파일의 부모 디렉터리
- HTML(`file://` 또는 로컬 절대경로): URL이 가리키는 파일의 부모 디렉터리. 그 외(http/https/about/data 등)는 None
- Image / Empty / ClipboardViewer: None

`general.inherit_cwd` 설정(default true)을 false로 바꾸면 모든 단축키 경로에서 fallback이 비활성화되어 셸의 home에서 시작한다. IPC/CLI에서 명시적으로 전달한 `cwd` 인자는 이 설정과 무관하게 그대로 사용된다.

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

#### 단축키 프리셋

4개의 프리셋을 제공한다. 설정 UI의 Preset 서브탭에서 전환 가능.

| 프리셋 | 참고 앱 | 특징 |
|--------|---------|------|
| **Tasty** (기본) | 자체 설계 | 모든 플랫폼의 복사/붙여넣기/줌 바인딩을 통합 |
| **Mac** | iTerm2 / Terminal.app | `alt+` (= ⌘) 중심 |
| **Windows** | Windows Terminal | `ctrl+shift+` 중심, `ctrl+c/v`로 복사/붙여넣기 |
| **Linux** | GNOME Terminal | `ctrl+shift+` 중심, `ctrl+shift+c/v`로 복사/붙여넣기 |

#### 기본 단축키 (Tasty 프리셋)

| 단축키 | 동작 |
|--------|------|
| Alt+N | 새 워크스페이스 |
| Alt+T | 포커스된 Pane에 새 탭 |
| Alt+E | Pane 수직 분할 |
| Alt+Shift+E | Pane 수평 분할 |
| Alt+D | Surface 수직 분할 (탭 내부) |
| Alt+Shift+D | Surface 수평 분할 (탭 내부) |
| Alt+] / Alt+[ | Surface 포커스 다음/이전 |
| Ctrl+] / Ctrl+[ | Pane 포커스 다음/이전 |
| Alt+1~9 | 워크스페이스 전환 |
| Ctrl+1~0 | 탭 전환 |
| Alt+Shift+N | 새 윈도우 |
| Ctrl+Shift+B | 사이드바 토글 (숨김/표시) |
| Ctrl+B | 사이드바 접기/펼치기 |
| Ctrl+Shift+H | 클립보드 히스토리 뷰어 토글 |
| Ctrl+, | 설정 열기 |
| Ctrl+C / Alt+C / Ctrl+Shift+C | 복사 |
| Ctrl+V / Alt+V / Ctrl+Shift+V | 붙여넣기 |
| Alt+Shift+C | 경로 복사 (Explorer) |
| Ctrl+X / Alt+X | 잘라내기 (Explorer) |
| Ctrl+A / Alt+A | 전체 선택 (Explorer) |
| Ctrl+= / Alt+= | 확대 |
| Ctrl+- / Alt+- | 축소 |
| Ctrl+0 / Alt+0 | 줌 리셋 |
| Alt+' | Surface 타입 전환 |
| Ctrl+` | 포커스된 탭 이름 변경 다이얼로그 |
| Alt+` | 활성 워크스페이스 이름 변경 다이얼로그 |
| Alt+Shift+` | 활성 워크스페이스 부제목 변경 다이얼로그 |

#### Surface별 단축키 동작

동일한 단축키가 포커스된 Surface 타입에 따라 다르게 동작한다.

| 단축키 | Terminal | Explorer | Markdown |
|--------|----------|----------|----------|
| copy | 선택 텍스트를 클립보드에 복사 | 선택된 파일을 OS 파일 클립보드에 복사 | egui 텍스트 선택 복사 |
| paste | 클립보드 텍스트를 터미널에 입력 | OS 파일 클립보드에서 파일 붙여넣기 | - |
| copy_path | - | 선택된 파일 경로를 텍스트로 클립보드에 복사 | - |
| cut | - | 선택된 파일을 잘라내기 | - |
| select_all | - | 모든 파일 선택 | - |

### 방향성 포커스 이동 (키보드 단축키)
- 키보드 단축키를 통한 분할 트리 구조 기반 방향성 포커스 이동
- 알고리즘: SplitDirection 트리를 역방향으로 탐색하여 이동 방향에 맞는 분할을 찾고, 시블링 서브트리의 엣지 리프로 포커스 이동
  - `SplitDirection::Vertical`(좌우 경계) → Left/Right 방향에 대응
  - `SplitDirection::Horizontal`(상하 경계) → Up/Down 방향에 대응
  - Left/Up: 시블링의 rightmost/bottommost 리프로 이동 (인접한 엣지)
  - Right/Down: 시블링의 leftmost/topmost 리프로 이동 (인접한 엣지)
- 탭 내부 서피스 간 이동 우선, 이동 불가 시 Pane 간 이동
- close_surface 단축키: 포커스된 서피스 닫기. cascade: surface → pane → workspace. 마지막 workspace면 닫고 새로 생성
- close_active 단축키 (기본 Ctrl+W): 활성 항목 닫기. cascade: tab → pane → workspace. 마지막 workspace면 닫고 새로 생성. 설정 가능
- Ctrl+Shift+W: 포커스된 패인 닫기. cascade: pane → workspace. 마지막 workspace면 닫고 새로 생성
- Alt+Shift+W: 활성 워크스페이스 닫기. 마지막 workspace면 닫고 새로 생성
- Ctrl+Shift+I: 알림 패널 토글
- Ctrl+,: 설정 모달 윈도우 열기 (독립 OS 윈도우, 모달 활성 시 다른 윈도우 입력 차단)
- Ctrl+D: 터미널에 전달 (EOF). 이전에는 Surface 수직 분할이었으나, Ctrl+Shift+D로 변경
- 호스트(`src/window/main`)에서 winit `ModifiersState`로 수정자 키를 추적. `tasty-settings`는 winit에 의존하지 않으며, `LinkModifier::matches`는 `(ctrl, alt, super)` 원시 bool을 받는다

### 마우스 인터랙션
- **클릭으로 Pane 포커스**: 터미널 영역 좌클릭 시 해당 Pane이 포커스됨. `cursor_position` 추적 + `focus_pane_at_position()`으로 어떤 Pane인지 판별
- **클릭으로 Surface 포커스**: 탭 내부 분할에서 특정 터미널을 클릭하면 해당 Surface가 포커스됨. `focus_surface_at_position()`으로 클릭 좌표에서 Surface ID를 찾아 전환
- **디바이더 드래그로 분할 비율 조절**: Pane 또는 탭 내부 분할 경계선을 마우스 드래그하여 비율 조정 (0.1~0.9 범위 클램프). `DividerDrag` 상태 머신으로 드래그 시작/이동/종료를 추적. 드래그 중 실시간 리사이즈 적용
- **디바이더 호버 시 커서 변경**: 분할 경계선에 4px 이내로 마우스를 가져가면 커서가 리사이즈 아이콘으로 변경 (수직 분할: ColResize, 수평 분할: RowResize). 벗어나면 Default로 복귀
- **마우스 스크롤**: 일반 모드에서 마우스 휠은 스크롤백 버퍼를 탐색함. 대체 화면(vim, less 등)에서는 방향키 시퀀스(`\x1b[A`/`\x1b[B`)를 PTY에 전달. LineDelta와 PixelDelta 모두 지원. 포커스와 무관하게 마우스 커서 아래의 surface가 스크롤 대상이 됨 (커서가 터미널 영역 밖이면 포커스된 surface로 폴백)
- **egui와의 이벤트 충돌 방지**: egui가 이벤트를 소비한 경우 (사이드바, 설정 윈도우 등) 터미널에는 전달하지 않음
- **터미널 내 링크 hover·클릭 오픈**: 터미널 출력에 포함된 URL(`http://`, `https://`, `ftp://`, `file://`), OSC 8 hyperlink, 그리고 **스키마 없는 경로**(Unix 절대 `/foo/bar`, Windows 절대 `C:\foo`/`C:/foo`, 상대 `./foo`·`../foo`)를 감지. 경로는 터미널 OSC 7 기반 CWD를 기준으로 실제 존재할 때만 링크로 판정되어 오탐을 줄임. 설정된 수식키(기본 `Ctrl`, 설정에서 `Alt`/`없음` 선택 가능)를 누른 채 마우스를 올리면 해당 링크가 blue로 하이라이트되고 커서가 PointingHand로 변경됨. 수식키+좌클릭 시 `webbrowser` crate로 기본 브라우저/연결 프로그램을 열어 URI를 처리. 수식키+클릭은 링크 위가 아니면 아무 동작도 하지 않으며 selection과 충돌하지 않음. 사용자의 키보드/마우스 동작이므로 CLI/IPC로 노출되지 않음 (`docs/design/ubiquitous-language.md`의 사용자/에이전트 분리 원칙)
- **탭 드래그 재정렬**: 탭 바에서 탭을 마우스 왼쪽 버튼으로 드래그하여 순서 변경. 드래그 중 반투명 고스트 탭 + 파란 삽입 마커 표시. 드롭 시 `Pane::move_tab()`으로 이동
- **워크스페이스 드래그 재정렬**: 사이드바에서 워크스페이스 카드를 드래그하여 순서 변경. 드래그 중 반투명 고스트 카드 + 가로 파란 삽입 마커 표시. 드롭 시 `AppState::move_workspace()`로 이동
- **탭/워크스페이스 우클릭 이동**: 탭 우클릭 → Move Left / Move Right, 워크스페이스 우클릭 → Move Up / Move Down. 끝에 있으면 비활성화
- 관련 모델 메서드: `Rect::contains()`, `PaneNode::find_divider_at()`, `PaneNode::update_ratio_for_rect()`, `SurfaceLayout::find_divider_at()`, `SurfaceLayout::update_ratio_for_rect()`, `SurfaceLayout::find_surface_at()`

### 추가 Surface 타입 (Markdown / HTML / Empty + plugin 기반 Explorer)
- 모든 Surface 타입은 고유 surface_id를 가지며, 닫기/포커스/리스트 등 공통 surface 동작이 동일하게 적용됨
- Markdown/Empty: 호스트가 egui로 렌더링
- Explorer: **com.tasty.explorer 기본 제공 plugin**이 RemoteSurface로 제공 — UiTree 트리를 IPC로 호스트에 전송, 호스트는 egui로 그대로 렌더링
- HTML: OS 네이티브 WebView (macOS: WKWebView, Windows: WebView2, Linux: WebKitGTK)를 wgpu 윈도우 위에 child view로 오버레이
- Empty: 빈 placeholder. 중앙에 타입 전환 버튼 표시

#### Markdown Viewer
- 마크다운 파일을 egui로 렌더링하는 읽기 전용 뷰어
- **자동 리로드**: 파일 변경 시 1초 간격으로 mtime 체크하여 자동 갱신 (live reload)
- 지원 문법: 제목(#, ##, ###), 목록(-, *), 인용(>), 수평선(---), 코드 블록(```), 테이블(|), 인라인 서식(**볼드**, *이탤릭*, \`코드\`)
- 파일 경로를 지정하여 열기 (IPC/CLI/우클릭 메뉴)
- 탭으로 열리며 파일명이 탭 이름이 됨
- surface 전체 너비를 채우며, 텍스트는 surface 너비에 맞춰 자동 줄바꿈

#### Explorer (기본 제공 plugin)
- `com.tasty.explorer` 외부 plugin 바이너리가 디렉토리 트리와 파일 미리보기를 제공한다. 호스트는 plugin이 보낸 UiTree를 egui로 렌더링할 뿐, 모델/IO를 직접 다루지 않는다. plugin 메뉴에서 비활성/제거 가능 (제거 시 `removed_builtins`에 기록되어 재실행 시 다시 설치되지 않음)
- 왼쪽 트리 + 오른쪽 뷰어의 2-컬럼 레이아웃, **divider 드래그로 비율 조절** 가능 (0.15~0.85)
- .md 파일 선택 시 마크다운 렌더링, 기타 파일은 모노스페이스 텍스트 표시
- 숨김 파일 기본 제외 (.env, .gitignore, .claude는 표시)
- 디렉토리 우선, 대소문자 무시 이름순 정렬
- **파일/디렉토리 선택**: 클릭으로 선택 (디렉토리도 선택 가능), 더블클릭 또는 Enter로 디렉토리 확장/축소
- **다중 선택**: Ctrl/Cmd+클릭으로 개별 토글, Shift+클릭으로 범위 선택, Ctrl/Cmd+A로 전체 선택
- **키보드 내비게이션**: ArrowUp/Down으로 이동, Shift+Arrow로 범위 확장, Enter로 디렉토리 토글
- **화살표 아이콘**: 디렉토리 트리의 ▶/▼ 아이콘을 별도 클릭 영역으로 분리하여 선택 없이 토글 가능
- **파일 클립보드**: 선택된 파일/디렉토리를 Ctrl/Cmd+C/X로 OS 클립보드에 복사/잘라내기, Ctrl/Cmd+V로 붙여넣기. OS 파일 탐색기(Finder, Windows Explorer, Nautilus)와 호환
  - macOS: NSPasteboard writeObjects로 다중 파일 NSURL 전달, pasteboardItems로 다중 파일 읽기
  - Windows: CF_HDROP (TODO)
  - Linux: `x-special/gnome-copied-files`(GNOME/Thunar/Dolphin/Nemo) + `text/uri-list` 폴백. Wayland는 `wl-copy`/`wl-paste`, X11은 `xclip` 사용 (런타임 의존성 — `wl-clipboard` 또는 `xclip` 패키지 필요)
- **붙여넣기 대상**: 선택된 디렉토리가 1개면 해당 디렉토리, 아니면 포커스 파일의 부모 디렉토리, 없으면 루트 디렉토리
- **주소표시줄**: 상단에 현재 루트 경로를 표시하는 텍스트 입력 필드. 직접 경로를 입력하고 Enter로 해당 경로로 이동 가능
- **우클릭 컨텍스트 메뉴**: 파일/폴더/배경에서 우클릭 시 상태에 따른 컨텍스트 메뉴 표시
  - **경로 복사**: 대상 경로를 OS 클립보드에 복사. 다중 선택 시 개행 구분. 배경 우클릭 시 cwd 경로
  - **즐겨찾기 추가/삭제**: 단일 폴더 선택 또는 배경 우클릭에서만 표시. 다중 선택 시 숨김
  - **복사**: 선택된 파일/폴더를 OS 파일 클립보드에 복사
  - **삭제**: OS 휴지통으로 이동 (`trash` 크레이트 사용)
  - 우클릭 대상이 현재 선택 목록에 포함되면 선택 전체가 메뉴 대상, 포함되지 않으면 선택 초기화 후 클릭 항목만 대상 (VS Code 방식)
  - 상세 동작 분기: `docs/design/explorer-context-menu.md` 참조
- **즐겨찾기**: 좌측 하단 영역에 즐겨찾기 목록 표시
  - 추가 시 이름 입력 팝업 표시. 비워두면 폴더명 사용
  - 즐겨찾기를 더블클릭하면 해당 경로로 탐색기 루트 이동
  - `~/.tasty/state.db` (SQLite)의 `bookmarks` 테이블에 persist. 모든 Explorer 패널이 동일한 즐겨찾기를 공유. 구 `bookmarks.json`은 첫 실행 시 자동 이관되고 `bookmarks.json.bak`으로 이름이 바뀜

#### 컨텍스트 메뉴
- 터미널 영역 또는 탭 바 빈 공간에서 마우스 우클릭 시 컨텍스트 메뉴 표시
- "Open Markdown..." → 파일 경로 입력 다이얼로그 → 마크다운 탭 열기
- "Open HTML..." → URL 입력 다이얼로그 → HTML WebView 탭 열기
- "새 이미지" → 빈 이미지 surface 탭 생성 (기본 800×600 흰 캔버스가 즉시 그려진 상태로 시작, 다른 크기를 원하면 surface 안의 `+` 버튼으로 팝업 호출)
- 좌클릭 또는 Cancel로 메뉴 닫기

#### 키보드 단축키
- `open_markdown`: 마크다운 열기 (파일 경로 입력 다이얼로그 표시)
- 기본값 미설정 (설정 UI에서 Pane 서브탭에서 바인딩 가능)

#### Surface 타입 전환
- `convert_surface` 단축키 (기본 `Alt+'`): Surface 스코프 팝업으로 전환 메뉴 표시 — Terminal(T) / Markdown(M)... / HTML(H)... / Image(I). 팝업은 해당 surface 영역 중앙에 배치되며, 항목 수에 맞게 크기 자동 계산. (Explorer 등 plugin 제공 surface는 plugin 자신이 변환을 다룬다)
- `convert_to_markdown`: 직접 전환 단축키 (기본값 없음, 설정에서 할당)
- 현재 타입과 동일한 항목은 체크 표시 + 비활성
- Markdown 전환 시 파일 경로 입력 다이얼로그 표시
- Terminal 전환 시 새 PTY 생성
- Esc / 외부 클릭 / X 버튼으로 팝업 닫기
- 키보드 탐색: Up/Down 방향키로 항목 이동, Enter로 선택 확정
- 단축키: T/M/H/I 키로 즉시 선택
- 팝업이 열려 있으면 키보드 입력이 터미널로 전달되지 않음 (PopupManager 포커스 자동 관리)
- **개별 surface 교체 원칙**: 타입 전환은 대상 surface의 구현체만 교체한다. 기존 구현체는 메모리에서 해제되고 새 구현체로 대체된다. 탭 레이아웃, 다른 surface 등 주변 구조에는 어떤 영향도 주지 않는다.
  - 탭 내부 분할의 surface를 전환해도 다른 surface는 그대로 유지됨
  - 단독 surface(탭에 1개)를 전환하면 탭의 surface가 교체됨. Terminal로 전환 시 탭 이름이 자동(CWD 기반)으로 복원됨
  - 비터미널 surface(Markdown, Html, Empty, RemoteSurface)는 탭 내부 분할에서도 올바르게 렌더링됨 (egui 렌더링)

#### HTML WebView
- OS 네이티브 WebView를 wgpu 윈도우 위에 child view로 오버레이
- macOS: WKWebView (objc2-web-kit), Windows: WebView2 (webview2-com), Linux: WebKitGTK (webkit2gtk + x11-dl)
- wry 소스 참조 자체 구현 (~120줄/플랫폼, 6개 API: create, set_bounds, set_visible, load_url, load_html, drop)
- URL 또는 HTML 문자열 직접 로드 지원 (file:// 로컬 파일 로드 가능)
- 탭 전환 시 자동 show/hide, 리사이즈 시 자동 bounds 동기화
- 비활성 워크스페이스/탭의 WebView는 자동 hidden

#### IPC/CLI 지원
- IPC: `tab.create`에 `type` 파라미터로 통합 (`terminal` / `markdown` / `explorer` / `html`)
- CLI: `tasty new tab --pane <PANE> --type html --url <URL>`

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
- **Surface 하이라이트 추적**: 알림이 발생한 surface를 `highlighted_surfaces` HashSet으로 관리. 해당 surface에 포커스하면 자동으로 하이라이트 해제

### 시스템 알림 (notify-rust)
- 윈도우가 비활성 상태일 때 OS 네이티브 알림 전송
- 초당 1회 제한(rate limiting)으로 알림 폭주 방지
- Windows/macOS/Linux 크로스 플랫폼 지원

### Surface 알림 하이라이트
- 알림이 발생한 surface에 파란색 테두리 강조 표시
- 해당 surface에 포커스하면 하이라이트 자동 해제 (매 렌더 프레임에서 focused surface의 하이라이트 제거)

### 사이드바 알림 배지
- 하이라이트된 surface가 있는 워크스페이스에 `!` 배지 표시 (테두리 스타일)
- 확장 사이드바: 워크스페이스 이름 우측에 파란색 테두리 `!` 배지
- 축소 사이드바: 워크스페이스 번호 버튼에 파란색 테두리 강조
- 모든 하이라이트된 surface를 방문하면 배지 자동 소멸

### 도구 메뉴
- 사이드바 하단의 "도구" 버튼을 클릭하면 버튼 위쪽에 headless 팝업(타이틀바 없음)이 표시
- 팝업에는 사용 가능한 도구 목록이 메뉴 형태로 나열됨
- 현재 도구: 클립보드 히스토리
- 바깥 클릭 시 자동으로 닫힘 (`close_on_outside_click`)

### Busy Indicator (실행 중 표시)
- PTY foreground 프로세스를 1초 간격으로 폴링하여 surface별 busy 상태를 캐시(`busy_surfaces`)
- 판정: foreground가 shell 자신이거나 알려진 shell 이름이면 idle. 그 외에는 **최근 2초 안에 PTY 출력이 있었을 때만** busy. 즉 `claude`/`vim` 같은 TUI를 띄워둔 채 가만히 있으면 idle로 떨어지고, 토큰을 흘리거나 `cargo build`처럼 출력이 나오는 동안에만 busy로 표시됨 (tmux/iTerm2의 activity monitor와 동일한 시멘틱)
- 플랫폼별 메커니즘: Linux `/proc/<pid>/stat` tpgid, macOS `ps -o tpgid=`, Windows `CreateToolhelp32Snapshot` 기반 자손 트리 탐색
- 집계: 탭/워크스페이스는 포함된 surface 중 하나라도 busy면 busy (OR)
- 시각 표시:
  - 탭 라벨 우측에 녹색 점 (active 탭은 진한, inactive 탭은 dim 알파)
  - 워크스페이스 사이드바: 접힘 모드는 번호 버튼 우상단의 점, 펼침 모드는 카드 우측의 점 + 카운트
- IPC: `surface.list`에 `busy: bool`, `tab.list` / `workspace.list` / `tree`에 `busy_count: number`
- focus와 무관하게 동작 (focus-policy.md §6 참조). 상세: `docs/design/busy-indicator.md`

### 알림 패널 (Ctrl+I) — Popup (Window 스코프)
- Popup으로 분류: 터미널 입력을 차단하지 않으며, 포커스를 빼앗지 않음
- Window 스코프: 워크스페이스 전환과 무관하게 항상 보임
- egui Window 오버레이로 구현된 알림 목록
- 스크롤 가능한 최신순 정렬 알림 표시
- 각 알림에 워크스페이스 이름, 제목, 본문, 경과 시간 표시
- "Jump" 버튼으로 해당 워크스페이스로 즉시 전환
- 패널 열 때 자동으로 전체 읽음 처리
- "Mark all read" 버튼 제공

### Surface 영역 계산
- `AppState::surface_regions()`가 모든 surface(터미널, Explorer, Markdown 등)의 영역을 통합 계산
- `SurfaceRegion { id, rect, surface: &dyn Surface }` 구조체로 타입 구분 없이 일관된 접근 제공
- toast, popup, surface highlight 등이 모두 이 통합 API를 사용

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
- **General**: 셸 경로 (OS별 자동 감지: COMSPEC/SHELL), 시작 명령, 스크롤백 줄 수 (기본 10,000), 작업 디렉토리 상속 (기본 on), 셸 모드 (default / tasty / custom). tasty 모드는 `~/.tasty/bashrc`를 source하여 OSC 7 등의 빌트인 설정을 적용한다. 기존 설정 파일의 `"fast"`는 unknown 값으로 간주되어 default로 fallback. 레이아웃 저장/복원 (기본 off): 체크 시 워크스페이스/페인/탭/서피스 구조를 `~/.tasty/layout.json`에 저장하고 다음 시작 시 복원.
- **Appearance**: 폰트 패밀리 (기본값: 시스템 모노스페이스), 폰트 크기, 테마 (dark/light), 배경 투명도, 사이드바 너비, focused surface 배경색, Font DPI 스케일링 모드 (auto: 모니터 DPI에 맞춰 동일 물리 크기 유지 / fixed: 픽셀 고정, 기본값)
- **Clipboard**: OS별 기본 활성화 (macOS: Alt+C/V, Linux: Ctrl+Shift+C/V, Windows: Ctrl+C/V)
- **Notifications**: 알림 활성화, 시스템 알림, 사운드, 병합 간격(ms)
- **Keybindings**: 서브탭으로 분류된 단축키 설정 (General / Workspace / Pane / Tab / Surface / Clipboard / Zoom / Preset). 유비쿼터스 언어 계층 구조(Workspace → Pane → Tab → Surface) 순서. 각 서브탭 내부 항목은 생성/분할 → 탐색 → 수정 → 닫기 순서로 정렬
  - 중복 바인딩 방지: 녹화한 조합이 다른 액션에 이미 할당되어 있으면 확인 팝업 표시. Enter/Y/Overwrite 수락 시 기존 바인딩을 비우고 새 필드에 적용, Esc/N/Cancel 취소 시 값 변경 없음. 팝업이 열린 동안 녹화 버튼은 비활성화됨.
  - **Preset 서브탭**: 좌측에 프리셋 목록, 우측에 미리보기 패널 (3열 테이블 — 기능 / 이전 / 이후). 변경되는 행은 bold 강조. 하단 "적용" 버튼으로 Draft에 반영 (실제 저장은 하단 Save 버튼). Draft가 이미 프리셋과 동일하면 적용 버튼 비활성화.
- **Performance**: targeted PTY polling, scrollback disk swap, lazy PTY init (background 탭 생성 시 PTY를 즉시 spawn하지 않고 최초 접근 시점에 spawn)
- **Misc (기타)**: 좌측 서브탭 메뉴 + 우측 콘텐츠 구조 (Keybindings 탭과 동일한 레이아웃, 향후 서브탭 확장 대비).
  - **tastyrc 서브탭**: Tasty 모드 bashrc 편집기. 사용자 편집분은 `~/.tasty/bashrc.user`에 저장되고, 빌트인 블록(OSC 7 emission / UTF-8 / PATH)은 코드 상수로 유지되어 Save 시마다 `~/.tasty/bashrc`가 `builtin + user` 형태로 자동 재생성된다. 이로써 빌트인 템플릿이 업데이트되면 기존 사용자에게도 즉시 반영된다. Reset 버튼으로 user 파트를 초기 기본값으로 되돌릴 수 있다.

### GUI 설정 윈도우
- Ctrl+, 단축키로 설정 윈도우 토글
- egui Window 기반 탭 인터페이스 (General / Appearance / Clipboard / Notifications / Keybindings / Language / Performance / Misc)
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
- `settings.appearance.default_font`: 기본 폰트 5종 묶음 (`font_family`, `font_size`, `custom_font_path`, `line_height`, `font_scale_mode`). Terminal·Markdown·Explorer 모두에 일괄 적용되며, 각 surface는 아래 override 그룹으로 항목별 재정의 가능. 설정 UI에서는 Theme 서브탭 하단의 "기본 폰트 설정" 섹션에서 편집
- `settings.appearance.terminal_font` / `markdown_font` / `explorer_font`: surface별 per-field override. 5개 필드 모두 `Option<T>`이며 `None`이면 `default_font`를 사용. 각 surface 서브탭에 "기본값 사용" 체크박스 + 입력 위젯 패턴으로 노출
- `font_family`: cosmic-text(터미널) 또는 egui FontDefinitions(Markdown/Explorer)에 전달. 빈 문자열이나 "monospace"이면 번들 D2Coding ligature를 사용. 다른 폰트를 지정해도 D2Coding은 폰트 DB에 남아 fallback face로 동작. 설정 UI에서 시스템 폰트 목록(번들 `D2Coding ligature` 포함)을 검색 가능한 드롭다운으로 선택
- `font_size`: 픽셀 단위. 기본값 14.0. 단축키 `Ctrl+/-/0`은 포커스된 surface(Terminal/Markdown/Explorer)의 `font_size` override만 변경하며, `Ctrl+0`은 override를 제거해 기본값으로 회귀
- `custom_font_path`: 커스텀 폰트 파일(.ttf/.otf) 경로. 지정 시 FontSystem 또는 egui FontDefinitions에 해당 파일을 추가 로드한 후 `font_family`로 참조 가능
- `line_height`: 행간 배수. 1.0(기본, 틈 없음 - ASCII 아트에 최적) ~ 2.0. 값이 클수록 행 간격이 넓어짐
- `font_scale_mode`: "auto"는 `font_size * scale_factor`(고DPI에서 동일 물리 크기 유지), "fixed"는 픽셀 크기 고정
- 레거시 평면 형식(`appearance.font_family = ...` 등)은 자동으로 `default_font`로 마이그레이션되어 기존 설정 파일이 그대로 동작
- `settings.appearance.theme`: 테마 프리셋 ID. "catppuccin-mocha"(기본 다크), "catppuccin-latte"(라이트). 설정 저장 시 `set_theme()`로 런타임 반영. 레거시 "dark"/"light"는 시작 시 자동 마이그레이션
- `settings.appearance.background_opacity`: wgpu clear color의 알파 값으로 적용. 0.0(투명)~1.0(불투명)
- `settings.appearance.terminal_colors`: 터미널 surface의 focused/unfocused 배경색·글자색 (HexColor, 기본 focused_bg `#000000`)
- `settings.appearance.markdown_colors`: 마크다운 surface의 focused/unfocused 배경색·글자색
- `settings.appearance.explorer_colors`: 익스플로러 surface의 focused/unfocused 배경색·글자색
- `settings.appearance.sidebar_width`: 사이드바 너비가 UI, GPU 렌더러, 터미널 rect 계산에 반영. 렌더 루프에서 설정값과 자동 동기화
- `settings.clipboard.history_enabled`: 클립보드 히스토리 기록 여부
- `settings.clipboard.history_max`: 히스토리 최대 항목 수 (기본 100)
- `settings.clipboard.poll_interval_ms`: 시스템 클립보드 폴링 주기(ms, 재시작 필요)
- `settings.keybindings.copy` / `settings.keybindings.paste`: 복사·붙여넣기 단축키 (다중 바인딩). 플랫폼별 기본값 — Windows: `ctrl+c` / `ctrl+v`, Linux: `ctrl+shift+c` / `ctrl+shift+v`, macOS: `alt+c` / `alt+v`
- `settings.keybindings.zoom_in` / `zoom_out` / `zoom_reset`: 줌 단축키 (다중 바인딩). 플랫폼별 기본값 — Windows/Linux: `ctrl+=` / `ctrl+-` / `ctrl+0`, macOS: `alt+=` / `alt+-` / `alt+0`
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
- `settings.keybindings.copy` 바인딩 목록 중 하나와 매칭되면 복사 (다중 바인딩 지원)
- 플랫폼별 기본값:
  - **Windows**: `ctrl+c` — 선택 있으면 복사, 없으면 SIGINT 전달
  - **Linux**: `ctrl+shift+c`
  - **macOS**: `alt+c`
- 선택 텍스트를 시스템 클립보드에 복사 후 선택 해제
- 키보드 입력 시 선택 자동 해제
- 사용자가 Keybindings → Clipboard 서브탭에서 바인딩을 추가/삭제/변경 가능
- **소프트 랩 인지 복사**: 셸이 긴 명령을 터미널 너비에 맞게 자동 줄바꿈한 라인은 복사 시 한 줄로 다시 합쳐진다. 진짜 `\n`(hard newline)은 그대로 보존. wrap 여부는 라인의 마지막 컬럼이 비공백 글자로 채워졌는지로 판정하며, 화면과 스크롤백(메모리·디스크) 모두 동일 규칙으로 동작. cursor positioning 기반 TUI(vim, htop, Claude Code 등)는 보통 마지막 컬럼을 채우지 않아 영향이 없음

### 붙여넣기 (Paste)
- `settings.keybindings.paste` 바인딩 목록 중 하나와 매칭되면 붙여넣기 (다중 바인딩 지원)
- 플랫폼별 기본값:
  - **Windows**: `ctrl+v`
  - **Linux**: `ctrl+shift+v`
  - **macOS**: `alt+v`
- 브래킷 붙여넣기 모드(DECSET 2004) 지원: 활성화 시 `\x1b[200~` ... `\x1b[201~`로 감싸서 전송
- 포커스된 터미널의 PTY에 직접 전송
- **이미지 붙여넣기**: 클립보드에 텍스트가 없고 이미지가 있는 경우, 이미지를 PNG 파일로 저장(`/tmp/tasty-clipboard/paste-{timestamp}.png`)하고 파일 경로를 터미널에 붙여넣기. AI 에이전트가 이미지를 참조할 수 있도록 지원
- **Paste 후 Ctrl+C 보호 (cooldown 500ms)**: 터미널에 paste를 보낸 직후 500ms 안에 들어온 Ctrl+C는 SIGINT 전송도, 클립보드 복사도 하지 않고 무시한다. Ctrl+V를 누르려다 옆 키 Ctrl+C를 잘못 눌러 입력을 통째로 날려버리는 사고를 막기 위함. 무시될 때 toast로 알림. 500ms 이후의 Ctrl+C는 기존 동작(선택 영역이 있으면 복사, 없으면 SIGINT)으로 정상 처리

### IME 입력 (한글/CJK)
- winit의 IME 이벤트를 통한 CJK 입력기 지원
- 조합 중인 문자를 GPU 셰이더 파이프라인(CellRenderer)으로 터미널 셀 위에 직접 렌더링 (파란색 배경). 터미널 글리프와 동일한 좌표계를 사용하여 셀 그리드와 정확히 일치
- 조합 확정(Commit) 시 PTY로 전송
- 조합 중 단축키(Ctrl/Cmd/Alt + 키)는 physical key 기반으로 올바르게 동작
- 마우스 클릭 시 조합 중인 텍스트를 먼저 커밋한 후 커서 이동
- 분할 패널에서 포커스된 서피스의 실제 위치에 preedit 오버레이 표시
- OS IME 후보창 위치를 실제 셀 좌표 기반으로 동기화 (`set_ime_cursor_area`)
- macOS: 스페이스 이중 입력 방지 및 쉼표/마침표 유실 복구 (winit quirk 대응)
- macOS: 입력소스 전환(한영 전환) 직후 첫 글자가 조합 없이 확정되는 문제 수정 (winit 포크에서 `interpretKeyEvents` 재시도로 해결)
- Linux: Ime::Commit에 트리거 키가 포함되지 않는 동작을 올바르게 처리
- **IME 시뮬레이션 API**: IPC/CLI를 통해 IME 입력을 프로그래밍 방식으로 시뮬레이션. AI 에이전트가 한글/CJK 입력 파이프라인을 직접 테스트할 수 있음
  - `surface.ime_enable` / `surface.ime_disable`: IME 활성/비활성 전환
  - `surface.ime_preedit`: 조합 중 텍스트 표시 (preedit 렌더링 테스트)
  - `surface.ime_commit`: 조합 확정 → PTY 전송
  - `surface.ime_status`: 현재 IME 상태 조회 (active, preedit_text)
  - CLI: `tasty debug ime-enable`, `tasty debug ime-disable`, `tasty debug ime-preedit "ㅎ"`, `tasty debug ime-commit "한"`, `tasty debug ime-status`

### OSC 52 클립보드 설정
- 터미널 프로그램이 OSC 52 시퀀스로 시스템 클립보드에 텍스트를 설정할 수 있음
- termwiz의 `SetSelection` 파싱을 활용하여 이벤트 발생 → main.rs에서 arboard로 클립보드에 반영

### 시스템 클립보드 히스토리
- `EngineState.clipboard_history`(메모리 전용)에 시스템 클립보드 변경 기록
- 별도 스레드가 `settings.clipboard.poll_interval_ms`(기본 500ms) 주기로 `AppEvent::ClipboardTick` 발송 → 메인 스레드가 arboard로 현재 값을 읽어 모든 Window의 history에 기록
- 연속 중복 자동 제거 (`last_seen` 비교), 빈 문자열 무시
- 출처 태그: `ClipboardSource::System`(외부 앱) / `Internal`(Tasty 내부 복사). 내부 복사 지점(터미널 선택 복사, OSC 52)에서는 즉시 `record_internal_copy`로 기록
- 설정: `clipboard.history_enabled`(기본 on), `history_max`(기본 100), `poll_interval_ms`(재시작 필요)
- 주의: 비밀번호 관리자 등 민감 정보도 기록된다. OS 레벨 민감 플래그를 구분할 수단이 제한적이라 1차는 필터 없음
- 재시작 시 휘발(디스크 영속화는 별도 TODO)

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

#### 디버그 전용 (debug 빌드에서만 사용 가능)
다음 메서드들은 `cfg(debug_assertions)` 게이트로 릴리즈 빌드에서 제외된다. 개발 및 테스트 용도로만 존재한다.

- `system.shutdown`: 테스트 종료 시 프로세스 정상 종료
- `ui.state`: GUI 오버레이 상태 조회 (settings_open, notification_panel_open, active_workspace, workspace_count, pane_count, tab_count)
- `ui.screenshot`: 현재 화면을 PNG로 저장
- `debug.info`: 개발용 디버그 정보 조회 (scale_factor, cell 크기, viewport 등). `src/debug_info.rs`를 수정하여 커스텀 정보 추가 가능
- `debug.cell_info`: 특정 셀(row, col)의 텍스트, 색상(fg/bg), 속성을 termwiz `CellAttributes` 단계에서 조회. 노출 필드: `text`, `fg`, `bg`, `bold`, `italic`, `underline`, `strikethrough`, `inverse`, `width`, `intensity` (`normal`/`bold`/`half`), `underline_style` (`none`/`single`/`double`/`curly`/`dotted`/`dashed`), `underline_color`, `blink` (`none`/`slow`/`rapid`), `invisible`, `overline`, `vertical_align` (`baseline`/`super`/`sub`)
- `debug.screen_attrs`: 특정 행 전체의 셀 속성을 일괄 조회 (필드 구성은 `cell_info`와 동일, `col` 추가)
- `debug.glyph_color`: 특정 셀에 대해 **렌더러가 GPU에 push하는 (bg, fg) RGBA**를 반환. `cell_info`가 termwiz 단계의 속성을 보여준다면 이쪽은 그 속성이 실제 색상 결정에 반영되었는지를 검증한다 (`bg_mode: "focused" | "unfocused"` 옵션)
- `debug.inject_mouse`: SGR 마우스 이벤트를 PTY에 주입 (`--enable-input-simulation` 필요)
- `debug.inject_key`: 임의 바이트/텍스트를 PTY에 주입 (`--enable-input-simulation` 필요)
- `--enable-input-simulation` CLI 플래그: debug 빌드에서 입력 시뮬레이션 IPC를 활성화. 이 플래그 없이는 inject_mouse/inject_key가 거부됨 (2단계 게이트: 컴파일 + 런타임)
- `debug` CLI 서브커맨드: `tasty debug info`, `tasty debug ime-*`, `tasty debug cell-info`, `tasty debug screen-attrs`, `tasty debug glyph-color` 등 디버그 관련 CLI 명령

#### 워크스페이스
- `workspace.list`: 전체 워크스페이스 목록 (이름, 활성 여부, 패인 수)
- `workspace.create`: 새 워크스페이스 생성 (선택적 이름, 타입 지정). `--type markdown --file <path>` 등으로 비터미널 워크스페이스 생성 가능
- `workspace.update`: 워크스페이스 이름, 부제, 설명 수정
- `workspace.move`: 워크스페이스 순서 이동 (`from_index`, `to_index`)

#### 윈도우
- `window.list`: 전체 윈도우 목록 (id, focused, title)
- `window.create`: 새 독립 윈도우 생성
- `window.close`: 포커스된 윈도우 닫기
- `window.focus`: 특정 윈도우에 포커스

#### 패인
- `pane.list`: 전체 워크스페이스의 패인 목록 (포커스 여부, 탭 수)
- `split`: 통합 분할 명령. `level`(pane/surface), `target_surface`(surface ID/nickname) 또는 `target_pane`(pane ID)으로 대상 지정, `direction`(vertical/horizontal), `type`(terminal/markdown/explorer/html) 파라미터. pane/surface 레벨 모두 비터미널 타입 지원. 포커스 이동 없음
- `pane.close`: 패인 닫기 (unsplit)

#### 탭
- `tab.list`: 지정 패인의 탭 목록 (id, name, type, surface_id, active)
- `tab.create`: 지정 패인에 새 탭 추가
- `tab.close`: 탭 닫기
- `tab.move`: 탭 순서 이동 (`pane_id`, `from_index`, `to_index`)

#### 서피스
- `surface.list`: 전체 워크스페이스의 서피스 목록 (id, type, pane_id, tab_index, cols/rows). 비터미널 서피스(Markdown, Explorer, Html)도 포함
- `surface.close`: 서피스 닫기
- `surface.close_self`: 호출한 서피스 자신을 닫기 (TASTY_SURFACE_ID 기반)

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
- `window.create` IPC 또는 `tasty new window` CLI로 새 독립 윈도우 생성
- 키바인딩: `new_window` (기본: Alt+Shift+N, macOS에서 Cmd+Shift+N)
- 각 윈도우는 자체 GPU 서피스, egui 컨텍스트, 터미널 세트를 보유
- `window.list`: 전체 윈도우 목록 (id, focused, title)
- `window.close`: 포커스된 윈도우 닫기
- `window.focus`: 특정 윈도우에 포커스
- 윈도우 닫기 시 HashMap에서 제거
- 마지막 윈도우 닫기: `close_behavior` 설정에 따라 동작 (ask/minimize/quit)
- Minimize 동작 플랫폼 분기:
  - macOS: 윈도우 파괴 + 모든 state를 parked_states에 보존 → dock 클릭으로 복원
  - Windows/Linux: `set_minimized(true)`로 태스크바에 유지 → 클릭으로 복원
- 멀티윈도우 minimize 시 모든 윈도우의 state를 보존 (Vec 기반)
- 모달 활성 시 다른 윈도우 입력 차단
- macOS: 커스텀 NSApplicationDelegate로 dock/메뉴 통합
  - dock 아이콘 클릭 시 윈도우가 없으면 자동 복원 (applicationShouldHandleReopen)
  - dock 우클릭 메뉴에 "New Window" 항목 (applicationDockMenu)
  - 앱 상단 메뉴바 File → New Window (Cmd+Shift+N)

### 앱 아이콘
- 커스텀 앱 아이콘 (수박 디자인) 적용
- **런타임 윈도우 아이콘**: 모든 윈도우(메인, 설정, 종료 확인)에 256x256 PNG를 winit `with_window_icon`으로 설정. Windows 태스크바, Linux WM 등에서 표시됨
- **Windows exe 아이콘**: `build.rs` + `winresource`로 `.ico`를 exe에 임베드. 탐색기, 작업 관리자에서 표시됨
- **Windows 트레이 아이콘**: 32x32 PNG에서 디코딩한 실제 아이콘 사용 (기존 하드코딩 사각형 대체)
- **macOS 번들**: `assets/macos/Info.plist` + `assets/icons/icon.icns` 제공. `.app` 번들 생성 시 사용
- **Linux 데스크탑**: `assets/linux/tasty.desktop` 엔트리 파일 + 다양한 크기의 PNG 아이콘 제공
- 아이콘 에셋: `assets/icons/` (icon.icns, icon.ico, icon_16~1024.png)

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
- clap 기반 그룹형 서브커맨드: `new`, `close`, `list`, `set`, `send`, `read`, `unset`, `claude`, `notify`, `surface-meta`, `is-typing`, `debug`
- 포트 파일에서 포트 번호를 읽어 TCP 연결 후 JSON-RPC 요청/응답
- `list tree` 커맨드: 워크스페이스/패인/탭 계층을 트리 형태로 표시 (ID, surface type 포함)
- `list tabs --pane ID` 커맨드: 지정 패인의 탭 목록 조회 (id, name, type, surface_id)
- 에러 시 종료 코드 1 반환

#### 포커스 독립 원칙
- 모든 CLI/IPC 명령은 focus에 의존하지 않고 명시적 ID로 대상을 지정
- 생성/삭제 명령: `--pane`, `--surface`, `--target` 등이 필수. 미지정 시 에러 + 사용법 안내
- 동작 명령 (`send`, `read`, `set` 등): `--surface` 미지정 시 `TASTY_SURFACE_ID` 환경변수에서 자동 채움
- 모든 응답에 실제 적용된 파라미터 값(surface_id, pane_id 등)을 포함하여 묵시적 기본값도 확인 가능

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
  - `ClaudeError`: Claude child PTY가 알려진 비정상 패턴(API Error, content filter, rate limit 등)을 출력했을 때 자동 fire
- **ProcessExit 구현**: 터미널 프로세스 종료 시 ProcessExited 이벤트 자동 발생 및 훅 실행. 프로세스 종료 후 해당 서피스를 자동으로 닫음. 서피스 → 탭 → 패인 → 워크스페이스 순으로 계층을 올라가며 적절한 레벨에서 정리. 마지막 워크스페이스의 마지막 서피스인 경우 새 셸을 스폰
- **정규식 캐싱**: OutputMatch 훅 등록 시 정규식을 사전 컴파일하여 매칭 시 재컴파일 방지
- **once 옵션**: true로 설정하면 한 번 실행 후 자동 삭제
- **비동기 실행**: 훅 명령은 백그라운드 스레드에서 실행 (메인 루프 블로킹 없음)
- **이벤트 루프 통합**: main.rs에서 TerminalEvent 수집 후 Bell/Notification/ProcessExit 이벤트에 대해 자동으로 훅 체크 및 실행
- **Surface ID 추적**: 각 이벤트가 발생한 Surface ID를 추적하여 훅이 올바른 Surface에서 실행
- CLI: `tasty set hook --event bell --command "notify-send 'bell'" --once`
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
- CLI: `tasty set mark`, `tasty read since-mark --strip-ansi`
- IPC: `surface.set_mark`, `surface.read_since_mark` 메서드

### Claude Code 런처 (claude.launch)

Claude Code를 새 워크스페이스에서 자동으로 실행하는 전용 런처.

- 새 워크스페이스 자동 생성 및 이름 설정
- 지정된 디렉토리로 이동 후 `claude` 명령 실행 (shell-escape로 인젝션 방지)
- `--task` 옵션으로 작업 설명 전달 가능 (shell-escape 적용)
- CLI: `tasty claude launch --workspace "my-project" --directory "/path/to/project" --task "Fix the bug"`
- IPC: `claude.launch` 메서드 (workspace, directory, task 파라미터)

### Claude Parent-Child 관계 관리

부모 Claude 인스턴스가 자식 Claude 인스턴스를 생성하고 관리하는 시스템. AI 에이전트가 멀티 에이전트 워크플로우를 구성할 때 사용한다.

- **ClaudeChildEntry**: 자식 surface ID, 인덱스, cwd, role, nickname을 추적하는 데이터 구조
- **부모-자식 매핑**: `HashMap<u32, Vec<ClaudeChildEntry>>`로 부모별 자식 목록 관리, `HashMap<u32, u32>`로 자식에서 부모 역참조
- **자동 정리**: 부모 또는 자식 surface가 닫힐 때 관계를 자동으로 정리. 부모가 먼저 닫혀도 자식이 살아있는 동안 관계 유지 (ghost cleanup)
- **claude.spawn**: 두 가지 모드 지원:
  - **`--surface` 모드** (기존): 대상 surface의 pane을 분할하여 새 터미널 생성 후 `claude` 명령 자동 실행. `--surface`는 pane 분할 위치만 결정
  - **`--workspace` 모드** (신규): workspace를 지정하면 tasty가 spawn pane을 자동 관리. parent surface마다 지정된 workspace 내에 전용 spawn pane을 갖고, 2×2 그리드 알고리즘으로 최적 배치 (1→좌우분할→좌측상하분할→우측상하분할→새탭). 4개 초과 시 탭 확장
  - `--workspace`와 `--surface`는 동시 사용 불가
  - workspace는 ID(숫자) 또는 이름(문자열)으로 지정 가능
  - 부모(parent)는 항상 spawn 명령을 실행한 surface(`TASTY_SURFACE_ID`). cwd, role, nickname, prompt 파라미터 지원
- **claude.children**: 부모 surface의 자식 목록 조회. 각 자식의 surface ID, 인덱스, 메타데이터 반환
- **claude.parent**: 자식 surface의 부모 조회. 부모의 surface ID와 상태(active/closed) 반환
- **claude.kill**: 자식 surface를 종료하고 관계를 정리
- **claude.respawn**: 기존 자식 surface의 터미널 프로세스만 종료하고 같은 surface에서 새 쉘 + `claude`를 재시작. 레이아웃(pane/surface 구조)은 변경하지 않는다. child index와 부모-자식 관계도 유지. cwd, role, nickname, prompt 재설정 가능
- **claude.broadcast**: 부모의 모든 자식에게 텍스트를 동시에 전송. `role` 파라미터로 특정 역할의 자식에만 필터링 가능. 반환: `{ sent_count, children }`
- **claude.wait**: 자식 surface의 현재 상태를 조회. surface가 존재하지 않으면 "exited" 반환. 반환: `{ state: "idle"|"needs_input"|"active"|"exited" }`. CLI에서 폴링 루프로 대기 구현. CLI(`tasty claude wait`)는 시작 시 `~/.claude/settings.json`의 tasty Stop 훅 등록 여부를 점검하며, 미설치 시 stderr에 안내 메시지를 출력하고 비정상 종료(exit 1)한다 (Stop 훅이 없으면 idle/needs-input 이벤트가 fire되지 않아 wait이 영원히 진행되지 않기 때문)
- CLI: `tasty claude spawn --direction vertical --cwd /path --role worker --nickname "agent-1" --prompt "Fix bugs"`
- CLI: `tasty claude children`, `tasty claude parent`, `tasty claude kill --child 1`, `tasty claude respawn --child 1`
- CLI: `tasty claude broadcast "text\r" [--role ROLE]`, `tasty claude wait --child 1 [--timeout SECS]`
- `--child` 파라미터는 child index를 받는다 (spawn 시 반환되는 `child_index` 값)
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
- **자동 정리**: surface가 닫힐 때 (unregister_child, mark_parent_closed) idle/needs_input/error_scan_enabled 상태 자동 제거
- **ClaudeError 자동 감시**: `claude.spawn` / `claude.launch`로 만들어진 child surface는 PTY 출력에 대한 패턴 스캐너가 자동으로 활성화된다. 매 redraw에서 `Terminal::output_since_scan_mark`로 새 출력 슬라이스를 ANSI strip 후 catalog 정규식과 매칭하고, 매칭되면 `claude-error` 훅을 fire한다. 카탈로그(`src/state/claude_error.rs`): `API Error`, `Output blocked by content filtering policy`, `overloaded_error`, `rate_limit_error`, `Bad Request`, `Internal Server Error`, `network error` (대소문자 무시). 일반 셸 surface는 영향 없음. 사용자도 `tasty set hook --event claude-error --surface ID --command ...`로 추가 hook을 걸 수 있다.
- **`tasty claude install` / `uninstall`**: `~/.claude/settings.json`의 `hooks` 객체에 4종 hook entry를
  idempotent하게 추가/제거한다. 등록 대상: `Stop`(메인 응답 종료), `Notification`(권한 요청·idle 알림),
  `SessionEnd`(세션 종료), `SubagentStop`(Task tool 종료). 각 entry의 command는
  `[ -n "$TASTY_SURFACE_ID" ] && tasty claude hook <token> || true` 형태로, tasty 외부에서 claude를 실행할 때는
  무해하게 통과한다. 매칭은 `tasty claude hook <token>` substring 기준으로 동작하며, 사용자가 손수 등록한 다른
  hook entry는 보존한다. 제거 시 빈 이벤트 배열과 빈 `hooks` 객체도 함께 정리한다.
- **wait의 사전 요구사항 점검**: `tasty claude wait`는 시작 시 `is_tasty_stop_hook_installed()` 헬퍼로 Stop 훅
  등록 여부를 확인하고, 미설치(또는 settings.json 파싱 실패)면 안내 메시지를 stderr에 출력하고 exit code 1로
  종료한다. 점검은 install과 동일한 marker 기반 매칭(`is_marker_installed_in_value`)을 공유하므로 등록 판정이
  자동으로 일치한다.
- CLI: `tasty claude install`, `tasty claude uninstall`, `tasty claude hook stop|notification|session-end|subagent-stop|prompt-submit|session-start [--surface ID]`
- IPC: `claude.set_idle_state`, `claude.set_needs_input`, `surface.fire_hook` 메서드

### Surface Metadata Store (surface_meta.rs)

파일 기반 키-값 스토어로, 어떤 프로세스(Claude Code 포함)든 서피스별 임의 메타데이터를 읽고 쓸 수 있다.

- **저장 위치**: OS 임시 디렉토리 — `{tmp}/tasty-surfaces/<surface_id>/meta.json` (Windows: `%TEMP%`, macOS: `/var/folders/...`, Linux: `/tmp`)
- **SurfaceMetaStore**: 정적 메서드만 가지는 유틸리티 구조체 (상태 없음)
- **ensure_created(surface_id)**: 서피스 생성 시 메타 디렉토리와 빈 JSON 파일 생성. `send_fast_init()` 내부에서 자동 호출됨
- **remove(surface_id)**: 서피스 닫힐 때 메타 디렉토리 전체 삭제. 탭/패인/서피스 닫기 시 자동 호출됨
- **set/get/unset/list**: 파일을 읽어 HashMap으로 역직렬화, 수정 후 pretty JSON으로 재직렬화
- **범용 키-값**: 역할(role), 닉네임, 상태 등 에이전트가 필요한 임의 데이터를 저장 가능
- CLI: `tasty surface-meta set|get|unset|list --key KEY [--value VALUE] [--surface ID]`
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
- GPU 렌더 에러, 셸 셋업 에러, 셸 respawn 에러에서 자동 호출
- `record_error()` 글로벌 함수로 호출 (release에서는 no-op)
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
- `AppState::move_focus_forward` / `move_focus_backward`: 탭 내부 Surface 우선 이동, 단일이면 Pane 간 이동
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

### tasty-tui-simulator (TUI 시뮬레이터)
고수준 명령을 raw VTE escape sequence로 변환하여 출력하는 VTE 시뮬레이터. 터미널 입장에서 실제 TUI 앱과 동일한 바이트 스트림을 받는다.
- **인터랙티브 모드**: stdin REPL — 외부에서 `surface.send`로 명령을 단계별로 전송. 명령마다 `OK` 응답으로 동기화
- 명령어: cursor, print, sgr, fg/bg, bold/italic/underline, altscreen, scroll-region, erase, raw, esc 등
- 종료 제어: `quit`(정상), `exit-code N`(코드 지정), `crash`(SIGABRT), `panic`(Rust panic)
- 원샷 시나리오: cursor, colors, attrs, altscreen, unicode, scroll-region (수동 확인용)
- `debug.cell_info` / `debug.screen_attrs` IPC와 조합하여 셀 속성 자동 검증 가능

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
- `image_viewer.*`: 이미지 뷰어 + 그림판
- `convert_popup.image`: 이미지 타입 변환

## 이미지 뷰어 & 그림판

egui 기반 Image Surface 타입. 이미지 파일을 로드하여 표시하고, 간단한 드로잉 편집이 가능하다.

### 뷰어 기능
- **이미지 표시**: PNG, JPEG, BMP, WebP, ICO, TIFF 포맷 로드 및 egui 텍스처 렌더링
- **폴더 내 탐색**: 같은 디렉토리의 이미지 파일을 자동 인식하여 이전(◀)/다음(▶) 버튼으로 이동
- **새로고침**: 디스크에서 이미지를 다시 로드
- **줌**: 마우스 휠로 확대/축소 (0.1x ~ 20x), 줌 1.0 이하에서는 fit-to-window
- **팬**: 확대 상태에서 드래그로 위치 이동
- **더블 클릭**: 줌 리셋 (1.0으로 복귀)

### 그림판 기능
- **편집 모드 토글**: 편집 버튼(✏)으로 드로잉 모드 진입
- **연필 드로잉**: 마우스 드래그로 자유 곡선 그리기 (Bresenham 알고리즘 기반)
- **브러시 조절**: 크기 슬라이더 (1~20px), 색상 선택기
- **저장**: 원본 + 오버레이 합성 후 PNG로 저장 (원본 포맷 무관)
- **취소**: 편집 내용 폐기, 원본으로 복귀
- **새 이미지**: 빈 이미지 surface는 800×600 흰 캔버스가 채워진 상태로 즉시 시작하며, 별도의 크기 입력 단계를 거치지 않는다. 사용자가 다른 크기로 새 캔버스를 만들고 싶을 때만 `+` 버튼으로 크기 입력 팝업을 띄울 수 있다 (팝업 기본값도 800×600).

### 저장 규칙
- 저장 형식은 항상 PNG
- 기존 파일 편집 시: 같은 경로에 `.png` 확장자로 저장
- 새 이미지: 사용자가 파일 경로 지정

### CLI/IPC
- `tasty split --type image --file <path>`: 이미지 뷰어로 분할
- `tasty split --type image`: 새 이미지 (빈 캔버스)
- `tasty new tab --pane ID --type image --file <path>`: 이미지 탭 생성
- `tasty new workspace --type image --file <path>`: 이미지 워크스페이스 생성
- Surface 타입 변환 팝업에서 Image 옵션 선택 가능

### 닫기/복원
- 이미지 탭 닫기 시 ClosedItem에 파일 경로 저장
- Ctrl+Shift+T로 복원 시 같은 이미지를 다시 로드

## 터미널 검색

### 개요
터미널의 스크롤백 + 화면 전체를 대상으로 텍스트 검색. GPU 렌더러에서 매치를 하이라이트하며, 현재 매치(active)와 나머지 매치(inactive)를 다른 색으로 구분한다.

### 단축키
- Tasty 프리셋: `Ctrl+F` / `Alt+F`
- Mac 프리셋: `Cmd+F` (`alt+f`)
- Windows 프리셋: `Ctrl+F`
- Linux 프리셋: `Ctrl+F`
- Escape: 검색 바 닫기
- Enter: 다음 매치
- Shift+Enter: 이전 매치
- 화살표 ↑/↓: 매치 탐색

### 기능
- 대소문자 무시 검색 (기본), 토글 버튼으로 대소문자 감도 전환
- 매치 카운터 표시 (예: 3/42)
- 매치 선택 시 해당 위치로 자동 스크롤
- 검색 바는 sticky_focus PopupDef로 구현: 키보드는 검색 바가 받고, 마우스는 터미널에 전달

### 구현
- 검색 엔진: `tasty-terminal/src/search.rs` (Terminal::search)
- UI 상태: `src/search_state.rs` (SearchState)
- 검색 바: `src/ui/search_bar.rs` (PopupDef, headless + sticky_focus)
- 하이라이트: `src/renderer/mod.rs` (SearchHighlights → 셀별 bg 오버라이드)

## 레이아웃 영속화

### 개요
- 설정 (`general.restore_layout`, 기본 off) 활성화 시 워크스페이스/페인/탭/서피스 구조를 `~/.tasty/layout.json`에 JSON으로 저장
- 앱 시작 시 저장된 레이아웃을 복원하여 이전 세션의 창 배치를 재현

### 저장 대상
- 워크스페이스 목록 (이름, 부제, 설명)
- 페인 트리 구조 (split direction, ratio)
- 탭 목록 (이름, explicit_name, active_tab)
- 서피스 레이아웃 트리 (split direction, ratio)
- 각 서피스의 타입별 최소 정보:
  - Terminal: cwd
  - Markdown: file path
  - Explorer: root path
  - Html: url
  - Image: file path
  - Empty: (없음)
- 활성 워크스페이스 인덱스, 포커스된 페인 인덱스

### 저장 타이밍
- 구조적 변경 시 dirty flag 설정 + 500ms 디바운스
- 대상 이벤트: 워크스페이스/페인/탭/서피스 추가·삭제·분할, 이름 변경, split ratio 드래그, 서피스 타입 변환, 닫힌 항목 복원
- 앱 종료 시 dirty 상태면 즉시 flush

### 복원 시점
- 앱 시작 시 1회만 (`EngineState::new()`)
- `layout.json` 파싱 실패 또는 파일 없음 시 기본 "Workspace 1"로 폴백
- 개별 서피스 복원 실패 시 해당 서피스만 스킵하고 나머지 계속 복원

### TUI 세션 복원

Claude Code 등 TUI 앱이 실행 중이던 터미널을 복원할 때, 해당 앱의 세션을 자동으로 재개한다.

- `tasty claude install`로 `SessionStart` hook이 등록되면, Claude Code 세션 시작 시 세션 ID가 surface 메타데이터(`claude-session-id`)에 저장됨
- `SessionEnd` hook에서 세션 메타를 자동 삭제하여 종료된 세션은 복원 시도하지 않음
- Claude Code의 `${CLAUDE_SESSION_ID}` 치환 기능을 활용하므로 파일 파싱이나 PID 매칭 불필요
- `tasty claude install` 없이 사용해도 오류 없이 일반 셸로 복원됨

복원이 발동하는 경로:
1. **앱 재시작 (레이아웃 복원)**: `restore_layout` 설정 활성화 시, 레이아웃 저장 시점에 세션 ID가 있는 터미널은 `restore_command`를 함께 저장. 복원 시 셸 초기화 후 자동 실행
2. **닫힌 항목 복원 (Ctrl+Shift+T)**: surface/tab/workspace 닫기 시 `ClosedSurface`에 `restore_command`를 포함하여 스냅샷. 복원 시 셸 시작 후 `restore_command` 자동 실행

### 저장하지 않는 것
- 화면 내용 (screen/scrollback)
- PTY 상태, 환경변수, 실행 중인 명령
- 팝업 상태
- ClipboardViewerPanel (Empty로 대체)

## Plugin 시스템

외부 plugin 프로세스를 별도 OS 프로세스로 띄워 surface 종류를 확장한다.
릴리스 에셋의 `plugins.md` 참조.

### 기본 제공 plugin (built-in)
- Tasty 바이너리에 함께 묶여 배포되는 plugin은 첫 실행 시 `~/.tasty/plugins/<id>/`에 자동 설치된다 (`BUILTIN_PLUGIN_IDS` 목록)
- 현재 기본 제공: `com.tasty.explorer` (파일 탐색기 surface)
- 사용자가 plugin 메뉴에서 "제거"를 선택하면 `removed_builtins`에 기록되어 다음 실행에서 자동 재설치되지 않는다 — 외부 plugin과 완전히 동일한 라이프사이클 적용
- 번들 위치 탐색 순서: `TASTY_BUILTIN_PLUGINS_DIR` env > 실행 파일 옆 `plugins/` > dev 빌드 시 `target/<profile>/builtin-plugins/` (workspace 자동 부트스트랩)

### Plugin 관리 모달
- 사이드바 좌측 메뉴의 🧩 버튼으로 PluginsWindow 모달 진입 (Settings 모달과 동일 패턴)
- 좌측 plugin 목록 + 우측 상세: 이름/버전/설명/저자/홈페이지, 활성 토글, 등록 surface kinds, 매니페스트 권한 / grant 상태, 로그 파일 경로
- 권한 grant/revoke 버튼으로 즉시 반영 (process 재시작 없이)
- "제거" 버튼은 사전 확인 다이얼로그를 거친 뒤 plugin 실행 종료 + 디스크 삭제. built-in plugin인 경우 추가 경고 표시

### 매니페스트 + 디스커버리
- `~/.tasty/plugins/<id>/tasty-plugin.toml` 형식 (manifest_version=1, api_version=1)
- 부팅 시 자동 스캔, 매니페스트 검증 실패한 plugin은 warn 로그 후 스킵
- `~/.tasty/plugins.toml`로 활성/비활성 + `removed_builtins` 영속화

### 프로세스 생명주기
- 호스트가 `127.0.0.1:0` 으로 listen, plugin이 token 들고 connect 하는 인증 방식
- stdout/stderr 자동 redirect → `~/.tasty/plugins-logs/<id>.log`
- 15초 ping / 60초 timeout 헬스체크, 비응답 시 자동 재시작
- 10초 내 spawn 실패 3회 시 자동 비활성화 (사용자 수동 enable까지 정지)
- 종료 시 모든 plugin에 graceful shutdown 송신 후 2초 timeout, 그 후 kill

### Surface 렌더링 (UI tree DSL)
- plugin이 JSON UI tree를 보내면 호스트가 egui로 렌더 (vbox/hbox/scroll/splitter/label/icon/button/tree/addressbar/text_preview/spacer)
- 호스트가 사용자 이벤트를 모아 `surface.event`로 plugin에 송신 (click/key/tree_*/addressbar_*/scroll/focus_changed/resize)
- `RemoteSurface` 어댑터가 layout tree에 끼워지므로 본체 surface와 동등하게 split/tab 가능

### 권한 모델
- 매니페스트의 `permissions = [...]`에 14가지 권한 토큰 (surface.read/write, notification, clipboard.read/write, fs.read/write, process.spawn, terminal.*, claude.*, network) 선언
- IPC 라우터 진입에서 plugin caller일 때 `method_meta` 테이블과 매니페스트 권한 대조 — 권한 미선언 시 -32001 거부
- `plugin.*`, `window.*`, `surface.ime_*`, debug.* 메서드는 항상 plugin 호출 불가 (local-only)
- `~/.tasty/plugins.toml`의 `[grants."<id>"].granted = [...]`로 grant 영속화. CLI install은 매니페스트 권한 자동 grant (사용자 의도적 명령으로 간주)
- 권한 변경은 plugin process 재시작 없이 즉시 반영

### 격리 디렉터리
- 호스트가 spawn 시 환경변수 주입: `TASTY_PLUGIN_DIR / DATA_DIR / CONFIG_PATH / LOG_PATH`
- `~/.tasty/plugin-data/<id>/` 런타임 데이터 (업그레이드 시 보존), `~/.tasty/plugin-config/<id>.toml` 사용자 설정

### 관리 IPC/CLI
- `plugin.list / install / remove / enable / disable / permissions / grant / revoke` IPC
- `tasty plugin list / install <path> / remove <id> / enable <id> / disable <id> / logs <id> [--follow] / permissions <id> / grant <id> <perm> / revoke <id> <perm>`
- `logs`는 호스트 IPC 무관 — 파일 직접 출력 (호스트 죽었을 때도 동작)

### plugin → 호스트 IPC 호출
- plugin이 `PluginEvent::IpcCall {call_id, method, params}` 송신
- 호스트가 권한 게이트 통과 시 라우터로 디스패치, 결과를 `ipc.result` 요청으로 회신 (`call_id`로 매칭)

### Plugin SDK
- `crates/tasty-plugin-protocol`: 호스트와 plugin이 공유하는 wire 타입 (UI tree DSL, JSON-RPC envelope). serde 외 의존성 없음
- `crates/tasty-plugin-sdk`: plugin 작성용 SDK — `Plugin` trait 구현 후 `tasty_plugin_sdk::run(plugin)` 호출이면 핸드셰이크/메시지 루프 자동 처리
- `ui::*` 빌더 헬퍼 (vbox/hbox/scroll/splitter/label/button/tree/addressbar)
- `env::PluginEnv`로 호스트 환경변수 일괄 로딩

### 동봉 plugin 예시
- `tasty-plugin-explorer`: 외부 binary로 작성된 파일 탐색기. SDK만 의존하며 호스트 코드 의존 없음. 디렉터리 트리/미리보기/주소창 입력으로 root 변경 지원
- 매니페스트 `[[contributes.commands]]`: `explorer.refresh` (F5), `explorer.go_up` (alt+up)
- 호스트의 빌트인 `ExplorerPanel`은 단계 08D에서 제거되어 plugin으로 일원화

### Plugin 단축키
- 매니페스트 `[[contributes.commands]]`로 plugin이 자기 surface에서 받을 단축키를 선언. `id`, `title_i18n_key`, `default_keybinding`, `binding_mode` 필드
- `binding_mode`:
  - `"independent"` (기본): plugin 자체 키. 호스트 키와 무관
  - `"inherit:<host_action>"`: 호스트의 의미론적 액션을 따라감. 화이트리스트는 `clipboard.copy`, `clipboard.paste`, `clipboard.cut`, `select_all`
- 매칭 우선순위: focused surface가 plugin RemoteSurface일 때 plugin 키가 호스트 액션보다 먼저 매칭. 매칭 시 이벤트 소모 → 호스트 액션은 트리거되지 않음 (`src/plugin/key_dispatch.rs`)
- 호스트 → plugin: `command.invoke` IPC 메시지 (`{ surface_id, command_id }`). SDK는 `Plugin::handle_command(CommandInvokeCtx)` 콜백으로 전달
- 사용자 오버라이드 영속화: `~/.tasty/plugins.toml`의 `[keybindings."<plugin-id>"]` 섹션. 형태: `mode = "key" | "inherit" | "none"` + 부속 필드
- 설정 → 단축키 → **Plugins** 탭: 좌측 카테고리에서 `Plugins` 선택 → 상단 드롭다운으로 plugin 선택 → 각 command별로 Mode 콤보(Inherit/Custom/None) + Inherit source 콤보(화이트리스트 4종) 또는 Custom 키 텍스트 입력 + Reset 버튼. 변경은 모달 close 시점에 plugins.toml에 기록

### Plugin i18n
- 매니페스트 `lang_dir` (기본 `"lang"`): plugin 디렉터리 내 lang 파일들이 위치
- 호스트는 plugin 디스커버리 시 `<lang_dir>/en.toml`(fallback) + `<lang_dir>/<active>.toml`을 읽어 namespace overlay로 호스트 i18n registry에 머지
- lookup 순서: 호스트 base → plugin namespaces. base에 동일 키가 있으면 plugin은 호스트 키를 덮어쓸 수 없음
- plugin install 시 `register_namespace`, remove 시 `unregister_namespace` (`crates/tasty-core/src/i18n.rs`)

### 한계
- IPC 게이트는 plugin이 호스트를 통한 호출만 막음. plugin이 직접 fs를 쓰면 호스트가 알 수 없음 — 향후 OS-level 샌드박스/WASM으로 보강
- 호스트의 빌트인 ExplorerPanel은 단계 08D에서 외부 plugin으로 일원화 예정 (1300+ 줄 침습적 refactor라 별도 작업으로 분리)
