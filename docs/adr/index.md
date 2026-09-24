# 아키텍처 결정 기록

오래 유지할 설계 선택과 그 이유를 주제별로 정리했다. 현재 동작·명령·개발 절차는 각 그룹의 운영 문서에서 확인한다.
새 기록을 작성할 때는 [작성 규칙](template.md)을, 표를 갱신할 때는 [인덱스 관리](../dev-guide/adr-index.md)를 따른다. 표는 ADR 헤더에서 생성하므로 직접 수정하지 않는다.

## 애플리케이션 구조와 IPC

크레이트 의존 방향부터 IPC 응답·재시도·자원 제한, 저장소와 권한까지 다룬다. [ADR-0001](0001-crate-dependency-boundaries.md)에서 관련 선택을 따라갈 수 있다.

운영 문서: [아키텍처](../architecture/index.md) · [IPC 서버](../architecture/ipc-server.md) · [API 규약](../dev-guide/api-conventions.md)

<!-- adr-rows:begin foundation -->
| # | Title | Status | Date | Tags |
|---|-------|--------|------|------|
| 0001 | [크레이트는 의존 관계와 실행 환경에 따라 나눈다](0001-crate-dependency-boundaries.md) | Accepted | 2026-09-24 | architecture, crates, headless |
| 0002 | [도메인 작업과 자원 정리는 공용 실행 계층이 맡는다](0002-domain-execution-and-ports.md) | Accepted | 2026-09-24 | architecture, domain, ipc |
| 0003 | [헤드리스는 화면 없이 완료할 수 있는 작업을 직접 처리한다](0003-headless-behavior.md) | Accepted | 2026-09-24 | headless, lifecycle, features |
| 0004 | [IPC는 지원 조건과 실패 원인을 응답으로 구분한다](0004-ipc-discovery-and-errors.md) | Accepted | 2026-09-24 | ipc, compatibility, capabilities |
| 0005 | [변경 요청 재시도는 호출자별 멱등 키로 구분한다](0005-idempotent-mutation-retries.md) | Accepted | 2026-09-24 | ipc, idempotency, retry |
| 0006 | [로컬 IPC는 TCP를 쓰고 수신·송신 자원을 제한한다](0006-bounded-ipc-transport.md) | Accepted | 2026-09-24 | ipc, transport, backpressure |
| 0007 | [IPC는 제한된 시간만 실행하고 완료 결과를 돌려준다](0007-ipc-scheduling-and-deadlines.md) | Accepted | 2026-09-24 | ipc, scheduling, timeout |
| 0008 | [요청 압력은 프로세스 단위의 제한된 진단 정보로 제공한다](0008-ipc-pressure-observability.md) | Accepted | 2026-09-24 | ipc, telemetry, diagnostics |
| 0009 | [사용 기록과 진단 로그는 수명에 맞춰 저장한다](0009-state-storage-and-retention.md) | Accepted | 2026-09-24 | storage, retention, state |
| 0010 | [저장소는 적용된 설정과 저장 실패를 구분해 알린다](0010-storage-failure-reporting.md) | Accepted | 2026-09-24 | storage, sqlite, durability |
| 0011 | [비밀 데이터의 보호 범위를 IPC와 파일 권한으로 구분한다](0011-secrets-and-local-trust.md) | Accepted | 2026-09-24 | security, secrets, passkey |
| 0012 | [요청은 라우팅 전에 권한을 확인하고 사용자 입력과 분리한다](0012-request-admission-and-isolation.md) | Accepted | 2026-09-24 | security, permissions, ipc |
<!-- adr-rows:end foundation -->

## 터미널과 원격 연결

PTY와 터미널 호환성, 입력·포커스, 점유와 원격 화면 동기화를 다룬다. [ADR-0013](0013-terminal-io-and-process-lifetime.md)에서 관련 선택을 따라갈 수 있다.

운영 문서: [터미널](../features/terminal/index.md) · [입력과 포커스](../design/policies/focus.md) · [원격 연결](../dev-guide/attach-behavior.md)

