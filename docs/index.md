# Tasty - 기능 계획 문서

cmux의 기능을 크로스 플랫폼(Windows, macOS, Linux) GPU 가속 네이티브 터미널 에뮬레이터로 구현하기 위한 계획 문서.

## 기능 목록

| # | 기능 | 문서 | 우선순위 |
|---|------|------|----------|
| 01 | [터미널 엔진](#터미널-엔진) | [plans/01-terminal-engine.md](plans/01-terminal-engine.md) | 핵심 |
| 02 | [워크스페이스 & 탭](#워크스페이스--탭) | [plans/02-workspace-tabs.md](plans/02-workspace-tabs.md) | 핵심 |
| 03 | [사이드바 메타데이터](#사이드바-메타데이터) | [plans/03-sidebar-metadata.md](plans/03-sidebar-metadata.md) | 핵심 |
| 04 | [알림 시스템](#알림-시스템) | [plans/04-notification-system.md](plans/04-notification-system.md) | 핵심 |
| 05 | [분할 패인](#분할-패인) | [plans/05-split-panes.md](plans/05-split-panes.md) | 핵심 |
| 06 | [CLI 도구](#cli-도구) | [plans/06-cli-tool.md](plans/06-cli-tool.md) | 핵심 |
| 07 | [소켓 API](#소켓-api) | [plans/07-socket-api.md](plans/07-socket-api.md) | 핵심 |
| 08 | [세션 복원](#세션-복원) | [plans/08-session-persistence.md](plans/08-session-persistence.md) | 중요 |
| 09 | [명령 팔레트](#명령-팔레트) | [plans/09-command-palette.md](plans/09-command-palette.md) | 중요 |
| 10 | [키보드 단축키](#키보드-단축키) | [plans/10-keyboard-shortcuts.md](plans/10-keyboard-shortcuts.md) | 핵심 |
| 11 | [검색](#검색) | [plans/11-search.md](plans/11-search.md) | 중요 |
| 12 | [클립보드 통합](#클립보드-통합) | [plans/12-clipboard.md](plans/12-clipboard.md) | 핵심 |
| 13 | [IME 지원](#ime-지원) | [plans/13-ime-support.md](plans/13-ime-support.md) | 중요 |
| 14 | [포트 스캐닝](#포트-스캐닝) | [plans/14-port-scanning.md](plans/14-port-scanning.md) | 부가 |
| 15 | [원격 SSH](#원격-ssh) | [plans/15-remote-ssh.md](plans/15-remote-ssh.md) | 확장 |
| 16 | [자동 업데이트](#자동-업데이트) | [plans/16-auto-update.md](plans/16-auto-update.md) | 부가 |
| 17 | [설정 시스템](#설정-시스템) | [plans/17-settings-system.md](plans/17-settings-system.md) | 핵심 |
| 18 | [Claude Code 통합](#claude-code-통합) | [plans/18-claude-code-integration.md](plans/18-claude-code-integration.md) | 핵심 |
| 19 | [마크다운 뷰어](#마크다운-뷰어) | [plans/19-markdown-viewer.md](plans/19-markdown-viewer.md) | 부가 |
| 20 | [윈도우 관리](#윈도우-관리) | [plans/20-window-management.md](plans/20-window-management.md) | 중요 |
| 21 | [복사 모드](#복사-모드) | [plans/21-copy-mode.md](plans/21-copy-mode.md) | 부가 |
| 22 | [Surface Hook](#surface-hook) | [plans/22-surface-hooks.md](plans/22-surface-hooks.md) | 중요 |
| 23 | [Read Mark API](#read-mark-api) | [plans/23-read-mark-api.md](plans/23-read-mark-api.md) | 중요 |
| 24 | [에이전트 자동화](#에이전트-자동화) | [plans/24-agent-automation.md](plans/24-agent-automation.md) | 핵심 |

## 아키텍처 문서

| 문서 | 설명 |
|------|------|
| [아키텍처 개요](architecture/index.md) | 모듈 구조, 의존성 그래프, 계층 |
| [모듈별 상세](architecture/modules.md) | 17개 소스 파일 상세 분석 |
| [데이터 흐름](architecture/data-flows.md) | 입력/출력/IPC/알림/설정 흐름 |
| [라이브러리 분리](architecture/library-separation.md) | 분리 후보 7개 분석 |
| [리팩토링 분석](architecture/refactoring.md) | 개선 가능성, 로드맵 |

## 구현 현황

구현된 기능의 상세 설명은 [features.md](features.md) 참조.

## 전략 문서

기능 횡단적인 설계 전략 문서.

| 문서 | 설명 |
|------|------|
| [GPU 활용 전략](plans/gpu-strategy.md) | 렌더링 아키텍처, 셰이더 설계, 버퍼 전략, 폴백 |
| [설치 전략](plans/install-strategy.md) | 환경 감지, 설치 스크립트, 하드웨어 프로파일링 |
| [접근성](plans/accessibility.md) | 키보드 내비게이션, 고대비, 스크린 리더, 색맹 지원 |
| [에러 처리/로깅](plans/error-logging.md) | 에러 카테고리, tracing 로깅, 크래시 리포팅 |
| [테스트 전략](plans/testing-strategy.md) | 단위/통합/시각적 회귀 테스트, 벤치마크 |

## 우선순위 정의

- **핵심**: 최소 동작 제품(MVP)에 반드시 포함
- **중요**: MVP 직후 구현 대상
- **확장**: 사용자 요구에 따라 구현
- **부가**: 있으면 좋지만 후순위

## 기술 스택

- **언어**: Rust
- **윈도우/입력**: winit
- **GPU 렌더링**: wgpu
- **UI 위젯**: egui (UI) + 커스텀 셰이더 (터미널)
- **VTE 파싱**: termwiz
- **PTY**: portable-pty (Windows: ConPTY)
- **IPC**: Unix socket / Named pipe
- **CLI**: clap
- **라이선스**: MIT

## 기능 요약

### 터미널 엔진
wgpu 기반 GPU 가속 터미널 렌더링. termwiz(WezTerm)로 VTE 파싱 및 셀 그리드 관리, cosmic-text/swash로 폰트 래스터라이징. cmux는 libghostty(Metal)를 사용하지만, tasty는 wgpu로 크로스 플랫폼 GPU 가속을 달성한다.

**현재 구현된 기능:**
- PTY 기반 셸 실행 및 입출력 (ConPTY/Unix PTY), PTY 리사이즈 전파
- termwiz Parser/Surface를 통한 포괄적 VTE 파싱 및 셀 그리드 관리 (SGR, 커서, 화면 편집, ESC 시퀀스)
- `Arc<Window>` 기반 안전한 wgpu surface 생명주기 관리
- cosmic-text FontSystem/SwashCache를 이용한 폰트 로딩 및 글리프 래스터라이징 (베이스라인 기반 오프셋)
- 2048x2048 R8 텍스처 아틀라스에 선반(shelf) 기반 글리프 패킹
- 인스턴스 렌더링 기반 셀 배경색 패스 + 글리프 텍스처 패스 (2-pass)
- WGSL 셰이더: 배경 컬러 쿼드 + 알파 블렌딩 글리프 쿼드
- xterm-256color 팔레트 지원 (ANSI 16색, 216색 큐브, 24단계 그레이스케일, TrueColor)
- winit `KeyEvent.text` 기반 수정자 키 반영 입력 처리 (Ctrl 조합, 특수키, F키)
- 윈도우 리사이즈 시 터미널 그리드 자동 재조정
- 모노스페이스 폰트 기반 셀 그리드 레이아웃 (기본 14pt)
- 이벤트 드리븐 렌더 루프: `EventLoopProxy<AppEvent>` 기반 PTY 웨이크업, 무조건적 `request_redraw()` 제거로 유휴 시 CPU 0%
- 상세: [features.md](features.md)

### 워크스페이스 & 탭
cmux 분석 기반 계층적 데이터 모델. Workspace → PaneLayout → Pane(독립 탭 바) → Tab → Panel(Terminal/SurfaceGroup).

**현재 구현된 기능:**
- Workspace / PaneNode / Pane / Tab / Panel / SurfaceGroupNode 계층 데이터 모델
- egui 좌측 사이드바 (워크스페이스 목록) + Pane별 독립 탭 바
- 두 가지 분할: Pane 분할(물리적 화면, 독립 탭 바) + SurfaceGroup 분할(탭 내부)
- 키보드 단축키: Ctrl+Shift+N(워크스페이스), Ctrl+Shift+T(탭), Ctrl+Shift+E/O(Pane분할), Ctrl+Shift+D/Ctrl+Shift+J(Surface분할), Alt+1~9(WS전환), Ctrl+Tab/Shift+Tab(탭전환), Alt+Arrow(Pane포커스), Ctrl+Shift+I(알림)
- 마우스 인터랙션: 클릭으로 Pane/Surface 포커스, 디바이더 드래그로 분할 비율 조절, 호버 시 리사이즈 커서, 마우스 스크롤
- 분할/리사이즈 시 모든 터미널 자동 크기 재조정
- 상세: [features.md](features.md)

### 사이드바 메타데이터
GPU 렌더링된 사이드바에 Git 브랜치, PR 상태, 작업 디렉토리, 리스닝 포트 등의 실시간 정보를 아이콘/색상과 함께 표시.

### 알림 시스템
인앱 시각 알림 + OS 네이티브 알림. OSC 시퀀스(9/99/777/7) 및 BEL 감지, 알림 병합, 레이트 리미팅.

**현재 구현된 기능:**
- OSC 9(iTerm2), OSC 99(Kitty), OSC 777(rxvt), OSC 7(CWD), OSC 0/2(타이틀), BEL 감지
- NotificationStore: FIFO 저장(최대 100개), 500ms 병합, 워크스페이스별 카운트
- notify-rust를 통한 OS 네이티브 알림 (비활성 윈도우, 초당 1회 제한)
- 사이드바 알림 배지 및 워크스페이스 하이라이트
- Ctrl+I 알림 패널: 스크롤 목록, 워크스페이스 점프, 읽음 처리
- 상세: [features.md](features.md)

### 분할 패인
두 가지 분할 지원. Pane 분할(Ctrl+Shift+E/O): 물리적 화면 분할, 새 독립 탭 바 생성. SurfaceGroup 분할(Ctrl+Shift+D/Ctrl+Shift+J): 탭 내부 분할, 하나의 탭으로 표시. 기본 구현 완료.

### CLI 도구
`tasty` 명령으로 워크스페이스 생성, 알림 전송, 키 입력 등을 자동화. IPC로 실행 중인 GUI 앱과 통신.

**현재 구현된 기능:**
- clap 기반 서브커맨드: list, new-workspace, select-workspace, send, send-key, notify, notifications, tree, split, new-tab, surfaces, panes, info
- 포트 파일(`~/.config/tasty/tasty.port`) 기반 자동 연결
- 서브커맨드 없으면 GUI 모드, 있으면 CLI 모드
- 상세: [features.md](features.md)

### 소켓 API
외부 프로그램이 tasty를 제어할 수 있는 JSON-RPC IPC 인터페이스. 윈도우/레이아웃/외형 등 풍부한 제어 가능.

**현재 구현된 기능:**
- TCP 기반 JSON-RPC 2.0 서버 (127.0.0.1, 랜덤 포트)
- 20개 메서드: system.info, workspace.list/create/select, pane.list/split, tab.list/create, surface.list/send/send_key/set_mark/read_since_mark, notification.list/create, tree, hook.set/list/unset, claude.launch
- 메인 스레드 채널 통신으로 스레드 안전한 상태 접근
- 앱 시작 시 자동 기동, 종료 시 포트 파일 자동 삭제
- 상세: [features.md](features.md)

### 세션 복원
앱 재시작 시 워크스페이스 레이아웃, 작업 디렉토리, 스크롤백, 윈도우 위치/크기를 복원.

### 명령 팔레트
VS Code 스타일 GPU 렌더링 명령 팔레트. 퍼지 검색, 카테고리 필터링, 키보드 단축키 표시.

### 키보드 단축키
winit 기반 네이티브 키 입력 처리. 커스터마이징 가능한 단축키 시스템.

### 검색
GPU 렌더링 검색 바 오버레이. 스크롤백 텍스트 검색, 정규식, 결과 하이라이트.

### 클립보드 통합
OS 클립보드 직접 접근. arboard 크레이트 기반 크로스 플랫폼 클립보드.

**현재 구현된 기능:**
- arboard 기반 시스템 클립보드 읽기/쓰기
- 붙여넣기: Ctrl+V (Windows), Ctrl+Shift+V (Linux), Alt+V (macOS). 설정에서 개별 활성화/비활성화
- 브래킷 붙여넣기 모드 (DECSET 2004) 지원
- OSC 52 클립보드 설정: 터미널 프로그램이 시스템 클립보드에 텍스트 설정 가능
- 상세: [features.md](features.md)

### IME 지원
winit의 IME 이벤트 처리로 CJK (한국어/중국어/일본어) 입력기 직접 지원. 조합 문자 인라인 표시.

### 포트 스캐닝
셸 프로세스가 리스닝하는 포트를 감지하여 사이드바에 표시.

### 원격 SSH
SSH를 통한 원격 서버 워크스페이스 연결.

### 자동 업데이트
GUI 업데이트 다이얼로그. 새 버전 자동 감지, 다운로드 진행률 표시, 원클릭 업데이트.

### 설정 시스템
TOML 기반 설정 파일 + GUI 설정 윈도우. 라이브 리로드.

### Claude Code 통합
Claude Code 훅 연동, 활동 상태 추적, 전용 런처, 멀티 에이전트 워크플로우.

### 마크다운 뷰어
GPU 가속 리치 마크다운 렌더링. 코드 블록 신택스 하이라이팅, 이미지 인라인 표시, 라이브 리로드.

### 윈도우 관리
다중 OS 윈도우, 전체화면, 윈도우 위치/크기 기억, 포커스 관리, 멀티 모니터 지원.

### 복사 모드
마우스 드래그 선택 + vi 스타일 키보드 복사 모드. 셀 단위 정밀 선택.

### Surface Hook
Surface별 이벤트 훅 등록 API. 프로세스 종료, 출력 패턴 매칭 등의 이벤트에 명령을 바인딩하여 에이전트 자동화를 지원한다.

**현재 구현된 기능:**
- HookManager: 훅 등록/삭제/조회/실행 (hooks.rs)
- HookEvent: ProcessExit, OutputMatch(regex), Bell, Notification, IdleTimeout
- once 옵션, 백그라운드 스레드 실행, 이벤트 루프 자동 통합
- CLI: set-hook, list-hooks, unset-hook
- IPC: hook.set, hook.list, hook.unset
- 상세: [features.md](features.md)

### Read Mark API
터미널 출력에 마크를 설정하고 마크 이후의 새 출력만 효율적으로 읽는 델타 트래킹 API. 에이전트가 명령 결과만 추출할 때 사용한다.

**현재 구현된 기능:**
- output_buffer: 최대 1MB 순환 버퍼, 마크 오프셋 자동 조정
- set_mark / read_since_mark: 바이트 오프셋 기반 델타 트래킹
- ANSI 이스케이프 제거 옵션 (regex 기반)
- Surface ID로 특정 터미널 대상 지정 가능
- CLI: set-mark, read-since-mark
- IPC: surface.set_mark, surface.read_since_mark
- 상세: [features.md](features.md)

### 에이전트 자동화
AI 에이전트 간 자동화 통합 기능. Claude Code 전용 런처, 멀티 에이전트 배치 실행, 에이전트 상태 추적 및 사이드바 표시. tasty의 핵심 차별점으로 "에이전트가 에이전트를 제어하는 자동화"를 제공한다.

**현재 구현된 기능:**
- Claude Code 런처: 새 워크스페이스 생성 + 디렉토리 이동 + claude 실행
- CLI: tasty claude --workspace NAME --directory DIR --task TASK
- IPC: claude.launch 메서드
- Surface Hook + Read Mark API와 조합하여 완전한 에이전트 자동화 파이프라인 구성 가능
- 상세: [features.md](features.md)
