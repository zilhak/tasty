# Tasty 문서 인덱스

크로스 플랫폼 GPU 가속 네이티브 터미널 에뮬레이터. 본 인덱스는 현재 상태 문서로 진입하는 시작점이다. 구현된 기능의 상세는 [features.md](features.md), 설계는 `design/`, 아키텍처는 `architecture/`, 개발 가이드는 `dev-guide/`, 에이전트용 가이드는 `agent-guide/` 에 있다.

## 설치

| 문서 | 설명 |
|------|------|
| [installation.md](installation.md) | 사용자/에이전트 설치 가이드 — OS·아키텍처별 산출물, 설치 방법, 설치 위치 |

## 구현 현황 빠른 안내

본 인덱스의 옛 "기능 목록 표" 는 구현 완료된 항목의 옛 기획 링크로 채워져 있었다. 현재는 모두 [features.md](features.md) 에 흡수되어 있으니, 어떤 기능이 어떻게 구현되어 있는지 확인하려면 그쪽을 본다. (옛 docs/plans/* 파일은 제거되었으며, 아직 미구현인 기획은 `.claude-workspace/plans/archived-from-docs/` 로 옮겨졌다.)

## AI 에이전트 가이드

### 사용자의 AI 에이전트용 (Tasty 사용법)

릴리스 에셋으로 배포. AI 에이전트가 Tasty를 IPC/CLI로 조작하기 위한 가이드.

| 문서 | 설명 |
|------|------|
| [agent-guide/index.md](agent-guide/index.md) | 개요 + 환경별 링크 |
| [agent-guide/api-reference.md](agent-guide/api-reference.md) | IPC/CLI 전체 레퍼런스 |
| [agent-guide/clipboard.md](agent-guide/clipboard.md) | 클립보드 히스토리 (tool.clipboard.*) 사용 가이드 |
| [agent-guide/file-handler.md](agent-guide/file-handler.md) | 파일 핸들러 시스템 — detector/handler 등록 + picker + user TOML |
| [agent-guide/plugins.md](agent-guide/plugins.md) | Plugin 설치/관리 (`tasty plugin ...`) 가이드 |
| [agent-guide/event-catalog.md](agent-guide/event-catalog.md) | Surface Hook/Plugin 이벤트 카탈로그 |
| [agent-guide/output.md](agent-guide/output.md) | 터미널 출력 구조화 (parse_since_mark / commands / observer) |
| [agent-guide/output-parsers.md](agent-guide/output-parsers.md) | 출력 파서 카탈로그 (tasty-output 빌트인 10종) |
| [agent-guide/approval.md](agent-guide/approval.md) | 휴먼 핸드오프 — approval / diff surface |
| [agent-guide/telemetry.md](agent-guide/telemetry.md) | 텔레메트리 — 관측 / 비용 / 이상 탐지 / 세션 요약 |
| [agent-guide/agent.md](agent-guide/agent.md) | 다중 에이전트 협업 — task DAG / barrier / semaphore / lease / reducer / rate-limit |
| [agent-guide/capabilities.md](agent-guide/capabilities.md) | 권한 / capability_elevation / audit log |
| [agent-guide/blackboard.md](agent-guide/blackboard.md) | 공유 컨텍스트 — Blackboard (`memory.bb_*`, snapshot 포함) |
| [agent-guide/plan.md](agent-guide/plan.md) | 공유 컨텍스트 — Plan (`memory.plan_*`) + [plan.schema.json](agent-guide/plan.schema.json) |
| [agent-guide/cache.md](agent-guide/cache.md) | 공유 컨텍스트 — Cache (`memory.cache_*`) |
| [agent-guide/lua-hooks.md](agent-guide/lua-hooks.md) | `~/.tasty/init.lua` 사용자 hook 가이드 — 등록·이벤트 목록·예제 |
| [agent-guide/themes.md](agent-guide/themes.md) | 테마 파일 추가/관리 — `~/.tasty/themes/*.toml` partial TOML 포맷 |
| [agent-guide/linux.md](agent-guide/linux.md) | Linux 사용 가이드 |

### 개발 AI 에이전트용 (Tasty 개발 가이드)

이 프로젝트를 개발하는 AI 에이전트를 위한 가이드. 빌드, 디버깅, UI 검증 등.

| 문서 | 설명 |
|------|------|
| [dev-guide/index.md](dev-guide/index.md) | 개요 + 환경별 링크 |
| [dev-guide/build.md](dev-guide/build.md) | 워크스페이스 구조, 빌드 프로필(dev/release/dist), LTO, 빌드 시간 측정 |
| [dev-guide/dist-build.md](dev-guide/dist-build.md) | manual dist 빌드 명령 카탈로그 (macOS/Windows/Linux) |
| [dev-guide/release.md](dev-guide/release.md) | 릴리스 절차 (버전 → 체인지로그 → 태그 → push) |
| [dev-guide/release-runners.md](dev-guide/release-runners.md) | self-hosted 러너 인벤토리, 1회 도구 설치, 운영 명령 |
| [dev-guide/linux.md](dev-guide/linux.md) | Linux 개발 환경 가이드 |
| [dev-guide/context-menu.md](dev-guide/context-menu.md) | 우클릭 컨텍스트 메뉴 (네이티브 메뉴 필수, PendingNativeMenu 패턴) |
| [dev-guide/popup-implementation.md](dev-guide/popup-implementation.md) | Popup 구현 (PopupDef 시스템, `egui::Window` 직접 사용 금지) |
| [dev-guide/gpu-rendering.md](dev-guide/gpu-rendering.md) | GPU 렌더링 구조 (공유 버퍼 + submit 분리 규칙) |
| [dev-guide/model-view-split.md](dev-guide/model-view-split.md) | Model + Host View 분리 패턴 (GUI-free 도메인 유지) |
| [dev-guide/debug-ipc.md](dev-guide/debug-ipc.md) | Debug 빌드 전용 IPC 메서드 (사용자 입력 재현, popup 트리거) |
| [dev-guide/crash-diagnostics.md](dev-guide/crash-diagnostics.md) | Crash & 에러 진단 (로그, strace, gdb) |
| [dev-guide/tui-testing.md](dev-guide/tui-testing.md) | TUI 테스트 — 터미널 에뮬레이션 버그 재현 및 자동 검증 |
| [dev-guide/cli-naming.md](dev-guide/cli-naming.md) | CLI 명령 네이밍 규칙 |
| [dev-guide/ipc-stability.md](dev-guide/ipc-stability.md) | IPC 메서드 안정성 정책 |
| [dev-guide/unsafe-checklist.md](dev-guide/unsafe-checklist.md) | unsafe 블록 작성 체크리스트 |
| [dev-guide/plugin-development.md](dev-guide/plugin-development.md) | Plugin 제작 가이드 — 크레이트 골격, Plugin trait, UI 빌더, snapshot/restore, 빌드/설치 |
| [dev-guide/plugin-permissions.md](dev-guide/plugin-permissions.md) | Plugin 권한 모델 — method_meta, CallerContext, grant/revoke 흐름 |
| [dev-guide/plugin-ecosystem.md](dev-guide/plugin-ecosystem.md) | Plugin 생태계 — 번들 plugin 목록과 책임 분담 |
| [dev-guide/lua-hooks.md](dev-guide/lua-hooks.md) | Lua hook 호스트 매핑 — 이벤트별 발화 site / payload 스키마 / 추가 가이드 |
| [dev-guide/git-hooks.md](dev-guide/git-hooks.md) | pre-commit / pre-push 훅 규칙 — 설치, 검사 목록, 예외 |
| [dev-guide/i18n.md](dev-guide/i18n.md) | 국제화 정책 — `t()` API, lang 파일 위치, 새 문자열 추가 절차 |
| [dev-guide/error-handling.md](dev-guide/error-handling.md) | 에러 처리 정책 — `Result` 무시 금지, `tracing::warn!`/`error!` 사용 규칙 |
| [dev-guide/commit-convention.md](dev-guide/commit-convention.md) | Conventional Commits 형식, type 목록, 단위 분할 기준 |

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
| [design/action-dispatch.md](design/action-dispatch.md) | Action Dispatch (Intent 큐) — 호스트 내부 동작 디스패치, User/Agent origin, Event Bus bridge |
| [design/intent-coroutine.md](design/intent-coroutine.md) | Intent Coroutine Runtime — 경량 thread + genawaiter 기반 multi-step workflow. 일반 Intent 큐 모델의 보강 |
| [design/toast-system.md](design/toast-system.md) | 토스트 시스템 — 휘발성 인앱 알림, 스코프, 사용자 행동 트리거 정책 |
| [design/settings-system.md](design/settings-system.md) | 설정 시스템 — 탭/서브탭 구조, 항목 순서 규칙, 배치 판단 기준 |
| [design/input-layer.md](design/input-layer.md) | 마우스 입력 계층 — z-order 기반 이벤트 소비/버블링 설계 |
| [design/typed-length.md](design/typed-length.md) | 타입 안전 길이 시스템 — PhysicalPx/LogicalPx newtype, DPI 혼동 방지 |
| [design/cwd-policy.md](design/cwd-policy.md) | CWD 정책 — OSC 7 기반 CWD 감지 (모든 플랫폼 공통) |
| [design/explorer-context-menu.md](design/explorer-context-menu.md) | Explorer 컨텍스트 메뉴 — 우클릭 대상 결정, 메뉴 항목 분기, 액션 동작 정의 |
| [design/linux-system-tray.md](design/linux-system-tray.md) | Linux 시스템 트레이 미지원 결정 — DE 분열, GNOME 기본 미지원, 태스크바 유지로 충분 |
| [design/busy-indicator.md](design/busy-indicator.md) | 실행 중 표시 — 탭/워크스페이스 busy 판정 정책, 시각 표시, 플랫폼별 foreground 감지 |
| [design/memory-system.md](design/memory-system.md) | 에이전트 메모리 — regular/secret 두 계층, owner 자동 도출, plugin 별 사전 분할 |
| [design/lua-hooks.md](design/lua-hooks.md) | Lua hook 설계 — host 전용·observe-only·event matrix·사용자-직접 변경 의미 |

## 아키텍처 문서

| 문서 | 설명 |
|------|------|
| [아키텍처 개요](architecture/index.md) | 워크스페이스 크레이트 14개, 본 바이너리 모듈 구조, 의존성 DAG |
| [모듈별 상세](architecture/modules.md) | 디렉토리 모듈별 책임, 설계 목적, 한계 |
| [데이터 흐름](architecture/data-flows.md) | 5가지 주요 데이터 흐름 (파일+함수 기준) |
| [리팩토링 분석](architecture/refactoring.md) | 남아있는 개선 가능성, 우선순위별 로드맵 |
| [라이브러리 분리](architecture/library-separation/index.md) | 워크스페이스 28 crate 현황 + 분리 의사결정 회고 |

## 구현 현황

구현된 기능의 상세 설명은 [features.md](features.md) 참조.

GPU 렌더링·테스트·설치 같은 횡단 주제는 dev-guide 와 design 의 해당 문서로 흡수되었다 (예: `dev-guide/gpu-rendering.md`, `dev-guide/tui-testing.md`, `installation.md`). 아직 미구현인 횡단 기획(접근성 / 추가 에러 정책 등)은 `.claude-workspace/plans/archived-from-docs/` 에서 확인할 수 있다.

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
cmux 분석 기반 계층적 데이터 모델. Workspace → Pane (상위 레이아웃, PaneNode 트리) → Tab (SurfaceLayout 트리) → Surface.

**현재 구현된 기능:**
- Workspace / PaneNode / Pane / Tab / SurfaceLayout 계층 데이터 모델
- egui 좌측 사이드바 (워크스페이스 목록) + Pane별 독립 탭 바
- 두 가지 분할: Pane 분할(물리적 화면, 독립 탭 바) + Surface 분할(탭 내부)
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
두 가지 분할 지원. Pane 분할(Alt+E/Shift+E): 물리적 화면 분할, 새 독립 탭 바 생성. Surface 분할(Alt+D/Shift+D): 탭 내부 분할, 하나의 탭으로 표시. 기본 구현 완료.

### CLI 도구
`tasty` 명령으로 워크스페이스 생성, 알림 전송, 키 입력 등을 자동화. IPC로 실행 중인 GUI 앱과 통신.

**현재 구현된 기능:**
- clap 기반 서브커맨드 그룹: `new {window|workspace|tab}`, `close {tab|pane|surface|self}`, `list {workspaces|windows|tree|surfaces|panes|tabs|info|notifications|hooks|global-hooks|queue}`, `set {hook|mark|workspace|global-hook}`, `move {tab|workspace}`, `unset {hook|global-hook}`, `send {text|key|queue}`, `read {since-mark|parse-since-mark|queue|screen|commands|last-command|command-at}`, `split`, `notify`, `surface-meta {set|get|unset|list}`, `is-typing`, `wake`, `tool clipboard {...}`, `plugin {list|show|install|remove|enable|disable|logs|permissions|grant|revoke|extension}`, `memory {put|get|delete|list|exists|count|scopes|stats}`, `output observe {start|stop|list|info}`. debug 빌드에서만 `debug {info|cell-info|screen-attrs|glyph-color|feed-bytes|inject-key|inject-mouse|ime-...|tool|popup|extension|event-bus|switch-input-source|raw-key}` 가 추가된다. 번들 plugin이 자체적으로 `claude {launch|spawn|children|parent|kill|respawn|broadcast|wait|install|uninstall|hook|tell}`, `codex {...}`, `image {open|save|export|next|prev|paste|list}` 등을 등록한다.
- 포트 파일(`~/.tasty/tasty.port`) 기반 자동 연결
- 서브커맨드 없으면 GUI 모드, 있으면 CLI 모드
- 상세: [features.md](features.md)

### 소켓 API
외부 프로그램이 tasty를 제어할 수 있는 JSON-RPC IPC 인터페이스. 윈도우/레이아웃/외형 등 풍부한 제어 가능.

**현재 구현된 기능:**
- TCP 기반 JSON-RPC 2.0 서버 (127.0.0.1, 랜덤 포트)
- 프로덕션 메서드: system.info, tree, split, workspace.{list,create,update,move}, window.{list,create,close,focus}, pane.{list,close}, tab.{list,create,close,move}, surface.{list,close,close_self,send,send_key,send_combo,send_to,set_mark,read_since_mark,parse_since_mark,commands,last_command,command_at,screen_text,cursor_position,is_typing,send_wait_idle,fire_hook,foreground_process,locate,respawn_terminal,wake,switch_input_source,raw_key,meta.{set,get,unset,list},ime_{enable,disable,preedit,commit,status}}, notification.{list,create}, hook.{set,list,unset}, global_hook.{set,list,unset}, message.{send,read,count,clear}, tool.clipboard.{list,get,paste,remove,clear}, image.{open,save,export_png,next,prev,paste,list}, plugin.{list,show,install,remove,enable,disable,permissions,grant,revoke,extension.list}, memory.{put,get,delete,list,exists,count,scopes,stats,query,export,import,gc}, memory.secret.{put,get,delete,list,exists,count,scopes,stats}, output.observe_{start,stop,list,info}. 번들 plugin이 `claude.{launch,spawn,children,parent,kill,respawn,broadcast,wait,hook,tell,install,uninstall,set_idle_state,set_needs_input}`, `codex.*` namespace를 자체 등록 (사용자 시점에서는 동일한 IPC dispatch)
- 디버그 전용 메서드 (debug 빌드에서만): system.shutdown, ui.state, ui.screenshot, debug.{info,cell_info,screen_attrs,glyph_color,feed_bytes,inject_key,inject_mouse,tool.{list,invoke},popup.{list,open,close},extension.invoke_hook,event_bus.{list_subscribers,publish,trace}}
- 메인 스레드 채널 통신으로 스레드 안전한 상태 접근
- 앱 시작 시 자동 기동, 종료 시 포트 파일 자동 삭제
- 헤드리스 모드: `--headless` 플래그로 GUI 없이 IPC 전용 실행 (E2E 테스트/CI 활용)
- IPC Waker: IPC 명령 도착 시 `EventLoopProxy`로 이벤트 루프 즉시 깨움
- E2E 테스트 프레임워크: `TastyInstance` 헬퍼 기반 14개 통합 테스트 (헤드리스)
- GUI 통합 테스트 프레임워크: `GuiTestInstance` 헬퍼 기반 24개 GUI 테스트 (enigo 입력 시뮬레이션 + IPC 검증 + 속도 측정)
- 상세: [features.md](features.md)

### 세션 복원
앱 재시작 시 워크스페이스 레이아웃, 작업 디렉토리, 스크롤백, 윈도우 위치/크기를 복원. TUI 앱의 세션은 plugin이 surface 메타데이터 `restore.command` 키에 복원 명령을 set하면 자동 재개된다 (Claude Code의 경우 `com.tasty.claude` plugin이 `claude -r <session-id>`를 set).

### 명령 팔레트
VS Code 스타일 GPU 렌더링 명령 팔레트. 퍼지 검색, 카테고리 필터링, 키보드 단축키 표시.

### 키보드 단축키
winit 기반 네이티브 키 입력 처리. 커스터마이징 가능한 단축키 시스템. macOS에서 바인딩의 `alt` 토큰은 Cmd(⌘)에 매핑되어, 물리적 키 위치가 Windows/Linux의 Alt와 일치한다.

### 검색
터미널 텍스트 검색. 스크롤백 + 화면 전체를 대상으로 검색하며, 매치를 GPU 하이라이트로 표시. Ctrl+F (macOS: Cmd+F)로 검색 바 열기, Enter/Shift+Enter로 매치 탐색, 대소문자 감도 토글 지원.

### 클립보드 통합
OS 클립보드 직접 접근. arboard 크레이트 기반 크로스 플랫폼 클립보드.

**현재 구현된 기능:**
- arboard 기반 시스템 클립보드 읽기/쓰기
- 텍스트 선택: 마우스 드래그(Normal), 더블클릭(Word), 트리플클릭(Line) 모드. 스크롤백/화면 영역 통합. 선택 영역 시각적 하이라이트
- 복사: Ctrl+C (Windows, 선택 시 복사 / 미선택 시 SIGINT), Ctrl+Shift+C (Linux), Alt+C (macOS). 설정에서 개별 활성화/비활성화. 셸 자동 줄바꿈된 명령은 한 줄로 다시 합쳐서 복사 (soft-wrap aware)
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

### 업데이트 알림
GitHub Releases 폴링으로 새 버전을 감지하고 popup 에 현재/최신 버전 + 릴리스 노트를 표시한다. **자체 다운로더/설치는 지원하지 않으며**, `Open release page` 버튼이 브라우저로 GitHub 릴리스 페이지를 열어 사용자가 직접 다운로드한다. 상세: [features.md "자동 업데이트 확인"](features.md).

### 설정 시스템
TOML 기반 설정 파일 + GUI 설정 윈도우. 라이브 리로드.

### Claude Code 통합 (com.tasty.claude plugin)
Claude Code 훅 연동, 활동 상태 추적(idle/needs_input/active), 전용 런처, 멀티 에이전트 워크플로우.
번들 plugin `com.tasty.claude`로 제공되며 호스트는 plugin 등록만 처리한다. `tasty claude hook`
CLI 서브커맨드로 Claude Code의 훅 시스템에서 직접 호출 가능.

### 레이아웃 프리셋 (Layout Presets)
Workspace / Tab / Pane 레이아웃과 leaf surface 초기화 파라미터(kind, cwd, 시작 명령어, kind 별 params)를 디스크(`~/.tasty/presets/{workspace,tab,pane}/<name>.toml`)에 저장/재사용. 우클릭 메뉴/단축키로 저장·적용, PresetWindow 에서 편집, IPC `preset.*` 7종 + CLI `tasty preset {list,get,save,delete,rename,capture,apply}` 노출. CLI/IPC apply 는 항상 포커스 이동 없음. 상세: [features.md](features.md).

### 마크다운 뷰어 & 파일 탐색기
egui 기반 추가 Surface 타입. 마크다운 뷰어(제목/목록/인용/코드 블록/인라인 서식 렌더링)와 파일 탐색기(트리 + 미리보기)를 탭으로 열 수 있다. IPC/CLI/우클릭 컨텍스트 메뉴로 사용 가능.

### 이미지 뷰어 & 그림판 (com.tasty.image plugin)
egui 기반 Image Surface 타입. 이미지 파일을 로드하여 표시(PNG/JPEG/BMP/WebP/ICO/TIFF), 폴더 내 이전/다음 탐색, 줌/팬, 편집 모드(연필 드로잉, 브러시 크기/색상 조절), PNG 저장, 새 이미지(빈 캔버스) 생성을 지원한다. 번들 plugin `com.tasty.image`가 surface kind 등록(`rendering = "host"`)과 `image.*` IPC 네임스페이스를 점유하며, `tasty image {open|save|export|next|prev|paste|list}` CLI를 노출한다. 픽셀 렌더링과 편집은 호스트 본문이 담당한다.

**현재 구현된 기능:**
- Panel enum에 Markdown/Explorer/Html/Empty 변형 추가 (egui 렌더링 패널)
- 마크다운 렌더링: 제목, 목록, 인용, 코드 블록, 테이블, 인라인 서식(**볼드**, *이탤릭*, \`코드\`)
- 파일 탐색기: 트리 뷰 + 파일 미리보기 (마크다운 렌더링 또는 모노스페이스 텍스트)
- 패인 우클릭 또는 탭 바 빈 공간 우클릭 컨텍스트 메뉴: Open Markdown... / Open Explorer
- 키보드 단축키: open_markdown, open_explorer (설정 UI에서 바인딩)
- IPC: tab.create에 type 파라미터로 통합 (terminal/markdown/explorer)
- CLI: tasty new markdown, tasty new explorer
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
- HookEvent: ProcessExit, OutputMatch(regex), Bell, Notification, IdleTimeout, ClaudeIdle, NeedsInput, ClaudeError
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

**현재 구현된 기능 (번들 plugin 제공):**
- `com.tasty.claude` plugin: Claude Code 런처, parent-child 관계 관리, hook 통합 전체
- `com.tasty.codex` plugin: Codex CLI 런처, parent-child 관계 관리, hook 통합 전체
- `com.tasty.image` plugin: image surface kind 등록(`rendering = "host"`) + `image.*` IPC trampoline + `tasty image *` CLI
- 호스트는 plugin 등록만 처리. claude.*/codex.* IPC와 `tasty claude *` / `tasty codex *` CLI는 plugin이 자체 노출
- CLI: tasty claude launch --workspace NAME --directory DIR --task TASK
- CLI: tasty claude spawn/children/parent/kill/respawn/broadcast/wait
- CLI: tasty claude install / uninstall (~/.claude/settings.json에 Stop/Notification/SessionEnd/SubagentStop 4종 등록·제거; wait/이벤트 훅 사용 전 필수)
- CLI: tasty claude hook stop|notification|session-end|subagent-stop|prompt-submit|session-start
- tasty claude wait는 시작 시 Stop 훅 설치 여부를 점검하여 미설치 시 안내 메시지와 함께 즉시 종료
- IPC: claude.launch, claude.spawn, claude.children, claude.parent, claude.kill, claude.respawn, claude.broadcast, claude.wait, claude.hook 메서드 (plugin이 노출)
- 권한 토큰: 다른 plugin이 claude namespace를 호출하려면 `ipc.invoke:claude`가 필요
- Surface Hook + Read Mark API와 조합하여 완전한 에이전트 자동화 파이프라인 구성 가능
- 상세: [features.md](features.md)

### 파일 핸들러 시스템
경로/URI 입력을 형식 식별(`FileFormatRegistry`) → 핸들러 디스패치(`FileHandlerRegistry`) 두 단계로 라우팅. host default + plugin contribute + user TOML(`~/.tasty/file-handlers.toml`) 통합. 사용자가 직접 핸들러를 고를 수 있는 picker popup 과 LRU 캐시(`~/.tasty/file-handler-recent.json`) 제공.

**현재 구현된 기능:**
- DetectorRule 종류: `extension`, `path_glob`, `is_directory` (cheap) / `mime`, `magic`, `lua` (deep, 평가 구현됨) / `structure_check` (stub — Phase D MD2 예정)
- HandlerAction: `OpenSurface { kind, param_key }`, `Ipc { method, owner_plugin_id }`, `System` (OS 기본 열기 — plugin 불가)
- Contribution-based registry: plugin uninstall 시 그 plugin 의 entry 만 제거, host/user 유지
- Plugin 매니페스트 `[[contributes.detector]]` / `[[contributes.handler]]` + 권한 토큰 `file_handler.define` / `extend:<id>` / `handle:<id>` — 빌트인 예시: `com.tasty.image` (image detector + viewer), `com.tasty.html` (viewer handler 만, detector 는 host 유지)
- Picker popup (`file_handler_picker` PopupDef): 후보/최근 두 열, 더블클릭/[열기] dispatch, [취소]/ESC Cancelled
- Extension Mapping (Phase E): 같은 확장자를 광고하는 detector 가 여러 개일 때 사용자가 `[[extension_priority]]` 표로 우선순위 지정. Settings UI 의 File Handler 탭 → Extension Mapping sub-tab 에서 ↑/↓ 버튼으로 편집
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
