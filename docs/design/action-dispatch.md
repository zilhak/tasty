# Action Dispatch (Intent 큐)

> **스코프**: 호스트 내부의 동작 디스패치 (popup open, preset apply, surface split 등 모듈 경계를 넘는 동작). plugin ↔ host 경계의 read 채널인 Event Bus 1.0과는 **다른 layer**이며 별도 네임스페이스다.

## 배경

호스트 내부에서 동일한 동작(예: command_palette popup 열기)을 단축키·도구 메뉴·IPC·plugin·우클릭 메뉴 등 여러 진입점에서 발화한다. 진입점마다 디스패치 방식이 달라 일부는 `mem::replace`로 take된 빈 PopupManager에 직접 호출되어 **noop 버그**가 발생한다.

산발적으로 도입된 deferred 1슬롯 큐(`dialogs.pending_popup_open`, `pending_preset_save`, `pending_preset_apply`, `pending_open_preset_window` 등)는 Intent 큐의 부분 구현이다. 흩어진 큐를 단일 진입점으로 통합하면 발화 위치 무관하게 메인 루프 단계에서 일관되게 처리된다.

## 용어

| 용어 | 정의 |
|------|------|
| **Intent** | 호스트 내부 명령. "host가 다음 메인 루프 단계에 무엇을 해야 하는가". 큐를 통과한다. Event Bus 발화 키와 다른 네임스페이스. |
| **Event** | Event Bus 1.0의 broadcast/unicast 사건. 이미 일어난 일의 알림. plugin이 구독한다. Intent와는 다른 layer. |
| **Origin** | Intent를 발화한 주체. `User { source }` 또는 `Agent { source }`. 핸들러가 정책 분기에 사용. |
| **Cascade** | popup A → popup B 같은 파생 Intent. 별도 origin 종류를 만들지 않고, 호출 측이 직전 Intent의 origin을 명시 전달한다. |
| **Bridge** | Intent → Event Bus 변환 단일 지점. envelope의 `meta.origin` / `meta.trace_id`를 채운다. |

## 원칙

1. **모듈 경계를 넘는 동작 / 둘 이상의 진입점에서 발화 가능한 동작 / draw 콜백 안에서 자신이 속한 매니저를 mutate해야 하는 동작 / timing이 중요한 cascade 동작은 반드시 Intent를 통과한다.**
2. **단일 모듈 내 선형 호출은 Intent를 강제하지 않는다** (init 시 popup 등록, 단순 필드 setter, draw-prep 동기화).
3. **진입점은 Intent를 발화할 뿐, 핸들러를 직접 호출하지 않는다.**
4. **Intent 처리 후 결과 변화는 Event Bus 1.0을 통해 plugin/hook에 알린다.** 변환은 중앙 bridge 한 곳에서만.

### 핵심 원칙 — Intent 의 위치

> **Intent 는 "의도" 다 — 이벤트 처리 흐름의 *시작점* 에만 존재할 수 있다.
> 끝부분에서 *결과로써* 나오거나, 중간에서 *정보 전달용도로* 쓰이면 안 된다.**

흐름은 항상 한 방향:

```
이벤트 (발화지점) → 해석된 의도 (Intent → 큐로) → 처리 (drain) → 결과 (state mutate / cascade)
```

**Intent 의 형식적 위치 vs 의미적 위치 — 헷갈리기 쉬운 부분**

함수가 `-> Intent` 또는 `-> Option<Intent>` 를 반환해도, 그 함수가 호출되는
*위치* 에 따라 합법/불법이 갈린다:

