# ADR-0066: 0.x에서는 호환성을 깨는 변경을 유예 기간 없이 `(BREAK)`로 낸다

- **Status**: Accepted
- **Date**: 2026-10-06
- **Tags**: ipc, compatibility, versioning, changelog
- **Group**: foundation
- **개정 대상**: [ADR-0004](0004-ipc-discovery-and-errors.md) — 호환성을 깨는 변경의 유예 조항

## Context

[ADR-0004](0004-ipc-discovery-and-errors.md)는 호환성을 깨는 일반 변경에 deprecation 절차를 요구했다.
API 규약의 0.x 정책은 break 전에 한 minor 이상 deprecation을 두라고 적었고,
보안·심각한 버그·불가침 원칙 위반만 유예를 생략할 수 있는 예외로 두었다.

실제 운영은 이 규칙을 따르지 않았다.
루트 `CHANGELOG.md`의 `(BREAK)` 항목 중 `### Deprecated` 절을 거쳐 한 minor 이상 유예된 변경은 없었다.
2026-10-06 기준 `(BREAK)` 항목은 12개, `### Deprecated` 절은 0개였다.
예외에 들지 않는 break도 유예 없이 냈다. 예를 들어 0.10.2의 `tasty design *` 명령 제거는 대체 명령 없이 바로 이뤄졌다.
규칙과 관행 중 어느 쪽을 고칠지 정해야 했다.

## Decision

0.x 동안 호환성을 깨는 변경은 유예 기간 없이 바로 낸다.
`CHANGELOG.md`의 해당 항목 앞에 `(BREAK)`를 붙여 알린다.
"한 minor 이상 deprecation" 규칙과 유예를 생략할 수 있는 예외 목록은 두지 않는다.

이전 이름을 alias로 잠시 남기는 것은 막지 않는다. 남길 때의 표기는 [API 규약](../dev-guide/api-conventions.md)의 안정성 정책을 따른다.
안정선 이후의 deprecation 기간은 안정선에 진입할 때 정한다.

## Consequences

규칙이 실제 운영과 일치한다. break마다 예외 해당 여부를 따지는 문장이 필요 없다.

외부 호출자는 0.x에서 경고 기간 없이 동작 변경을 받는다.
`(BREAK)` 표기와 capability 선언이 호출자가 변경을 알아차리는 수단이다.

## Alternatives Considered

- 규칙을 유지하고 관행을 고친다: 기존 `(BREAK)` 항목을 예외별로 분류하고 앞으로 `### Deprecated` 절을 운영할 담당과 시점을 정해야 한다. 0.x에서 즉시 break하는 관행이 이미 자리 잡아 채택하지 않았다.
- 규칙은 두고 예외 목록만 넓힌다: 대부분의 break가 예외가 되어 원칙이 의미를 잃는다.

## Reconsideration Triggers

- 안정선(1.x) 진입을 정하면 deprecation 기간을 이 결정과 별도로 정한다.
- 외부 client가 유예 없는 break로 피해를 보았다는 보고가 반복되면 0.x에도 유예를 둘지 다시 검토한다.

## References

- [API 규약 — 안정성 정책](../dev-guide/api-conventions.md)
- [ADR-0004](0004-ipc-discovery-and-errors.md)
- 운영 기록: `CHANGELOG.md`
