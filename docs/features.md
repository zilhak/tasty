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
- 다중 서피스를 단일 submit cycle 로 배치 (Phase F.G.a): `CellRenderer` 는 `begin_frame` / `append_terminal_viewport` / `flush_buffers` 로 모든 서피스 인스턴스를 누적한 뒤 kind 당 `queue.write_buffer` 1 회 + 하나의 encoder 로 제출. per-surface 오프셋은 인스턴스 attribute (`viewport_offset`) 에 박혀 있어 셰이더가 read
- WGSL 셰이더: NDC 변환, 텍스처 샘플링

### 폰트 래스터라이징
- cosmic-text FontSystem/SwashCache를 이용한 글리프 래스터라이징
- **번들 D2Coding ligature 폰트** (NAVER, OFL 1.1): Regular/Bold ttf를 바이너리에 임베드해 사용자의 OS에 D2Coding이 설치되지 않아도 동작. `font_family`가 빈 문자열이거나 `"monospace"`일 때 자동 적용되며, 다른 폰트를 지정해도 D2Coding은 폰트 DB에 남아 fallback face로 동작. `Shaping::Advanced`로 합자(`==`, `!=`, `=>`, `->`, `<=`, `>=` 등) 자동 적용. 한자/가나는 시스템 CJK 폰트로 자동 fallback
- 2048×2048 R8 텍스처 페이지에 선반(shelf) 기반 글리프 패킹
- 다중 페이지 글리프 아틀라스 + LRU eviction (Phase F.G.b): D2Array 4 layer (`MAX_PAGES = 4`, 페이지당 ~4 MiB) 로 확장. 활성 페이지가 가득 차면 다음 페이지로 이동, 모든 페이지 full 이면 활성 페이지를 제외한 가장 오래 미사용 (`last_access_frame` 최소) 페이지를 evict 후 해당 layer 만 zero-clear. 옛 *전체 아틀라스 리셋* 정책은 폐기 — CJK 세션의 주기적 stutter 해소
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
- Surface trait: 모든 콘텐츠 타입의 공통 인터페이스. 각 타입이 독립 struct로 구현. **`tasty-model`는 GUI-free** — 모델은 식별 정보와 직렬화 가능한 상태만 보유한다 (egui는 optional `egui-compat` feature, 헤드리스 플러그인은 비활성 가능)
  - `kind()`: 소문자 식별자 — 호스트 빌트인 2종(`"terminal"`, `"empty"`) + plugin 등록 kind(예: `"explorer"`, `"image"`, `"markdown"`). IPC/registry/플러그인이 식별자로 사용
  - `type_name()`: 표시용 라벨. 식별 비교 금지
  - `webview_url()`: webview-enabled surface(plugin) 가 자신의 URL 을 반환. host 의 native WebView 동기화 가 다운캐스트 없이 generic 으로 사용
  - TerminalSurface: 단일 PTY 터미널
  - HtmlPanel, EmptySurface: 호스트 빌트인 비터미널 콘텐츠
  - ImagePanel: `com.tasty.image` plugin이 host-rendered kind로 등록하는 비터미널 콘텐츠
  - MarkdownPanel: `com.tasty.markdown` plugin이 host-rendered kind로 등록하는 비터미널 콘텐츠
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
- HTML (com.tasty.html plugin): URL 의 부모 디렉터리는 plugin 측에서 결정
- Image / Empty: None (ClipboardViewer 는 surface 가 아닌 plugin popup 으로 이전됨)

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
- 호스트(`src/view/main`)에서 winit `ModifiersState`로 수정자 키를 추적. `tasty-settings`는 winit에 의존하지 않으며, `LinkModifier::matches`는 `(ctrl, alt, super)` 원시 bool을 받는다

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

### 추가 Surface 타입 (Markdown / Empty + plugin 기반 Explorer, HTML, Image)
- 모든 Surface 타입은 고유 surface_id를 가지며, 닫기/포커스/리스트 등 공통 surface 동작이 동일하게 적용됨
- Markdown/Empty: 호스트가 egui로 렌더링
- Explorer: **com.tasty.explorer 기본 제공 plugin**이 RemoteSurface로 제공 — UiTree 트리를 IPC로 호스트에 전송, 호스트는 egui로 그대로 렌더링
- HTML: **com.tasty.html plugin** 이 webview-enabled surface kind 로 등록. host 는 OS 네이티브 WebView (macOS: WKWebView, Windows: WebView2, Linux: WebKitGTK) 토대만 제공 (`crate::webview::*`) 하고, html 도메인 로직은 모두 plugin 안. plugin 이 `webview.set_url(surface_id, url)` IPC 로 URL 제어.
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
- **자동 upgrade**: 부팅 시 bundle 의 manifest version 이 설치본보다 높으면 자동으로 디렉토리를 덮어쓴다 (semver 기반, mtime fallback 없음). 다운그레이드는 자동으로 차단되며, 복구가 필요할 땐 `tasty plugin upgrade-builtins --force` 로 수동 재설치 가능. `removed_builtins` 에 박힌 항목을 다시 설치하려면 `--restore-removed <ID>` (여러 개 반복) 또는 `--restore-removed-all` flag 로 명시 호출. 실행 중인 builtin process 를 새 binary 로 즉시 교체하려면 `--restart-running` (graceful swap). 상세: [`docs/dev-guide/plugin-ecosystem.md` §6](dev-guide/plugin-ecosystem.md).
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
  - **복사**: 선택된 파일/폴더를 OS 파일 클립보드에 복사
  - **삭제**: OS 휴지통으로 이동 (`trash` 크레이트 사용)
  - 우클릭 대상이 현재 선택 목록에 포함되면 선택 전체가 메뉴 대상, 포함되지 않으면 선택 초기화 후 클릭 항목만 대상 (VS Code 방식)
  - 상세 동작 분기: `docs/design/explorer-context-menu.md` 참조

#### 컨텍스트 메뉴
- 터미널 영역 또는 탭 바 빈 공간에서 마우스 우클릭 시 컨텍스트 메뉴 표시
- "Open Markdown..." → 파일 경로 입력 다이얼로그 → 마크다운 탭 열기
- "Open HTML..." → URL 입력 다이얼로그 → HTML WebView 탭 열기
- "새 이미지" → 빈 이미지 surface 탭 생성 (기본 800×600 흰 캔버스가 즉시 그려진 상태로 시작, 다른 크기를 원하면 surface 안의 `+` 버튼으로 팝업 호출)
- 터미널 surface 영역 우클릭 시: "터미널 ID 복사" → 해당 surface id를 클립보드에 복사하고 surface 스코프 toast로 알림
- 좌클릭 또는 Cancel로 메뉴 닫기

#### 키보드 단축키
- `open_markdown`: 마크다운 열기 (파일 경로 입력 다이얼로그 표시)
- 기본값 미설정 (설정 UI에서 Pane 서브탭에서 바인딩 가능)

#### Surface 타입 전환
- `convert_surface` 단축키 (기본 `Alt+'`): Surface 스코프 팝업으로 전환 메뉴 표시. **항목은 `SurfaceKindRegistry`에 등록된 모든 kind에서 동적으로 enumerate된다** — 빌트인 Terminal(T)/Markdown(M) + plugin 제공 kind (예: Image(I), Explorer(E)). `empty` 같은 시스템 kind는 제외. 팝업 크기는 항목 수에 맞춰 sizer가 매 프레임 재계산
- `convert_to_markdown`: 직접 전환 단축키 (기본값 없음, 설정에서 할당)
- 현재 타입과 동일한 항목은 체크 표시 + 비활성
- Markdown 전환 시 파일 경로 입력 다이얼로그 표시
- Terminal 전환 시 새 PTY 생성
- Esc / 외부 클릭 / X 버튼으로 팝업 닫기
- 키보드 탐색: Up/Down 방향키로 항목 이동, Enter로 선택 확정
- 단축키: 각 kind 첫 글자(영문)로 즉시 선택 — 빌트인은 T/M/H/I, plugin은 kind 첫 글자(중복 시 뒷 항목 무시)
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
- IPC: `tab.create`에 `type` 파라미터로 통합 (`terminal` / `markdown` / `explorer` + plugin contribute)
- CLI: `tasty new tab --pane <PANE> --type html --url <URL>`

## 레이아웃 프리셋 (Layout Presets)

Workspace / Tab / Pane 레이아웃과 각 leaf surface 의 초기화 파라미터(kind, cwd, 시작 명령어, kind 별 params)를 미리 저장해두고 재사용할 수 있다. `ClosedItem`(닫힌 항목 복원, 인메모리 LIFO)과 달리 디스크에 영구 저장되며 반복 사용을 의도한다.

### 종류
- **Workspace Preset**: 워크스페이스 전체 (상위 레이아웃 + 모든 pane/tab/surface)
- **Tab Preset**: 단일 탭 (이름 + 하위 레이아웃 + surface 들)
- **Pane Preset**: 단일 페인 (탭 목록 + 활성 탭 + 각 탭의 하위 레이아웃)

세 종류 모두 `LayoutPreset` trait 를 구현하며 `tasty-presets` 크레이트에 정의된다.

### 저장
- 사이드바 워크스페이스 카드 우클릭 → "워크스페이스 프리셋으로 저장"
- 탭 타이틀 우클릭 → "탭 프리셋으로 저장" 또는 "페인 프리셋으로 저장"
- 탭바 빈 공간 우클릭 → "페인 프리셋으로 저장"
- 좌측 하단 도구 메뉴 → "프리셋" 으로 PresetView 직접 오픈

저장 위치: `~/.tasty/presets/{workspace,tab,pane}/<name>.toml`. 파일명이 정본 — 같은 kind 내 이름 중복 불가. 충돌 시 `unique_name`이 `-N` suffix 를 자동 부여.

### 편집
PresetView(EditorView 계열, modeless, 종류별 1개 인스턴스)에서 좌측 리스트로 항목을 고르고 우측에서 이름, subtitle(workspace), 레이아웃 트리, 각 leaf surface 의 (kind, cwd, 시작 명령어, kind 별 파라미터)를 편집한다. 시작 명령어 입력 폼은 surface kind 가 `terminal` 일 때만 표시된다.

### 적용
- 단축키(`apply_workspace_preset` / `apply_tab_preset` / `apply_pane_preset` — 기본 빈 칸, 사용자 할당): 적용 popup 을 열고 항목 선택 → Enter → 새 워크스페이스/탭/페인 생성 + 포커스 이동
- CLI: `tasty preset apply --kind ... --name ...` (포커스 이동 없음)
- IPC: `preset.apply` (포커스 이동 없음)

terminal 의 시작 명령어는 PTY 가 ready 된 직후 stdin 에 한 줄로 자동 입력된다.

### IPC / CLI 표면
| IPC method | CLI subcommand | 권한 |
|------------|----------------|-----|
| `preset.list` | `tasty preset list --kind <k>` | `SurfaceRead` |
| `preset.get` | `tasty preset get --kind <k> --name <n>` | `SurfaceRead` |
| `preset.save` | `tasty preset save --kind <k> --name <n> --file <p> [--overwrite]` | `SurfaceWrite` |
| `preset.delete` | `tasty preset delete --kind <k> --name <n>` | `SurfaceWrite` |
| `preset.rename` | `tasty preset rename --kind <k> --from <a> --to <b>` | `SurfaceWrite` |
| `preset.capture` | `tasty preset capture --kind <k> --source-id <id> [--name <n>]` | `SurfaceWrite` |
| `preset.apply` | `tasty preset apply --kind <k> --name <n> [--target-pane <id>] [--target-workspace <id>]` | `SurfaceWrite` |

`preset.apply` 는 CLI/IPC 경로에서 항상 `focus: false` — 포커스 독립성 원칙. 단축키 호출만 새 인스턴스로 포커스가 이동한다.

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

### 알림 사운드
- `settings.notification.sound` 가 true 일 때 신규 알림 발화 시 OS 기본 beep 1 회 재생 (cascade 진입점은 `cascade_notification_pushed`)
- coalesce 로 묶인 알림 (동일 source + 500ms 내) 은 자동 비음 — host event 가 생성되지 않으므로 sound gate 도 통과하지 않음
- 터미널 `\a` (Bell) 경로는 OS 가 자체 beep 할 수 있어 안전 default 로 skip — 사용자 인지 비용 0
- 플랫폼 impl: macOS `NSBeep`, Windows `MessageBeep(MB_OK)`, Linux `paplay → aplay → stderr \a` 3 단 폴백. headless 빌드는 NoopPlayer 로 대체

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
- 항목 출처: **Plugin contribute** 전용. `[[contributes.tool]]` + `ui.tool_item` 권한 grant된 활성 plugin이 항목을 제공한다 (호스트 자체 빌트인 항목은 없음 — 클립보드 히스토리 등은 모두 plugin이 contribute한다)
- 클릭 dispatch (`ToolAction`):
  - `event` — Event Bus로 `event_key` 발화 (payload `{"tool_id": "<key>"}`)
  - `open_surface` — 포커스된 pane에 `surface_kind` 새 탭 추가
  - `open_popup` — `[[contributes.popup]]`로 contribute된 popup 인스턴스를 새로 open (`popup_id`는 `<plugin_id>/<id>` 형식)
- 정렬: `order_hint` 오름차순 (기본 100), 동률은 키 순
- 라벨: `label_i18n_key`를 `t()`로 번역. 키가 catalog에 없으면 키 자체를 fallback 표시
- 바깥 클릭 시 자동으로 닫힘 (`close_on_outside_click`)
- 디버그: `tasty debug tool list` / `tasty debug tool invoke --key <key>`로 IPC 조작 가능 (debug 빌드 한정)

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

## 휴먼 핸드오프 (Approval)

위험한 동작 전에 사용자 결정을 동기적으로 받는 요청-응답 결정 게이트. `tasty-approval` 크레이트가 도메인 로직을, 호스트가 popup/persistence/CLI/IPC를 담당한다. 상세 사용 가이드: [agent-guide/approval.md](agent-guide/approval.md).

### 핵심 컴포넌트
- `tasty-approval` 크레이트: `ApprovalRequest`/`ApprovalRecord`/`ApprovalState`/`Severity` 도메인 모델 + `ApprovalStore` (in-memory queue + waiters). plugin이 자기 요청에 응답하는 self-response는 `-32011 self_response_forbidden`으로 거부
- IPC 9종 (Permission `Approval`; summary set/get은 `MemoryWrite`/`MemoryRead`도 함께 필요):
  - `approval.request` — 새 요청 생성. `severity` ∈ {info, warn, danger}, `workspace_id` 자동 fallback
  - `approval.respond` — GUI/CLI 양쪽이 같은 IPC로 수렴
  - `approval.await` — local-only (blocking + timeout). plugin 호출 미지원, host 내부 worker thread에서만
  - `approval.cancel`, `approval.get`, `approval.list`, `approval.history` — 조회/취소
  - `approval.summary.set`/`get` — workspace별 markdown 세션 요약 (수동 작성)

### Popup 통합 (Phase 3.2)
- popup `"approval"`이 `pending_approval_ids` 큐의 head를 그림. severity ∈ {warn, danger}는 popup + 알림, info는 알림만
- 응답 경로 3가지가 모두 같은 IPC `approval.respond`로 수렴: ① popup 선택지 버튼 클릭, ② popup 단축키 1..=9 (선택지 순서), ③ CLI `tasty approval respond`
- 응답 후 head pop → 큐가 비어 있지 않으면 자동으로 다음 head로 재오픈
- Esc는 의도적으로 차단 — 우회 응답 방지

### 영속 & 히스토리 (Phase 3.3)
- 매 상태 전이마다 record를 `tasty.approval.<id>` 키로 직렬화. `workspace_id` 있으면 `scope=workspace:<id>`, 없으면 `scope=global`
- `approval.history`는 모든 scope의 prefix `tasty.approval.` 키를 훑어 in-process 필터링 (since/until/workspace_id/requester_id/decision/state/limit). 재시작 후에도 조회 가능
- 응답/타임아웃/취소 후에도 record는 보존됨

### 세션 요약 (Phase 3.5)
- 별도 키 `tasty.approval.summary` (`scope=workspace:<id>`) — history와 격리
- markdown 자유 입력. CLI는 `@file` prefix로 파일 내용 첨부 지원
- 향후 Phase 4.5의 `telemetry.session_summary`(자동 생성)와는 별개 경로

### CLI
- `tasty approval {request,respond,await,cancel,get,list,history,summary {set,get}}`

## 에이전트 텔레메트리 (Telemetry)

