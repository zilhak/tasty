# 리스닝 포트 뷰어 (Listening ports)

- **Status**: Implemented
- **주체**: 로컬 사용자 (원격 접속 사용자는 mirror 로 동일 화면을 본다)
- **ADR**: 없음
- **코드**: `src/adapters/ui/popup/port_scanner.rs`, `crates/tasty-portscan`
- **화면**: [screens/listening-ports.md](screens/listening-ports.md)

## 목적

로컬 사용자가 워크스페이스(또는 시스템 전체)의 listening TCP 포트를 한눈에 보고, 포트를 클릭해 브라우저로 열 수 있게 하는 **GUI 진단 편의 기능**. "내 dev 서버가 몇 번에 떴지?" 를 셸 명령 없이 UI 로 확인한다.

## 내부 동작

### 스캔 상태 머신

`Idle → (열림 / scope 변경 / Refresh) → Loading → (poll) → Ready | Failed`. 팝업을 닫으면 `Idle` 로 reset.

- 백그라운드 스레드가 스캔하고 mpsc 채널로 결과를 보낸다. 메인 루프가 매 프레임 poll 한다.
- 동일 scope 결과는 캐시(`CachedScan`)로 재사용한다.

### 데이터 (7컬럼)

Port / Proto / Address / Process / Workspace / Tab / State.

- **Proto**: 현재 TCP 고정 (UDP 미지원).
- **Workspace / Tab**: Tasty 프로세스 트리 소속 행만 채워지고, External 행은 em-dash(`—`).
- **State**: 연결 상태(`PortState` — Listen / Established / CloseWait / …). `LISTEN` → green dot + pulse, 그 외 → yellow dot. `reduced_motion` 시 정적 dot.

### scope (Tasty / System)

- **Tasty**: Tasty 셸 프로세스 트리 자손 PID 가 listening 중인 포트만.
- **System**: host 의 모든 TCP 소켓. Tasty 자손 PID 와 일치하는 행은 workspace/tab 으로 태그.
- toggle 시 자동 재스캔.

### 검색 / 정렬

- **검색**: 전 컬럼 case-insensitive substring (port / addr / pid / process / workspace / tab). 빈 query 는 전체 통과.
- **정렬**: Port / Address / Process / Workspace / Tab 가능, **Proto / State 는 불가**. None·빈 값은 정렬 방향과 무관하게 항상 tail.
- 필터·정렬 상태는 `egui::Memory` 에 영속.

### 빈 결과 3분기

`Ready` 인데 행이 0개일 때 우선순위: **search_zero**(검색어 있음) → **system_empty**(System scope) → **tasty_empty**(Tasty scope). `Loading` / `Failed` 는 별도 분기.

### 카운트 표시

header 태그 `{listening} listening`, footer `{shown} of {total} ports`.

## 인터페이스

- **사용자 트리거**: 사이드바 Tools 메뉴 상단 `Listening ports…` → `port_scanner` popup (`PopupScope::Window`). 포트 번호 클릭 시 wildcard(`0.0.0.0` / `[::]`)는 `localhost` 로 치환해 브라우저로 연다.
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
- [ ] 행이 0개일 때 scope/검색 조합에 맞는 빈 메시지(search_zero / system_empty / tasty_empty)가 뜬다.
- [ ] 포트 번호 클릭 시 `http://<host>:<port>` 가 브라우저로 열린다 (wildcard → localhost).

> GUI 기능이라 검증은 gallery 데모 + 스크린샷(시각)으로 한다. scan/filter 로직은 `tasty-portscan` 단위 테스트로 독립 검증 가능.

## 구현

- crate `tasty-portscan` — `scan_all()` / `scan_for_pids(pids)` / `collect_descendant_pids(pid)`, OS 백엔드 분기(Linux `/proc/net/tcp`, macOS `lsof`, Windows `GetExtendedTcpTable`), 캐시 `cache.rs`.
- popup: `src/adapters/ui/popup/port_scanner.rs` — `draw_port_scanner_popup`(state 결선) + `draw_port_scanner_view`(pure view).
- 비동기 상태: `AppState.port_scan: PortScanState`. 필터 상태: `egui::Memory`.
- gallery 데모: `crates/tasty-gallery/src/catalog/components/port_scanner.rs`.
