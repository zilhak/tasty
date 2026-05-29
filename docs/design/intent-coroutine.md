# Intent Coroutine Runtime

> **상태: 설계 단계.** PoC 구현 예정. 본 문서는 *경량 thread + coroutine* 기반의
> Intent workflow runtime 구조를 정의한다. 일반적인 Intent 큐 모델 ([action-dispatch.md](./action-dispatch.md))
> 의 보강 — 단순 cascade 가 아닌 *yield/resume 워크플로우* 가 필요한 도메인용.

## 개요

Tasty 의 일반 Intent 는 *fire-and-forget* 모델로 큐를 통과한다 ([action-dispatch.md](./action-dispatch.md)).
대부분의 경우 이걸로 충분하지만, 다음 같은 흐름은 *명시적 yield* 가 더 자연:

- 사용자 확인 popup 발화 → 응답 대기 → 응답 후 후속 작업
- 다른 Intent 의 *완료를 기다린 후* 자기 로직 계속
- 다단계 workflow 의 *중간 점에서 다른 Intent 가 끼어들고* 완료된 후 resume

이런 흐름은 *명시적 state machine* 으로도 구현 가능하지만 *코드 가독성* 이
떨어진다 (step1/step2/step3 분리 + state 안 어디까지 진행됐는지 flag).
coroutine yield/resume 으로 *직선 코드* 처럼 표현할 수 있으면 유지보수가 쉽다.

## 핵심 원칙

1. **별도 thread 에 coroutine runtime 을 둔다.** main thread 의 winit event
   loop 와 격리. coroutine 의 yield/resume 은 *intent thread 안에서만* 일어난다.
2. **main thread 는 *state owner + mutate executor*** — winit + state mutate
   단일 진입점. coroutine 은 *결정만* 하고 mutate 명령은 main 에 mpsc 로 보낸다.
3. **yield 의미는 *"main 의 적용 완료를 기다림"*** — async IO 가 아니라
   *step-wise progression*. ack signal 로 resume.
4. **Intent 의 본질 보존** — coroutine 안에서 새 Intent 발화는 *큐에 enqueue*
   만 OK (Result/return 으로 흘러나오는 패턴 금지, [action-dispatch.md](./action-dispatch.md) 참조).
5. **coroutine 의 yield 도 본질적으로는 *Intent 발화* 다** — yield 가 *큐에 던지는*
   행위와 동등하지만, *다음 처리 후 자기 위치로 돌아옴* 이 추가된 형태.

## 구조

### 3-Layer

```
                                                                              
   ┌─────────────────────────┐                                              
   │   main thread           │                                              
   │   - winit event loop    │                                              
   │   - state owner         │   coroutine_intent_rx                        
   │   - mutate executor     │ ◄──────────────────  ┌────────────────────┐  
   │   - drain (Intent 큐)   │                       │  intent thread      │ 
   │                         │   mutate_cmd_rx       │  - coroutine runtime│ 
   │                         │ ◄──────────────────   │    (genawaiter)     │ 
   │                         │                       │  - workflow A/B/C   │ 
   │                         │   ack_tx              │    coroutines         
   │                         │ ──────────────────►  │  - mpsc multiplex   │ 
   └─────────────────────────┘                       └────────────────────┘  
            ▲                                                                 
            │ mpsc channels                                                   
            │ (blocking IO results)                                           
            │                                                                 
   ┌────────┴────────────────┐                                              
   │   worker threads        │                                              
   │   - approval.await      │                                              
   │   - observer Memory sink│                                              
   │   - IPC server          │                                              
   └─────────────────────────┘                                              
```

### 채널 구조

main ↔ intent thread 간 3 개 mpsc 채널:

| 채널 | 방향 | 페이로드 | 용도 |
|------|------|----------|------|
| `intent_tx` | main → intent | `CoroutineIntent` | main 이 coroutine workflow 시작 명령 |
| `mutate_cmd_rx` | intent → main | `MutateCommand` | coroutine 이 main 에 state mutate 요청 |
| `ack_tx` | main → intent | `AckSignal { workflow_id }` | main 이 mutate 적용 완료 알림 → coroutine resume |

추가로 양쪽 모두 `CoreIntent` 큐와 연결됨 (cascade 발화용).

## 흐름 예시 — preset apply with confirmation

워크플로우: 사용자가 preset 적용 → confirmation popup → "확인" 클릭 → preset 적용.

