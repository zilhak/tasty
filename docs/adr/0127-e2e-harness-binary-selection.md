# ADR-0127: e2e 하네스가 띄울 바이너리는 한 곳에서 정한다 — 기본 조합의 GPU 종속은 `App` 이분이 선행이다

- **Status**: Accepted
- **Date**: 2026-09-04
- **Tags**: testing, e2e, headless, gpu, feature-flags, cargo, harness, adr-0090

## Context

e2e 하네스(`tests/common`·`tests/webhook_common`)는 `CARGO_BIN_EXE_tasty` 를 띄운다 — **테스트 자신과 같은 feature 로 빌드된 자기 바이너리**다. 기본 조합에서 그것은 GUI 바이너리이고, GUI 부팅은 창 + wgpu 디바이스를 만든 뒤 `finish_boot` 안에서 **비로소** IPC 를 시작한다. 즉 GPU 초기화가 막히면 IPC 는 시작조차 못 하고 port file 이 안 써진다.

문제는 그 대가를 치르는 스위트가 GPU 를 검증하지 않는다는 것이다. 인스턴스를 띄우는 test binary 11 개(기본 조합에서 실제로 도는 것) 중 화면을 보는 것은 2 개뿐이고 나머지 9 개는 IPC(JSON-RPC)와 attach stream 프레임만 쓴다. 실측(2026-09-04): 워크트리 4 곳이 동시에 스위트를 돌린 날 기록된 **모든** 실패가 같은 지점(port file 미작성, 30 초 상한)에서 났고, run 마다 깨지는 스위트가 달랐다 — 코드 인과가 아니라 GPU 디바이스 경합의 무작위 희생자라는 직접 증거다.

`--headless` 플래그는 존재하지만 gui 빌드에서는 무시된다. `run_headless` 와 `App::new_headless` 가 `#[cfg(not(feature = "gui"))]` 라 headless 진입점이 그 빌드에 **존재하지 않기** 때문이다.

## Decision

**하네스가 띄울 바이너리는 `spawn_diag::instance_bin()` 한 곳에서 정한다.** 기본값은 `CARGO_BIN_EXE_tasty` 그대로이고, `TASTY_E2E_BIN` 이 있으면 그 경로를 띄운다. 두 하네스와 웹훅 CLI 러너가 모두 이 함수를 거치며, `tests/e2e_single_instance_guard.rs` 의 세 번째 축이 다른 자리에서의 직접 선택을 막는다.

그 위에서 **기본(gui) 조합의 GPU 종속은 당분간 유지한다.** 두 대안이 지금은 닫혀 있기 때문이다.

- **런타임 headless 분기는 `App` 이분이 선행이다.** gui 빌드에서 `--headless` 가 실제로 동작하려면 `run_headless`/`App::new_headless` 의 `cfg` 를 걷어내야 하는데, `src/app.rs` 한 파일에만 `#[cfg(feature = "gui")]` 가 약 80 곳이고 그중 다수가 `App` 구조체의 **필드**다. gui 필드를 전부 `Option` 으로 바꾸거나 타입을 이분하는 본체 구조 변경이고, 그 자체가 별도 결정을 요구한다.
- **별도 test-only 바이너리 타깃은 cargo 구조상 막혀 있다.** 루트 패키지에 lib 타깃이 없어 다른 워크스페이스 멤버가 `default-features = false` 로 의존할 대상이 없고, 만들더라도 resolver v2 의 feature 통합이 normal dependency 사이에서 그대로 일어나 같은 `cargo test --workspace` 안에서 누군가 `gui` 를 켜면 headless 쪽도 함께 켜진다.

**반면 `--no-default-features` 조합에서는 전환이 이미 일어나 있다.** 같은 `CARGO_BIN_EXE_tasty` 가 그 조합에서는 곧 headless 데몬이고, 실측(`DISPLAY`·`WAYLAND_DISPLAY` 둘 다 없는 상태)으로 port file 까지 **54 ms** 다 — 기본 조합이 GPU 경합에서 30~56 초 만에 실패하던 그 단계다. `TASTY_E2E_BIN` 은 이 사실을 기본 조합에서도 쓸 수 있게 하는 로컬 탈출구다.

