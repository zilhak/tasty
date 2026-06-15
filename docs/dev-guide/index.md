# 개발 가이드 — AI 에이전트용

이 프로젝트를 **개발하는** AI 에이전트를 위한 가이드. 빌드, 디버깅, 테스트, UI 검증 방법을 다룬다.

> Tasty를 **사용하는** AI 에이전트를 위한 가이드는 `docs/agent-guide/`를 참조.

## 최초 셋업 (clone 직후 1회)

```bash
./scripts/dev-setup.sh
```

git hooks 디렉토리를 `.githooks/` 로 설정한다. 안 하면 pre-commit / pre-push
검사가 안 돈다. 상세: [git-hooks.md](git-hooks.md).

## 환경별 가이드

| 환경 | 문서 |
|------|------|
| Linux | [linux.md](linux.md) |
| Windows | (예정) |
| macOS | (예정) |

## 주제별 가이드

| 주제 | 문서 |
|------|------|
| 개발 환경 셋업 + git hooks 규칙 | [git-hooks.md](git-hooks.md) |
| 수정 후 자체 검증 (커밋 전에 직접 돌려볼 것) | [self-verification.md](self-verification.md) |
| 커밋 컨벤션 (Conventional Commits) | [commit-convention.md](commit-convention.md) |
| 에러 처리 정책 (Result 무시 금지) | [error-handling.md](error-handling.md) |
| 국제화 (i18n — t() / lang 파일) | [i18n.md](i18n.md) |
| 빌드 & 빌드 최적화 | [build.md](build.md) |
| dist 빌드 명령 카탈로그 | [dist-build.md](dist-build.md) |
| 릴리스 절차 | [release.md](release.md) |
| 릴리스 러너 설정 | [release-runners.md](release-runners.md) |
| 의존성 이슈 모니터링 (future-incompat 등) | [dep-issues.md](dep-issues.md) |
| Crash & 에러 진단 | [crash-diagnostics.md](crash-diagnostics.md) |
| Debug 전용 IPC | [debug-ipc.md](debug-ipc.md) |
| Attach 동작 명세 (서버=loopback / 로컬-원격=클라이언트) | [attach-behavior.md](attach-behavior.md) |
| 컨텍스트 메뉴 구현 | [context-menu.md](context-menu.md) |
| Popup 구현 | [popup-implementation.md](popup-implementation.md) |
| GPU 렌더링 구조 | [gpu-rendering.md](gpu-rendering.md) |
| 색 생성 정책 (newtype + clippy 강제) | [color-policy.md](color-policy.md) |
| Model + Host View 분리 (surface 추가/뷰 상태 관리) | [model-view-split.md](model-view-split.md) |
| Lua hook 개발 (호스트 측 매핑) | [lua-hooks.md](lua-hooks.md) |
| 에이전트 식별 (surface ↔ agent 매핑, 잠정 모델) | [agent-identification.md](agent-identification.md) |
| 에이전트 task runner | [agent-runner.md](agent-runner.md) |
| 에이전트 runner primitive (semaphore / barrier / rate_limit) | [agent-runner-primitives.md](agent-runner-primitives.md) |
| TUI 테스트 | [tui-testing.md](tui-testing.md) |
| E2E 테스트 (격리 / timeout 정책) | [e2e-tests.md](e2e-tests.md) |
| Plugin 제작 | [plugin-development.md](plugin-development.md) |
| Plugin 권한 모델 | [plugin-permissions.md](plugin-permissions.md) |
| Plugin 민감 데이터 다루기 | [plugin-sensitive-data.md](plugin-sensitive-data.md) |
| Plugin 서명 | [plugin-signing.md](plugin-signing.md) |
| Plugin staging 동기화 (7 위치) | [plugin-staging-sync.md](plugin-staging-sync.md) |
| Plugin 생태계 정책 (1.0 전 결정) | [plugin-ecosystem.md](plugin-ecosystem.md) |
| Plugin 분류 정책 (host-native / bundled / user) | [../architecture/plugin-categories.md](../architecture/plugin-categories.md) |
| CLI/IPC 명명 규칙 | [cli-naming.md](cli-naming.md) |
| IPC 안정성 정책 (break 분류·deprecation) | [ipc-stability.md](ipc-stability.md) |
| Unsafe 작성 체크리스트 | [unsafe-checklist.md](unsafe-checklist.md) |
| 외부 라이브러리 노트 | [libs/index.md](libs/index.md) |
