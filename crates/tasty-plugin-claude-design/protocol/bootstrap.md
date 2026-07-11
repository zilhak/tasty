# Claude Design lock 프로토콜 부트스트랩 (AI 절차)

충돌 방지는 코드로 하드코딩하지 않는다. 대신 **AI 가 이 절차로 대상 디자인 프로젝트에
규약을 심고**, 이후 designer·구동측이 파일로 협응한다. 멱등 — 이미 있으면 재기입 안 함.

프로토콜 전문: `tasty design protocol`.

## 전제

- 대상 디자인 프로젝트에 DesignSync(claude_design MCP)로 접근 가능해야 한다.
- 이 저장소의 projectId/접근 정보는 로컬 지침(`.claude/CLAUDE.md`)에서 확인한다.

> **⚠️ DesignSync 는 `CLAUDE.md` / `.claude/` 쓰기를 차단한다** — 이들은 designer 에게
> 지시를 나르는 **예약 경로**라, 외부에서 지시를 주입하지 못하도록 `write_files` 가 막힌다
> (읽기 `get_file` 은 허용). 따라서 designer 규율은 **write_files 로 직접 심을 수 없고**,
> 아래 (a)/(b) 로 설치한다.

## 단계

1. 대상 프로젝트의 `CLAUDE.md` 를 DesignSync `get_file` 로 읽는다(읽기는 허용).
2. **"Claude Design 동시성 lock" designer 규율**(첫 액션 `.WORKING` 생성 → 마지막 `.DONE`
   rename)이 이미 있는지 확인한다. **있으면** "설치됨"만 보고하고 끝(멱등).
3. **없으면** — write_files 가 막혀 있으니 둘 중 하나로 설치한다:
   - **(a) 사용자 설치 (권장)**: 아래 designer 규율 블록을 사용자에게 제시하고, 사용자가
     claude.ai/design 프로젝트의 `CLAUDE.md` 끝에 직접 붙여넣게 한다(1회성). 지시를
     프로젝트 지침에 넣는 것은 원래 사용자 권한이라 보안 모델과도 정합.
   - **(b) design chat 자기설치**: `tasty design chat` 으로 designer 에게 "아래 규율을 네
     `CLAUDE.md` 끝에 추가하라"고 요청한다(designer 는 프로젝트 안에서 자기 파일을 편집할
     수 있다). 단 이 turn 자체도 동시성 제약 대상이므로 다른 turn 이 없을 때 보낸다.
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
