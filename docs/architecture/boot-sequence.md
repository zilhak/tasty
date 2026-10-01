# 부팅 시퀀스 (첫 윈도우) — 부팅 상태 머신

첫 윈도우는 `src/app/boot_machine.rs`의 `BootPhase` 상태 머신으로 부팅한다. 숨긴 창에서 첫 로딩 프레임을 그린 뒤 창을 표시한다. 대기는 sleep 대신 프레임마다 진행 상태를 확인하는 방식으로 처리한다.

## 시퀀스

```text
resumed → 숨긴 Window/GPU 준비 → begin_boot
  App.boot: BootResources(window, gpu, worker receiver, pending events)
  App.state.boot: BootProgress(settings, phase, restore presentation)
  첫 loading present 뒤 Window 표시

GpuInit → WaitingEngine
  worker가 EngineSession과 PluginManager를 준비
  결과를 pending engine 관계에 설치
WaitingJournal
  journal binding·초기 projection → 필요한 kind 확인
  → WaitingPlugins(필요한 경우) → WaitingJournal로 복귀
  → 선택한 자료의 activation/복원 완료를 poll
RestoringLayout
  native/plugin 복원 응답을 관측 → finish_boot
finish_boot
  MainViewState 조립 → IPC 서버와 window 등록 → startup_complete
  → 보류된 AppEvent 처리 → 정상 프레임
```

`BootResources`의 handle과 `BootProgress`의 값은 소유자가 다르다. worker 반환형도 CoreState 한 개가 아니라 EngineSession과 PluginManager다. journal의 초기 projection만 재생하는 단계와 실제 자원 activation을 구분한다. 오류 화면을 만들 수 있는 Window/GPU가 있으면 진단을 표시하고, 초기 View 조립 실패로 기존 journal stream을 삭제하지 않는다.

일반 부팅과 shell setup 완료는 같은 begin_boot로 합류한다. 새 창·parked 복원은 `window_lifecycle/pending.rs`의 pending 관계와 journal 준비 관측을 사용한다. 공유 registry는 AppServices의 같은 Arc를 주입한다. 특정 첫 View의 실행 자원을 전역 서비스 원본으로 사용하지 않는다.

## 부팅 가드 (bootstrap 불변식)

초기 journal projection과 activation을 공개하기 전에는 일반 입력·변경 처리를 보류한다. 부팅 미완 동안:

- **window event** 는 전부 소비한다 (`handle_boot_window_event` — RedrawRequested
  = 스텝 구동, Resized = gpu.resize, CloseRequested = 종료, 그 외 무시).
  CloseRequested 시 `shutdown_step_reclaim_boot_worker` 가 먼저 불려 —
  `WaitingEngine` 체류 중이면 워커 결과(최대 5s 대기)를 회수해 그 안의
  `PluginManager` 를 graceful shutdown 한다(잔존 plugin 자식 프로세스 방지).
  WaitingEngine 이 아니거나 부팅 완료 후(steady-state 종료 cascade)에는 no-op.
  이 구간의 계측은 종료 쪽 마커 `S2 boot_worker_reclaim` 이다 —
  [shutdown-sequence](shutdown-sequence.md).
- **AppEvent** 는 종료 계열(Shutdown/QuitRequested)만 즉시 처리하고 나머지는
  `BootResources.pending_events` 에 지연 → Ready 후 도착 순서대로 재생한다. 특히
  `TerminalOutput` 을 부팅 중 소비하면 대상 engine 이 아직 창에 붙기 전의 임시 관계
  (`App.engines`)에 있어 waker dedup 게이트가 닫힌 채 wake 가 유실된다.
- **IPC** 는 서버 자체가 `finish_boot` 에서 시작하므로 부팅 중 유입이 구조적으로
  없다. `about_to_wait` 의 steady-state 파이프라인(plugin pump / intent drain 등)도
  부팅 미완 동안 타지 않는다.
- `resumed()` 재진입(macOS 등)은 `boot.is_some()` 가드로 창 중복 생성을 막는다.

## 부팅에 걸리는 일 (트리거와 무관한 일)

