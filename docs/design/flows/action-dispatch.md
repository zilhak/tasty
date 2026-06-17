# Action Dispatch (Intent 큐)

호스트 내부에서 모듈 경계를 넘는 동작(popup open, preset apply, surface split 등)을 **Intent 큐**로 통일 발화·처리하는 모델. plugin↔host 경계의 read 채널인 [Event Bus](../../reference/event-catalog.md)와는 **다른 layer**다. 코드: `src/intent.rs` + `src/intent/<domain>.rs`.

## 왜 큐인가

같은 동작(예: command palette popup 열기)을 단축키·도구 메뉴·IPC·plugin·우클릭 등 여러 진입점에서 발화한다. 직접 호출하면 `mem::take` 로 비워진 매니저에 호출돼 **noop 버그**가 나거나, draw 콜백 안에서 자기 매니저를 mutate 못 한다. **모든 진입점이 Intent 를 큐에 던지고, 메인 루프 단계에서 일관 처리**하면 발화 위치와 무관해진다.

## 용어

| 용어 | 정의 |
|------|------|
| **Intent** | 호스트 내부 명령 — "다음 메인 루프 단계에 무엇을 할지". 큐 통과. Event Bus 키와 다른 네임스페이스 |
| **Event** | [Event Bus](../../reference/event-catalog.md)의 사건(이미 일어난 일의 알림). plugin 구독. Intent 와 다른 layer |
| **Origin** | Intent 발화 주체 — `User { source }` / `Agent { source }`. 핸들러가 정책 분기에 사용 |
| **Cascade** | Intent A 핸들러가 발화하는 파생 Intent B. 별 origin 종류 없이 A 의 origin 전파 |
| **Bridge** | Intent 처리 결과 → Event Bus 변환 단일 지점(`meta.origin`/`trace_id` 채움) |

## 핵심 원칙 — Intent 의 위치

> **Intent 는 "의도" 다 — 흐름의 *시작점* 에만 존재한다. 끝에서 결과로 나오거나 중간에서 정보 전달용으로 쓰이면 안 된다.**

흐름은 한 방향: `이벤트 → 해석된 의도(Intent→큐) → 처리(drain) → 결과(state mutate / cascade)`.

- **이벤트→Intent 변환 함수 리턴은 OK**(시작점): `fn parse(e) -> Option<Intent>`.
- **처리 핸들러가 Intent 를 *리턴 경로로* 흘려보내면 금지** — cascade 는 핸들러 *본문 안에서* `state.dispatch_intent(...)` 로 큐에 enqueue. 반환 경로로 흘리면 호출 트리가 재귀가 되어 큐 모델이 깨진다.
- **Intent 는 응답 데이터를 갖지 않는다**(`Result<Intent>`, 응답 필드 금지). 응답이 필요한 mutate(새 ID 발급, 분기용 status)는 Intent 가 아니라 **Core method**(sync 리턴) 또는 **Query(read)** 로.

| 유형 | 메커니즘 | 응답 |
|------|----------|------|
| Query | `&CoreState` 직접 read | 데이터 |
| **Intent** (fire-and-forget) | enqueue → cascade | 없음 |
| Core method | `core.create_workspace(...) -> WorkspaceCreated` | sync 리턴 |

### 사용자 입력 대기 = 반드시 2 Intent 분리

"확인 popup → 응답 → 후속" 같은 *사용자 입력 대기* 는 **한 Intent 안에서 wait 하지 않는다.** 1차 Intent(popup 띄움 + 컨텍스트를 state 에 저장 + 종료) / 2차 Intent(응답 시 state 에서 읽어 진행 또는 폐기)로 분리한다.

```rust
Intent::PresetApplyRequest{kind,name} → handler: state.dialogs.pending_preset_apply = Some(ctx);
                                                  state.dispatch_intent(OpenPopup{"confirm_preset_apply"});
Intent::PresetApplyConfirmed → handler: let Some(ctx)=…take() else {return}; apply(ctx);
Intent::PresetApplyCancelled → handler: …pending_preset_apply = None;
```

이유: 사용자 응답은 *무한정* — 한 Intent 가 메모리에 lock 되면 그동안의 state 변경(window close, settings change)에 취약하고 큐가 정체된다. 명시적 분리는 *진행 중 컨텍스트* 가 state 에 보여 디버깅도 쉽다. (시스템 내부의 유한-시간 multi-step cascade 는 핸들러 본문 cascade enqueue 로 충분.)

## Intent 자료형

`Intent` 는 **flat enum**(nested 안 함). variant 는 `src/intent.rs`, 도메인 핸들러는 `src/intent/<domain>.rs`(popup/preset/surface/tab/pane/workspace). 발화 시 envelope 으로 감싼다:

