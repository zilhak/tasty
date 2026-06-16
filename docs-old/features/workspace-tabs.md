# 워크스페이스 & 탭

- **Status**: Implemented

### 데이터 모델

용어 정의는 `docs/concepts/ubiquitous-language.md` 참조.

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
- 탭 UI: 너비/폰트 크기를 Appearance settings 에서 직접 조정 (기본 150px / 11px). 모니터 scale 은 egui 가 자동 반영 (= auto). 1px 세로 구분선(surface1), active 탭 상단 강조선(blue)
- 탭 스크롤: 탭이 영역을 초과하면 좌우 화살표 버튼(< >)으로 스크롤 가능
- 탭 바 우측 아이콘 버튼: **Split**(해당 pane 분할 — 단축키 split_pane 과 동일 경로) + **Search**(해당 pane 활성 surface 검색 — find 와 동일 경로). 클릭 시 대상 pane 으로 focus 이동 후 동작
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

단축키나 IPC/CLI(cwd 미지정)로 새 surface를 만들 때, 분할 대상 또는 포커스된 source surface의 `Surface::source_cwd()` 값을 새 터미널의 시작 디렉터리로 사용한다 (`docs/design/flows/split-command.md` 참조).

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
- **터미널 내 링크 hover·클릭 오픈**: 터미널 출력에 포함된 URL(`http://`, `https://`, `ftp://`, `file://`), OSC 8 hyperlink, 그리고 **스키마 없는 경로**(Unix 절대 `/foo/bar`, Windows 절대 `C:\foo`/`C:/foo`, 상대 `./foo`·`../foo`)를 감지. 경로는 터미널 OSC 7 기반 CWD를 기준으로 실제 존재할 때만 링크로 판정되어 오탐을 줄임. 설정된 수식키(기본 `Ctrl`, 설정에서 `Alt`/`없음` 선택 가능)를 누른 채 마우스를 올리면 해당 링크가 blue로 하이라이트되고 커서가 PointingHand로 변경됨. 수식키+좌클릭 시 `webbrowser` crate로 기본 브라우저/연결 프로그램을 열어 URI를 처리. 수식키+클릭은 링크 위가 아니면 아무 동작도 하지 않으며 selection과 충돌하지 않음. 사용자의 키보드/마우스 동작이므로 CLI/IPC로 노출되지 않음 (`docs/concepts/ubiquitous-language.md`의 사용자/에이전트 분리 원칙)
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
  - 상세 동작 분기: `docs/design/flows/explorer-context-menu.md` 참조

#### 컨텍스트 메뉴
- 터미널 영역 또는 탭 바 빈 공간에서 마우스 우클릭 시 컨텍스트 메뉴 표시
- "Open Markdown..." → 파일 경로 입력 다이얼로그 → 마크다운 탭 열기
- "Open HTML..." → URL 입력 다이얼로그 → HTML WebView 탭 열기
- "새 이미지" → 빈 이미지 surface 탭 생성 (기본 800×600 흰 캔버스가 즉시 그려진 상태로 시작, 다른 크기를 원하면 surface 안의 `+` 버튼으로 팝업 호출)
- 터미널 surface 영역 우클릭 시: 항상 "터미널 ID 복사" → 해당 surface id를 클립보드에 복사하고 surface 스코프 toast로 알림
- 드래그로 선택 영역이 있을 때 터미널 우클릭 시 위에 더해 두 복사 항목 표시 (좌버튼을 누른 채 드래그하다 우클릭해도 선택이 유지되어 동작):
  - **복사**: 선택 텍스트를 그대로 복사 (soft-wrap 합침 + 하드 개행 `\n` 유지). 단축키 복사와 동일 동작
  - **줄바꿈 없이 복사**: 추출 텍스트의 모든 `\n`을 공백 한 칸으로 치환하여 한 줄로 복사
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
