# 워크스페이스 close 시퀀스와 계측

워크스페이스가 통째로 사라지는 경로(스냅샷 캡처 → 대상 수집 → memory purge →
surface 정리)와 그 상시 tracing 계측(`tasty::close`)을 기술한다. 부팅
([boot-sequence](boot-sequence.md)) / 종료([shutdown-sequence](shutdown-sequence.md))
계측과 같은 관례를 따르는 세 번째 다단계 동기 구간이다.

## 세 경로

워크스페이스 close 는 진입점이 셋이고, 셋 다 렌더 루프(또는 IPC 디스패치) 안에서
**동기** 실행된다.

| `path` | 진입점 | 트리거 | 스냅샷 |
|--------|--------|--------|--------|
| `gui` | `AppState::close_workspace_at` (`src/state/workspace.rs`) | 워크스페이스 컨텍스트 메뉴 "Close workspace" / 단축키 `close_active_workspace` | 항상 |
| `inline` | `AppState::close_case_workspace` (`src/state/pane.rs`) | surface→tab→pane→workspace cascade 의 인라인 디스패처 (PTY exit, egui diff close 등) | `save_snapshot` 조건부 |
| `cascade` | `Core::close_case_workspace` (`src/core/impl_close.rs`) → `cascade_surface_closed` (`src/app/dispatch_domain.rs`) | `DomainIntent::CloseSurface` 도메인 이벤트 경로 (IPC `surface.close` 등) | `save_snapshot` 조건부 (IPC 는 false) |

**세 경로의 비용 구조는 근본적으로 다르다.** `gui` 만 "탭이 N 개인 워크스페이스를
통째로" 닫는다 — 나머지 둘은 cascade 특성상 *마지막 한 개의 surface* 가 닫히면서
workspace 까지 무너지는 경우라 cleanup 대상이 사실상 항상 1개다. UI 멈춤이
보고되는 조건(탭 많은 워크스페이스 닫기)은 `gui` 경로에서만 재현된다.

**세 경로 모두 workspace 벡터에서 원소를 제거한 뒤 활성 포인터를 대상 기준으로 보정해야
한다** — 인덱스 SoT(`AppState::active_workspace`)는 앞쪽 원소가 빠지면 가리키는 대상이
바뀌기 때문이다. 규칙과 헬퍼는 [design/policies/focus.md](../design/policies/focus.md)
"삭제로 인한 인덱스 이동에서도 포커스 대상은 보존된다". 네 번째 경로를 추가하면 같은 보정을
함께 태운다(계측 단계에는 포함되지 않는 O(1) 작업이다).

`cascade` 경로만 단계가 두 함수로 갈린다 — C1~C3 은 도메인(`Core`) 쪽, C4/C5 는
앱(`cascade_surface_closed`) 쪽이다. 그래서 이 경로의 로그 순서는 **C5 가 C4 보다
먼저** 나온다(앱 쪽 1단계가 cleanup, 3단계가 workspace purge). `gui`/`inline` 은
C1→C2→C3→C4→C5 순이다.

## 단계

```
close 진입
 ├─ C1 snapshot            capture_workspace_snapshot — 전 surface 화면+스크롤백 캡처
 ├─ C2 push_closed_item    restore.command 주입 + 스크롤백 디스크 write + evict
 │   ├─ C2a restore_inject       surface 마다 surface_meta sqlite 조회
 │   ├─ C2b scrollback_persist   ~/.tasty/scrollback/<id>.bin write
 │   └─ C2c evict                LIFO 상한 초과분의 backing 파일 삭제
 ├─ C3 collect_targets     pane × tab × leaf 3중 순회
 ├─ C4 ws_memory_purge     purge_scope(Scope::Workspace) — sqlite 풀스캔
 └─ C5 cleanup_targets     surface 마다 cleanup_surface (합계)
     ├─ C5a scrollback_delete   fs::remove_file
     ├─ C5b terminal_drop       Terminal drop → PTY kill + master 해제
     ├─ C5c indices_drop        host-side per-surface 인덱스 해제 (observer sender drop — join 은 S3b)
     └─ C5d memory_purge        purge_scope(Scope::Surface) — sqlite 풀스캔 (surface 당 1회)
close_total
```

## close 계측 (target: `tasty::close`)

