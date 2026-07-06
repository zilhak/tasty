# Clippy 설정 정책

tasty 의 clippy 경고는 **워크스페이스 단위로 lint 를 끄기보다 위치별 `#[allow(...)]` / 소스 수정** 을 선호한다. `clippy.toml` 의 `disallowed-methods` 주석이 원칙을 명시한다 — *예외가 필요한 곳은 파일/함수 단위 `#[allow(...)]`*.

## 현재 워크스페이스 설정

| 위치 | 설정 | 의미 |
|------|------|------|
| `Cargo.toml [workspace.lints.clippy]` | `undocumented_unsafe_blocks = "deny"` | 모든 unsafe 블록 위 `// SAFETY:` 강제 ([unsafe-checklist](unsafe-checklist.md)) |
| 〃 | `multiple_unsafe_ops_per_block = "warn"` | 블록당 unsafe op 2개+ (deny 승격 전 비용 측정) |
| 〃 | `result_large_err = "allow"` | 도메인 Error enum ↔ IPC fault path 1:1 매핑이라 일괄 allow |
| `clippy.toml` | `disallowed-methods` | 색 생성 함수의 우연한 호출 차단 ([color-policy](color-policy.md)) — 옛 pre-commit C.8 이 이리로 이관 |

clippy 내장 threshold config(`type-complexity-threshold`, `too-many-arguments-threshold` 등)는 **위반을 봐주려고 느슨하게 풀지 않는다**(원칙 #3) — 위반은 소스 개선(struct/type alias) 또는 위치별 attr 로 처리한다. 이는 threshold 를 *올려 신규 위반까지 은폐하는* 것을 금하는 것이며, 복잡도 상한 강제 자체를 배제하지 않는다. 복잡도 상한은 별도의 **복잡도 게이트**(아래)가 담당한다 — 두 축은 독립이다.

## 복잡도 게이트

함수 cognitive 복잡도(clippy 내장 `cognitive_complexity` = `deny`, 임계 20)와 파일 SLOC(`tokei` + `scripts/check-file-size.sh`, 상한 1000)의 **신규/증가분**을 차단한다. 기존 초과분은 위치 단위로 동결한다 — 함수는 `#[allow(clippy::cognitive_complexity)] // complexity-exempt: <사유>`, 파일은 `.complexity-file-allowlist`. 여기서 threshold 를 *조이는* 것(cognitive 게이트 신설)은 위 원칙 #3(threshold 를 *푸는* 것 금지)과 모순이 아니라 계층 분리다. 상세: [complexity-gate.md](complexity-gate.md), [ADR-0037](../adr/0037-complexity-gate.md).

## 결정 원칙

1. **소스 수정이 본질 개선이면 소스를 고친다** — `derivable_impls`(manual Default→derive), `too_many_arguments`(인자→context struct), `should_implement_trait`(inherent `from_str`→`FromStr`).
2. **의도된 패턴이면 모듈/위치 단위 `#[allow]`** — 예: `intent` 의 `from_*` 메서드는 `From` 변환이 아니라 *intent dispatch source 부착* 의미라 `#![allow(clippy::wrong_self_convention)]`. 테스트 fixture 의 `type_complexity` 도 모듈 allow.
3. **워크스페이스 단위로 끄지 않는다** — 신규 코드의 정당한 위반까지 묻히기 때문. threshold 조정도 우회라 지양.

위치별 처리 비용이 과해지면(예: 비영어 doc 의 `doc_lazy_continuation` 빈발) 워크스페이스 allow 재검토.
