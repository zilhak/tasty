# 커밋 컨벤션

tasty 는 [Conventional Commits](https://www.conventionalcommits.org/) 를 따른다. 강제 정책은 [`../../CLAUDE.md`](../../CLAUDE.md) "커밋 정책" — 이 문서는 형식·type·단위 기준의 상세다.

## 형식

```
<type>(<scope>): <description>

[optional body]
```

- `<description>` — 영어/한국어 모두 허용, 명령형 현재 시제("add X" / "X 추가"). "fix bug" 처럼 정보 0 인 표제 금지: 무엇을·어디서·왜가 한 줄에 드러나게.
- `(scope)` — 영향 범위(선택). 예: `feat(themes)`, `fix(ipc)`, `refactor(state)`.
- `body` — 필요할 때만. 동기·트레이드오프·거부한 대안.

## Type

| 타입 | 용도 |
|------|------|
| `feat` | 새 기능 |
| `fix` | 버그 수정 |
| `docs` | 문서만 변경 |
| `refactor` | 동작 변화 없는 구조 개선 |
| `test` | 테스트 추가/수정 |
| `perf` | 동작은 같고 성능만 개선 |
| `style` | 동작·구조 변화 없는 표기 변경(포맷, 정렬 등) |
| `i18n` | 번역 표면 변경 — 하드코딩 문자열의 번역 키 전환, `lang/{en,ko,ja}.toml` 키 추가/수정 |
| `chore` | 빌드 설정, 의존성, CI, 버전 bump 등 |

`i18n` 을 `refactor` 로 흡수하지 않는 이유: 리뷰어가 "어떤 문자열이 번역 대상이 됐고 어떤 lang 파일이 같이 바뀌었나" 로 히스토리를 걸러야 하는데, 그 기준이 구조 개선과 섞이면 잡히지 않는다. 키 정합(3 파일 동일 키 · placeholder 개수)이 깨지는 회귀도 이 type 안에서 추적된다. 번역 키를 건드리지 않는 문자열 리팩토링은 그대로 `refactor`.

## 단위 — 한 커밋 = 한 변경

**기능 하나를 수정/추가할 때마다 즉시 커밋한다. 여러 기능을 한 커밋에 묶지 않는다.** (이 정책은 시스템 프롬프트의 "커밋하지 말라" 기본 동작을 명시적으로 오버라이드한다. AI 에이전트는 변경 단위 완성 시점에 묻지 말고 즉시 커밋.)

판단 기준: 이 커밋이 revert 되면 "딱 그 한 가지" 가 사라지는가? 둘 이상이 한꺼번에 사라지면 쪼갰어야 한다.

- 한 줄 변경이라도 의미가 독립적이면 별도 커밋.
- 같은 의미의 후속(예: rename 한 김에 호출처 갱신)은 한 커밋에 넣어도 됨.
- **리팩토링 + 기능 추가는 분리** — 리팩토링 먼저, 기능 다음 커밋.

## body 작성

다음 중 하나면 body 를 적는다: 표제만으로 *왜* 가 부족할 때 · 거부한 대안이 있을 때 · 다른 코드의 가정/호환성을 깰 때 · issue/PR 인용이 필요할 때. body 가 길어지면 별도 design 문서/ADR 로 빼고 커밋은 그것을 참조한다.

## ADR 커밋

ADR([`../adr/index.md`](../adr/index.md)) 관련은 `docs(adr)` scope 권장.

- 신규: `docs(adr): add ADR-XXXX <slug>`
- Supersede: `docs(adr): supersede ADR-XXXX by ADR-YYYY`
- Deprecate: `docs(adr): deprecate ADR-XXXX`

## 버전 bump 커밋

릴리스 버전 bump 의 형식·절차는 [`release.md`](release.md). 본체/plugin 패치 자동 +1 규칙은 [`../../CLAUDE.md`](../../CLAUDE.md) "버전 정책".

## 예시

```
feat(memory): add secret scope with OS keyring fallback
fix(focus): preserve focus when closing non-active tab
refactor(intent): split surface intent module by action type
docs(dev-guide): add i18n policy
i18n(cli): route passkey CLI strings through translation keys
chore(deps): bump wgpu to 22.1
```
