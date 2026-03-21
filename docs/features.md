# Tasty - 구현된 기능

## 터미널 엔진

### PTY 기반 셸 실행
- ConPTY(Windows) / Unix PTY를 통한 네이티브 셸 실행
- `TERM=xterm-256color` 환경 설정
- PTY 리사이즈 전파: 윈도우 크기 변경 시 자식 프로세스에 새 크기 통보
- 자식 프로세스 핸들 관리: 생존 여부 확인 가능

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
- 분할 시 새 터미널 자동 생성 (PTY 포함)
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
- winit ModifiersState를 이용한 수정자 키 추적

### 터미널 뷰포트 관리
- egui 사이드바를 제외한 전체 영역에 PaneLayout 렌더링
- PaneNode에서 각 Pane의 rect를 계산, 탭 바 높이를 뺀 영역에 터미널 렌더링
- 리사이즈 시 모든 Pane, 모든 Tab, 모든 Surface의 행/열 재계산
- wgpu RenderPass의 forget_lifetime()을 이용한 egui-wgpu 호환
