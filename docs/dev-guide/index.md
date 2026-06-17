# 개발 가이드

tasty 를 **개발하는** AI 에이전트용 가이드. tasty 를 *사용하는* 에이전트용 표면은 [reference/](../reference/index.md).

> 핵심 원칙 — **독립 검증**: tasty 개발 환경이 곧 tasty(dogfooding)다. debug 빌드는 release 와 환경을 격리(`tasty-debug.port` 등)해, agent 가 release tasty 안에서 동작 중이어도 자기 debug 빌드를 따로 띄워 충돌 없이 검증할 수 있다. [independent-verification](independent-verification.md).

## 시작 / 검증

| 문서 | 내용 |
|------|------|
| [git-hooks](git-hooks.md) | clone 직후 `./scripts/dev-setup.sh` + pre-commit/pre-push 검사 |
| [self-verification](self-verification.md) | 커밋 전 직접 검증(사용자에게 떠넘기지 않기) |
| [independent-verification](independent-verification.md) | dogfooding · debug↔release 격리 |
| [linux](linux.md) | Linux dev 환경(실행/재시작/스크린샷) |

## 코드 정책

| 문서 | 내용 |
|------|------|
| [commit-convention](commit-convention.md) | Conventional Commits |
| [error-handling](error-handling.md) | Result 무시 금지 |
| [clippy-policy](clippy-policy.md) | 위치별 allow 선호, 워크스페이스 끄기 지양 |
| [unsafe-checklist](unsafe-checklist.md) | `// SAFETY:` 작성 + 자가검토 5문 |
| [color-policy](color-policy.md) | 색 생성 newtype + clippy 강제 |
| [i18n](i18n.md) | `t()` / lang 파일 |

## 빌드 / 릴리스

| 문서 | 내용 |
|------|------|
| [build](build.md) | 워크스페이스·빌드 프로필 |
| [dist-build](dist-build.md) | 로컬 dist 산출물 명령 |
| [release](release.md) | 릴리스 워크플로(버전 bump → 태그 → CI) |
| [release-runners](release-runners.md) | self-hosted runner 인벤토리·운영 |
| [dep-issues](dep-issues.md) | 의존성 future-incompat 모니터링 |

## 구현 패턴

| 문서 | 내용 |
|------|------|
| [model-view-split](model-view-split.md) | Model + Host View 분리 |
| [gpu-rendering](gpu-rendering.md) | GPU 렌더링 구조 |
| [perf-benchmarks](perf-benchmarks.md) | GPU 성능 측정 |
| [popup-implementation](popup-implementation.md) | Popup(`PopupDef` 시스템) |
| [context-menu](context-menu.md) | OS 네이티브 컨텍스트 메뉴 |
| [crash-diagnostics](crash-diagnostics.md) | 크래시 진단·로그 위치 |

## IPC / Agent

| 문서 | 내용 |
|------|------|
| [api-conventions](api-conventions.md) | CLI/IPC 명명 + 안정성/버전 정책 |
| [debug-ipc](debug-ipc.md) | debug 전용 IPC + 격리 |
| [attach-behavior](attach-behavior.md) | attach(서버=loopback / 로컬-원격=클라이언트) |
| [agent-runner](agent-runner.md) | task DAG executor + 동기화 primitive |
| [agent-identification](agent-identification.md) | `AgentId` 도출(잠정 모델) |
| [lua-hooks](lua-hooks.md) | Lua hook 호스트 측 매핑 |

## 테스트

| 문서 | 내용 |
|------|------|
| [e2e-tests](e2e-tests.md) | E2E 격리/timeout 정책 |
| [tui-testing](tui-testing.md) | tui-simulator + debug 셀 검증 |

## Plugin

| 문서 | 내용 |
|------|------|
| [plugin-development](plugin-development.md) | plugin 제작 + 호스트 런타임 계약 |
| [plugin-permissions](plugin-permissions.md) | 권한 모델 |
| [plugin-sensitive-data](plugin-sensitive-data.md) | 민감 데이터 |
| [plugin-packaging](plugin-packaging.md) | 서명 + staging 동기화 |
| [plugin-ecosystem](plugin-ecosystem.md) | 생태계 정책 + 자동 upgrade |