```
시간      main thread                  intent thread (coroutine A)
                                                                
t0        user click → 큐에 Apply       
t1        drain: Apply intent 발견                              
            ├─ workflow 식별: A         
            │  (multi-step)             
            ├─ intent_tx.send(A)  ──►   recv: A 시작            
                                          │                     
t2                                        ├─ step1: validate    
                                          │  preset 존재 확인   
                                          │  (state read만)    
                                          │                     
t3                                        ├─ yield_!(           
                                          │    OpenPopup        
                                          │      "confirm_      
                                          │       preset_apply" 
                                          │  )                  
                                          │                     
                                          ├─ mutate_cmd_rx.send(
            mutate_cmd_rx.recv() ◄────────┤    OpenPopup + 큐    
                                          │    enqueue          
            ├─ Intent::OpenPopup           │   workflow_id=A     
            │  큐에 enqueue (cascade)      │  )                  
            ├─ drain 다음 라운드           │                     
                                          │  (yield 중. resume  
t4        popup draw_fn 처리             │   대기.)            
          사용자 "확인" 클릭             │                     
                                          │                     
t5        drain: ApplyConfirmed intent   │                     
            ├─ workflow_id=A 확인         │                     
            │  ack_tx.send(A, true) ──►   resume               
                                          │                     
t6                                        ├─ step2: apply      
                                          │   preset           
                                          │                     
                                          ├─ mutate_cmd_rx.send(
            mutate_cmd_rx.recv() ◄────────┤    ApplyPreset      
                                          │  )                  
            ├─ engine.apply_preset()       │                     
            ├─ ack_tx.send(A, done) ──►   coroutine return    
                                          │                     
t7        workflow 종료                                          
```

### yield 의 의미 (timeline)

```
coroutine A 의 time line:

  ─── step1 ───┬─── (yield) ───┬─── step2 ───┬─── (yield) ───┬── done
              │                 │              │
              ▼                 ▲              ▼
          OpenPopup           ack            ApplyPreset
          mutate_cmd        (popup           mutate_cmd
                            응답 후
                            main 이 send)

main thread 의 timeline:

  ─── drain ─┬─ recv mutate ─┬─── popup ─┬─ drain ─┬─ ack send ─┬─ recv mutate ─┬─ apply ─┬─ ack send
             │                │            │         │            │              │         │
             ▼                ▼            ▼         ▼            ▼              ▼         ▼
        OpenPopup          popup        user      ApplyConfirmed              engine    workflow
        enqueue            display      click     intent dispatch             mutate    cleanup
```

## genawaiter 사용 방식

### 얇은 wrapper

```rust
// src/intent_coroutine/runtime.rs (예정)

use genawaiter::sync::{Co, Gen};

/// coroutine 의 yield value — main 에 보낼 mutate 명령.
pub enum MutateCommand {
    EnqueueIntent(crate::intent::DispatchedIntent),
    Apply(crate::core::intent::CoreIntent),
    // ... 도메인별 명령
}

/// 도메인 workflow trait. coroutine 본문은 async fn 으로 작성.
#[async_trait::async_trait(?Send)]
pub trait Workflow {
    /// coroutine entry point.
    async fn run(self, co: Co<MutateCommand>);
}

/// runtime — intent thread 의 main loop.
pub fn run_intent_thread(
    intent_rx: Receiver<Box<dyn Workflow>>,
    mutate_tx: Sender<MutateCommand>,
    ack_rx: Receiver<AckSignal>,
) {
    let mut active: HashMap<WorkflowId, Gen<MutateCommand, AckSignal, _>> = HashMap::new();

    loop {
        // 새 workflow 시작
        if let Ok(wf) = intent_rx.try_recv() {
            let gen = Gen::new(|co| wf.run(co));
            // ...
        }

        // 기존 workflow 의 ack 처리
        if let Ok(ack) = ack_rx.try_recv() {
            if let Some(gen) = active.get_mut(&ack.workflow_id) {
                match gen.resume_with(ack) {
                    GeneratorState::Yielded(cmd) => mutate_tx.send(cmd).unwrap(),
                    GeneratorState::Complete(_) => { active.remove(...); }
                }
            }
        }
    }
}
```

### workflow 작성 예

```rust
// src/intent_coroutine/workflows/preset_apply.rs

pub struct PresetApplyWorkflow {
    pub kind: PresetKind,
    pub name: String,
}

#[async_trait::async_trait(?Send)]
impl Workflow for PresetApplyWorkflow {
    async fn run(self, co: Co<MutateCommand>) {
        // step1: popup 표시
        co.yield_(MutateCommand::EnqueueIntent(
            Intent::OpenPopup {
                id: "confirm_preset_apply",
                mode: OpenPopupMode::CenteredFocused,
            }.from_user_menu("preset_apply_workflow"),
        )).await;
        // ← 여기까지 main 이 popup 까지 처리. resume 시 ack 수신.

        // step2: confirm 응답 후 실제 apply
        co.yield_(MutateCommand::Apply(
            CoreIntent::ApplyPresetConfirmed { kind: self.kind, name: self.name }
        )).await;
        // ← main 이 apply 완료. resume.

        // workflow 종료.
    }
}
```

