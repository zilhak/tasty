# 종료 시퀀스 — 종료 cascade + Drop tail

GUI 종료는 `App::begin_shutdown`에서 `ShutdownPhase` 상태 머신을 설치해 진행한다. 표시할 윈도우가 있으면 대기 단계 사이에 종료 화면을 그리며, 없으면 같은 단계를 블로킹 루프로 실행한다. Observer join이나 프로세스 kill·wait처럼 동기 대기가 있는 단계에서는 렌더링도 기다릴 수 있다. 선택 이유는 [종료 설계](../adr/0016-window-platform-and-shutdown.md)를 따른다.

계측은 `event_loop.exit()`까지의 종료 단계와 `run_app` 반환 뒤 `App`을 drop하는 정리 구간(Drop tail)을 나눈다. 후자는 종료 화면을 더 그리지 않는 구간이며, 전체 소요는 `shutdown_total_with_drop`으로 확인한다.

## 시퀀스

```text
begin_shutdown
  AppState에 ShutdownState 설치; native webview 숨김
  원 task scope stop·profile/port scan/screenshot 신규 admission 중단
SavingLayout
  최신 final View capture 요청 → checkpoint/필요한 retirement 완료 관측
ReclaimingBootWorker
  부팅 중 worker 결과를 회수할 기회 제공
ClosingSurfaces
  원 runner receipt 관측 → shutdown 통지 → surface lifecycle 정리
  observer join → plugin shutdown 요청
StoppingPlugins
  Remote의 socket/SSH 실제 retirement receipt
  profile/port/screenshot worker의 실제 남은 수
  plugin shutdown/kill/wait 관측
Done → event_loop.exit()
run_app 반환 → drop_app_with_trace → shutdown_total_with_drop
```

진행 중 worker의 대기 상한이 지나면 미회수 수 또는 해당 retirement의 TimedOut 관측을 기록한다. timeout은 실제 join 완료를 뜻하지 않는다. Task runner의 `RunnerStopObservation::WorkerFailed`는 이와 달리 실제 join 뒤 확인한 worker 실패다. `Joined`와 성공 여부는 다르지만 두 관측 모두 원 thread의 join은 끝났다는 뜻이다. 정상 EngineRegistry의 retiring owner를 해제하는 경로는 원 runner와 물리 자원 receipt를 기다리며 앱 종료의 상한과 구별한다.

Remote의 established tunnel과 늦은 연결 결과는 retirement worker가 child wait를 수행한다. App 스레드가 정상 Remote 세션의 SshTunnel을 직접 blocking Drop하는 경로로 설명하지 않는다. CLI의 동기 SshTunnel Drop과 예외적인 Drop tail은 별도다. observer join이나 plugin 강제 종료 후 wait 등 남은 동기 구간은 실제 지연을 측정해야 한다.


- `begin_shutdown`을 공통 진입점으로 사용해 시작 시각과 단계 순서를 맞춘다. Layout 저장은 surface 닫기보다 먼저 수행한다.
- 이미 종료 중이면 중복 요청은 바로 반환하며 단계가 처음으로 돌아가지 않는다.
- 상태 머신으로 나눴다고 모든 단계가 짧게 끝나는 것은 아니다. checkpoint 완료 대기, observer join, 강제 종료 뒤 wait 등의 실제 대기를 함께 측정한다.
- S4에서 프로세스 목록을 비웠다면 `PluginProcess::drop`에 남은 대상은 없다. 예외 경로에서 남은 대상의 drop은 kill과 wait를 수행할 수 있다.
- 정상 GUI 종료와 DB 오류 안내 모달의 종료는 App 상태 머신을 거쳐 `event_loop.exit()`로 요청한다. 오류 모달은 종료 상태만 요청하고 프로세스를 직접 끝내지 않는다.
- 오류 모달로 종료할 때는 fallback 표시 상태로 마지막 checkpoint를 덮어쓰지 않도록 새 layout 저장을 생략한다. 이미 접수한 journal 작업과 자원 회수는 계속 처리한다. `App` Drop 뒤 boot가 요청된 종료 코드를 반환한다.

## plugin 종료 대기의 겹침 (S4)

