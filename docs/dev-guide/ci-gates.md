# CI · 훅 게이트 매트릭스 — 어떤 검사가 어디서 도는가

이 문서는 **각 검증 명령이 실제로 어디서 실행되는지**만 기술한다. "누군가 돌리고
있겠지" 로 남는 검사가 있으면 그것이 곧 사각이므로, 각 행은 자동 채널이 없으면 없다고
적는다.

명령의 내용·정책(왜 그 lint 인지, 왜 그 임계값인지)은 각 항목이 가리키는 문서에 있다.

## 자동으로 도는 것

| 검사 | 명령 | 채널 | 트리거 |
|---|---|---|---|
| 포맷 | `cargo fmt --check` (+ `site/` · `crates/tasty-plugin-sdk-wasm/` 매니페스트 각각) | `format-check.yml` (ubuntu-latest) | main push · PR · 수동 |
| SemVer 가드 | `cargo test --locked --no-default-features --test api_baseline_0_7 --test changelog_unreleased --test cli_naming_count_drift` | `test.yml` 의 `semver-guards` (self-hosted Linux X64) | main push · 수동 |
| macOS 컴파일 | `cargo check --workspace --locked` | `crossplatform-check.yml` (self-hosted macOS) | main push · PR · 수동 |
| Windows lint + 단위테스트 | `cargo clippy --workspace --all-targets --locked` · `cargo test --workspace --lib --bins --locked --no-fail-fast` | `crossplatform-check.yml` (self-hosted Windows) | main push · PR · 수동 |
| headless 컴파일 · **전체 스위트** · lint | `cargo check --workspace --no-default-features --locked` · `cargo test --workspace --no-default-features --locked --no-fail-fast -- --skip <1 건>` · `cargo clippy --workspace --all-targets --no-default-features --locked` | `crossplatform-check.yml` 의 `check-headless` (self-hosted Linux X64) | main push · PR · 수동 |
| 파일 SLOC | `bash scripts/check-file-size.sh` | `complexity-check.yml` (self-hosted Linux X64) | main push(문서·site 제외) · PR · 수동 |
| plugin 버전 bump | `bash scripts/check-plugin-version-bump.sh --range <before> <after>` | `plugin-version-check.yml` (self-hosted Linux X64) | main push · PR — **둘 다 `crates/tasty-plugin-*/**` 가 바뀐 경우만** · 수동 |
| 공급망 | `cargo deny check` | `supply-chain-check.yml` | PR 전용 · 매주 월 09:00 UTC · 수동 → **schedule 만 실효** |

**문서만 담은 push 는 세 크로스플랫폼 잡을 발사하지 않는다.** `crossplatform-check.yml` 의
push 트리거에 `paths-ignore`(`docs/**` · `site/**` · `**/*.md`)가 걸려 있다. 컴파일 입력이
아닌 경로로 러너를 깨우지 않으려는 안전판인데, **문서 가드에는 정확히 거꾸로 작동한다** —
문서를 고치는 push 가 문서를 검사하는 채널을 돌리지 않는다. 소스를 함께 담은 push 에서는
걸러지지 않으므로 실무상 드물게 나타나지만, "문서만 고쳤으니 CI 가 봐 줄 것" 은 성립하지
않는다.

**자동 잡은 push 된 커밋만 본다.** 로컬에 쌓아 둔 커밋은 push 전까지 어느 자동 채널도
보지 않는다 — 채널이 배선돼 있다는 사실과 그 채널이 네 커밋을 봤다는 사실은 다르다.

**채널이 있다는 것은 그 잡이 초록이라는 뜻이 아니다.** 어떤 검사를 "CI 가 본다" 를 근거로
면제하려면 **그 잡이 최근에 실제로 통과했는지**까지 확인해라 — `gh run list` 로 최근 실행의
성패를 보고, 빨간 것이 있으면 `gh run view <id>`(실패 잡의 로그까지 보려면 `--log-failed`)로
어느 잡이 왜 죽었는지 확인한다. 이 조회는 코드 상태 판정이 아니라 실행 이력 조회다.
이 문서는 **배선**을 기술하고, 배선은 건강을 보장하지 않는다. 그리고 특정 시점의 적/녹은
여기 적지 않는다 — 적는 순간 낡기 시작하고, 낡은 시점 정보를 영구 서술로 읽는 것이 이
문서가 막으려는 실패 그 자체다.

