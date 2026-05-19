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
| popup focus 셋팅 | 가능 (`open_centered_focused`) | 금지 (`open_centered`로 대체) |
| closed-tab restore 스택 push | 가능 | 금지 |
| OS 윈도우 focus API 호출 | 가능 | 금지 (focus 독립성 원칙) |
| Workspace activate | 가능 | 금지 (focus 독립성 원칙) |
| Window 생성 (PresetWindow 등) | 가능 | 금지 |

분기 위치는 핸들러 본문의 `origin.is_user()`. enum variant로 분기하지 않는다.

> `CLAUDE.md`의 "사용자 입력 재현은 debug 한정" 원칙은 IPC/CLI 표면에 적용된다. 호스트 popup 자체에는 release 빌드의 IPC가 없다. debug 빌드에서 사용자 입력 재현용 IPC를 추가할 경우 `#[cfg(debug_assertions)]`로 격리한다.

## 기존 패턴과의 관계

### `AppEvent` (winit user event proxy)

cross-thread winit user event 용도 (`src/main.rs:120-162`). Intent와 layer가 다르므로 합치지 않는다. winit event_loop가 `AppEvent`를 받으면 필요 시 Intent로 변환해 dispatch.

### `pending_host_events`

기존 host event 큐. Intent 핸들러가 mutation 후 결과 이벤트를 여기에 push한다. **Intent → Event Bus 변환의 단일 bridge**.

### `pending_popup_opens` (plugin용)

plugin manifest의 `[[contributes.popup]]` instance open 큐. 그대로 유지. 호스트 popup용 Intent는 별도 enum variant (`Intent::OpenPopup`).

### `dialogs.pending_popup_open` (popup deferred)

Intent 큐의 도메인 한정 부분 구현. TODO 04에서 흡수.

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

## 마이그레이션 정책

- 도메인별 점진 적용. 우선순위: popup → preset → surface → tab → pane → workspace.
- 신규 코드는 Intent 발화만 허용 (코드 리뷰 규칙).
- clippy custom lint로 잡을 수 있는 직접 호출 패턴(`state.popups.open*` 등)은 lint 작성 (TODO 05).

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

```rust
// src/ipc/handler/debug_popup.rs (cfg debug_assertions)
// agent origin 은 focus 를 가져가지 않는 것이 정책 — Default 또는 focus 없는 변형 사용.
state.dispatch_intent(
    Intent::OpenPopup {
        id: params.popup_id,
        mode: OpenPopupMode::Default,
    }
    .from_agent_ipc(),
);
```

dispatcher 는 강제 분기를 하지 않으므로, 호출자가 정책에 맞는 `OpenPopupMode` 를 선택할 책임이 있다 (PR 리뷰에서 강제).

## 관련 문서

- [popup-system.md](popup-system.md) — popup 추가 방법, 공통 규칙
- [focus-policy.md](focus-policy.md) — focus 독립성 원칙
- [../agent-guide/event-catalog.md](../agent-guide/event-catalog.md) — Event Bus 1.0 envelope 명세
- [../dev-guide/popup-implementation.md](../dev-guide/popup-implementation.md) — popup 구현 가이드
