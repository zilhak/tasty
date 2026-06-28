# 리스닝 포트 뷰어 (Listening ports)

- **Status**: Implemented
- **주체**: 로컬 사용자 (원격 접속 사용자는 mirror 로 동일 화면을 본다)
- **ADR**: 없음
- **코드**: `src/adapters/ui/popup/port_scanner.rs`, `crates/tasty-portscan`
- **화면**: [screens/listening-ports.md](screens/listening-ports.md)

## 목적

로컬 사용자가 워크스페이스(또는 시스템 전체)의 listening TCP 포트를 한눈에 보고, 행을 선택해 그 주소를 클립보드로 복사할 수 있게 하는 **GUI 진단 편의 기능**. "내 dev 서버가 몇 번에 떴지?" 를 셸 명령 없이 UI 로 확인한다.

## 내부 동작

### 스캔 상태 머신

`Idle → (열림 / scope 변경 / Refresh) → Loading → (poll) → Ready | Failed`. 팝업을 닫으면 `Idle` 로 reset.

- 백그라운드 스레드가 스캔하고 mpsc 채널로 결과를 보낸다. 메인 루프가 매 프레임 poll 한다.
- 동일 scope 결과는 캐시(`CachedScan`)로 재사용한다.

### 데이터 (7컬럼)

Port / Proto / Address / Process / Workspace / Tab / State.

- **Proto**: 현재 TCP 고정 (UDP 미지원).
- **Workspace / Tab**: Tasty 프로세스 트리 소속 행만 채워지고, External 행은 em-dash(`—`).
- **State**: 연결 상태(`PortState` — Listen / Established / CloseWait / …). `LISTEN` → green dot + pulse, 그 외 → yellow dot. `reduced_motion` 시 정적 dot. 백엔드는 전 상태를 수집하지만 뷰어는 기본적으로 LISTEN 만 표시한다(아래 상태 필터).

### scope (Tasty / System)

- **Tasty**: Tasty 셸 프로세스 트리 자손 PID 가 listening 중인 포트만.
- **System**: host 의 모든 TCP 소켓. Tasty 자손 PID 와 일치하는 행은 workspace/tab 으로 태그.
- toggle 시 자동 재스캔.

### 검색 / 정렬

- **검색**: 전 컬럼 case-insensitive substring (port / addr / pid / process / workspace / tab). 빈 query 는 전체 통과. **데이터 기준**이라 컬럼 숨김과 독립(숨긴 컬럼도 검색 대상).
- **정렬**: Port / Address / Process / Workspace / Tab 가능, **Proto / State 는 불가**. None·빈 값은 정렬 방향과 무관하게 항상 tail. 숨긴 컬럼이 활성 sort key 여도 정렬은 그대로 유지(데이터 정렬은 표시와 독립).
- 필터·정렬·컬럼 표시 상태는 `egui::Memory` 에 영속.

### 컬럼 폭 / 가로 스크롤

- 각 컬럼은 **최소폭**을 가진다 (Port 84 / Proto 76 / Address 140 / Process 200 / Workspace 120 / Tab 80 / State 140). 보이는 컬럼 최소폭 합이 본문 가용폭을 넘으면 **테이블 본문이 가로 스크롤**된다(말줄임 대신). 가용폭이 남으면 flex 컬럼(Address / Process)이 여유폭을 나눠 받아 빈 공간 없이 채운다.
- 가로 스크롤은 **본문 영역에만** 갇힌다. sticky 헤더는 본문과 수평 동기 이동(세로로는 고정), footer / 구역 divider 는 popup 폭에 고정 유지된다.

### 컬럼 표시/숨김 (column chooser)