**GUI 가 실제로 필요한 스위트는 두 개로 확정한다** — `tests/attach_attention_loopback.rs`(매 프레임 실-포커스 해제가 검증 대상 그 자체)와 `tests/e2e_tests.rs`(`window.create` + gui cfg 로 묶인 debug 메서드). 이 판정은 **세 독립적인 방법이 같은 경계선을 그린 것**에 근거한다: ① 파일별 코드 독해로 얻은 목록, ② 헤드리스 조합 전수 실행에서 실제로 실패한 목록, ③ 아래 `TASTY_E2E_BIN` 으로 headless 인스턴스를 띄워 돌린 IPC 전용 9 개가 통과한다는 반대편 확인. ①②는 "GUI 가 필요한 쪽" 을, ③은 "필요 없는 쪽" 을 봤다 — 본 방향이 서로 달랐는데 같은 선이 나왔다. 한 방법만 쓰면 "소스만 보고 실행을 오판" 하거나 "실행 결과만 보고 원인을 오판" 하는데, 이 세션의 오판이 모두 그 형태였다.

## 발화한 트리거 — `check-headless` 가 전체 스위트로 올라갔다 (병합됨)

본 ADR 이 첫 재검토 트리거로 걸어 둔 조건이 **작성과 병합 사이에 충족됐다.** `check-headless`
잡이 `--lib --bins` 에서 **전체 스위트**로 올라갔고(`--skip` 3 건 제외), 그 결과:

- **IPC 전용 통합 검증이 자동 채널에서 headless 로 돈다.** `TASTY_E2E_BIN` 은 여전히 로컬
  탈출구이지만, "IPC 전용 스위트가 GPU 를 통과한다" 가 CI 에서는 더 이상 참이 아니다.
- **기본(gui) 조합의 GPU 종속을 유지한다는 결정 자체는 그대로다.** 기본 조합에는 여전히
  전체 스위트의 자동 실행 채널이 없고(Windows 잡은 `--lib --bins`), 대안 A·B 를 막고 있던
  이유(`App` 이분 선행 / cargo 구조)도 그대로다. 바뀐 것은 *어느 조합이 그 검증을 자동으로
  보는가* 이지 *기본 조합을 어떻게 부팅하는가* 가 아니다.

**부수 확인 하나 — GUI 필수 스위트 2 개 판정이 독립적으로 재확인됐다.** 그 잡의 명명 skip
3 건 중 둘(`hard_occupied_attention_survives_the_servers_local_focus` ·
`all_e2e_tests`)이 본 ADR 이 "GUI 가 실제로 필요하다" 고 지목한 두 스위트
(`tests/attach_attention_loopback.rs` · `tests/e2e_tests.rs`)의 테스트다. 헤드리스 잡을
넓힌 쪽은 본 조사와 다른 경로로 같은 경계선에 도달했다 — Decision 이 근거로 든 "세 방법의
수렴" 에 네 번째가 붙은 셈이다. (세 번째 skip 은 GUI 요구가 아니라 headless 배선 결함이다.)

## Consequences

- **얻은 것**: 무엇을 띄우는지가 한 곳에 모였다 — 나중에 기본 조합을 전환할 때 바꿀 자리가 하나다. GPU 경합이 실재하는 환경(워크트리 병렬)에서 IPC 전용 스위트를 GPU 밖으로 뺄 수단이 생겼다. "IPC 전용 검증이 GPU 를 통과한다" 는 사실과 그 이유가 문서·ADR 에 남았다.
- **잃은 것**: 기본 조합의 GPU 종속은 그대로다 — `TASTY_E2E_BIN` 은 **선택적 탈출구**이지 문제의 제거가 아니다. 쓰려면 사람이 headless 를 따로 빌드해야 한다.
- **운영 비용 / 유지 부담**: override 는 반드시 별도 `CARGO_TARGET_DIR` 의 산출물을 가리켜야 한다 — 같은 target 디렉토리는 `target/debug/tasty` 를 다투므로 다음 `cargo test` 가 headless 를 gui 로 덮어쓴다. 이 함정을 `docs/dev-guide/e2e-tests.md` §0-1 과 함수 doc 에 적었고, 존재하지 않는 경로는 하네스가 그 자리에서 실패시킨다.
- headless 로 띄우면 IPC 표면이 다르다 — 어느 스위트가 왜 빠지는지는 워크플로의 headless 스텝 주석이 정본이며 다른 문서는 그것을 가리키기만 한다.

