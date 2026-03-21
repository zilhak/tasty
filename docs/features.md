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
  - **화면 편집**: EraseInDisplay(ED), EraseInLine(EL), ScrollUp(SU), ScrollDown(SD), ClearScreen, ClearToEndOfLine, ClearToEndOfScreen
  - **ESC 시퀀스**: DECSC/DECRC(커서 저장/복원), RI(역방향 인덱스), RIS(전체 리셋)

### 키보드 입력
- winit `KeyEvent.text`를 활용한 수정자 키 반영 (Ctrl+C 등 제어 문자 자동 처리)
- 특수 키 매핑: Enter, Backspace, Tab, Escape, 방향키, Home/End, PageUp/PageDown, Insert/Delete, F1~F12

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
- 모노스페이스 폰트 기반 셀 그리드 레이아웃 (기본 14pt)

## 워크스페이스 & 탭

### 데이터 모델 (Workspace / PaneNode / Pane / Tab / Panel / SurfaceGroupNode)
- Workspace: 좌측 사이드바 항목. PaneLayout(PaneNode 이진 트리)을 포함
- PaneNode: 물리적 화면 분할 이진 트리. Leaf(Pane) 또는 Split
- Pane: **독립적인 탭 바**를 가진 화면 영역. 여러 Tab을 포함
- Tab: Pane 탭 바의 탭 하나. Panel에 매핑
- Panel: 콘텐츠 타입 enum. Terminal(단일) 또는 SurfaceGroup(탭 내부 분할)
- SurfaceGroupNode: 탭 내부에서 여러 터미널을 분할하는 이진 트리. 탭 바에서는 하나의 탭으로 표시
- SurfaceNode: 개별 터미널 인스턴스 (PTY + termwiz Surface)
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
- 분할 시 새 터미널 자동 생성 (PTY 포함), 소유권 이동(by-value) 패턴으로 placeholder PTY 좀비 프로세스 방지
- Workspace/Tab/SurfaceGroupNode 내부 Option 래핑으로 구조적 변경 시 안전한 take/put 패턴 적용
- 각 Surface를 scissor rect로 독립 렌더링
- 뷰포트별 유니폼 갱신 (grid_offset을 각 Surface rect에 맞게 조정)

### 키보드 단축키
- Ctrl+Shift+N: 새 워크스페이스
- Ctrl+Shift+T: 포커스된 Pane에 새 탭
- Ctrl+Tab: 다음 탭 (포커스된 Pane)
- Ctrl+Shift+Tab: 이전 탭 (포커스된 Pane)
- Alt+1~9: 워크스페이스 전환
- Ctrl+Shift+E: Pane 수직 분할 (새 독립 탭 바)
- Ctrl+Shift+O: Pane 수평 분할 (새 독립 탭 바)
- Ctrl+D: SurfaceGroup 수직 분할 (탭 내부)
- Ctrl+Shift+D: SurfaceGroup 수평 분할 (탭 내부)
- Alt+Arrow: Pane 간 포커스 이동
- Ctrl+I: 알림 패널 토글
- winit ModifiersState를 이용한 수정자 키 추적

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
- FIFO 방식 알림 저장소 (최대 100개, 초과 시 오래된 항목 자동 삭제)
- 알림 병합(coalescing): 같은 소스에서 500ms 이내 연속 알림이 오면 기존 알림에 합침
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
- egui 사이드바를 제외한 전체 영역에 PaneLayout 렌더링
- PaneNode에서 각 Pane의 rect를 계산, 탭 바 높이를 뺀 영역에 터미널 렌더링
- 리사이즈 시 모든 Pane, 모든 Tab, 모든 Surface의 행/열 재계산
- wgpu RenderPass의 forget_lifetime()을 이용한 egui-wgpu 호환

## 설정 시스템

### TOML 기반 설정 파일
- 설정 파일 경로: `~/.config/tasty/config.toml` (전 플랫폼 통일)
- `directories` 크레이트로 플랫폼별 홈 디렉토리 추상화
- `toml` + `serde` 기반 직렬화/역직렬화
- 설정 파일이 없거나 파싱 실패 시 기본값으로 폴백