- 헤더 우측 컬럼 아이콘 버튼을 누르면 컬럼 목록 팝업이 열리고, 컬럼별 체크박스로 표시/숨김을 토글한다 (예: System scope 에서 의미가 옅은 Workspace / Tab 숨김).
- **Port 는 식별 / 기본 정렬 컬럼이라 항상 표시**(체크박스 잠금) — 전부 숨김이 구조적으로 불가능하다.
- 표시 상태는 `egui::Memory` 에 영속하여 팝업을 닫았다 열어도 유지된다. (per-column 값 필터가 아니라 컬럼 전체의 표시/숨김이다.)

### 상태 필터 (state filter, 기본 LISTEN-only)

- 필터 행 우측의 funnel 버튼을 누르면 **표시할 TCP 상태** 드롭다운이 열린다. `FilterState.visible_states` 는 **표시(shown) 집합**으로, 기본값은 `{Listen}`(LISTEN-only). 백엔드가 전 상태를 수집해도 뷰어는 기본적으로 listening 소켓만 보여준다.
- 드롭다운은 **현재 scope 에 실제로 존재하는 상태들**(필터 전 전체 rows 에서 도출, LISTEN 선두 + 알파벳)만 체크박스로 나열한다 — 꺼둔 상태도 다시 켤 수 있다. 체크 = 표시. **모두 선택 / 모두 해제 / 초기화(LISTEN-only 복원) / 적용** 버튼을 제공한다.
- **Apply-on-confirm**: 드롭다운 편집은 임시 draft(egui temp memory)에만 쌓이고 **적용**을 눌러야 반영된다. 드롭다운이 열려 있을 때 Escape 는 드롭다운만 닫고 팝업은 유지한다.
- 표시 상태 집합은 `egui::Memory` 에 영속해 재오픈에도 유지된다(앱 종료 시 휘발 → LISTEN-only 복원).
- **적용 순서**: 상태 필터 → 검색 → 정렬. footer `total` 은 *상태 통과·검색 전* 개수, 헤더 `{listening} listening` 은 상태 필터와 무관한 scope 전체 LISTEN 개수다.

### 빈 결과 4분기

`Ready` 인데 행이 0개일 때 우선순위: **search_zero**(검색어 있음) → **state_filtered**(검색은 비었으나 상태 필터가 scope 행을 전부 가림) → **system_empty**(System scope) → **tasty_empty**(Tasty scope). `Loading` / `Failed` 는 별도 분기. state_filtered 분기 덕에 LISTEN 0개를 "포트 없음" 으로 오인하지 않는다.

### 레이아웃 (footer 하단 고정)

- 헤더 / 필터 행을 그린 뒤 남은 영역을 `TopBottomPanel::bottom`(footer) + `CentralPanel`(본문)로 나눈다. footer 가 하단 공간을 먼저 예약하므로 **footer 는 항상 popup 하단에 고정**되고, 본문이 남은 높이 전체를 채운다.
- 행이 적으면 테이블이 위로 붙고 **빈 공간은 본문(목록) 영역 하단(footer 위)** 에 남는다 — footer 아래가 비지 않는다.
- 구역 divider(헤더/필터/footer 위)는 popup 전체폭(`full.x_range()`)에 고정. 본문 가로 스크롤은 테이블 자체 ScrollArea 에 갇혀 footer/divider 폭에 영향을 주지 않는다.

### 헤더 / footer 구성

- **헤더**: leading 포트 아이콘 + 제목 + accent Tag(`{listening} listening` / `scanning…`) + 검색 입력 + 컬럼 chooser 아이콘 버튼 + Refresh 아이콘 버튼(상시 노출, 현재 scope 재스캔) + close(`×`).
- **필터 행**: 좌측 `Show all (system-wide)` 체크박스 + 우측 상태 필터 funnel 버튼.
- **footer**: 카운터(`{shown} of {total} ports`) + `Copy address`(행 미선택 시 disabled) + `Close`. popup 하단 고정.

### 행 선택 / 주소 복사