**포맷 잡만 PR 을 함께 받는 이유**: `format-check.yml` 은 공용 `ubuntu-latest` 에서 돌아
러너 줄서기가 없다. 나머지 자동 잡은 self-hosted 러너를 쓰고, 특히 Linux X64 는 **한 대**를
`check-headless` · complexity-check · supply-chain-check · release/dist 빌드가 함께 쓴다 —
그래서 semver 가드는 PR 트리거를 붙이지 않았다(트리거가 잡마다 다른 것은 러너가 다르기
때문이지 중요도가 달라서가 아니다).

이 저장소는 PR 을 열지 않고 main 에 직접 push 한다. **"거의" 가 아니라 실측 0 이다** —
최근 200 run 의 이벤트 분포가 `push 48 · schedule 8 · workflow_dispatch 1`, `pull_request`
**0** 이다. 그래서 **PR 전용 트리거인 두 워크플로**(`complexity-check` ·
`supply-chain-check`)에서 PR 트리거는 장식이다 — `supply-chain-check` 는 주간 cron 이
있어 자동으로 돌지만, `complexity-check` 는 **등록 이래 run 이력이 0 건**이다
(워크플로는 `active` 로 등록돼 있다 — 조회 실패가 아니라 실제 0).

그러니 채널 판정에는 층이 둘이다: **① 그 명령이 그 테스트를 도는가**(아래 배치별 표)
**② 그 잡이 애초에 발화하는가**(트리거 열). ①만 보면 PR 트리거를 채널로 세게 된다.

## "안 돈다" 를 쓰기 전에 두 가지를 갈라라

**① 실행인가 컴파일인가.** 자동 잡의 clippy 는 `--all-targets` 라 `tests/*.rs` 를
**컴파일한다.** 그러므로 통합 테스트에 대해 두 문장이 **모두 거짓**이다 — "CI 가 이
테스트를 돌린다"(실행 채널이 없다) 와 "CI 가 컴파일조차 안 본다"(컴파일은 본다).
정확한 서술은 **"컴파일은 자동으로 검사되고, 실행은 수동"** 이다. 한쪽 거짓을 고치다
반대쪽 거짓을 심는 것이 이 축에서 가장 흔한 실패다.

**② 강제 수단이 워크플로 안에 있는가.** "워크플로가 안 돌린다" 는 "아무도 안 막는다" 가
**아니다.** clippy `deny`·`#[deny]` 어트리뷰트·pre-commit 훅·타입 시스템은 워크플로 밖에서
막는다. 실례로 복잡도 게이트는 축이 둘인데 채널이 갈린다.

| 복잡도 게이트의 축 | 강제 수단 | 실효 자동성 |
|---|---|---|
| 함수 cognitive | clippy `cognitive_complexity = "deny"` | **있다** — 자동 잡의 컴파일 단계에서 막힌다 |
| 파일 SLOC | `scripts/check-file-size.sh` (`complexity-check.yml`) | **있다** — main push 마다 돈다. 2026-09-04 까지는 PR 전용이라 **run 이력이 0 건**이었고, 그 사이 임계를 새로 넘은 26 건이 부채로 동결됐다([ADR-0131](../adr/0131-file-sloc-gate-needs-a-firing-trigger.md)) |

**③ 그 채널이 실패할 수 있는가.** 트리거가 붙어 잡이 도는 것과, 그 잡이 문제를 만났을 때
실제로 빨개지는 것은 다른 질문이다. 파일 SLOC 게이트가 그 예였다 — 트리거를 붙인 뒤에도
`tokei` 가 죽거나 빈 결과를 주면 스크립트가 **"게이트 통과" 를 출력하며 exit 0** 이었다.
측정 실패를 위반 없음으로 읽는 형태라, 러너 환경이 어긋나는 순간 그 채널은 영원히 초록이 된다.

