# Tasty 문서 인덱스

크로스 플랫폼 GPU 가속 네이티브 터미널 에뮬레이터. 본 인덱스는 현재 상태 문서로 진입하는 시작점이다. 구현된 기능의 상세는 [features.md](features.md), 설계는 `design/`, 아키텍처는 `architecture/`, 개발 가이드는 `dev-guide/`, 에이전트용 가이드는 `agent-guide/` 에 있다.

## 설치

| 문서 | 설명 |
|------|------|
| [installation.md](installation.md) | 사용자/에이전트 설치 가이드 — OS·아키텍처별 산출물, 설치 방법, 설치 위치 |

## 개념 정의

프로젝트가 합의한 용어와 개념. 코드 작업 시작 전 ubiquitous-language 를 먼저 읽는다.

| 문서 | 설명 |
|------|------|
| [concepts/index.md](concepts/index.md) | 개념 문서 인덱스 |
| [concepts/ubiquitous-language.md](concepts/ubiquitous-language.md) | 유비쿼터스 언어 — 용어 정의, 계층 구조, 코드 매핑 |
| [concepts/layout.md](concepts/layout.md) | 두 레벨 레이아웃 — 상위(고정)/하위(탭 종속) 분할 개념 |
| [concepts/typed-length.md](concepts/typed-length.md) | 타입 안전 길이 시스템 — PhysicalPx/LogicalPx newtype, DPI 혼동 방지 |

## ADR (결정 기록)

아키텍처/정책 결정의 근거·대안·재검토 조건을 기록한다. 신규 작성은 [adr/template.md](adr/template.md) 양식.

| 문서 | 설명 |
|------|------|
| [adr/index.md](adr/index.md) | ADR 목록 (번호 / 상태 / 날짜 / 태그) |
| [adr/0001-linux-system-tray-unsupported.md](adr/0001-linux-system-tray-unsupported.md) | ADR-0001 — Linux 시스템 트레이 미지원 결정 (DE 분열, 태스크바 유지로 충분) |

## 디자인 문서

현재 시스템이 *어떻게 동작하는가* 의 명세. 분류별 진입은 [design/index.md](design/index.md).

| 분류 | 진입 | 설명 |
|------|------|------|
| Systems | [design/systems/](design/systems/index.md) | popup, theme, settings, storage, toast, memory |
| Policies | [design/policies/](design/policies/index.md) | focus, cwd, key-mapping, busy-indicator 등 |
| Flows | [design/flows/](design/flows/index.md) | action-dispatch, intent-coroutine, split-command 등 |

## 아키텍처 문서

| 문서 | 설명 |
|------|------|
| [아키텍처 개요](architecture/index.md) | 워크스페이스 크레이트 33개, 본 바이너리 모듈 구조, 의존성 DAG |
| [멀티 윈도우](architecture/multi-window.md) | 멀티 윈도우 아키텍처 — 엔진/윈도우/모달 구조 |
| [입력 계층](architecture/input-layer.md) | 마우스 입력 계층 — z-order 기반 이벤트 소비/버블링 구조 |
| [모듈별 상세](architecture/modules.md) | 디렉토리 모듈별 책임, 설계 목적, 한계 |
| [데이터 흐름](architecture/data-flows.md) | 5가지 주요 데이터 흐름 (파일+함수 기준) |
| [라이브러리 분리](architecture/library-separation/index.md) | 워크스페이스 33 crate 현황 + 분리 의사결정 회고 (G.E — tasty-model 분리, Phase F.B — tasty-ipc / tasty-plugin-manifest / tasty-host-plugin / tasty-cli 4 crate 추가, plugin_bridge/ 본 바이너리 잔존) |
| [UI widgets crate](architecture/ui-widgets-crate.md) | `tasty-ui-widgets` 의 목적·의존·위젯 카탈로그 — 본체와 갤러리가 공유하는 egui layout primitive (two_depth_layout / horizontal_tab_bar_with_arrows / tab_content_frame) + tokens (SUB_TAB_PANEL_WIDTH 등 layout 상수) |
| [Plugin categories](architecture/plugin-categories.md) | host-native / bundled plugin / user plugin 3 카테고리 분류 정책 + 기존 "builtin" 표현 매핑 |
| [성능 벤치마크](architecture/performance-benchmarks.md) | F.G GPU 최적화 실측 — terminals_ms p50/p99/max + draw call 수 + atlas eviction 카운터 (10 surface ASCII / CJK 4 surface, release / dist 프로필) |
| [Invariants](architecture/invariants/index.md) | *깨지면 안 되는 시스템 약속* 모음 — 코드 변경 시 가장 먼저 점검할 리스트 (surface-cwd 등) |
| [Surface cwd invariant](architecture/invariants/surface-cwd.md) | Surface 변환·생성 시 cwd carry 강제 규칙 + Surface trait / `SurfaceKindDef.create` / `ConvertSurfaceTarget::Kind` / `SurfaceCreateCtx.cwd` 의 compile-time guard |

