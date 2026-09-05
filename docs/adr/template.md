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
  - **사실 정정 예외**: 이 불변 원칙은 *결정이 바뀌는* 경우를 위한 것이다 — 이전 결정을 기록으로 보존하고, 바뀐 결정은 새 ADR 로 대체한다. 반면 처음부터 **사실과 어긋난 서술**(존재하지 않는 경로·API·동작, git 에 없는 위치를 실제 사용처로 적은 것 등)은 보존할 "이전 결정" 자체가 없으므로 정정 대상이며, 본문을 직접 고친다. Status·Date 는 그대로 둔다.
  - **부분 개정 예외**: 결정의 **한 조항만** 다른 결정으로 대체하고 나머지는 유효로 남기는
    형태. 구 ADR 의 본체가 살아 있는데 Supersede 하면 유효한 결정까지 죽은 것으로 읽히므로,
    조항 단위로 갈아끼운다. **Supersede 와 갈리는 지점은 "본체가 살아 있는가" 하나다.**
    - 새 ADR — 제목에 `— ADR-XXXX 의 <조항> 개정` 을 달고, Decision 에 무엇을 개정하는지
      적는다. **개정하지 않는 것을 이름으로 열거하는 것이 필수다** — 무엇이 안 바뀌는지
      안 적으면 다음 사람이 구 ADR 전체를 의심한다. 부분 개정의 값이 거기서 나온다. References 에
      `개정 대상: [ADR-XXXX](<파일>) (<조항>)` 과 `개정 패턴 선례: [ADR-0030](<파일>)` 을 적는다
      (꺾쇠는 실제 값으로 채운다).
    - 구 ADR — References 에 `부분 개정: [XXXX](<파일>) (<조항> 개정)` 한 줄만 더한다.
      **Status 는 `Accepted` 로 유지하고 `Date`·`Accepted` 도 그대로 둔다.**
    - 선례: [ADR-0030](0030-image-egui-mesh-bitmap-texture.md) 이 [ADR-0028](0028-plugin-egui-mesh-render-channel.md) 의
      image Canvas-하이브리드 조항만, [ADR-0065](0065-markdown-webview-render-channel.md) 이 같은 ADR 의
      markdown 채널 조항만 개정했다. 0028 은 지금도 `Accepted` 다.
  - **References-only errata 예외**: Accepted ADR 이라도 References 섹션의 깨진(비-git 위치 등) 링크를 위생 차원에서 고치는 것은 결정 변경이 아니므로 허용한다. 단 본문(Context / Decision / Consequences 등 결정 내용)은 그대로 둔다.
  - **구현 확정 보강 예외**: Decision 이 아키텍처/방향만 정하고 구체적 형태·영향 범위를 다루지
    않았던 지점(명시적으로 "후속에서 확정한다" 고 적어뒀든, 그냥 언급이 없었든)을 후속 트랙이
    실제 구현으로 확정한 경우 — 확정된 내용을 같은 ADR 의 Decision/Consequences 에 반영하는
    것을 허용한다. 후속 구현이 실제로 만들어낸 사실(예: 새 저장 위치, 새 불변식)로 기존 서술을
    갱신하는 것도 포함한다. ADR 은 만고불변의 텍스트가 아니다 — 그 결정이 실제로 확정되는
    순간까지 새 ADR 로 쪼개 미룰 이유가 없다.
    - 이미 확정돼 있던 결정**자체**(무엇을 고를지, 왜 그걸 골랐는지)를 뒤집거나 다른 선택으로
      바꾸는 것은 이 예외에 해당하지 않는다 — 그 경우는 여전히 새 ADR 로 Supersede 한다. 이
      예외는 **그 결정을 실현하는 세부사항을 채우거나 갱신하는 것**에만 적용되고, **결정
      자체를 다시 여는 것**에는 적용되지 않는다.
    - 보강한 절은 소제목 등으로 구분해 어느 트랙/시점에 채워졌는지 드러낸다. `Date` 필드는 최초 Accepted 시점을 그대로 유지한다 — 나중에 읽는 사람이 "Date 시점에 전부 결정됐다" 고 오인하지 않게 하기 위함이다.
- **비-git 경로 참조 금지.** ADR(및 모든 committed 문서)은 git 이 추적하지 않는 경로 — `.gitignore` 로 제외된 레포 로컬 작업 폴더와 로컬 지침 폴더 — 를 적지 않는다. 그 안의 문서를 SoT 로 참조하는 것뿐 아니라 **폴더 이름을 단독으로 언급하는 것도 금지**다. 규칙 전문·범위 밖 4 종·판정 기준은 [ADR-0105](0105-no-nongit-path-refs-in-tracked-sources.md).
  - 그곳에만 존재하는 근거 자료가 ADR 에 필요하면, **그 내용을 `docs/` 의 적절한 위치로 재구성해 git-tracked 문서로 만들고, 그 문서를 참조**한다. ADR 본문은 외부 휘발성 자료 없이 자족적이어야 한다.
  - 위치를 알려야 하는 서술은 위치 대신 위임으로 쓴다 — "커밋되지 않는 로컬 전용 지침이 정한다". (예전에는 "폴더의 존재·용도 설명은 허용" 이라는 예외가 있었으나 ADR-0105 이 폐지했다. 예외가 곧 누수 통로였다.)
  - 이 규칙(및 CLAUDE.md 의 TODO 파일·디자인 changelog 인용 금지)은
    `crates/tasty-doc-guards/tests/no_todo_file_citation.rs` 가 강제한다 — `doc-guards.yml` 이 main push · PR 마다 자동 실행한다(경로 필터 없음). 자동 잡은 push 된 커밋만 보므로 커밋 전에는 직접 돌린다 — [ci-gates](../dev-guide/ci-gates.md).
    금지 형태를 담는 것이 본질인 파일은 그 테스트의 `ALLOWLIST` 에 **(경로, 허용 패턴)** 으로 등록한다.
- 새 ADR 작성 시 [`index.md`](index.md) 의 표에 행을 추가한다.
