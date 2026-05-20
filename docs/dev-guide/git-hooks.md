# Git Hooks

`.githooks/` 디렉토리에 pre-commit / pre-push 훅이 있다. 새로 clone 한 직후
한 번만 설치한다:

```bash
git config core.hooksPath .githooks
```

이후 `git commit` / `git push` 시 자동으로 실행된다. 긴급 우회는 `--no-verify`.

## pre-commit (빠른 검사, 1-3초)

staged 된 `.rs` 파일만 검사 (변경 없는 파일은 건너뜀).

| ID | 검사 | 목적 |
|----|------|------|
| A.1 | top-level 에서 `use` 뒤에 `mod` 가 나오는지 | 선언 순서 깨짐 방지 |
| A.2 | `cargo fmt --check` | rustfmt 포맷 강제 |
| C.6 | 주석 없는 `let _ = ` | 왜 무시하는지 흔적 강제 |
| C.8 | `Color32::from_rgb` / `Rgba::from_rgb` 직접 사용 | Theme 시스템 강제 |
| C.9 | `egui::Window::` 직접 사용 | PopupManager 강제 |
| C.11 | `println!` / `eprintln!` | `tracing::*` 강제 |
| C.12 | `dbg!` | release leak 방지 |

### 예외 (화이트리스트)

- **C.8 색상**: `src/theme.rs`, `crates/tasty-core/src/theme/` — Theme 정의 자체.
- **C.9 egui::Window**: `src/ui/popup/`, `src/ui/popup_defs.rs` — popup 인프라.
- **C.11 println**: `src/main.rs`, `src/cli/`, `src/boot/cli_routing.rs` — CLI 출력
  자체가 본질.

## pre-push (무거운 검사, 수십초)

| ID | 검사 | 목적 |
|----|------|------|
| B.5 | `cargo check --workspace --all-targets` | 컴파일 + test 빌드 검증 |
| B.4 | `cargo clippy --workspace --all-targets -- -D clippy::correctness` | 진짜 버그 lint |

clippy 의 `style` / `pedantic` 등은 warning 으로만 띄우고 error 승격은 안 함 —
false positive 노이즈 방지.

## 우회

긴급 시:
- `git commit --no-verify` — pre-commit 우회
- `git push --no-verify` — pre-push 우회

다만 CI 가 같은 검사를 돌리므로 이후 fix 필요.

## 새 검사 추가

`.githooks/pre-commit` 의 `check_*` 함수 패턴을 따라 추가. 각 함수는:
1. staged 파일 목록 (`$STAGED_RS`) 순회
2. 위반 검출 시 `fail` 함수 호출 (FAIL=1 설정 + 메시지 출력)
3. 마지막에 `FAIL` 체크해서 exit code 결정

새 검사가 느려지면 (1초 이상) `pre-push` 로 옮긴다.