<!-- adr-rows:begin terminal -->
| # | Title | Status | Date | Tags |
|---|-------|--------|------|------|
| 0013 | [PTY 처리와 자식 프로세스 수명을 GUI에서 분리한다](0013-terminal-io-and-process-lifetime.md) | Accepted | 2026-09-24 | terminal, pty, lifecycle |
| 0014 | [터미널 호환성은 실제 수요와 사용자 상태 보호를 기준으로 정한다](0014-terminal-compatibility-scope.md) | Accepted | 2026-09-24 | terminal, compatibility |
| 0015 | [마우스와 입력 상태는 실제 입력 경로에서 판단한다](0015-terminal-user-input-routing.md) | Accepted | 2026-09-24 | input, mouse, busy |
| 0016 | [창의 OS 통합과 종료 처리를 앱 수명에 맞춘다](0016-window-platform-and-shutdown.md) | Accepted | 2026-09-24 | window, platform, shutdown |
| 0017 | [구조 변경은 ID를 기준으로 하고 사용자 포커스를 보존한다](0017-workspace-identity-and-focus.md) | Accepted | 2026-09-24 | workspace, focus, routing |
| 0018 | [화면 캡처와 전체화면은 대상을 명확히 구분한다](0018-explicit-capture-and-fullscreen-stage.md) | Accepted | 2026-09-24 | screenshot, fullscreen |
| 0019 | [단축키 문법과 설정을 공유하고 도움말은 실제 키 입력을 따른다](0019-keybinding-settings-and-hints.md) | Accepted | 2026-09-24 | keybindings, settings |
| 0020 | [원격 연결과 attach 설정을 분리한다](0020-remote-connection-profiles.md) | Accepted | 2026-09-24 | remote, ssh, profiles |
| 0021 | [점유한 작업은 연결 소유권에 따라 보호한다](0021-occupancy-and-attach-admission.md) | Accepted | 2026-09-24 | attach, occupancy, permissions |
| 0022 | [원격 화면은 서버 상태를 확인한 뒤 표시한다](0022-remote-mirror-content-and-queries.md) | Accepted | 2026-09-24 | attach, mirror, content |
| 0023 | [attach 연결의 출력과 구조 변경을 같은 순서로 동기화한다](0023-attach-state-sync-and-forwarding.md) | Accepted | 2026-09-24 | attach, stream, synchronization |
| 0024 | [주의 환기 상태는 surface 소유자가 관리한다](0024-attention-ownership-and-clear.md) | Accepted | 2026-09-24 | attention, notifications, ownership |
<!-- adr-rows:end terminal -->

## 플러그인·이벤트·파일 열기

플러그인의 신뢰·수명·렌더링과 Lua·훅, 파일 열기, Webhook, 이벤트·출력 조회를 다룬다. [ADR-0025](0025-plugin-trust-and-distribution.md)에서 관련 선택을 따라갈 수 있다.

운영 문서: [플러그인 제작](../dev-guide/plugin-development.md) · [파일 열기](../features/file-handler/index.md) · [이벤트 카탈로그](../reference/event-catalog.md)

<!-- adr-rows:begin plugins -->
| # | Title | Status | Date | Tags |
|---|-------|--------|------|------|
| 0025 | [플러그인 샌드박스와 마켓플레이스는 보류한다](0025-plugin-trust-and-distribution.md) | Deferred | 2026-09-24 | plugins, runtime |
| 0026 | [플러그인의 소유권과 실행 상태를 따로 관리한다](0026-plugin-registration-and-lifecycle.md) | Accepted | 2026-09-24 | plugins, runtime |
| 0027 | [자동화는 호스트 메인 스레드를 기다리게 하지 않는다](0027-lua-and-hook-execution.md) | Accepted | 2026-09-24 | plugins, runtime |
| 0028 | [플러그인이 그린 mesh를 호스트가 합성한다](0028-egui-mesh-rendering.md) | Accepted | 2026-09-24 | plugins, runtime |
| 0029 | [Webview 콘텐츠와 호스트 창의 책임을 나눈다](0029-webview-host-integration.md) | Accepted | 2026-09-24 | plugins, runtime |
| 0030 | [번들 도구의 데이터 범위와 보존 수준을 정한다](0030-bundled-plugin-data.md) | Accepted | 2026-09-24 | plugins, runtime |
| 0031 | [파일 열기는 대상과 사용자 조작 여부를 끝까지 보존한다](0031-file-handler-routing.md) | Accepted | 2026-09-24 | plugins, runtime |
| 0032 | [Webhook은 정해진 작업을 접수하고 고정 응답을 보낸다](0032-webhook-admission.md) | Accepted | 2026-09-24 | plugins, runtime |
| 0033 | [이벤트 피드는 짧게 보관하고 소비자가 읽은 위치를 관리한다](0033-event-feed-delivery.md) | Accepted | 2026-09-24 | plugins, runtime |
| 0034 | [터미널 출력은 스트림과 바이트 위치로 이어 읽는다](0034-output-cursor-contract.md) | Accepted | 2026-09-24 | plugins, runtime |
<!-- adr-rows:end plugins -->

## 화면·테마·국제화

공용 디자인과 테마, 팝업·모달, 입력·모션, 프리셋 편집, 길이 단위와 번역을 다룬다. [ADR-0035](0035-shared-design-and-theme.md)에서 관련 선택을 따라갈 수 있다.

운영 문서: [테마](../design/systems/theme.md) · [갤러리 작업](../dev-guide/gallery-first.md) · [길이 타입](../concepts/typed-length.md) · [국제화](../dev-guide/i18n.md)