```rust
// ✓ OK — 이벤트 해석 함수. 시작점에서 호출됨.
fn parse_user_input(event: WindowEvent) -> Option<Intent> {
    // 이벤트 → Intent 변환. 리턴된 Intent 는 곧장 큐로 발화된다.
}

// ✗ 금지 — Intent 처리 핸들러의 *리턴 경로* 로 새 Intent 가 흘러나옴.
fn handle_intent(intent: &DispatchedIntent) -> Option<Intent> {
    // 처리 결과로 Intent 를 반환하는 시그니처 자체가 잘못된 디자인.
    // cascade 가 필요하면 함수 본문 안에서 `state.dispatch_intent(...)` 로 *큐 발화*.
}
```

따라서 다음은 모두 **잘못된 패턴**:

1. **처리 핸들러가 새 Intent 를 *리턴 경로로* 흘려보냄**
   — 형식이 어떻게 생겼든, Intent context *안* 에서 흘러나오면 안 된다.
   cascade 는 *큐에 enqueue* 만 (함수 본문 안에서).
2. **Intent 가 응답 데이터를 가짐** (`Result<Intent, _>`, `ApplyResult { data }` 등)
   — Intent 는 데이터 전달 매개체가 아니다.
3. **Intent enum 에 응답 필드를 넣음**
   — 응답이 필요한 mutate 는 *Intent 가 아니다*.

응답이 필요한 mutate (예: 새 ID 발급, 분기 결정용 status) 는 *Intent 가 아니라*
**Core method 호출** (sync 리턴) 또는 **Query (read)** 로 처리한다 — Intent 와는
다른 개념의 메커니즘.

#### 허용되는 패턴

- **이벤트 → Intent 변환 함수 리턴**: `fn parse(e) -> Option<Intent>` — 시작점.
- **Intent 처리 후 결과를 보고 추가 행동**: 처리 함수 본문 끝에서 state 를
  다시 query 해 새 Intent 발화 (큐에).
- **cascade 발화**: Intent A 의 핸들러가 *본문 안에서* `state.dispatch_intent`
  로 Intent B 를 큐에 던짐.
- **coroutine yield**: Intent A 처리 중 *큐 우선순위* 로 다른 Intent 가 끼어들고
  완료된 후 A resume. 별 thread 의 coroutine runtime 이 host (별 문서 참조).

#### 분류표

| 유형 | 메커니즘 | 응답 |
|------|----------|------|
| Query | `&CoreReader` / `&CoreState` 직접 read | 데이터 |
| **Intent** (fire-and-forget) | enqueue → cascade | **없음** |
| Core method | `core.create_workspace(...) -> WorkspaceCreated` | sync 리턴 |
| Long-poll | worker thread blocking | 별개 패턴 |

#### Cascade — Intent 처리 중 새 Intent 발화

Intent A 의 핸들러가 *처리 도중* 새 Intent B 를 발화하는 것은 허용된다.
**단, 반드시 함수 본문 안에서 큐로**:

```rust
// ✓ OK — 핸들러 본문 안에서 큐 발화. B 는 별도 흐름.
fn handle_some_intent(state: &mut AppState, intent: &DispatchedIntent) {
    // ... A 처리 ...
    state.dispatch_intent(
        Intent::OpenPopup { id: "approval_result", mode: ... }
            .cascaded_from(intent),  // origin 전파
    );
    // A 의 흐름은 여기서 끝. B 는 다음 drain 라운드에서 처리.
}

// ✗ 금지 — Intent A 의 핸들러가 *리턴 경로* 로 새 Intent 를 흘려보냄.
// 호출자가 그 Intent 를 다시 큐에 넣는 패턴 (`dispatch(handle(intent)?)`) 도 같은 죄.
fn handle_some_intent(...) -> Option<Intent> {  // ❌ 시그니처 자체가 잘못.
    Some(Intent::OpenPopup { ... })
}
```

이유: Intent 는 *발화* 와 *처리* 가 분리된 큐 모델이다. 처리 흐름의 *반환*
경로로 새 Intent 가 흘러나오면 호출 cascade 가 *재귀적 함수 호출 트리* 가
되어 큐 모델이 깨진다. 새 Intent 는 항상 *별개의 흐름* 으로 시작되어야 한다.

