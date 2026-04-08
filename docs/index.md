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
| 25 | [국제화 (i18n)](#국제화-i18n) | - | 중요 |
| 26 | [타이핑 감지](#타이핑-감지) | - | 중요 |
| 27 | [비터미널 패널](#마크다운-뷰어--파일-탐색기) | - | 부가 |
| 28 | [Crash Report & 진단](#crash-report--진단) | - | 핵심 |

## AI 에이전트 가이드

### 사용자의 AI 에이전트용 (Tasty 사용법)

릴리스 에셋으로 배포. AI 에이전트가 Tasty를 IPC/CLI로 조작하기 위한 가이드.

| 문서 | 설명 |
|------|------|
| [agent-guide/index.md](agent-guide/index.md) | 개요 + 환경별 링크 |
| [agent-guide/api-reference.md](agent-guide/api-reference.md) | IPC/CLI 전체 레퍼런스 |
| [agent-guide/linux.md](agent-guide/linux.md) | Linux 사용 가이드 |

### 개발 AI 에이전트용 (Tasty 개발 가이드)

이 프로젝트를 개발하는 AI 에이전트를 위한 가이드. 빌드, 디버깅, UI 검증 등.

| 문서 | 설명 |
|------|------|
| [dev-guide/index.md](dev-guide/index.md) | 개요 + 환경별 링크 |
| [dev-guide/linux.md](dev-guide/linux.md) | Linux 개발 환경 가이드 |

## AI 자체 검증 지침

| 문서 | 설명 |
|------|------|
| [ai-verification/visual-verification.md](ai-verification/visual-verification.md) | UI 변경 시 색상 대비, 레이어 순서, 픽셀 수치 검증 규칙 |
| [ai-verification/screenshot-methods.md](ai-verification/screenshot-methods.md) | GUI 스크린샷 촬영 방법 (IPC / PowerShell) |
| [ai-verification/egui-layout.md](ai-verification/egui-layout.md) | egui 레이아웃, 레이어 순서 주의사항 |
| [ai-verification/state-none-gpu-separation.md](ai-verification/state-none-gpu-separation.md) | state None 시 GPU 호출 분리 패턴 |
| [ai-verification/ipc-usage.md](ai-verification/ipc-usage.md) | IPC를 통한 Tasty 조작 방법 |
| [ai-verification/python-execution.md](ai-verification/python-execution.md) | Windows에서 python 실행 주의 |
| [ai-verification/tcp-communication.md](ai-verification/tcp-communication.md) | TCP 통신 도구 (Python socket) |
| [ai-verification/windows-process-cleanup.md](ai-verification/windows-process-cleanup.md) | Windows 프로세스 트리 종료 |
| [ai-verification/ime-testing.md](ai-verification/ime-testing.md) | IME 시뮬레이션을 이용한 디버깅 가이드 |

## 디자인 문서

| 문서 | 설명 |
|------|------|
| [design/theme-system.md](design/theme-system.md) | 테마 시스템 — 색상, 타이포그래피, 간격, 크기 규칙 |
| [design/multi-window-architecture.md](design/multi-window-architecture.md) | 멀티 윈도우 아키텍처 — 엔진/윈도우/모달 구조 |
| [design/focus-policy.md](design/focus-policy.md) | 포커스 정책 — 윈도우/모달 간 입력 라우팅 규칙 |
| [design/ubiquitous-language.md](design/ubiquitous-language.md) | 유비쿼터스 언어 — 용어 정의, 계층 구조, 코드 매핑 |
| [design/layout-concept.md](design/layout-concept.md) | 두 레벨 레이아웃 — 상위(고정)/하위(탭 종속) 분할 설계 |
| [design/split-command.md](design/split-command.md) | Split 명령어 설계 — 통합 split 명령, 레벨/대상/방향 파라미터, 포커스 정책 |
| [design/key-mapping.md](design/key-mapping.md) | 키 매핑 설계 — OS별 물리적 키 위치 매핑, 프리셋, 캡처/매칭 규칙 |
| [design/popup-system.md](design/popup-system.md) | 내부 팝업 시스템 — 공통 규칙 7가지, PopupManager 구조 |

## 아키텍처 문서

| 문서 | 설명 |
|------|------|
| [아키텍처 개요](architecture/index.md) | 91개 파일 모듈 구조, 의존성 DAG, 계층 |
| [모듈별 상세](architecture/modules.md) | 디렉토리 모듈별 책임, 설계 목적, 한계 |
| [데이터 흐름](architecture/data-flows.md) | 5가지 주요 데이터 흐름 (파일+함수 기준) |
| [리팩토링 분석](architecture/refactoring.md) | 남아있는 개선 가능성, 우선순위별 로드맵 |
| [라이브러리 분리](architecture/library-separation/index.md) | 크레이트 분리 후보 다관점 분석 |

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
- **IPC**: TCP (127.0.0.1, 동적 포트, ~/.tasty/tasty.port)
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
cmux 분석 기반 계층적 데이터 모델. Workspace → Pane Group (상위 레이아웃, PaneNode 트리) → Pane (탭) → Surface (하위 레이아웃, SurfaceGroupNode 트리).

**현재 구현된 기능:**
- Workspace / PaneNode / Pane / Tab / Panel / SurfaceGroupNode 계층 데이터 모델
- egui 좌측 사이드바 (워크스페이스 목록) + Pane별 독립 탭 바
- 두 가지 분할: Pane 분할(물리적 화면, 독립 탭 바) + SurfaceGroup 분할(탭 내부)
- 키보드 단축키: Alt+N(워크스페이스), Alt+T(탭), Alt+E/Shift+E(Pane분할), Alt+D/Shift+D(Surface분할), Alt+1~9(WS전환), Ctrl+1~0(탭전환), Ctrl+Tab/Shift+Tab(탭순환). macOS에서 `alt` 바인딩은 Cmd(⌘)로 매핑 (물리적 키 위치 일치)
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
두 가지 분할 지원. Pane 분할(Alt+E/Shift+E): 물리적 화면 분할, 새 독립 탭 바 생성. SurfaceGroup 분할(Alt+D/Shift+D): 탭 내부 분할, 하나의 탭으로 표시. 기본 구현 완료.

### CLI 도구
`tasty` 명령으로 워크스페이스 생성, 알림 전송, 키 입력 등을 자동화. IPC로 실행 중인 GUI 앱과 통신.

**현재 구현된 기능:**
- clap 기반 서브커맨드: list, new-workspace, select-workspace, send, send-key, notify, notifications, tree, split, new-tab, surfaces, panes, info
- 포트 파일(`~/.tasty/tasty.port`) 기반 자동 연결
- 서브커맨드 없으면 GUI 모드, 있으면 CLI 모드
- 상세: [features.md](features.md)

### 소켓 API
외부 프로그램이 tasty를 제어할 수 있는 JSON-RPC IPC 인터페이스. 윈도우/레이아웃/외형 등 풍부한 제어 가능.

**현재 구현된 기능:**
- TCP 기반 JSON-RPC 2.0 서버 (127.0.0.1, 랜덤 포트)
- 39개 메서드: system.info/shutdown, workspace.list/create/select, pane.list/split/close, tab.list/create/close, surface.list/close/send/send_key/set_mark/read_since_mark/screen_text/cursor_position/fire_hook/ime_enable/ime_disable/ime_preedit/ime_commit/ime_status, notification.list/create, tree, hook.set/list/unset, global_hook.set/list/unset, claude.launch/spawn/children/parent/kill/respawn/set_idle_state/set_needs_input
- 메인 스레드 채널 통신으로 스레드 안전한 상태 접근
- 앱 시작 시 자동 기동, 종료 시 포트 파일 자동 삭제
- 헤드리스 모드: `--headless` 플래그로 GUI 없이 IPC 전용 실행 (E2E 테스트/CI 활용)
- IPC Waker: IPC 명령 도착 시 `EventLoopProxy`로 이벤트 루프 즉시 깨움
- E2E 테스트 프레임워크: `TastyInstance` 헬퍼 기반 14개 통합 테스트 (헤드리스)
- GUI 통합 테스트 프레임워크: `GuiTestInstance` 헬퍼 기반 24개 GUI 테스트 (enigo 입력 시뮬레이션 + IPC 검증 + 속도 측정)
- 상세: [features.md](features.md)

### 세션 복원
앱 재시작 시 워크스페이스 레이아웃, 작업 디렉토리, 스크롤백, 윈도우 위치/크기를 복원.

### 명령 팔레트
VS Code 스타일 GPU 렌더링 명령 팔레트. 퍼지 검색, 카테고리 필터링, 키보드 단축키 표시.

### 키보드 단축키
winit 기반 네이티브 키 입력 처리. 커스터마이징 가능한 단축키 시스템. macOS에서 바인딩의 `alt` 토큰은 Cmd(⌘)에 매핑되어, 물리적 키 위치가 Windows/Linux의 Alt와 일치한다.

### 검색
GPU 렌더링 검색 바 오버레이. 스크롤백 텍스트 검색, 정규식, 결과 하이라이트.

### 클립보드 통합
OS 클립보드 직접 접근. arboard 크레이트 기반 크로스 플랫폼 클립보드.

**현재 구현된 기능:**
- arboard 기반 시스템 클립보드 읽기/쓰기
- 텍스트 선택: 마우스 드래그(Normal), 더블클릭(Word), 트리플클릭(Line) 모드. 스크롤백/화면 영역 통합. 선택 영역 시각적 하이라이트
- 복사: Ctrl+C (Windows, 선택 시 복사 / 미선택 시 SIGINT), Ctrl+Shift+C (Linux), Alt+C (macOS). 설정에서 개별 활성화/비활성화
- 붙여넣기: Ctrl+V (Windows), Ctrl+Shift+V (Linux), Alt+V (macOS). 설정에서 개별 활성화/비활성화
- 브래킷 붙여넣기 모드 (DECSET 2004) 지원
- OSC 52 클립보드 설정: 터미널 프로그램이 시스템 클립보드에 텍스트 설정 가능
- 마우스 트래킹 모드에서 Shift+드래그로 강제 선택
- 터미널 영역 위에서 I-beam 마우스 커서 표시
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
Claude Code 훅 연동, 활동 상태 추적(idle/needs_input/active), 전용 런처, 멀티 에이전트 워크플로우. `claude-hook` CLI 서브커맨드로 Claude Code의 훅 시스템에서 직접 호출 가능.

### 마크다운 뷰어 & 파일 탐색기
egui 기반 비터미널 패널. 마크다운 뷰어(제목/목록/인용/코드 블록/인라인 서식 렌더링)와 파일 탐색기(트리 + 미리보기)를 탭으로 열 수 있다. IPC/CLI/우클릭 컨텍스트 메뉴로 사용 가능.

**현재 구현된 기능:**
- Panel enum에 Markdown/Explorer 변형 추가 (비터미널 egui 패널)
- 마크다운 렌더링: 제목, 목록, 인용, 코드 블록, 테이블, 인라인 서식(**볼드**, *이탤릭*, \`코드\`)
- 파일 탐색기: 트리 뷰 + 파일 미리보기 (마크다운 렌더링 또는 모노스페이스 텍스트)
- 패인 우클릭 컨텍스트 메뉴: Open Markdown... / Open Explorer
- IPC: tab.open_markdown, tab.open_explorer
- CLI: tasty open-markdown, tasty open-explorer
- 상세: [features.md](features.md)

### 윈도우 관리
다중 OS 윈도우, 전체화면, 윈도우 위치/크기 기억, 포커스 관리, 멀티 모니터 지원.

### 복사 모드
마우스 드래그 선택 구현 완료. vi 스타일 키보드 복사 모드는 미구현.

**현재 구현된 기능:**
- 마우스 드래그로 문자/단어/줄 단위 텍스트 선택
- 선택 영역 시각적 하이라이트 + 클립보드 복사 단축키 3종
- 상세: [features.md](features.md)

### Surface Hook
Surface별 이벤트 훅 등록 API. 프로세스 종료, 출력 패턴 매칭 등의 이벤트에 명령을 바인딩하여 에이전트 자동화를 지원한다.

**현재 구현된 기능:**
- HookManager: 훅 등록/삭제/조회/실행 (hooks.rs)
- HookEvent: ProcessExit, OutputMatch(regex), Bell, Notification, IdleTimeout, ClaudeIdle, NeedsInput
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
- Claude Parent-Child 관계 관리: 부모 Claude가 자식 Claude를 생성/조회/종료/재시작
- CLI: tasty claude --workspace NAME --directory DIR --task TASK
- CLI: tasty claude-spawn/claude-children/claude-parent/claude-kill/claude-respawn
- CLI: tasty claude-hook stop|notification|prompt-submit|session-start (Claude Code 훅 통합)
- IPC: claude.launch, claude.spawn, claude.children, claude.parent, claude.kill, claude.respawn, claude.set_idle_state, claude.set_needs_input, surface.fire_hook 메서드
- Surface Hook + Read Mark API와 조합하여 완전한 에이전트 자동화 파이프라인 구성 가능
- 상세: [features.md](features.md)

### 국제화 (i18n)
TOML 기반 번역 시스템. 영어/한국어/일본어 내장, 사용자 커스텀 번역 오버라이드 지원. `config.toml`의 `general.language` 필드로 언어 설정.

### 타이핑 감지
서피스별 최근 키 입력 시각을 추적하여 AI 에이전트가 사용자/프로세스가 터미널에 입력 중인지 감지할 수 있는 API.

**현재 구현된 기능:**
- AppState에 `last_key_input: HashMap<u32, Instant>` 로 서피스별 타이핑 시각 추적
- `record_typing(surface_id)`: 키 입력 전송 후 자동 기록
- `is_typing(surface_id)`: 5초 내 입력 여부 반환
- IPC: `surface.is_typing` — `{ typing: bool, idle_seconds: f64 }` 반환
- IPC: `surface.send_wait_idle` — 유휴 상태일 때만 전송, 타이핑 중이면 `{ sent: false, reason: "typing" }` 반환
- CLI: `tasty is-typing [--surface ID]`
- 설정: `general.confirm_close_running` (기본 true) — 프로세스 실행 중 서피스 닫기 시 확인 다이얼로그 표시 여부
- 상세: [features.md](features.md)

**현재 구현된 기능:**
- `include_str!`로 바이너리에 번역 파일 임베드 (en/ko/ja)
- 영어 베이스 + 선택 언어 오버레이 계층 구조
- 사용자 커스텀 번역: `~/.tasty/lang/{code}.toml`
- `OnceLock` 기반 글로벌 번역 스토어
- `t(key)`, `t_fmt(key, arg)`, `current_language()` API
- 상세: [features.md](features.md)