여러 플러그인의 정상 종료 대기를 겹치기 위해 요청과 확인을 나눈다.

1. `begin_shutdown_all()`은 시작 시각에서 2s 뒤의 공통 deadline을 정하고 각 플러그인에 shutdown을 요청한다. 이 호출에서 각 자식의 정상 종료를 차례로 기다리지는 않는다.
2. `poll_shutdown_all()`은 `try_wait`로 종료 여부를 확인한다. 기한이 지났거나 조회에 실패하면 kill을 시도하고 `child.wait()`로 기다린다. 이 wait는 동기 호출이므로 함수 전체를 논블로킹으로 볼 수 없다.

`shutdown_all()`은 두 단계를 반복하는 블로킹 함수다. GUI는 begin/poll을 나누어 대기 사이에 렌더링하지만, 각 호출 안의 동기 대기까지 없애지는 않는다.

지켜야 하는 제약:

- **요청 순서 계약** — shutdown 요청은 `dispatch_pending_surface_lifecycle()` 이
  같은 `req_tx` 에 이미 넣어 둔 `surface.closed` 뒤에 놓인다. plugin 이 cleanup
  대상 surface 를 모르는 채 종료되면 안 되므로 S3 → S4 순서는 고정이다.
- **요청이 큐에 못 들어갈 수 있다** — 그 큐는 개수로도 바이트로도 유한하고(바이트는 큐마다와
  모든 plugin 채널의 합계로 — [ADR-0006](../adr/0006-bounded-ipc-transport.md)),
  호스트→plugin 방향의 포화는
  대기가 아니라 **거절**이다([ADR-0006](../adr/0006-bounded-ipc-transport.md)).
  writer 스레드가 소켓에서 막혀 큐가 차 있으면 shutdown 요청이 거절되고, 그 plugin 은
  정상 종료 요청을 받지 못하고 deadline 뒤 kill·wait 경로로 처리된다. **S4a의 `killed`만으로는 원인을 구분할 수 없다.** 이 값은 "요청이 거절됐다" 와 "요청은 갔는데 plugin 이 2s 안에 안
  빠졌다" 를 모두 포함한다. 전자는 호스트 큐·writer를, 후자는 plugin을 조사해야 한다.
  큐의 거절 여부는 다음 호스트 로그로 구분한다:
  `plugin '<id>' shutdown send failed: ...` 한 줄이고(사유는 `request queue full` ·
  `request queue over its byte budget` 둘 중 하나 — shutdown 은 제어 요청이라 합계 바이트
  판정을 면제받으므로 `plugin channels over their total byte budget` 로는 거절되지 않는다,
  ADR-0006의 전송 거절 처리), 그 줄이 있으면 거절이다. 그 줄은 `warn` 이라 기본 필터(stderr `warn` · 파일 dev `debug`/release `warn`)
  에 남는다.
- **2s의 범위**: 정상 종료를 기다리는 공통 기한이다. 요청 준비와 kill·wait까지 포함한 전체 종료 시간의 상한이 아니다. S4a는 개별 처리 시간과 `graceful|killed` 결과를 기록한다.
- **종료 결과의 한계**: `poll_shutdown_all()`의 true는 관리 중인 대기 항목을 모두 처리했다는 뜻이다. kill이나 wait 실패도 로그를 남기고 `Killed`를 반환하므로, 이 값만으로 모든 OS 프로세스가 사라졌다고 단정하지 않는다.
- **단건 종료**: disable과 헬스체크 재시작은 보통 `plugin-retire-<id>` 스레드에 기다리기를 맡긴다. 스레드 생성이 실패하면 호출한 스레드가 직접 기다린다. 50ms 주기의 `PluginRetire` 타이머는 완료한 항목을 확인하며 `plugin process retired` 로그에 `ms`와 `reason`을 남긴다.
- **재시작**: 회수 작업이 끝난 tick에서 새 프로세스를 시작한다. 회수 중 disable이 오면 재시작 예약을 취소한다. 명시적 enable은 해당 회수를 기다린 뒤 시작하므로 호출이 지연될 수 있다. 이 대기도 전체 2s 상한은 아니다. 호스트 종료 중에는 다시 시작하지 않고 S4a에 `retiring before exit`를 기록한다.
- **파일 변경 전 대기**: plugin remove, swap, upgrade-builtins의 실제 쓰기, 명시적 enable은 기존 회수 작업을 기다린다. 더 높은 설치 버전이나 변경 없는 동일 버전 때문에 쓰기를 생략하면 기다리지 않는다. 쓰기 경로는 재시작 예약도 인계받아 작업 뒤 다시 시작한다. [플러그인 수명 관리](../adr/0026-plugin-registration-and-lifecycle.md)를 따른다.