첫 요청이 없어도 필요한 초기화는 부팅에서 수행한다. 플러그인 설치·namespace 표 등록과 프로세스 기동은 구분한다. 판단 기준은 [ADR-0026](../adr/0026-plugin-registration-and-lifecycle.md)에 있다.

지금 명부에 오른 일과 조합별 자리:

| 일 | headless | gui |
|----|----------|-----|
| 번들 plugin 설치 (`install_builtins_if_needed`) | `run_headless` (`src/boot.rs`) | `build_plugin_manager` (`src/app/window_lifecycle.rs`) |
| namespace 소유 표 설치 (`install_namespace_table`) | `run_headless` (`src/boot.rs`) | `build_plugin_manager` (`src/app/window_lifecycle.rs`) |
| agent 재시작 기록 정리·핸들 재적재 (`purge_stale_agent_state_on_boot`) | `bootstrap_engine` (`src/boot.rs`) | `finish_boot` (`src/app/boot_machine.rs`) |

소유 표 설치는 `PluginManager`가 사용하는 표의 핸들을 `tasty-ipc`에 전달하는 작업이다. 두 크레이트는 같은 표를 읽으며, `refresh_packages`가 설치된 매니페스트에서 표 내용을 갱신한다. 표를 복사하지 않는 이유는 [ADR-0026](../adr/0026-plugin-registration-and-lifecycle.md)에 있다.

설치와 namespace 표 등록 자체는 프로세스를 시작하지 않는다. 헤드리스는 이 메타데이터만 준비하고 플러그인이 필요한 첫 호출까지 기동을 미룬다. GUI의 `build_plugin_manager`는 준비 후 `discover_and_start`를 호출해 활성 플러그인을 기동한다. agent 러너 스레드는 두 환경 모두 `agent.task_run --action start` 전까지 시작하지 않는다([작업 러너](../dev-guide/agent-runner.md)).

`src/source_guards/jobs_anchored_at_boot.rs`는 GUI와 헤드리스의 지정된 부팅 함수가 필요한 초기화를 호출하는지 확인한다. 한쪽 환경의 호출이 빠져도 실패한다.

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
  [shutdown-sequence "종료 화면"](shutdown-sequence.md) · [ADR-0016](../adr/0016-window-platform-and-shutdown.md).
- 갤러리 specimen: `crates/tasty-gallery/src/catalog/chrome_loading.rs`
  (Chrome 카테고리) — 부팅 5종(기본/최소창/phase 문구 3종/문구 없음/Latte) +
  종료 2종(기본/phase 문구 4종) + 첫 실행 셸 설정 버튼 줄 1종.
- **첫 실행 셸 설정 버튼** — `render_shell_setup` 의 버튼 줄은 오른쪽 정렬이고
  간격은 `spacing_sm` 이다. 취소는 공용 `Button` Secondary, 확인은 Primary(md,
  `settings.terminal.shell_confirm` "Use this shell")다. 경로가 bash/zsh 실행 파일이
  아니면 확인을 disabled 로 그린다. 모양은 공용 disabled 규칙(중립 상자와 disabled ink)을
  따르고 별도 성공 채움은 없다. Enter 확인도 같은 조건을 따른다.

## 부팅 계측 (target: `tasty::boot`)

부팅 경로는 tracing으로 단계별 소요 시간을 기록한다. debug 빌드는
`$TASTY_HOME/debug-dev.log`(debug 레벨 file layer)에 수집되고, stderr 기본 필터가
warn 이라 콘솔 노이즈는 없다. release 검증은 `TASTY_LOG=info` 로 실행한다.

| 마커 | 구간 |
|------|------|
| T1 window_create | `resumed()` 진입 → `create_window` 반환 (hidden) |
| T2 gpu_init | `create_gpu_state` |
| T2.5 db_theme | `begin_boot` 진입 → 첫 로딩 프레임 직전 (db::init + theme apply) |
| T2.9 window_visible | 부팅 시작 → `set_visible(true)` (첫 로딩 프레임 present 후) |
| resumed_total | `resumed()` 전체 (T1~T2.5 + 첫 프레임 — 메인 스레드 점유 구간) |
| T2.6 engine_init | EngineSession 실행 owner 준비 (**부팅 워커 스레드**에서 계측; journal 선택·복원은 별도 대기) |
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