지금은 종료코드가 **0(통과) / 1(위반) / 2(측정 실패)** 로 갈리고,
`tests/file_sloc_gate_fails_loudly.rs` 가 스텁 `tokei` 로 그 셋을 고정한다(통합 테스트라
`check-headless` 가 자동으로 돌린다). **채널의 존재 · 그 채널이 대상을 실제로 보는가 ·
그 채널이 실패할 수 있는가 — 셋은 따로 확인해야 한다.**

**②의 실물 하나 더 — 헤드리스 잡의 명명 `--skip`.** libtest 의 `--skip` 은 테스트 경로
전체에 대한 **부분일치**라, 이름이 사라지면 아무것도 안 잡고(과소, 무음) 그 문자열을 품는
이름이 새로 생기면 의도 없이 함께 빠진다(과대, 초록인데 커버리지 감소). 헤드리스 잡은 전체
스위트를 자동으로 도는 유일한 조합이라 여기서 빠지면 어디서도 안 돈다.
`tests/headless_skip_names_are_exact.rs` 가 **워크플로에서 skip 을 읽어와**(목록을 박아두면
만료된다) 각각이 **정확히 하나**의 식별자와 맞는지 본다 — 0 건도 2 건 이상도 실패다.
그 가드는 이름 집합을 소스 텍스트에서 얻으므로 매크로 생성 이름은 못 본다.

## 테스트는 **어디 있느냐**로 채널이 갈린다

위 표에서 가장 자주 오해되는 줄이다. 자동 잡이 돌리는 테스트 명령은 **조합마다 다르다** —
기본 조합은 좁혀져 있고(`--lib --bins`, 또는 `--test <이름>` 으로 이름 지목), 헤드리스
조합만 전체 스위트를 돌린다. 그래서 **같은 주제의 두 가드라도 파일이 어디 있느냐에 따라,
그리고 같은 파일이라도 조합에 따라 채널이 갈린다.**

| 테스트가 어디 있나 | 자동 **실행** | 자동 **컴파일** | 실례 |
|---|---|---|---|
| lib 유닛 테스트 (`src/`·`crates/*/src/` 안의 `#[cfg(test)] mod tests`) | **있다** — **두 조합 모두**가 실행한다(Windows 잡은 `--lib --bins`, 헤드리스 잡은 그 상위집합인 전체 스위트). 명령이 같아서가 아니라 둘 다 유닛 타깃을 포함해서다 | 있다 | `ui_font_size_tokens_are_integers_at_every_zoom` |
| 통합 테스트 (`tests/*.rs`) | **헤드리스 조합에만 있다** — `check-headless` 가 전체 스위트를 돌린다(`--skip` 1 건 제외). **기본 조합에는 없다** — Windows 잡은 `--lib --bins` 이고 `test.yml` 의 전체 스위트는 `workflow_dispatch` 전용 | **있다** — clippy `--all-targets` 가 타깃으로 잡는다 | `tests/design_token_adherence.rs` |
| SemVer 가드 3종 | **있다** — `semver-guards` 가 `--test` 로 이름을 지목한다 (main push) | 있다 | `api_baseline_0_7` · `changelog_unreleased` · `cli_naming_count_drift` |
| 포맷 | **있다** — `format-check.yml` (main push · PR) + pre-commit | — | `cargo fmt --check` |

### 조합에서 사라지는 이유는 대개 **파일 위치**다 (실측)

같은 파일이라도 조합에 따라 채널이 갈리는데, 그 갈림의 원인이 대부분 그 테스트 자신에게
있지 않다. 루트 bin 타깃의 유닛 테스트를 두 조합의 `-- --list` 로 갈라 보면(main
`d7dc4079` 실측) 기본 2039 / 헤드리스 1094 이고, **기본 조합에만 있는 949** 의 내역은:

| 헤드리스에서 사라지는 것을 무엇이 설명하나 | 수 | 비율 |
|---|---|---|
| **다른 파일의 `#[cfg(feature = "gui")] mod …;` 선언 아래에 있다** (위치 상속) | **909** | 95.8% |
| 같은 파일 안의 인라인 `#[cfg(all(test, feature = "gui"))] mod tests { … }` | 11 | 1.2% |
| 개별 `#[test]` 에 직접 붙은 `#[cfg(feature = "gui")]` | 29 | 3.1% |