## 종료 화면

종료가 확정되면 첫 종료 프레임 전에 모든 MainView 가 소유한 native webview 를
숨기고 표시 여부 캐시도 비운다. 활성 창·탭뿐 아니라 비활성 탭의 인스턴스도 대상이다.
이 처리는 일반 redraw 가 끊기기 전의 마지막 프레임에 의존하지 않고
`App::begin_shutdown` 에서 수행한다. native 자식 뷰는 GPU 표면보다 위에 있으므로
로딩 프레임만 다시 그려서는 웹뷰를 가릴 수 없다.

확인 모달을 열거나 취소하는 경로와 최소화는 이 진입점을 거치지 않는다.
숨김은 포커스를 요청하거나 웹뷰를 조기 파괴하지 않으며, layout 저장·lifecycle 통지·
plugin 종료 및 backend 최종 정리 순서는 그대로 유지한다.
웹뷰가 없거나 부팅 중인 경우에는 숨길 인스턴스가 없어 추가 대기 없이 진행한다.

상태 머신이 대기하는 프레임마다 `GpuState::render_loading` 으로 로딩 화면을
present 한다. **부팅과 같은 렌더 함수와 화면 구성**이고 다른 것은 phase 문구뿐이다 —
`render_loading` 은 phase 타입이 아니라 i18n 키를 받고, 두 상태 머신이 각자의
매핑을 소유한다(`boot_phase_text_key` / `ShutdownPhase::text_key`).

| phase | 문구 키 | 프레임을 넘기는가 |
|-------|---------|-------------------|
| `SavingLayout` | `shutdown.phase_saving_layout` | publication·View 저장·retirement가 남으면 Waiting으로 다음 회차에서 관측 |
| `ReclaimingBootWorker` | `shutdown.phase_finishing_startup` | 예 (부팅 중 종료 전용) |
| `ClosingSurfaces` | `shutdown.phase_closing_surfaces` | runner stop의 실제 join 관측 등이 남으면 기한까지 Waiting. 완료 또는 명시한 미회수 처리 뒤 다음 단계로 진행 |
| `StoppingPlugins` | `shutdown.phase_stopping_plugins` | 남은 대상이 있으면 다음 회차에서 확인. kill·wait는 동기 호출 |

SavingLayout과 ClosingSurfaces가 1ms 미만으로 끝난 측정은 아래 조건의 관측값이며 단계의 시간 제한은 아니다.

동작 규칙:

- **빠른 종료는 화면을 띄우지 않는다.** `drive_shutdown_frame` 은 한 호출 안에서
  더 진행할 수 없을 때까지 스텝을 반복하므로, 대기가 없는 종료(plugin 0 개 — 실측
  0.63ms)는 첫 구동에서 완료에 도달해 **한 프레임도 그리지 않는다.** 최소 표시
  시간이나 지연 표시 타이머가 필요 없다.
- **표시할 윈도우가 없어도 같은 상태 머신을 쓴다.** `begin_shutdown`에서 상태를 먼저 설치하고, 렌더 대상이 없으면 블로킹 루프로 끝까지 진행한다.
- **창이 여럿이면 전부 종료 화면으로 바꾼다.** 하나만 그리고 나머지를 먼저 닫으면
  창이 하나씩 사라지는 것으로 보여 크래시와 구분되지 않는다.