비용·관측·이상 탐지의 기반이 되는 메트릭 수집 계층. `tasty-telemetry` 크레이트가 도메인 로직(이벤트 모델 / 키 컨벤션 / 순수 집계 / Cost Cap 타입)을, 호스트가 `tasty-memory` 영속화와 IPC/CLI 어댑터를 담당한다. 단계 4.1-4.2 는 raw event 기록 / 즉시 집계 / dispatcher 자동 카운트, 4.3a 는 Cost Cap CRUD, 4.3b 는 record 후 inline cap 평가 + `Notify` 액션 발화. 잔여 액션(Stop/Pause/RequireApproval)·이상 탐지·자동 요약은 후속 sub-phase 에서 켜진다.

### 핵심 컴포넌트
- `tasty-telemetry`: `TelemetryEvent` (agent / workspace_id / metric / value / op / ts / tags) + `MetricBucket` (1m/1h/1d 윈도우) + `Op::{Set,Inc,Dec}` + `summarize_events`/`aggregate_into_buckets`/`top_n` 같은 pure aggregation. `validate_metric` (`[a-z][a-z0-9_]*`, 1..=64) / `validate_agent_id` (`[a-zA-Z0-9_-]+`, 1..=64) 으로 키 안전성을 보장
- `AgentId`: 단계 4.0의 잠정 식별 모델. Plugin caller → manifest `plugin_id`, Local caller → env `TASTY_AGENT_ID` (없으면 `_host`). Phase 6의 session token 인증 도입 시 verifiable 로 승격됨 (자세한 한계: `docs/dev-guide/agent-identification.md`)
- 키 컨벤션: `tasty.telemetry.event.{ts:013}.{seq:04}` (이벤트), `tasty.telemetry.bucket.{w}.{m}.{a}.{ws:013}` (롤업 버킷, 4.2+에서 사용). 같은 ms 안의 충돌을 막기 위해 `TelemetrySeq` AtomicU64 가 host singleton 으로 단조 시퀀스 발급

### Dispatcher 자동 카운트 (Phase 4.2)
- `handle_with_caller` 가 권한 검사 직후 `record_ipc_call(state, caller, method)` 호출 — 비-host caller 의 모든 IPC 가 자동으로 `ipc_calls` metric 으로 적재된다 (`method` 태그로 식별자 구분)
- `_host` agent 와 `telemetry.*` 메서드 자체는 카운트 제외 (자기-측정 / 재귀 폭주 방지)
- 실패는 best-effort warn 로그 — IPC dispatch 를 막지 않음
- cap_eval 통합 (Phase 4.3) 은 같은 진입점에 후행 도입 예정

### IPC 측정 5종 (Permission `Telemetry`)
- `telemetry.record` — 단일 메트릭 이벤트 기록. `metric` (필수), `value`, `op` ∈ {set, inc, dec}, `agent` (선택, default caller agent), `workspace_id` (선택, default 활성 워크스페이스), `tags` (선택, string→string). 응답: `{ key, ts, agent, metric }`
- `telemetry.record_batch` — `events: []` 배열을 한 번에. 모든 이벤트는 동일한 `ts` 와 단조 증가 `seq` 로 저장됨
- `telemetry.summary` — (metric, agent) 별 집계 (`sum/count/min/max/last`). 필터: `metric`/`agent`/`workspace_id`/`since`/`until` (unix ms)
- `telemetry.timeseries` — 윈도우 단위 버킷 시계열. `metric` 필수, `window` ∈ {1m, 1h, 1d, default 1m}. 단계 4.1은 raw event 에서 즉시 집계 (사전 롤업 캐시는 4.2+ 도입)
- `telemetry.top` — `by` ∈ {agent, workspace} 기준 sum 내림차순 top-N (default limit 10)

### Cost Cap CRUD (Phase 4.3a, Permission `Telemetry`)
- 도메인 타입: `CapWindow ∈ {Total, Hour, Day}`, `CapAction ∈ {Stop, Pause, RequireApproval, Notify}`, `CostCap { id, agent, metric, threshold, window, action, created_at, triggered? }`, `CapTriggered { at, value }`. 모두 `tasty-telemetry` 가 순수 도메인으로 보유
- 영속: `Scope::Global` 의 `tasty.telemetry.cap.{id}` (workspace 비종속, agent 단위). cap id 는 `cap_{ts:013}{seq:04}` (`TelemetrySeq` 재사용)
- `telemetry.cap.set` — 새 cap 정의. `{ agent, metric, threshold>0, window?=total∈{total,1h,1d}, action?=notify∈{stop,pause,require_approval,notify} }` → `CostCap` (생성된 `id` 포함)
- `telemetry.cap.list` — `{ agent? }` 필터로 cap 목록 조회 → `{ entries[], count }` (created_at 오름차순)
- `telemetry.cap.remove` — `{ id }` → `{ removed: true, id }` (없으면 `-32004 not_found`)
- `telemetry.cap.status` — `{ agent? }` → `{ entries[], count }`. 각 entry 는 cap 본체 + `current_value` (윈도우 내 raw event sum) + `ratio` (current/threshold)
- `telemetry.cap.reset` — `{ id? }` 또는 `{ agent? }` (둘 중 최소 하나) → `{ reset_ids[], count }`. 매칭된 cap 들의 `triggered` 필드를 비워 액션 재발화 가능 상태로 되돌림
- **평가 (Phase 4.3b)**: `record` / `record_batch` / dispatcher 자동 카운트 직후 inline 으로 cap 평가 — agent+metric 가 일치하는 미발화 cap 들의 `current_value` 를 즉시 계산해 `threshold` 이상이면 `triggered: { at, value }` 마크 후 액션 발화. evaluate 자체는 best-effort warn 로그 — 실패해도 record 응답은 영향 없음
- **Notify 액션 (Phase 4.3b)**: 활성 워크스페이스에 알림 추가 (`title="Cap '<metric>' 임계 도달"`, body 에 agent/metric/value/threshold/window/cap_id 포함). 차단 없음
- **Stop / Pause 액션 (Phase 4.3c)**: cap 이 `triggered` 인 plugin agent 의 모든 IPC 는 dispatcher pre-check 에서 `-32007 cap_blocked` 로 거부된다. trigger 시점에 동시에 알림을 발행해 차단 사실이 사용자에게 보인다. CLI/Local caller 는 검사 대상이 아니므로 `tasty telemetry cap reset --id <ID>` 로 해제 가능. `Stop` 의 OS 프로세스 종료(claude.kill) 트리거는 별도 `claude.kill` IPC 가 도입될 때 결합 (현재 Stop 과 Pause 의 실효 동작은 동일 — 후속 IPC 거부)
- **RequireApproval 액션 (Phase 4.3d)**: cap 이 처음 triggered 되면 host 가 자동으로 `approval.request` 발행 (severity=warn, body 에 reset 명령 포함). 이후 plugin IPC 는 `Stop`/`Pause` 와 동일하게 `-32007 cap_blocked` 로 거부 — 사용자가 popup 에서 결정한 뒤 `cap.reset` 으로 재개

### 영속화 정책
- workspace_id 있는 이벤트 → `scope=workspace:<id>`, 없으면 `global`
- 조회 시 workspace_id 명시되면 단일 scope 만, 아니면 store 의 모든 scope 순회 후 필터링
- TTL 없음 (단계 4.1) — 추후 retention 정책은 cap/롤업과 함께 도입

### 이상 탐지 (Phase 4.4)
- `tasty-telemetry::AnomalyDetector` — in-memory sliding window 기반 휴리스틱. 호스트 재시작 시 윈도우 비워짐 (영속은 anomaly 레코드만)
- **CallBurst (활성)**: 동일 (agent, method) 가 1분 내 1000 회 이상 호출되면 발화. dedup 쿨다운 1분 — 같은 burst 가 매 호출마다 spam 되지 않음
- **SlowLoop / RssSurge (미활성)**: 타입만 정의됨. 추가 신호(메서드 시퀀스 패턴, agent RSS 보고)가 필요해 후속 sub-phase 에서 켜진다
- 발화 시: notification 발행 + `tasty.telemetry.anomaly.{ts:013}.{id}` 키로 Global scope 영속
- `telemetry.anomaly.list` IPC — `agent` / `kind` / `since` / `until` 필터, `detected_at` 오름차순. anomaly_rule.set/remove 는 후속 phase

### 세션 요약 (Phase 4.5)
- `telemetry.session_summary` IPC — 결정론적 순수 집계 (LLM 호출 없음)
- 파라미터: `workspace_id?` (없으면 전 workspace 합산), `since?`, `until?`, `format?∈{markdown,json}` (기본 `markdown`), `top_n?` (기본 10)
- 집계 항목:
  - **tokens**: `ipc_calls` 를 제외한 모든 metric 의 sum (k:v map)
  - **ipc_calls**: `ipc_calls_total` 과 method 별 top-N (sort desc by count, tiebreak by method name)
  - **approvals**: total / pending / responded / timed_out / cancelled + responded choice 별 count (`tasty.approval.*` 영속 레코드를 모든 scope 에서 prefix scan)
  - **anomalies**: Global scope 에서 prefix scan, since/until 윈도우 적용
- markdown 출력은 헤더+표 구조. json 은 동일한 SessionSummary 구조체를 그대로 직렬화
- 영속(`tasty.telemetry.session_summary.*`) 은 옵션 — 본 sub-phase 에선 생략

### Claude Code hook 통합 (Phase 4.6)
- `tasty-plugin-claude` 가 `claude.hook` 이벤트를 텔레메트리에 자동 적재 (manifest 에 `telemetry` 권한 추가)
- `session-start`: state 에 시작 시각 기록 (HostCall 없음)
- `stop` / `subagent-stop` / `session-end`: 시작 시각이 있으면 `wall_time_ms = now - start` 를 `telemetry.record` 로 발행 (`tags.surface_id` 포함)
- `notification --message <text>`: 텍스트에 `\btokens?:\s*(\d+)\b` (정규식 없이 수동 스캔, 워드 경계 검증) 매칭 시 매칭값으로 `input_tokens` 발행
- 측정 주체 agent 는 `tasty.com.tasty.claude`. 호스트 재시작 시 wall_time_starts 휘발 — 진행 중 세션은 누락만 발생하고 잘못된 값은 나오지 않는다
- CLI: `tasty claude hook <event> [--surface] [--session] [--message]`

### CLI
- `tasty telemetry {record,summary,timeseries,top}` — 단일 record 기록 / 집계 조회
- `tasty telemetry cap {set,list,remove,status,reset}` — Cost Cap CRUD
- `tasty telemetry anomaly list` — 검출된 이상 신호 조회 (`--agent`, `--kind`, `--since`, `--until`)
- `tasty telemetry session-summary` — 세션 요약 (`--workspace-id`, `--since`, `--until`, `--format`, `--top-n`)

## 협업 primitive (Phase 5)

다중 에이전트가 협업할 때 필요한 동기화·의존성·합성 primitive. 신규 단일 namespace `agent`에 Task/Barrier/Semaphore/Lease/Reducer/Rate Limit가 modifier로 묶인다. 신규 권한 토큰 `agent` (`Permission::AgentManage`).

### Task primitive (Phase 5.1)

**DAG + state 머신** — `tasty-agent` 크레이트에 `Task`/`TaskState`/`TaskCommand`/`TaskGraph`/`TaskStore` 정의. 영속은 `tasty.agent.task.<id>` 키, scope = `workspace:<id>`.

- **TaskState 8종**: `Waiting` (의존성 미충족) / `Ready` / `Running` / `Succeeded` / `Failed { error }` / `Cancelled` / `Skipped` (의존성 실패로 자동 스킵) / `Unknown` (재시작 후 Running이던 task — 사용자가 retry/cancel 결정 필요)
- **TaskCommand 4종**: `ClaudeSpawn` (claude.spawn 호출) / `Run` (terminal에서 명령 실행) / `Custom` (임의 IPC 위임) / `Reduce` (5종 reducer 전략) — 실제 실행 wiring은 후속 sub-phase
- **OnFailure 3종**: `Abort` (downstream 모두 Skipped, 기본) / `ContinueDownstream` (실패를 성공처럼 취급) / `Fallback { task?, inline? }` (대체 task로 우회 — Phase 5.6 부터 main 실패 시 fallback task 가 자동 Ready, fallback 의 succeed/fail 도 main 의 downstream 으로 전파됨. Phase J.A 부터 `inline: InlineFallbackSpec` 으로 동적 생성 지원 — main Failed 시 `TaskStore::create` 가 새 task 발급, `metadata.fallback_of` 로 idempotency)
- **사이클 검출**: DFS 3-color로 `create()` 시점에 검증. unknown dependency도 같은 단계에서 거부
- **자동 cascade**: 임의 task의 state가 바뀌면 transitive downstream을 재평가해 `Waiting → Ready/Skipped`로 자동 전이

### IPC

| method | 권한 | 동작 |
|---|---|---|
| `agent.task_create` | AgentManage | 새 task 생성. `workspace_id`/`name`/`command`/`depends_on?`/`on_failure?`/`metadata?` |
| `agent.task_list` | AgentManage | 워크스페이스 task 목록. `state?` 필터 |
| `agent.task_get` | AgentManage | 단건 조회 |
| `agent.task_await` | AgentManage | task 가 종결 상태에 도달할 때까지 **blocking** 대기 (Phase J.A). 응답 `{outcome: "terminal"|"timed_out"|"not_found", state?, result?}`. `timeout_ms` 미지정 또는 0 = 무한 대기. TaskWakerHub 가 set_state 종결 분기에서 fire |
| `agent.task_cancel` | AgentManage | 명시적 취소. downstream cascade |
| `agent.task_retry` | AgentManage | Failed/Cancelled/Skipped/Unknown task 재시작. `reset_downstream?`로 downstream도 Waiting으로 |
| `agent.task_graph` | AgentManage | DAG 출력. `format`=`json` (기본) 또는 `dot` (Graphviz) |
| `agent.task_run` | AgentManage | Workspace runner thread 시작/중단/상태. `action` ∈ {start, stop, status}. 응답: `{running, crashed, ready_count, running_count}`. Phase H.F |
| `agent.task_set_result` | AgentManage | 외부/수동 task 의 terminal 결과 보고. `state` ∈ {succeeded, failed}, `output?`/`error?`/`exit_code?`. Phase H.F |

### CLI

```
tasty agent task-create --workspace-id <id> --name <n> --command @spec.json [--depends-on T1,T2] [--on-failure abort|continue_downstream|fallback:T3] [--metadata @meta.json]
tasty agent task-list   --workspace-id <id> [--state <s>]
tasty agent task-get    --workspace-id <id> --id <T>
tasty agent task-await  --workspace-id <id> --id <T> [--timeout-ms <ms>]
tasty agent task-cancel --workspace-id <id> --id <T>
tasty agent task-retry  --workspace-id <id> --id <T> [--reset-downstream]
tasty agent task-graph  --workspace-id <id> [--format json|dot]
tasty agent task-run    --workspace-id <id> [--action start|stop|status]
tasty agent task-set-result --workspace-id <id> --id <T> --state succeeded|failed [--output @out.json] [--error <msg>] [--exit-code <n>]
```

`--command`/`--metadata`는 인라인 JSON 또는 `@path` (파일 로드). `--on-failure fallback:<task_id>`처럼 `kind`만 단축 표기.

본 sub-phase는 **state 머신 + 영속 + IPC/CLI 표면**만 책임진다. `Ready` task를 실제로 실행하는 스케줄러, blocking `task_await`, reducer 실행, lease/rate-limit는 후속 sub-phase에서 추가된다.

### Task runner (Phase H.F)

**executor 루프** — `Ready` task 를 자동 dispatch 하고 `Running` task 의 완료를 polling 으로 감지해 state 를 진행시키는 host 측 thread. workspace 1개당 1개, 500ms tick. 상세: [dev-guide/agent-runner.md](dev-guide/agent-runner.md).

- `TaskExecutor` trait (`tasty-agent::runner`) — pure 로직. `dispatch` (비차단) + `poll` (1tick) 두 메서드. host 가 `HostExecutor` 로 구현.
- `HostExecutor` (`src/core/agent/runner_host.rs`) — `ClaudeSpawn` → `claude.spawn` 동기 IPC + `claude.wait` polling. `Run` → `std::process::Command` + `try_wait`. `Custom` → host IPC 동기 dispatch. `Reduce` → 즉시 collect + `reduce_with_custom`.
- `RunnerRegistry` (`src/core/agent/runner_thread.rs`) — workspace 별 thread 의 start/stop/status. 중복 start no-op (idempotent). panic 시 `catch_unwind` 흡수 + crashed 마킹.
- `HostIpcInjector` (`src/app/ipc/host_call.rs`) — off-main thread 가 plugin IPC 메서드를 동기 호출하는 통로. `IpcCommand` 를 App 큐에 직접 push + `IpcWaker` 깨움 + `sync_channel(1)` `recv_timeout(5s)`.
- `crates/tasty-agent/src/platform/process_alive.rs` — cross-platform pid liveness probe (Unix `kill(pid, 0)` / Windows `OpenProcess + GetExitCodeProcess`).

