# ADR-0155: 전역 상태 경합 flake 의 두 갈래와 처방을 "인자화됐는가" 에 건다

- **Status**: Proposed
- **Date**: 2026-09-05
- **Tags**: testing, flaky-tests, concurrency, test-isolation, env

## Context

"프로세스/인스턴스 전역 상태를 여러 테스트가 병렬로 밟아 깨진다" 는 flake 가 한 라운드에
세 층에서 관측됐다.

- **프로세스 전역 env var** — `TASTY_HOME`/`HOME` 을 테스트가 `set_var` 로 바꾸는 곳
  (`tasty-settings` 셸 통합, `tasty-host-plugin` 홈 격리 test_support).
- **바이너리 내 static 레지스트리** — `webview_kind` 의 `WEBVIEW_KINDS`
  (`static RwLock<Option<HashSet<String>>>`) 를 두 테스트가 `reset_for_test()` 로 비운다.
- **공유 tasty 인스턴스의 plugin 등록 상태** — e2e 공유 하네스에서 plugin 등록이 완료되기
  전에 요청이 나가는 경합.

셋 다 "전역 상태 경합" 이라는 한 단어로 뭉칠 수 있으나 **처방이 같지 않다.** 한 단어로
뭉치면 "전역이니 락 걸면 된다" 로 가는데, 그게 틀리는 자리가 있다. [ADR-0129](0129-flaky-test-classes-and-standard-fixes.md)
가 flake 를 부류 A/B/C 로 갈랐다면, 이 ADR 은 그중 A(인-프로세스 공유 상태) 안에서 **처방
등급이 갈리는 축**을 확정한다.

## Decision

**전역 상태 경합은 두 갈래로 가르고, 처방은 crate 이름이 아니라 "프로덕션이 이미 그 상태를
인자로 받는가" 에 건다.**

### 두 갈래

- **(가) 테스트 전용 전역 + `cfg(test)` 변경자** — 그 전역을 바꾸는 코드가 `#[cfg(test)]`
  뿐이라 프로덕션은 안 건드린다. 테스트끼리만 경합한다.
  → **모듈 전용 락으로 충분.** 예: `webview_kind`(`WEBVIEW_KIND_TEST_LOCK`, `reset_for_test`
  는 `cfg(test)`).
- **(나) 프로덕션도 읽는 OS/프로세스 전역 + 비원자 변경** — `set_var` 처럼 프로덕션 읽기
  (`var_os`)와 UB 를 이루는 전역. glibc `environ` 은 `set_var` 시 배열을 realloc 하고, 동시
  `var_os` 는 use-after-free 다(edition 2024 가 `set_var`/`remove_var` 를 `unsafe` 로 표시한
  이유). → **락으로 불충분.** 락 밖 읽기 테스트가 피해자이기 때문이다.

### 처방 등급 (R163 — "안 깨진다" 는 확률이 아니라 불변식이라야 말할 수 있다)

- **ⓐ 전역 뮤텍스로 쓰기 테스트 직렬화** — (가)엔 충분. (나)엔 **불충분**: 읽기까지 다 락을
  잡게 하려면 수십 자리에 락을 넣고 사람이 계속 기억해야 한다 = 확률 저감. "덜 깨진다" 만
  말할 수 있고 "안 깨진다" 는 못 말한다.
- **ⓑ 프로덕션이 env 를 안 읽고 홈 루트를 인자(`dir`)로 받게** — env 미접촉 → 경합 원천
  소멸(구조적 불변식). 근본이다. 비용은 자리마다 다르다(아래 ★).
- **ⓒ 쓰기 테스트를 별 바이너리(`tests/*.rs`)로 분리** — 그 바이너리에 `set_var` 가 0 이면
  같은 프로세스의 write·read 동시성이 구조적으로 불가 → 불변식. 내부 단위 테스트를 옮기려면
  pub 노출 비용이 든다.