#### 사용자 입력 대기 — *반드시* 2 Intent 로 분리

"확인 popup → 사용자 응답 → 후속 작업" 같은 *사용자 입력 대기* 흐름은
**한 개의 Intent 처리 안에서 wait 하지 않는다**. 반드시 두 개의 Intent 로 분리.

```rust
// ✓ OK — 2 event 분리
// 1차 Intent: popup 띄움 + 관련 정보를 state 에 저장 + 종료
Intent::PresetApplyRequest { kind, name } 
    → handler 가 처리:
        state.dialogs.pending_preset_apply = Some(PresetApplyContext { kind, name });
        state.dispatch_intent(Intent::OpenPopup { id: "confirm_preset_apply", ... });
    → 1차 처리 끝.

// 2차 Intent: 사용자 응답 → state 에서 정보 읽어 진행 또는 폐기
Intent::PresetApplyConfirmed
    → handler 가 처리:
        let Some(ctx) = state.dialogs.pending_preset_apply.take() else { return; };
        // ctx 로 실제 작업.

Intent::PresetApplyCancelled
    → handler 가 처리:
        state.dialogs.pending_preset_apply = None;  // 정보 폐기.
```

```rust
// ✗ 금지 — 한 Intent 가 사용자 응답을 *기다림*
fn handle_preset_apply(state: &mut AppState, kind, name) {
    let confirmed = wait_for_user_confirmation();  // ❌
    if confirmed { apply(kind, name); }
}
```

이유:
- 사용자 응답 시간이 *무한정* — 한 Intent 가 메모리에 lock 되어 있으면
  *그 동안의 state 변경* 에 취약 (windowclose, settings change 등).
- 큐 모델의 일관성: Intent 는 *발화 시점에 결정된 모든 정보* 로 시작되어
  *유한 시간에 종료* 한다. 사용자 응답이 *Intent 의 일부* 가 되면 큐 정체.
- 명시적 분리는 state 에 *진행 중인 컨텍스트* 가 보이게 만들어 디버깅 용이.

#### "프로세스 내부 cascade 대기" — coroutine pattern (별 문서)

사용자 입력이 아닌 *시스템 내부* 의 multi-step 흐름 (예: 다른 workflow 의
acquire 완료, 시스템 자동 step 의 cascade 진행) 은 *경량 thread + coroutine*
으로 처리할 수 있다. main thread 와 mpsc 로 통신. 별도 설계 문서:
[`intent-coroutine.md`](./intent-coroutine.md).

**중요**: coroutine 도 *사용자 입력을 yield 로 기다리면 안 된다* — 사용자
입력은 위의 *2 event 분리* 로만 처리. coroutine 의 yield 는 *시스템 내부의
유한 시간 cascade 대기* 에만 쓴다.

## Intent 자료형

### 구조

`Intent`는 **flat enum**이다. 도메인이 늘어나도 nested 하지 않는다.

```rust
pub enum Intent {
    Noop,
    OpenPopup { id: PopupId, mode: OpenPopupMode },
    ClosePopup { id: PopupId },
    TogglePopup { id: PopupId, mode: OpenPopupMode },
    // 도메인 추가 (preset, surface, tab, pane, workspace, ...) 시 여기에 variant 추가
}

pub enum OpenPopupMode {
    Default,
    CenteredFocused,
    WithScope(PopupScope),
    AtTopOfScope(PopupScope),
    AtFocused(egui::Pos2),
}
```

variant는 `src/intent/mod.rs`에 모은다. 도메인별 핸들러는 `src/intent/<domain>.rs`로 분리한다 (예: `intent/popup.rs`, `intent/preset.rs`).

### Envelope

발화 시 항상 envelope(`DispatchedIntent`)에 감싼다.