- **종료 가드** — 종료 진행 중에는 steady-state 파이프라인(IPC 처리 / intent drain /
  plugin pump)을 실행하지 않고 `AppEvent` 는 폐기한다(부팅 가드는 지연 후 재생하지만
  종료에는 재생할 미래가 없다). 키/마우스도 core 에 닿지 않는다.
  가드는 `event_loop.exit()` **이후에도 유지된다** — winit 은 exit 요청 즉시 루프를
  끊지 않고 콜백을 한 번 더 돌리며(Linux/X11 실측), 그 패스에서 가드가 풀려 있으면
  이미 정리가 끝난 상태로 파이프라인이 돈다. 그래서 `finish_shutdown` 은
  `App.state.shutdown` 을 비우지 않고 phase 를 `Exited` 로 옮긴다.
- **IPC 요청은 무시하지 않고 거절한다** — 가드가 `process_ipc()` 를 막으므로 이
  구간의 요청은 정상 핸들러로 처리하지 않는다. 클라이언트가 응답 없이 기다리지 않도록 매 프레임과 `exit()` 직전에 큐를 drain 해
  핸들러를 실행하지 않고 `-32000 "host is shutting down"` 으로 회신한다
  ([ADR-0016](../adr/0016-window-platform-and-shutdown.md)). 창 없는 블로킹 경로도
  같은 루프를 쓰므로 함께 덮인다.

갤러리 specimen 은 Chrome 카테고리의 "Shutdown loading screen" — 부팅 specimen 과
같은 `draw_frame` 을 공유해 두 화면의 동일성을 눈으로 확인하는 자리다.

## 헤드리스와의 비대칭

헤드리스 빌드(`--no-default-features`)의 `AppEvent::Shutdown` 은 `rx.recv()` 루프를
`break` 할 뿐 `shutdown_all` 을 호출하지 않는다(`src/boot.rs` 의 `run_headless`).
plugin 정리는 `PluginProcess::drop` 의 즉시 kill 로만 이뤄지므로 graceful 2s 대기가
없고, 이 문서의 종료 cascade 계측(S1~S4)도 기록하지 않는다. GUI 종료 상태 머신을
헤드리스에도 적용한 것으로 이해하면 안 된다.

## 종료 계측 (target: `tasty::shutdown`)

부팅 계측(`target: "tasty::boot"`, [boot-sequence](boot-sequence.md))과 같은 관례를
따른다: 항상 기록하며 레벨 `info!`, 소요는 `ms` 필드(f64 밀리초), 분기 사유는 `reason`
필드. debug 빌드는 `$TASTY_HOME/debug-dev.log`(debug 레벨 file layer)에 수집되고,
stderr 기본 필터가 warn 이라 콘솔 노이즈는 없다. release 검증은 `TASTY_LOG=info`.

| 마커 | 구간 | 추가 필드 |
|------|------|-----------|
| S1 layout_flush | `flush_layout_persistence(true)` | — |
| S2 boot_worker_reclaim | `ReclaimingBootWorker` phase (부팅 중 종료 전용, timeout 5s) | `reason = reclaimed\|unreclaimed` |
| S3 surface_close_cascade | close 큐 push + plugin broadcast | `surfaces` = 큐에 push 한 surface 수 |
| S3b observer_sink_join | close 경로가 미뤄둔 observer sink 워커 join | — |
| S4 plugin_shutdown | `StoppingPlugins` 단계 전체. 정상 종료 대기는 겹치지만 동기 정리 비용도 포함 | `plugins` = 종료 대상 plugin 수 |
| S4a plugin_shutdown_one | plugin 1개 종료 (graceful deadline 2s) | `plugin_id`, `reason = graceful\|killed\|no_child` |
| shutdown_total | 종료 진입 → `event_loop.exit()` 직전 | — |
| S5d ipc_server_drop | `TcpIpcServer::drop` (accept stop + port 파일 제거) | — |
| S5a lua_join | `LuaEngine::drop` (Shutdown send + 워커 join) | — |
| S5b pty_drop | `Pty::drop` 합계 (자식 종료 **대기는 포함하지 않는다** — [ADR-0016](../adr/0016-window-platform-and-shutdown.md)) | `ptys` = drop 된 PTY 수 |
| S5c ssh_tunnel_drop | `SshTunnel::drop` 합계 | `tunnels` = drop 된 터널 수 |
| S5 drop_tail | `run_app` 반환 → `App` drop 완료 | — |
| shutdown_total_with_drop | 종료 진입 → Drop tail 완료 (**체감 종료 시간**) | — |

