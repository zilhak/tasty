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
     ├─ C5c indices_drop        host-side per-surface 인덱스 해제 (observer join 포함)
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

### 계측 재현

`gui` 경로는 사용자 메뉴/단축키로만 트리거되므로 release IPC 로 도달할 수 없다.
debug 빌드의 `debug.close_workspace`(`index`)가 그 메뉴 항목을 재현한다
([debug-ipc](../dev-guide/debug-ipc.md)). 마지막 workspace 는 거절한다 — GUI 는 그
경우 창까지 닫지만 debug IPC 는 창 종료를 재현하지 않아, 그대로 두면 workspace 가
0 개인 상태로 다음 redraw 가 패닉한다.

## 실측 기준선

Linux(X11) / debug 빌드 / 번들 plugin 전부 활성 / `TASTY_LOG=info` / 격리
`TASTY_HOME`. `path="gui"`, `debug.close_workspace` 로 close. 스크롤백 "만재" 는
surface 마다 `seq 1 20000`(기본 상한 10000 줄까지 채워짐). 단위 ms.

| 탭 수 | 스크롤백 | close_total | C1 snapshot | C2 push | C3 collect | C4 ws_purge | C5 cleanup | (C5b terminal_drop) |
|-------|----------|-------------|-------------|---------|-----------|-------------|------------|---------------------|
| 1  | 없음 | **52** | 0.80 | 0.07 | 0.013 | 0.078 | 51 | 51 |
| 1  | 만재(10k) | **150** | 86 | 6.3 | 0.016 | 0.11 | 57 | 57 |
| 10 | 없음 | **552** | 17 | 0.11 | 0.021 | 0.057 | 534 | 529 |
| 10 | 만재(100k) | **3035** | 2369 | 82 | 0.030 | 0.16 | 584 | 573 |
| 30 | 없음 | **1644** | 110 | 3.9 | 0.042 | 0.085 | 1530 | 1519 |
| 30 | 만재(300k) | **9021** | 7062 | 256 | 0.054 | 0.18 | 1703 | 1668 |

읽는 법:

- **지배 구간은 둘뿐이다 — C1 과 C5b.** 스크롤백이 비면 C5b(PTY drop)가 전부고,
  만재면 C1(스냅샷)이 70~80% 를 먹는다. C3/C4 는 어느 조건에서도 0.2ms 미만이라
  최적화 대상이 아니다.
- **C5b 는 surface 수에 선형이고 상수가 크다** — surface 당 약 50ms. 탭 30개면 그
  자체로 1.5초다. 스크롤백 유무에 거의 영향받지 않는다.
- **C1 은 라인 총합에 선형이다** — 10k 라인당 약 200~240ms. 탭 수가 아니라
  `lines` 필드에 비례한다(탭 30개·스크롤백 없음 = 110ms, 탭 10개·10만 라인 =
  2369ms).
- **C2b(스크롤백 디스크 write)는 C1 의 1/25~1/30 수준**이다 — 같은 데이터를
  다루는데도 캡처가 write 보다 압도적으로 비싸다.
- `close_total` − 단계 합은 어느 행에서도 1ms 미만이라 미계측 구간은 없다.

### memory.db 크기 의존

memory.db 를 24276 엔트리(3.6MB)까지 채우고 탭 10개·스크롤백 없음으로 close 한
결과(위 표 3행과 비교). **아래는 surface scope purge 중복을 걷어내기 전 측정이다**
— 당시엔 `SurfaceMetaStore::remove` 와 `purge_surface_memory_scope` 가 같은
`purge_scope(Scope::Surface)` 를 surface 당 2회 불러 두 단계로 잡혔다:

| | C4 ws_purge | (구) C5c meta_remove | (구) C5e memory_purge |
|---|---|---|---|
| 기본 상태 | 0.057 | 3.1 | 0.33 |
| 24k 엔트리 | 3.0 | 23 | 20 |

- purge 계열은 db 크기에 비례해 커지지만(50배 이상), 절대값이 C5b(505ms) 대비
  여전히 한 자릿수 %다.
- **두 단계의 비대칭이 중복의 증거다** — 먼저 부른 쪽(구 C5c)만 실제로 행을 지우고,
  뒤에 부른 쪽(구 C5e)은 0행을 지운 뒤 풀스캔만 한다. 그런데도 20ms(24k 기준
  10 surface 합계)를 썼다. 즉 그 20ms 는 결과에 기여하지 않는 순수 낭비였다.
- 중복 제거 후 남는 단계는 하나(현 **C5d memory_purge**)이며, 그 단계가 삭제와
  풀스캔을 모두 한다 — 비용은 구 C5c 쪽(24k 기준 23ms)에 대응하고, 구 C5e 의
  20ms 가 사라진다. 위 표는 재측정한 값이 아니라 중복 제거 **전** 측정이므로,
  현재 값을 알려면 같은 조건으로 다시 재야 한다.

### `scrollback_disk_swap`

탭 10개·스크롤백 만재, `performance.scrollback_disk_swap = true`, 새 `TASTY_HOME`:

| | close_total | C1 | C2b | C5a | C5b |
|---|---|---|---|---|---|
| off (`lines`=100000) | 3035 | 2369 | 82 | 6.3 | 573 |
| on (`lines`=199580) | 2455 | 1777 | 135 | 6.0 | 535 |

disk swap 이 켜지면 상한 10000 줄을 넘겨 유지하므로 캡처되는 `lines` 자체가 두
배가 되는데도 C1 은 오히려 낮다. 대신 C2b(디스크 write)가 1.6배로 늘어난다. 각
1 회 측정이라 경향으로만 읽는다.

## 관련

- [boot-sequence](boot-sequence.md) — 부팅 계측(T1~T7)
- [shutdown-sequence](shutdown-sequence.md) — 종료 계측(S1~S5), C5b 와 같은 PTY drop 누적기를 S5b 로 소비
- [debug-ipc](../dev-guide/debug-ipc.md) — `debug.close_workspace`