부팅/종료 계측과 같은 관례: 상시 발화, 레벨 `info!`, 소요는 `ms` 필드(f64
밀리초). debug 빌드는 `$TASTY_HOME/debug-dev.log`(debug 레벨 file layer)에
수집되고 stderr 기본 필터가 warn 이라 콘솔 노이즈는 없다. release 검증은
`TASTY_LOG=info`.

| 마커 | 구간 | 추가 필드 |
|------|------|-----------|
| C1 snapshot | `capture_workspace_snapshot` / `ClosedItem::from_workspace` | `surfaces`, `lines` = 캡처된 인라인 스크롤백 라인 총합 |
| C2 push_closed_item | `CoreState::push_closed_item` 전체 | `restore_inject_ms`(C2a) · `scrollback_persist_ms`(C2b) · `evict_ms`(C2c) |
| C3 collect_targets | `collect_workspace_close_targets` | `surfaces` |
| C4 ws_memory_purge | `purge_scope(Scope::Workspace)` | — |
| C5 cleanup_targets | cleanup 루프 전체 | `surfaces` · `scrollback_delete_ms`(C5a) · `terminal_drop_ms`(C5b) · `indices_drop_ms`(C5c) · `memory_purge_ms`(C5d) |
| close_total | close 진입 → 완료 | `surfaces`, `snapshot`(C1/C2 를 탔는지) |

모든 마커는 `path` 필드(`gui`/`inline`/`cascade`)를 함께 찍는다.

읽는 법:

- **C5a~C5d 는 surface 마다가 아니라 합계다.** 종료 계측 S5b(`PtyBackend::drop`
  누적) 선례와 같다. 탭 30개를 surface 단위로 찍으면 로그 150줄이 close 구간
  *안에서* 발생해 그 write 비용이 측정을 왜곡한다. N 에 대한 선형성은 `surfaces`
  필드와 합계 ms 의 조합으로 판정한다.
- **`lines` 는 C1 시점의 인라인 라인 수다.** C2b(`persist_closed_scrollback`)가
  라인을 디스크로 내리면 0 이 되므로 캡처 직후에만 의미가 있다.
- **`close_total` ≥ 단계 합**이며, 차이가 크면 계측이 덮지 않은 구간이 있다는
  뜻이다(예: `enqueue_surface_closed`, `surface_kind` 재조회, workspace 벡터
  remove).
- **`snapshot=false` 는 C1/C2 마커가 아예 없는 상태를 뜻한다.** "안 걸렸다" 와
  "계측이 없다" 를 로그만으로 구분하기 위한 필드다.
- **surface 가 0 개인 워크스페이스는 만들 수 없다** — 마지막 탭은 닫히지 않는다
  (`close tab` 이 "cannot close the last tab" 으로 거절). C3/C5 는 대상이 0 이어도
  발화하도록 되어 있지만, 실제로 `surfaces=0` 을 관측하려면 layout 이 깨진
  상태여야 한다.
- **앱 quit 과는 중복되지 않는다.** 종료 cascade(`src/app/shutdown_cascade.rs`)는
  surface 마다 lifecycle 이벤트를 *큐에 넣기만* 하고 `cleanup_surface` /
  `close_workspace_at` 을 호출하지 않는다 — 실측에서도 quit 시 `tasty::shutdown`
  마커만 나오고 `tasty::close` 는 발화하지 않는다.
- **C5b 는 `PtyBackend::drop` 전체다.** `pty_master` 해제(Windows 는 여기서
  `ClosePseudoConsole` 이 자식 종료를 기다린다)를 포함하도록 `pty_master` 를
  `Option` 으로 두고 drop 본문 안에서 `take()` 한다 — 필드 자연 해제에 맡기면 그
  비용이 계측 구간 밖으로 새어나간다. 종료 계측 S5b 도 같은 누적기를 쓴다.
- **C5b 는 자식이 죽기를 기다리지 않는다** — unix 는 SIGHUP 만 보내고 유예 폴링과
  SIGKILL escalation 을 detached reap 스레드에 넘긴다([ADR-0076](../adr/0076-close-path-per-surface-blocking-removal.md)).
  그래서 C5b 는 "종료 신호 발사 + master 해제" 비용이지 "자식 종료 확인" 비용이
  아니다. 자식이 실제로 회수됐는지는 이 마커로 판정할 수 없다.
