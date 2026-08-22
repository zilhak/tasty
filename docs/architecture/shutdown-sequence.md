# 종료 시퀀스 — 종료 cascade + Drop tail

사용자 종료(Cmd/Ctrl+Q, quit 모달, 창 닫기)는 **한 콜스택 안 동기 실행**이다. 부팅과
달리 상태 머신이 없고 프레임 스텝도 돌지 않으므로, 이 구간 내내 창은 얼어 있다.

종료 비용은 두 구간으로 나뉜다. **`event_loop.exit()` 까지**(종료 cascade)와
**그 이후**(Drop tail)다. 후자는 창이 이미 사라진 뒤에 도는 destructor 구간이라
**어떤 종료 화면으로도 덮을 수 없다** — 종료 UX 를 논할 때 이 경계가 기준선이다.

## 시퀀스

```
진입 (3경로 — 모두 App::begin_shutdown 으로 수렴)
  ├─ AppEvent::Shutdown            (src/app/event_handler.rs)
  ├─ quit 모달 "종료" → 재진입      (src/app/modal/quit.rs — 모달이 이미 열린 상태)
  └─ close_behavior == "quit"       (src/app/modal/quit.rs)

begin_shutdown (src/app/shutdown_cascade.rs)          [t0 확정]
  ├─ S1  flush_layout_persistence(true)
  │      main + parked engine 각각 SaveLayoutNow{force} → layout.json
  │      (surface.closed 발화 전에 끝나야 한다 — layout 은 *살아있는* 상태를 기록)
  └─ shutdown_lifecycle_cascade
       ├─ S2  reclaim_boot_engine_worker_for_exit    (부팅 중 종료 전용, 최대 5s)
       │      steady-state 종료에서는 no-op — 마커도 발화하지 않는다
       ├─ ―   system.shutdown_initiated 발화 (plugin cleanup hook 기회)
       ├─ S3  cascade_shutdown_close_all_surfaces + dispatch_pending_surface_lifecycle
       │      전 workspace→pane→tab→surface 순회 close 큐 push 후 plugin 으로 broadcast
       ├─ S3b observer_router.join_retired()  (창별 engine + parked engine)
       │      surface close 가 뒤로 미뤄둔 output observer sink 워커 회수
       ├─ S4  plugin_manager.shutdown_all()
       │      전 plugin 에 shutdown 요청을 **먼저 다 뿌린 뒤** 대기를 겹친다
       │      └─ S4a plugin 별 2s graceful deadline → 초과 시 force kill
       ├─ shutdown_total
       └─ event_loop.exit()

run_app 반환 (src/boot.rs) — 여기부터 Drop tail. 창은 이미 없다.
  drop_app_with_trace(app)
    ├─ S5d TcpIpcServer::drop        accept 스레드 stop + **port 파일 제거**
    ├─ S5a LuaEngine::drop           Shutdown send + 워커 join (블로킹)
    ├─ S5b PtyBackend::drop 합계     자식 셸 kill (surface 수만큼 반복)
    ├─ S5c SshTunnel::drop 합계      child.kill + wait (attach 세션 수만큼, 블로킹)
    ├─ S5  drop_tail                 run_app 반환 → App drop 완료 전체
    └─ shutdown_total_with_drop      **사용자 체감에 대응하는 값**
```

- 진입 3경로가 `begin_shutdown` 하나로 수렴하는 것은 계측 요구이자 순서 요구다.
  t0 이 경로마다 어긋나면 `shutdown_total` 의 의미가 흔들리고, S1(layout 저장)이
  S3(close cascade) 앞에 온다는 제약도 호출자마다 재현해야 한다.
- `PluginProcess::drop` 도 Drop tail 에서 블로킹(`child.wait()`)하지만, S4 의
  `shutdown_all` 이 이미 `processes` 를 drain 했다면 남은 대상이 없다.
- **정상 종료 경로에 `std::process::exit` 는 없다** — 위 Drop 들은 전부 실행된다.
  (`std::process::exit` 호출부는 초기화 실패/에러 경로 전용이다.)

## plugin 종료 대기의 겹침 (S4)

plugin 은 서로 독립 프로세스라 graceful 대기가 직렬일 이유가 없다. `shutdown_all`
은 두 단계로 나뉜다.

1. `begin_shutdown_all()` — 전 plugin 의 `req_tx` 에 shutdown 요청을 넣고 자식
   핸들만 회수한 뒤 **즉시 반환**한다. 요청을 먼저 전부 뿌리는 것이 대기 겹침의
   전제다.
