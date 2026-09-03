# ADR-0096: 유닛 테스트는 사용자 환경을 읽지 않는다 — 설정은 주입, env 는 RAII 복원

- **Status**: Accepted
- **Date**: 2026-09-02
- **Tags**: testing, isolation, settings, env, harness, ci, regression-detection

## Context

`cargo test --workspace --locked` 의 결과가 **실행하는 사람의 로컬 상태에 따라 달라졌다.**

- `CoreState::new_with_ids` 가 `Settings::load()` 로 `$TASTY_HOME/config.toml` 을 읽었고, 테스트 전용 생성자 `CoreState::new` 도 그 경로를 그대로 탔다. 사용자 홈에 `workspace_categories_enabled = true` 가 있으면 기본값(`false`)을 전제한 테스트 3 건이 깨졌다 — `state::tests::crosses_category_on_single_category_falls_back_to_local_wrap`, `adapters::ui::input::shortcuts::tests::{axis_combos_do_not_cross_route, shortcut_new_workspace_stays_normal_when_categories_off}`.
- 여러 테스트가 `std::env::set_var` 로 프로세스 전역 env 를 바꾼 뒤 마지막 줄의 `remove_var` 로 "정리" 했다. 단언이 패닉하면 그 줄에 도달하지 못하고, 도달하더라도 실행 환경에 원래 값이 있었으면(`TASTY_HOME` · `TASTY_SURFACE_ID` · `TASTY_AGENT_ID` 는 tasty 터미널 안에서 실제로 설정돼 있다) 그 값을 잃는다. 오염은 같은 프로세스의 뒤따르는 테스트로 전파된다.
- `tasty_home()` 은 `TASTY_HOME` 을 `$HOME` 보다 우선한다. `HOME` 만 임시 디렉토리로 바꿔 격리하던 `tasty-host-plugin::bundle_sig` 의 trust DB 테스트는, 실행 환경에 `TASTY_HOME` 이 잡혀 있으면 임시 홈이 아니라 그 루트를 읽어 실패했다.

비용은 실측됐다. 2026-09-02 병렬 작업에서 서로 다른 4 개 worktree 의 작업자가 같은 실패를 각자 만나 **독립적으로 같은 원인을 4 중으로 재조사**했다. 변경과 무관한 실패가 상시로 섞이면 "내 변경이 깬 것인가" 판정이 매번 수동 대조가 되고, 회귀 감지력이 그만큼 떨어진다. CI 러너와 개발자 머신의 결과도 갈린다.

## Decision

**유닛 테스트는 사용자 환경을 입력으로 받지 않는다.** 두 축으로 강제한다.

1. **설정은 생성자가 주입한다.** `CoreState::new`(테스트 / non-host 진입점)는 `Settings::default()` 를, `CoreState::new_with_ids`(host 부팅 경로)는 `Settings::load()` 를 넣고 둘 다 `new_with_ids_and_settings(..., settings)` 로 합류한다. 설정 출처를 고르는 지점이 하나뿐이라, 새 진입점을 만들 때도 "파일을 읽을 것인가" 가 명시적 선택이 된다. 특정 설정이 필요한 테스트는 엔진을 만든 뒤 해당 필드만 바꾼다.
2. **env 조작은 RAII 가드로만 한다.** 가드는 진입 시 원값을 보관하고 `Drop` 에서 되돌린다(원래 없었으면 제거) — 패닉 경로에서도 복원된다. 호스트 crate 는 `crate::test_support` 의 `TastyHomeGuard`(공유 락 + 임시 홈 + 복원)와 `EnvVarGuard`(임의 키 + 복원)를 쓰고, 다른 crate 는 같은 형태의 로컬 가드를 둔다. `HOME` 을 갈아끼워 격리하는 가드는 `TASTY_HOME` 도 함께 비운다 — 그러지 않으면 우선순위 때문에 격리가 통째로 무효가 된다.

격리 여부의 판정 기준은 **같은 명령을 다른 환경에서 돌려 결과가 같은가** 다: 사용자 실제 홈 / 빈 `TASTY_HOME` / `--test-threads=1` 세 조합이 모두 같은 결과를 내야 한다.

## Consequences

- **얻은 것**: 사용자 홈에 `workspace_categories_enabled = true` 가 있는 상태 그대로 `cargo test --workspace --locked` 가 전부 통과한다. 위 세 조합의 결과가 일치해, 실패가 나오면 그것이 곧 변경의 회귀다. 같은 원인을 여러 작업자가 재조사하는 비용이 사라진다.
- **잃은 것**: 테스트가 "사용자 설정이 실제로 반영되는가" 를 우연히 검증하던 경로가 없어진다. 그 검증은 `Settings` 자신의 테스트(임시 디렉토리를 명시적으로 가리키는 쪽)가 담당해야 하며, 엔진 조립 단계의 설정 반영을 보려면 필드를 명시적으로 세팅하는 테스트를 따로 써야 한다.
- **운영 비용 / 유지 부담**: env 를 만지는 새 테스트마다 가드를 쓰도록 지켜야 한다. 현재는 리뷰 규율이며 정적 강제는 없다 — 위반은 "환경에 따라 다른 결과" 로 늦게 드러난다.

## Alternatives Considered

- **A: 테스트 러너를 항상 격리된 `TASTY_HOME` 으로 실행(`.cargo/config.toml` 의 env, 또는 래퍼 스크립트)** — 설정 유입은 막히지만 두 가지가 남는다. env 미복원 오염은 그대로이고(오히려 `TASTY_HOME` 이 항상 설정된 상태가 돼 `HOME` 기반 격리 가드를 전부 무력화한다 — 실제로 이 조합에서 `bundle_sig` 2 건이 새로 실패했다), `cargo test` 를 직접 친 사람은 여전히 오염된 결과를 본다. 격리를 실행 방법이 아니라 코드에 두는 편이 우회 불가능하다.
- **B: `Settings::load()` 를 `cfg(test)` 에서 기본값으로 단락** — 호출 지점 수정 없이 끝나지만, `Settings` 자신의 파일 로드 테스트까지 무력화되고 "테스트에서는 이 함수가 거짓말을 한다" 는 숨은 규칙이 생긴다. 주입 지점을 드러내는 편이 읽는 사람에게 정직하다.
- **C: 실패하는 테스트 3 건만 `engine.settings` 를 명시 세팅해 고친다** — 증상만 없앤다. 같은 함정에 빠질 테스트가 계속 늘고(엔진을 만드는 테스트 전부가 후보), env 미복원 축은 손도 대지 못한다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

- `CoreState::new` 가 테스트 밖(프로덕션 경로)에서 호출되기 시작한다 — 그 경로가 기본 설정으로 도는 것이 맞는지 다시 판단해야 한다.
- env 를 만지는 테스트가 가드 없이 재유입되는 사례가 반복된다 — 리뷰 규율 대신 정적 검사(테스트로 소스를 훑는 형태)를 도입할지 판단한다.
- `tasty_home()` 의 우선순위(`TASTY_HOME` > `HOME`)가 바뀐다 — 3 번 항목의 전제가 무너진다.

## References

- [`docs/dev-guide/unit-test-isolation.md`](../dev-guide/unit-test-isolation.md) — 현재 운영 규칙과 가드 사용법
- [ADR-0090](0090-test-isolation-by-workspace-not-process.md) — e2e 테스트의 격리 단위(다른 축)
- `crates/tasty-utils/src/path.rs` — `tasty_home()` 의 override 우선순위