상위 기여: `adapters::ui` 463 · `view` 177 · `gfx` 31 · `app::attach_client` 30. src 안의
gui 게이트된 `mod` 선언은 67 개다.

**세 행의 근거 강도가 다르다.** 아래 둘은 소스에서 게이트를 직접 찾아 붙인 **양성 귀속**이고
(40 건 전부가 셋 중 하나로 분류됐다 — 미분류 0), 위 909 는 **충분조건이지 유일 원인이 아니다**
— 그중 몇이 자기 항목에도 cfg 를 달고 있는지는 재지 않았다(모듈 게이트 하나로 이미 사라지므로
채널 판정에는 영향이 없지만, "cfg 를 떼면 살아난다" 를 이 수로 추론하면 틀린다).

**분류 자체가 텍스트 근사라는 한계도 함께 남긴다.** 게이트는 그 파일에도 부모에도 없이
**조부모의 다른 파일**에 있을 수 있고(`#[path]` 재지정·`cfg_attr`·매크로 생성 모듈도 같다),
그래서 줄 단위 grep 은 **양성만** 말할 수 있고 "없다" 는 말할 수 없다. 위 표는 조합별
`cargo test -- --list` 차집합(949)을 **모수로 고정한 뒤** 그 안에서 원인을 찾은 것이라,
총량은 실행이 정하고 분류만 텍스트가 한다 — 분류가 틀려도 949 는 안 움직인다.

**귀결이 둘이다.**

- **리팩터가 조합 노출을 바꾼다.** 파일을 게이트된 모듈 밖으로 `git mv` 하면 본문과 cfg 를
  한 줄도 안 고쳐도 그 파일의 테스트가 양 조합으로 늘고, 반대로 옮겨 넣으면 한 조합에서
  사라진다. 코드 리뷰에서 "이동뿐" 으로 보이는 변경이 채널을 바꾼다.
- **이동만이 아니라 생성도 그렇다.** 위 909 는 "게이트된 파일 수" 가 아니라 **"게이트된
  루트 아래에 있는 테스트 수"** 다 — `mod` 선언 하나가 서브트리 전체에 게이트를 물려주므로,
  그 아래에 파일을 **새로 만들기만 해도** 그 수가 조용히 는다. **가드를 어디에 둘지는 이
  축에서 먼저 결정한다**: 조합 대조가 목적인 가드를 `adapters::ui` 나 `view` 아래에 만들면
  그 가드는 태어날 때부터 한 조합에서만 돈다.
- **"텍스트 스캔 가드는 cfg 에 면역" 에는 선행 조건이 있다.** 런타임에 `.rs` 를 읽는
  가드는 컴파일된 심볼을 참조하지 않아 cfg 소거에 강하지만, 그건 **그 가드 파일 자체가
  게이트된 모듈 아래에 있지 않을 때** 이야기다. 위 909 가 그 조건이 얼마나 자주 깨지는지의
  값이다 — 스캔 로직이 통째로 컴파일에서 빠지면 스캔 대상이 디스크에 있어도 아무 일도
  일어나지 않는다.

**소스를 런타임에 스캔하는 드리프트 가드에게 "컴파일만 자동" 은 0 이다** — 스캔 로직이
컴파일돼도 실행되지 않으면 아무것도 보지 않는다. `tests/*.rs` 에 있는 가드는 이제
`check-headless` 에서 돌지만, 그 잡은 `paths-ignore` 로 **문서·site 만 담은 push 에서는
발사되지 않는다.** 레포 전체를 훑는 문서 가드는 하필 그 push 에서 위반을 가장 잘
들이므로, 그 구멍은 남아 있다. 그리고 그 잡은 러너 한 대에 묶여 있어(위 §러너 참고)
채널의 존재가 곧 즉시성은 아니다.

