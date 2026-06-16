# Tasty 정체성과 불가침 원칙

> Tasty 가 *무엇이며 왜 그렇게 만들어졌는지*, 그리고 그 정체성에서 필연적으로 나오는 **개발 시 절대 어기면 안 되는 원칙** 을 정의한다. 모든 문서·API·기능 설계는 이 문서 위에 얹힌다. **작업 전 가장 먼저 읽는다.**

## 1. Tasty 는 무엇인가

### 제작자 입맛에 맞춘 터미널

Tasty 는 "제작자가 자기 업무 환경에 딱 맞는 터미널이 없어서, 입맛에 맞게 직접 만든 터미널" 이다. 이름 그대로(*tasty* = 입맛에 맞는) **범용 합의가 아니라 제작자의 워크플로가 기본값의 기준** 이다. 기본 기능과 기본 플러그인은 제작자가 평소 쓰던 것들을 모은 것이다.

동시에 Tasty 는 **AI Agent 와 개인화 환경에 맞게 커스터마이징할 수 있도록 열려 있다** — 테마·단축키·플러그인으로 각자의 환경에 맞춘다.

### 동시성이 핵심 (가장 중요한 정체성)

Tasty 의 가장 중요한 요소는 **동시성** 이다:

- **여러 AI Agent 가 여러 터미널에서 동시에 동작** 하고, 그것을 **오케스트레이션** 할 수 있다.
- 동시에 **사용자는 독립적으로 자기 작업** 을 한다.
- 즉 Tasty 는 *다중 에이전트 + 독립 사용자* 의 동시 작업에 초점을 맞춘 터미널이다.

이 동시성이 아래 모든 불가침 원칙의 뿌리다 — 에이전트들이 서로를, 또는 사용자를 침범하면 동시성이 깨진다.

### 크로스 플랫폼

Windows · macOS · Linux 모두 1급 지원.

## 2. 정체성에서 나오는 불가침 원칙

> 이 원칙이 깨지는 순간 Tasty 는 더 이상 "동시성에 초점을 맞춘, 신뢰할 수 있는 다중 에이전트 터미널" 이 아니게 된다.

### 2.1 사용자 행동 ↔ 에이전트 행동 분리 (soul)

동시성의 토대. 사용자와 에이전트가 같은 환경을 공유해도 서로를 침범하지 않아야 동시 작업이 성립한다.

- **사용자 행동** = 키보드/마우스/OS 입력 (사용자 본인). **에이전트 행동** = IPC/CLI (에이전트가 자기 작업을 수행).
- **① 에이전트 행동의 부수효과가 사용자 상태에 닿지 않는다** — 포커스 / 닫은 항목 히스토리(Ctrl+Shift+T) / 선택·스크롤·커서. 에이전트가 surface 100개를 열었다 닫아도 사용자의 복원 스택은 *사용자가 닫은 것만* 복원한다.
- **② 사용자 입력 재현은 release 에 없다** — 키/마우스 주입, popup 강제 open/close, 메뉴 강제 invoke, 프로그래밍적 포커스 전환은 release IPC/CLI 표면에 존재하지 않는다. debug 빌드(`#[cfg(debug_assertions)]`)에서만 격리 제공한다 (debug 코드는 `debug/` 디렉토리로 모은다 — 상세 [`dev-guide/debug-ipc.md`](dev-guide/debug-ipc.md)).
- **②의 목적 — debug 에서는 사용자 전용 동작도 IPC 로 구동해 자기검증한다.** ②의 재현 기능을 *debug 에 제공하는 이유* 는 agent 가 자기가 만든 기능을 스스로 검증하기 위해서다. debug 빌드에서는 사용자에게만 허용되는 동작(키/마우스 주입 `debug.inject_key`/`inject_mouse`, popup 강제 open/close `debug.popup.*`, 도구 메뉴 클릭 `debug.tool.invoke` 등)을 **IPC 로 구동** 할 수 있어, 사용자 입력 흐름까지 포함한 기능을 release(= 사용자 환경)와 격리된 채 검증한다. → dev-guide [독립 검증](dev-guide/independent-verification.md).
- **판단 기준**: *에이전트가 자기 작업을 하기 위해 필요한가(→ release) vs 사용자가 직접 하는 조작을 재현하는가(→ debug)*.

### 2.2 AI 에이전트 조작 가능성

2.1 의 positive 면. 에이전트가 자기 작업에 필요한 기능은 **반드시 제공한다.**

- 에이전트 기능(surface/tab/workspace 생성·닫기·조회, 클립보드, 알림, 파일 열기, 메타데이터 등)은 **IPC + CLI 양면** 으로 동작해야 한다. GUI 전용 에이전트 기능 금지.
- 에이전트가 문제를 직접 확인·조작할 기능이 부족하면, 그건 *Tasty 가 에이전트가 자유롭게 조작할 수 있는 터미널이 아니라는 의미* 이므로 기능을 추가한다.
- **headless 동작-우선**: 기능의 진실은 내부 동작이고 화면은 그 투영이다 → [`documentation-model.md`](documentation-model.md).

### 2.3 포커스 독립성

다중 에이전트가 포커스를 두고 다투지 않게. 2.1① 의 API 차원 구체화.

- 모든 명령은 대상을 **ID 로 직접 지정** 한다. list 는 **전 워크스페이스 순회**.
- 활성 상태 *조회* 는 허용(`focused` 필드 등), 활성 상태에 *의존* 하는 동작은 금지.
- **release 빌드엔 포커스를 바꾸는 API 가 없다** (상세 [`design/policies/focus.md`](design/policies/focus.md)).

### 2.4 개인화 / 하드코딩 금지

커스터마이징 가능해야 하므로 값을 코드에 박지 않는다.

- 길이 = `PhysicalPx`/`LogicalPx` 타입, 색 = `Theme`, 단축키 = `KeybindingSettings`, 문자열 = `t()`. 전부 경유한다 (상세는 CLAUDE.md 코드 정책).

### 2.5 크로스 플랫폼

- 플랫폼 분기는 `#[cfg(windows)]` / `#[cfg(target_os = "macos")]` 등. 한 OS 전용 기능도 다른 OS 컴파일이 깨지지 않게 조건부 컴파일한다.

## 관련

- [`documentation-model.md`](documentation-model.md) — 이 정체성(특히 headless 동작-우선)에서 도출된 문서 구조
- [`adr/0006-docs-taxonomy-behavior-first.md`](adr/0006-docs-taxonomy-behavior-first.md) — 문서 분류체계 결정
- [`concepts/ubiquitous-language.md`](concepts/ubiquitous-language.md) — 용어
- [`design/policies/focus.md`](design/policies/focus.md) — 포커스 독립성 운영 상세
