# ADR-0180: `test_only_files` 가 "출하되는가" 의 정본이다 — cargo 통합테스트 타깃은 안 나가고, 미러는 위임한다

- **Status**: Accepted
- **Date**: 2026-09-05
- **Tags**: guards, shipping-scope, test-isolation, canonical-judge, cargo-layout, r414, adr-0129, adr-0165

## Context

"이 파일이 출하 산출물(라이브러리/바이너리)에 들어가는가" 를 여러 가드가 묻는다. 격리
가드([`env_isolation`]·[`poison_recovery`])는 이 답으로 "test 맥락인가 / 출하 코드인가" 를
가르고, 파일 SLOC 게이트([ADR-0165](0165-the-file-sloc-gate-measures-shipped-lines.md))는
"복잡도 예산에 세는가" 를 가른다.

이 물음의 정본은 `tasty_doc_guards::shipping_scope::test_only_files` 였다. 그런데 그 판정은
`#[cfg(test)] mod x;` 선언 간선의 **전이 폐쇄**로만 "출하 안 됨" 을 잡았다 — 즉 선언에서
닿는 파일만 봤다. **cargo 예약 통합테스트**(루트 `tests/*.rs`, `crates/*/tests/*.rs`)는 그
그물에 안 걸린다: 이들은 별도 테스트 타깃의 루트 파일이라 **들어오는 `mod` 간선이 없다.**
결과로 격리 가드는 통합테스트의 env/cwd 변형·조용한 poison 복구를 "출하 코드가 아니다" 가
아니라 **"test 맥락이 아니다"** 로 흘려보냈다 — 조용한 거짓 음성이다.

같은 물음이 **한 번 더** 구현돼 있었다. 본체 SLOC 게이트 대리인
(`src/source_guards/sloc_gate_skip_proxy.rs`)은 면제 근거를 종류별로 세는데, 그중
`CargoTestTarget` 가지가 `is_cargo_test_target`(패키지 루트 바로 아래 `tests/`) 을 **자체
사본**으로 판정하고 있었다. 즉 "출하되는가" 의 답이 선언 기반(정본)과 레이아웃 기반(사본)
두 곳으로 갈려 있었고, 격리 가드는 앞쪽만 봐서 뒤쪽을 놓쳤다. 소비자를 변경 전/후로 돌려
판정을 diff 하니(R442) 이 갈림이 드러났다 — 입력 필터를 정독하는 것만으로는 소비자를
절반만 세고 파손을 0 으로 셌다.

## Decision

세 문장으로 못박는다.

1. **`test_only_files` 가 "이 파일이 출하되는가" 의 정본이다.** 이 물음을 묻는 자리는 이
   판정기를 부르고 모수(스캔 범위)만 각자 넘긴다 — 답을 자기 자리에서 다시 구현하지 않는다.
2. **cargo 예약 통합테스트 타깃은 출하되지 않는다.** 패키지 루트(`Cargo.toml` 이 있는
   디렉토리) 바로 아래 `tests/` 는 cargo 가 별도 타깃으로 빌드해 lib/bin 산출물에 안 넣는다.
   이것은 **레이아웃이 강제하는 성질**이라 `#[cfg(test)]` 선언이 없어도 성립하고, 그래서
   전이 폐쇄와 **별개 갈래**로 판정한다([`is_cargo_test_target`]). `src/**/tests/` 같은 일반
   모듈 디렉토리는 패키지 루트 바로 밑이 아니라 제외된다 — 그건 출하될 수 있다.