## 평가 / POC

시점성 평가 / POC 결과 보존본 — ADR 의 *근거* 원본. 진입은 [evaluations/index.md](evaluations/index.md) (시점·결론·재검토 trigger 표).

| 문서 | 설명 |
|------|------|
| [Plugin sandbox 평가](evaluations/plugin-sandbox.md) | WASM / OS-level / 현 상태 비교 — 0.7 보류 근거 + 재검토 trigger |
| [WASM Plugin POC 결과](evaluations/wasm-poc.md) | Phase J.C — clipboard-history WASM 변환 + wasmtime host runtime 실측 (cold-start ~142ms, host_call ~3µs) + 정식 도입 권고 |
| [Plugin marketplace 평가](evaluations/plugin-marketplace.md) | registry / install-by-id / trust / install flow 옵션 비교 — 0.7.x 보류 근거 + 도입 순서 + sandbox 와 묶음 의사결정 |
| [리팩토링 진행 상태](evaluations/refactoring-status.md) | 남아있는 개선 가능성, 우선순위별 로드맵 |
| [라이브러리 분리 — 옛 6 관점 분석](evaluations/library-separation/) | 2025 분리 계획 시점 분석 보존본 (technical-feasibility 외 5) |

## 기능 명세 (Features)

구현된 기능의 *현재 상태 상세* 는 [features.md](features.md) 에 모두 흡수되어 있다. 기능 단위 *행동 명세 + Acceptance Criteria* 는 features/ 인덱스를 본다.

| 문서 | 설명 |
|------|------|
| [features.md](features.md) | 구현된 전체 기능의 현재 상태 상세 (터미널 엔진 / 워크스페이스·탭 / 알림 / 분할 / CLI 등) |
| [features/index.md](features/index.md) | Feature Spec 인덱스 — 기능 단위 행동 명세 + Acceptance Criteria |

## AI 에이전트 가이드

> **agent-guide/ 이름 유지 결정**: "사용자의 AI 에이전트용" 가이드 폴더명은 `agent-guide/` 로 유지한다. *Tasty 개발 AI 에이전트* (dev-guide) 와 헷갈릴 수 있으나, 외부 빌드/배포/유저 문서가 이 경로를 참조하므로 rename 의 통일성 이득보다 link rot 비용이 크다. (실제 변경은 별도 PR 시 재검토.)

### 사용자의 AI 에이전트용 (Tasty 사용법 — 폴더명 `agent-guide/`)

릴리스 에셋으로 배포. AI 에이전트가 Tasty를 IPC/CLI로 조작하기 위한 가이드.