```rust
pub struct DispatchedIntent {
    pub body: Intent,
    pub origin: IntentOrigin,
    pub trace_id: Option<String>,  // None이면 bridge가 새로 발급
}

pub enum IntentOrigin {
    User { source: UserSource },
    Agent { source: AgentSource },
}

pub enum UserSource {
    Shortcut(&'static str),
    Menu(&'static str),
    ContextMenu,
}

pub enum AgentSource {
    Ipc,
    Plugin(String),
    Cli,
}

impl IntentOrigin {
    pub fn is_user(&self) -> bool { matches!(self, IntentOrigin::User { .. }) }
    pub fn is_agent(&self) -> bool { matches!(self, IntentOrigin::Agent { .. }) }
}
```

### Cascade origin 처리

popup A의 draw 콜백이 popup B를 여는 cascade에서, popup B의 Intent origin은 **호출 측이 popup A의 origin을 그대로 명시 전달한다**. 별도 `Cascade` variant를 만들지 않는다.

이유: "왜 이 popup이 열렸나"를 origin이 정확히 가리킨다. 처음 단축키로 시작된 approval cascade라면 cascade 끝의 popup도 `Shortcut("approve")` origin을 유지한다. 로깅/audit에서 시작 지점 추적이 가능하다.

## 발화 / 처리 흐름

### 발화 API

```rust
impl AppState {
    pub fn dispatch_intent(&mut self, intent: DispatchedIntent) {
        self.pending_intents.push(intent);
    }
}
```

내부 구현은 `Vec::push` 한 줄. 동기 처리 없음. 결과 반환 없음(fire-and-forget).

발화 ergonomics는 빌더로 제공한다.

```rust
Intent::OpenPopup { id: "command_palette", mode: OpenPopupMode::CenteredFocused }
    .from_user_menu("tools_menu/command_palette");
// → DispatchedIntent { body, origin: User { source: Menu("tools_menu/command_palette") }, trace_id: None }
```

### 처리 시점

메인 루프에서 `App::dispatch_pending_intents`가 모든 windows와 parked_states를 순회해 drain한다.

**호출 위치** (`src/event_handler.rs:460-471`): `dispatch_pending_tool_events` 다음, `dispatch_pending_popup_opens` 앞.

이유: Intent 처리 중 plugin popup 큐(`pending_popup_opens`)를 발화할 수 있어야 한다.

### 처리 순서

- 발화 순서대로 처리.
- drain 중 핸들러가 새로 발화한 Intent는 **다음 프레임**에 처리 (재진입 방지).
- 구현은 `mem::take(&mut state.pending_intents)` 후 별도 `Vec`를 순회.

### 핸들러 분기

`dispatch_pending_intents`는 **도메인 단위 함수 분기**를 한다. trait dispatch를 사용하지 않는다.

```rust
fn dispatch_one(&mut self, intent: DispatchedIntent) {
    match &intent.body {
        Intent::OpenPopup { .. } | Intent::ClosePopup { .. } | Intent::TogglePopup { .. } => {
            crate::intent::popup::handle(self, &intent);
        }
        // Intent::ApplyPreset { .. } | Intent::SavePreset { .. } => {
        //     crate::intent::preset::handle(self, &intent);
        // }
        Intent::Noop => {}
    }
}
```

trait를 쓰지 않는 이유: `&mut AppState`를 trait object가 통째로 잡으면 borrow checker가 핸들러 내부의 partial mutation을 통과시키지 못한다. flat enum + 도메인 함수가 가장 단순하다.

### 에러 처리

- 핸들러 분기 안에서 에러 시 `tracing::warn!`.
- 패닉 금지.
- `Result<_, _>`를 `let _ =`로 무시 금지 (`CLAUDE.md` 규칙).
- 사용자에게 보여야 하면 핸들러가 `state.toasts.push`로 toast 추가.

## Intent → Event Bus Bridge

핸들러가 mutation 성공 후 결과를 알려야 하면 `state.pending_host_events`에 host event를 push한다. 기존 `dispatch_pending_host_events`가 Event Bus 1.0 envelope로 변환·발화한다.

