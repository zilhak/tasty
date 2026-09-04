# 복잡도 게이트

함수 cognitive 복잡도와 파일 SLOC 이 임계를 넘는 **신규/증가분**을 차단한다. 기존 초과분은 위치 단위 예외로 동결(grandfather)하고, 리팩터로 줄면 예외를 지워 래칫을 조인다.

**두 축의 강제 채널은 다르다** — cognitive 는 clippy `deny` 라 자동 잡에서 컴파일 자체가 막히고, 파일 SLOC 은 전용 워크플로가 main push 마다 돌린다(2026-09-04 이전에는 PR 전용이라 한 번도 발화하지 않았다 — [ADR-0131](../adr/0131-file-sloc-gate-needs-a-firing-trigger.md)). 채널 정본은 [ci-gates](ci-gates.md). 결정 근거·대안은 [ADR-0037](../adr/0037-complexity-gate.md).

## 무엇을·도구·임계값

| 축 | 도구 | 임계값 | 동결 위치 |
|----|------|--------|-----------|
| **함수 cognitive** | clippy 내장 `cognitive_complexity`(deny) | **20** | 함수 `#[allow]` + `// complexity-exempt:` (현재 35곳) |
| **파일 SLOC** | `tokei` + `scripts/check-file-size.sh` | code SLOC **1000** | `.complexity-file-allowlist` (현재 44개 — 도입 시 동결 18 + 채널 부재로 쌓인 부채 26) |

- 카운트 기준: `grep -rn 'allow(clippy::cognitive_complexity)'` 로 센 **전체** 위치 수. `// complexity-exempt:` 태그는 감사(grep) 가능성을 위한 필수 컨벤션이라, `#[allow(clippy::cognitive_complexity)]`가 있는데 태그가 없는 레거시가 발견되면 그 자리에서 태그를 붙여 카운트에 편입한다(둘을 별도 숫자로 두지 않는다).

- cognitive 임계 20 은 외부 도구 rca cognitive ≈ 50 등가다. clippy 는 egui 즉시모드 draw 의 `ui.horizontal(|ui|{…})` 클로저를 부모에 합산하지 않아 구조적 draw 를 자동 배제 → 임계 초과 baseline 이 거의 순수 로직 함수라 신호가 깨끗하다.
- clippy 에는 파일 SLOC lint 가 없어 파일 축은 tokei 로 별도 강제한다.

## 예외 컨벤션

### 함수 (cognitive)

정당하게 임계를 넘는 함수에 부착한다:

```rust
#[allow(clippy::cognitive_complexity)] // complexity-exempt: <왜 분해가 부적절/무의미한지 구체적으로>
fn draw_something(...) { ... }
```

- 사유는 **왜 분해가 부적절한지**를 적는다. 빈 사유·"TODO"·"나중에"는 금지.
- 클로저가 임계를 넘겨 리포트되면 **enclosing 함수**에 부착한다(lint level 은 렉시컬 스코프라 내부 클로저까지 suppress 된다).
- 전형적 정당 초과: egui 즉시모드 draw(클로저 중첩이 구조적), 평면 match 디스패치(arm 많으나 중첩 얕음), 반복 assert 테스트(clippy 과대계상 — rca 는 0). 진짜 리팩터 대상이라도 게이트 도입 시엔 분해하지 않고 `// complexity-exempt: 리팩터 후보 …` 로 동결한다(리팩터는 별건).

### 파일 (SLOC)

- 정당하게 큰 파일은 `.complexity-file-allowlist` 에 레포 상대경로(슬래시)를 한 줄 추가한다.
- **allowlist 안에 블록이 둘이다.** 위 18 건은 게이트 도입(2026-07-06) 시점의 기존 대형 파일이고, 아래 26 건은 게이트가 한 번도 실행되지 않은 60 일 동안 새로 임계를 넘은 것이다 — **정당화된 예외가 아니라 부채 대장**이다(도입 시점에 이미 초과였던 것은 그 26 중 0 건). 새 항목은 위 블록에 넣고, 아래 블록은 래칫으로 **지우기만 한다**. 근거는 [ADR-0131](../adr/0131-file-sloc-gate-needs-a-firing-trigger.md).
- 테스트 모듈(`tests.rs`·`*_tests.rs`·`tests/`)·생성/전사 코드(`*generated*`, `design-tokens/generated/`)는 스크립트 `skip()` 이 게이트에서 아예 제외하므로 allowlist 등록이 불요하다.

## 로컬 재현

```bash
# 함수 cognitive — 신규 초과 함수가 있으면 error 로 컴파일 실패.
cargo clippy --workspace --all-targets

# 파일 SLOC — allowlist 밖 대형 파일이 있으면 목록 출력 후 exit 1.
bash scripts/check-file-size.sh
```

`check-file-size.sh` 는 `tokei` 와 `python`(JSON 파싱)을 요구한다. 러너에 없으면 설치 가드가 exit 2 로 안내한다.

## baseline 갱신

- **조이기(권장)**: 리팩터로 함수가 20 이하 / 파일이 1000 이하로 내려가면, 해당 `#[allow]` 줄 또는 allowlist 항목을 **삭제**한다. 래칫이 한 칸 조여진다.
- **느슨하게(회피)**: 정당한 새 예외를 등록할 때는 구체 사유 필수. 임계값 자체 조정은 [ADR-0037](../adr/0037-complexity-gate.md) 의 Reconsideration Trigger 를 근거로 한 새 결정으로만 하며, `clippy.toml`·스크립트 상단 1곳에서만 바꿔 diff 로 이력이 남게 한다.

## CI 배선

- **cognitive**: 기존 `crossplatform-check.yml` 의 Windows clippy 잡이 `cargo clippy` 를 돌리므로, `cognitive_complexity = "deny"` 는 별도 배선 없이 `main` push 마다 자동 차단된다(`-D warnings` 불요 — deny 자체가 에러).
- **파일 SLOC**: `.github/workflows/complexity-check.yml` 의 `check-file-size` 잡(self-hosted Linux X64)이 `push:[main]`(문서·site 제외) + `pull_request:[main]` + `workflow_dispatch` 로 `bash scripts/check-file-size.sh` 를 돌린다. **실질 채널은 main push 다** — 이 저장소는 PR 을 열지 않아 PR 트리거는 장식이고, 2026-09-04 에 push 트리거를 붙이기 전까지 이 워크플로는 run 이력이 0 건이었다([ADR-0131](../adr/0131-file-sloc-gate-needs-a-firing-trigger.md)). tokei 미설치 시 `cargo install tokei --locked` 가드가 선행한다. 컴파일 불요·초경량이라 mac/win 러너 부담을 피하려 Linux 단일 잡으로 두었고, cognitive 와 관심사 1:1 분리를 위해 crossplatform-check 에 섞지 않고 전용 워크플로로 둔다.
