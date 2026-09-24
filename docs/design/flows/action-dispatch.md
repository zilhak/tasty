# Action Dispatch (Intent 큐)

호스트의 모듈 경계를 넘는 동작은 Intent 큐에 등록해 처리한다. 팝업 열기, 프리셋 적용, surface 분할 등이 여기에 해당한다. 이미 일어난 일을 plugin에 알리는 [Event Bus](../../reference/event-catalog.md)와는 역할이 다르다. 구현은 `src/intent.rs`와 `src/intent/<domain>.rs`에 있다.

## 왜 큐인가

명령 팔레트 열기 같은 동작은 단축키·메뉴·IPC·plugin 등 여러 곳에서 요청한다. 그리기 콜백에서 매니저를 직접 변경하려 하면 대여 충돌이 나거나, `mem::take`로 잠시 비워 둔 매니저에 요청해 아무 일도 일어나지 않을 수 있다. 요청을 큐에 넣고 메인 루프에서 처리하면 요청 위치와 처리 시점을 분리할 수 있다.

## 용어

| 용어 | 정의 |
|------|------|
| **Intent** | 다음 메인 루프 단계에서 처리할 호스트 내부 명령. Event Bus 키와 별도 이름 공간을 사용 |
| **Event** | 이미 일어난 일을 [Event Bus](../../reference/event-catalog.md)로 알리는 이벤트. plugin이 구독 |
| **Origin** | 요청 출처인 `User { source }` 또는 `Agent { source }`. 핸들러가 사용자·에이전트 정책을 구분할 때 사용 |
| **Cascade** | Intent 처리 중 추가로 등록하는 Intent. 원래 요청의 origin을 유지 |
| **Bridge** | 처리 결과를 Event Bus 메시지로 바꾸는 공통 경로. `meta.origin`과 `trace_id`를 생성 |

## 핵심 원칙 — Intent 의 위치

Intent는 동작을 요청할 때 사용한다. 처리 결과나 중간 데이터를 전달하는 용도로 반환하지 않는다.

흐름은 `이벤트 → Intent 등록 → 큐 처리 → 상태 변경 또는 추가 Intent 등록` 순서다.

- 이벤트를 해석하는 함수는 `fn parse(e) -> Option<Intent>`처럼 Intent를 반환할 수 있다.
- 처리 핸들러가 추가 작업을 요청할 때는 본문에서 `state.dispatch_intent(...)`를 호출한다. 반환값으로 Intent를 전달해 호출자가 재귀 처리하게 만들지 않는다.
- Intent에는 응답 데이터를 넣지 않는다. 새 ID나 처리 상태를 받아야 하면 결과를 반환하는 Core 메서드를 사용하고, 상태 조회에는 Query를 사용한다.

| 유형 | 처리 방식 | 응답 |
|------|----------|------|
| Query | `&CoreState` 직접 조회 | 데이터 |
| Intent | 큐에 등록한 뒤 처리 | 없음 |
| Core method | `core.create_workspace(...) -> WorkspaceCreated` | 동기 반환값 |

### 사용자 입력 대기 = 반드시 2 Intent 분리

확인 팝업의 사용자 응답을 한 Intent 안에서 기다리지 않는다. 첫 Intent는 팝업을 열고 대기 상태를 저장한 뒤 끝낸다. 응답이 오면 두 번째 Intent가 저장된 상태를 읽어 실행하거나 취소한다.

```rust
Intent::PresetApplyRequest{kind,name} → handler: state.dialogs.pending_preset_apply = Some(ctx);
                                                  state.dispatch_intent(OpenPopup{"confirm_preset_apply"});
Intent::PresetApplyConfirmed → handler: let Some(ctx)=…take() else {return}; apply(ctx);
Intent::PresetApplyCancelled → handler: …pending_preset_apply = None;
```

사용자는 언제 응답할지 정해져 있지 않다. 그동안 큐를 막지 않아야 하며, 창 닫기나 설정 변경도 처리해야 한다. 대기 상태를 별도로 저장하면 진행 중인 작업도 확인할 수 있다. 사용자 대기 없이 끝나는 내부 후속 작업은 핸들러에서 추가 Intent를 등록하면 된다.

