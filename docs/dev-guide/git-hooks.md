# Git Hooks

`.githooks/` 에 pre-commit / pre-push / pre-merge-commit 훅이 있다. clone 직후 **1회 셋업**:

```bash
./scripts/dev-setup.sh   # core.hooksPath 를 .githooks/ 로 설정 (멱등)
```

안 하면 hook 이 안 돈다(CI 가 같은 검사를 돌리므로 결국 잡히지만 로컬 피드백을 위해 권장). 긴급 우회: `git commit --no-verify` / `git push --no-verify`. (merge 차단 우회는 아래 pre-merge-commit 절 참고)

## pre-commit (1–3초)

A.1/A.2 는 파일 전체, C.* 는 **staged diff 의 추가 라인만** 검사(기존 코드 통과, 신규 위반만 차단). 화이트리스트·정확한 검출 로직은 `.githooks/pre-commit` 가 SoT.

| ID | 검사 | 목적 |
|----|------|------|
| A.1 | top-level 선언 영역에서 `mod` 가 `use` 뒤에 나오는지 | 선언 순서 |
| A.2 | `cargo fmt --check` | rustfmt 강제 |
| C.6 | 주석 없는 `let _ =` | 왜 무시하는지 흔적 강제 |
| C.9 | `egui::Window::` 직접 사용 | PopupManager 강제 ([popup-implementation](popup-implementation.md)) |
| C.11 | `println!`/`eprintln!` | `tracing::*` 강제 (예외: CLI 출력 — `crates/tasty-cli/*`, `src/boot/cli_routing.rs`) |
| C.12 | `dbg!` | release leak 방지 |

> 색상 하드코딩(옛 C.8)은 pre-commit 에서 빠지고 **clippy `disallowed-methods`** 로 이관됐다 — `#[allow]` 와 path 예외를 정확히 인식한다([color-policy](color-policy.md), [clippy-policy](clippy-policy.md)).

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