Phase H.F 시점에는 handle 이 runner thread 메모리에만 — Phase J.A 에서 영속 + restart reload 로 진화.

**Phase J.A — runner 완성**:
- **DispatchHandle 영속** (`tasty.agent.handle.<task_id>`): Started 직후 영속,
  release_permit 시 evict. 호스트 재시작 시 `reload_persistent_handles` 가
  pid liveness 검사 (`process_alive::is_alive`) — alive 복원 / dead Failed 마감.
- **Lease-gated dispatch**: `task.metadata.lease = {resource, holder?, ttl_ms?, mode?}`
  컨벤션. dispatch 게이트 순서 lease → semaphore (R-4 dead-lock 회피).
- **`OnFailure::Fallback { inline }` 동적 task 생성**: main Failed 시
  `InlineFallbackSpec` 으로 새 task 발급. `metadata.fallback_of` 로 idempotency.
- **`agent.task_await` 진짜 blocking**: `TaskWakerHub` (sync_channel + waiters
  HashMap + recv_timeout). set_state 종결 분기 / Core wrapper / runner thread tick
  의 모든 경로에서 fire.

**Phase K.A — runner 잔여 한계 fix**:
- **`ShellProcess` exit_code 정확 복원**: `HostExecutor::dispatch` 가 Run 자식 spawn
  직후 watcher thread 를 띄워 `child.wait()` 종료 status 를
  `tasty.agent.run_result.<task_id>` 영속 + shared cell 양쪽에 기록. poll 은 cell
  만 조회 (try_wait 우회), reload 의 dead pid 분기는 영속을 조회해 exit_code 까지
  정확히 Succeeded / Failed 마감 (`precise` 분기). host 가 spawn 과 watcher 의
  영속 완료 사이에 죽으면 손실 — cross-platform 으로 회피 불가.
- **`ClaudeChild` reload injector grace**: poll 첫 dispatch 실패가 injector 미초기화
  사유면 deadline = now + `INJECTOR_GRACE_MS (30s)` 세팅 → 도래 전까지는 `Active`
  로 흡수, 도래 후이면 `Failed("injector grace expired")`. injector 외 Err (timeout
  등) 는 기존대로 즉시 Failed. 정상 dispatch 1회 성공 시 deadline reset.

**Semaphore-gated dispatch + WaitBarrier task (Phase I.A)** — `TaskExecutor::dispatch` 가 `DispatchOutcome::{Started, Deferred, PermanentFail}` 3-way 결과를 반환. `task.metadata.semaphore = { name, holder? }` 컨벤션이 있으면 dispatch 진입에서 `SemaphoreStore::acquire` 시도, 부족 시 `Deferred` 로 다음 tick 재시도. permit 회수는 종결 (Succeeded/Failed/Cancelled) 시 자동. 추가로 `TaskCommand::WaitBarrier { name }` 로 DAG 안에서 명시적 barrier gate 가능 — barrier `Closed` → Succeeded, `TimedOut` → Failed. 호스트 재시작 시 영속된 holder 의 leak 방지를 위해 workspace runner thread 시작 직전 `holder == task.id` 컨벤션이 맞는 Running 잔여 task 의 permit 정화 + Failed("host restart") 마감 단계 1 회. `metadata.semaphore.holder` 가 다르면 *외부 도구가 직접 acquire 한 항목* 으로 간주, 정화 대상 아님. 상세: [dev-guide/agent-runner-primitives.md](dev-guide/agent-runner-primitives.md).

### Barrier / Semaphore primitive (Phase 5.2)

**poll-based 동기화 게이트와 자원 점유** — `tasty-agent` 크레이트에 `Barrier`/`BarrierState`/`BarrierStore`, `Semaphore`/`AcquireOutcome`/`ReleaseOutcome`/`SemaphoreStore` 정의. 영속 키는 각각 `tasty.agent.barrier.<name>` / `tasty.agent.semaphore.<name>`, scope = `workspace:<id>`.

- **Barrier**: N개 신호가 모일 때까지 기다리는 게이트. 상태 `Open → Closed` (count 충족) 또는 `Open → TimedOut` (timeout 경과). 도장 찍기는 lazy — `signal` / `state` / `list(now_ms)` 호출 시점에 timeout 검사. 별도 스레드/타이머 없음.
- **Semaphore**: N permit 까지 동시 점유 허용. 같은 holder의 재acquire는 idempotent 성공 (retry-safe). `acquired:false` 응답으로 polling, permit 회복은 `release` (지정 holder가 점유 중일 때만).

이 단계도 **poll-based**다. 호출자가 `*_await`를 반복 호출하며 polling 한다. blocking + queue/wakeup은 scheduler 도입 후 추가.

#### IPC

| method | 권한 | 동작 |
|---|---|---|
| `agent.barrier_create` | AgentManage | `workspace_id`/`name`/`count_required≥1`/`timeout_ms?`. 이름 중복은 `-32602` |
| `agent.barrier_signal` | AgentManage | count_signaled++. 도달 시 `Closed`, timeout 경과 시 `TimedOut` + 거부 |
| `agent.barrier_await` | AgentManage | 현 단계: `barrier_state`와 동일 (즉시 응답) |
| `agent.barrier_state` | AgentManage | 현 상태 (조회 시점에 timeout 도장 적용) |
| `agent.barrier_list` | AgentManage | `{ total, barriers: [...] }`. 조회 시점에 timeout 도장 적용 |
| `agent.barrier_delete` | AgentManage | barrier 삭제. 존재하지 않으면 no-op |
| `agent.semaphore_create` | AgentManage | `workspace_id`/`name`/`permits≥1` |
| `agent.semaphore_acquire` | AgentManage | `{ acquired, semaphore }`. 동일 holder는 idempotent |
| `agent.semaphore_release` | AgentManage | `{ released, semaphore }`. 점유 중이 아니면 no-op |
| `agent.semaphore_list` | AgentManage | `{ total, semaphores: [...] }` |
| `agent.semaphore_delete` | AgentManage | semaphore 삭제. 존재하지 않으면 no-op |

#### CLI

```
tasty agent barrier-create   --workspace-id <id> --name <n> --count-required <N> [--timeout-ms <ms>]
tasty agent barrier-signal   --workspace-id <id> --name <n>
tasty agent barrier-await    --workspace-id <id> --name <n>
tasty agent barrier-state    --workspace-id <id> --name <n>
tasty agent barrier-list     --workspace-id <id>
tasty agent barrier-delete   --workspace-id <id> --name <n>
tasty agent semaphore-create --workspace-id <id> --name <n> --permits <N>
tasty agent semaphore-acquire --workspace-id <id> --name <n> --holder <h>
tasty agent semaphore-release --workspace-id <id> --name <n> --holder <h>
tasty agent semaphore-list   --workspace-id <id>
tasty agent semaphore-delete --workspace-id <id> --name <n>
```

### Lease primitive (Phase 5.3)

**협조적(advisory) 자원 점유 + TTL** — `tasty-agent::lease`. 다중 에이전트가 임의 resource(예: `file:/path`, `workspace:foo`)의 점유 상태를 공유하기 위한 마커. OS 락이 아니므로 lease 를 무시한 채 resource 를 만지는 행위 자체는 막지 못한다. 영속 키는 `tasty.agent.lease.<encoded-resource>`, scope = `workspace:<id>`. resource 문자열은 memory 키 허용 문자(`[a-z0-9._-]`)로 escape 되어 저장 (디코딩 불필요 — 원본은 JSON 에 같이 저장).

- **상태**: `{ workspace_id, resource, holder, acquired_at, expires_at? }`. `ttl_ms` 가 있으면 `expires_at = acquired_at + ttl_ms`. 만료 lease 는 다음 `acquire` 또는 `list` 호출 시점에 lazy 하게 evict
- **모드**: `fail` (기본 — 충돌 시 `-32009 lease_conflict` 즉시 실패) / `block` (충돌 시 `acquired:false` 반환, 호출자가 polling)
- **점유 규칙**: 같은 holder 재acquire 는 idempotent 갱신 (TTL 재설정). release 는 점유 holder 만 가능 — 다른 holder 호출은 no-op
- **한계**: 협조적 마커이므로 lease 를 보지 않는 외부 프로세스의 접근은 차단되지 않는다. 진정한 락이 필요하면 OS flock/fcntl 을 별도로 써야 한다

#### IPC

| method | 권한 | 동작 |
|---|---|---|
| `agent.lease_acquire` | AgentManage | `workspace_id`/`resource`/`holder`/`ttl_ms?`/`mode?`. 충돌 + `fail` 시 `-32009 lease_conflict`, `block` 시 `acquired:false` |
| `agent.lease_release` | AgentManage | `{ released, lease? }`. 점유 중이 아니면 no-op |
| `agent.lease_list` | AgentManage | `{ total, leases: [...] }`. 만료 lease 자동 evict |

#### CLI

```
tasty agent lease-acquire --workspace-id <id> --resource <r> --holder <h> [--ttl-ms <ms>] [--mode fail|block]
tasty agent lease-release --workspace-id <id> --resource <r> --holder <h>
tasty agent lease-list    --workspace-id <id>
```

### Reducer (Phase 5.4)

**N개 task 의 결과를 단일 값으로 합성** — `tasty-agent::reducer` 모듈에 4종 in-process 전략 + 1종 host-bridged 전략(`custom`). 본 단계는 동기적으로 동작 — 입력 task 가 아직 끝나지 않았으면 `output` 은 `null` 로 들어간다 (완료 보장은 호출자 책임).

| 전략 | 동작 |
|---|---|
| `first_success` | 첫 `Succeeded` task 의 `output`. 성공한 입력이 없으면 `-32602` |
| `all` | 모든 입력 `output` 을 순서대로 JSON 배열로 (상태 무관) |
| `merge_json` | 모든 입력 `output` (JSON object) 을 left-to-right deep merge. non-object 는 거부 |
| `concat_text` | 모든 입력 `output` 을 텍스트로 이어 붙임 |
| `custom` | 호스트 shell (`sh -c` / `cmd /C`) 로 명령 실행, stdin 에 `[output1, output2, ...]` JSON 배열, stdout 이 결과 (JSON 시도 → 실패 시 string) |

#### IPC

| method | 권한 | 동작 |
|---|---|---|
| `agent.task_reduce` | AgentManage | `workspace_id`/`inputs: [TaskId]`/`strategy: { kind, command? }` → `{ value }`. 입력 task 부재는 `-32004` |

#### CLI

```
tasty agent task-reduce --workspace-id <id> --inputs T1,T2,T3 --strategy first_success|all|merge_json|concat_text|custom:<command>
```

### Rate limit (Phase 5.5)

**시간당 비율 제한 (token bucket)** — `tasty-agent::rate_limit`. (agent, metric) 쌍에 대해 `limit` 토큰 / `per_ms` 윈도우의 보충률로 차감-기반 제한을 건다. 영속 키는 `tasty.agent.rate_limit.<id>`, scope = `Global` (워크스페이스 무관 — agent 전역 비율).

**`telemetry.cap` (04) 과의 차이:**

| 시스템 | 의미 | 차단 시점 |
|---|---|---|
| `telemetry.cap` | 누적 임계 (예: `input_tokens` 총합 ≥ 100000) | 합산값이 임계 도달 시 |
| `agent.rate_limit` | 시간당 비율 (예: `ipc_calls` 100/분) | 윈도우 내 토큰 소진 시 |

`burst` 가 비면 `burst = limit` 으로 기본값을 채워 burst-허용 없이 즉시 윈도우 동등 차감으로 동작. 등록되지 않은 (agent, metric) 쌍에 대한 `try_consume` 은 항상 허용 (rate_limit 미적용 = throttle 안 함).

**IPC dispatcher 미들웨어 (Phase I.A)** — 모든 비-Local / 비-`_host` IPC 호출은 dispatcher 진입에서 (agent, `ipc_calls`) 1 차감을 자동 시도한다. throttle 차단 시 `-32010 throttled` 응답 + audit Deny. 면제: `telemetry.*` (재귀 폭주), `agent.rate_limit_*` (자가 회복 경로 — 차단 시 throttle 된 agent 가 영구 차단됨), `system.info`. 정책상 throttled 분기는 `record_ipc_call` 을 건너뛰므로 `ipc_calls` 텔레메트리 이벤트로 카운트되지 않는다. throttle 폭주 추적은 `RateLimit.throttled_count` 가 담당.

#### IPC

| method | 권한 | 동작 |
|---|---|---|
| `agent.rate_limit_set` | AgentManage | `agent`/`metric`/`limit`/`per_ms`/`burst?` upsert. 동일 (agent, metric) 은 같은 id 유지하며 버킷 reset |
| `agent.rate_limit_list` | AgentManage | 전체 버킷 (refill 적용 후) `{ total, rate_limits: [...] }` |
| `agent.rate_limit_remove` | AgentManage | `id` 로 삭제 |
| `agent.rate_limit_status` | AgentManage | `agent?` / `metric?` 필터로 현재 상태 조회 |

#### CLI

```
tasty agent rate-limit-set    --agent <id> --metric <name> --limit <n> --per-ms <ms> [--burst <n>]
tasty agent rate-limit-list
tasty agent rate-limit-remove --id <rate-limit-id>
tasty agent rate-limit-status [--agent <id>] [--metric <name>]
```

### 실패 처리 / Retry (Phase 5.6)

**3 가지 OnFailure 정책 + retry 의 downstream 리셋 옵션**:

| 정책 | 메인 task 실패 시 동작 |
|---|---|
| `Abort` (기본) | downstream 전부 `Skipped` 로 cascade |
| `ContinueDownstream` | 실패를 성공처럼 취급 — downstream 은 정상 `Ready` |
| `Fallback { task }` | main 실패 시 fallback task 가 자동 `Ready`. fallback 이 `Succeed` 하면 main 의 downstream 도 `Ready`, fallback 도 실패하면 downstream `Skipped`. main 의 downstream 은 fallback 결과 나올 때까지 `Waiting` 유지 |

`agent.task_retry { id, reset_downstream? }` — `Failed` / `Cancelled` / `Skipped` / `Unknown` 상태의 task 를 `Waiting` 으로 되돌려 dep 평가로 자동 재진행. `reset_downstream=true` 면 transitive downstream 중 `Skipped` / `Failed` / `Cancelled` 도 `Waiting` 으로 되돌려 한 번에 재시도 가능.

## 공유 컨텍스트 (Phase 7)

여러 에이전트가 같은 워크스페이스 상태를 보고/갱신하기 위한 표면 3 종. 모두 일반 `memory.*` 영역 위의 얇은 래퍼 — 별도 저장소가 아니라 키 컨벤션 + 검증을 묶어둔 것이다. 권한은 `memory.write` / `memory.read` 를 그대로 재사용하고, 변경은 일반 `memory.changed` 이벤트로 발화된다. 신규 에러 코드 `-32009 already_exists`.

상세 가이드: [agent-guide/blackboard.md](agent-guide/blackboard.md), [agent-guide/plan.md](agent-guide/plan.md), [agent-guide/cache.md](agent-guide/cache.md).

### Blackboard (Phase 7.1)

워크스페이스 단위 명명된 키-값 컬렉션. 키 컨벤션 `tasty.bb.<name>._meta` + `tasty.bb.<name>.fields.<field>`. `_meta.schema` 는 임의 JSON 으로 보관 (호스트는 검증하지 않음 — 호출자가 직접).

- IPC 9 종: `memory.bb_{create,put,get,get_all,get_meta,delete_field,delete,list,exists}`. 모두 `workspace_id` 필수.
- 이름 규칙: name/field 1..=64 자 `[a-z0-9_-]+`. `bb_put` 은 `_meta` 가 없으면 `-32004`.
- owner 규칙: 일반 memory 와 동일 — caller 가 만든 entry 만 caller 가 수정, `_host` 는 root.
- CLI: `tasty memory bb {create,put,get,get-all,get-meta,delete-field,delete,list,exists}`.

### Plan (Phase 7.2)

워크스페이스 단위 선언적 work breakdown. 한 plan = `tasty.plan.<plan_id>` 단일 JSON entry (step 1 개 갱신도 plan 전체 JSON put 1 회).

