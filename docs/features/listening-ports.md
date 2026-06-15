# 리스닝 포트 뷰어

- **Status**: Implemented

### 개요
워크스페이스 전체 또는 시스템 전체에서 TCP 포트를 연결 상태(`LISTEN`, `CLOSE_WAIT`, `ESTABLISHED` 등)와 함께 7컬럼 테이블로 표시. 포트 번호 클릭 시 시스템 기본 브라우저에서 `http://<host>:<port>`를 연다. 백그라운드 스레드 비동기 스캔 + Spinner 로딩 상태, 검색·정렬, Tasty / System 토글을 지원한다. popup 크기는 660×520 (디자인 canonical).

### 트리거
- 사이드바 하단 Tools 메뉴 상단에 `Listening ports...` 빌트인 항목
- 클릭 시 `port_scanner` popup 오픈 (`PopupScope::Window`)

### 7컬럼 테이블
`egui_extras::TableBuilder` 기반. 컬럼 순서: **Port / Proto / Address / Process / Workspace / Tab / State**.

- **Port**: 포트 번호. 셀 클릭 시 wildcard(`0.0.0.0`/`[::]`)는 `localhost`로 치환하여 브라우저 열기
- **Proto**: 현재는 `TCP` 고정 (UDP 미지원)
- **Address**: bind 주소 (monospace). v6 는 `[…]` 형식
- **Process**: 프로세스 이름 + `PID …` 배지
- **Workspace**: Tasty 행은 workspace 이름, External 행은 em-dash(`—`)
- **Tab**: Tasty 행이 tab 이름을 가지면 표시, 그 외 em-dash
- **State**: 연결 상태 dot + 실제 state 텍스트. `LISTEN` → 초록(running) dot + pulse 링, 그 외 상태 → 노란(waiting) dot + pulse 없음. `reduced_motion` 시 `LISTEN` 도 정적 dot. dot 색은 `accent_success`(green) / `accent_warning`(yellow)

### 비동기 스캔
- `PortScanState` 머신: `Idle` → (kick) → `Loading { rx, scope }` → (poll) → `Ready { rows, scope }` 또는 `Failed(msg)`
- popup 열림 / scope toggle / Refresh 시 `kick_off_scan` 호출 — 백그라운드 스레드가 스캔 결과를 mpsc 채널로 보냄
- 메인 루프는 매 프레임 `poll_scan` 으로 Loading 채널을 비결합으로 polling, 결과 도착 시 `Ready` 또는 `Failed` 로 전이. 스레드는 결과 송신 후 `request_repaint()` 호출
- popup close → `Idle` 으로 reset. rx drop 으로 송신 쪽 send 가 실패하나 스레드는 자연 종료
- **Loading 상태 표시**: 본문 중앙에 `egui::Spinner` + "Scanning…" 메시지, footer 에 동일 메시지

### 검색
- 헤더 행 우측의 단일 라인 `TextEdit` — 입력 즉시 모든 컬럼에 case-insensitive substring 매칭
- 매칭 대상: `port`, `addr_display`, `pid`, `process_name`, `workspace_name`, `tab_name` (External 행의 workspace/tab 은 빈 문자열로 취급)
- 빈 query 는 모든 행 통과

### 정렬
- 정렬 가능 컬럼: Port / Address / Process / Workspace / Tab (Proto / State 는 정렬 불가)
- 헤더 클릭 시 같은 컬럼이면 Asc ↔ Desc 토글, 다른 컬럼이면 그 컬럼 Asc 로 전환
- 활성 헤더에 `▲` (Asc) / `▼` (Desc) 인디케이터 표시 + 색상 강조
- `None` / 빈 값 (External 의 workspace/tab, 미상 process 등) 은 **방향과 무관하게 항상 tail** (정보 없음을 상위로 끌어올리지 않는 디자인)
- 필터·정렬 상태는 `egui::Memory` 에 `port_scanner.filter` ID 로 영속화

### 전체 보기 토글
- 필터 행 좌측의 `전체 보기 (system)` 체크박스
- `false` (Tasty): Tasty 셸 프로세스 트리의 자손 PID 가 listening 중인 포트만
- `true` (System): host 의 모든 TCP 소켓(전 상태) — Tasty 자손 PID 와 일치하는 행은 `SourceTag::Tasty { workspace_name, tab_name }` 로 태그, 그 외 `SourceTag::External`
- toggle 시 wrapper 가 scope 변경을 감지해 자동 재스캔

### 빈 결과 분기 (3종)
`Ready { rows: empty }` 상태에서 다음 우선순위로 메시지를 표시한다:
1. **search_zero**: query 가 비어있지 않음 → "검색 결과가 없습니다."
2. **system_empty**: `show_all_system = true` + query 없음 → 시스템 전체에서도 0건
3. **tasty_empty**: `show_all_system = false` + query 없음 → 활성 Tasty 트리에 listening 포트 없음

`Loading` / `Failed` 상태는 별도 분기 (Spinner, 에러 + Refresh 버튼).

### OS별 백엔드
모든 플랫폼이 전 연결 상태를 스캔하고 각 소켓의 state 를 `PortState` enum 으로 매핑한다 (`Listen` / `Established` / `CloseWait` / … / `Unknown`).
- **Linux**: `/proc/net/tcp` + `/proc/net/tcp6` 파싱 (state 필터 없음, 16진 state 코드 → `PortState`) → inode → `/proc/{pid}/fd/*` symlink 매칭
- **macOS**: `lsof -nP -iTCP [-p <pids>]` subprocess → human-readable 출력 파싱 (`(STATE)` 토큰 → `PortState`, 연결 소켓은 `local->remote` 의 local 엔드포인트 사용)
- **Windows**: `GetExtendedTcpTable` Win32 API 호출 (v4: `MIB_TCPTABLE_OWNER_PID`, v6: `MIB_TCP6TABLE_OWNER_PID`, `TCP_TABLE_OWNER_PID_ALL`, row `dwState` → `PortState`)

### 프로세스 트리
- **Linux**: `/proc/*/stat`의 ppid 필드 수집 → 부모-자식 맵 → BFS
- **macOS**: `ps -A -o pid=,ppid=` subprocess
- **Windows**: `CreateToolhelp32Snapshot` + `Process32FirstW/NextW`

### 구현
- crate: `tasty-portscan` (lib only, OS별 분기) — `scan_for_pids(pids)`, `scan_all()`, `collect_descendant_pids(pid)` 제공
- popup wrapper / view: `src/adapters/ui/popup/port_scanner.rs` — `draw_port_scanner_popup` (state 결선) + `draw_port_scanner_view` (pure view, AppState 비의존)
- AppState: `port_scan: PortScanState` 필드 (비동기 머신)
- 필터 상태: `egui::Memory` 의 `port_scanner.filter` (FilterState — show_all_system / query / sort_key / sort_dir)
- 트리거: 사이드바 Tools 메뉴 `Listening ports…` 빌트인 항목
- 갤러리 데모: `crates/tasty-gallery/src/catalog/components/port_scanner.rs` — 6종 시각 케이스 (Loading / Tasty 기본 / System 전체 / Search Zero / Tasty Empty / Desc 정렬)
