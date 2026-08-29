# E2E 테스트 — 격리 + timeout 정책

`tests/e2e_tests.rs` 는 실 tasty 바이너리를 spawn 하여 IPC 로 조작하는 end-to-end 테스트다. `tests/common/mod.rs` 가 공통 하네스이며 진입점이 둘이다 — 공유 인스턴스 `common::shared()` 와 전용 인스턴스 `TastyInstance::spawn`. 자체 검증 절차는 [self-verification.md](self-verification.md), debug 전용 IPC 는 [debug-ipc.md](debug-ipc.md).

## 0. 전제: plugin 바이너리 최신화 (필수)

**e2e 는 `cargo test --test e2e_tests` 단독 실행 전에 `cargo build --workspace` 가 선행돼야 한다** (또는 처음부터 `cargo test --workspace` 사용). package 한정 test 는 본체(tasty.exe)만 빌드하고 plugin bin crate 들을 빌드하지 않는데, dev bundle(`target/debug/builtin-plugins/`)은 **매니페스트는 소스에서, 바이너리는 target exe 에서 독립적으로** `copy_if_newer` 하므로 stale plugin exe 가 최신 매니페스트를 달고 격리 TASTY_HOME 에 설치된다. 이 drift 는 plugin↔host 계약이 바뀐 직후(예: `markdown.recent` 의 host adapter 이관) namespace 호출을 "Method not found" 로 깨뜨린다. 호스트는 hello 시 바이너리 보고 버전 ≠ 매니페스트 버전이면 `version drift` warn 을 남긴다 — spawn 실패 진단 시 stderr tail 에서 이 경고를 먼저 확인.

## 1. 환경 격리

인스턴스마다 `$TMPDIR/tasty-test-home-{pid}-{nanos}/` 를 새 HOME 으로 만들고, host 환경 누수를 spawn 직전에 차단한다:

| env | 처리 | 이유 |
|-----|------|------|
| `HOME` / `ZDOTDIR` | 격리 HOME override | zsh rc 위치 격리 (macOS/Linux 의 `~/.tasty/` 격리도 겸함) |
| `TASTY_HOME` | 격리 `.tasty` 로 명시 | tasty 루트 해석은 `directories::BaseDirs`(=Windows 는 USERPROFILE) 기반이라 **HOME 만으로는 Windows 에서 격리되지 않는다** — 실사용자 `~/.tasty-debug` 세션 복원이 새어든다. `TASTY_HOME` 이 루트 override 의 SoT |
| `SHELL` | 제거 | host login shell 누수 차단(`detect_bash` 의 `$SHELL` 경로) |
| `OH_MY_ZSH` / `ZSH` | 제거 | oh-my-zsh customization 누수 차단 |
| `TASTY_SURFACE_ID` | 제거 | 부모가 tasty 안일 때 augmented-help 분기 차단 |
| `RUST_LOG` | `tasty=info` | child stderr 폭주에 의한 OS pipe backpressure 회피 |

격리 HOME 에 사전 작성하는 파일:

| 파일 | 내용 | 이유 |
|------|------|------|
| `.zshrc` / `.bashrc` | 빈 파일 | shell rc customization 차단 |
| `.tasty/config.toml` | `shell="/bin/sh"`(POSIX) / Git Bash(Windows) + `restore_layout=false` | `is_shell_valid()` 즉시 true → `detect_bash()`(host `/etc/passwd` 의존) 미호출 → **shell_setup_mode 진입 차단** |

`shell_setup_mode` 에 진입하면 port file 이 영구히 안 써져 spawn 이 timeout panic 한다. config.toml 사전 작성이 이 경로의 *결정적* 차단이다.

## 1-a. 공유 인스턴스 (`common::shared()`)

`common::shared()` 는 **호출한 test binary 하나가 공유하는** tasty 인스턴스를 돌려준다. 첫 호출에서만 프로세스를 띄우고 이후 호출은 같은 `&'static` 핸들을 준다.