- 행을 클릭하면 선택(강조)되고, 같은 행을 다시 클릭하면 해제된다(`selected_port`, `egui::Memory` 영속).
- footer `Copy address` 는 선택 행의 `host:port`(IPv6 는 `[..]` bracket)를 클립보드에 복사한다(egui platform-output copy → `handle_platform_output`).

## 인터페이스

- **사용자 트리거**: 사이드바 [도구 메뉴](../tools-menu/index.md) 의 `Listening ports…` → `port_scanner` popup (`PopupScope::Window`). 행 클릭으로 선택 후 footer `Copy address` 로 주소를 복사한다.
- **IPC/CLI**: 없음 — **의도된 설계.** 포트 목록은 agent 가 일반 셸 명령(`ss` / `lsof` / `netstat`)으로 직접 조회 가능하므로 tasty 가 중복 제공하지 않는다. 이 기능은 *사람이 UI 로 편하게 보는* 편의일 뿐이다.

## 비-목표

- UDP 포트 (TCP 전용).
- 포트/프로세스 종료(kill) — 보기 전용.
- IPC/CLI 제공 (셸로 충분하므로 중복하지 않음).

## Acceptance Criteria

- [ ] Tools 메뉴 `Listening ports…` 클릭 시 팝업이 열리고 스캔이 시작된다 (Loading → Ready).
- [ ] `LISTEN` 포트는 green dot, 그 외 상태는 yellow dot 으로 표시된다.
- [ ] scope 를 System 으로 토글하면 재스캔되어 host 전체 포트가 나온다.
- [ ] 검색어 입력 시 모든 컬럼에 substring 매칭으로 행이 필터된다.
- [ ] 행이 0개일 때 scope/검색/상태필터 조합에 맞는 빈 메시지(search_zero / state_filtered / system_empty / tasty_empty)가 뜬다.
- [ ] 기본 열림 시 LISTEN 상태 행만 표시된다. 상태 필터로 ESTABLISHED 등을 켜면 해당 행이 나타나고 footer `{shown} of {total}` 이 갱신된다. 드롭다운 열림 중 Escape 는 드롭다운만 닫는다.
- [ ] footer 는 팝업을 리사이즈해도 항상 하단에 고정되고, 행이 적을 때 빈 공간은 목록 영역 하단(footer 위)에 생긴다.
- [ ] 행 클릭 시 선택 강조되고 재클릭 시 해제된다. 클릭으로 브라우저가 열리지 않는다.
- [ ] footer `Copy address` 는 선택 시에만 활성화되고, 클릭 시 선택 행의 주소가 클립보드에 복사된다.
- [ ] 헤더 Refresh 버튼이 상시 노출되어 정상 상태에서도 재스캔할 수 있다.
- [ ] 보이는 컬럼 최소폭 합이 본문 폭을 넘으면 테이블이 가로 스크롤되고, footer `Close` / `Copy address` 와 헤더 구역은 잘리지 않고 popup 폭에 고정된다.
- [ ] 헤더 컬럼 chooser 로 컬럼을 숨기면 폭이 줄고, 팝업을 닫았다 열어도 표시 상태가 유지된다. Port 컬럼은 숨길 수 없다.

> GUI 기능이라 검증은 gallery 데모 + 스크린샷(시각)으로 한다. scan/filter 로직은 `tasty-portscan` 단위 테스트로 독립 검증 가능.

## 구현

- crate `tasty-portscan` — `scan_all()` / `scan_for_pids(pids)` / `collect_descendant_pids(pid)`, OS 백엔드 분기(Linux `/proc/net/tcp`, macOS `lsof`, Windows `GetExtendedTcpTable`), 캐시 `cache.rs`.
- popup: `src/adapters/ui/popup/port_scanner.rs` — `draw_port_scanner_popup`(state 결선) + `draw_port_scanner_view`(pure view).
- 비동기 상태: `AppState.port_scan: PortScanState`. 필터 상태: `egui::Memory`.
- gallery 데모: `crates/tasty-gallery/src/catalog/components/port_scanner.rs`.