bridge가 envelope의 메타데이터를 채울 때 다음 규칙을 따른다 (변환 함수는 `src/intent/mod.rs`에 한 곳).

| envelope 필드 | 규칙 |
|---------------|------|
| `meta.origin` | `IntentOrigin` → envelope origin 변환. `User` → `{"kind":"host"}`, `Agent { source: Plugin(id) }` → `{"kind":"plugin","plugin_id":id}`, 그 외 Agent → `{"kind":"host"}` |
| `meta.trace_id` | `intent.trace_id`가 `Some`이면 그대로, `None`이면 `Uuid::new_v4().to_string()` 새로 발급 |
| `meta.scope` | 핸들러가 mutation 결과를 보고 채움 (`"system"` / `"surface"`). Intent envelope 자체에는 `scope` 없음 |
| `meta.hop` | 호스트 발화이므로 `0`. plugin이 다시 publish하면 기존 정책대로 `+1` |

## 정책 차이: User Intent vs Agent Intent

Tasty의 개발 정책상 다음 차이를 둔다. **dispatcher가 강제 거부하지 않으며**, 핸들러 작성자가 다음 표를 따라 분기를 작성한다 (PR 리뷰에서 강제).

| 동작 | User Intent | Agent Intent |
|------|-------------|--------------|
| **popup open 자체 (UI Intent 발화)** | **가능** | **금지** (debug 빌드의 사용자 입력 재현 IPC 제외) |
| popup focus 셋팅 | 가능 (`open_centered_focused`) | 금지 (release 표면에 agent popup 자체 없음) |
| closed-tab restore 스택 push | 가능 | 금지 |
| OS 윈도우 focus API 호출 | 가능 | 금지 (focus 독립성 원칙) |
| Workspace activate | 가능 | 금지 (focus 독립성 원칙) |
| Window 생성 (PresetView 등) | 가능 | 금지 |

분기 위치는 핸들러 본문의 `origin.is_user()`. enum variant로 분기하지 않는다.

> **UI Intent vs Domain Intent** (제1 분류축): 모든 Intent 는 *시각 상태 변경*
> (UI: popup open/close/toggle 3개) 또는 *영속 도메인 mutate* (Domain: 나머지)
> 중 하나. release 빌드의 Domain Intent 처리 흐름은 UI Intent 를 발화할 수 없으며,
> 이는 *타입 차원에서 강제* 한다 (Core 가 `UiIntent` 를 모름). GUI adapter
> 안에서만 UI Intent 발화 가능. 자동 popup / 자동 toast / 자동 dialog 는 release
> 빌드에 존재하지 않는다. 상세: `../../.claude-workspace/plans/archived/phase-d/intent-ui-vs-domain.md`,
> 자매 정책: `toast-system.md` "트리거 정책 (CRITICAL)", `popup-system.md`
> "Popup 발화 정책 (CRITICAL)".

> `CLAUDE.md`의 "사용자 입력 재현은 debug 한정" 원칙은 IPC/CLI 표면에 적용된다. 호스트 popup 자체에는 release 빌드의 IPC가 없다. debug 빌드에서 사용자 입력 재현용 IPC를 추가할 경우 `#[cfg(debug_assertions)]`로 격리한다.

## 기존 패턴과의 관계

### `AppEvent` (winit user event proxy)

cross-thread winit user event 용도 (`src/main.rs:120-162`). Intent와 layer가 다르므로 합치지 않는다. winit event_loop가 `AppEvent`를 받으면 필요 시 Intent로 변환해 dispatch.

### `pending_host_events`

기존 host event 큐. Intent 핸들러가 mutation 후 결과 이벤트를 여기에 push한다. **Intent → Event Bus 변환의 단일 bridge**.

### `pending_popup_opens` (plugin용)