(나)에서 ⓐ 는 부적합하다(불변식을 못 준다). ⓑ/ⓒ 가 불변식을 준다. 둘 중 선택은 캡슐화와
비용으로 한다.

### ★ 처방을 crate 가 아니라 "인자화됐는가" 에 건다

crate 마다 우월한 처방이 다르지만, 갈림의 진짜 축은 crate 이름이 아니다.

- **프로덕션이 이미 `_in(dir)` 로 인자화돼 있으면 → ⓑ-완성이 소수술.** 테스트를 `_in` 으로
  돌리면 env 미접촉이 된다. 실례 `tasty-settings`: `bash_rcfile_args_in`/`zsh_shell_envs_in`/
  `tasty_bashrc_default_path_in` 등 pure 본문이 이미 있고 pub wrapper 만 `tasty_dir()` 를
  읽었다. wrapper 는 그대로 두고 테스트만 `_in(Some(dir))` 로 돌렸다.
- **프로덕션이 `tasty_home()` 을 직접 읽으면 → ⓑ 는 대수술.** ⓒ(이동) 또는 thread-local
  override 를 비교해야 한다. 실례 `tasty-host-plugin`: `known_plugins.rs`·`discovery.rs`·
  `registry_state.rs` 가 `tasty_utils::path::tasty_home()` 을 직접 읽는다 — 인자화하려면
  호출 사슬 전체를 바꿔야 한다.

**다음 사람 판정 기준: "이 자리의 프로덕션이 홈 루트를 인자로 받는가" 를 먼저 봐라.** 받으면
ⓑ-완성, 안 받으면 인자화 비용(몇 곳인가)과 ⓒ/thread-local 을 비교하라. "전역 경합이니 락"
도, "전역 경합이니 이동" 도 성급하다.

### 한 crate 안에서도 처방이 섞일 수 있다 (settings 실측)

`tasty-settings` 는 순수 ⓑ 가 아니라 **ⓑ-완성(다수) + ⓐ-락(소수) 혼합**이다. 셸 통합 경로를
계산하는 대다수 테스트는 `_in(dir)` 로 격리했지만, **상대 `TASTY_HOME` 의 상대→절대 해석 자체가
검증 대상인** 3 개 테스트(`relative_tasty_home_is_absolutized_for_child_processes`,
`bash_rcfile_args_uses_the_resolved_root`, `effective_shell_envs_uses_the_resolved_root`)는 env
설정이 곧 피험자라 `set_var` 를 뺄 수 없다 — 이들만 `set_var` + 직렬화 락을 유지한다. 즉
**처방은 crate 단위가 아니라 "그 테스트에서 env 가 도구인가 피험자인가" 로도 한 번 더 갈린다.**

## Consequences

- **얻은 것**: (나)에 대해 "안 깨진다" 를 불변식으로 말할 수 있게 됐다. `tasty-settings` 의
  셸 통합 테스트는 env 를 아예 만지지 않으므로 다른 완주·다른 테스트의 `set_var` 와 경합할
  수 없다. 다음 사람이 처방을 고를 결정 규칙(인자화 여부)이 생겼다.
- **잃은 것**: ⓑ-완성은 프로덕션에 pure 본문(`_in`)을 요구한다 — 없으면 만드는 비용이 든다.
  ⓒ 는 내부 단위 테스트를 통합 테스트로 옮기며 pub 노출을 강제한다.