| 문서 | 설명 |
|------|------|
| [agent-guide/index.md](agent-guide/index.md) | 개요 + 환경별 링크 |
| [agent-guide/api-reference.md](agent-guide/api-reference.md) | IPC/CLI 전체 레퍼런스 |
| [agent-guide/clipboard.md](agent-guide/clipboard.md) | 클립보드 히스토리 (tool.clipboard.*) 사용 가이드 |
| [agent-guide/file-handler.md](agent-guide/file-handler.md) | 파일 핸들러 시스템 — detector/handler 등록 + picker + user TOML |
| [agent-guide/plugins.md](agent-guide/plugins.md) | Plugin 설치/관리 (`tasty plugin ...`) 가이드 |
| [agent-guide/event-catalog.md](agent-guide/event-catalog.md) | Surface Hook/Plugin 이벤트 카탈로그 |
| [agent-guide/output.md](agent-guide/output.md) | 터미널 출력 구조화 (parse_since_mark / commands / observer) |
| [agent-guide/output-parsers.md](agent-guide/output-parsers.md) | 출력 파서 카탈로그 (tasty-output 빌트인 10종) |
| [agent-guide/approval.md](agent-guide/approval.md) | 휴먼 핸드오프 — approval / diff surface |
| [agent-guide/telemetry.md](agent-guide/telemetry.md) | 텔레메트리 — 관측 / 비용 / 이상 탐지 / 세션 요약 |
| [agent-guide/agent.md](agent-guide/agent.md) | 다중 에이전트 협업 — task DAG / barrier / semaphore / lease / reducer / rate-limit |
| [agent-guide/capabilities.md](agent-guide/capabilities.md) | 권한 / capability_elevation / audit log |
| [agent-guide/blackboard.md](agent-guide/blackboard.md) | 공유 컨텍스트 — Blackboard (`memory.bb_*`, snapshot 포함) |
| [agent-guide/plan.md](agent-guide/plan.md) | 공유 컨텍스트 — Plan (`memory.plan_*`) + [plan.schema.json](agent-guide/plan.schema.json) |
| [agent-guide/cache.md](agent-guide/cache.md) | 공유 컨텍스트 — Cache (`memory.cache_*`) |
| [agent-guide/lua-hooks.md](agent-guide/lua-hooks.md) | `~/.tasty/init.lua` 사용자 hook 가이드 — 등록·이벤트 목록·예제 |
| [agent-guide/themes.md](agent-guide/themes.md) | 테마 파일 추가/관리 — `~/.tasty/themes/*.toml` partial TOML 포맷 |
| [agent-guide/linux.md](agent-guide/linux.md) | Linux 사용 가이드 |

### 개발 AI 에이전트용 (Tasty 개발 가이드 — 폴더명 `dev-guide/`)

이 프로젝트를 개발하는 AI 에이전트를 위한 가이드. 빌드, 디버깅, UI 검증 등. 주제별 전수 목록은 [dev-guide/index.md](dev-guide/index.md).

