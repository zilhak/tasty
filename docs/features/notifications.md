# 알림 시스템

- **Status**: Implemented

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
- focus와 무관하게 동작 (policies/focus.md §6 참조). 상세: `docs/design/policies/busy-indicator.md`

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
- CoreState::collect_events()가 모든 워크스페이스의 모든 터미널에서 이벤트 수집
- PTY drain 은 `AppEvent::TerminalOutput` 핸들러가 수행 — targeted wake 는 `Core::process_pty_output`(해당 surface 만), default wake 는 `Core::process_all_pty_output`(전 engine)
- 같은 핸들러가 drain 직후 TerminalEvent → CoreEvent 변환과 알림 cascade 까지 처리 (redraw 는 렌더링만 담당)
- 윈도우 포커스 상태 추적으로 시스템 알림 발송 조건 판단

### 터미널 뷰포트 관리
- egui 사이드바를 제외한 전체 영역에 상위 레이아웃(PaneNode 트리) 렌더링
- PaneNode에서 각 Pane의 rect를 계산, 탭 바 높이를 뺀 영역에 터미널 렌더링
- 탭 바 높이는 egui 렌더링 시 실측된 값을 사용 (하드코딩 아님)
- 리사이즈 시 모든 Pane, 모든 Tab, 모든 Surface의 행/열 재계산
- wgpu RenderPass의 forget_lifetime()을 이용한 egui-wgpu 호환