읽는 법:

- **S2 는 마커가 없는 것이 정상이다.** steady-state 종료에서는 회수할 부팅 워커가
  없어 상태 머신이 이 phase 를 건너뛴다. 마커가 보였다면 부팅 로딩 화면 상태에서
  창을 닫은 것이다. 이 경로에서 회수한 `PluginManager` 는 그 자리에서
  `shutdown_all` 되지 않고 `self.plugin_manager` 에 장착되므로, **S4/S4a 는 S2 에
  중첩되지 않고 그 뒤에 나란히** 나온다(대기 중에도 종료 화면이 돈다).
- **S4 는 plugin 이 0개여도 `plugins=0` 으로 발화한다.** "안 걸렸다" 와 "계측이 안
  붙었다" 를 로그만으로 구분할 수 있어야 하기 때문이다. plugin manager 자체가
  없으면 `S4 plugin_shutdown (no plugin manager)` 로 구분된다.
- **S4와 S4a는 측정 구간이 다르다.** 정상 종료 대기는 겹치므로 각 S4a를 단순 합산하지 않는다. 그렇다고 S4가 항상 개별 최댓값과 같은 것도 아니다. 순회, kill·wait, 기존 retire 작업의 대기와 스케줄링이 영향을 준다.
- **S3b는 동기 join 시간이다.** 닫기에서 미뤄 둔 observer sink 워커가 끝날 때까지 기다린다. 워커가 이미 끝났으면 짧지만 파일 쓰기 등이 남아 있으면 길어질 수 있다. Observer가 없어도 순회와 계측 비용이 있어 항상 0이라고 보장하지 않는다. 이후 `ObserverRouter::drop`도 남은 워커를 join하므로 정리가 뒤로 미뤄질 수도 있다.
- **S3 의 `surfaces` 와 S5b 의 `ptys` 는 세는 대상이 다르다.** 전자는 layout 상의
  surface, 후자는 PTY 를 실제로 가진 backend 다 — child terminal / 헤드리스 PTY 는
  layout 밖에도 있고, PTY 없는 surface(webview 등)도 있어 두 값은 일치하지 않는다.
- **S5b/S5c 는 크레이트 전역 누적기의 전후 델타**다(`tasty_terminal::pty_drop_totals`
  / `tasty_ssh::tunnel_drop_totals`). destructor 가 개수만큼 반복돼 개별 로그로는
  읽기 어렵고, 평시(surface 닫기·attach 해제)의 drop 도 같은 누적기에 쌓이므로
  **절대값이 아니라 델타로만** 의미가 있다.
- `shutdown_total`과 각 단계의 합을 비교해 별도 계측하지 않은 시간을 찾는다. `shutdown_total_with_drop`은 Drop tail까지 포함한 값이다.

## 실측 기준치

다음은 shutdown 요청을 받은 SDK가 스스로 종료하는 경로의 측정 기록이다
(2026-09-23, Linux Xvfb /
debug GUI 빌드 / 격리 `TASTY_HOME` / 번들 plugin 9 개 전부 running 확인 후 `system.shutdown`
IPC, 정지시킨 plugin 없음, 부하 평균 36~56):

| | plugins | S1 | S3 | S4 | shutdown_total | S5 | **with_drop** |
|---|---|---|---|---|---|---|---|
| run 1 | 9 (전부 graceful) | 0.89 | 0.06 | 3.44 | 4.88 | 60 | **66** |
| run 2 | 9 (전부 graceful) | 0.83 | 0.05 | 3.78 | 5.00 | 64 | **69** |
| run 3 | 9 (전부 graceful) | 0.85 | 0.05 | 3.70 | 4.92 | 64 | **70** |

S4a 는 세 회 모두 plugin 마다 1.6~3.8 ms 였다.

