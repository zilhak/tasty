# 워크스페이스 close 시퀀스와 계측

워크스페이스를 닫을 때의 스냅샷 캡처, 대상 수집, memory scope 삭제, surface 정리 순서와
`tracing` 계측(`tasty::close`)을 설명한다. [부팅](boot-sequence.md)과
[종료](shutdown-sequence.md) 계측도 같은 로그 형식을 쓴다.

## 세 경로

워크스페이스 close 는 진입점이 셋이고, 셋 다 렌더 루프(또는 IPC 디스패치) 안에서
**동기** 실행된다.

| `path` | 진입점 | 트리거 | 스냅샷 |
|--------|--------|--------|--------|
| `gui` | `AppState::close_workspace_at` (`src/state/workspace.rs`) | 워크스페이스 컨텍스트 메뉴 "Close workspace" / 단축키 `close_active_workspace` | 항상 |
| `inline` | `AppState::close_case_workspace` (`src/state/pane.rs`) | surface→tab→pane→workspace cascade 의 인라인 디스패처 (PTY exit, egui diff close 등) | `save_snapshot` 조건부 |
| `cascade` | `Core::close_case_workspace` (`src/core/impl_close.rs`) → `cascade_surface_closed` (`src/core/structural_cascade.rs`) | `DomainIntent::CloseSurface` 도메인 이벤트 경로 (IPC `surface.close` 등) | `save_snapshot` 조건부 (IPC 는 false) |

**세 경로의 비용 구조는 근본적으로 다르다.** `gui` 만 "탭이 N 개인 워크스페이스를
통째로" 닫는다 — 나머지 둘은 cascade 특성상 *마지막 한 개의 surface* 가 닫히면서
workspace 까지 무너지는 경우라 cleanup 대상이 사실상 항상 1개다. UI 멈춤이
보고되는 조건(탭 많은 워크스페이스 닫기)은 `gui` 경로에서만 재현된다.

**세 경로 모두 workspace 벡터에서 원소를 제거한 뒤 활성 포인터를 대상 기준으로 보정해야
한다** — 활성 인덱스(`AppState::active_workspace`)는 앞쪽 원소가 빠지면 가리키는 대상이
바뀌기 때문이다. 규칙과 헬퍼는 [design/policies/focus.md](../design/policies/focus.md)
"삭제로 인한 인덱스 이동에서도 포커스 대상은 보존된다". 네 번째 경로를 추가하면 같은 보정을
함께 적용한다(계측 단계에는 포함되지 않는 O(1) 작업이다).

`cascade` 경로만 단계가 두 함수로 갈린다 — C1~C3 은 `Core::apply` 쪽, C4/C5 는
cascade(`cascade_surface_closed`) 쪽이다. 그래서 이 경로의 로그 순서는 **C5 가 C4 보다
먼저** 나온다(cascade 쪽 1단계가 cleanup, 3단계가 workspace purge). `gui`/`inline` 은
C1→C2→C3→C4→C5 순이다.

## 자원 회수의 소유

`Core::apply` 의 close 계열(`DomainIntent::{CloseSurface,ClosePane,CloseTab}`)은 트리만
바꾸고 닫힌 surface 목록(`cleanup_targets`)을 이벤트에 실어 **반환**한다. 실제 회수(PTY
kill · 스크롤백 파일 삭제 · per-surface 인덱스 해제 · memory scope purge · attach 점유 흔적
제거)와 `surface.closed` lifecycle 통지는 cascade 쪽이 한다.

그 회수는 **한 함수**가 소유한다 — `src/core/structural_cascade.rs` 의
`reclaim_closed_surfaces`. close cascade 셋(`cascade_surface_closed` · `cascade_pane_closed_full` ·
`cascade_tab_closed_full`)이 모두 그것을 부르고, 그 셋과 split / tab 생성 cascade 를 사용자
GUI dispatcher · IPC 핸들러 · 원격 forward 실행이 함께 부른다. 두 빌드(gui / headless)가 같은
파일을 컴파일하므로 함수 하나에 본문 하나이고, 빌드 형태의 차이는 그 본문 안의
`#[cfg(feature = "gui")]` 블록으로만 존재한다. 근거는
[ADR-0002](../adr/0002-domain-execution-and-ports.md)에 정리되어 있다.


