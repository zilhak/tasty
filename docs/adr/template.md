# ADR-XXXX: <한 줄 결정 제목>

- **Status**: Proposed | Accepted | Deprecated | Superseded by ADR-XXXX
- **Date**: YYYY-MM-DD
- **Tags**: <kebab-case 태그, 콤마 구분>

## Context

무엇이 문제였나. 어떤 제약/요구사항/기존 동작이 있었나.

## Decision

무엇을 골랐나. 한 문단으로 단언.

## Consequences

- **얻은 것**: ...
- **잃은 것**: ...
- **운영 비용 / 유지 부담**: ...

## Alternatives Considered

- **A**: ... — 왜 안 골랐나
- **B**: ... — 왜 안 골랐나

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

- ...

## References

- 관련 design / dev-guide 문서
- 관련 PR / commit / 외부 자료

---

## 작성 규칙

- 파일명: `XXXX-<kebab-case-slug>.md` (번호 4 자리, 0001 시작).
- 본 템플릿의 "작성 규칙" 섹션은 실제 ADR 에 포함하지 않는다.
- **Accepted 후에는 본문을 수정하지 않는다.** Status 필드만 갱신하고, 결정이 바뀌면 새 ADR 로 Supersede 한다 (구 ADR 의 Status 를 `Superseded by ADR-YYYY` 로 변경).
- 새 ADR 작성 시 [`index.md`](index.md) 의 표에 행을 추가한다.