plugin manifest의 `[[contributes.popup]]` instance open 큐. 그대로 유지. 호스트 popup용 Intent는 별도 enum variant (`Intent::OpenPopup`).

### `dialogs.pending_popup_open` (popup deferred)

~~Intent 큐의 도메인 한정 부분 구현. TODO 04에서 흡수.~~ **TODO 04 에서 제거됨** —
모든 deferred open 요청은 `state.dispatch_intent(...)` 로 통일.

### `debug.popup.*` IPC 와의 관계

`debug.popup.open` / `debug.popup.close` IPC 핸들러 (`src/ipc/handler/popup.rs`) 는
**plugin popup instance** (`PluginManager::popup_instances`) 를 대상으로 한다.
호스트 PopupManager (`state.popups`) Intent 와 **다른 layer** 이며, debug 빌드 한정
사용자 입력 재현 (CLAUDE.md "사용자 입력 재현 = debug 한정" 원칙) 으로 분류된다.

호스트 popup 자체는 debug 빌드에서도 IPC 로 노출하지 않는다. host popup 직접 조작이
필요한 시나리오가 생기면 별도 debug IPC (`debug.host_popup.*`) 를 신설하되, 본 TODO
범위에서는 다루지 않는다.

### `dialogs.pending_preset_save / pending_preset_apply / pending_open_preset_window` (preset deferred)

popup-deferred와 동일 패턴의 1슬롯 큐 3종. TODO 07(preset 도메인 마이그레이션)에서 흡수.

### `fire_popup_triggers` (plugin event-trigger popup)

`src/plugin/manager.rs:415-437`. 호스트 event 발화 시 plugin manifest의 `trigger.kind = "event"` 매칭으로 plugin popup 자동 open. **호스트 Intent와 무관** (plugin popup은 별도 큐).

단, Intent → host event → `fire_popup_triggers` → plugin popup chain에서 envelope의 `trace_id`가 보존되도록 bridge가 보장한다.

### `command.invoked`

plugin owner unicast 계약. **호스트 내부 Intent 가시화에 재사용하지 않는다**. 호스트 통합 가시화 이벤트(`host.intent.dispatched` 등)는 카탈로그에 추가하지 않는다.

호스트 모든 Intent 발화를 가시화하고 싶을 때는 debug 빌드 전용 `intent::watch` 모듈이 `tracing::debug!`로 로그한다 (release 빌드에서 완전 제거).

## 예외 케이스 (Intent 강제 안 함)

다음 호출은 원칙 1·2의 예외다. 직접 호출을 유지하되 주석으로 사유를 명시한다.

| 호출 | 사유 |
|------|------|
| `notification.rs:165-169` `state.popups.get_mut(...).title = ...` | 매 프레임 i18n/sizer refresh. Intent 통과 시 1프레임 latency 발생. **draw-prep 동기화 예외** |
| `notification.rs:196` popup draw_result의 `state.popups.close(id)` | popup 라이프사이클 내부 cleanup. 자신을 닫는 동작이 Intent를 거치면 의미 순환 |
| `settings_ui/mod.rs:137` `ui_state.popups.register(...)` | Settings 윈도우 init 시점의 popup 등록. 단일 모듈 선형 호출(원칙 2) |

각 예외는 호출 라인 위에 `// intent-exempt: <사유>` 주석을 단다. clippy custom lint(TODO 05 산출물)도 이 주석을 allow-list로 인식한다.

## Intent dedup

같은 popup id로 OpenPopup이 동일 사이클에 중복 들어오면 핸들러가 `state.popups.is_open(id)` 체크로 두 번째 발화를 무시한다. 큐 push 시점에서는 dedup하지 않는다 (origin/trace_id가 다른 두 발화를 같다고 판단할 근거 없음).

## 마이그레이션 강제력 (intent discipline)

