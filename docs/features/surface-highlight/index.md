# Surface Highlight (주의 환기)

- **Status**: Implemented
- **주체**: AI Agent · 로컬 사용자
- **ADR**: [ADR-0039](../../adr/0039-surface-highlight-shared-primitive.md)
- **코드**: `src/core/state/highlight.rs` (상태·헬퍼) · `src/gfx/gpu.rs` (focus 해제) · `src/adapters/ui/{divider,tab_bar,sidebar}` (소비처) · `src/adapters/ipc/handler/surface/completion.rs` (completion producer)
- **화면**: 없음 — 세 소비처(테두리·탭·사이드바)에 투영되는 상태 (전용 화면 없음)

## 목적

surface 가 "확인 대기(주의 환기)" 상태임을 사용자에게 알린다. 여러 경로(producer)가
같은 상태를 발동시킬 수 있는 **producer 중립 공유 primitive** 로, 특정 producer(toast·
completion 등)의 소유물이 아니다. 이렇게 두면 후속 producer(hook·명령완료 자동감지·plugin)가
같은 API 를 호출해 동일한 시각 효과를 재사용할 수 있다.

## 내부 동작 (headless-valid)

- **상태**: CoreState `highlighted_surfaces: HashSet<u32>` (surface id 집합). `busy_surfaces` 미러.
- **발동(raise)**: `raise_surface_highlight(surface_id)` — `surface_id == 0`(미지정)은 무시.
- **해제(clear)**: `clear_surface_highlight(surface_id)`. 두 경로에서 호출된다:
  1. 매 렌더 프레임 **실제 렌더 시점 포커스** surface 에 대해 자동 호출(`gpu.rs`) —
     에이전트가 주입한 포커스가 아니라 실 사용자 포커스라 불가침 원칙 1 에 안전.
  2. 그 surface 를 source 로 하는 알림을 읽음 처리(개별/전체)했을 때, `CoreState::
     mark_notification_read`/`mark_all_notifications_read`(`state/highlight.rs`)가
     그 surface 에 남은 안읽음 알림이 없는 경우에만 호출 — 같은 surface 의 다른 알림이
     아직 안읽음이면 지우지 않는다(TODO 23).
- **조회**: `is_surface_highlighted(id) -> bool`, `has_highlight(&[id]) -> bool`,
  `highlight_count(&[id]) -> usize`.
- **효과 3채널**:
  1. **테두리** — highlight surface 둘레 강조 보더 (`divider.rs`).
  2. **탭 제목** — 그 탭에 highlight surface 가 있으면 제목색 강조(yellow, `accent_warning`).
     busy 녹색 dot 과 별개 채널 (`tab_bar.rs`).
  3. **워크스페이스 개수 배지** — 사이드바 워크스페이스 행 우측: full 은 highlight surface **개수
     숫자 배지**(디자인 Badge variant="primary", 99 초과 "99+"), collapsed 은 dot (`sidebar/view.rs`).

## Producer

highlight 를 발동시키는 경로. 여러 종류가 공존한다 — highlight 자체는 어느 producer 에도
종속되지 않는다.

- **Toast 알림** — toast 알림이 신규 발화(coalesce 아님)되면 그 source surface 를 highlight
  (`dispatch_domain.rs` 주경로 + `event_handler.rs` windows resume 경로).
- **Completion (IPC/CLI, 이번 범위)** — surface 가 작업을 완료했다는 신호. `surface.completion`
  IPC / `tasty surface completion --surface <id>` CLI. cascade 가 `raise_surface_highlight` + redraw.
- **후속(미구현)** — hook · 명령완료 자동감지 · plugin. highlight API 는 이들이 호출 가능하게 열려 있다.

## 인터페이스

- **AI Agent (IPC/CLI)**: `tasty surface completion --surface <id>` / IPC `surface.completion`
  `{ surface_id }` — 대상 surface 를 highlight 발동. `surface_id` **필수**(포커스 독립, 불가침 원칙 1).
  권한 `Notification`.
- **사용자 트리거**: 직접 트리거 없음. 효과는 세 소비처로 표시되고, surface 에 포커스하거나
  그 surface 발 알림을 알림 패널에서 읽음 처리하면 해제(잔여 안읽음이 없을 때).

## 비-목표 (Out of scope)

- 에이전트가 포커스를 주입해 highlight 를 해제하는 경로(사용자 입력 재현 → debug 격리 대상).
- completion 외 다른 highlight 의미/카테고리(예: "확인필요" 분류) — 후속.
- toast 알림 시스템 자체 리팩터링(발행/coalesce/store 는 불변, highlight insert 위치만 이전됨).

## Acceptance Criteria

- [ ] Given surface S When `tasty surface completion --surface S` Then S 가 highlight 되어
      `has_highlight([S])` 가 true.
- [ ] Given highlight 된 S When S 가 실제 포커스를 얻음 Then highlight 자동 해제.
- [ ] Given 워크스페이스 W 에 highlight surface N개 Then full 사이드바에 개수 배지 N(>99 시 "99+").
- [ ] Given toast 알림 신규 발화 Then 그 source surface 가 highlight (producer 공존 회귀 없음).
- [ ] Given surface S 발 안읽음 알림 1건 When 그 알림을 읽음 처리 Then S 의 highlight 해제.
- [ ] Given surface S 발 안읽음 알림 2건 When 그중 1건만 읽음 처리 Then S 의 highlight 유지(엣지
      케이스, TODO 23) — 나머지도 읽음 처리하면 그제서야 해제.

## 구현

- 상태·헬퍼: `src/core/state/highlight.rs` (`raise/clear/is/has/count`).
- completion producer: intent `SurfaceCompletion` → event `SurfaceCompletionRequested`
  (`src/core/{intent.rs,mod.rs}`) → `cascade_surface_completion` (`src/app/dispatch_domain.rs`).
- 알림 읽음 clear: `CoreState::mark_notification_read`/`mark_all_notifications_read`
  (`src/core/state/highlight.rs`) — `NotificationStore::has_unread_for_surface`
  (`src/store/notification.rs`)로 잔여 안읽음을 확인 후 clear. 호출부는
  `cascade_notification_read`/`cascade_all_notifications_read`(`src/app/dispatch_domain.rs`).