<!-- adr-rows:begin ui -->
| # | Title | Status | Date | Tags |
|---|-------|--------|------|------|
| 0035 | [디자인 값과 아이콘은 공용 정의에서 가져온다](0035-shared-design-and-theme.md) | Accepted | 2026-09-24 | design, theme, gallery |
| 0036 | [오버레이의 입력과 수명은 종류와 소유 범위로 정한다](0036-overlay-scope-and-lifetime.md) | Accepted | 2026-09-24 | popup, scope, lifecycle |
| 0037 | [UI 입력과 시각 피드백은 공용 동작으로 맞춘다](0037-ui-input-motion-and-elevation.md) | Accepted | 2026-09-24 | ui, input, motion |
| 0038 | [프리셋 초안은 편집 중에 보존하고 저장 충돌을 확인한다](0038-preset-drafts-and-store-conflicts.md) | Accepted | 2026-09-24 | preset, draft, concurrency |
| 0039 | [길이 타입으로 좌표계를 구분하고 변환 경계는 따로 검사한다](0039-typed-length-and-dpi-boundaries.md) | Accepted | 2026-09-24 | typed-length, dpi, guards |
| 0040 | [언어 설정과 번역 문구는 표시하는 프로세스가 일관되게 처리한다](0040-locale-catalogs-and-display-text.md) | Accepted | 2026-09-24 | i18n, locale, plugin |
<!-- adr-rows:end ui -->

## 에이전트 실행과 CLI

에이전트 상태·완료 전달, 작업 조율, CLI 오류와 transcript 중계를 다룬다. [ADR-0041](0041-agent-state-and-completion.md)에서 관련 선택을 따라갈 수 있다.

운영 문서: [작업 러너](../dev-guide/agent-runner.md) · [CLI 구조](../dev-guide/cli-structure.md) · [Agent Stream](../plugins/agent-stream/index.md)

<!-- adr-rows:begin agents -->
| # | Title | Status | Date | Tags |
|---|-------|--------|------|------|
| 0041 | [에이전트 상태 보고와 완료 전달을 분리한다](0041-agent-state-and-completion.md) | Accepted | 2026-09-24 | agents, hooks, completion, observation |
| 0042 | [에이전트 작업 조율과 DAG 화면은 호스트가 맡는다](0042-agent-coordination-and-task-views.md) | Accepted | 2026-09-24 | agents, tasks, concurrency, dag |
| 0043 | [CLI 오류 정보는 보존하고 진단 로그는 기록 주체를 나눈다](0043-cli-errors-and-diagnostic-logs.md) | Accepted | 2026-09-24 | cli, errors, logging, hooks |
| 0044 | [에이전트 응답은 transcript에서 수집해 별도 SSE로 중계한다](0044-agent-transcript-stream.md) | Accepted | 2026-09-24 | agents, transcript, sse, recovery |
<!-- adr-rows:end agents -->

## 개발·검증·문서·배포

테스트 격리와 검증 근거, CI·소스 검사, 문서 작성과 릴리스 관리의 선택을 다룬다. [ADR-0045](0045-test-isolation-and-harness.md)에서 관련 선택을 따라갈 수 있다.

운영 문서: [검증](../dev-guide/self-verification.md) · [CI](../dev-guide/ci-gates.md) · [문서 작성](../documentation-model.md) · [릴리스](../dev-guide/release.md)

<!-- adr-rows:begin rules -->
| # | Title | Status | Date | Tags |
|---|-------|--------|------|------|
| 0045 | [테스트는 사용자 환경과 분리하고 E2E 인스턴스를 공유한다](0045-test-isolation-and-harness.md) | Accepted | 2026-09-24 | testing, isolation, e2e |
| 0046 | [검증 결과에는 검사 범위와 실패 원인을 구분할 근거를 남긴다](0046-verification-evidence-and-diagnostics.md) | Accepted | 2026-09-24 | testing, verification, diagnostics |
| 0047 | [CI는 실제 병합 결과를 검사하고 복잡도 증가를 제한한다](0047-ci-and-complexity-checks.md) | Accepted | 2026-09-24 | ci, complexity, quality |
| 0048 | [소스 검사는 공용 분석기를 쓰고 검사 범위와 예외를 함께 확인한다](0048-source-guards-and-exemptions.md) | Accepted | 2026-09-24 | guards, testing, source-analysis |
| 0049 | [문서는 현재 동작을 설명하고 근거를 다시 확인할 수 있게 쓴다](0049-documentation-structure-and-evidence.md) | Accepted | 2026-09-24 | documentation, architecture, evidence |
| 0050 | [ADR은 중요한 선택을 기록하고 현재 규칙은 가이드에서 관리한다](0050-architecture-decision-records.md) | Accepted | 2026-09-24 | documentation, adr, architecture |
| 0051 | [릴리스는 서명과 고지문을 갖춘 번들로 만들고 내용 변경에 버전을 올린다](0051-release-artifacts-and-versioning.md) | Accepted | 2026-09-24 | release, plugins, versioning |
<!-- adr-rows:end rules -->