**두 방향 모두 틀릴 수 있다.** 통합 테스트에 "CI 가 강제한다" 를 붙이면 사실보다 강하고,
lib 유닛 테스트에서 그 서술을 지우면 사실보다 약하다. 어느 쪽이든 다음 사람의 판단을
망친다 — 채널을 서술할 때 "없다" 는 "있다" 만큼 확인이 필요하다.

## 사람이 돌리는 것 (자동 채널 없음)

| 검사 | 명령 | 누가 언제 |
|---|---|---|
| 전체 스위트 | `cargo test --workspace --locked` | 병합 후 main 에서 conductor 1회. `test.yml` 의 `test-linux-x64` 잡을 수동 실행해도 같다 |
| Linux x64 gui 컴파일 | (위 전체 스위트에 포함) | 상동 — 이것만 보는 자동 잡은 없다 |
| 기본 조합 clippy (Linux) | `cargo clippy --workspace --all-targets --locked` | 각 작업 lane. CI 에서 이 조합을 보는 것은 Windows 잡뿐이다 |
| dist 산출물 빌드 | `scripts/build-*.sh` | `build-check.yml` 수동 실행 |

**전체 스위트를 자동화하지 않는 이유**는 `test.yml` 헤더에 있다 — 실측 274.5s 중
222.4s 가 GUI 인스턴스를 띄우는 11개라 러너 GPU 가용성에 따라 그대로 flaky 가 된다.
e2e 하네스가 헤드리스로 뜨게 되면 그 비용이 사라지고 자동화가 훨씬 싸진다.

## 로컬 훅이 앞당겨 주는 것

훅은 **옵트인**이다(`git config core.hooksPath .githooks` 1회) — 설치하지 않아도
커밋·push 는 된다. 그래서 훅은 "게이트" 가 아니라 CI 게이트의 **빠른 피드백**으로
읽는다. 상세는 [git-hooks](git-hooks.md).

| 훅 | 검사 | CI 에도 있는가 |
|---|---|---|
| pre-commit | `cargo fmt --check` | ✅ `format-check.yml` |
| pre-commit | mod/use 선언 순서 · `egui::Window` 직접 사용 · `println!`/`dbg!` | ❌ 훅에만 있다 |
| pre-commit | plugin 산출물이 바뀌었는데 매니페스트 `version` 이 그대로 (P.1) | ✅ `plugin-version-check.yml` — **같은 스크립트를 부른다**. 훅은 index 를 `main` 과의 merge-base 와 비교하고(amend·rebase 에 안 흔들리게), CI 는 밀어넣은 범위의 두 끝점을 비교한다 |
| pre-commit | 주석 없는 `let _ =` (C.6) | 부분 — 전수판 `tests/let_underscore_documented.rs` 가 훅의 상위집합이고, 그 12 건은 `check-headless` 에서 자동 실행된다(기본 조합 잡은 `--lib --bins` 라 못 본다). **자동 잡의 clippy 는 `let_underscore_must_use`(warn)로 그 자리를 표면화하지만 이 규칙을 집행하지는 않는다** — 주석을 못 읽어 사유가 달린 정상 코드까지 세는 명부이고, `-D warnings` 가 없어 빌드도 막지 않는다([error-handling](error-handling.md)) |
| pre-push | `cargo clippy --workspace --all-targets -- -D clippy::correctness` | 부분 — Windows 잡의 clippy 는 `--locked` 를 쓰고 correctness deny 를 걸지 않는다 |
| pre-push | `cargo check --workspace --all-targets` | 부분 — CI 는 `--all-targets` 없이 macOS 에서 본다 |
| pre-push | `cargo check --no-default-features` | ✅ `crossplatform-check.yml` |

즉 **훅에만 있는 검사가 셋**이다(mod/use 순서 · `egui::Window` · `println!`/`dbg!`).
훅을 설치하지 않은 체크아웃이나 `--no-verify` 커밋은 그 셋을 통과한다 — 이것들은 diff
기반이라 CI 로 옮기려면 "무엇을 신규로 볼 것인가" 를 다시 정의해야 해서 지금은 훅에
남아 있다. `let _ =` 만 성격이 다르다: 전수판이 이미 있고 diff 기반이 아니므로, 위
"전체 스위트" 에 자동 채널이 생기면 그 순간 함께 자동화된다.

