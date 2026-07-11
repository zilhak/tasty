# 부팅 시퀀스 (첫 윈도우) — 부팅 상태 머신

첫 윈도우 부팅은 **부팅 상태 머신**(`src/app/boot_machine.rs`, `BootPhase`)이 담당한다.
창은 hidden 으로 생성되고, 첫 로딩 프레임 present 후에야 표시되며(흰/OS 기본 배경
프레임 0장), 부팅 대기는 sleep 이 아니라 프레임 스텝으로 진행돼 메인 스레드가 얼지
않는다.

## 시퀀스

```
resumed() (src/app/event_handler.rs)
  ├─ 창 생성: WindowAttributes .with_visible(false)     — 축A: hidden 생성
  ├─ Settings::load + normalize
  ├─ create_gpu_state (동기, pollster)                  — 이 동안 창은 hidden
  ├─ shell 무효 시: shell setup 첫 프레임 렌더 → set_visible(true) → early return
  └─ begin_boot(window, gpu, settings, window_hidden=true)
       ├─ db::init + theme apply (첫 present 전 — 부팅 중 배경색 전환 방지)
       ├─ render_loading 첫 프레임 present (theme bg_app 단색)
       ├─ set_visible(true)                             — 실패 시에도 표시 (fallback)
       └─ App.boot = Some(BootState { phase: GpuInit })

부팅 미완(App.boot Some) 동안 매 프레임:
  about_to_wait → drive_boot_frame() → ControlFlow::WaitUntil(+16ms) 재예약
  RedrawRequested → drive_boot_frame()   (같은 스텝 함수 — 중복 호출 무해)

  phase 스텝 (BootPhase):
  GpuInit          엔진(CoreState, layout.json 로드) + plugin manager 초기화 —
                   동기 원자 스텝(이 동안 프레임 정지는 수용).
                   pending layout restore 있으면 → WaitingPlugins, 없으면 → Ready
  WaitingPlugins   pump → finalize_plugin_hello → 필요 surface kind 등록 확인.
  (deadline 300ms) 미충족이면 다음 프레임 재시도. 충족/초과 시
                   ApplyPendingLayoutRestore 1회 apply 후 → RestoringLayout
  RestoringLayout  pump(조건 확인 전 무조건 1회) → RemoteSurface 복원 round-trip
  (deadline 500ms) pending 확인. 완료/초과 시 → Ready

finish_boot (Ready):
  AppState 조립 → db/theme 실패 InfoModal → IPC 서버 시작 + 웹훅 init →
  register_window(MainView 등록) → system.startup_complete 발화 →
  부팅 중 지연된 AppEvent 재생 → 첫 실 UI 프레임 요청
```

- 진입 경로는 2개이며 둘 다 `begin_boot` 로 들어온다: ① 일반 부팅(`resumed()`,
  hidden 생성 → 첫 present 후 표시), ② shell setup 완료(`Confirmed` — 창이 이미
  보이므로 표시 전환만 스킵, phase 구동 동일).
- 다중 창(`create_new_window`)·parked 복원은 상태 머신을 타지 않고 동기 경로
  (`create_app_state`)를 유지한다. 두 경로는 같은 스텝 함수
  (`ensure_engine_and_plugins` / `boot_pump_step_*` /
  `boot_apply_pending_layout_restore` / `assemble_app_state`,
  `src/app/window_lifecycle.rs`)를 공유하므로 대기 의미론이 이중화되지 않는다.

## 부팅 가드 (bootstrap 불변식)

`ApplyPendingLayoutRestore` 는 "적용 전 다른 mutate 가 없다"는 bootstrap 전제를
가진다. 상태 머신에서는 이벤트 루프가 이미 도는 중에 apply 되므로, 부팅 미완 동안:

- **window event** 는 전부 소비한다 (`handle_boot_window_event` — RedrawRequested
  = 스텝 구동, Resized = gpu.resize, CloseRequested = 종료, 그 외 무시).
- **AppEvent** 는 종료 계열(Shutdown/QuitRequested)만 즉시 처리하고 나머지는
  `BootState.pending_events` 에 지연 → Ready 후 도착 순서대로 재생한다. 특히
  `TerminalOutput` 을 부팅 중 소비하면 대상 engine 이 아직 views 밖
  (`App.core_state`)이라 waker dedup 게이트가 닫힌 채 wake 가 유실된다.
- **IPC** 는 서버 자체가 `finish_boot` 에서 시작하므로 부팅 중 유입이 구조적으로
  없다. `about_to_wait` 의 steady-state 파이프라인(plugin pump / intent drain 등)도
  부팅 미완 동안 타지 않는다.
- `resumed()` 재진입(macOS 등)은 `boot.is_some()` 가드로 창 중복 생성을 막는다.

## 로딩 프레임

`GpuState::render_loading`(`src/gfx/gpu/loading.rs`) — 배경 단색(theme `bg_app`
토큰) clear 만 그리는 빈 egui 프레임. 콘텐츠(스피너·문구)는 로딩 화면 UI 작업이
`BootPhase` 인자를 받아 얹는다. hidden 창은 `RedrawRequested` 를 못 받을 수
있으므로 첫 프레임은 `begin_boot` 가 이벤트 대기 없이 직접 그리고, 이후는
`about_to_wait` 의 WaitUntil 워치독이 스텝을 보장한다.

## 부팅 계측 (target: `tasty::boot`)

부팅 경로에는 상시 tracing 계측이 박혀 있다. debug 빌드는
`$TASTY_HOME/debug-dev.log`(debug 레벨 file layer)에 수집되고, stderr 기본 필터가
warn 이라 콘솔 노이즈는 없다. release 검증은 `TASTY_LOG=info` 로 실행한다.

| 마커 | 구간 |
|------|------|
| T1 window_create | `resumed()` 진입 → `create_window` 반환 (hidden) |
| T2 gpu_init | `create_gpu_state` |
| T2.5 db_theme | `begin_boot` 진입 → 첫 로딩 프레임 직전 (db::init + theme apply) |
| T2.9 window_visible | 부팅 시작 → `set_visible(true)` (첫 로딩 프레임 present 후) |
| resumed_total | `resumed()` 전체 (T1~T2.5 + 첫 프레임 — 메인 스레드 점유 구간) |
| T2.6 engine_init | CoreState 생성 + layout.json 로드 |
| T3a/T3b | plugin discovery / spawn (T3b 의 total_ms = T3 전체) |
| T4 layout_wait_plugins | WaitingPlugins 체류 (탈출 사유 satisfied/deadline) |
| T5 layout_apply | ApplyPendingLayoutRestore |
| T6 remote_surface_wait | RestoringLayout 체류 (탈출 사유 satisfied/deadline) |
| boot_total | 부팅 시작 → Ready |
| T7 first_paint | Ready → 첫 실 UI present (`src/boot/trace.rs` 원샷) |

실측 기준치(Windows/7950X3D, debug): T2 ≈ 540~570ms, T3 ≈ 430~465ms(플러그인 11개
설치 시; 첫 설치는 ≈0.6ms)가 지배 구간이고 T4/T6 은 <3ms(satisfied)다. 두 원자
스텝(T2·T3)은 동기라 그 동안 로딩 프레임이 갱신되지 않는다 — 워커 분리 escalate
판단은 이 수치가 기준.