2. `poll_shutdown_all()` — 논블로킹 폴링. 각 자식이 스스로 종료했는지
   `try_wait` 로 확인하고, 자기 deadline(2s)을 넘긴 자식만 force kill 한다.
   남은 대상이 없으면 `true`.

`shutdown_all()` 은 이 둘을 묶은 블로킹 형태다. 프레임을 계속 돌려야 하는
호출자는 두 함수를 직접 조합해 대기 중에도 렌더를 유지할 수 있다.

지켜야 하는 제약:

- **요청 순서 계약** — shutdown 요청은 `dispatch_pending_surface_lifecycle()` 이
  같은 `req_tx` 에 이미 넣어 둔 `surface.closed` 뒤에 놓인다. plugin 이 cleanup
  대상 surface 를 모르는 채 종료되면 안 되므로 S3 → S4 순서는 고정이다.
- **타임아웃 의미론** — 겹치는 것은 대기 구간뿐이고, plugin 하나가 받는 graceful
  기회는 여전히 2s 다. S4a 는 개별 소요와 `graceful|killed` 사유를 그대로 남긴다.
- **잔존 프로세스 없음** — `poll_shutdown_all()` 이 `true` 를 반환한 시점에 모든
  자식이 회수(exit 관측 또는 kill+wait 완료)돼 있다. 폴링을 끝내지 않고 매니저가
  drop 되면 남은 자식은 그 자리에서 kill 된다.
- **단건 경로는 그대로 블로킹** — `plugin disable` / 헬스체크 재시작 / swap 은
  대상이 하나뿐이라 겹칠 것이 없다. 요청 후 최대 2s 동기 대기를 유지한다.

## 헤드리스와의 비대칭

헤드리스 빌드(`--no-default-features`)의 `AppEvent::Shutdown` 은 `rx.recv()` 루프를
`break` 할 뿐 `shutdown_all` 을 호출하지 않는다(`src/boot.rs` 의 `run_headless`).
plugin 정리는 `PluginProcess::drop` 의 즉시 kill 로만 이뤄지므로 graceful 2s 대기가
없고, 본 문서의 종료 cascade 계측(S1~S4)도 발화하지 않는다. 이 비대칭은 의도된
것이다 — 헤드리스는 GUI 종료 UX 가 없어 graceful 대기의 값이 다르다.

## 종료 계측 (target: `tasty::shutdown`)

부팅 계측(`target: "tasty::boot"`, [boot-sequence](boot-sequence.md))과 같은 관례를
따른다: 상시 발화, 레벨 `info!`, 소요는 `ms` 필드(f64 밀리초), 분기 사유는 `reason`
필드. debug 빌드는 `$TASTY_HOME/debug-dev.log`(debug 레벨 file layer)에 수집되고,
stderr 기본 필터가 warn 이라 콘솔 노이즈는 없다. release 검증은 `TASTY_LOG=info`.

| 마커 | 구간 | 추가 필드 |
|------|------|-----------|
| S1 layout_flush | `flush_layout_persistence(true)` | — |
| S2 boot_worker_reclaim | `reclaim_boot_engine_worker_for_exit` (부팅 중 종료 전용, timeout 5s) | `reason = reclaimed\|unreclaimed` |
| S3 surface_close_cascade | close 큐 push + plugin broadcast | `surfaces` = 큐에 push 한 surface 수 |
| S3b observer_sink_join | close 경로가 미뤄둔 observer sink 워커 join | — |
| S4 plugin_shutdown | `shutdown_all` 전체 (겹친 대기의 합계 = 최댓값) | `plugins` = 종료 대상 plugin 수 |
| S4a plugin_shutdown_one | plugin 1개 종료 (graceful deadline 2s) | `plugin_id`, `reason = graceful\|killed\|no_child` |
| shutdown_total | 종료 진입 → `event_loop.exit()` 직전 | — |
| S5d ipc_server_drop | `TcpIpcServer::drop` (accept stop + port 파일 제거) | — |
| S5a lua_join | `LuaEngine::drop` (Shutdown send + 워커 join) | — |
| S5b pty_drop | `PtyBackend::drop` 합계 (자식 종료 **대기는 포함하지 않는다** — [ADR-0076](../adr/0076-close-path-per-surface-blocking-removal.md)) | `ptys` = drop 된 PTY 수 |
| S5c ssh_tunnel_drop | `SshTunnel::drop` 합계 | `tunnels` = drop 된 터널 수 |
| S5 drop_tail | `run_app` 반환 → `App` drop 완료 | — |
| shutdown_total_with_drop | 종료 진입 → Drop tail 완료 (**체감 종료 시간**) | — |