이 조건에서는 S4가 3.4~3.8ms였고 Drop tail(S5)이 60~64ms로 전체 시간의 대부분을
차지했다. 응답하지 않는 plugin에는 여전히 2s deadline이 적용된다. Drop tail은 창이
사라진 뒤 실행하므로 종료 화면으로 표시할 수 없다.

헤드리스에서 번들 9개를 `plugin enable`로 시작한 뒤 하나씩 `plugin disable`한
별도 측정은 9/9 `reason="graceful"`, 개별 55~160ms였다(`plugin process retired` 로그).
이는 단건 disable 경로의 기록이며, 헤드리스 호스트 종료에 graceful 대기가 있다는 뜻은 아니다.

포트 파일은 `TcpIpcServer::drop`(S5d)에서 제거된다. 해당 측정에서는
`event_loop.exit()` 후 약 14ms에 제거됐지만, 고정된 시간 제한은 아니다.

실측 시 유의:

- 비교할 때는 plugin 수와 각 S4a의 `reason`을 함께 확인한다. 여러 plugin이 동시에
  deadline을 기다리는 경우 S4가 개별 대기의 합으로 늘어나지 않아야 한다.
- 응답 없는 plugin은 검증용으로 직접 실행한 인스턴스에서만 재현한다. 실행 기록의
  plugin PID와 격리 `TASTY_HOME`의 소유 관계를 확인한 뒤 해당 PID에만 `SIGSTOP`을
  보낸다. 이름 검색으로 찾은 프로세스나 소유를 확인할 수 없는 PID는 대상으로 삼지 않는다.
  정지시킨 plugin 수(0/1/2)를 바꾸며 대기가 직렬화되지 않는지 비교한다.

- 위 값은 Linux/debug 기준이다. 부팅 계측의 기준치(Windows/7950X3D)와는 환경이
  달라 직접 비교하지 않는다.
- **S2(부팅 중 종료)는 X11 에서 `wmctrl -i -c <win>` 로 재현된다.** 창이 뜨자마자
  닫으면 `GpuInit` 단계라 워커가 아직 없어 S2 가 발화하지 않고, ~0.1s 뒤에 닫으면
  `WaitingEngine` 단계에 걸려 S2 가 발화한다. quit 모달 경로는 키 입력이 필요한데,
  `debug.settings.apply` 로 `keybindings.quit` 을 라이브 지정한 뒤 `xdotool key`
  로 실제 키 이벤트를 보내면 된다(합성 이벤트가 아니라 XTEST 경로여야 한다).
- **종료 중 IPC는 핸들러 실행 대신 종료 오류로 응답해야 한다.** 종료 대기 중에 아무 메서드나 던져
  `{"error":{"code":-32000,"message":"host is shutting down"}}` 가 오는지 본다. 응답을 받기까지의 시간도 기록한다. 동기 종료 단계가 막히면
  큐를 확인하는 시점도 늦어질 수 있다.
- **화면이 실제로 도는지는 창 캡처로 판정한다.** `xwd -id <win>` 로 0.25~0.3s 간격
  3장 이상을 찍어 스피너 각도가 서로 다른지 본다 — 각도가 같으면 프레임이 안 도는
  것이다(정지 프레임). tasty 자체 `ui.screenshot` IPC 는 종료 중에 처리되지 않으므로
  (종료 가드) 이 판정에는 쓸 수 없다.

## 관련

- [boot-sequence](boot-sequence.md) — 대칭 구조인 부팅 상태 머신 + 부팅 계측(T1~T7)
- [`docs/dev-guide/error-handling.md`](../dev-guide/error-handling.md) — 로그 레벨 선택 기준
- [`docs/dev-guide/self-verification.md`](../dev-guide/self-verification.md) — debug 인스턴스로 시나리오 재현
- [ADR-0016](../adr/0016-window-platform-and-shutdown.md) — 종료를 프레임 구동으로 전개하고 로딩 화면을 씌운 결정

일반 종료와 오류 모달 종료가 같은 이벤트 회차에 겹치면 오류 종료 코드와 최종 화면 저장 생략을 우선한다. DAG 조회 워커는 종료 시작부터 새 요청을 받지 않으며 View I/O 회수 단계에서 완료를 기다린다.
