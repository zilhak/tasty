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
  GpuInit          엔진(CoreState)+plugin manager 원자 초기화(T2.6·T3) 워커
                   스레드 spawn (`build_engine_and_plugins`, App-free 함수) →
                   WaitingEngine 으로 전이. cols/rows 는 GPU cell metrics
                   의존이라 spawn 전 메인에서 계산해 워커에 값으로 전달.
  WaitingEngine    워커 결과 채널을 매 스텝 try_recv 폴링 — 원자 구간(T2.6+T3
                   ≈470ms)에도 메인은 로딩 프레임만 그리므로 스피너가 멈추지
                   않는다. 채널 payload 는 `anyhow::Result<(CoreState,
                   PluginManager)>` 로, 결과가 셋 중 하나다.
                   · Ok  → 장착하고 pending layout restore 있으면 →
                     WaitingPlugins, 없으면 → Ready.
                   · Err → engine 생성 실패(셸 spawn·PTY/fd 등). 이 단계는
                     부팅 GPU init 이후라 **GPU·창이 살아있으므로**, 진단을
                     `boot_error_info` 로 담아(`boot_engine_error_info`) 두고
                     `drive_boot_frame` 이 `enter_boot_error_mode` 로 전환해
                     **실패 화면을 창에 그려 유지**한다(사용자가 종료할 때까지 →
                     `exit(1)`). 런처로 실행해 stderr 를 못 보는 사용자도 원인을
                     본다. 진단 3줄은 `tracing::error!`(stderr + 파일 로그)로도
                     남긴다. GPU 어댑터 부재·부팅 창 생성 실패는 그릴 수단이 없어
                     이 경로가 아니다(진단 후 즉시 `exit(1)`). (ADR-0117)
                   · disconnect → **워커 스레드 자체의 예상 밖 panic** 만
                     여기로 온다(engine 생성 실패는 위 Err 로 온다). 메인 동기
                     `ensure_engine_and_plugins` 재시도로 fallback 하고, 그것도
                     실패하면 위와 같이 실패 화면으로 전환한다.
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
  (`create_app_state` → `ensure_engine_and_plugins`)를 유지한다. 동기 경로와
  워커(`WaitingEngine`)는 원자 초기화 본문으로 같은 App-free 함수
  `build_engine_and_plugins`(첫 부팅 전용 하위 함수, `src/app/window_lifecycle.rs`)
  를 공유하고, `boot_pump_step_*` / `boot_apply_pending_layout_restore` /
  `assemble_app_state` 도 두 경로가 공통으로 쓰므로 대기 의미론이 이중화되지
  않는다. 두 번째 main window 의 글로벌 Arc 공유 분기(`any_main_engine`)는
  첫 부팅엔 source 가 없어 워커 본문 밖(동기 wrapper `ensure_engine_and_plugins`)
  에만 존재한다.

## 부팅 가드 (bootstrap 불변식)

`ApplyPendingLayoutRestore` 는 "적용 전 다른 mutate 가 없다"는 bootstrap 전제를
가진다. 상태 머신에서는 이벤트 루프가 이미 도는 중에 apply 되므로, 부팅 미완 동안:

- **window event** 는 전부 소비한다 (`handle_boot_window_event` — RedrawRequested
  = 스텝 구동, Resized = gpu.resize, CloseRequested = 종료, 그 외 무시).
  CloseRequested 시 `shutdown_step_reclaim_boot_worker` 가 먼저 불려 —
  `WaitingEngine` 체류 중이면 워커 결과(최대 5s 대기)를 회수해 그 안의
  `PluginManager` 를 graceful shutdown 한다(잔존 plugin 자식 프로세스 방지).
  WaitingEngine 이 아니거나 부팅 완료 후(steady-state 종료 cascade)에는 no-op.
  이 구간의 계측은 종료 쪽 마커 `S2 boot_worker_reclaim` 이다 —
  [shutdown-sequence](shutdown-sequence.md).
- **AppEvent** 는 종료 계열(Shutdown/QuitRequested)만 즉시 처리하고 나머지는
  `BootState.pending_events` 에 지연 → Ready 후 도착 순서대로 재생한다. 특히
  `TerminalOutput` 을 부팅 중 소비하면 대상 engine 이 아직 views 밖
  (`App.core_state`)이라 waker dedup 게이트가 닫힌 채 wake 가 유실된다.
