# ADR-0426: `file_handler.reload` 는 버린 user 항목을 `rejected` 필드로 알린다

- **Status**: Accepted
- **Date**: 2026-09-21
- **Tags**: ipc, cli, file-handler, agent-facing, compatibility, settings

## Context

`file_handler.reload` 는 user 설정(`<TASTY_HOME>/file-handlers.toml`)을 다시 읽어 user 출처 handler 를
통째로 갈아 끼운다. 응답은 `{"path", "exists"}` 뿐이었다. 그런데 registry 는 user 항목을 두 자리에서
버린다.

1. **설치 전**: id 에 `<owner>/` 접두사가 없으면 설치하지 않는다(`install_user`).
2. **finalize**: 모든 출처를 겹쳐도 detector 나 action 이 비면 그 id 를 등록하지 않는다
   (`ensure_finalized`). finalize 는 조회 때 게으르게 돌아 **reload 시점에는 아직 일어나지 않았다.**

두 버림 모두 `warn!` 한 줄로만 남았다. 실측(2026-09-21, base `290d89f56`, 격리 헤드리스 데몬):
세 항목(접두사 없는 `md-as-html` · action 없는 `user/no-action` · 온전한 `user/good`)을 적고
`tasty file-handler reload` → 응답 `{"exists": true, "path": …}`, CLI rc 0. 로그에는 접두사 경고 한
줄만 있었다 — 두 번째 버림은 reload 가 끝난 뒤 조회가 일어날 때까지 로그에도 없다.

사용자와 에이전트는 설정이 무시된 것을 dispatch 결과로만 알 수 있다. 에이전트가 자기 작업의 결과를
알 수 있어야 한다는 [identity](../identity.md) 원칙 2 에 어긋난다.

## Decision

**응답에 `rejected` 필드를 더한다.** 값은 이 reload 가 버린 user 항목의 배열이다 —
`[{"id": <TOML 에 적힌 그대로>, "reason": <코드>}]`, 버린 것이 없으면 `[]`. `path` · `exists` 는
그대로다.

사유 코드는 둘이다.

| 코드 | 뜻 |
|------|----|
| `missing_owner_prefix` | id 에 `<owner>/` 접두사가 없어 설치하지 않았다 |
| `missing_detector_or_action` | 모든 출처를 겹쳐도 detector 나 action 이 없어 등록되지 않는다 |

두 번째는 finalize 를 기다리지 않고 reload 가 **같은 판정**으로 미리 고른다
("어느 출처든 detector 하나 · action 하나를 가졌는가" — finalize 는 마지막 non-None 이 이기므로
이것과 같다). 한 판정을 두 자리가 따로 들고 있으므로 단위 시험이 보고와 finalize 결과를 대조한다.

`FileHandlerRegistry::reload_user_config` 가 이 목록을 돌려주고, `Core::reload_file_handlers` 가
응답에 싣는다. CLI 는 응답 JSON 을 그대로 찍으므로 `tasty file-handler reload` 출력에 같이 보인다.
부팅 로드(`install_user_config`)는 돌려줄 호출자가 없어 버림을 로그로만 남긴다.

## Consequences

- **얻은 것**: 설정 일부가 무시된 것을 reload 응답 한 번으로 안다. 어느 항목이 왜 빠졌는지까지 온다.
- **얻은 것**: action 없는 항목의 버림이 reload 시점에 드러난다. 전에는 조회가 일어나기 전까지 로그에도
  없었다.
- **잃은 것 / 한계**: 파일을 읽거나 파싱하지 못해 reload 자체가 멈춘 경우는 항목을 모르므로 `rejected`
  가 빈 배열이다. 그 경우 이전 user 설정이 그대로 남는데 응답은 여전히 그 사실을 말하지 않는다. 이
  ADR 의 범위는 **항목 단위 거절**이고, 전체 중단의 신호는 따로 정한다(아래 트리거).
- **호환성**: 필드 추가만이다. 기존 필드의 뜻과 CLI rc 는 그대로다. 알 수 없는 필드를 무시하는
  호출자는 영향이 없다.

## Alternatives Considered

- **A. 버린 항목이 있으면 JSON-RPC 오류로 답한다.** 안 고른 이유: 나머지 항목은 이미 적용됐다. 오류로
  답하면 "아무것도 안 됐다" 로 읽히고, 기존 호출자의 성공 경로(rc 0)가 바뀐다.
- **B. `rejected` 를 id 문자열 배열로만 싣는다.** 안 고른 이유: 같은 증상(설정이 안 먹었다)에 원인이
  둘이라, 사유 없이는 무엇을 고칠지 모른다.
- **C. reload 에서 finalize 를 강제로 돌려 그 결과로 보고한다.** 안 고른 이유: finalize 는 host ·
  plugin 항목도 함께 판정해 user 가 만들지 않은 버림이 섞일 수 있다. 보고의 주어는 이 reload 가 받은
  user 항목이어야 한다.

## Reconsideration Triggers

**채널이 붙는 것**

- `crates/tasty-file-handler/src/registry_tests.rs` 의 `reload_user_config_reports_the_entries_it_dropped`
  가 실패한다 — 보고가 finalize 의 실제 판정과 어긋났다는 뜻이다. 변이 확인: 판정 함수를 늘 참으로
  바꾸면 이 시험이 죽는다(2026-09-21).
- `tests/e2e_tests.rs` 의 `file_handler_reload_reports_the_entries_it_dropped` 가 실패한다 — 응답
  모양이 바뀌었다. 변이 확인: 응답 조립에서 항목을 비우면 죽는다(2026-09-21).

**원리적으로 안 붙는 것**

- user 항목을 버리는 **세 번째 자리**가 registry 에 생긴다. 그 자리는 이 보고에 자동으로 안 들어간다.
  재는 법: registry 의 `warn!` 중 user 항목을 버리는 것을 세어 사유 코드 수와 견준다.
- 파싱 · 읽기 실패로 reload 가 멈춘 사실을 응답에 실어야 한다는 요구가 생긴다. 그때 이 응답에 필드를
  하나 더 정한다.

## References

- [file-handler 기능 문서](../features/file-handler/index.md) — 인터페이스 절의 reload 응답
- [ADR-0425](0425-headless-file-dispatch-answers-that-this-build-cannot-open-files.md) — 같은 namespace 의
  dispatch 가 거짓 성공을 고친 결정
- 코드 근거(결정이 실현된 현재 위치): `crates/tasty-file-handler/src/registry.rs` 의
  `reload_user_config` · `RejectedUserHandler` · `is_complete` · `src/core/file.rs` 의
  `ReloadFileHandlersOutcome` · `src/adapters/ipc/handler/file_handler.rs` 의 `handle_reload`