### gui 와 headless 의 차이

| | gui | headless |
|---|---|---|
| `cleanup_surface` (PTY · 스크롤백 · 인덱스 · memory scope) | 한다 | 한다 |
| `surface.closed` lifecycle enqueue | 한다 | **안 한다** |
| `tab.closed` / `pane.closed` / `workspace.closed` host event | 한다 | **안 한다** |
| C4 워크스페이스 memory scope purge (`after_workspace_removed`) | 한다 | **안 한다** — 아래 기준으로는 회수라 양쪽에 있어야 하는 단계다. `workspace.closed` 통지와 한 함수에 묶여 함께 빠져 있고, 의도인지는 정해지지 않았다 |
| 활성 포인터 보정 (`fix_workspace_pointers_after_removal`) | 한다 | 한다 |
| 워크스페이스가 비면 재생성 | 한다 | 한다 |
| C5 계측 · `close_total` 기록 | `cascade_surface_closed` 만 | 안 한다 |
| `surface.created` / `pane.split` / `pane.created` / `tab.created` host event (split · 탭 생성) | 한다 | **안 한다** |
| 사용자 origin 의 split 포커스 이동 | 한다 | 한다 — headless 에는 `User` origin을 만드는 호출자가 없어 닿지 않는다 |
| 튜토리얼 관찰 (split) | 한다 | 안 한다 (튜토리얼이 gui 전용) |

**차이를 가르는 것은 "그 통지에 소비자가 있는가" 하나다.** headless 에는 두 큐를 plugin
event bus 로 내보내는 주체(plugin manager / view)가 없다. `pending_lifecycle_events` 는
headless 에서 아무도 비우지 않아 enqueue 하면 프로세스 수명 동안 자라고,
`pending_host_events` 는 headless drain(`drain_pending_host_events`)이 `HookFired` 만 적용하고
나머지 종류를 버리므로 여기서 넣는 종류(`tab.*` · `pane.*` · `surface.created` ·
`workspace.closed`)는 넣어도 닿는 곳이 없다. 반대로 자원 회수와 포인터 보정은 소비자가 상태
자신이라 양쪽에 똑같이 필요하다. 같은 기준의 서술이
`src/intent/headless.rs` 모듈 주석에도 있다.

공통 단계와 `SurfaceCloseCascade` 생성자는 두 빌드가 같은 소스를 컴파일한다.
다만 `#[cfg(feature = "gui")]` 안의 코드는 헤드리스 빌드에서 빠진다. 헤드리스에서
사용하지 않는 필드·인자는 `cfg_attr(not(feature = "gui"), expect(...))`로 표시하며,
예상과 달리 사용되면 경고가 난다. 이 파일을 고친 뒤에는 다음 두 빌드를 모두 확인한다.

- `cargo check --workspace --all-targets`
- `cargo check --workspace --no-default-features --all-targets`

### forward 경로의 결과 타입

원격 mirror 가 forward 한 구조 op 의 실행(`src/core/attach_runtime.rs`)은 도메인 값
(`Result<(), String>`)으로 답하고, wire 타입(`JsonRpcResponse`)을 만들지 않는다. split /
tab.create / tab.close / tab.move / pane.close / surface.close 는 IPC 핸들러가 부르는 것과
**같은** 도메인 실행 함수(`src/core/structural_exec.rs`)를 부르고, 그 실패
(`StructuralFailure`)를 `forward_result` 한 함수가 사유 문자열로 바꾼다. IPC 핸들러는 같은
실패를 `invalid_params` / `internal_error` / `structural_apply_error` 로 감싼다 — 그래서 같은
입력에 두 진입점의 사유 문구가 같다. 두 시험으로 사유 문구를 확인한다: 두 진입점의 결과가 같은지
(`forward_and_ipc_fail_with_the_same_reason_for_the_same_input`), 문구가 옛 기준과 같은지
(`failure_reasons_keep_the_base_literals` — 기대 문자열과 비교). convert / restore / move-surface 는 `Core::apply` 를
직접 부른다. 호출자가 회신하는 것은 `StreamControl::StructuralResult` 다. 근거는
[ADR-0002](../adr/0002-domain-execution-and-ports.md).

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