- **C5c 는 observer 워커를 join 하지 않는다** — surface close 로 인한 자동 해제는
  sender 만 떨어뜨리고 join 을 종료 시퀀스(S3b)로 미룬다(같은 ADR). 명시 해제
  (`output.observe_stop`)만 그 자리에서 join 한다.

### 계측 재현

`gui` 경로는 사용자 메뉴/단축키로만 트리거되므로 release IPC 로 도달할 수 없다.
debug 빌드의 `debug.close_workspace`(`index`)가 그 메뉴 항목을 재현한다
([debug-ipc](../dev-guide/debug-ipc.md)). 마지막 workspace 는 거절한다 — GUI 는 그
경우 창까지 닫지만 debug IPC 는 창 종료를 재현하지 않아, 그대로 두면 workspace 가
0 개인 상태로 다음 redraw 가 패닉한다.

## 실측 기준선

Linux(X11) / debug 빌드 / 번들 plugin 전부 활성 / `TASTY_LOG=info` / 격리
`TASTY_HOME`. `path="gui"`, `debug.close_workspace` 로 close. 스크롤백 "만재" 는
surface 마다 `seq 1 20000`(기본 상한 10000 줄까지 채워짐). 각 조건 1 회 측정이라
절대값이 아니라 구간 비율과 N 에 대한 기울기로 읽는다. 단위 ms.

아래는 **벌크 캡처(C1) · surface purge 중복 제거(C5) ·
[ADR-0076](../adr/0076-close-path-per-surface-blocking-removal.md)(C5b) 이 모두
적용된 현재 상태** 측정이다.

| 탭 수 | 스크롤백 | close_total | C1 snapshot | C2b sb_persist | C3 collect | C4 ws_purge | C5 cleanup | (C5b terminal_drop) |
|-------|----------|-------------|-------------|----------------|-----------|-------------|------------|---------------------|
| 1  | 없음 | **1.1** | 0.63 | 0.0002 | 0.007 | 0.048 | 0.30 | 0.13 |
| 1  | 만재(10k) | **15** | 2.9 | 8.2 | 0.013 | 0.14 | 3.2 | 2.3 |
| 10 | 없음 | **6.8** | 4.7 | 0.0002 | 0.014 | 0.050 | 1.9 | 1.3 |
| 10 | 만재(100k) | **109** | 40 | 45 | 0.022 | 0.10 | 24 | 18 |
| 30 | 없음 | **33** | 20 | 0.0004 | 0.036 | 0.094 | 12 | 7.1 |
| 30 | 만재(300k) | **403** | 97 | 234 | 0.038 | 0.12 | 71 | 53 |

#### ADR-0076 전후 (같은 조건, 스크롤백 없음)

| 탭 수 | close_total (전 → 후) | C5 cleanup (전 → 후) | C5b terminal_drop (전 → 후) |
|-------|-----------------------|----------------------|------------------------------|
| 1  | 52 → **1.1** | 51 → **0.30** | 50 → **0.13** |
| 10 | 513 → **6.8** | 507 → **1.9** | 505 → **1.3** |
| 30 | 1541 → **33** | 1528 → **12** | 1518 → **7.1** |

읽는 법:

- **C3/C4 는 어느 조건에서도 0.2ms 미만**이라 최적화 대상이 아니다.
- `close_total` − 단계 합은 어느 행에서도 1ms 미만이라 미계측 구간은 없다.
- **C5b 의 큰 상수 항은 사라졌다.** ADR-0076 이전에는 surface 당 약 50ms 로,
  탭 수에만 붙는 이 상수가 close 전체를 지배했다(탭 30개면 그 자체로 1.5 초). 그
  50ms 는 전부 `portable-pty` 의 unix `ChildKiller::kill` 안에 있는
  `thread::sleep(50ms)` 유예 폴링이었다. 지금은 SIGHUP 만 보내고 유예를 detached
  reap 스레드에 넘긴다.