3. **같은 물음의 사본을 두지 않는다.** SLOC 게이트 대리인은 자체 `is_cargo_test_target` 을
   버리고 정본에 위임한다. 대리인의 종류별 면제 taxonomy 는 남기되(그 물음은 "무엇이 이
   면제를 떠받치나" 로 다르다), 레이아웃 근거를 선언 근거보다 먼저 분류해 정본이 cargo
   타깃까지 잡아도 `CargoTestTarget` 가지가 굶지 않게 한다.

### 왜 통일했나 — 그리고 poison 은 왜 안 합쳤나 (R414)

통일의 근거는 "둘 다 가드라서" 가 아니라 **물음이 같아서**다. `test_only_files` 와 대리인의
`is_cargo_test_target` 은 글자 그대로 같은 것을 묻는다("cargo 타깃이라 안 나가는가") — 한
항목(어떤 파일)이 양쪽에서 다른 답을 가질 수 없다. 물음이 하나면 판정기도 하나여야 하고,
사본은 갈려서 조용히 틀린다. 이것이 이 통일의 유일한 정당화다.

**반대로 [`poison_recovery`] 는 합치지 않았다** — 물음이 다르기 때문이다. 격리 가드는 "이
테스트가 전역 상태를 직렬화 없이 만지나" 를 묻고 test 맥락을 **판정 대상**으로 삼는다.
poison 가드는 "출하되는 락 복구가 조용히 삼켜지나" 를 묻고 test 를 **면제 대상**으로 삼는다.
같은 `test_only_files` 를 쓰지만 그 답을 반대 방향으로 쓴다. 그래서 scan 범위도 다르다
(격리는 루트 `tests/` 를 보고, poison 은 출하 표면인 `src`·`crates` 만 본다). **합친 이유와
안 합친 이유가 한 문서에 있어야** 다음 사람이 규칙을 안다.

### 이 결정이 기대는 전제 (R437)

이 결정은 **"패키지 루트 바로 아래 `tests/` 는 출하 산출물에 안 들어간다"** 는 cargo
레이아웃 전제 위에 선다. 전제가 깨지는 경우 — cargo 가 통합테스트를 lib/bin 에 링크하기
시작하거나, 프로젝트가 `Cargo.toml` 경계 밖에 통합테스트를 두거나, `[[test]]` 로 `tests/`
아닌 경로를 테스트 타깃으로 선언하는 경우 — [`is_cargo_test_target`] 이 **거짓 음성**(출하
파일을 test-only 로 오판)을 낸다. 그 오판은 격리·poison·SLOC 게이트 모두에서 조용한
면제로 나타난다. 이 전제를 지키는 것은 cargo 자신의 타깃 규칙이지, 이 코드가 아니다 —
그래서 재검토 조건에 레이아웃 변경을 넣는다.

## Consequences

- **얻은 것**: 격리 가드가 통합테스트를 본다 — 요약하면 통합테스트의 env/cwd 변형과 조용한
  poison 복구가 이제 "출하 아님" 으로 올바로 분류돼 판정 안에 든다. "출하되는가" 의 답이
  한 곳에서만 나온다.
- **잃은 것**: `test_only_files` 가 파일마다 `Cargo.toml` 조상을 찾는 파일시스템 왕복을 한다
  (기존에도 `declaration_edges` 가 후보 실재를 `is_file()` 로 확인했으므로 층은 같다).
  대리인의 taxonomy 는 이제 분류 **순서**에 민감하다(레이아웃을 선언보다 먼저 봐야 한다) —
  주석으로 못박았다.
- **운영 비용 / 유지 부담**: 새 소비자가 "출하되는가" 를 물으면 `test_only_files` 를 부르고
  모수만 넘긴다. 자체 판정을 만들면 이 ADR 이 금지하는 사본이 된다.

## Alternatives Considered

- **A — 사본 유지(각 가드가 자기 판정)**: 지금까지의 상태다. `test_only_files` 는 선언만,
  대리인은 레이아웃까지 봤다. **사각이 실제로 났다** — 격리 가드가 통합테스트를 조용히
  놓쳤다. 사본은 답이 갈리고 갈린 쪽은 면제하는 방향으로 조용히 틀린다.
- **B — 형태별 하드코딩 목록**(`tests/`·`benches/`·`examples/` … 을 문자열로 나열): 성질이
  아니라 형태를 세므로 **새 형태가 생기면 조용히 샌다**. `benches/`·`examples/` 는 이 레포에
  현재 없어(양성 대조 불가, R415) 목록에 넣어도 죽은 가지가 되고, 생겼을 때 갱신을 잊으면
  출하 아님이 출하로 샌다. 성질("cargo 가 별도 타깃으로 빌드하는가")로 판정하면 그 형태가
  자동으로 들어온다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

- cargo 의 타깃 레이아웃 규칙이 바뀌어 "패키지 루트 `tests/` 는 출하 안 됨" 전제가 깨질 때
  (통합테스트가 lib/bin 에 링크되거나, `[[test]]` 로 `tests/` 밖 경로가 타깃이 될 때).
- 어떤 소비자가 "출하되는가" 를 지금과 다른 뜻으로 쓰기 시작할 때(예: example 바이너리를
  배포 산출물로 세기 시작) — 그러면 물음이 갈리므로 정본을 나누거나 각자 물음을 적어야 한다.

## References

- 정본 판정기: `crates/tasty-doc-guards/src/shipping_scope.rs` (`test_only_files`,
  `is_cargo_test_target`)
- 위임하는 미러: `src/source_guards/sloc_gate_skip_proxy.rs` (`backing`)
- 소비자: `crates/tasty-doc-guards/src/env_isolation.rs` ·
  `crates/tasty-doc-guards/src/poison_recovery.rs` ·
  `crates/tasty-doc-guards/src/bin/strip-cfg-test.rs` ·
  `src/source_guards/plugin_locale_specific_literals.rs` ·
  `tests/cli_method_table_parity.rs`
- 착지: commit `1f516da07`(정본 통일) · `72127fa86`(env SCAN_ROOTS 루트 tests/ 확장)
- 관련: [ADR-0165](0165-the-file-sloc-gate-measures-shipped-lines.md)(SLOC 게이트는 출하 줄을
  잰다) · [ADR-0129](0129-flaky-test-classes-and-standard-fixes.md)(격리 가드가 겨냥하는
  flake 형태)