- IPC 7 종: `memory.plan_{create,get,list,delete,add_step,remove_step,update_step}`.
- `PlanStepState` 5 종: `pending` / `in_progress` / `completed` / `failed` / `skipped`.
- 검증: flat step 수 ≤ 256, step id 중복 금지, `depends_on` ref 유효성 + 자기 의존/사이클 금지 (DFS).
- `update_step` 의 notes 3 분기: `notes` (set) / `clear_notes:true` (해제) / 둘 다 없음 (유지).
- CLI: `tasty memory plan {create,get,list,delete,add-step,remove-step,update-step}`.
- JSON Schema: [agent-guide/plan.schema.json](agent-guide/plan.schema.json).

**`agent.task_*` 와의 구분**: `agent.task_*` (Phase 5.1) 는 실행기 (스케줄러가 `ready → running → done` 진행), `memory.plan_*` 는 상태 기록 — 호출자가 명시적으로 update-step 호출. 같은 워크스페이스에서 둘을 함께 써도 무방.

### Cache (Phase 7.3)

워크스페이스 단위 TTL 캐시. 키 prefix `tasty.cache.<key>` + 필수 양수 `ttl_secs` 규약.

- IPC 5 종: `memory.cache_{put,get,invalidate,clear,list}`.
- `cache_get` 의 만료/미존재는 둘 다 `null` — 호출자가 구분 불필요.
- `cache_invalidate` 는 idempotent (없어도 성공). `cache_clear` 는 caller 가 수정권 있는 entry 만.
- CLI: `tasty memory cache {put,get,invalidate,clear,list}`.

### Snapshot / Restore (Phase 7.4)

bb 의 한 시점을 통째로 캡처해 복원. 키 컨벤션 `tasty.bb.<name>.snapshots.<sid>` (sid 1..=64 자 `[a-z0-9_-]+`).

- IPC 5 종: `memory.bb_snapshot{,_get,_list,_delete,_restore}`.
- 페이로드: `BlackboardSnapshot { bb_name, snapshot_id, taken_at, taken_by, meta?, fields[] }`. `fields[].payload` 는 `content_type` 별로 평탄화 (text → string / json → value / binary → base64 string) — `MemoryValue` 의 internally-tagged enum 이 `serde_json::to_value` 경로에서 깨지는 문제를 피함.
- restore 동작: 현재 field 를 모두 지운 뒤 snapshot 의 field 를 **현재 caller** 가 owner 로 다시 put. 원래 owner 정보는 복원되지 않는다. bb 가 (수동 삭제 등으로) 사라졌으면 snapshot 의 `meta` 로 재생성.
- `bb_delete` 는 fields + snapshots + meta 모두 삭제. snapshot 보존이 필요하면 외부에서 `bb_snapshot_get` 으로 복사.
- CLI: `tasty memory bb {snapshot,snapshot-get,snapshot-list,snapshot-delete,snapshot-restore}`.

## 설정 시스템

### TOML 기반 설정 파일
- 설정 파일 경로: `~/.tasty/config.toml` (전 플랫폼 통일)
- `directories` 크레이트로 플랫폼별 홈 디렉토리 추상화
- `toml` + `serde` 기반 직렬화/역직렬화
- 설정 파일이 없거나 파싱 실패 시 기본값으로 폴백

### 설정 카테고리
- **General**: 레이아웃 저장/복원 (기본 off): 체크 시 워크스페이스/페인/탭/서피스 구조를 `~/.tasty/layout.json`에 저장하고 다음 시작 시 복원. 마지막 윈도우 닫기 동작 (ask / minimize / quit).
- **Terminal**: 셸 경로 (OS별 자동 감지: COMSPEC/SHELL), 셸 모드 (default / tasty / custom — tasty 모드는 `~/.tasty/bashrc`를 source하여 OSC 7 등의 빌트인 설정을 적용. 기존 설정 파일의 `"fast"`는 unknown 값으로 간주되어 default로 fallback), 시작 명령, 스크롤백 줄 수 (기본 10,000), 실행 중 프로세스 닫기 확인, 작업 디렉토리 상속 (기본 on), 링크 클릭 수식키 (ctrl / alt / none). 데이터는 여전히 `settings.general.*`에 저장되며 UI 탭만 분리되어 있다.
- **Appearance**: 폰트 패밀리 (기본값: 시스템 모노스페이스), 폰트 크기, 테마 (dark/light), 배경 투명도, 사이드바 너비, focused surface 배경색, Font DPI 스케일링 모드 (auto: 모니터 DPI에 맞춰 동일 물리 크기 유지 / fixed: 픽셀 고정, 기본값)
- **Clipboard**: OS별 기본 활성화 (macOS: Alt+C/V, Linux: Ctrl+Shift+C/V, Windows: Ctrl+C/V)
- **Notifications**: 알림 활성화, 시스템 알림, 사운드, 병합 간격(ms)
- **Keybindings**: 서브탭으로 분류된 단축키 설정 (General / Workspace / Pane / Tab / Surface / Clipboard / Zoom / Preset). 유비쿼터스 언어 계층 구조(Workspace → Pane → Tab → Surface) 순서. 각 서브탭 내부 항목은 생성/분할 → 탐색 → 수정 → 닫기 순서로 정렬
  - 중복 바인딩 방지: 녹화한 조합이 다른 액션에 이미 할당되어 있으면 확인 팝업 표시. Enter/Y/Overwrite 수락 시 기존 바인딩을 비우고 새 필드에 적용, Esc/N/Cancel 취소 시 값 변경 없음. 팝업이 열린 동안 녹화 버튼은 비활성화됨.
  - **Preset 서브탭**: 좌측에 프리셋 목록, 우측에 미리보기 패널 (3열 테이블 — 기능 / 이전 / 이후). 변경되는 행은 bold 강조. 하단 "적용" 버튼으로 Draft에 반영 (실제 저장은 하단 Save 버튼). Draft가 이미 프리셋과 동일하면 적용 버튼 비활성화.
- **Performance**: targeted PTY polling, scrollback disk swap, lazy PTY init (background 탭 생성 시 PTY를 즉시 spawn하지 않고 최초 접근 시점에 spawn — 레이아웃 복원으로 만들어진 비활성 워크스페이스의 deferred 터미널은 사용자가 워크스페이스를 전환하거나, 에이전트가 send/`surface.wake` IPC로 접근하는 시점에 PTY가 자동 생성된다. `surface.list`/`tree` 결과의 `pty_ready` 필드로 현재 상태를 확인할 수 있다)
- **Misc (기타)**: 좌측 서브탭 메뉴 + 우측 콘텐츠 구조 (Keybindings 탭과 동일한 레이아웃, 향후 서브탭 확장 대비).
  - **tastyrc 서브탭**: Tasty 모드 bashrc 편집기. 사용자 편집분은 `~/.tasty/bashrc.user`에 저장되고, 빌트인 블록(OSC 7 emission / UTF-8 / PATH)은 코드 상수로 유지되어 Save 시마다 `~/.tasty/bashrc`가 `builtin + user` 형태로 자동 재생성된다. 이로써 빌트인 템플릿이 업데이트되면 기존 사용자에게도 즉시 반영된다. Reset 버튼으로 user 파트를 초기 기본값으로 되돌릴 수 있다.

### GUI 설정 윈도우
- Ctrl+, 단축키로 설정 윈도우 토글
- egui Window 기반 탭 인터페이스 (General / Terminal / Appearance / Clipboard / Notifications / Keybindings / Language / Performance / Accessibility / FileHandler / Misc)
- egui에 시스템 CJK 폰트 로드: Windows(맑은 고딕), macOS(AppleSDGothicNeo), Linux(Noto Sans CJK)
- 편집 중 원본 설정을 보존하는 드래프트 패턴
- Save 버튼: 디스크에 저장 후 즉시 적용
- Cancel 버튼: 변경 사항 폐기

### 설정 로드/저장
- `Settings::load()`: 설정 파일 로드, 없으면 기본값 반환
- `Settings::save()`: 설정 디렉토리 자동 생성 후 TOML 형식으로 저장
- `Settings::config_path()`: 플랫폼 독립적 설정 파일 경로 반환
- `Settings::normalize()`: enum-like 필드(`appearance.ui_scale`, `appearance.*_font.font_scale_mode`, `general.shell_mode`, `general.close_behavior`, `general.link_click_modifier`)에 알려진 값 외의 문자열이 있으면 안전한 기본값으로 치환하고 `NormalizeReport`를 반환. `appearance.theme` 은 legacy id 매핑(`catppuccin-mocha` → `mocha`, `catppuccin-latte` → `latte`)만 수행 — 실제 valid 검증/fallback 은 부팅 흐름의 `tasty_themes::apply_theme()` 가 담당한다. `general.language`는 사용자가 `~/.tasty/lang/{code}.toml` 로 임의 코드를 추가할 수 있으므로 정규화 대상에서 제외
- 부팅 경로(첫 윈도우 `init_app_state`, 새 윈도우 `create_new_window`, shell-setup 종료)에서는 `Settings::load()` 직후 `normalize()`를 호출하고 `report.changed`면 즉시 `save()`. 결과로 디스크의 invalid 값이 한 번에 정리되어, 다음 부팅·다음 윈도우에서 같은 popup·warning이 반복되지 않음
- 앱 시작 시 자동 로드, AppState에 통합

### 설정 연동
- `settings.general.shell`: Terminal 생성 시 커스텀 셸 경로 사용 (비어있으면 OS 기본 셸)
- `settings.general.startup_command`: 새로 생성되는 모든 터미널에 prompt 직후 1회 자동 실행할 명령 (`send_fast_init` 안에서 `tasty_mode_init_command` 다음에 전송). split/새 탭/새 워크스페이스/새 윈도우/레이아웃 복구 모두 적용. 공백·빈 문자열이면 전송 안 함. `surface.respawn_terminal` IPC는 `send_fast_init` 미호출 경로라 적용되지 않음 (플러그인 PTY 갈아끼우기 용도이므로 의도된 제외)
- `settings.appearance.default_font`: 기본 폰트 5종 묶음 (`font_family`, `font_size`, `custom_font_path`, `line_height`, `font_scale_mode`). Terminal·Markdown·Explorer 모두에 일괄 적용되며, 각 surface는 아래 override 그룹으로 항목별 재정의 가능. 설정 UI에서는 Theme 서브탭 하단의 "기본 폰트 설정" 섹션에서 편집
- `settings.appearance.terminal_font` / `markdown_font` / `explorer_font`: surface별 per-field override. 5개 필드 모두 `Option<T>`이며 `None`이면 `default_font`를 사용. 각 surface 서브탭에 "기본값 사용" 체크박스 + 입력 위젯 패턴으로 노출
- `font_family`: cosmic-text(터미널) 또는 egui FontDefinitions(Markdown/Explorer)에 전달. 빈 문자열이나 "monospace"이면 번들 D2Coding ligature를 사용. 다른 폰트를 지정해도 D2Coding은 폰트 DB에 남아 fallback face로 동작. 설정 UI에서 시스템 폰트 목록(번들 `D2Coding ligature` 포함)을 검색 가능한 드롭다운으로 선택
- `font_size`: 픽셀 단위. 기본값 14.0. 단축키 `Ctrl+/-/0`은 포커스된 surface(Terminal/Markdown/Explorer)의 `font_size` override만 변경하며, `Ctrl+0`은 override를 제거해 기본값으로 회귀
- `custom_font_path`: 커스텀 폰트 파일(.ttf/.otf) 경로. 지정 시 FontSystem 또는 egui FontDefinitions에 해당 파일을 추가 로드한 후 `font_family`로 참조 가능
- `line_height`: 행간 배수. 1.0(기본, 틈 없음 - ASCII 아트에 최적) ~ 2.0. 값이 클수록 행 간격이 넓어짐
- `font_scale_mode`: "auto"는 `font_size * scale_factor`(고DPI에서 동일 물리 크기 유지), "fixed"는 픽셀 크기 고정
- `settings.appearance.theme`: 현재 선택된 테마 id (= `~/.tasty/themes/<id>.toml` 의 파일명 stem). 빌트인은 `mocha`(기본 다크), `latte`(라이트). 사용자는 themes 폴더에 자유롭게 `*.toml` 추가 가능. 알려지지 않은 id 는 부팅 시 `tasty_themes::apply_theme()` 가 mocha 로 fallback 하고 InfoModal 로 사용자에게 알린다. 상세는 [docs/design/theme-system.md](design/theme-system.md), 사용자 가이드는 [docs/agent-guide/themes.md](agent-guide/themes.md)
- `settings.appearance.theme_base`: 누적된 테마 색상 풀 세트 (`ThemeColors`). 테마 변경 시 새 테마의 partial 이 이 위에 덮어쓰여진다 — 누락 필드는 보존되므로 partial 테마도 자연스럽게 적용
- `settings.appearance.theme_overrides`: 사용자가 픽커로 직접 손댄 색상 흔적 (`PartialColors`, 모든 필드 `Option`). 테마 변경 시 클리어
- `settings.appearance.theme_is_light`: 라이트/다크 플래그. `hover_overlay` / `active_overlay` / `separator` 같은 반투명 의미 색이 이 값에서 자동 도출됨
- `settings.appearance.background_opacity`: wgpu clear color의 알파 값으로 적용. 0.0(투명)~1.0(불투명)
- surface 종류별(focused/unfocused × bg/fg) 색은 `theme.surface_themes` map 에 들어있다. 빌트인 mocha 가 `"terminal"`, `"markdown"` entry 를 채우고, theme TOML 의 `[surfaces.<id>]` sub-table 로 사용자/plugin 이 추가 가능. 렌더러는 `theme().surface(id)` 로 접근하며 미정의 id 는 `FALLBACK_SURFACE` 로 안전하게 동작
- `settings.appearance.sidebar_width`: 사이드바 너비가 UI, GPU 렌더러, 터미널 rect 계산에 반영. 렌더 루프에서 설정값과 자동 동기화
- `settings.clipboard.history_enabled`: 클립보드 히스토리 기록 여부
- `settings.clipboard.history_max`: 히스토리 최대 항목 수 (기본 100)
- `settings.clipboard.poll_interval_ms`: 시스템 클립보드 폴링 주기(ms, 재시작 필요)
- `settings.keybindings.copy` / `settings.keybindings.paste`: 복사·붙여넣기 단축키 (다중 바인딩). 플랫폼별 기본값 — Windows: `ctrl+c` / `ctrl+v`, Linux: `ctrl+shift+c` / `ctrl+shift+v`, macOS: `alt+c` / `alt+v`
- `settings.keybindings.zoom_in` / `zoom_out` / `zoom_reset`: 줌 단축키 (다중 바인딩). 플랫폼별 기본값 — Windows/Linux: `ctrl+=` / `ctrl+-` / `ctrl+0`, macOS: `alt+=` / `alt+-` / `alt+0`
- `settings.notification.enabled`: 알림 활성화/비활성화. 비활성 시 알림 수집 및 시스템 알림 모두 차단
- `settings.notification.system_notification`: OS 네이티브 알림 개별 제어
- `settings.notification.coalesce_ms`: NotificationStore 생성 시 병합 간격 전달
- `settings.notification.sound`: true 일 때 신규 알림 발화 시 OS 기본 beep 1 회 재생 (Phase F.E — macOS `NSBeep` / Windows `MessageBeep(MB_OK)` / Linux `paplay → aplay → stderr \a` 3 단 폴백). headless 빌드는 `NoopPlayer` 로 대체. 상세는 "알림 사운드" 절 참조
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
  - **Block**: vi 복사 모드의 `Ctrl+v` 로 진입하는 사각형 선택 (마우스에서는 사용 안 함)
- 선택 영역 시각적 하이라이트 (배경색 오버라이드, Catppuccin Surface2 기반)
- 스크롤백 영역과 화면 영역을 넘나드는 선택 지원
- 전각 문자(CJK, 한글) 2셀 너비 올바르게 처리
- 마우스 트래킹 모드(1000/1002/1003) 활성 시 Shift+드래그로 강제 선택