읽는 법:

- **S2 는 마커가 없는 것이 정상이다.** steady-state 종료에서는 회수할 부팅 워커가
  없어 함수가 즉시 return 하고, 그 구간의 비용은 0 이다. 마커가 보였다면 부팅
  로딩 화면 상태에서 창을 닫은 것이다. 이 경로에서는 회수한 `PluginManager` 를
  그 자리에서 `shutdown_all` 하므로 **S4/S4a 가 S2 안에 중첩 발화한다**.
- **S4 는 plugin 이 0개여도 `plugins=0` 으로 발화한다.** "안 걸렸다" 와 "계측이 안
  붙었다" 를 로그만으로 구분할 수 있어야 하기 때문이다. plugin manager 자체가
  없으면 `S4 plugin_shutdown (no plugin manager)` 로 구분된다.
- **S4 는 개별 S4a 의 합이 아니라 최댓값에 수렴한다.** 대기가 겹치므로
  plugin 이 6개면 S4a 가 각각 ≈2000ms 여도 S4 는 ≈2000ms 다. S4 가 plugin 수에
  비례해 커졌다면 대기가 다시 직렬화된 것이다.
- **S3b 는 정상 경로에서 0 에 가깝다.** surface close 는 sink 워커를 join 하지 않고
  모아두기만 하므로([ADR-0076](../adr/0076-close-path-per-surface-blocking-removal.md)),
  여기서 한 번에 회수한다. 워커들이 그동안 병렬로 이미 배수를 끝냈기 때문에 실제
  대기는 거의 없다. **이 단계를 빼면 아직 배수 중인 워커가 프로세스와 함께 죽어
  sink 파일의 마지막 항목이 잘린다** — observer 를 쓰지 않으면 항상 0 이다.
- **S3 의 `surfaces` 와 S5b 의 `ptys` 는 세는 대상이 다르다.** 전자는 layout 상의
  surface, 후자는 PTY 를 실제로 가진 backend 다 — child terminal / 헤드리스 PTY 는
  layout 밖에도 있고, PTY 없는 surface(webview 등)도 있어 두 값은 일치하지 않는다.
- **S5b/S5c 는 크레이트 전역 누적기의 전후 델타**다(`tasty_terminal::pty_drop_totals`
  / `tasty_cli::ssh::tunnel_drop_totals`). destructor 가 개수만큼 반복돼 개별 로그로는
  읽기 어렵고, 평시(surface 닫기·attach 해제)의 drop 도 같은 누적기에 쌓이므로
  **절대값이 아니라 델타로만** 의미가 있다.
- `shutdown_total` ≥ S1+S2+S3+S4 이며, 차이가 크면 계측이 덮지 않은 구간이 있다는
  뜻이다. 마찬가지로 `shutdown_total_with_drop` − `shutdown_total` = S5 다.
- **마지막 줄은 유실되지 않는다.** file layer 는 `Mutex<File>` 을 writer 로 직접 쓰고
  (`src/platform/crash_report.rs`) BufWriter 를 끼우지 않아, 프로세스가 곧 죽는
  Drop tail 구간에서도 이벤트마다 write 가 완료된다.

## 실측 기준치

Linux(X11) / debug 빌드 / 번들 plugin 전부 활성, `TASTY_LOG=info`, `system.shutdown`
IPC 로 종료. 단위 ms.

| | plugins | S1 | S3 | S4 | shutdown_total | S5a | S5b | S5 | **with_drop** |
|---|---|---|---|---|---|---|---|---|---|
| 겹친 대기, 정지 0개 | 6 (전부 killed) | — | 0.4 | 2008 | 2009 | — | — | 115 | **2124** |
| 겹친 대기, SIGSTOP 2개 | 7 (전부 killed) | 0.7 | 0.4 | 2073 | 2074 | — | — | 262 | **2336** |
| 겹친 대기, SIGSTOP 1개 | 5 (전부 killed) | — | — | 2050 | 2051 | — | — | 286 | **2337** |
| plugin 0개 | 0 | 0.5 | 0.05 | 0.001 | 0.63 | 0.18 | 101 | 153 | **158** |