- **IPC** 는 서버 자체가 `finish_boot` 에서 시작하므로 부팅 중 유입이 구조적으로
  없다. `about_to_wait` 의 steady-state 파이프라인(plugin pump / intent drain 등)도
  부팅 미완 동안 타지 않는다.
- `resumed()` 재진입(macOS 등)은 `boot.is_some()` 가드로 창 중복 생성을 막는다.

## 부팅에 걸리는 일 (트리거와 무관한 일)

**필요성이 트리거와 무관한 일은 부팅 경로에 건다. 기동만 지연에 둔다.** 지연 자리에
같은 호출이 남아 있는 것은 재시도라 무해하고, 결함은 지연이 **유일한** 채널일 때
생긴다. 근거·부류 구분·대안은 [ADR-0178](../adr/0178-a-job-whose-need-is-independent-of-the-trigger-is-anchored-at-boot.md).

지금 명부에 오른 일과 조합별 자리:

| 일 | headless | gui |
|----|----------|-----|
| 번들 plugin 설치 (`install_builtins_if_needed`) | `run_headless` (`src/boot.rs`) | `build_plugin_manager` (`src/app/window_lifecycle.rs`) |
| namespace 소유 표 설치 (`install_namespace_table`) | `run_headless` (`src/boot.rs`) | `build_plugin_manager` (`src/app/window_lifecycle.rs`) |
| agent 재시작 정화·핸들 재적재 (`purge_stale_agent_state_on_boot`) | `bootstrap_engine` (`src/boot.rs`) | `finish_boot` (`src/app/boot_machine.rs`) |

소유 표 설치는 `PluginManager` 가 든 표의 핸들을 그것을 **해소하는** 크레이트
(`tasty-ipc`)에 넘기는 일이다 — 사본을 만드는 것이 아니라 같은 표를 가리키게 한다.
표의 *내용*은 그 뒤 `refresh_packages` 가 설치된 매니페스트에서 유도한다.

**여기서 프로세스는 하나도 안 뜬다** — 설치는 디스크에 놓는 것까지고, plugin 기동은
첫 호출까지 지연된다. agent 러너 스레드도 수동 `agent.task_run --action start` 전까지
안 뜬다([agent-runner](../dev-guide/agent-runner.md)).

`src/source_guards/jobs_anchored_at_boot.rs` 가 조합마다 그 함수 본문이 호출을 갖는지
본다. 명부가 한 조합만 덮어도 실패한다 — 원래 사고의 형태가 "한 조합에만 있었다" 였다.

## 로딩 프레임

`GpuState::render_loading`(`src/gfx/gpu/loading.rs`) — 배경(theme `bg_app` 토큰)
위에 워드마크 → 스피너 → phase 문구를 세로 중앙 스택으로 그리는 egui 프레임.
hidden 창은 `RedrawRequested` 를 못 받을 수 있으므로 첫 프레임은 `begin_boot`
가 이벤트 대기 없이 직접 그리고, 이후는 `about_to_wait` 의 WaitUntil 워치독이
스텝을 보장한다.

- **워드마크** — 수박 마크(64px) + `tasty.` mono(38px, `.` 는 `--tasty-brand-melon-flesh`)
  브랜드 락업(`guidelines/brand-logo.html` verbatim, 14px UI 폰트 상한의 sanctioned
  예외). `src/adapters/ui/brand.rs::draw_wordmark` — 사이드바 헤더(22px/17px)와
  같은 함수를 크기만 다르게 호출해 공유한다.
- **스피너** — `tasty-ui-widgets::Spinner`(공용 위젯) 재사용, 크기 32(기본
  16→boot hero), 색 `accent_primary()` 명시 지정(미지정 시 기본은
  `text_muted()`). 부팅 시작 `Instant` 가 아니라 egui `ctx().input(|i| i.time)`
  경과 기반 등속 회전 — 프레임 드랍에도 각도는 시간에 비례한다.