## 이 문서와 레포가 어긋나지 않게 하는 것

문서가 "CI 가 잡아 준다" 고 적어 두고 실제로는 아무것도 돌지 않는 상태가 이 저장소에서
열여덟 자리에 쌓여 있었다. 컴파일도 통과하고, 틀렸다는 사실은 워크플로 파일을 직접
열어야만 보인다 — 그래서 리뷰로는 걸러지지 않는다.

`tests/ci_channel_claims_match_workflows.rs` 가 그 형태를 막는다(이 가드 자신도 통합
테스트라 자동 실행은 `check-headless` 잡에서만 일어난다 — 위 규칙이 자기에게도 그대로
적용된다). 문서를 문서로 검사하지 않고 **워크플로에서 자동 트리거를 가진 잡을 읽는다.**
네 축이 있다.

- **명령을 인용한 형태** — **기본 조합**의 자동 잡이 전체 스위트를 돌리는지 보고,
  돌리지 않으면 그것을 강제 장치로 서술한 자리를 전부 짚는다. 문서가 인용하는
  `cargo test --workspace` 는 기본 조합의 명령이라, 헤드리스 잡이 전체 스위트를 돌리는
  것과 섞어 보면 이 축이 통째로 잠잠해진다.
- **명령을 적지 않는 형태** — 자동 잡이 `--test` 로 **이름을 지목한** 통합 테스트 목록을
  워크플로에서 읽어, 그 밖의 `tests/*.rs` 를 집행 장치로 부르는 서술을 짚는다. 좁히지
  않은 자동 잡이 하나라도 있으면 이 축은 스스로 잠잠해진다.
- **반대 방향** — 자동 잡이 lib 유닛 테스트를 돌리는 동안, `src/` 안의 유닛 테스트를 두고
  부재를 적은 서술을 짚는다(사실보다 약하다). 이 전제도 상수가 아니라 워크플로에서 읽는다.
- **조합** — 통합 테스트를 지목하면서 자동 채널의 부재를 적었는데 그 테스트가 실제로는
  도는 자리를 짚는다. 판정 단위는 **그 테스트가 자동으로 도는 조합의 수**다: 0 이면 부재
  서술이 참, 1 이면 어느 조합인지 함께 적어야 참, 2 면 어떻게 적어도 거짓이다. 조합별로
  빌드되는지(`required-features`), 그 호출이 통합 타깃을 만드는지, `--skip` 이 그 타깃을
  통째로 걷어내는지를 함께 본다.

부재를 함께 적은 문장(`수동 전용` 등)은 정당한 서술로 통과시키므로 등록 절차가 없다.
조합이 하나뿐인 채널은 그 조합을 함께 적어야 통과한다(`check-headless 잡에서만` 등).
목록을 가드 안에 복사해 두지 않고 워크플로에서 런타임에 읽으므로, 전체 스위트가 자동
채널에 올라가거나 `--test` 열거·`--skip` 이 바뀌는 날 이 가드는 스스로 따라간다 — 그때
문서를 손으로 다시 훑지 않아도 된다.

## 파생 문서는 채널을 다시 쓰지 않는다

이번 스윕에서 실제로 어긋나 있던 것은 **이 문서가 아니라 채널을 따로 서술한 파생
문서들**이었다(복잡도 게이트의 두 축을 한 문장에 묶어 "자동 차단" 이라 적은 자리들).
정본 하나를 고쳐도 파생이 자기 문장을 들고 있으면 다시 어긋난다.

그래서 규칙은 **다시 서술하지 말고 여기를 링크한다** 이다. 서술이 꼭 필요하면 그 문장이
**실행/컴파일**과 **축 단위 실효성** 둘 다에서 이 문서와 같은 말을 하는지 확인한다.

## 관련

- [git-hooks](git-hooks.md) — 훅 각 검사의 내용과 설치
- [clippy-policy](clippy-policy.md) · [complexity-gate](complexity-gate.md) — lint 정책
- [release-runners](release-runners.md) — self-hosted 러너 구성