참고 — 대기를 겹치기 전(plugin 하나씩 순차 대기)의 같은 환경 실측:

| | plugins | S1 | S3 | S4 | shutdown_total | S5a | S5b | S5 | **with_drop** |
|---|---|---|---|---|---|---|---|---|---|
| run 1 | 8 (전부 killed) | 1.8 | 0.07 | 16130 | 16132 | 0.11 | 55 | 518 | **16664** |
| run 2 | 6 (전부 killed) | 0.6 | 0.06 | 12094 | 12095 | 0.19 | 122 | 335 | **12444** |
| run 3 | 8 (전부 killed) | 0.6 | 0.04 | 16052 | 16053 | 0.14 | 100 | 149 | **16208** |

이 값들이 말하는 것:

- **S4 는 plugin 수와 무관하게 ≈2s 로 평탄하다.** 5/6/7개 모두 2.0~2.1s 이고,
  일부 plugin 을 `SIGSTOP` 으로 정지시켜 응답을 막아도 값이 움직이지 않는다.
  순차 대기 시절의 같은 plugin 수(6개=12.1s)와 비교하면 6배다.
- **S4 는 여전히 종료 시간의 대부분이다.** 나머지 단계(S1/S3)는 1ms 미만이고,
  체감 종료(with_drop)의 하한은 S4 2s + Drop tail 0.1~0.3s 다.
- **번들 plugin 은 실측상 하나도 graceful 로 빠지지 않는다** — 전부 2s deadline 을
  꽉 채우고 force kill 된다(`reason="killed"`). plugin 로그에는 `plugin received
  shutdown` 이 찍혀 있어 **shutdown 요청 자체는 도달**하지만 프로세스가 2s 안에
  종료되지 않는다. 즉 남은 2s 는 대기 구조가 아니라 plugin 쪽 종료 지연이다.
- **Drop tail 은 100~520ms** 로 plugin 구간에 가리지만 무시할 크기는 아니다.
  내역은 S5b(PTY) 55~122ms, S5a(Lua join) 0.2ms 미만이고, 나머지 수십~수백 ms 는
  GPU/egui/View 등 그 밖의 destructor 다. **이 구간은 종료 화면으로 덮을 수 없다** —
  창이 이미 사라진 뒤이므로, 종료 화면을 도입해도 체감 시간의 하한으로 남는다.
- **`~/.tasty/tasty.port` 는 Drop tail 초반(S5d, `event_loop.exit()` 후 ~14ms)에
  사라진다.** 제거는 `TcpIpcServer::drop` 에서만 일어나지만 Drop tail 의 앞쪽이라,
  Drop tail 이 길어도 stale 포트 파일이 남는 창은 짧다.

실측 시 유의:

- plugin 수가 run 마다 다른 것은 이 환경에서 일부 plugin 이 spawn 되지 않기
  때문이다. 순차 대기 시절에는 plugin 수가 S4 를 그대로 좌우했으므로 비교 시
  `plugins` 필드를 함께 봐야 했다. 겹친 대기에서는 S4 가 plugin 수에 둔감해지는
  것 자체가 판정 기준이다 — 비례해서 늘어난다면 병렬화가 깨진 것이다.
- 정지시킨 plugin 개수(0/1/2)를 바꿔도 S4 가 평탄한지가 회귀 확인의 핵심이다.
  재현은 `pgrep -f tasty-plugin- | head -N | xargs -r kill -STOP` 후 종료.
- 위 값은 Linux/debug 기준이다. 부팅 계측의 기준치(Windows/7950X3D)와는 환경이
  달라 직접 비교하지 않는다.
- **S2(부팅 중 종료)와 quit 모달 경로는 자동화로 재현하지 못했다.** 전자는 IPC
  서버가 `finish_boot` 에서야 시작되므로 부팅 중에는 IPC 트리거가 없고, 후자는
  키 입력/창 닫기가 필요하다(Linux 에는 키 주입 debug IPC 가 없다). 두 경로 모두
  `begin_shutdown` 이라는 같은 함수로 수렴하므로 마커 세트는 구조적으로 동일하다.

## 관련

- [boot-sequence](boot-sequence.md) — 대칭 구조인 부팅 상태 머신 + 부팅 계측(T1~T7)
- [`docs/dev-guide/error-handling.md`](../dev-guide/error-handling.md) — 로그 레벨 선택 기준
- [`docs/dev-guide/self-verification.md`](../dev-guide/self-verification.md) — debug 인스턴스로 시나리오 재현