- **phase 문구** — `BootPhase` → i18n 키 매핑(`boot_phase_text_key`,
  `GpuInit`/`WaitingEngine` 은 문구 공유): `boot.phase_gpu_init`("Initializing graphics…") /
  `boot.phase_waiting_plugins`("Loading plugins…") /
  `boot.phase_restoring_layout`("Restoring layout…"). 스피너 아래 고정 높이
  슬롯(16px)에 그려 문구 유무와 무관하게 레이아웃이 흔들리지 않는다(첫 설치는
  `RestoringLayout` 을 스킵할 수 있어 문구 순서 불연속 허용).
- **레이아웃은 창 크기 불변** — 1280×720 기본 창과 640×480 최소 창 모두 동일
  절대 크기의 중앙 스택(반응형 축소 없음), 남는 공간만큼 `top_pad` 로 수직
  중앙 정렬.
- **전환** — Ready 도달 시 즉시 스냅(0ms). 로딩 프레임과 첫 실 UI 프레임은
  서로 다른 렌더 경로(`render_loading` → `finish_boot` → 통상 프레임 파이프라인)라
  크로스페이드는 두 경로를 한 프레임에서 합성해야 하는 구조적 비용이 크고,
  부팅 자체가 대개 1초 미만이라 디자인이 허용한 저비용 폴백을 채택했다
  (페이드 채택 시 재검토 지점 — `--tasty-motion-ui-fade` 200ms).
- **테마** — 하드코딩 다크 없음, `boot_apply_theme()` 가 첫 present 전에 저장된
  테마(Mocha/Latte)를 적용하므로 GPU clear color 도 resolved theme 을 그대로
  따라간다.
- **종료와 공유한다** — `render_loading` 은 phase 타입이 아니라 i18n 키를 받으므로
  종료 상태 머신도 같은 함수로 같은 락업을 그린다(문구만 다르다). 종료 쪽은
  [shutdown-sequence "종료 화면"](shutdown-sequence.md) · [ADR-0077](../adr/0077-shutdown-loading-screen.md).
- 갤러리 specimen: `crates/tasty-gallery/src/catalog/chrome_loading.rs`
  (Chrome 카테고리) — 부팅 5종(기본/최소창/phase 문구 3종/문구 없음/Latte) +
  종료 2종(기본/phase 문구 4종).

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
| T2.6 engine_init | CoreState 생성 + 슬롯 파일 로드 (**부팅 워커 스레드**에서 계측) |
| T3a/T3b | plugin discovery / spawn (T3b 의 total_ms = T3 전체, **부팅 워커 스레드**) |
| T2.7 engine_wait | `WaitingEngine` 체류 — 메인이 워커 결과를 기다린 시간. `frames` 필드 = 그 동안 돈 로딩 프레임 스텝 수(로딩 프레임이 실제로 갱신됐다는 계측 증거) |
| T4 layout_wait_plugins | WaitingPlugins 체류 (탈출 사유 satisfied/deadline) |
| T5 layout_apply | ApplyPendingLayoutRestore |
| T6 remote_surface_wait | RestoringLayout 체류 (탈출 사유 satisfied/deadline) |
| boot_total | 부팅 시작 → Ready |
| T7 first_paint | Ready → 첫 실 UI present (`src/boot/trace.rs` 원샷) |

실측 기준치(Windows/7950X3D, debug): T2(GPU init, 메인 고정) ≈ 540~570ms, T3(plugin
discovery/spawn) ≈ 430~465ms(플러그인 11개 설치 시; 첫 설치는 ≈0.6ms)가 지배
구간이고 T4/T6 은 <3ms(satisfied)다.

- **T2 는 메인 스레드 고정**이다 — wgpu surface 가 winit window 핸들에 결합돼
  있어 워커로 옮길 수 없다. 이 구간은 로딩 프레임 자체가 아직 없으므로(첫 present
  이전) 스피너 정지가 발생하지 않는다.
- **T2.6+T3(≈470ms)는 `WaitingEngine` 워커 스레드로 옮겨졌다** — 원자 스텝이지만
  메인은 이 구간에도 `about_to_wait` 워치독(16ms)이 매 프레임 로딩 렌더를
  지속하므로 스피너가 멈추지 않는다(T2.7 의 `frames` 로 실측 확인).
- 대기 루프(T4·T6)는 이미 <3ms 수준이라 워커로 옮길 대상이 아니다.
