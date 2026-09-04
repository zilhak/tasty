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
| Windows lint + 단위테스트 | `cargo clippy --workspace --all-targets --locked` · `cargo test --workspace --lib --bins --locked` | `crossplatform-check.yml` (self-hosted Windows) | main push · PR · 수동 |
| headless 컴파일 · 단위테스트 · lint | `cargo check --workspace --no-default-features --locked` · `cargo test --workspace --lib --bins --no-default-features --locked` · `cargo clippy --workspace --all-targets --no-default-features --locked` | `crossplatform-check.yml` 의 `check-headless` (self-hosted Linux X64) | main push · PR · 수동 |
| 파일 SLOC | `bash scripts/check-file-size.sh` | `complexity-check.yml` | **PR 전용** · 수동 |
| 공급망 | `cargo deny check` | `supply-chain-check.yml` | **PR 전용** · 매주 월 09:00 UTC · 수동 |

이 저장소는 PR 을 거의 열지 않고 main 에 직접 push 한다. 그래서 **PR 전용 트리거인
두 워크플로**(`complexity-check` · `supply-chain-check`)는 사실상 수동/주간 채널만
살아 있다고 보는 편이 맞다 — `supply-chain-check` 는 주간 cron 이 있어 자동으로
돌지만, `complexity-check` 는 PR 이 없으면 영영 돌지 않는다.

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
| pre-commit | 주석 없는 `let _ =` (C.6) | 부분 — 전수판 `tests/let_underscore_documented.rs` 가 훅의 상위집합이지만, 그것이 도는 `cargo test --workspace` 에 자동 채널이 없다 |
| pre-push | `cargo clippy --workspace --all-targets -- -D clippy::correctness` | 부분 — Windows 잡의 clippy 는 `--locked` 를 쓰고 correctness deny 를 걸지 않는다 |
| pre-push | `cargo check --workspace --all-targets` | 부분 — CI 는 `--all-targets` 없이 macOS 에서 본다 |
| pre-push | `cargo check --no-default-features` | ✅ `crossplatform-check.yml` |

즉 **훅에만 있는 검사가 셋**이다(mod/use 순서 · `egui::Window` · `println!`/`dbg!`).
훅을 설치하지 않은 체크아웃이나 `--no-verify` 커밋은 그 셋을 통과한다 — 이것들은 diff
기반이라 CI 로 옮기려면 "무엇을 신규로 볼 것인가" 를 다시 정의해야 해서 지금은 훅에
남아 있다. `let _ =` 만 성격이 다르다: 전수판이 이미 있고 diff 기반이 아니므로, 위
"전체 스위트" 에 자동 채널이 생기면 그 순간 함께 자동화된다.

## 관련

- [git-hooks](git-hooks.md) — 훅 각 검사의 내용과 설치
- [clippy-policy](clippy-policy.md) · [complexity-gate](complexity-gate.md) — lint 정책
- [release-runners](release-runners.md) — self-hosted 러너 구성