| 축 | 정책 | 이유 |
|----|------|------|
| 공유 범위 | **test binary 당 1개** (`cargo test` 전체 1개가 아니다) | `OnceLock` 은 프로세스 로컬 정적 상태이고 cargo 는 test 타겟마다 별도 프로세스를 띄운다. 더 줄이려면 test binary 개수 자체를 줄여야 한다 |
| 직렬화 | **안 한다** — lock 없이 `&'static` 만 공유 | IPC 서버는 연결마다 별도 스레드로 받아 mpsc 로 큐잉하므로 동시 호출이 안전하다. (`gui_common::shared()` 가 `MutexGuard` 로 완전 직렬화하는 건 실제 데스크톱 마우스/포커스를 뺏는 입력 주입을 쓰기 때문이고, 이쪽은 IPC 전용이라 해당 없음) |
| 테스트 격리 | `TastyInstance::create_workspace()` 로 테스트마다 자기 workspace | IPC 생성은 `IntentOrigin::Agent` 라 active 를 전환하지 않고(원칙 1·3), attach 점유도 workspace/surface 단위 lock 이라 서로 다른 workspace 는 병렬 공존한다 |
| 정리 | `Drop` 이 아니라 `atexit` | 정적 저장이라 `Drop` 이 영원히 돌지 않는다. atexit 가 graceful `system.shutdown` → force kill → port file·격리 HOME 삭제를 수행한다. `Drop` 은 전용 인스턴스 경로로 그대로 남는다 |
| spawn 실패 | 첫 실패 후 **재시도하지 않는다** | `OnceLock::get_or_init` 은 초기화 클로저가 panic 하면 미초기화로 남아 다음 테스트가 그대로 재시도한다 — 부팅 timeout 상황에서 테스트 수만큼 GUI 프로세스가 더 뜨는 증폭을 막는다. S1 timeout panic 자체도 이제 자기 child 를 kill 하고 격리 HOME 을 지운다(`Child` 의 Drop 은 kill 하지 않아 그냥 두면 orphan 이 된다) |

격리 헬퍼가 돌려주는 `TestWorkspace` 는 `workspace.create` 응답의 `id` / `index` / `surface_id` 를 그대로 담는다. 공유 경로에서는 `first_surface_id()` / `first_pane_id()`(목록의 `[0]` 번째를 집는다 — 전용 인스턴스 전용) 대신 `first_surface_id_in_workspace()` / `first_pane_id_in_workspace()` 를 쓴다.

**공유 대상이 아닌 경로**:

- `spawn_with_inherit_cwd(true)` — `inherit_cwd` 는 격리 HOME 의 `config.toml` 에 미리 써넣는 *프로세스 기동 시점* 설정이라 런타임 교체가 불가능하다.
- `tests/soak_memory.rs` — 프로세스 트리 RSS 를 외부에서 측정하므로 전용 프로세스가 맞다.

**workspace 로 격리되지 않는 전역 상태**: headless PTY(`pty.*`), `global_hook.*`, notification 은 전역 목록이라 같은 binary 의 다른 테스트가 만든 항목까지 함께 조회된다. 공유 인스턴스 위의 목록 검증은 "내 것이 있는가"(`any`) 형태로 쓰고 길이나 `[0]` 번째를 assert 하지 않는다. surface hook(`hook.unset`)과 headless PTY(`pty.kill`)는 인스턴스가 test 프로세스와 함께 죽으므로 회수가 필수는 아니지만, 같은 binary 의 후속 테스트를 오염시키지 않도록 만든 테스트가 회수하는 것을 기본으로 한다. workspace 자체는 회수하지 않는다 — `workspace.close` IPC 가 없고 회수할 이유도 없다.

`attach_*` test binary 들이 쓰는 attach 스트림 frame/handshake 헬퍼(`read_frame` / `write_control_frame` / `open_workspace_attach` / `open_surface_attach` / `open_stream_without_attach` / `wait_for_control_event`)는 **`tests/attach_common/mod.rs`** 한 곳에 있다 — `tests/common`(인스턴스 하네스)·`tests/webhook_common`(웹훅 하네스)과 같은 층위의 세 번째 공유 test 모듈이다. 개별 `#[test]` 파일끼리는 서로 `mod` 할 수 없지만 디렉토리 모듈은 여러 test binary 가 각자 `mod attach_common;` 으로 가져갈 수 있으므로, 파일마다 복제하지 않는다. 이 모듈에는 "첫 workspace 를 집는" 헬퍼를 두지 않는다 — 공유 인스턴스 위에서 그 습관이 남으면 남의 격리 단위를 밟는다.

