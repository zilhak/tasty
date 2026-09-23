# Git Hooks

`.githooks/`에는 커밋, push, merge 전에 실행하는 검사 세 가지가 있다. clone 후 `./scripts/dev-setup.sh`를 실행하면 설치된다. `git config core.hooksPath .githooks`로 직접 설정할 수도 있다. 셸 스크립트를 커밋하려면 PATH에 `shellcheck`가 필요하다.

CI에서도 실행하는 검사와 로컬 훅에서만 실행하는 검사는 [ci-gates](ci-gates.md)에서 확인한다.

## pre-commit

코드 형식과 추가된 코드, 플러그인 버전, 문서 참조를 확인한다. 검사 시간은 변경 범위와 빌드 캐시에 따라 달라진다.

| ID | 검사 | 범위와 수정 방법 |
|----|------|------------------|
| M.1 | merge 커밋 차단 | 진행 중인 merge를 취소하고 rebase 후 fast-forward merge한다. |
| A.1 | `mod`와 `use` 선언 순서 | 커밋 대상 Rust 파일의 선언 영역에서 `mod`를 `use`보다 앞에 둔다. |
| A.2 | `cargo fmt --check` | Rust 파일이 변경되면 워크스페이스를 검사한다. 제외된 크레이트는 해당 경로가 변경됐을 때 별도로 검사한다. |
| A.3 | 셸 스크립트 정적 검사 | `scripts/check-shell-assets.sh`에 커밋 대상 파일을 전달한다. `shellcheck`가 없거나 warning 이상의 문제가 있으면 중단한다. |
| C.6 | `let _ =`의 사유 주석 | 추가된 코드에서 값을 무시하는 이유를 같은 줄, 윗줄 또는 다음 줄의 주석으로 설명한다. 윗줄을 찾을 때 빈 줄과 속성은 건너뛴다. |
| C.8 | 색상 검사 이전 안내 | 함수는 호출되지만 별도 검사는 하지 않는다. 실제 검사는 Clippy로 이전됐다. [clippy-policy](clippy-policy.md) 참고. |
| C.9 | `egui::Window` 직접 사용 | 추가된 코드에서는 PopupManager/PopupDef를 사용한다. 예외는 [popup-implementation](popup-implementation.md)에 있다. |
| C.11 | `println!`와 `eprintln!` 사용 | 추가된 로그는 `tracing`으로 출력한다. CLI 등 표준 출력이 필요한 파일은 제외한다. |
| C.12 | `dbg!` 사용 | 추가된 코드에서 디버그 매크로를 제거한다. |
| P.1 | 플러그인 버전 변경 | `scripts/check-plugin-version-bump.sh`로 staged 내용과 `main`의 공통 조상 커밋을 비교한다. 공통 조상을 찾지 못하면 HEAD를 사용한다. |
| T.1 | 로컬 전용 문서 참조 | `cargo test -q -p tasty-doc-guards --test no_todo_file_citation`으로 저장소 전체를 검사한다. 커밋 대상이 아닌 파일도 포함된다. |
| W.1 | CHANGELOG 누락 안내 | 사용자 기능 관련 선언이 바뀌었지만 CHANGELOG가 변경되지 않았으면 안내한다. 커밋은 막지 않는다. |
| W.2 | 새 파일에 필요한 검사 안내 | 새 파일의 종류에 따라 테스트 명령을 안내한다. 커밋은 막지 않는다. |

C.6, C.9, C.11, C.12는 staged diff에 추가된 코드만 확인한다. 전체 문서·소스 검사는 `cargo test -p tasty-doc-guards`로 실행한다. 번역 키 정합 검사는 별도이며 [i18n](i18n.md)에 실행 방법이 있다.

플러그인 버전 검사는 파일 경로로 대상을 미리 제한하지 않는다. 공용 라이브러리나 vendor 의존성 변경도 플러그인에 영향을 줄 수 있기 때문이다. 검사 스크립트가 실제 의존 관계를 확인한다.

## pre-push

먼저 push할 커밋의 플러그인 버전과 파일 수를 검사한다. 둘 중 하나라도 실패하면 컴파일을 시작하지 않고 중단한다. 두 검사에 통과하면 아래 순서대로 빌드와 문서를 검사한다. 빌드 검사 중 하나가 실패해도 나머지는 실행하여 오류를 한 번에 확인할 수 있다.

