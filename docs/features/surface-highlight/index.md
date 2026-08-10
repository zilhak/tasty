# Surface Highlight (주의 환기)

- **Status**: Implemented
- **주체**: AI Agent · 로컬 사용자
- **ADR**: [ADR-0039](../../adr/0039-surface-highlight-shared-primitive.md) ·
  [ADR-0062](../../adr/0062-attention-store-kind-aware-primitive.md)(kind 확장)
- **코드**: `src/core/state/attention.rs` (상태·헬퍼) · `src/gfx/gpu.rs` (focus 해제) ·
  `src/adapters/ui/{divider,tab_bar,sidebar}` (소비처) ·
  `src/adapters/ipc/handler/surface/completion.rs` (completion producer) ·
  `crates/tasty-plugin-claude/src/hook.rs` (Claude Stop/session-end/notification hook producer) ·
  `src/app/dispatch_domain.rs::cascade_terminal_command_completed` (OSC 133 명령 완료 producer)
- **화면**: 없음 — 세 소비처(테두리·탭·사이드바)에 투영되는 상태 (전용 화면 없음)

## 목적

surface 가 "확인 대기(주의 환기)" 상태임을 사용자에게 알린다. 여러 경로(producer)가
같은 상태를 발동시킬 수 있는 **producer 중립 공유 primitive** 로, 특정 producer(toast·
completion 등)의 소유물이 아니다. 이렇게 두면 후속 producer(hook·명령완료 자동감지·plugin)가
같은 API 를 호출해 동일한 시각 효과를 재사용할 수 있다.

## 내부 동작 (headless-valid)

- **상태**: `AttentionStore`(`CoreState.attention`) — surface id → `{ kind, raised_at }`
  레코드. `busy_surfaces` 형태(폴링 아닌 직접 CRUD 헬퍼)를 미러하되, bool 집합이 아니라
  kind 를 들고 있어 "왜 발동했는지" 를 표현한다. kind 는 `Completion`(작업 완료) ·
  `NeedsInput`(응답 대기, 우선순위가 더 높다 — 아래 "kind → 효과") 2종.
  `NotificationStore`(알림 패널)와는 **별개 저장소** — attention 레코드가 곧 패널 아이템은
  아니다(아래 "kind → 효과" 참고).
- **발동(raise)**: `raise_attention(surface_id, kind)` — `surface_id == 0`(미지정)은 무시.
  같은 surface 에 다시 raise 하면 최신 kind 로 완전히 대체된다(레코드는 surface 당 1개).
- **해제(clear)**: `clear_attention(surface_id)`. 두 경로에서 호출된다:
  1. 매 렌더 프레임 **실제 렌더 시점 포커스** surface 에 대해 자동 호출(`gpu.rs`) —
     에이전트가 주입한 포커스가 아니라 실 사용자 포커스라 불가침 원칙 1 에 안전. kind 와
     무관하게 단일 규칙(`Completion`/`NeedsInput` 모두 이 경로로 해제).
  2. 그 surface 를 source 로 하는 알림을 읽음 처리(개별/전체)했을 때, `CoreState::
     mark_notification_read`/`mark_all_notifications_read`(`state/attention.rs`)가
     그 surface 에 남은 안읽음 알림이 없는 경우에만 호출 — 같은 surface 의 다른 알림이
     아직 안읽음이면 지우지 않는다(무조건 clear 시 다른 안읽음 알림의 주의 환기를
     오해제하는 엣지 케이스가 있어 조건부로만 지운다).
- **조회**: `attention_kind(id) -> Option<AttentionKind>`,
  `attention_count_of_kind(kind, &[id]) -> usize`,
  `attention_dominant_kind(&[id]) -> Option<AttentionKind>`(목록 중 최고 우선순위 kind —
  탭 제목·collapsed rail dot 처럼 여러 surface 를 하나의 색으로 압축해야 하는 소비처 전용).