| 문서 | 설명 |
|------|------|
| [dev-guide/index.md](dev-guide/index.md) | 개요 + 환경별 링크 + 주제별 전수 목록 |
| [dev-guide/self-verification.md](dev-guide/self-verification.md) | 수정 후 자체 검증 — 커밋 전 직접 재현 (사용자에게 떠넘기지 않기) |
| [dev-guide/build.md](dev-guide/build.md) | 워크스페이스 구조, 빌드 프로필(dev/release/dist), LTO, 빌드 시간 측정 |
| [dev-guide/dist-build.md](dev-guide/dist-build.md) | dist 빌드 명령 카탈로그 (Justfile + 자동 sanity check + SHA256SUMS) |
| [dev-guide/release.md](dev-guide/release.md) | 릴리스 절차 (버전 → 체인지로그 → 태그 → push) + 0.7.x SemVer 가드 + 0.7.1 walkthrough |
| [dev-guide/release-runners.md](dev-guide/release-runners.md) | self-hosted 러너 인벤토리, 1회 도구 설치, 운영 명령 |
| [dev-guide/commit-convention.md](dev-guide/commit-convention.md) | Conventional Commits 형식, type 목록, 단위 분할 기준 |
| [dev-guide/dep-issues.md](dev-guide/dep-issues.md) | 의존성 이슈 모니터링 — block v0.1.6 future-incompat 점검 방법과 전환 트리거 |
| [dev-guide/linux.md](dev-guide/linux.md) | Linux 개발 환경 가이드 |
| [dev-guide/context-menu.md](dev-guide/context-menu.md) | 우클릭 컨텍스트 메뉴 (네이티브 메뉴 필수, PendingNativeMenu 패턴) |
| [dev-guide/popup-implementation.md](dev-guide/popup-implementation.md) | Popup 구현 (PopupDef 시스템, `egui::Window` 직접 사용 금지) |
| [dev-guide/gpu-rendering.md](dev-guide/gpu-rendering.md) | GPU 렌더링 구조 (공유 버퍼 + submit 분리 규칙) |
| [dev-guide/color-policy.md](dev-guide/color-policy.md) | 색 생성 정책 — newtype + clippy 강제, 테마 두 레이어 모델 |
| [dev-guide/model-view-split.md](dev-guide/model-view-split.md) | Model + Host View 분리 패턴 (GUI-free 도메인 유지) |
| [dev-guide/debug-ipc.md](dev-guide/debug-ipc.md) | Debug 빌드 전용 IPC 메서드 (사용자 입력 재현, popup 트리거) |
| [dev-guide/crash-diagnostics.md](dev-guide/crash-diagnostics.md) | Crash & 에러 진단 (로그, strace, gdb) |
| [dev-guide/tui-testing.md](dev-guide/tui-testing.md) | TUI 테스트 — 터미널 에뮬레이션 버그 재현 및 자동 검증 |
| [dev-guide/e2e-tests.md](dev-guide/e2e-tests.md) | e2e 테스트 환경 격리 정책 + spawn timeout 정책 + flaky 진단 절차 |
| [dev-guide/i18n.md](dev-guide/i18n.md) | 국제화 정책 — `t()` API, lang 파일 위치, 새 문자열 추가 절차 |
| [dev-guide/error-handling.md](dev-guide/error-handling.md) | 에러 처리 정책 — `Result` 무시 금지, `tracing::warn!`/`error!` 사용 규칙 |
| [dev-guide/lua-hooks.md](dev-guide/lua-hooks.md) | Lua hook 호스트 매핑 — 이벤트별 발화 site / payload 스키마 / 추가 가이드 |
| [dev-guide/agent-identification.md](dev-guide/agent-identification.md) | 에이전트 식별 — surface ↔ agent 매핑 (잠정 모델) |
| [dev-guide/agent-runner.md](dev-guide/agent-runner.md) | Agent task runner — TaskExecutor trait, HostExecutor 매핑, RunnerRegistry, host→plugin sync IPC |
| [dev-guide/agent-runner-primitives.md](dev-guide/agent-runner-primitives.md) | Agent primitive 통합 — semaphore-gated dispatch, WaitBarrier task, IPC rate_limit 미들웨어 |
| [dev-guide/cli-naming.md](dev-guide/cli-naming.md) | CLI 명령 네이밍 규칙 |
| [dev-guide/ipc-stability.md](dev-guide/ipc-stability.md) | IPC 메서드 안정성 정책 |
| [dev-guide/unsafe-checklist.md](dev-guide/unsafe-checklist.md) | unsafe 블록 작성 체크리스트 |
| [dev-guide/plugin-development.md](dev-guide/plugin-development.md) | Plugin 제작 가이드 — 크레이트 골격, Plugin trait, UI 빌더, snapshot/restore, 빌드/설치 + `crates/tasty-plugin-markdown/` 템플릿 |
| [dev-guide/plugin-permissions.md](dev-guide/plugin-permissions.md) | Plugin 권한 모델 — method_meta, CallerContext, grant/revoke 흐름 |
| [dev-guide/plugin-sensitive-data.md](dev-guide/plugin-sensitive-data.md) | Plugin 민감 데이터 다루기 — secret 계층, 암호화 미적용 근거 |
| [dev-guide/plugin-signing.md](dev-guide/plugin-signing.md) | Plugin 매니페스트 서명 — Ed25519 dev/release 키, sign-bundle.sh, CI secret, 키 회전 |
| [dev-guide/plugin-ecosystem.md](dev-guide/plugin-ecosystem.md) | Plugin 생태계 — 번들 plugin 목록과 책임 분담 |
| [dev-guide/plugin-staging-sync.md](dev-guide/plugin-staging-sync.md) | Plugin staging 7 위치 동기화 (deb / rpm / wix / 빌드 스크립트 / BUILTINS) — 새 plugin 추가 시 수정해야 할 곳 |
| [dev-guide/git-hooks.md](dev-guide/git-hooks.md) | pre-commit / pre-push 훅 규칙 — 설치, 검사 목록, 예외 |
| [dev-guide/libs/index.md](dev-guide/libs/index.md) | 외부 라이브러리 노트 — 의존성별 사용 패턴/함정 (clap, egui, wgpu, winit, termwiz 등) |

