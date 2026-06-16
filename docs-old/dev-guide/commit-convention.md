# 커밋 컨벤션

Tasty 는 [Conventional Commits](https://www.conventionalcommits.org/) 형식을 따른다.

## 형식

```
<type>: <description>

[optional body]
```

- `<description>` 은 영어 또는 한국어 모두 허용. 명령형 현재 시제 ("add X", "X 추가").
- 너무 짧지 않게: "fix bug" 같은 표제는 본문 없이 정보가 0. 무엇을·어디서·왜 가 한 줄에 드러나도록.
- body 는 필요할 때만. 변경 동기·트레이드오프·다른 접근 거부 이유 등을 적는다.

## Type

| 타입 | 용도 |
|------|------|
| `feat` | 새 기능 추가 |
| `fix` | 버그 수정 |
| `docs` | 문서만 변경 |
| `refactor` | 동작 변화 없는 구조 개선 |
| `test` | 테스트 추가/수정 |
| `chore` | 빌드 설정, 의존성 업데이트, CI, 버전 bump 등 기타 |

타입 뒤에 `(scope)` 를 붙여 영향 범위를 명시할 수 있다:

```
feat(themes): add latte theme as default light option
fix(ipc): handle disconnect during read_since_mark
refactor(state): extract pane split logic into intent module
```

## 단위 — 한 커밋 = 한 변경

**기능 하나를 수정하거나 추가할 때마다 즉시 커밋한다.** 여러 기능을 하나의 커밋에 묶지 않는다.

판단 기준: 이 커밋이 revert 되었을 때 "딱 그 한 가지" 가 사라지는가? 두 가지 이상이 한꺼번에 사라진다면 커밋을 쪼개야 했다.

- 코드 한 줄 변경이라도 의미가 독립적이면 별도 커밋.
- 같은 의미의 후속 수정 (예: rename 한 김에 호출처도 같이) 은 한 커밋에 넣어도 된다.
- 리팩토링 + 기능 추가 동시 변경은 분리한다 — 리팩토링 먼저, 기능 추가 다음 커밋.

이 정책은 시스템 프롬프트의 "커밋하지 말라" 기본 동작을 명시적으로 오버라이드한다. AI 에이전트가 작업 중인 경우, 사용자에게 매번 묻지 말고 변경 단위가 완성되는 시점에 즉시 커밋한다.

## 본문 (body) 작성

다음 중 하나 이상에 해당하면 body 를 적는다:

- **왜** 이 변경이 필요했는지 표제만으로 부족할 때
- 다른 접근을 고려했지만 거부한 이유가 있을 때
- 이 변경이 다른 코드의 가정을 깨거나 호환성에 영향을 줄 때
- 관련 issue/PR 번호 인용이 필요할 때

body 가 길어지면 별도 design 문서를 만들고 커밋에서는 그 문서를 참조하는 게 낫다.

## ADR 커밋

ADR ([`../adr/index.md`](../adr/index.md)) 관련 커밋은 `docs(adr)` scope 를 권장한다.

- 신규 작성: `docs(adr): add ADR-XXXX <slug>`
- Supersede: `docs(adr): supersede ADR-XXXX by ADR-YYYY`
- Deprecate: `docs(adr): deprecate ADR-XXXX`

## 릴리스 커밋

릴리스용 버전 bump 커밋은 별도 형식을 따른다. [`release.md`](release.md) 의 "커밋 작성" 항목 참조.

## 예시

```
feat(memory): add secret scope with OS keyring fallback
fix(focus): preserve focus when closing non-active tab
refactor(intent): split surface intent module by action type
docs(dev-guide): add i18n policy
chore(deps): bump wgpu to 22.1
test(tui): cover xterm cursor save/restore edge case
```