`Co::yield_().await` 가 *intent thread* 안에서만 일어남 — main thread 의
winit event loop 는 영향 없음.

## main thread 통합

### Intent 큐 drain 시점

기존 `App::dispatch_pending_intents` 는 *flat Intent* 만 처리. workflow Intent
는 다른 분기:

```rust
fn dispatch_one(&mut self, intent: DispatchedIntent) {
    match &intent.body {
        Intent::OpenPopup { .. } => { ... }
        Intent::ApplyPreset { kind, name } => {
            // workflow 로 분기
            self.intent_thread_tx.send(Box::new(PresetApplyWorkflow {
                kind: *kind, name: name.clone(),
            }));
        }
        Intent::Noop => {}
    }
}
```

### mutate command 처리 시점

`event_handler.rs` 의 main loop 한 단계에서 `try_recv` 로 polling:

```rust
fn poll_intent_thread_commands(&mut self) {
    while let Ok(cmd) = self.intent_mutate_rx.try_recv() {
        match cmd {
            MutateCommand::EnqueueIntent(intent) => {
                self.dispatch_intent(intent);
            }
            MutateCommand::Apply(core_intent) => {
                self.core.apply(core_intent).ok();
            }
        }
        // workflow ack 전송
        self.intent_ack_tx.send(...).ok();
    }
}
```

### 호출 위치

`dispatch_pending_intents` 다음, `dispatch_pending_popup_opens` 앞. (action-dispatch.md
와 동일 위치, intent_coroutine 의 mutate 도 popup 발화 가능.)

## 위험 / mitigation

### Risk 1: genawaiter 의 long-term 유지

- 1.0 미달, 활성도 적당. 폐기되거나 breaking change 가능.
- **mitigation**: 본 runtime 을 *얇은 wrapper* 안에 가둠. `Workflow` trait 와
  `Co<MutateCommand>` 정도만 외부 노출. genawaiter 교체 시 wrapper 만 갱신.

### Risk 2: deadlock — main 이 ack 못 보내고, coroutine 이 yield 중 무한 대기

- popup 이 사용자 응답 없이 영원히 열려 있으면 coroutine 영구 대기.
- **mitigation**: workflow timeout (예: 60 초 후 cancel signal). Cancel 시
  coroutine 안에서 적절한 cleanup 후 종료.

### Risk 3: state 일관성 — coroutine 이 *오래된 state* 가정으로 step2 실행

- popup 처리 동안 다른 변경이 일어났다면 step2 의 가정이 깨질 수 있음.
- **mitigation**: workflow 내부에서 *각 step 시작 시 state 재검증*. invalidation
  시 cleanup 후 종료. (coroutine 안에서 read query 는 mutate_cmd 의
  request-response 패턴 — `Co::yield_` 에 `Query` variant 추가.)

### Risk 4: panic 전파

- coroutine panic 이 intent thread 를 죽이면 모든 workflow 가 stop.
- **mitigation**: 각 workflow 를 `catch_unwind` 로 격리. panic 시 workflow 만 종료.

### Risk 5: 디버깅 어려움 — yield point 가 main 의 시점 추적과 분리

- timeline 로깅 필요. 각 yield + ack 시점에 `tracing::debug!` 로 workflow_id +
  step 마킹.

## API 예시

```rust
// 사용자 코드 (handler)
fn handle_preset_apply_request(state: &mut AppState, params: ...) {
    state.dispatch_workflow(PresetApplyWorkflow {
        kind: params.kind,
        name: params.name,
    });
    // 이 함수는 즉시 return. workflow 는 intent thread 에서 비동기 진행.
}
```

## PoC 단계 (예정)

1. **단일 workflow PoC** — `PresetApplyWorkflow` 만 구현. main ↔ intent thread
   mpsc + ack. 동작 확인.
2. **multi-workflow 지원** — HashMap 기반 workflow_id 추적.
3. **timeout / cancel 메커니즘** — Drop 시 workflow cleanup.
4. **query 패턴 도입** — coroutine 이 main 의 state read 요청.
5. **production migration** — preset/approval/agent task 같은 multi-step 도메인
   하나씩 이전.

## 비범위 (out of scope)

- *모든 Intent 를 workflow 로* — 단순 cascade 는 그대로 큐 모델.
- *async IO* — worker thread + mpsc 가 이미 처리 중. intent thread 는 *workflow
  coordinator* 만.
- *multi-thread coroutine* — single intent thread 로 충분. 워크플로우 동시 진행은
  cooperatively (timeslice 없음).

## 참조

- [action-dispatch.md](./action-dispatch.md) — Intent 큐 모델의 본 문서.
- [popup-system.md](./popup-system.md) — popup 의 PopupDef 구조 (workflow 가 발화하는 popup 의 정의).
- [genawaiter on crates.io](https://crates.io/crates/genawaiter) — coroutine runtime.