### vi 스타일 키보드 복사 모드
- `Ctrl+Shift+Space` (기본 키, `enter_copy_mode` 액션) 로 진입. 진입 시 PTY 입력이 차단되고 커서 위치가 1셀 하이라이트로 표시됨
- 이동 키: `h`/`j`/`k`/`l` (좌/하/상/우), `w`/`b`/`e` (단어 점프), `0`/`$` (줄 시작/끝), `gg`/`G` (스크롤백 최상단/하단 — vim 과 동일한 double-key 시퀀스, 첫 `g` 후 다른 키를 누르면 시퀀스 취소), `H`/`M`/`L` (viewport top/middle/bottom)
- cursor cell 강조: 진입 직후 및 visual selection 활성 중에도 cursor 가 위치한 셀은 `theme.vi_cursor_bg` (Catppuccin lavender) 로 selection 보다 진한 톤으로 강조되어 식별 가능
- count prefix 지원: `3w`, `5j`, `10l` 등 (6자리 cap)
- visual 선택: `v` (문자), `V` (줄 전체), `Ctrl+v` (사각형 블록)
- `y` 로 클립보드 복사 + 모드 종료, `q` 또는 `Esc` 로 종료 (visual 중이면 visual 만 해제, 한 번 더 누르면 모드 종료)
- 검색: `/` 또는 `?` 로 mini-prompt 활성 → 텍스트 입력 후 `Enter` 로 commit. `n`/`N` 으로 다음/이전 매치. 검색 결과는 기존 `SearchState` 와 동일한 하이라이트
- viewport 자동 정렬: cursor 가 화면 밖이면 스크롤이 자동으로 따라감
- 마우스 좌클릭 시 모드 자동 종료. alt-screen 앱 (vim/tmux) 실행 중이면 진입 차단 + toast 안내
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
- `EngineState.clipboard_history`(메모리 전용)에 시스템 클립보드 변경 기록 (호스트가 소유)
- 별도 스레드가 `settings.clipboard.poll_interval_ms`(기본 500ms) 주기로 `AppEvent::ClipboardTick` 발송 → 메인 스레드가 arboard로 현재 값을 읽어 모든 Window의 history에 기록
- 연속 중복 자동 제거 (`last_seen` 비교), 빈 문자열 무시
- 출처 태그: `ClipboardSource::System`(외부 앱) / `Internal`(Tasty 내부 복사). 내부 복사 지점(터미널 선택 복사, OSC 52)에서는 즉시 `record_internal_copy`로 기록
- 설정: `clipboard.history_enabled`(기본 on), `history_max`(기본 100), `poll_interval_ms`(재시작 필요)
- 주의: 비밀번호 관리자 등 민감 정보도 기록된다. OS 레벨 민감 플래그를 구분할 수단이 제한적이라 1차는 필터 없음
- 재시작 시 휘발(디스크 영속화는 별도 TODO)
- 사용자 viewer 는 빌트인 `com.tasty.clipboard-history` plugin 이 popup 으로 제공 (단축키 `toggle_clipboard_viewer`, 기본 `Ctrl+Shift+H`). plugin 은 `tool.clipboard.list` 로 호스트 history 를 읽어 표시하고 다음 액션을 노출한다:
  - **항목 클릭** → `tool.clipboard.paste { index }` 호출 후 popup 자동 종료
  - **항목 옆 × 버튼** → `tool.clipboard.remove { index }` 호출 후 트리만 재렌더 (popup 유지)
  - **헤더 Clear all** → `tool.clipboard.clear {}` 호출 후 트리 재렌더
  - 이미 떠 있을 때 다시 토글하면 placeholder ("Clipboard viewer is already open") 만 표시 — outside-click / Esc 로 닫고 다시 열어야 한다

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
다음 메서드들은 `cfg(debug_assertions)` 게이트로 릴리즈 빌드에서 제외된다. 개발 및 테스트 용도로만 존재한다. release 빌드에서는 `src/ipc/method_meta.rs::DEBUG_METHODS`가 빈 슬라이스로 컴파일되어 `method_meta(name)` lookup이 `None`을 반환한다 (자세한 동작은 [dev-guide/debug-ipc.md](dev-guide/debug-ipc.md) 참조).

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
- `split`: 통합 분할 명령. `level`(pane/surface), `target_surface`(surface ID/nickname) 또는 `target_pane`(pane ID)으로 대상 지정, `direction`(vertical/horizontal), `type`(terminal/markdown/explorer + plugin contribute) 파라미터. pane/surface 레벨 모두 비터미널 타입 지원. 포커스 이동 없음
- `pane.close`: 패인 닫기 (unsplit)

#### 탭
- `tab.list`: 지정 패인의 탭 목록 (id, name, type, surface_id, active)
- `tab.create`: 지정 패인에 새 탭 추가
- `tab.close`: 탭 닫기
- `tab.move`: 탭 순서 이동 (`pane_id`, `from_index`, `to_index`)

#### 서피스
- `surface.list`: 전체 워크스페이스의 서피스 목록 (id, type, pane_id, tab_index, cols/rows). 비터미널 서피스(Markdown, Explorer, Html)도 포함
- `surface.close`: 서피스 닫기. cascade(surface → tab → pane → workspace)로 마지막 워크스페이스의 마지막 서피스까지 닫혀도 윈도우는 종료되지 않으며, 빈 워크스페이스가 새로 생성되어 invariant("열린 윈도우는 ≥1 workspace")가 유지된다 (에이전트가 사용자의 윈도우를 끄는 부작용 방지).
- `surface.close_self`: 호출한 서피스 자신을 닫기 (TASTY_SURFACE_ID 기반). `surface.close`와 동일한 cascade·자동 재생성 규칙 적용.

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

#### 에이전트 전용 (번들 plugin이 제공)
- `claude.launch`: Claude Code 전용 워크스페이스 생성 및 실행 — `com.tasty.claude` plugin이 등록
- `codex.launch`: Codex CLI 전용 워크스페이스 생성 및 실행 — `com.tasty.codex` plugin이 등록

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
- clap 기반 그룹형 서브커맨드: `new`, `close`, `list`, `set`, `send`, `read`, `unset`, `notify`, `surface-meta`, `is-typing`, `debug` (`claude`, `codex` 등은 번들 plugin이 자체적으로 등록)
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

### Claude Code 통합 (com.tasty.claude plugin)

Claude Code 워크스페이스 런처, parent-child 자식 관리, hook 통합 전체가 번들 plugin
`com.tasty.claude`로 이전되었다. 호스트는 plugin 등록만 처리하고, 모든 claude.*
IPC 메서드와 `tasty claude *` CLI 서브커맨드는 plugin이 자체적으로 노출한다.

#### Claude 워크스페이스 런처 (claude.launch)
- 새 워크스페이스 자동 생성 및 이름 설정
- 지정된 디렉토리로 이동 후 `claude` 명령 실행 (shell-escape로 인젝션 방지)
- `--task` 옵션으로 작업 설명 전달 가능 (shell-escape 적용)
- CLI: `tasty claude launch --workspace "my-project" --directory "/path/to/project" --task "Fix the bug"`
- IPC: `claude.launch` 메서드 (workspace, directory, task 파라미터)

#### Parent-Child 관계 관리
부모 Claude 인스턴스가 자식 Claude 인스턴스를 생성·관리하는 시스템. AI 에이전트가
멀티 에이전트 워크플로우를 구성할 때 사용한다.

- **자식 추적**: plugin 내부 state에서 surface ID, 인덱스, cwd, role, nickname을 보존
- **자동 정리**: 부모 또는 자식 surface가 닫힐 때 관계를 자동으로 정리. 부모가 먼저
  닫혀도 자식이 살아있는 동안 관계 유지 (ghost cleanup)
- **claude.spawn**: 두 가지 모드 지원:
  - **`--surface` 모드**: 대상 surface의 pane을 분할하여 새 터미널 생성 후 `claude` 명령 자동 실행
  - **`--workspace` 모드**: workspace를 지정하면 plugin이 spawn pane을 자동 관리. parent surface마다
    지정된 workspace 내에 전용 spawn pane을 갖고, 2×2 그리드 알고리즘으로 최적 배치
    (1→좌우분할→좌측상하분할→우측상하분할→새탭). 4개 초과 시 탭 확장
  - `--workspace`와 `--surface`는 동시 사용 불가
  - 부모(parent)는 항상 spawn 명령을 실행한 surface(`TASTY_SURFACE_ID`). cwd, role, nickname, prompt 파라미터 지원
- **claude.children / claude.parent / claude.kill / claude.respawn / claude.broadcast / claude.wait**:
  자식 목록 조회, 부모 역참조, 자식 종료, 자식 재시작(레이아웃 유지), 일괄 전송,
  상태 폴링(idle|needs_input|active|exited). CLI 동일.
- **claude.wait_any**: 여러 자식 중 *먼저* idle / needs_input / exited 가 되는 것을 즉시
  깨운다. 응답 JSON 에 `child_index` 키가 포함되어 어느 자식이 깨어났는지 알 수 있다.
  우선순위는 입력 children 순서 (동시 다수 terminal 시 결정적). timeout 도달 또는
  iteration 중 전원 active 인 tick 의 응답은 `{"state":"pending"}` (child_index 키 없음).
- CLI: `tasty claude spawn --direction vertical --cwd /path --role worker --nickname "agent-1" --prompt "Fix bugs"`
- CLI: `tasty claude children`, `tasty claude parent`, `tasty claude kill --child 1`, `tasty claude respawn --child 1`
- CLI: `tasty claude broadcast "text\r" [--role ROLE]`, `tasty claude wait --child 1 [--timeout SECS]`
- CLI: `tasty claude wait-any --children "1,2,3" [--timeout SECS]` — 첫 깨어난 자식의 응답을
  즉시 출력. `--timeout` 누락 시 무한 polling (`wait` 와 동일 정책).
- `--child`는 child index를 받는다 (spawn 시 반환되는 `child_index` 값)

#### Hook 통합
- **상태 추적**: plugin이 surface별 idle/needs_input 상태를 자체 관리
- **claude.hook**: stop, notification, session-end, subagent-stop, prompt-submit, session-start 이벤트를
  받아 상태 갱신 + host의 `surface.fire_hook` / `surface.meta.set` / `surface.meta.unset` 호출
- **상태 priority**: needs_input > idle > active. claude.children 응답의 `state` 필드에 반영
- **자동 정리**: surface가 닫힐 때 plugin state에서 자식·상태·error scan plate 함께 정리
- **HookEvent**: 호스트 측 `ClaudeIdle`/`NeedsInput`/`ClaudeChildIdle`/`ClaudeChildNeedsInput`/
  `ClaudeError` 이벤트 타입 유지 (`claude-idle`, `needs-input`, `claude-child-idle`,
  `claude-child-needs-input`, `claude-error` 키). plugin이 host의 fire_hook IPC로 발화
- **parent fan-out**: child Claude 에서 `stop` / `subagent-stop` / `session-end` 가 들어오면
  child surface 의 `claude-idle` 외에 *parent surface* 의 `claude-child-idle` 도 함께 fire 한다.
  `notification` 도 동일하게 parent surface 의 `claude-child-needs-input` 을 fire. parent 매핑이
  없는 (top-level) 호출에는 영향 없음. conductor 가 polling 없이 자식 완료를 감지하기 위한 채널.
- **ClaudeError 자동 감시**: `claude.spawn` / `claude.launch`로 만들어진 child surface는 PTY 출력에
  대한 패턴 스캐너가 plugin 안에서 자동 활성화된다. plugin이 `surface.read_since_mark`로 새
  출력 슬라이스를 받아 ANSI strip 후 catalog 정규식과 매칭하고, 매칭되면 `surface.fire_hook`으로
  `claude-error` 훅을 fire한다. 카탈로그(plugin 내장): `API Error`, `Output blocked by content
  filtering policy`, `overloaded_error`, `rate_limit_error`, `Bad Request`, `Internal Server Error`,
  `network error` (대소문자 무시). 일반 셸 surface는 영향 없음.
- **`tasty claude install` / `uninstall`**: `~/.claude/settings.json`의 `hooks` 객체에
  Stop/Notification/SessionEnd/SubagentStop 4종 entry를 idempotent하게 추가·제거한다. 각 entry의
  command는 `[ -n "$TASTY_SURFACE_ID" ] && tasty claude hook <token> || true` 형태로 tasty 외부에서
  claude를 실행할 때는 무해하게 통과한다.
- **wait의 사전 요구사항 점검**: `tasty claude wait`는 시작 시 Stop 훅 등록 여부를 확인하고
  미설치면 안내 메시지를 stderr에 출력하고 exit code 1로 종료한다.
- CLI: `tasty claude install`, `tasty claude uninstall`, `tasty claude hook stop|notification|session-end|subagent-stop|prompt-submit|session-start [--surface ID]`
- IPC: `claude.hook`, `claude.launch`, `claude.spawn`, `claude.children`, `claude.parent`, `claude.kill`,
  `claude.respawn`, `claude.broadcast`, `claude.wait`, `claude.wait_any` 메서드. 권한 토큰: `ipc.invoke:claude`

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

`com.tasty.image` 번들 plugin이 제공하는 image surface kind. plugin이 surface kind
등록(`rendering = "host"`)과 `image.*` IPC 네임스페이스를 점유하고, 픽셀 렌더링과
편집은 호스트 본문이 직접 담당한다. plugin이 비활성화되면 image surface 항목은
convert popup / pane context menu에서 사라진다.

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

`tasty image <sub>` CLI (plugin `[[contributes.cli]]`이 노출):

| 서브커맨드 | IPC 메서드 | 설명 |
|------------|------------|------|
| `tasty image open <path> --surface ID` | `image.open` | surface를 image kind로 변환하고 파일 로드 |
| `tasty image save --surface ID [--path PATH]` | `image.save` | 현재 image surface를 PNG로 저장 (path 생략 시 원본 경로의 `.png`) |
| `tasty image export <path> --surface ID` | `image.export_png` | 명시한 경로로 PNG 내보내기 |
| `tasty image next --surface ID` | `image.next` | 같은 폴더의 다음 이미지로 이동 |
| `tasty image prev --surface ID` | `image.prev` | 이전 이미지로 이동 |
| `tasty image paste --surface ID` | `image.paste` | 클립보드의 이미지를 floating selection으로 붙여넣기 |
| `tasty image list` | `image.list` | 열려 있는 모든 image surface의 ID + 경로 |

기존 호스트 CLI도 그대로 동작 (변환/생성 경로):
- `tasty split --type image --file <path>`: 이미지 뷰어로 분할
- `tasty split --type image`: 새 이미지 (빈 캔버스)
- `tasty new tab --pane ID --type image --file <path>`: 이미지 탭 생성
- `tasty new workspace --type image --file <path>`: 이미지 워크스페이스 생성
- Surface 타입 변환 팝업에서 Image 옵션 선택 가능

### 닫기/복원
- 이미지 탭 닫기 시 ClosedItem에 surface kind + snapshot이 저장됨 (generic 경로)
- Ctrl+Shift+T로 복원 시 surface registry의 image kind `restore`가 호출되어 같은 이미지를 다시 로드

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
- 검색 옵션 토글 3종 (검색 바 우측에 위치):
  - `Aa` 대소문자 구분 (기본 off)
  - `.*` 정규식 (기본 off, Rust `regex` 문법)
  - `ab` 단어 단위 일치 (기본 off, `\b` 경계. literal/regex 모두 적용)
- 정규식 컴파일 실패 시 상태 영역에 "Invalid regex" 빨간 메시지 표시
- 매치 카운터 표시 (예: 3/42)
- 매치 선택 시 해당 위치로 자동 스크롤
- 검색 바는 sticky_focus PopupDef로 구현: 키보드는 검색 바가 받고, 마우스는 터미널에 전달
- 검색 바는 `PopupScope::Surface(focused_surface_id)`로 열려 포커스된 surface 영역 상단(가로 중앙)에 anchor된다. 사이드바·탭 바 위에 떠 있지 않는다.

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
  - Terminal: cwd, `restore.command`(있을 때), `scrollback_ref`(설정 `restore_terminal_content` on 일 때)
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

- `tasty claude install`로 `SessionStart`/`SessionEnd` hook이 등록되면, plugin이 세션 시작 시
  `restore.command` (예: `claude -r <session-id>`) 및 `claude-session-id` 메타키를 surface 메타데이터에
  set/unset 한다. 호스트는 agent-agnostic하게 `restore.command` 값만 읽어 복원에 사용한다.
- Claude Code 의 hook 시스템은 hook 명령 실행 시 **stdin 으로 JSON payload**(`session_id`, `message` 등)를
  전달한다. `tasty claude hook` CLI 가 stdin JSON 을 자동으로 파싱해 `session_id` → `--session`,
  `message` → `--message` 로 채우므로, settings.json 에 등록되는 명령은 어떤 event 든 `tasty claude
  hook <event>` 형태로 단순하다. (옛 버전이 `--session ${CLAUDE_SESSION_ID}` 쉘 확장에 기대다 실패하던
  회귀가 있어, 재 `tasty claude install` 시 옛 entry 는 자동으로 갱신된다.)
