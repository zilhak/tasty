# Git Hooks

`.githooks/` 에 pre-commit / pre-push 훅이 있다. clone 직후 **1회 셋업**:

```bash
./scripts/dev-setup.sh   # core.hooksPath 를 .githooks/ 로 설정 (멱등)
```

안 하면 hook 이 안 돈다(CI 가 같은 검사를 돌리므로 결국 잡히지만 로컬 피드백을 위해 권장). 긴급 우회: `git commit --no-verify` / `git push --no-verify`.

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
| B.4 | `cargo clippy --workspace --all-targets -- -D clippy::correctness` |

clippy 의 `style`/`pedantic` 은 warning 으로만(error 승격 안 함 — false positive 노이즈 방지).

## 새 검사 추가

`.githooks/pre-commit` 의 `check_*` 함수 패턴: staged 파일 순회 → 위반 시 `fail`(메시지+FAIL=1) → 마지막 exit code 결정. 1초 이상 느려지면 pre-push 로 옮긴다.