### 설정 카테고리
- **General**: 셸 경로 (OS별 자동 감지: COMSPEC/SHELL), 시작 명령
- **Appearance**: 폰트 패밀리, 폰트 크기, 테마 (dark/light), 배경 투명도, 사이드바 너비
- **Clipboard**: OS별 기본 활성화 (macOS: Alt+C/V, Linux: Ctrl+Shift+C/V, Windows: Ctrl+C/V)
- **Notifications**: 알림 활성화, 시스템 알림, 사운드, 병합 간격(ms)
- **Keybindings**: 워크스페이스/탭/패인/서피스 분할 단축키

### GUI 설정 윈도우
- Ctrl+, 단축키로 설정 윈도우 토글
- egui Window 기반 탭 인터페이스 (General / Appearance / Clipboard / Notifications)
- 편집 중 원본 설정을 보존하는 드래프트 패턴
- Save 버튼: 디스크에 저장 후 즉시 적용
- Cancel 버튼: 변경 사항 폐기

### 설정 로드/저장
- `Settings::load()`: 설정 파일 로드, 없으면 기본값 반환
- `Settings::save()`: 설정 디렉토리 자동 생성 후 TOML 형식으로 저장
- `Settings::config_path()`: 플랫폼 독립적 설정 파일 경로 반환
- 앱 시작 시 자동 로드, AppState에 통합

## CLI 도구 & 소켓 API

### JSON-RPC IPC 서버 (ipc/)
- GUI 모드 시작 시 `127.0.0.1`의 랜덤 포트에 TCP 서버 자동 기동
- 포트 번호를 `~/.config/tasty/tasty.port` 파일에 기록하여 CLI 클라이언트가 접속 가능
- 앱 종료 시 포트 파일 자동 삭제 (Drop trait)
- JSON-RPC 2.0 프로토콜: 줄 단위 JSON 요청/응답
- 멀티클라이언트: 각 TCP 연결을 별도 스레드에서 처리
- 메인 스레드 채널 통신: IPC 스레드 -> mpsc 채널 -> 이벤트 루프에서 처리 -> oneshot 응답

### 지원 메서드
- `system.info`: 버전, 워크스페이스 수, 활성 워크스페이스 인덱스
- `workspace.list`: 전체 워크스페이스 목록 (이름, 활성 여부, 패인 수)
- `workspace.create`: 새 워크스페이스 생성 (선택적 이름 지정)
- `workspace.select`: 인덱스로 워크스페이스 전환
- `pane.list`: 활성 워크스페이스의 패인 목록 (포커스 여부, 탭 수)
- `pane.split`: 포커스된 패인 분할 (vertical/horizontal)
- `tab.list`: 포커스된 패인의 탭 목록
- `tab.create`: 포커스된 패인에 새 탭 추가
- `surface.list`: 활성 워크스페이스의 전체 서피스(터미널) 목록 (cols, rows 포함)
- `surface.send`: 포커스된 터미널에 텍스트 전송
- `surface.send_key`: 포커스된 터미널에 키 입력 전송 (enter, tab, escape, 방향키 등 이름 매핑)
- `notification.list`: 최근 50개 알림 목록
- `notification.create`: 알림 생성
- `tree`: 전체 워크스페이스/패인/탭 트리 구조 조회

### CLI 클라이언트 (cli.rs)
- `tasty` 명령에 서브커맨드가 있으면 CLI 모드, 없으면 GUI 모드로 동작
- clap 기반 서브커맨드: `list`, `new-workspace`, `select-workspace`, `send`, `send-key`, `notify`, `notifications`, `tree`, `split`, `new-tab`, `surfaces`, `panes`, `info`
- 포트 파일에서 포트 번호를 읽어 TCP 연결 후 JSON-RPC 요청/응답
- `tree` 커맨드: 워크스페이스/패인/탭 계층을 트리 형태로 표시
- 에러 시 종료 코드 1 반환