- plugin이 비활성화되거나 `tasty claude install` 없이 사용해도 오류 없이 일반 셸로 복원됨
- 다른 agent plugin도 `restore.command` 메타키만 set하면 동일한 메커니즘으로 자체 세션 복원을 지원할 수 있음

복원이 발동하는 경로:
1. **앱 재시작 (레이아웃 복원)**: `restore_layout` 설정 활성화 시, 레이아웃 저장 시점에 세션 ID가 있는 터미널은 `restore_command`를 함께 저장. 복원 시 셸 초기화 후 자동 실행
2. **닫힌 항목 복원 (Ctrl+Shift+T)**: surface/tab/workspace 닫기 시 `ClosedSurface`에 `restore_command`를 포함하여 스냅샷. 복원 시 셸 시작 후 `restore_command` 자동 실행

명령 주입 타이밍은 PTY 가 spawn 되는 그 순간이다. `TerminalConfig.initial_input` 으로 `Terminal::new` 에 명령을 넘기면, writer thread 가 시작되기 전에 PTY master fd 에 동기적으로 write_all + flush 된다. 따라서 child shell 이 stdin 을 처음 read 하는 순간 무조건 이 바이트가 첫 입력으로 들어가, GUI redraw / BusyPoll / 워크스페이스 전환 등 추가 트리거 없이 spawn 과 동시에 실행된다. deferred 경로는 `DeferredSpawn.restore_command` 가 `ensure_initialized` 시 `initial_input` 으로 변환되어 동일하게 처리된다.

### 터미널 내용 복원 (scrollback)

설정 `general.restore_terminal_content` (기본 on) 가 활성화되어 있으면 레이아웃 저장 시 각 터미널의 scrollback (위로 스크롤 가능한 출력 히스토리) **과 현재 화면 (visible) 라인** 이 함께 보존되어, 앱 재시작 후에도 이전 출력을 그대로 볼 수 있다. 화면 라인은 scrollback 뒤에 이어 붙여 저장하므로 복원 후 위로 스크롤하면 [이전 scrollback → 이전 화면 → 새 prompt] 순으로 보인다 (trailing blank row 는 capture 시 trim).

- 저장 위치: `~/.tasty/scrollback/<persist_id>.bin` — `layout.json` 의 `Terminal.scrollback_ref` 가 이 파일을 가리킨다. 직렬화 포맷은 메모리 ↔ 디스크 swap 에 쓰이는 것과 동일 (magic `TSSB`, version 2, line-by-line records).
- `persist_id` 는 surface 가 처음 capture 될 때 발급되어 `surface-meta` (`scrollback.persist_id`) 에 보관된다. 다음 capture 가 같은 surface 면 같은 파일을 atomic 하게 덮어쓰므로 orphan 누적이 없다.
- 옵션 OFF: capture 단계에서 디스크 쓰기를 스킵하고, restore 단계에서도 `scrollback_ref` 가 있어도 무시한다.
- 옵션 ON → OFF 전환: Settings Save 시점에 `~/.tasty/scrollback/` 전체를 비운다 (사용자가 "더 이상 안 쓴다" 라고 명시한 상태).
- 비활성 워크스페이스 / 다른 탭의 deferred 터미널은 PTY 가 실제로 spawn 되는 시점 (`ensure_active_workspace_initialized` / `ensure_surface_initialized`) 에 큐에서 꺼내 inject 된다.
- 닫힌 항목 복원 (`Ctrl+Shift+T`) 의 scrollback 은 **옵션과 무관하게 항상** 복원된다 (메모리에 들고 있다가 즉시 재사용; 디스크 쓰기 없음).

#### Lifecycle / 정리

- Surface 닫힘: `surface-meta` 정리 직전에 `scrollback.persist_id` 를 회수해 `~/.tasty/scrollback/<id>.bin` 도 함께 삭제.
- 앱 시작: `layout.json` 의 모든 `scrollback_ref` 집합을 기준으로 디렉터리를 스캔해 알려지지 않은 `.bin` 파일을 일괄 삭제 (capture 도중 크래시 잔재 정리).
- 옵션 OFF 전환: 디렉터리 전체 삭제.
- Atomic rename (`<id>.bin.tmp` → `<id>.bin`) 로 부분 쓰기 잔재를 방지.

### 저장하지 않는 것
- 화면 내용 (visible cells) — scrollback 만 보존하며 현재 화면은 새 셸 프롬프트로 채워진다.
- PTY 상태, 환경변수, 실행 중인 명령
- 팝업 상태

## Plugin 시스템

외부 plugin 프로세스를 별도 OS 프로세스로 띄워 surface 종류를 확장한다.
릴리스 에셋의 `plugins.md` 참조.

### 기본 제공 plugin (built-in)
> 분류 정책 전체 (host-native / bundled / user 3 카테고리): [architecture/plugin-categories.md](architecture/plugin-categories.md). 본 절은 *카테고리 2 (bundled plugin)* 의 현재 구현 상태.

- Tasty 바이너리에 함께 묶여 배포되는 plugin은 첫 실행 시 `~/.tasty/plugins/<id>/`에 자동 설치된다 (`builtin::BUILTINS` 목록)
- 현재 기본 제공:
  - `com.tasty.explorer` (파일 탐색기 surface)
  - `com.tasty.claude` (Claude Code 통합 — `tasty claude launch|spawn|children|parent|broadcast|wait|kill|respawn|install|uninstall|hook`)
  - `com.tasty.codex` (Codex CLI 통합 — `tasty codex launch|spawn|children|parent|tell|wait|broadcast|kill|respawn|install|uninstall|hook`. claude plugin과 동일 사용법, `--surface` 미지정 시 호출자 surface의 `TASTY_SURFACE_ID` env로 fallback)
- 사용자가 plugin 메뉴에서 "제거"를 선택하면 `removed_builtins`에 기록되어 다음 실행에서 자동 재설치되지 않는다 — 외부 plugin과 완전히 동일한 라이프사이클 적용
- 번들 위치 탐색 순서: `TASTY_BUILTIN_PLUGINS_DIR` env > 실행 파일 옆 `plugins/` > dev 빌드 시 `target/<profile>/builtin-plugins/` (workspace 자동 부트스트랩, 등록된 모든 builtin 동기화)
- **권한 자동 복구**: builtin plugin이 사용자 디렉터리에는 있지만 `plugins.toml`에 grant 엔트리가 없는 경우(예: 이전 버전에서 builtin으로 인식되지 않은 채 외부 plugin처럼 설치됨), 부팅 시 매니페스트의 모든 권한을 자동 grant. `granted = []`로 명시 비워둔 경우는 entry가 있으니 건드리지 않음

### Plugin 관리 모달
- 사이드바 좌측 메뉴의 🧩 버튼으로 PluginsView 모달 진입 (Settings 모달과 동일 패턴)
- 상단 탭: **Installed**(설치된 플러그인 목록 + 상세) / **Add plugin**(외부 디렉터리에서 import)
- Installed 탭: 좌측 plugin 목록 + 우측 상세 — 이름/버전/설명/저자/홈페이지, 활성 토글, 등록 surface kinds, 매니페스트 권한 / grant 상태, 설치 경로(폴더 열기 버튼 포함), 로그 파일 경로
- 권한 grant/revoke 버튼으로 즉시 반영 (process 재시작 없이)
- "제거" 버튼은 사전 확인 다이얼로그를 거친 뒤 plugin 실행 종료 + 디스크 삭제. built-in plugin인 경우 추가 경고 표시
- Add plugin 탭: 경로 입력 + "확인" 버튼, 하단 "찾기" 버튼(rfd 네이티브 폴더 선택). 검증 시 매니페스트 정보(id/name/version/설명/권한/surface kinds/원본 경로) 미리보기 + 추가/취소. 이미 같은 id가 설치되어 있으면 추가 버튼 비활성화. 추가 시 `~/.tasty/plugins/<id>/`로 복사 + discovery 재실행 + 매니페스트 권한 자동 grant + auto-enable. 결과는 모달 윈도우 영역의 toast로 통지

### 매니페스트 + 디스커버리
- `~/.tasty/plugins/<id>/tasty-plugin.toml` 형식 (manifest_version=1, api_version=1)
- 부팅 시 자동 스캔, 매니페스트 검증 실패한 plugin은 warn 로그 후 스킵
- `~/.tasty/plugins.toml`로 활성/비활성 + `removed_builtins` 영속화

### 매니페스트 schema 확장 (F.H)
- `[surface_kinds.default_colors]` — plugin 이 자기 surface 의 권장 색
  (`focused_bg/fg`, `unfocused_bg/fg`) 을 직접 노출. hello 시점에 host 가
  `Theme.surface_themes` 에 머지하며, 사용자 theme TOML 의 `[surfaces.<kind>]`
  가 정의돼 있으면 *그쪽이 우선* (priority: 사용자 TOML > plugin default >
  FALLBACK_SURFACE). `crates/tasty-themes/src/plugin_defaults.rs` 가 누적 +
  user-defined 보호 invariant 유지.
- `[[contributes.window]]` + `permissions = ["window.spawn"]` — plugin 이
  OS-level 별도 윈도우를 contribute. 1.0 schema-only — host 가 hello 시
  `tracing::info!` 로그 + `plugin.window_declared` host event 발화. 실 spawn
  handler / multi-window 라우팅은 별도 영역.
- `crates/tasty-plugin-markdown/tasty-plugin.toml` 에 schema 사용 예시 +
  실 plugin binary. `BUILTINS` 등록 — 첫 부팅 시 자동 install 되며 markdown
  surface kind / detector / handler / cli 를 contribute (image plugin 과 동일
  host-rendered 패턴).
- `[[contributes.cli]]` 의 subcommand 엔트리에 `[polling]` 옵션 — `state_field`
  / `terminal_states` (예: `["idle", "needs_input", "exited"]`) / `interval_ms`
  (default 500) / `timeout_field`. 호스트가 plugin CLI 응답을 polling 하여
  terminal state 도달까지 *반복 IPC 호출* 을 manifest 기반으로 일반화. `tasty
  claude wait` 가 이 메커니즘 위에 동작 (Phase F 후속). `polling` 미설정 시는
  옛 *1 회 응답 + 즉시 종료* 동작.

### 프로세스 생명주기
- 호스트가 `127.0.0.1:0` 으로 listen, plugin이 token 들고 connect 하는 인증 방식
- stdout/stderr 자동 redirect → `~/.tasty/plugins-logs/<id>.log`
- 15초 ping / 60초 timeout 헬스체크, 비응답 시 자동 재시작
- 10초 내 spawn 실패 3회 시 자동 비활성화 (사용자 수동 enable까지 정지)
- 종료 시 모든 plugin에 graceful shutdown 송신 후 2초 timeout, 그 후 kill
- **개발용 자동 reload**: 환경변수 `TASTY_PLUGIN_AUTO_RELOAD` 가 비어있지 않고 `"0"` 도 아니면 활성. 매 pump tick 2초 간격으로 실행 중인 plugin 의 *entry binary mtime* + *manifest version* 을 baseline 과 비교, 변화 감지 시 `--restart-running` 과 동일한 graceful swap (`swap_shutdown_internal` → `swap_respawn_internal`) 으로 새 binary 부팅. `plugins.toml::disabled` 미수정. production 기본 off — flag off 시 polling cost 0. 상세: [`docs/dev-guide/plugin-ecosystem.md` §6.6](dev-guide/plugin-ecosystem.md)

### Surface 렌더링 (UI tree DSL)
- plugin이 JSON UI tree를 보내면 호스트가 egui로 렌더 (vbox/hbox/scroll/splitter/label/icon/button/tree/addressbar/text_preview/spacer)
- 호스트가 사용자 이벤트를 모아 `surface.event`로 plugin에 송신 (click/key/tree_*/addressbar_*/scroll/splitter_drag/focus_changed/resize)
- `RemoteSurface` 어댑터가 layout tree에 끼워지므로 본체 surface와 동등하게 split/tab 가능
- **Draggable splitter**: `UiNode::Splitter`의 `id: Option<String>`이 `Some`이면 호스트가 divider에 6px hit-test 영역 + 1px 중앙선을 그리고, 드래그하면 `UiEvent::SplitterDrag { node_id, ratio }`를 plugin에 송신. 부드러운 시각 피드백을 위해 egui memory에 사용자 ratio를 저장하며 plugin이 다른 값으로 응답 시 동기화. 양쪽 pane은 최소 40px 보장. SDK 헬퍼: `ui::splitter_id(id, dir, ratio, first, second)`
- **Canvas (픽셀 출력)**: `UiNode::Canvas { buffer_id, width, height, format, filter }` — plugin이 SharedBuffer에 RGBA 픽셀을 직접 쓰면 호스트가 wgpu 텍스처에 dirty rect만 부분 업로드해 egui로 합성. `tasty-shm` footer 8B `AtomicU64` generation으로 tear-free 동기화 (plugin Release-fetch_add → host Acquire-load). 포맷은 `Rgba8`/`Bgra8` (sRGB 해석), 샘플링 필터는 `Linear`/`Nearest`. `id`가 있으면 마우스 입력이 `UiEvent::CanvasPointer { node_id, x, y, phase, button }`로 라우팅 (phase: Move/Down/Up/Drag/Leave). SDK 헬퍼: `ui::canvas`, `ui::canvas_with_id`, `ui::canvas_full`

### 권한 모델
- 매니페스트의 `permissions = [...]`에 권한 토큰 (surface.read/write, notification, clipboard.read/write, fs.read/write, process.spawn, terminal.*, network, 그리고 `ipc.invoke:<plugin-prefix>`로 다른 plugin의 namespace 호출 권한) 선언
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
- `crates/tasty-plugin-sdk-wasm` (POC, main workspace 외부 격리): wasi-preview2 component 형식 plugin 의 host-side runtime. clipboard-history 변환 결과는 [architecture/wasm-poc-result.md](architecture/wasm-poc-result.md) 참조

### 동봉 plugin 예시
- `tasty-plugin-explorer`: 외부 binary로 작성된 파일 탐색기. SDK만 의존하며 호스트 코드 의존 없음
- 레이아웃: 상단 주소바 + ★+ 즐겨찾기 추가 버튼 / 좌측(트리+즐겨찾기 vertical split, drag로 비율 조절) ↔ 우측(미리보기) horizontal split, drag 가능. 비율은 `tree_ratio` / `left_inner_ratio` 두 splitter ID로 추적되어 layout snapshot에 영속화
- 즐겨찾기: `TASTY_PLUGIN_DATA_DIR/bookmarks.json`에 `{entries: [{name, path}]}` JSON으로 저장. 선택된 항목이 있고 미등록 상태일 때만 ★+ 버튼 활성. 즐겨찾기 항목 클릭 시 디렉터리면 root 이동, 파일이면 select+preview, ✕ 버튼으로 삭제
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

### Plugin CLI / IPC namespace
- 매니페스트 `[[contributes.cli]]` + `[[contributes.ipc_namespace]]`로 plugin이 자기 CLI 서브커맨드와 IPC prefix를 선언하면, `tasty <name> <sub>` 명령과 `<prefix>.<method>` IPC가 plugin에 직접 라우팅된다
- 호스트 CLI는 부팅 시 `~/.tasty/plugins/*/tasty-plugin.toml`을 스캔하여 매니페스트 기반 clap 서브커맨드를 정적 명령에 이어 동적으로 합친다. 정적 명령이 항상 우선 매칭되어 plugin이 호스트 명령을 가릴 수 없다
- CLI 인자는 매니페스트의 `arg_groups` 스키마대로 JSON-RPC params 객체로 직렬화되어 plugin에 전달된다
- IPC dispatcher는 정적 라우팅에 매칭되지 않은 메서드를 namespace registry에서 lookup하여 owner plugin에 `ipc.invoke` 메시지로 forward한다. 응답은 비동기로 도착하며, 호스트가 client 응답 채널에 연결해둔다
- 예약 prefix(`system`, `surface`, `tab`, `pane`, `workspace`, `plugin`, `hook`, `global_hook`, `message`, `tool`, `notification`, `window`, `debug`, `ui`, `ime`, `ipc`, `split`, `tree`)는 사용 불가. 같은 prefix를 두 plugin이 동시에 선언하면 나중에 로드된 쪽은 거부 (참고: `claude`/`codex` 등 번들 plugin이 점유한 prefix는 외부 plugin이 중복 선언 시 동일하게 거부됨)
- 다른 plugin namespace 호출은 `ipc.invoke:<prefix>` 권한이 필요하다. 자기 자신을 호출하는 무한 forward는 호스트가 `-32001`로 거부
- SDK: plugin은 `Plugin::handle_ipc_method(IpcMethodCtx) -> Result<Value, IpcMethodError>` 콜백 하나로 namespace 전체 dispatch를 처리. `ctx.caller_plugin_id`로 호출자가 plugin인지 사용자(CLI/IPC)인지 구분