```rust
pub struct DispatchedIntent { pub body: Intent, pub origin: IntentOrigin, pub trace_id: Option<String> }
pub enum IntentOrigin { User { source: UserSource }, Agent { source: AgentSource } }
//   UserSource: Shortcut(&str) / Menu(&str) / ContextMenu
//   AgentSource: Ipc / Plugin(String) / Cli
```

**Cascade origin**: popup A→B cascade 에서 B 의 origin 은 A 의 origin 을 그대로 명시 전달(`별 Cascade variant 없음`). "왜 이 popup 이 열렸나" 를 origin 이 정확히 가리켜 audit 에서 시작점 추적 가능.

## 발화 / 처리

- **발화**: `state.dispatch_intent(dispatched)` = `Vec::push` 한 줄(fire-and-forget). ergonomics 빌더 `.from_user_shortcut("…")` / `.from_user_menu("…")` / `.cascaded_from(intent)`.
- **처리**: 메인 루프의 `App::dispatch_pending_intents` 가 모든 window/parked_state 를 순회 drain. 발화 순서대로 처리, drain 중 새로 발화한 Intent 는 **다음 프레임**(재진입 방지, `mem::take` 후 별 Vec 순회).
- **핸들러 분기**: trait dispatch 아니라 **도메인 함수 분기**(`match &intent.body`). `&mut AppState` 를 trait object 가 통째 잡으면 partial mutation 이 borrow checker 를 못 통과하기 때문.
- **에러**: 핸들러 안에서 `tracing::warn!`(패닉 금지, `let _=` 금지). 사용자에게 보여야 하면 `state.toasts.push`.

## Intent → Event Bus Bridge

핸들러가 mutation 성공 후 `state.pending_host_events` 에 host event 를 push → 단일 bridge(`src/intent.rs`)가 [Event Bus](../../reference/event-catalog.md) envelope 으로 변환:

| envelope | 규칙 |
|----------|------|
| `meta.origin` | `User`→`{kind:host}`, `Agent{Plugin(id)}`→`{kind:plugin,plugin_id}`, 그 외 Agent→`{kind:host}` |
| `meta.trace_id` | `intent.trace_id` 있으면 그대로, 없으면 새 발급 |
| `meta.scope` | 핸들러가 mutation 결과 보고 채움 · `meta.hop` | 호스트 발화이므로 `0` |

## User vs Agent 정책

dispatcher 가 강제 거부하지 않고 **핸들러 작성자가 `origin.is_user()` 로 분기**(PR 리뷰 강제):

| 동작 | User | Agent |
|------|------|-------|
| popup open(UI Intent 발화) | 가능 | **금지**(debug 사용자 입력 재현 IPC 제외) |
| closed-tab restore push · OS 윈도우 focus · Workspace activate · Window 생성 | 가능 | **금지**(focus 독립) |

> **UI Intent vs Domain Intent**(제1 분류축): UI(popup open/close/toggle) 또는 Domain(나머지). release 의 Domain 처리 흐름은 UI Intent 를 발화할 수 없고 **타입 차원에서 강제**된다(Core 가 `UiIntent` 를 모름). 자동 popup/toast/dialog 는 release 에 없다. 자매 정책: [popup](../systems/popup.md) · [toast](../systems/toast.md) 의 "발화 정책 (CRITICAL)".

## intent-discipline 강제

popup 도메인 직접 호출(`state.popups.open*`/`.close`/`.toggle*`)은 `scripts/check-intent-discipline.sh`(grep 기반 CI)로 금지. 예외는 동일 라인 `// intent-exempt: <사유>` 주석으로 suppress(query API `is_open`/`get_mut`/`register` 는 제외). draw-prep i18n refresh, popup 자기-close cleanup, Settings 윈도우 init 등록이 현재 예외. 호스트 모든 Intent 가시화는 debug 전용 `intent::watch`(`src/intent/watch.rs`)가 `tracing::debug!` 로(release 제거).

## dedup

같은 popup id 의 OpenPopup 이 동일 사이클에 중복 들어오면 핸들러가 `state.popups.is_open(id)` 로 두 번째를 무시(큐 push 시점엔 dedup 안 함 — origin/trace_id 다른 둘을 같다 볼 근거 없음).

## 관련

- [popup](../systems/popup.md) · [toast](../systems/toast.md) — 발화 정책 · [focus](../policies/focus.md) — 독립성
- [reference/event-catalog](../../reference/event-catalog.md) — Event Bus envelope · [dev-guide/popup-implementation](../../dev-guide/popup-implementation.md)
