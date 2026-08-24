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
  - **References-only errata 예외**: Accepted ADR 이라도 References 섹션의 깨진(비-git 위치 등) 링크를 위생 차원에서 고치는 것은 결정 변경이 아니므로 허용한다. 단 본문(Context / Decision / Consequences 등 결정 내용)은 그대로 둔다.
- **외부(비-git) 위치 문서 참조 금지.** ADR(및 모든 committed 문서)은 `.claude-workspace/` 등 git 에 올라가지 않는 경로의 문서를 참조/링크하지 않는다. 그곳에만 존재하는 근거 자료가 ADR 에 필요하면, **그 내용을 `docs/` 의 적절한 위치로 재구성해 git-tracked 문서로 만들고, 그 문서를 참조**한다. ADR 본문은 외부 휘발성 자료 없이 자족적이어야 한다.
  - 이 규칙(및 CLAUDE.md 의 TODO 파일·디자인 changelog 인용 금지)은
    `tests/no_todo_file_citation.rs` 가 `cargo test --workspace`(CI)로 강제한다.
    금지 형태를 담는 것이 본질인 파일은 그 테스트의 `ALLOWLIST_FILES` 에 등록한다.
  - 예외(허용): 폴더의 *존재·용도(컨벤션) 자체를 정의/설명*하는 서술(예: `.claude-workspace/temp` 가 임시 출력 위치라는 설명)은 특정 문서를 SoT 로 참조하는 것과 다르므로 허용한다.
- 새 ADR 작성 시 [`index.md`](index.md) 의 표에 행을 추가한다.
