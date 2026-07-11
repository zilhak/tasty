# Claude Design lock 프로토콜 부트스트랩 (AI 절차)

충돌 방지는 코드로 하드코딩하지 않는다. 대신 **AI 가 이 절차로 대상 디자인 프로젝트에
규약을 심고**, 이후 designer·구동측이 파일로 협응한다. 멱등 — 이미 있으면 재기입 안 함.

프로토콜 전문: `tasty design protocol`.

## 전제

- 대상 디자인 프로젝트에 DesignSync(claude_design MCP)로 접근 가능해야 한다.
- 이 저장소의 projectId/접근 정보는 로컬 지침(`.claude/CLAUDE.md`)에서 확인한다.

## 단계

1. 대상 프로젝트의 `CLAUDE.md`(없으면 `SKILL.md`)를 DesignSync `get_file` 로 읽는다.
2. **"Claude Design 동시성 lock" designer 규율**(첫 액션 `.WORKING` 생성 → 마지막 `.DONE`
   rename)이 이미 있는지 확인한다.
3. **없으면** 아래 designer 규율 블록을 그 파일 끝에 append 한다(`finalize_plan` →
   `write_files`). **있으면** "설치됨"만 보고하고 재기입하지 않는다(멱등).
4. `design-tasks/` 폴더는 designer 가 첫 `.WORKING` 파일을 만들 때 자동 생성된다(별도
   mkdir 불필요).
5. 이후 구동측(claude code)은 **발사 전 `list_files` 게이트 + `.DONE` 폴링**(프로토콜의
   claude code 규율)을 따른다. `tasty design chat` 을 쏘기 전 반드시 lock 확인.

## 대상 CLAUDE.md 에 append 할 designer 규율 (정본)

```markdown
## Claude Design 동시성 lock (매 turn 필수)

이 프로젝트를 대상으로 요청을 받으면, 매 turn 다음을 지킨다:

1. **첫 액션**: `design-tasks/<YYYYMMDD-hhmm>-<slug>.WORKING.md` 를 만든다(디자인 작업보다 먼저).
   frontmatter 에 `request`(원 요청 경로)·`started`·`state: WORKING` 를 적는다.
2. 디자인 작업(파일/컴포넌트 수정)을 수행한다.
3. **마지막 액션**: 그 파일에 결과 요약(`## 만든 것` — 수정/추가한 파일·컴포넌트)과
   `finished` 를 적고, **`.DONE.md` 로 rename** 한다(실패 `.FAILED`, 사용자 결정 필요
   `.NEEDS-INPUT`). rename 은 반드시 turn 의 맨 끝 — 중간에 바꾸면 lock 이 조기 해제된다.

이 규칙은 claude.ai/design 이 한 프로젝트에 동시 한 turn 만 허용하는 제약과, 구동측이
파일로 busy/완료를 판정하는 협응을 위한 것이다. 자세히는 `tasty design protocol`.
```