부팅·종료 계측과 같이 항상 기록하며, 레벨 `info!`, 소요는 `ms` 필드(f64
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
  기록하도록 되어 있지만, 실제로 `surfaces=0` 을 관측하려면 layout 이 깨진
  상태여야 한다.
- **앱 quit 과는 중복되지 않는다.** 종료 cascade(`src/app/shutdown_cascade.rs`)는
  surface 마다 lifecycle 이벤트를 *큐에 넣기만* 하고 `cleanup_surface` /
  `close_workspace_at` 을 호출하지 않는다 — 실측에서도 quit 시 `tasty::shutdown`
  마커만 나오고 `tasty::close` 로그는 나오지 않는다.
- **C5b 는 `PtyBackend::drop` 전체다.** `pty_master` 해제(Windows 는 여기서
  `ClosePseudoConsole` 이 자식 종료를 기다린다)를 포함하도록 `pty_master` 를
  `Option` 으로 두고 drop 본문 안에서 `take()` 한다 — 필드 자연 해제에 맡기면 그
  비용이 계측 구간 밖으로 새어나간다. 종료 계측 S5b 도 같은 누적기를 쓴다.
- **C5b 는 자식이 죽기를 기다리지 않는다** — unix 는 SIGHUP 만 보내고 유예 폴링과
  SIGKILL escalation 을 detached reap 스레드에 넘긴다([ADR-0016](../adr/0016-window-platform-and-shutdown.md)).
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
[ADR-0016](../adr/0016-window-platform-and-shutdown.md)(C5b) 이 모두
적용된 뒤의 측정 기록**이다. 현재 환경의 성능을 보장하는 값은 아니다.

| 탭 수 | 스크롤백 | close_total | C1 snapshot | C2b sb_persist | C3 collect | C4 ws_purge | C5 cleanup | (C5b terminal_drop) |
|-------|----------|-------------|-------------|----------------|-----------|-------------|------------|---------------------|
| 1  | 없음 | **1.1** | 0.63 | 0.0002 | 0.007 | 0.048 | 0.30 | 0.13 |
| 1  | 만재(10k) | **15** | 2.9 | 8.2 | 0.013 | 0.14 | 3.2 | 2.3 |
| 10 | 없음 | **6.8** | 4.7 | 0.0002 | 0.014 | 0.050 | 1.9 | 1.3 |
| 10 | 만재(100k) | **109** | 40 | 45 | 0.022 | 0.10 | 24 | 18 |
| 30 | 없음 | **33** | 20 | 0.0004 | 0.036 | 0.094 | 12 | 7.1 |
| 30 | 만재(300k) | **403** | 97 | 234 | 0.038 | 0.12 | 71 | 53 |

<a id="adr-0076-전후-같은-조건-스크롤백-없음"></a>

#### 자식 종료 대기 분리 전후 (같은 조건, 스크롤백 없음)

| 탭 수 | close_total (전 → 후) | C5 cleanup (전 → 후) | C5b terminal_drop (전 → 후) |
|-------|-----------------------|----------------------|------------------------------|
| 1  | 52 → **1.1** | 51 → **0.30** | 50 → **0.13** |
| 10 | 513 → **6.8** | 507 → **1.9** | 505 → **1.3** |
| 30 | 1541 → **33** | 1528 → **12** | 1518 → **7.1** |

이 측정에서는 C3/C4가 모두 0.2ms 미만이고, `close_total`과 단계 합의 차이가
1ms 미만이었다. 단계 밖의 작업이 없다는 뜻은 아니다.

- 스크롤백이 찬 30탭에서는 C2b(디스크 쓰기)가 234ms로 가장 컸다. 이 조건을
  3회 반복한 범위는 close_total 403~406 / C2b 196~234 / C1 97~129 / C5 70~81ms이며,
  모두 C2b가 가장 컸다. 디스크 쓰기는 close 프레임에서 동기로 실행된다.
- C1은 화면과 스크롤백을 복제하므로 데이터 양에 영향을 받는다. 스크롤백이 없는
  30탭에서는 화면 rows × cols 복제에 20ms가 걸렸다.
- C5b에는 `Terminal`이 가진 스크롤백 메모리를 해제하는 비용도 포함된다.
  자식 종료 대기는 별도 스레드에서 처리하므로 이 값으로 자식 회수 완료를 판단하지 않는다.
- C5a의 파일 삭제가 실패하면 다음 시작 때 `scrollback_store::gc_orphans`가 회수한다.
- C5c는 observer 워커를 join하지 않는다. 회수는 종료 단계 S3b에서 진행한다.
- C5d는 memory.db 크기의 영향을 받는다. 위 표는 새 `TASTY_HOME`에서 측정했으므로
  큰 DB의 비용과 같다고 볼 수 없다.

### 캡처 비용 (C1)

C1 은 surface 마다 화면(rows x cols)과 스크롤백 전량을 `ClosedItem` 으로 복제한다.
스크롤백 쪽은 라인 단위가 아니라 **벌크**로 가져온다 —
`Terminal::scrollback_lines_all()` 이 terminal state mutex 를 한 번만 잡고
스크롤백의 저장 표현인 `ScrollbackLine`(단일 text 버퍼 + cell 길이 + RLE 속성 런)을
그대로 복제한다. 라인당 비용이 헤더 3개 복제로 고정돼 cell 수에 비례하지 않는다.

라인당 경로(`scrollback_line_full`)도 남아 있지만 selection / search / link 처럼
소수 라인만 만지는 소비자용이다. 벌크 캡처에 쓰면 두 가지가 겹쳐 비싸진다:

- 라인마다 state mutex — 파서 스레드가 `ingest` 로 잡는 것과 같은 lock(ADR-0013)
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

현재는 surface scope purge를 C5d에서 한 번만 수행한다. 위 표는 중복 제거 전의
기록이므로 현재 C5d 비용이나 전체 close 시간으로 환산하지 않는다. 큰 DB에서
close가 느리면 C4와 C5d를 따로 확인한다.

### `scrollback_disk_swap`

탭 10개·스크롤백 만재(`seq 1 30000`), `performance.scrollback_disk_swap = true`,
새 `TASTY_HOME`. disk swap 이 켜지면 상한 10000 줄을 넘겨 유지하므로 캡처되는
`lines` 자체가 크게 늘어난다.

| | lines | close_total | C1 | C2b | C5b |
|---|---|---|---|---|---|
| off | 100000 | 601 | 33 | 41 | 521 |
| on | 279622 | 1445 | 544 | 246 | 547 |

> 이 표는 자식 종료 대기를 별도 스레드로 옮기기 전의 기록이다. 당시 C5b에는
> surface당 약 50ms의 대기가 포함됐다. 현재 값은 이 수치를 단순히 빼서 구할 수 없다.

disk 영역 라인은 캡처가 메모리 복제가 아니라 **라인마다 파일 read** 라 C1 의
라인당 단가가 memory-only 대비 한 자릿수 배 높다(10k 라인당 3ms → 19ms). 켜고
쓸 때 캡처 비용을 비교할 때는 전체 라인 수와 디스크 영역 라인 수를 함께 확인한다.

## 관련

- [boot-sequence](boot-sequence.md) — 부팅 계측(T1~T7)
- [shutdown-sequence](shutdown-sequence.md) — 종료 계측(S1~S5), C5b 와 같은 PTY drop 누적기를 S5b 로 소비
- [debug-ipc](../dev-guide/debug-ipc.md) — `debug.close_workspace`