## Intent 자료형

`Intent`는 `Ui(UiIntent)`, `Domain(DomainIntent)`와 프리셋·탭 등 개별 작업 변종을 가진다. 정의는 `src/intent.rs`, 도메인 핸들러는 `src/intent/<domain>.rs`에 있다. 큐에 넣을 때 요청 출처를 함께 전달한다.

```rust
pub struct DispatchedIntent { pub body: Intent, pub origin: IntentOrigin, pub trace_id: Option<String> }
pub enum IntentOrigin { User { source: UserSource }, Agent { source: AgentSource } }
//   UserSource: Shortcut(&str) / Menu(&str) / ContextMenu
//   AgentSource: Ipc / Plugin(String) / Cli
```

팝업 A의 처리에서 B를 열면 B에도 A의 origin을 전달한다. 별도의 Cascade 출처를 만들지 않는다. `DispatchedIntent.trace_id`는 현재 생성자에서 `None`이며 Event Bus의 `trace_id`와는 별개다.

<a id="발화--처리"></a>

## 요청 등록과 처리

`state.dispatch_intent(dispatched)`는 요청을 큐에 넣는다. 요청 출처를 붙이는 빌더로 `.from_user_shortcut("…")`, `.from_user_menu("…")`, `.cascaded_from(intent)`를 사용한다.

IPC 엔진 핸들러는 창 대신 요청별 `IntentOutbox`를 받는다. 핸들러는 `out.push(dispatched)`로 요청을 넣고, 진입점의 게이트·`dispatch_routed`·`record_plugin_rss_samples`가 요청 종료 시 창 큐로 옮긴다. 한 요청 안에서는 추가한 순서를 유지하므로 게이트가 만든 Intent가 먼저 처리된다. [ADR-0002](../../adr/0002-domain-execution-and-ports.md)를 따른다.

GUI의 `App::dispatch_pending_intents`는 창과 parked state의 큐를 처리한다. 전체 등록 순서를 그대로 따르지는 않으며 `classify_intent`가 정한 종류별로 처리한다.

| 종류 | 처리 순서 |
|---|---|
| `Immediate` | 같은 state에서 FIFO로 즉시 처리 |
| `Intent::Domain` | 단계 C인 `run_domain_cascade`에서 FIFO로 처리 |
| `AppearanceChanged` | 프레임 끝에 한 번만 처리 |

Domain 처리는 App 전체를 대여하므로 state별 처리 뒤로 분리한다. 큐를 `mem::take`로 꺼낸 뒤 순회하며, 처리 중 새로 등록한 Intent는 재진입을 피하기 위해 다음 프레임에 처리한다.

헤드리스는 `crate::intent::headless::drain_pending_intents`로 engine 하나의 큐를 다음 시점에 처리한다.

- IPC 요청 처리 직후, 응답을 보내기 전
- plugin 호출 결과를 회신하기 전
- 메인 루프가 대기하기 직전

attention·notification 적재, terminal mark, 설정 적용·저장처럼 engine 상태만으로 끝나는 작업을 처리한다. 화면 다시 그리기·토스트·테마 재설치·알림음은 제외한다. OSC 7 cwd의 탭 이름 갱신은 Intent가 아니라 PTY 처리에서 `intent::headless::apply_terminal_cwd_changed`를 직접 호출한다.

헤드리스의 `drain_pending_host_events`도 위 세 시점과 PTY 출력 처리 끝에서 실행한다. `pending_host_events`를 비우고 `HookFired`로 push 완료를 기다리는 agent task를 마감한다. 일반 plugin Event Bus 전달 경로는 없어 나머지 이벤트는 버린다. 지원 범위와 이유는 [ADR-0003](../../adr/0003-headless-behavior.md)을 따른다.

핸들러는 `match &intent.body`로 도메인 함수를 선택한다. AppState 전체를 trait object가 대여하면 필요한 필드만 따로 변경하기 어렵기 때문이다. 오류는 `tracing::warn!`으로 기록하고, 사용자에게 알려야 할 실패는 사용자·에이전트 정책에 따라 토스트로 표시한다. 패닉을 일으키거나 `let _ =`로 오류를 버리지 않는다.