popup 도메인 직접 호출 (`state.popups.open*` / `.close` / `.toggle*`) 은 grep 기반
CI 체크 스크립트 `scripts/check-intent-discipline.sh` 로 금지된다. 예외는 동일
라인에 `// intent-exempt: <사유>` 주석을 달아 suppress.

향후 dylint plugin ABI 안정화 시 동등한 clippy custom lint 로 승격. 현재 grep
체크는 `is_open` / `get_mut` / `register` 등 query API 는 매칭에서 제외한다.

후속 도메인 (preset, surface, tab, ...) 마이그레이션 시 본 스크립트의 패턴/예외
경로를 확장.

## 마이그레이션 정책

- 도메인별 점진 적용. 우선순위: popup → preset → surface → tab → pane → workspace.
- 신규 코드는 Intent 발화만 허용 (코드 리뷰 규칙).
- clippy custom lint로 잡을 수 있는 직접 호출 패턴(`state.popups.open*` 등)은 lint 작성 (TODO 05).

### 도메인 진행 상황

| 도메인 | 마이그레이션 | 비고 |
|--------|------|------|
| popup | 완료 | `OpenPopup` / `ClosePopup` / `TogglePopup` |
| preset | 완료 | `Apply` / `Save` / `Delete` / `Rename`. IPC inner-function 공유 패턴 |
| surface | 완료 | `SplitSurface` / `CloseSurface` / `ConvertSurface`. IPC EXEMPT |
| tab | 완료 | `NewTab { kind: Option }` / `CloseTab`. IPC EXEMPT |
| pane | 완료 | `SplitPane` (사용자 단축키 전용). ratio/focus API 는 S3=B 미마이그레이션 |
| workspace | 부분 | `NewWorkspace`. `CloseWorkspace` 는 cascade 의존, `RenameWorkspace`/`MoveWorkspace` 는 IPC handler 합성형이라 별도 결정 필요 |
| window | SKIP | WIN1=B per design |
| settings | SKIP | SET1=B per design |

## 예시

### User 단축키 → popup open

```rust
// src/shortcuts.rs
if matches_any_binding(&kb.toggle_command_palette, key, mods) {
    state.dispatch_intent(
        Intent::TogglePopup {
            id: COMMAND_PALETTE_POPUP_ID,
            mode: OpenPopupMode::CenteredFocused,
        }
        .from_user_shortcut("toggle_command_palette"),
    );
}
```

핸들러(`intent::popup::handle`)는 origin 분기 없이 `mode` 그대로 실행 — `state.popups.open_centered_focused(id)` 호출.

### Agent IPC → popup open (debug 빌드만)

release 빌드에는 agent 가 popup 을 발화하는 경로가 *전혀 존재하지 않는다*.
debug 빌드에서 *사용자 입력 재현* 용도의 IPC 가 한정적으로 popup 을 발화할 수
있을 뿐.

```rust
// src/adapters/ipc/handler/debug/popup.rs (모듈 전체 #![cfg(debug_assertions)])
state.dispatch_intent(
    Intent::Ui(UiIntent::OpenPopup {
        id: params.popup_id,
        mode: OpenPopupMode::Default,  // focus 없는 변형 — focus 독립성 원칙
    })
    .from_agent_ipc(),
);
```

release 빌드에서는 `Intent::Ui` variant 자체가 GUI feature 에 종속되며,
Domain handler 는 `UiIntent` 를 모르므로 *type level* 에서 agent 발화가 차단된다.
debug 빌드의 위 경로는 *유일한 예외* — 사용자 입력 시뮬레이션 용도로만 사용.

## 관련 문서

- [popup-system.md](popup-system.md) — popup 추가 방법, 공통 규칙
- [focus-policy.md](focus-policy.md) — focus 독립성 원칙
- [../agent-guide/event-catalog.md](../agent-guide/event-catalog.md) — Event Bus 1.0 envelope 명세
- [../dev-guide/popup-implementation.md](../dev-guide/popup-implementation.md) — popup 구현 가이드