- **지배 구간은 이제 C2b(스크롤백 디스크 write) 다.** 만재 30탭에서 C2b 234ms 로
  close_total 403ms 의 절반이 넘는다. 그다음이 C1 97ms(24%), C5 71ms(18%) 순이다.
  close 체감 지연을 더 줄이려면 여기를 봐야 한다. 이 write 는 close 프레임 안에서
  동기로 돈다. (만재 30탭 행은 3 회 반복했고 close_total 403~406 / C2b 196~234 /
  C1 97~129 / C5 70~81 범위였다 — C2b 가 최대 항이라는 순서는 3 회 모두 같았다.)
- **C1 은 라인 총합에 선형이고 기울기가 작다** — 10k 라인당 약 3ms. 캡처가 스크롤백의
  저장 표현(`ScrollbackLine`)을 그대로 벌크로 가져오기 때문이다(아래 "캡처 비용"
  참조). 같은 데이터를 다루는 C2b 보다 2 배 이상 싸다.
- **스크롤백이 없으면 close 는 전 구간이 수십 ms 다** — 탭 30개·스크롤백 없음에서
  close_total 33ms 로, ADR-0076 이전의 1.5 초에서 46 배 줄었다. 이 조건에서 남은
  최대 항은 C1(20ms, 화면 rows x cols 복제)이다.
- **C5b 는 스크롤백이 있으면 다시 커지지만 성격이 다르다** — 만재 30탭에서 53ms
  (surface 당 1.8ms)로, 스크롤백 없음(surface 당 0.24ms)의 7 배다. 이건 자식을
  기다리는 시간이 아니라 `Terminal` 이 들고 있던 인메모리 스크롤백 30 만 라인을
  해제하는 비용이다 — ADR-0076 이 걷어낸 대기 항과 무관하게 데이터 양에 붙는다.
- **C5a(`fs::remove_file`)는 스크롤백 유무로 두 자릿수 배 갈린다** — 스크롤백
  없음에서는 지울 파일이 없어 surface 당 약 5µs(탭 30개 1.6ms)지만, 만재에서는
  실제 파일 삭제라 surface 당 약 0.5ms(탭 30개 15ms)다. 후자도 close_total 의
  4% 수준이라 비동기화 대상은 아니다(ADR-0076 기각 근거). 삭제 실패는 다음 시작의
  `scrollback_store::gc_orphans` 가 회수한다.
- **C5c(인덱스 해제)는 observer 워커를 join 하지 않으므로 observer 수에 거의
  무관하다** — 파일 sink 12 개 기준 8.9ms → 0.39ms. 어느 행에서도 0.35ms 미만이다.
- **C5d(`purge_scope`, sqlite)는 memory.db 크기에 비례한다** — 위 표는 갓 만든
  `TASTY_HOME` 이라 탭 30개에서도 2~9ms 지만, db 가 커지면 C5 안의 지배 항이 된다.
  아래 "memory.db 크기 의존" 참조.

### 캡처 비용 (C1)

C1 은 surface 마다 화면(rows x cols)과 스크롤백 전량을 `ClosedItem` 으로 복제한다.
스크롤백 쪽은 라인 단위가 아니라 **벌크**로 가져온다 —
`Terminal::scrollback_lines_all()` 이 terminal state mutex 를 한 번만 잡고
스크롤백의 저장 표현인 `ScrollbackLine`(단일 text 버퍼 + cell 길이 + RLE 속성 런)을
그대로 복제한다. 라인당 비용이 헤더 3개 복제로 고정돼 cell 수에 비례하지 않는다.

라인당 경로(`scrollback_line_full`)도 남아 있지만 selection / search / link 처럼
소수 라인만 만지는 소비자용이다. 벌크 캡처에 쓰면 두 가지가 겹쳐 비싸진다:

- 라인마다 state mutex — 파서 스레드가 `ingest` 로 잡는 것과 같은 lock(ADR-0002)
  이라, 만재 스크롤백 캡처가 파서와 수만 회 경합한다.
- 디스크 영역 라인은 `line_owned` / `line_wrapped` 가 같은 인덱스를 독립적으로
  읽어 `File::open` 이 라인당 2회가 된다(현재는 `line_full` 단일 조회로 1회).

`layout_persistence::scrollback` 의 캡처도 같은 벌크 경로를 쓴다.