| ID | 검사 |
|----|------|
| B.9 | `scripts/check-plugin-version-bump.sh --range <원격 커밋> <로컬 커밋>`으로 원격에 게시된 플러그인 버전과 비교한다. |
| B.10 | `scripts/check-population-freshness.sh --rev <로컬 커밋>`으로 파일 수와 `crates/tasty-doc-guards/src/floored_walk.rs`의 검사 기준이 일치하는지 확인한다. |
| B.5 | `cargo check --workspace --all-targets` |
| B.8 | `cargo check --workspace --release --locked` |
| B.6 | `cargo check --no-default-features` |
| B.4 | `cargo clippy --workspace --all-targets -- -D clippy::correctness` |
| B.7 | `cargo test -p tasty-doc-guards` |

B.9와 B.10은 Git이 전달한 ref별 커밋을 검사한다. 파일 수 기준을 수정했다면 커밋한 뒤 다시 push해야 한다. B.9에서 원격 커밋을 로컬에서 찾지 못하면 `git fetch`가 필요하다. 새 ref는 비교할 원격 버전이 없으므로 B.9를 생략하고, 삭제할 ref는 두 검사를 모두 생략한다. ref별 검사·생략 수는 로그에 남는다.

훅을 직접 실행하면 Git의 ref 정보가 없어 B.9와 B.10을 생략했다고 표시한다. 두 검사는 표의 명령으로 따로 실행할 수 있다. push할 ref가 없으면 빌드도 생략한다.

빌드·문서 검사는 현재 작업 트리에서 실행한다. 따라서 다른 커밋을 지정해 push하거나 미커밋 변경이 있는 경우에는 push할 커밋의 빌드를 검증한 것으로 볼 수 없다.

### 실행 시간과 로그

각 검사의 시작, 성공·실패, 소요 시간을 표시하고 마지막에 전체 시간을 요약한다. 최초 빌드나 소스 변경 후에는 분 단위로 걸릴 수 있으며, 캐시가 있으면 짧아진다. 개발 빌드, release 빌드, headless 빌드는 설정이 달라 각각 검사한다.

로그는 `git rev-parse --git-path hook-logs`가 가리키는 디렉토리 아래 실행별 폴더에 저장한다. `B.4.log`처럼 검사 ID별 전체 출력과 `summary.log`를 확인할 수 있다. 실패하면 마지막 60줄과 전체 로그 경로를 표시한다. 이전 로그는 자동으로 삭제하지 않으며 필요 없으면 해당 실행 폴더를 지워도 된다.

검사 결과는 출력의 마지막 명령이 아니라 검사 명령 자체의 종료 코드로 결정한다. 로그 저장이나 요약 출력이 성공해도 실패한 검사가 통과로 바뀌지 않는다.

## pre-merge-commit

fast-forward가 아닌 merge 커밋 생성을 막는다. 차단되면 `git merge --abort`로 현재 merge를 취소한다. 작업 브랜치에서 `git rebase <대상-브랜치>`를 실행한 뒤 대상 브랜치로 전환해 `git merge --ff-only <작업-브랜치>`로 반영한다.

충돌한 merge는 이 훅 대신 충돌 해결 후 실행하는 `git commit`의 pre-commit M.1에서 차단한다. 충돌을 해결할 때는 양쪽 변경이 필요한 이유를 확인하고 내용을 합친다.

## 검사 추가와 검증

pre-commit에 검사를 추가할 때는 `check_*` 함수를 만들고 `CHECKS` 배열에 등록한다. 실패는 `fail` 또는 `FAIL=1`로 기록한다. 안내만 필요한 경우에는 `yellow`로 출력하고 실패 상태를 바꾸지 않는다.

`crates/tasty-doc-guards/tests/githooks_are_pinned.rs`는 훅 파일, 검사 ID, 이 문서의 표, pre-commit 실행 목록이 일치하는지 확인한다. 검사 ID는 훅의 구분선 바로 아래 주석에서 읽는다. 수정 후에는 `cargo test --locked -p tasty-doc-guards --test githooks_are_pinned`를 실행한다.
