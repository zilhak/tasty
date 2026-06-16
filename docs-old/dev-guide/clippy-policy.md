# Clippy 설정 정책

Tasty 의 clippy 경고는 **워크스페이스 단위로 lint 를 끄기보다 위치별 `#[allow(...)]` /
소스 수정** 을 선호한다. `clippy.toml` 의 `disallowed-methods` 주석부터 이 원칙을
명시한다 — *"예외가 필요한 곳은 파일/함수 단위로 `#[allow(...)]` 부여"*.

## 현재 워크스페이스 설정

| 위치 | 설정 | 의미 |
|------|------|------|
| `Cargo.toml [workspace.lints.clippy]` | `undocumented_unsafe_blocks = "deny"` | 모든 unsafe 블록 위 `// SAFETY:` 강제 |
| 〃 | `multiple_unsafe_ops_per_block = "warn"` | 블록당 unsafe op 2개 이상 적발 (deny 승격 전 비용 측정 단계) |
| 〃 | `result_large_err = "allow"` | 도메인 Error enum ↔ IPC fault path 1:1 매핑, 일괄 allow |
| `clippy.toml` | `disallowed-methods` | 색 생성 함수의 우연한 호출 차단 ([color-policy.md](color-policy.md)) |

`type-complexity-threshold`, `too-many-arguments-threshold` 등 **threshold 설정은 두지
않는다.** 해당 lint 의 위반은 소스 개선(struct 도입, type alias) 또는 위치별 attr 로 처리.

## 결정 원칙 (2026-06)

clippy 경고 정리 시 다음 우선순위로 판단한다.

1. **소스 수정이 본질 개선이면 소스를 고친다.** — `derivable_impls`(manual Default → derive),
   `too_many_arguments`(인자 묶음 → context struct), `should_implement_trait`(inherent
   `from_str` → `FromStr` 구현), `doc_lazy_continuation`(doc 들여쓰기 수정).
2. **의도된 패턴이라 침묵이 합리적이면 모듈/위치 단위 `#[allow]`.** — `intent.rs` 의
   `from_*` 메서드는 `From` trait 변환이 아니라 *intent dispatch source 부착* 의미이므로
   모듈 최상단 `#![allow(clippy::wrong_self_convention)]` 1줄. 테스트 fixture 의
   `type_complexity` 는 모듈 `#![allow]`.
3. **워크스페이스 단위로 lint 를 끄지 않는다.** — 신규 코드의 정당한 위반까지 묻히기
   때문. threshold 조정도 *우회* 에 가까워 지양. 위 두 단계로 처리.

향후 비영어 doc 에서 `doc_lazy_continuation` 가 빈발하는 등 **위치별 처리 비용이 과해지면**
워크스페이스 단위 allow 를 재검토한다 (현재는 건수가 적어 소스 수정이 정확).