- **운영 비용 / 유지 부담**: (나)의 불변식은 **결정론적 소스스캔**으로 지킨다("이 바이너리에
  `set_var` 0" / "이 전역은 이 모듈에서만 변경"). 부하 의존 flake 는 초록 N 회로 "고쳤다" 를
  증명하지 못하므로(부하가 낮으면 원래 안 난다) 초록 횟수를 회귀 가드로 쓰지 않는다. 합성
  픽스처(2 스레드 write/read 경합)는 "레이스 실재" 를 보이는 데모용이며 그 자체가 확률적이라
  회귀 가드로는 부적합하다.

### 소스스캔 가드의 함정 (R117 — 가드 이름 ≠ 가드 술어)

`tasty-host-plugin` 의 `home_env_is_only_touched_through_this_module` 는 이름과 달리 **쓰기
유일 모듈** 만 고정하고 **읽기는 못 막는다.** (나)의 피해자는 읽기 쪽이므로 이 가드가 초록이어도
읽기-쓰기 경합은 안 잡힌다. 가드가 재는 술어를 이름과 일치시키고, 초록이 무엇을 뜻하는지(같은
바이너리 write·read 동시성 불가)와 뜻하지 않는 것(프로덕션 경로 UB, 다른 env 변수, 다른 crate)을
doc 에 명시한다. `tasty-settings` 는 불변식이 "`set_var` 0" 이 아니라 "env-피험자 3 곳만" 이라
0-상한 소스스캔 가드가 성립하지 않는다 — 그래서 가드 대신 pure 본문 옆 주석으로 "왜 이 3 곳만
env 를 만지는가" 를 박았다(가드를 못 만드는 게 아니라, 부분 허용 불변식엔 allowlist 가드의
유지비가 값을 넘는다는 판단).

## Alternatives Considered

- **모두 ⓐ(락) 로 통일** — (나)에서 불변식을 못 준다. 락은 쓰기만 직렬화하고 읽기는 lock-free
  로 남아, 프로덕션 UB 경로와 락 밖 읽기 테스트를 보호하지 못한다. 기각.
- **모두 ⓒ(별 바이너리로 이동) 로 통일** — 이미 `_in(dir)` 이 있는 crate(settings)에서는
  불필요하게 pub 표면을 늘리고 내부 단위 테스트를 통합 테스트로 강등한다. 인자화된 자리엔
  ⓑ-완성이 더 싸다. 기각.
- **crate 이름으로 처방 배정** — "settings 는 ⓑ, host-plugin 은 ⓒ" 식 매핑은 실제 갈림축
  (인자화 여부)을 가린다. host-plugin 안에서도 dir 을 받는 자리는 ⓑ 가 낫고, settings 안에서도
  env-피험자 테스트는 ⓐ 가 유일해였다. 처방은 crate 가 아니라 자리(인자화·도구/피험자)로 건다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

- ⓑ-완성을 host-plugin 프로덕션 전반(`tasty_home()` 직접 읽기)으로 넓혀야 할 이유가 생길 때
  — 예: 핫 경로에서 실제 UB 관측, 또는 파일 IO 인자화가 다른 요구로 필요해질 때.
- e2e 공유 하네스의 plugin 등록 경합에 대한 처방이 확정될 때(현재 원인만 확정, 처방 미착수).
- `set_var` 없이도 env-피험자 테스트를 결정론적으로 검증할 수단(예: 프로세스 격리 실행기)이
  생겨 settings 의 ⓐ-락 3 곳을 제거할 수 있게 될 때.

## References

- [ADR-0129](0129-flaky-test-classes-and-standard-fixes.md) — flake 부류 A/B/C 와 표준 처방
  (이 ADR 은 그 A 안의 처방 등급 축을 확정).
- [ci-gates](../dev-guide/ci-gates.md) — 자동 채널(check-windows / check-headless)에 flake 가
  상주하면 이후 작업이 진짜 회귀를 "또 그 flake" 로 넘기는 문제.
- 소스: `crates/tasty-settings/src/general.rs`(ⓑ-완성 + ⓐ-락 혼합),
  `src/core/surface_registry/webview_kind.rs`(가 갈래 = 락으로 충분),
  `crates/tasty-host-plugin/src/{known_plugins.rs,discovery.rs,registry_state.rs}`
  (`tasty_home()` 직접 읽기 = ⓑ 대수술).