### Plugin Surface lifecycle (Event Bus)
- 매니페스트 `event_subscribe = ["surface.closed"]`로 구독한 plugin은 다른 surface가 닫혔을 때 Event Bus를 통해 `surface.closed` envelope을 받는다
- 알림 payload: `{ surface_id, kind, reason }`. reason은 `"user"`(PTY 종료/단축키/탭 우클릭) / `"ipc"`(IPC `surface.close` / `surface.close_self`) / `"crash"`(plugin 프로세스 크래시 cascade)
- 호스트가 fire-and-forget으로 fan-out 하며 plugin 응답은 무시. SDK는 `Plugin::on_event(EventDispatchCtx)` 트레이트 메서드로 dispatch
- cascade 닫힘(탭/팬/워크스페이스 전체 삭제로 따라가는 surface)은 envelope 대상이 아니며, 명시적으로 close된 surface_id만 발사된다

### Plugin extension (다른 plugin 확장)
- 매니페스트 `[extends]` 블록으로 특정 target plugin의 IPC 호출/이벤트 발화를 가로채는 extension plugin을 작성 가능. 단일 A+ 제약: target 1개당 활성 extension은 최대 1개 (lexicographic 우선순위로 winner 결정, 나머지는 `Conflict` 상태)
- hook 종류: `pre_event` / `post_event` / `pre_ipc` / `post_ipc`. mode: `transform`(payload 교체) / `filter`(`pass: bool` 차단) / `observe`(관찰만)
- 권한: `[extends]` 선언 plugin은 `permissions[]`에 `ext:<target_id>` 토큰 필수
- 상태: `Active` / `Pending(reason)` (대상 미설치/버전 불일치 등) / `Disabled` / `Conflict`. 대상 install/enable 시 자동 승격
- Fail-open: hook timeout/에러 시 원래 값 사용, 흐름은 계속. `(extension_id, target_key)` 연속 3회 실패 → 60초 backoff. self-loop(caller == extension_id) skip
- SDK: `Plugin::handle_extension_hook(ctx) -> ExtensionHookOutcome` (`pass()` / `block()` / `transformed(v)`). 작성 가이드: `docs/dev-guide/plugin-development.md` §6-1
- CLI: `tasty plugin extension list`로 전체 extension 상태 조회 (`plugin.extension.list` IPC)

### Plugin i18n
- 매니페스트 `lang_dir` (기본 `"lang"`): plugin 디렉터리 내 lang 파일들이 위치
- 호스트는 plugin 디스커버리 시 `<lang_dir>/en.toml`(fallback) + `<lang_dir>/<active>.toml`을 읽어 namespace overlay로 호스트 i18n registry에 머지
- lookup 순서: 호스트 base → plugin namespaces. base에 동일 키가 있으면 plugin은 호스트 키를 덮어쓸 수 없음
- plugin install 시 `register_namespace`, remove 시 `unregister_namespace` (`src/i18n.rs` — 본 바이너리 잔존, GUI-free 도메인 crate 후보)

### 한계
- IPC 게이트는 plugin이 호스트를 통한 호출만 막음. plugin이 직접 fs를 쓰면 호스트가 알 수 없음 — 향후 OS-level 샌드박스/WASM으로 보강 ([평가](architecture/plugin-sandbox-evaluation.md))
- 호스트의 빌트인 ExplorerPanel은 단계 08D에서 외부 plugin으로 일원화 예정 (1300+ 줄 침습적 refactor라 별도 작업으로 분리)

## 파일 핸들러 시스템 (file-handler-system)

URI/경로 입력을 받아 **(1) 파일 형식 식별 → (2) 등록된 핸들러 디스패치** 두 단계를 거치는 통합 라우팅 시스템. 두 단계는 독립 모듈(`src/file_format/`, `src/file_handler/`)로 분리되어 있고, `file_handler` 만 `file_format::DetectorId` 를 import 하는 단방향 의존 관계를 가진다.

### 형식 식별 (`FileFormatRegistry`)
- DetectorId 네임스페이스: 일반 `[a-z0-9-]{1,64}` (예: `markdown`), 호스트 예약 `$<word>` (예: `$directory`)
- detector rule 종류 (Cheap = file IO 없음, Deep = 8KB head read + `infer` MIME 추정):
  - `extension`: 확장자 대소문자 무시 매칭 (Cheap)
  - `path_glob`: 파일명 wildcard (`*` 만, 본격 globset 도입은 Phase B+) (Cheap)
  - `is_directory`: 대상이 디렉토리 (Cheap)
  - `magic`: `offset` + `bytes_hex` 매칭 (Deep, regular file 한정 / FIFO/socket/device skip)
  - `mime`: `infer` 기반 MIME 추정 후 대소문자 무관 비교 (Deep)
  - `lua`: 인라인 Lua 5.4 sandbox 평가 (Deep, host/user TOML 만 — plugin 출처는 install 시 drop+warn). `target = { path, is_directory, bytes_head, mime, has_prefix }` 글로벌 주입. 메모리 cap 8MB, 명령어 cap 1M, `io`/`os`/`debug`/`package`/`require`/`load*`/`dofile` 제거, bytecode 청크 금지
  - `structure_check`: 절대 경로의 JSON Schema 파일로 target 의 구조 검증 (Deep). 현재 JSON 입력만 지원 (`.json` 확장자), 5MB 초과 파일은 즉시 false. schema/target 읽기·파싱 실패 시 false + warn 로그
- Deep 평가는 한 `identify` 호출당 `DeepCtx` 가 head/MIME 캐시 → 같은 파일을 여러 detector 가 평가해도 IO 는 1회만
- pre-filter: 디렉토리 대상은 `is_directory` rule 가진 detector 만, 파일 대상은 그 외만 평가 (cross-match 방지)
- 호스트 default 는 `src/file/format/defaults/default-file-format.toml` 에 정의 — html, `$directory` 등. markdown / image detector 는 각각 `com.tasty.markdown` / `com.tasty.image` plugin 이 contribute

### 핸들러 디스패치 (`FileHandlerRegistry`)
- HandlerId 형식: `host/<name>`, `<plugin_id>/<name>`, `user/<name>` (`<name>` 은 `[a-z0-9-]{1,32}`)
- HandlerAction: `OpenSurface { surface_kind, param_key }`, `Ipc { method, owner_plugin_id }`, `System` (OS 기본 열기 위임)
- actor 별 schema 강제:
  - host TOML: OpenSurface / Ipc / System 전부 허용
  - plugin TOML: System 금지 (serde reject) — sandbox 일관성
  - user TOML: 전부 허용 (사용자가 자기 시스템에 명시 위임)
- `handlers_for(detector)` 정렬: priority asc → tie 시 owner 우선 `user > plugin > host` → handler id 사전순
- `all_handlers()`: picker 용 전체 enabled 목록

### Contribution-based registry
- 두 registry 모두 출처별(Host/Plugin(id)/User) contribution 을 보관하고, finalize 시 patch semantics 로 머지 (last-writer-wins, `None` 은 덮어쓰지 않음). rules 는 union + dedupe.
- plugin uninstall 시 그 plugin 의 contribution 만 제거 — host default / user 설정은 그대로 유지.

### Plugin 통합
- 매니페스트 `[[contributes.detector]]` / `[[contributes.handler]]` 로 추가. validate 단계:
  - detector: `$` prefix(예약 sentinel) plugin 추가 금지, 매직바이트 hex 검증, path traversal 차단
  - handler: short-name 패턴 검증, detector 참조 cross-check, surface_kind/ipc_prefix 등록 여부 확인
- 권한:
  - `file_handler.define`: 새 detector + handler 추가 권한
  - `file_handler.extend:<id>`: 기존 detector 에 rule 추가
  - `file_handler.handle:<id>`: 기존 detector 에 handler 만 추가
  - `$unknown` 같은 sentinel 은 모든 토큰에서 reject (예약어)

### User config
- `~/.tasty/file-handlers.toml` — 한 파일에 `[[detector]]` + `[[handler]]` 섹션 혼재 가능, 부팅 시 1회 로드
- patch 가능 필드: priority, display_name_i18n_key, disabled, action — `disabled = true` 만 명시 override (false 는 무시)
- 파일 없음 → 정상 (사용자 설정 없는 상태). parse 실패 시 warn + 전체 무시. entry 단위 schema 오류는 그 entry 만 reject.

### Picker popup
- `file_handler_picker` PopupDef (480x동적). 헤더(대상/형식) + 두 열(후보/최근) + [열기]/[취소]
- 좌측: handler id 사전순, 우측: `~/.tasty/file-handler-recent.json` LRU(cap 10) 순서 — 현재 등록 안 된 id 는 표시만 제외(저장 파일은 유지)
- 더블클릭 또는 [열기] → Selected dispatch + recent 기록 / [취소]/ESC/X → Cancelled
- picker 자체는 dispatch 하지 않고 `state.dialogs.file_handler_picker.result` 로만 결과를 남김 — 호스트 본체 layer 가 frame 끝에 소비해 실행 + atomic save

### RecentPicks
- 저장: `~/.tasty/file-handler-recent.json` (홈 못 찾으면 임시 디렉토리로 fallback)
- 원자적 쓰기: `<path>.tmp` 작성 → rename. fsync 는 안 함 (UX 영향)
- LRU dedupe + cap=10. parse 실패 시 빈 리스트로 시작 + warn

### User config 직렬화 (MD4)
- `FileFormatRegistry::export_user_config()` / `FileHandlerRegistry::export_user_config()` — RuleOrigin::User / HandlerOwner::User 만 추려 TOML 문자열로 emit
- `save_user_config(path)` — tempfile + rename 으로 atomic write. 빈 결과(사용자 항목 0)도 빈 파일로 덮어쓴다
- `file_handlers_save::save_combined_user_config(file_format, file_handler, path)` — 두 registry 의 user export 를 합쳐 한 파일에 atomic write. Settings UI 에서 한쪽만 저장 시 다른 쪽 섹션이 사라지는 문제 방지
- patch semantics 보존: user contribution 의 Some 필드만 emit, 호스트/플러그인이 제공한 base 는 미포함
- `DetectorRuleKind::Unknown` 의 raw payload 도 round-trip — forward-compat 유지
- 주석/공백/key 순서는 보존 안 함 (재발급). 사용자 손편집 친화 보존이 필요해지면 `toml_edit` 도입

### Settings UI — FileHandler 탭
- `Settings > File Handler` 탭, 3 개 sub-tab (Detectors / Handlers / Extension Mapping). 각 sub-tab 첫 진입 시 역할 paragraph + 컬럼 의미 bullet 리스트가 표시되어 사용자가 i18n key 외부 문서 없이 개념을 파악할 수 있다.
- **Detectors**: 등록된 모든 detector 의 id, 출처 (host/plugin/user), rule 종류 요약 (ext/glob/mime/magic/dir/lua/structure), enabled 토글
  - Enabled 체크박스 = host/plugin default 를 user-origin override (`disabled_override`) 로 덮어씀
  - user-origin 항목은 Remove 버튼으로 삭제 가능 (저장 시 적용)
  - "+ Add user detector" inline form — id + 확장자 (콤마/공백 구분) + 단일 path-glob 으로 간단 정의. 고급 rule (magic / mime / structure-check) 은 TOML 손편집
- **Handlers**: priority 오름차순 정렬된 handler 목록 — priority, id, owner, detector, action 요약 (`surface:<kind>` / `ipc:<method>` / `system`), enabled 토글
  - Enabled / Remove 동작은 detector 와 동일 (user-origin 만 Remove 가능)
  - "+ Add user handler" inline form — short-name (`user/<name>` 으로 저장) + detector dropdown + priority + action kind (open-surface / ipc / system) + 각 kind 별 필드 (surface_kind+param_key / method)
- 편집은 `FileHandlerEditDraft` 에 누적되며 Settings 의 Save 버튼이 registry 에 commit + `save_combined_user_config` 로 `~/.tasty/file-handlers.toml` atomic write
- Recent picks 는 picker popup 내 "최근" 열에서만 노출되며, Settings UI 에서는 sub-tab 으로 분리하지 않는다 (forget 은 `~/.tasty/file-handler-recent.json` 직접 편집)

### Extension Mapping (Phase E ME4)
- Plugin 매니페스트 등록 경로: `[[contributes.detector]]` + `[[contributes.handler]]` 로 plugin 이 자기 확장자와 핸들러를 contribute. host TOML 도 같은 구조 — last-writer-wins. 실 예시는 `crates/tasty-plugin-image/tasty-plugin.toml` (image detector + viewer handler) / `crates/tasty-plugin-html/tasty-plugin.toml` (html viewer handler, detector 는 host 유지)
- Settings UI 의 Extension Mapping sub-tab 은 광고 detector ≥ 2 인 ext 만 기본 노출. plugin 만 광고하는 ext (예: image) 는 plugin disabled 시 UI 에서 사라짐 — 단순화 의도, plugin enable 로 즉시 복귀
- 같은 확장자를 광고하는 detector 가 여러 개 있을 때 사용자가 직접 우선순위를 정할 수 있는 표 (`[[extension_priority]]`)
- 호스트 default / 사용자 설정 양쪽에서 정의 가능. plugin manifest 는 이 섹션을 못 씀 — 사용자 영역
- TOML: `[[extension_priority]] extension = "md" order = ["mdx-strict", "markdown"]`
- last-writer-wins (host → user 순서 install) — 사용자가 호스트 default 를 덮어쓸 수 있음
- 빈 `order = []` 는 entry 제거 의도로 해석
- `identify` 의 cheap path 가 파일 확장자가 있을 때 이 표를 fast path 로 사용 — 표에 적힌 detector 가 enabled + 광고 detector 안에 있으면 1순위로 선택. 표에 없거나 부적격이면 `install_order` 순서로 fallback
- Settings UI: `File Handler` 탭의 `Extension Mapping` sub-tab — 기본 노출 대상은 광고 detector 가 2개 이상인 확장자(=실제로 우선순위 의미가 있는 항목)이며, draft 에 추가된 확장자는 candidate 수와 무관하게 함께 노출된다. ↑/↓ 버튼으로 재정렬, 하단 textbox 로 새 확장자 직접 추가 가능, "Reset" 으로 user entry 제거 → host default 가 있으면 그것이, 없으면 `install_order` 순서가 다시 적용된다
- Settings 저장 시 `save_combined_user_config` 로 `~/.tasty/file-handlers.toml` 에 atomic write — `[[handler]]` 섹션 보존

### 한계 (현재)
- mouse.rs 콜사이트 변경은 별도 작업 — ctrl+click 시 여전히 기존 `terminal_link::open_uri` 가 동작
- `structure_check` 는 JSON 입력만 지원 — YAML/TOML 은 별도 deps 도입 후
- 상대 경로의 `spec_path` 는 호스트 CWD 기준으로 해석됨 — plugin 매니페스트 dir 기준 해석은 install 단계에서 수행 필요 (별도 작업)
- `path_glob` 은 단순 `*` wildcard 만, 본격 globset 도입은 후속
- Deep 평가는 sync — worker thread 분리 (`AppEvent::IdentifyDone`) 는 별도 작업

## Lua Hooks (사용자 init.lua)

호스트 전용 user scripting 레이어. 사용자가 `~/.tasty/init.lua` 에 `tasty.on("<event>", function(ctx) ... end)` 를 적어 GUI 동작에 외부 자동화를 붙일 수 있다. observe-only — 콜백은 호스트 흐름을 바꿀 수 없다. Plugin 은 Lua 를 사용하지 않는다 (Rust 전용).

### 엔진
- `tasty-lua` 크레이트 — mlua 0.10 (Lua 5.4, vendored)
- 단일 `LuaEngine` 인스턴스가 `App` 에 보유 — 메인 스레드 1 군데서만 호출
- 약 sandbox: 메모리 32 MB cap, 텍스트-청크만 (bytecode 거부), `debug`/`loadstring`/`loadfile`/`dofile`/`load`/`package.loadlib` 제거. `io`/`os.execute` 는 유지 (사용자 자기 머신/자기 스크립트)