## AI 자체 검증 지침

| 문서 | 설명 |
|------|------|
| [ai-verification/visual-verification.md](ai-verification/visual-verification.md) | UI 변경 시 색상 대비, 레이어 순서, 픽셀 수치 검증 규칙 |
| [ai-verification/screenshot-methods.md](ai-verification/screenshot-methods.md) | GUI 스크린샷 촬영 방법 (IPC / PowerShell) |
| [ai-verification/egui-layout.md](ai-verification/egui-layout.md) | egui 레이아웃, 레이어 순서 주의사항 |
| [ai-verification/state-none-gpu-separation.md](ai-verification/state-none-gpu-separation.md) | state None 시 GPU 호출 분리 패턴 |
| [ai-verification/ipc-usage.md](ai-verification/ipc-usage.md) | IPC를 통한 Tasty 조작 방법 |
| [ai-verification/python-execution.md](ai-verification/python-execution.md) | Windows에서 python 실행 주의 |
| [ai-verification/tcp-communication.md](ai-verification/tcp-communication.md) | TCP 통신 도구 (Python socket) |
| [ai-verification/windows-process-cleanup.md](ai-verification/windows-process-cleanup.md) | Windows 프로세스 트리 종료 |
| [ai-verification/ime-testing.md](ai-verification/ime-testing.md) | IME 시뮬레이션을 이용한 디버깅 가이드 |

## 디자인 시스템 요청

외부 디자인 시스템 (`.claude/Tasty Design System/`) 측에 *디자인 가이드를 요청* 하는 문서. 본체 docs 와는 별도 경로.

| 문서 | 설명 |
|------|------|
| [design-requests/workspace-card-description.md](design-requests/workspace-card-description.md) | 워크스페이스 카드 설명 — 디자인 시스템 측 요청 |

## 구현 현황 빠른 안내

본 인덱스의 옛 "기능 목록 표" 는 구현 완료된 항목의 옛 기획 링크로 채워져 있었다. 현재는 모두 [features.md](features.md) 에 흡수되어 있으니, 어떤 기능이 어떻게 구현되어 있는지 확인하려면 그쪽을 본다. GPU 렌더링·테스트·설치 같은 횡단 주제는 dev-guide 와 design 의 해당 문서로 흡수되었다 (예: `dev-guide/gpu-rendering.md`, `dev-guide/tui-testing.md`, `installation.md`). (옛 docs/plans/* 파일은 제거되었으며, 아직 미구현인 기획은 `.claude-workspace/plans/archived-from-docs/` 로 옮겨졌다.)

## 기술 스택

- **언어**: Rust
- **윈도우/입력**: winit
- **GPU 렌더링**: wgpu
- **UI 위젯**: egui (UI) + 커스텀 셰이더 (터미널)
- **VTE 파싱**: termwiz
- **PTY**: portable-pty (Windows: ConPTY)
- **IPC**: TCP (127.0.0.1, 동적 포트, ~/.tasty/tasty.port)
- **CLI**: clap
- **라이선스**: MIT