하네스 자체 검증은 `tests/shared_instance_harness.rs` — 공유 재사용(spawn 횟수 1 · 동일 port), workspace id 유일성, 전역 `pty.list` 의 `any` assert 가 병렬/`--test-threads=1` 양쪽에서 통과하는지를 확인한다.

## 2. Timeout (2단계)

| 단계 | 상수 | 값 | 조건 |
|------|------|-----|------|
| S1 | `SPAWN_PORT_TIMEOUT` | 30 s | `--port-file` 에 port 가 쓰여짐 |
| S2 | `SPAWN_SHELL_TIMEOUT` | 15 s | first surface `screen_text` 가 non-empty (첫 PTY prompt) |

값은 단순 증가가 아니라 **결정적 fix(config.toml 사전 작성) 위의 마진**이다 — dev cold path worst-case(GPU init + plugin discover/extract + theme/db init, dev 프로필 ~3.5× 느림) + self-hosted runner 변동 폭 흡수.

## 3. stderr tail 진단

spawn timeout panic 시 child stderr 마지막 30 라인을 panic 메시지에 첨부한다. `Stdio::piped()` + background drain thread + 링버퍼(capacity 256)로 OS pipe buffer(Linux 64KB / macOS 16KB)가 차서 child 가 write block 되는 것을 방지. `RUST_LOG=tasty=info` 로 verbosity 를 cap 한다(drain 1차 + cap 2차 방어).

## 4. Flaky 대응 절차

1. panic 의 `--- stderr (last 30 lines) ---` 확인.
2. 마지막 `tracing::info!` 단계 식별: `IPC server listening on 127.0.0.1:{port}` 가 보이면 **S2**(PTY prompt) timeout → shell path/rc 점검. 안 보이면 **S1** → config.toml shell 유효성·plugin·theme init 점검.
3. 재현: `cargo test --test e2e_tests -- --nocapture`.
4. 결정적 차단이 깨졌으면 `TastyInstance::spawn` 의 env/config 보강.

> `tests/gui_common/mod.rs`(gui_tests)는 전수 `#[ignore]` 라 일반 `cargo test --workspace` 에서 실행되지 않는다.

## VTE 시뮬레이터 (`tasty-tui-simulator`)

터미널 동작 검증용 도구 — 고수준 명령을 raw VTE escape 시퀀스로 변환해 출력한다(터미널 입장에선 실제 TUI 앱과 같은 바이트 스트림). **인터랙티브 모드**(stdin REPL — 외부에서 `surface.send` 로 명령 단계 전송, 명령마다 `OK` 동기화)와 원샷 시나리오를 제공한다. 명령: cursor/print/sgr/fg·bg/altscreen/scroll-region/erase/raw/esc 등, 종료 제어 `quit`/`exit-code N`/`crash`(SIGABRT)/`panic`. debug 의 `debug.cell_info`/`debug.screen_attrs`([debug-ipc](debug-ipc.md))와 조합하면 셀 속성을 결정적으로 자동 검증할 수 있다.

로직은 `lib.rs` 에 있고 두 진입점이 공유한다(SoT 하나) — 독립 바이너리 `tasty-tui-sim`(`cargo build -p tasty-tui-simulator`, release 빌드 가능) 과 `tasty debug sim <subcommand>`(debug 빌드 한정). **debug 빌드에선 별도 빌드/PATH 설정 없이** `tasty debug sim ...` 으로 바로 호출할 수 있다(이미 `tasty` 가 PATH 에 있으므로). surface 안에서 stdout 에 직접 VTE 를 뿜는 로컬 동작이라 IPC 를 거치지 않는다. 자세한 명령 목록·부하 모드(`flood`)는 [tui-testing](tui-testing.md).

## 관련

- [self-verification.md](self-verification.md) — 커밋 전 시나리오 재현
- [build.md](build.md) — dev/release/dist 프로필 (timeout 값 산정 근거)
