# 디자인 문서

현재 시스템이 *어떻게 동작하는가* 의 명세. *왜 이렇게 결정했는가* 는 `docs/adr/`, *무엇이 변하면 안 되는가* 는 `docs/architecture/invariants/` 참조.

| 분류 | 폴더 | 설명 |
|------|------|------|
| Systems | [systems/](systems/index.md) | 자체 lifecycle 을 가진 컴포넌트 (popup, theme, settings, storage, toast, memory) |
| Policies | [policies/](policies/index.md) | 코드가 따라야 할 규칙 (focus, cwd, key-mapping 등) |
| Flows | [flows/](flows/index.md) | 동작 흐름 / 명령 설계 (action-dispatch, intent-coroutine, split-command 등) |