## Alternatives Considered

- **A. gui 빌드에서 `--headless` 를 런타임 분기로 만든다** — 방향으로는 이것이 맞다(검증 대상이 IPC 인데 GPU 를 통과할 이유가 없다). 지금 고르지 않은 이유는 `App` 이분이 선행이라는 것뿐이고, 그것이 끝나면 이 ADR 의 결정을 다시 연다.
- **B. `--no-default-features` 로 빌드한 별도 test-only bin 타깃** — cargo 구조상 막혀 있다(위 Decision). 워크스페이스 밖으로 빼면 `cargo test --workspace` 가 그 바이너리를 만들지 않아 하네스가 사전 빌드 산출물에 의존하게 되는데, 그 의존은 "낡은 바이너리를 재고도 초록으로 읽는" 형태를 새로 들인다.
- **C. 구조는 두고 세마포어·직렬화로만 막는다** — 현행 완화책의 고착이다. 실패까지의 시간만 늘리고 GPU 를 못 잡는 환경에서는 애초에 풀리지 않는다.
- **D. GUI 부팅 순서에서 IPC 시작을 GPU 초기화 앞으로 옮긴다** — GPU 가 **느린** 경우는 풀리고 **없는** 경우는 안 풀린다. 게다가 IPC 핸들러가 `AppState` 를 요구해 "port file 만 먼저 쓰고 핸들러는 나중" 이라는 반쪽 상태의 설계가 선행 쟁점이 된다. 부팅 순서 변경은 A 와 같은 표면을 건드리므로 A 와 함께 판단하는 것이 맞다.
- **E. spawn 상한을 올린다** — GPU 를 못 잡으면 60 초든 120 초든 실패하고 실패까지의 시간만 길어진다. 상한 통일과 실패 원인 판정은 이미 별도로 처리했다(`tests/spawn_diag`).

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

- **기본 조합에도 전체 스위트의 자동 채널이 생기면** — 지금은 헤드리스 조합에만 있다(위 "발화한 트리거" 절). 기본 조합까지 자동으로 돌기 시작하면 GPU 경합이 게이트에 직접 얹히므로, 탈출구를 기본값으로 올릴지를 그때 판단한다.
- `App` 의 gui 필드 이분이 이루어져 gui 빌드에서 런타임 headless 분기가 가능해지면 — 대안 A 를 다시 연다.
- 루트 패키지에 lib 타깃이 생기거나 cargo 의 feature 통합 규칙이 바뀌어 대안 B 가 열리면.
- GUI 가 필요한 스위트가 2 개에서 늘거나 줄면 — 그 판정이 이 결정의 범위를 정한다.
- `TASTY_E2E_BIN` 이 탈출구가 아니라 상시 경로가 되면(예: CI 가 그것으로 돌기 시작하면) — 선택적 override 가 아니라 기본 동작이므로 결정을 다시 쓴다.

## References

- [ADR-0090](0090-test-isolation-by-workspace-not-process.md) — 격리 단위는 프로세스가 아니라 workspace
- [dev-guide/e2e-tests.md](../dev-guide/e2e-tests.md) §0-1 — 어느 바이너리를 띄우는가, 탈출구 절차와 함정
- [dev-guide/ci-gates.md](../dev-guide/ci-gates.md) — 어느 검사가 언제 도는지의 정본
- `tests/spawn_diag/mod.rs` — `instance_bin()`(선택) · spawn 상한 · 실패 원인 판정
- `tests/e2e_single_instance_guard.rs` — 세 번째 축(바이너리 선택의 초크포인트 강제)
