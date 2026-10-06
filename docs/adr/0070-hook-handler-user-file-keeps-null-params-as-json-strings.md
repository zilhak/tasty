# ADR-0070: 사용자 훅 핸들러 파일은 null을 품은 호출 params만 JSON 문자열로 쓴다

- **Status**: Accepted
- **Date**: 2026-10-06
- **Tags**: plugins, hooks, storage, compatibility
- **Group**: plugins

## Context

사용자 훅 핸들러는 `~/.tasty/hook-handlers.toml`에 저장된다. `tasty hook-handler upsert`, 설정 창 편집, 직접 편집이 모두 이 파일을 쓴다.
`IpcSequence` 호출의 `params`는 임의의 JSON 값이고, IPC 요청에는 `{"a":null}`처럼 null이 들어갈 수 있다. TOML에는 null이 없다.
그래서 이런 호출을 저장하면 action 직렬화가 실패했고, action 전체가 파일에서 빠진 채 다음 부팅에 사라졌다.
null은 "값 없음"과 다르다. 받는 IPC 메서드가 키의 존재 여부로 동작을 가를 수 있으므로 null 키를 지우는 방식으로는 풀 수 없다.

조건은 세 가지다.
- 이미 사용자가 손으로 쓴 파일과 이전 빌드가 쓴 파일을 그대로 읽어야 한다.
- 파일은 계속 사람이 읽고 고칠 수 있어야 한다.
- 파일에 오류가 있으면 경고 로그가 TOML 줄·열 위치를 보여 줘야 한다.

## Decision

파일 형식은 TOML로 유지하고, 호출 하나의 `params`만 상황에 따라 두 가지로 쓴다.

- null이 없는 params는 이전처럼 TOML 표 `params`로 쓴다.
- params 자체가 null이면 `params` 키를 쓰지 않는다. 읽을 때 null로 채운다.
- params 안쪽 어디든(중첩 객체, 배열 원소 포함) null이 있으면 그 호출의 params 전체를 JSON 문자열 `params_json`으로 쓴다.
  읽을 때 JSON으로 파싱해 `params`로 되돌린다.
- 한 호출에 `params`와 `params_json`이 함께 있거나 `params_json`이 JSON이 아니면 파일 전체를 파싱 실패로 본다.
  기존 규칙대로 이전 사용자 설정을 유지한다.
- 읽기는 TOML에서 파일 전용 선언 타입으로 바로 역직렬화한다. `params_json`의 JSON 파싱은 호출 변환 안에서 하므로 그 오류에도 toml이 위치를 붙인다.

IPC 계약은 바뀌지 않는다. IPC 요청은 JSON이라 null을 그대로 받는다. 위 규칙은 사용자 파일을 읽고 쓰는 부분에만 적용된다.
현재 규칙은 [훅 기능 문서](../features/hooks/index.md)의 핸들러 레지스트리 절에 있다.

## Consequences

- null을 품은 시퀀스도 저장과 재시작 뒤에 그대로 남는다.
- 이전 형식 파일은 그대로 읽힌다. null이 없는 시퀀스는 다시 저장해도 이전과 같은 모양으로 쓰인다.
- 한 파일 안에 params 표기가 두 가지가 된다. `params_json`은 JSON 문자열이라 TOML 편집기의 구조 표시와 검증을 받지 못한다.
- 사용자 파일 전용 action 타입(`UserFileActionDecl`)이 IPC 선언 `UserHookHandlerActionDecl`과 같은 모양을 따로 유지한다. action 종류나 필드를 추가하면 두 타입을 함께 고쳐야 한다.
- `params_json` 오류의 위치는 그 호출을 담은 action 표다. 호출이 인라인 배열 안에 있기 때문이다.

## Alternatives Considered

- **파일 전체를 JSON으로 바꾼다.** layout이 JSON이라 null을 담는 것과 같은 방식이다.
  - 기존 TOML 파일을 옮기는 단계와 두 형식의 공존 기간이 필요하다.
  - 다른 사용자 설정(config.toml, preset)과 표기가 갈라진다.
  - null이 드문 경우를 위해 파일 전체를 바꾸는 것은 비용이 크다.
- **null을 가진 키를 별도 목록으로 기록한다.** 배열 원소의 null(`[1,null]`)과 깊은 중첩을 가리키려면 경로 표기가 필요하다. 그러면 params 본문과 경로 목록이 서로 어긋날 수 있다.
- **TOML 미지원을 유지한다.** null을 담은 호출은 저장할 때 거부한다. 사용자는 같은 시퀀스를 IPC로는 등록할 수 있는데 파일에는 남길 수 없게 된다. 파일이 IPC보다 좁은 형식이 된다.
- **모든 params를 JSON 문자열로 쓴다.** 표현은 하나가 되지만 이전 형식 파일과 손으로 쓴 파일의 모양이 바뀌고 TOML 편집의 이점을 모든 호출에서 잃는다.
- **TOML을 JSON 값으로 바꾼 뒤 역직렬화한다.** 구현이 가장 짧지만 타입 오류의 TOML 줄·열 위치와 원문 발췌가 경고 로그에서 빠졌다. 그래서 채택하지 않았다.

## Reconsideration Triggers

코드와 설정에서 확인:
- 사용자 훅 핸들러 파일이 TOML이 아닌 형식으로 바뀌거나, 사용자 설정 전반이 null을 담을 수 있는 형식으로 옮겨 가면 `params_json`이 필요 없다.
- 설정 창에 IpcSequence 본문 편집기(`src/hook_handler/sequence_text.rs`)가 붙어 사람이 파일을 직접 고칠 일이 줄면 "모든 params를 JSON 문자열로" 대안을 다시 볼 수 있다.
- `HookHandlerAction`에 IpcSequence 외의 JSON 값 필드가 생기면 같은 문제가 그 필드에도 생긴다. 이 규칙을 넓힐지 검토한다.

실행 결과로 확인:
- 사용자가 `params_json`을 손으로 고치다 생기는 파싱 실패가 반복해서 보고되면 표기를 다시 검토한다. 경고 로그 `hook_handler: user config parse failed`를 본다.

## References

- [훅 기능 문서](../features/hooks/index.md) — 핸들러 레지스트리 CLI와 사용자 파일 규칙.
- `src/hook_handler/user_file.rs` — 쓰기(`action_toml`)와 파일 전용 읽기 타입.
- `src/hook_handler/registry.rs` — `export_user_config`, `parse_user_handler_section`.
- [ADR-0019](0019-keybinding-settings-and-hints.md) — 단축키 설정에서도 TOML에 null이 없어 형식 검토가 필요하다고 적었다.
- [ADR-0027](0027-lua-and-hook-execution.md) — IpcSequence 실행 순서.