- **kind → 효과**: `effects_of(kind) -> AttentionEffects { level, panel_item, os_notify, sound }`
  가 host/cascade 비의존 순수 함수로 정책을 표현한다(`crates/tasty-plugin-claude/src/hook.rs`
  의 `apply_hook` 과 동형 패턴). `level` 은 디자인 rank 토큰(`--tasty-attention-rank-*`)을
  미러링한 우선순위 — `NeedsInput`(30) > `Completion`(10). 두 kind 모두 `panel_item = false`
  — attention 레코드 자체는 알림 패널 아이템을 만들지 않는다. 패널 노출이 필요한
  producer(toast, windows resume)는 지금처럼 `NotificationStore::add()` 를 별도로 직접
  호출한다. OSC 133 명령 완료는 이 `add()` 를 호출하지 않으므로, 테두리/탭 제목은
  발동하되 알림 패널에는 아이템이 쌓이지 않는 조합이 성립한다.
- **효과 3채널** (kind 별로 색이 갈린다 — `NeedsInput`=노랑(`accent_warning`),
  `Completion`=파랑(`accent_primary`)):
  1. **테두리** — attention surface 둘레 강조 보더(2px, `divider.rs`). 우선순위:
     `NeedsInput` > 점유(soft/hard, ADR-0040) > `Completion` — 점유 중 surface 는
     `Completion` 테두리를 억제하지만 `NeedsInput` 은 억제하지 않는다("지금 답하지 않으면
     멈춘다"는 신호를 점유가 가리면 안 되기 때문).
  2. **탭 제목** — 그 탭에 속한 surface 들의 `attention_dominant_kind` 로 제목색 결정.
     순서: `NeedsInput`(노랑) → `Completion`(파랑) → active(`text_primary`) → 평상시
     (`text_muted`). busy 녹색 dot 과 별개 채널(`tab_bar.rs`).
  3. **워크스페이스 배지/dot** — 사이드바 워크스페이스 행 우측: full 은 kind 별 개수
     숫자 배지 2종(트레일링 슬롯은 kind 무관 유지 — 1개면 단독 그 자리, 2개면 `NeedsInput`
     이 좌측·`Completion`이 우측, 간격 `badge-group-gap`=`spacing_xs`), collapsed 은 dot
     1개(kind 우선순위로 대표색 선택: `NeedsInput` > `Completion` > running) (`sidebar/view.rs`).

## Producer

attention 을 발동시키는 경로. 여러 종류가 공존한다 — attention 자체는 어느 producer 에도
종속되지 않는다. Toast/completion IPC·CLI/OSC 133 은 `kind = Completion` 으로,
Claude hook 은 이벤트별로 `Completion` 또는 `NeedsInput` 으로 발동한다.

- **Toast 알림** — toast 알림이 신규 발화(coalesce 아님)되면 그 source surface 를 attention
  (`dispatch_domain.rs` 주경로 + `event_handler.rs` windows resume 경로). 이 두 경로는
  `NotificationStore::add()` 를 직접 호출해 패널 아이템도 함께 만든다. `kind = Completion`.
- **Completion (IPC/CLI)** — surface 가 작업을 완료했다는(또는 응답이 필요하다는) 신호.
  `surface.completion` IPC / `tasty surface completion --surface <id>` CLI. `kind` 는
  선택 파라미터(`"needs_input"` 이면 `NeedsInput`, 생략 포함 그 외는 `Completion` — 하위
  호환: CLI/OSC 133/toast/windows-resume producer 는 kind 를 모른 채 이 IPC 를 호출한다).
  cascade 가 `raise_attention(surface_id, kind)` + redraw. 패널 아이템은 만들지 않는다.
- **Claude hook (plugin)** — `crates/tasty-plugin-claude/src/hook.rs` 의 `apply_hook`이
  이벤트별로 `HostCall::SurfaceCompletion { surface_id, kind }`를 산출하고, `deliver()`가
  이를 위 `surface.completion` IPC(`{ "surface_id": surface_id, "kind": kind }`)로 매핑해
  그대로 재사용한다 — 새 프로토콜을 만들지 않고 기존 completion producer 를 plugin 이
  호출하는 형태.
  - `"stop"`/`"subagent-stop"`(턴 완료)·`"session-end"`(세션 종료) → `kind = "completion"`.
  - `"notification"`(비-`idle_prompt` — 승인/plan 허락 등 확인 대기)·`"pre-tool-use"`
    (`AskUserQuestion` matcher — 선택지 UI) → `kind = "needs_input"`.
  - `"prompt-submit"`/`"session-start"`/`"active"`(작업 시작 신호)·`"post-tool-use"`(답변
    직후 active 복귀)는 대상이 아니다.
  - `tasty claude install`이 등록한 훅(`crates/tasty-plugin-claude/src/install.rs`)을 통해
    Claude 세션이 이벤트를 발화할 때마다 자동 — 별도 사용자 조작 불필요.
- **OSC 133 명령 완료 (셸 통합)** — `cascade_terminal_command_completed`
  (`src/app/dispatch_domain.rs`)가 OSC 133 D phase(개별 셸 명령 종료)
  를 받을 때마다 exit code(성공/실패) 무관하게 항상 `raise_attention(surface_id,
  AttentionKind::Completion)` 을 호출한다 — Claude plugin 과 달리 IPC 왕복 없이 host
  프로세스 내부에서 engine 을 직접 호출(같은 cascade 함수가 이미 `engine`을 들고 있음).
  exit code 정보는 이 호출로 소비되지 않고 같은 이벤트가 이미 채우는
  `command_index::on_boundary` 의 memory 기록(`tasty.commands.*` 의 `exit_code`
  필드)과 `HookEvent::CommandCompleted(exit_code)` 훅 payload 양쪽에
  그대로 보존된다. 셸 프로세스 자체의 시작/끝(`terminal.spawn`/`ProcessExit`)이
  아니라 그 **안에서 실행되는 개별 명령**(`docker build` 등) 단위로 발동한다는 점이
  Claude producer 와의 차이 — 셸이 OSC 133 통합을 로드해야만 동작(미설치 시
  "셸 통합 미설치" 안내 배너만 뜨고 attention 은 발동하지 않음). `NotificationStore::add()`
  를 호출하지 않으므로 알림 패널에는 아이템이 쌓이지 않는다.
- **후속(미구현)** — 그 외 plugin(non-Claude AI 코딩 에이전트 등)이 자체 완료/확인대기
  신호를 이 attention API 에 연결하는 것. attention API 는 이들이 호출 가능하게 계속
  열려 있다.

## 인터페이스

- **AI Agent (IPC/CLI)**: `tasty surface completion --surface <id> [--kind needs_input]` /
  IPC `surface.completion { surface_id, kind? }` — 대상 surface 를 attention 발동.
  `surface_id` **필수**(포커스 독립, 불가침 원칙 1). `kind` 생략(또는 `completion` 외 값)은
  `Completion`. 권한 `Notification`.
- **사용자 트리거**: 직접 트리거 없음. 효과는 세 소비처로 표시되고, surface 에 포커스하거나
  그 surface 발 알림을 알림 패널에서 읽음 처리하면 해제(잔여 안읽음이 없을 때, kind 무관).

## 비-목표 (Out of scope)

- 에이전트가 포커스를 주입해 attention 을 해제하는 경로(사용자 입력 재현 → debug 격리 대상).
- `Completion`/`NeedsInput` 외 다른 kind(예: `error` rank 40, `approval` rank 20 — 디자인
  토큰은 예약돼 있으나 이번 구현 범위 밖).
- toast 알림 시스템 자체 리팩터링(발행/coalesce/store 는 불변, attention insert 위치만 이전됨).

## Acceptance Criteria

- [ ] Given surface S When `tasty surface completion --surface S` Then S 가 attention 발동되어
      `attention_kind(S) == Some(Completion)`.
- [ ] Given surface S When `tasty surface completion --surface S --kind needs_input` Then
      `attention_kind(S) == Some(NeedsInput)`.
- [ ] Given attention 발동된 S When S 가 실제 포커스를 얻음 Then attention 자동 해제(kind 무관).
- [ ] Given 워크스페이스 W 에 `Completion` surface M개·`NeedsInput` surface N개 Then full
      사이드바에 파란 배지 M·노란 배지 N 이 공존(각각 >99 시 "99+"), N==0 이면 파란 배지만
      단독으로 우측(기존 자리)에 표시.
- [ ] Given 대상(탭/워크스페이스)에 `Completion`·`NeedsInput` surface 가 섞여 있음 Then
      탭 제목 색과 collapsed rail dot 색은 `NeedsInput` 이 이긴다(우선순위 30 > 10).
- [ ] Given surface S 가 점유(soft/hard, ADR-0040) 중 When `NeedsInput` attention 이 발동
      Then 테두리가 억제되지 않고 노랑으로 그려진다(점유는 `Completion` 만 억제).
- [ ] Given toast 알림 신규 발화 Then 그 source surface 가 attention 발동(producer 공존 회귀 없음).
- [ ] Given surface S 발 안읽음 알림 1건 When 그 알림을 읽음 처리 Then S 의 attention 해제.
- [ ] Given surface S 발 안읽음 알림 2건 When 그중 1건만 읽음 처리 Then S 의 attention 유지(엣지
      케이스) — 나머지도 읽음 처리하면 그제서야 해제.
- [ ] Given surface S 에서 Claude 세션 실행 중 When Claude 가 한 턴 응답을 마쳐 Stop hook
      (`claude.hook stop`)이 발동 Then S 가 `Completion` kind 로 attention 발동.
- [ ] Given surface S 에서 Claude 가 승인/plan 허락 등 확인이 필요해 `notification`(비-
      `idle_prompt`) 또는 `pre-tool-use`(`AskUserQuestion`) hook 이 발동 Then S 가
      `NeedsInput` kind 로 attention 발동 — `Completion` 과 다른 색(노랑)으로 구분된다.
- [ ] Given surface S 에서 Claude 세션이 끝나(`session-end` hook) 자식 상태가 정리될 때 Then
      S 가 동일하게 attention 발동.
- [ ] Given surface S 에서 Claude 가 `prompt-submit`/`session-start`/`active`(작업 시작) 신호를
      보낼 때 Then attention 은 발동하지 않는다(완료/확인대기와 구분, 회귀 방지).
- [ ] Given surface S 에서 OSC 133 셸 통합이 설치된 셸이 명령을 성공으로 종료(exit code 0)
      When D phase 가 도착 Then S 가 `Completion` kind 로 attention 발동.
- [ ] Given 위와 동일 상황이지만 명령이 실패로 종료(exit code != 0) Then 동일하게 S 가
      attention 발동(성공/실패 무관, exit code 로 필터링하지 않음).
- [ ] Given 위 두 케이스 모두 Then exit code 정보가 `command_index` memory 기록과
      `HookEvent::CommandCompleted` 훅 payload 양쪽에서 유실되지 않는다(하나의 이벤트가
      attention/memory/hook 세 소비처로 fan-out).
- [ ] Given surface S 에서 OSC 133 명령이 완료 Then attention 은 발동하지만 알림 패널에는
      아이템이 생기지 않는다(`AttentionStore` 와 `NotificationStore` 가 별개 저장소).

## 구현

- 상태·헬퍼: `src/core/state/attention.rs` (`AttentionStore`, `AttentionKind`, `AttentionLevel`,
  `effects_of`, `raise_attention`/`clear_attention`/`attention_kind`/
  `attention_count_of_kind`/`attention_dominant_kind`).
- completion/needs-input producer: intent `SurfaceCompletion { surface_id, kind }` → event
  `SurfaceCompletionRequested { surface_id, kind }` (`src/core/{intent.rs,mod.rs}`) →
  `cascade_surface_completion(surface_id, kind)` (`src/app/dispatch_domain.rs`).
  `src/adapters/ipc/handler/surface/completion.rs::handle_completion` 가 IPC 파라미터의
  `kind` 문자열(`"needs_input"` 외 전부 `Completion`)을 파싱.
- Claude hook producer: `apply_hook` (`crates/tasty-plugin-claude/src/hook.rs`) → `HostCall::
  SurfaceCompletion { surface_id, kind }` → `deliver()` 가 `surface.completion` IPC 호출로
  매핑(`{ "surface_id", "kind" }`) → 위 producer 경로 그대로 재사용.
- OSC 133 명령 완료 producer: `Core::apply_terminal_event` (`src/core/mod.rs`, D phase 파싱)
  → `CoreEvent::TerminalCommandCompleted { surface_id, exit_code }` (`src/core/intent.rs`)
  → `App::cascade_terminal_command_completed` (`src/app/dispatch_domain.rs`)가
  `engine.raise_attention(surface_id, AttentionKind::Completion)` 직접 호출(자동 경로) +
  `HookEvent::CommandCompleted` 훅 발화(커스터마이즈 경로) 동시 처리.
- 알림 읽음 clear: `CoreState::mark_notification_read`/`mark_all_notifications_read`
  (`src/core/state/attention.rs`) — `NotificationStore::has_unread_for_surface`
  (`src/store/notification.rs`)로 잔여 안읽음을 확인 후 clear. 호출부는
  `cascade_notification_read`/`cascade_all_notifications_read`(`src/app/dispatch_domain.rs`).