## Intent → Event Bus Bridge

변경을 마친 핸들러가 `state.pending_host_events`에 이벤트를 넣는다. `src/app/dispatch/host_events.rs`의 공통 경로가 `PluginManager::emit_host_event`를 호출해 [Event Bus](../../reference/event-catalog.md) 메시지로 바꾼다.

| 필드 | 규칙 |
|------|------|
| `meta.origin` | 항상 `{kind:host}`(`EventOrigin::Host`). Intent의 origin을 복사하지 않음 |
| `meta.trace_id` | 이벤트마다 새로 발급하는 `h<seq 16진>`. Intent 값을 이어받지 않음 |
| `meta.scope` | 핸들러가 변경 결과를 기준으로 지정 |
| `meta.hop` | 호스트가 만든 이벤트이므로 `0` |

## User vs Agent 정책

핸들러는 `origin.is_user()`로 요청 출처를 구분한다. dispatcher가 모든 정책 위반을 자동 거절하지는 않으므로 핸들러를 검토할 때 확인한다.

| 동작 | User | Agent |
|------|------|-------|
| UI Intent로 팝업 열기 | 허용 | 금지. debug 사용자 입력 재현 IPC는 예외 |
| 닫은 항목 복원 기록 추가·OS 창 포커스·workspace 활성화 | 허용 | 금지 |

에이전트의 새 창 생성은 별도 `window.create` 경로로 허용하며 기존 사용자 포커스를 유지한다. [포커스 정책](../policies/focus.md)을 따른다.

Core는 `UiIntent`를 모르므로 release의 Domain 처리에서 UI Intent를 만들 수 없다. 팝업·토스트·대화상자의 표시 조건은 [팝업](../systems/popup.md)과 [토스트](../systems/toast.md)를 따른다. 원격 연결 상태 토스트처럼 허용된 예외를 일반 에이전트 요청의 성공·실패 알림으로 확대하지 않는다.

## intent-discipline 강제

`scripts/check-intent-discipline.sh`는 popup·preset·surface·tab·pane·workspace의 직접 변경 API 호출을 검사한다. `script-gates.yml`의 main push와 PR에서 실행하도록 설정돼 있다.

검사 대상은 `open`·`open_centered`·`open_centered_focused`·`open_with_scope`·`open_at_top_of_scope`·`open_at_focused`·`close`·`toggle*`다. 조회 함수인 `is_open`·`get_mut`·`open_geometry`는 포함하지 않는다.

다음 내용은 제외한다.

- 실제 호출이 아닌 주석·문자열 리터럴
- 도메인 자체를 시험하는 `#[cfg(test)] mod` 본문과 `*_tests.rs`
- 이름만 같은 다른 타입의 메서드. 패턴별 예외 경로로 관리한다.

허용할 호출은 같은 줄이나 바로 위·아래에 `// intent-exempt: <사유>`를 적는다. 현재 예외에는 팝업의 `on_close` 정리, 핸들러의 후속 요청, 응답이 필요한 동기 Core 호출, 포커스 보존을 위해 큐를 우회한 호출이 있다.

`intent::watch`(`src/intent/watch.rs`)는 GUI debug 빌드의 큐 처리에서 `tracing::debug!` 로그를 남긴다. release와 헤드리스 큐 처리에는 이 로그가 없다.

## dedup

같은 처리 주기에 같은 popup ID의 OpenPopup이 중복되면 핸들러가 `state.popups.is_open(id)`로 두 번째 요청을 무시한다. 큐에 넣는 시점에는 origin과 trace_id가 다른 요청을 같은 것으로 판단하지 않는다.

## 관련

- [popup](../systems/popup.md) · [toast](../systems/toast.md) — 표시 조건
- [focus](../policies/focus.md) — 사용자 포커스 보호
- [reference/event-catalog](../../reference/event-catalog.md) — Event Bus 메시지
- [dev-guide/popup-implementation](../../dev-guide/popup-implementation.md) — 팝업 구현