캡처 표현이 원본과 셀 단위로 동일하다는 것(그래핌 / cell 속성 / `wrapped`)은
`crates/tasty-terminal/tests/scrollback_bulk_capture.rs` 가, 그 표현이 디스크
왕복을 거쳐도 복원 payload 를 바꾸지 않는다는 것은
`src/store/scrollback.rs` 의 `capture_persist_restore_round_trip_preserves_lines`
가 고정한다.

### memory.db 크기 의존

memory.db 를 24276 엔트리(3.6MB)까지 채우고 탭 10개·스크롤백 없음으로 close 한
결과(위 표 3행과 비교). **아래는 surface scope purge 중복을 걷어내기 전 측정이다**
— 당시엔 `SurfaceMetaStore::remove` 와 `purge_surface_memory_scope` 가 같은
`purge_scope(Scope::Surface)` 를 surface 당 2회 불러 두 단계로 잡혔다:

| | C4 ws_purge | (구) C5c meta_remove | (구) C5e memory_purge |
|---|---|---|---|
| 기본 상태 | 0.057 | 3.1 | 0.33 |
| 24k 엔트리 | 3.0 | 23 | 20 |

- purge 계열은 db 크기에 비례해 커지지만(50배 이상), 이 측정 당시 절대값은
  C5b(505ms) 대비 한 자릿수 % 였다.
- **두 단계의 비대칭이 중복의 증거다** — 먼저 부른 쪽(구 C5c)만 실제로 행을 지우고,
  뒤에 부른 쪽(구 C5e)은 0행을 지운 뒤 풀스캔만 한다. 그런데도 20ms(24k 기준
  10 surface 합계)를 썼다. 즉 그 20ms 는 결과에 기여하지 않는 순수 낭비였다.
- 중복 제거 후 남는 단계는 하나(현 **C5d memory_purge**)이며, 그 단계가 삭제와
  풀스캔을 모두 한다 — 비용은 구 C5c 쪽(24k 기준 23ms)에 대응하고, 구 C5e 의
  20ms 가 사라진다. 위 표는 재측정한 값이 아니라 중복 제거 **전** 측정이므로,
  현재 값을 알려면 같은 조건으로 다시 재야 한다.
- **C5b 가 사라진 지금은 이 관계가 뒤집힌다** — 위 비교의 기준이던 C5b(505ms)가
  [ADR-0076](../adr/0076-close-path-per-surface-blocking-removal.md) 으로 한 자릿수
  ms 가 됐으므로, purge 계열은 더 이상 "C5b 대비 한 자릿수 %" 가 아니라 **C5 안의
  지배 항**이다. db 가 클수록 close 지연에 직접 드러난다.

### `scrollback_disk_swap`

탭 10개·스크롤백 만재(`seq 1 30000`), `performance.scrollback_disk_swap = true`,
새 `TASTY_HOME`. disk swap 이 켜지면 상한 10000 줄을 넘겨 유지하므로 캡처되는
`lines` 자체가 크게 늘어난다.

| | lines | close_total | C1 | C2b | C5b |
|---|---|---|---|---|---|
| off | 100000 | 601 | 33 | 41 | 521 |
| on | 279622 | 1445 | 544 | 246 | 547 |

> 이 표의 `close_total` / `C5b` 열은
> **[ADR-0076](../adr/0076-close-path-per-surface-blocking-removal.md) 이전** 측정이라
> surface 당 50ms 상수를 포함한다(탭 10개 = 약 500ms). 지금 같은 조건을 다시 재면
> 두 열에서 그만큼이 빠진다 — 이 절의 논점인 C1 의 라인당 단가는 영향받지 않는다.

disk 영역 라인은 캡처가 메모리 복제가 아니라 **라인마다 파일 read** 라 C1 의
라인당 단가가 memory-only 대비 한 자릿수 배 높다(10k 라인당 3ms → 19ms). 켜고
쓸 때 close 비용이 질적으로 달라지는 지점이므로, 라인 수가 아니라 *디스크 영역
라인 수* 로 읽어야 한다.

## 관련

- [boot-sequence](boot-sequence.md) — 부팅 계측(T1~T7)
- [shutdown-sequence](shutdown-sequence.md) — 종료 계측(S1~S5), C5b 와 같은 PTY drop 누적기를 S5b 로 소비
- [debug-ipc](../dev-guide/debug-ipc.md) — `debug.close_workspace`
