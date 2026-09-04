# Git Hooks

`.githooks/` 에 pre-commit / pre-push / pre-merge-commit 훅이 있다. clone 직후 **1회 셋업**:

```bash
./scripts/dev-setup.sh   # core.hooksPath 를 .githooks/ 로 설정 (멱등)
```

안 하면 hook 이 안 돈다. 긴급 우회: `git commit --no-verify` / `git push --no-verify`. (merge 차단 우회는 아래 pre-merge-commit 절 참고)

**어떤 검사가 CI 에도 있고 어떤 검사가 훅에만 있는지는 [ci-gates](ci-gates.md) 의 표가 정본이다.** 훅을 설치하지 않거나 우회하면 훅 전용 검사(mod/use 선언 순서 · `egui::Window` 직접 사용 · `println!`/`dbg!`)는 아무 데서도 잡히지 않는다. `let _ =` 는 전수판(C.6 아래 참고)이 따로 있지만 그것이 도는 `cargo test --workspace` 에도 자동 채널은 없다.

## pre-commit (1–3초)

A.1/A.2 는 파일 전체, C.* 는 **staged diff 의 추가 라인만** 검사(기존 코드 통과, 신규 위반만 차단). 화이트리스트·정확한 검출 로직은 `.githooks/pre-commit` 가 SoT.

| ID | 검사 | 목적 |
|----|------|------|
| A.1 | top-level 선언 영역에서 `mod` 가 `use` 뒤에 나오는지 | 선언 순서 |
| A.2 | `cargo fmt --check` | rustfmt 강제 |
| C.6 | 주석 없는 `let _ =` | 왜 무시하는지 흔적 강제 (전수판은 `tests/let_underscore_documented.rs` — 아래 참고) |
| C.9 | `egui::Window::` 직접 사용 | PopupManager 강제 ([popup-implementation](popup-implementation.md)) |
| C.11 | `println!`/`eprintln!` | `tracing::*` 강제 (예외: CLI 출력 — `crates/tasty-cli/*`, `src/boot/cli_routing.rs`) |
| C.12 | `dbg!` | release leak 방지 |
| W.1 | 사용자 표면 선언 파일(`crates/tasty-ipc/src/method_meta.rs` · `crates/tasty-cli/src/commands/` · `crates/tasty-plugin-*/tasty-plugin.toml`)이 staged 인데 `CHANGELOG.md` 는 아님 | CHANGELOG 누락 상기 — **경고만, 커밋은 통과** |

> W.1 이 경고에 그치는 이유: 그 파일을 만졌다고 반드시 사용자 표면이 바뀌는 것은 아니다(내부 refactor, 도움말 오타, 매니페스트 버전 bump). 하드 실패로 만들면 무해한 커밋마다 `--no-verify` 를 쓰게 되고 훅 전체가 무력화된다 — 판단은 사람이 한다.
>
> 색상 하드코딩(옛 C.8)은 pre-commit 에서 빠지고 **clippy `disallowed-methods`** 로 이관됐다 — `#[allow]` 와 path 예외를 정확히 인식한다([color-policy](color-policy.md), [clippy-policy](clippy-policy.md)).
>
> C.6 은 staged diff 만 보므로 **기존 코드의 위반은 못 잡는다**. 전수 검사는
> `tests/let_underscore_documented.rs` 가 한다 — 훅이 인정하는 세 형태(같은 줄·윗줄·다음 줄)를
> 모두 포함하고 조금 더 넓어(빈 줄·속성 건너뛰기, 멀티라인 문장 내부), 훅이 통과시킨 코드를 전수
> 검사가 떨어뜨리는 방향은 생기지 않는다. 다만 그 전수판이 도는 `cargo test --workspace` 는
> **자동 채널이 없다**(병합 후 main 에서 사람이 돌린다 — [ci-gates](ci-gates.md)). 판정 규약은
> [error-handling](error-handling.md) "주석 위치".
>
> i18n(번역 키 정합·자연어 하드코딩)은 pre-commit 검사가 아니다 — 소스 전체를 읽어야 해서 hook 예산(1–3초)을 넘는다. `tests/i18n_key_parity.rs`·`tests/no_hardcoded_ui_strings.rs` 가 집행하는데, 그 둘이 도는 `cargo test --workspace` 는 **자동 채널이 없다**(병합 후 main 에서 사람이 돌린다 — [ci-gates](ci-gates.md)). 로컬 확인 명령은 [i18n](i18n.md) "강제 테스트" 절.

## pre-push (수십초)

| ID | 검사 |
|----|------|
| B.5 | `cargo check --workspace --all-targets` |
| B.6 | `cargo check --no-default-features` (headless 빌드 — `gui` feature 없이 컴파일) |
| B.4 | `cargo clippy --workspace --all-targets -- -D clippy::correctness` |

clippy 의 `style`/`pedantic` 은 warning 으로만(error 승격 안 함 — false positive 노이즈 방지).

## pre-merge-commit (즉시)

non-fast-forward merge 를 차단한다. 이 훅은 merge 가 **merge 커밋을 만들 때만**, 즉 non-ff merge 일 때만 실행된다 — ff merge 는 커밋을 만들지 않으므로 훅이 돌지 않고 통과한다. 따라서 "ff 가 아닌 merge 시도" 만 정확히 잡는다.

| 정책 | 내용 |
|------|------|
| 권장 | merge 는 ff-merge(fast-forward) 를 권장 |
| 일반 merge 허용 조건 | rebase 또는 ff-merge 가 **정말로 불가능한** 경우에만 |

차단 시점엔 git 이 이미 merge 를 **진행 중 상태(MERGE_HEAD)** 로 남긴다. 따라서 우회는 두 갈래다:

- **되돌리기**: `git merge --abort` → rebase + `git merge --ff-only` 로 재시도.
- **그대로 마무리(선택)**: ff/rebase 가 정말 불가능함을 확인한 뒤에만 `git commit --no-verify` 로 진행 중인 merge 를 완료. (처음부터 선제 우회하려면 깨끗한 상태에서 `git merge --no-verify <브랜치>`)

rebase 충돌 해결 시 충돌 마커만 지우지 말고, 각 충돌이 어떤 변경끼리 부딪힌 것인지·양쪽 의도가 무엇인지 확인하고 '맞는' 합본을 판단한 뒤 진행한다(애매하면 `git rebase --abort` 후 원인 분석). conductor 의 merge 정책과 일치한다.

## 새 검사 추가

`.githooks/pre-commit` 의 `check_*` 함수 패턴: staged 파일 순회 → 위반 시 `fail`(메시지+FAIL=1) → 마지막 exit code 결정. 1초 이상 느려지면 pre-push 로 옮긴다.

경고만 내는 검사(W.*)는 `fail` 을 쓰지 않고 `yellow` 로 출력만 한다 — `FAIL` 을 건드리지 않으므로 exit code 에 영향이 없다.