### 호스트 API
- `tasty.on(event, callback)` — hook 등록
- `tasty.log(msg)` / `tasty.warn(msg)` — `tracing::info!` / `warn!`
- `tasty.notify(title, body)` — OS 네이티브 알림 (notify-rust)
- `tasty.run_cli(args)` — `tasty` CLI 를 자식 프로세스 detached 실행

### 이벤트 (15 hook point, post-only)
- `tasty.startup.post`
- `window.create.post` / `window.delete.post`
- `workspace.create.post` / `workspace.delete.post` / `workspace.change.post`
- `tab.create.post` / `tab.delete.post` / `tab.change.post`
- `pane.create.post` / `pane.delete.post`
- `surface.create.post` / `surface.delete.post`
- `change.post` 는 **사용자가 GUI 다이얼로그로 직접 변경** 한 경우만 발화. IPC/CLI 경유 변경은 plugin 이벤트 버스에는 가지만 Lua hook 으로는 안 감 (`PendingHostEvent::{WorkspaceRenamed,TabRenamed}` 에 `user_direct: bool` 플래그로 분기)

### 콜백 isolation
- 콜백 에러는 `tracing::warn!` 로 기록 + 같은 이벤트의 다음 콜백을 계속 호출
- payload 직렬화 실패 시 이 이벤트의 모든 콜백을 skip
- 콜백 리턴값은 무시 (observe-only)

### EmmyLua stub
- `crates/tasty-lua/meta/tasty.lua` — LuaLS 의 `workspace.library` 에 추가하면 자동완성/타입체크 가능

### Reload
- IPC: `script.reload` (local_only, plugin 호출 불가) — `{ loaded: bool }` 반환
- CLI: `tasty script reload`
- 재로딩 시 기존 등록 hook 모두 제거 후 같은 init.lua 재실행

### 한계 (현재)
- pre 이벤트 없음 — observe-only 에선 의미 없음. intervention 권한 도입 시 추가
- `tasty.shutdown.post` 없음 — shutdown 시 fire 인프라 별도 필요
- `surface.change.post` 발화 site 없음 — GUI 에서 surface 타입 변경하는 경로 부재

## 리스닝 포트 뷰어

### 개요
활성 surface의 셸 프로세스 트리에서 TCP LISTEN 중인 포트를 표시. 클릭하면 시스템 기본 브라우저에서 `http://<host>:<port>`를 연다. 5초 TTL 캐시 + 새로 고침 버튼.

### 트리거
- 사이드바 하단 Tools 메뉴 상단에 `Listening ports...` 빌트인 항목
- 클릭 시 `port_scanner` popup 오픈 (`PopupScope::Window`, 360x320)

### 동작
- popup 생성 시 활성 surface의 `shell_pid` 조회 → `tasty_portscan::collect_descendant_pids`로 자손 PID 수집
- `tasty_portscan::scan_for_pids(pids)` 호출 → `Vec<ListeningPort>` 캐시 저장
- 각 행: `<port> · <addr> · PID <pid>`. 클릭 시 wildcard(`0.0.0.0`/`::`)는 `localhost`로 치환하여 브라우저 열기
- 새로 고침 버튼: 캐시 즉시 무효화 후 재스캔

### OS별 백엔드
- **Linux**: `/proc/net/tcp` + `/proc/net/tcp6` 파싱 (state==0x0A 필터) → inode → `/proc/{pid}/fd/*` symlink 매칭
- **macOS**: `lsof -nP -iTCP -sTCP:LISTEN -p <pids>` subprocess → human-readable 출력 파싱
- **Windows**: `GetExtendedTcpTable` Win32 API 호출 (v4: `MIB_TCPTABLE_OWNER_PID`, v6: `MIB_TCP6TABLE_OWNER_PID`, `TCP_TABLE_OWNER_PID_LISTENER` 필터)

### 프로세스 트리
- **Linux**: `/proc/*/stat`의 ppid 필드 수집 → 부모-자식 맵 → BFS
- **macOS**: `ps -A -o pid=,ppid=` subprocess
- **Windows**: `CreateToolhelp32Snapshot` + `Process32FirstW/NextW`

### 구현
- crate: `tasty-portscan` (lib only, OS별 분기)
- 캐시: `tasty_portscan::PortScanCache` (5s TTL, surface_id 키)
- popup: `src/ui/port_scanner_popup.rs`
- AppState: `port_scan: tasty_portscan::PortScanCache` 필드
- 트리거: `src/ui/tools_menu.rs`의 `BUILTIN_TOOLS` 항목

## 명령 팔레트

### 개요
VS Code 스타일의 모든 단축키 명령을 쿼리로 검색하여 실행할 수 있는 popup. 키보드만으로 단축키를 외우지 않아도 모든 기능에 접근 가능.

### 트리거
- 기본 단축키: `Ctrl+Shift+P` (macOS는 `Alt+Shift+P` 추가)
- Tools 메뉴 `Command palette…` 항목

### 동작
- 텍스트 입력으로 `KeybindingSettings::GENERAL_BINDING_FIELDS` 의 i18n 라벨에 대해 case-insensitive 매칭
- 매칭 알고리즘: 정확 substring (단어 시작 보너스) → 부분 시퀀스 (gap 페널티) 순으로 점수화
- `↑/↓` 이동, `Enter` 실행, `Esc` 닫기, 클릭으로도 실행
- 우측에 첫 번째 바인딩(예: `ctrl+w`)을 회색으로 표시
- Enter 시 `state.command_palette.pending_run` 에 `field_id` 를 적재 → MainView가 다음 프레임 render 직후 drain하여 `dispatch_action_by_id` 호출
- dispatch는 동일한 action body를 사용하므로 단축키와 정확히 같은 효과

### 지원 명령
모든 `GENERAL_BINDING_FIELDS` 항목 (단축키 설정 탭에 나타나는 모든 동작). `toggle_command_palette` 자신만 제외.

### 사용자 vs 에이전트 행동
명령 팔레트 자체는 **사용자 행동**이다 (사용자가 키보드로 명령을 선택). 활성 surface/pane 등 포커스 상태를 사용하는 동작도 허용됨. CLI/IPC로 노출하지 않는다.

### 구현
- 상태: `src/command_palette.rs` — `CommandPaletteState { query, selected, pending_run }`, `search()`, `match_score()`
- popup: `src/ui/command_palette_popup.rs` (`command_palette` ID, 520x360, sticky_focus, close_on_outside_click)
- dispatch: `src/shortcuts.rs::MainView::dispatch_action_by_id(action_id: &str) -> bool`
- 단축키: `KeybindingSettings::toggle_command_palette` (`ctrl+shift+p`)
- drain: `src/view/main/redraw.rs` 의 render 직후
- i18n: `command_palette.*` (en/ko/ja)

## 자동 업데이트 확인

### 개요
GitHub Releases API를 폴링하여 새 버전이 있는지 확인하고, 발견 시 알림과 Settings → Updates 탭으로 노출한다. **Phase J.H — 알림 + `tasty update` 1-click**: 백그라운드 감지 + in-app 알림 + CLI `tasty update` 로 다운로드/SHA256 검증/원자 swap 까지 자동화. GUI 에서의 in-app 설치는 보류 (브라우저로 릴리스 페이지 열기).

### 트리거
- Tools 메뉴의 `Check for updates…` 빌트인 항목 (port_scanner 아래)
- 앱 시작 시 백그라운드 폴러가 1회 즉시 + 이후 1시간 간격으로 자동 폴링
- 새 버전 발견 시 1회 한도로 in-app 알림 발사 (`notified_version` 으로 중복 차단)
- Settings → Updates 탭의 `Check now` 버튼
- CLI: `tasty update` (standalone — 호스트 실행 불필요)

### 동작
- `tasty_update::check_latest(owner, repo, current_version, allow_prerelease)` 호출 → `Result<Option<ReleaseInfo>, UpdateError>`
- 응답이 `Some(ReleaseInfo)` 이면 `latest_version > current_version` 인 경우만 반환 (semver 비교; `v` prefix 자동 제거)
- popup 에서 현재 버전, 최신 버전, 릴리스 노트(스크롤), `Open release page` 버튼, `Check now` 버튼 표시
- Settings → Updates 탭: 현재/최신/마지막 확인 시각, `Check now`, `Open release page…`, CLI 안내
- 에러 발생 시 popup/탭에 `Error: <msg>` 빨간 라벨 표시
- 알림: `update.notify.title` / `update.notify.body` (3 lang)
- `tasty update` CLI 흐름: check → 사용자 확인 (`--yes` 로 skip) → asset 선택 (OS×arch) → 다운로드 + 진행률 표시 → `SHA256SUMS-{platform}.txt` 다운로드 + 검증 (hard fail) → atomic swap → 사용자에게 재시작 안내

### 자산 매트릭스 (`select_asset`)

| target_os | target_arch | 선택 우선순위 |
|-----------|-------------|---------------|
| macos     | any         | `Tasty-{v}-macos.dmg` (.app 교체는 J.H+ 보류 — 사용자 수동 DMG) |
| windows   | x86_64      | `.msi` → `.zip` (fallback) |
| linux     | x86_64      | `.deb` (Debian-like) / `.rpm` (RPM-like) / `.AppImage` / `.tar.gz` |
| linux     | aarch64     | 같은 우선순위, arm64 변종 |

Linux 가족 detect 는 `/etc/os-release` 의 `ID=` / `ID_LIKE=` 기반. SHA256SUMS 파일은 `macos`/`windows`/`linux-x64`/`linux-arm64` 4종.

### 구현
- crate: `tasty-update`
  - `check_latest(owner, repo, current_version, allow_prerelease) -> Result<Option<ReleaseInfo>, UpdateError>`
  - `is_newer(current, remote_tag) -> Result<bool, UpdateError>`
  - `select_asset(info) -> Option<AssetSpec>` — `(target_os, target_arch)` 4 케이스
  - `download_to(asset, dest, progress)` — 스트리밍 다운로드 + Content-Length 기반 진행률
  - `fetch_sha256_sums(info) -> HashMap<name, hex>` + `verify_sha256(path, expected)`
  - `atomic_swap(new, target) -> SwapOutcome::{Completed, RestartRequired}`
    - Unix: `rename(2)` + chmod 755 (실패 시 cross-device copy fallback). `.old` 백업 보존
    - Windows: `tasty.new.exe` 스테이징 + `tasty-swap.bat` (재실행 시 swap)
    - macOS: DMG 안내만 (Completed 아님)
  - 의존성: `ureq`, `serde`, `semver`, `sha2`, `thiserror`, `tracing`, `tempfile` (dev)
- 백그라운드 폴러: `src/state/update_check.rs`
  - `UpdateStatus { latest, last_error, last_checked, in_flight, notified_version, pending_notify }`
  - `spawn_poller(owner, repo, current, interval)` / `trigger_check(...)`
  - 새 버전 감지 (None→Some) 시 `pending_notify = Some(info)`
- 알림 drain: `src/app/dispatch/update_notifications.rs` — 매 frame `pending_notify` 를 take 해 `DomainIntent::PushNotification { source: "update" }` 발행. `notified_version` 으로 중복 차단
- AppState: `update_status: Arc<Mutex<UpdateStatus>>` (1시간 간격 폴러 자동 spawn)
- popup: `src/adapters/ui/popup/update.rs` (`update_check` ID)
- 트리거: `src/adapters/ui/tools_menu.rs` 의 `BUILTIN_TOOLS` 항목
- Settings 탭: `src/view/settings/ui/tabs/updates.rs` (`SettingsTab::Updates`)
- CLI: `crates/tasty-cli/src/commands/update.rs` — `tasty update [--check-only] [--yes] [--prerelease]`. standalone (호스트 미실행 OK), `run.rs` 가 IPC connect 이전에 가로채 실행

## 접근성 (Accessibility)

### 개요
**Phase 1 — 수동 토글만**: 설정 → Accessibility 탭에서 직접 켜는 두 가지 옵션. OS 자동 감지(Windows `ANIMATIONS`, macOS `NSWorkspace`), AccessKit 통합, 색맹 팔레트, 스크린 리더 라벨은 Phase 2 이후.

### Reduced motion
- 설정: `accessibility.reduced_motion: bool` (기본 false)
- 동작: 활성 시 토스트 페이드인/페이드아웃이 0ms로 처리됨. lifetime 동안 100%, 만료 즉시 0%로 전환.
- 터미널 출력의 깜빡임/스크롤 등 콘텐츠 애니메이션은 영향 없음 (CLAUDE.md "터미널 콘텐츠 애니메이션: 절대 0ms" 원칙에 따라 이미 모션 없음).

### High contrast (placeholder)
- 설정: `accessibility.high_contrast: bool` — UI 체크박스는 비활성(disabled). Phase 2에서 Theme 분기 추가 예정.

### 구현
- crate: `tasty-settings` — `AccessibilitySettings { reduced_motion, high_contrast }`
- 토스트: `src/ui/toast.rs::ToastManager::draw(ctx, layout, reduced_motion: bool)` — 알파 계산 분기
- 설정 UI: `src/settings_ui/tabs.rs::draw_accessibility_tab` (탭 = `SettingsTab::Accessibility`)
- i18n: `settings.accessibility.*` 키 (en/ko/ja)

## Git 뷰어 (builtin plugin)

### 개요
**read-only MVP**: 활성 surface 의 cwd 에서 git repo 를 찾아 상단에는 working tree 변경 목록을, 하단에는 커밋 평면 리스트(또는 선택된 파일의 diff)를 표시하는 popup. 모든 동작은 **read-only** — stage/commit/checkout 등 mutate 작업은 없다 (그건 터미널에서 직접 하라는 정책).

본바이너리에 박혀 있던 in-tree popup을 외부 plugin `com.tasty.git-viewer`로 분리해 release/dist 빌드 시 `plugins/` 디렉터리에 함께 배포된다. plugin 비활성 시 사이드바 도구 메뉴에서 항목이 사라지고 호스트는 git 관련 코드 0.

### 트리거
- 사이드바 도구 메뉴의 `Git` 항목 — plugin의 `[[contributes.tool]]` 로 노출.
- 클릭 시 호스트 `tools_menu::invoke_tool::OpenPopup` 분기가 활성 surface의 상속 cwd를 `context.cwd` 페이로드에 실어 `popup.open` IPC로 plugin에 전달.

### 동작
- 첫 진입 시 plugin process가 `git2::Repository::discover(cwd)` 로 repo 탐색.
- 상단(Changes): `M / A / D / R / ? / U` 아이콘 + 색상 (yellow/green/red/blue/overlay0/red), 파일 클릭 → 하단을 diff 패널로 전환. SelectableRow 위젯으로 행 강조.
- 하단 기본(Commits): 최근 200개 커밋, `[oid] (refs?) summary  author  time` 평면 리스트 — **그래프 없음**.
- 하단 diff: working tree vs HEAD 통합 (staged/unstaged 분리 없음). hunk 헤더(blue), `+`(green) / `-`(red) / context(text), 좌측 줄번호. `Back` 버튼으로 log 복귀.
- `Refresh` 버튼으로 status/log/diff 일괄 재수집.
- 단일 인스턴스 — 이미 열린 상태에서 다시 메뉴 클릭 시 "already open" placeholder만 표시.
- repo 없음/에러 시 안내 메시지.

### IPC 노출 없음
사용자 UI 편의 기능. 에이전트는 터미널에서 `git status`/`git log`/`git diff` 직접 호출하면 충분하므로 IPC 표면에 노출하지 않는다 (popup은 `trigger = "ipc"`이지만 호스트 내부 tool-action 경로로만 호출 가능).

### 구현
- crate: `crates/tasty-plugin-git-viewer/`
  - `src/git.rs` — git2 래핑 (discover/status/log/diff), 모두 read-only.
  - `src/view.rs` — UiNode tree 빌더. `SelectableRow` + `Label{ style: Mono, color }` 조합.
  - `src/main.rs` — Plugin impl, 단일 인스턴스 가드, popup event dispatch.
- manifest: popup 720×540, anchor=screen-center, dismiss_on_outside_click. permissions `ui.popup`, `ui.tool_item`, `fs.read`.
- 의존성: `git2 = "0.19"` (`default-features = false` — HTTPS/SSH 불필요, libgit2 vendored C 빌드). 본바이너리에는 더 이상 git2 의존 없음.
- i18n: plugin 자체 `lang/{en,ko,ja}.toml`. `tasty-plugin-sdk::i18n::Translator`가 `TASTY_LOCALE` 환경변수로 활성 언어 결정.

### 추후 항목
커밋 그래프, staged/unstaged 분리, 커밋 클릭 → 해당 커밋 diff, 브랜치/태그 목록, 자동 새로고침, 백그라운드 스레드 수집, 리사이즈 디바이더.
