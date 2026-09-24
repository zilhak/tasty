# 개발 가이드

Tasty 개발자를 위한 가이드다. Tasty를 사용하는 에이전트용 명령은 [reference/](../reference/index.md).

> 핵심 원칙 — **독립 검증**: tasty 개발 환경이 곧 tasty(dogfooding)다. debug 빌드는 별도 루트(`~/.tasty-debug/`)로 release(`~/.tasty/`)와 격리돼, agent 가 release tasty 안에서 동작 중이어도 자기 debug 빌드를 따로 띄워 충돌 없이 검증할 수 있다. [self-verification 독립 검증](self-verification.md#독립-검증--개발도-agent-가-스스로-확인할-수-있어야-한다).

## 시작 / 검증

| 문서 | 내용 |
|------|------|
| [git-hooks](git-hooks.md) | Git 훅 설치, 커밋·push 검사, 단계별 실행 시간과 오류 로그 확인 |
| [shell-scripts](shell-scripts.md) | `scripts/`·`.githooks/`·`Justfile`·워크플로 `run:` 규약 — 조기에 끝나는 소비자를 파이프 오른쪽에 두지 않는다(SIGPIPE) |
| [ci-gates](ci-gates.md) | CI·Git 훅의 실제 검사 명령과 실행 조건 |
| [self-verification](self-verification.md) | 격리 인스턴스에서 직접 검증하는 절차와 결과를 해석하는 기준 |

## 코드 정책

| 문서 | 내용 |
|------|------|
| [commit-convention](commit-convention.md) | Conventional Commits |
| [adr-index](adr-index.md) | ADR 헤더로 인덱스 생성, 충돌 해결, 병합할 결정의 중복 검토 |
| [adr-renumber](adr-renumber.md) | ADR 을 새 번호로 옮기고 레포 전체의 인용을 한 번에 고치는 도구(`adr-renumber` bin) — 매핑 파일 형식, 고치는 형태와 보고만 하는 형태, 삭제되는 ADR 을 부르는 자리가 쓰기를 막는 이유, 도구가 못 보는 것 |
| [error-handling](error-handling.md) | Result 처리·락 poison 복구와 관측 범위 |
| [clippy-policy](clippy-policy.md) | 위치별 allow 선호, 워크스페이스 끄기 지양 · unsafe `// SAFETY:` 작성 + 자가검토 7문 |
| [complexity-gate](complexity-gate.md) | 복잡도 게이트(cognitive deny + 파일 SLOC), 예외 컨벤션 |
| [duplicated-sets](duplicated-sets.md) | 같은 집합이 여러 곳에 적힐 때 — 자리로 셀 수 있는 것, 합칠 곳과 남길 곳을 가르는 기준 |
| [theme › 색 생성 정책](../design/systems/theme.md#색-생성-정책) | 색 생성 newtype + clippy 강제 (design/systems/theme 의 절) |
| [i18n](i18n.md) | 번역 키·CLI 도움말·사용자 오버라이드·폰트·하드코딩 예외 |

## 빌드 / 릴리스

| 문서 | 내용 |
|------|------|
| [build](build.md) | 워크스페이스·빌드 프로필 · debug 전용 번들 자동 동기화 · 공용 모듈의 GUI 정의 경계 · 로컬 dist 산출물 명령 |
| [release](release.md) | 릴리스 워크플로(버전 bump → 태그 → CI) · self-hosted 러너 인벤토리·운영 |
| [dep-issues](dep-issues.md) | 의존성 future-incompat 모니터링 |
| [site](site.md) | 공개 사이트(GitHub Pages) 생성·배포 — `site/` 생성기, 사용자 가이드 `site/content/`(docs/ 는 발행 안 함), 집필 규칙과 사이트 어조, URL 구조, 영어 번역 모델(`site/content/en/` + 폴백 + 스탬프) |

## 구현 패턴

| 문서 | 내용 |
|------|------|
| [model-view-split](model-view-split.md) | Model + Host View 분리 |
| [gpu-rendering](gpu-rendering.md) | GPU 렌더링 구조 · 성능 측정 |
| [egui-mesh-channel](egui-mesh-channel.md) | plugin egui mesh → host 합성 렌더 채널 (ADR-0028) |
| [design-change-workflow](design-change-workflow.md) | 디자인 변경 루프 — 요청문서→Claude design 시안→갤러리/본체/사이트 사본 정합 |
| [gallery-first](gallery-first.md) | 새 UI 컴포넌트는 디자인→갤러리→본체 순서 (cut 금지), specimen 기하의 역할 명명 |
| [popup-implementation](popup-implementation.md) | Popup(`PopupDef` 시스템) |
| [dag-layout](dag-layout.md) | Task DAG 좌표 계산(`tasty-dag-layout`) — 레이어 배치·엣지 라우팅·어댑터 경계 |
| [context-menu](context-menu.md) | OS 네이티브 컨텍스트 메뉴 |
| [timer-hub](timer-hub.md) | 메인 루프의 주기 작업 등록·취소·데드라인과 대기 방식 |
| [crash-diagnostics](crash-diagnostics.md) | 크래시 진단·로그 위치 · GUI/headless 진단 범위 · WebView 로드/배치/표시 실패 |
| [memory-leak-soak](memory-leak-soak.md) | 메모리 누수 soak 테스트 — 4계층 지표·판정·플랫폼별 attribution |

## IPC / Agent

| 문서 | 내용 |
|------|------|
| [api-conventions](api-conventions.md) | CLI/IPC 명명 + 안정성/버전 정책, CLI 진입점 유무를 가르는 판별식과 그것을 실행으로 세는 법 |
| [cli-structure](cli-structure.md) | CLI 크레이트 내부 세 갈래(commands/ · request/ · local/)와 `Dispatch` |
| [debug-ipc](debug-ipc.md) | debug 전용 IPC + 격리 |
| [headless-ipc-surface](headless-ipc-surface.md) | 헤드리스 IPC의 단일 진입 검사·관측·PTY 종료 수명과 메서드별 제공 범위 |
| [headless-build-boundaries](headless-build-boundaries.md) | GUI·헤드리스 컴파일 경계와 여덟 빌드 조합 검사 |
| [app-state-ownership](app-state-ownership.md) | `AppState` 필드마다 도메인 사실·사용자 view 상태·실행 자원 분류와 수명·소유자·headless 유무 |
| [attach-behavior](attach-behavior.md) | attach(서버=loopback / 로컬-원격=클라이언트) · self-attach connector 진입/완료 검증 |
| [agent-runner](agent-runner.md) | task DAG executor + 동기화 primitive |

## 외부 프로그램 구동

| 문서 | 내용 |
|------|------|
| [external-interaction](external-interaction.md) | PTY 로 구동하는 외부 TUI(child Claude Code / codex 등)의 동작 때문에 생기는 함정 모음 |

## 테스트

| 문서 | 내용 |
|------|------|
| [e2e-tests](e2e-tests.md) | E2E 인스턴스 공유·환경 격리·timeout·진단·스냅샷 검증 |
| [unit-test-isolation](unit-test-isolation.md) | 홈·환경변수·파일·프로세스·시간을 격리하는 단위 테스트 규칙 |
| [guard-population](guard-population.md) | 검사 대상 수집, 빈 결과 검출, 파일 수 기준 갱신 |
| [guard-verification](guard-verification.md) | 실제 검사 경로와 합성 반례·변이 결과 확인 |
| [guard-relocation](guard-relocation.md) | 검사 파일 이동 전후의 경로·대상·실행 조건 확인 |

## Plugin

| 문서 | 내용 |
|------|------|
| [plugin-development](plugin-development.md) | 상대 설치 루트의 실행·CWD·자산 경계, plugin 제작 + 민감 데이터(regular · secret · keyring) + 호스트 런타임 계약 (실행 중 tasty 에 플러그인만 반복 갱신 §9.1 — 호스트 재빌드 불필요) |
| [paired-agent-handlers](paired-agent-handlers.md) | Claude/Codex 짝 핸들러의 공개 응답·번역·완료 알림 호환 경계 |
| [plugin-runtime](plugin-runtime.md) | 호스트가 plugin 프로세스에게 주는 런타임 계약 — 수명주기 · namespace · 채널 |
| [plugin-permissions](plugin-permissions.md) | namespace owner 기동 범위 · 권한 모델 |
| [plugin-packaging](plugin-packaging.md) | 서명 + staging 동기화 + 생태계 정책(자동 upgrade · 호환성 분류) |
