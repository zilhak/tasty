# Tasty - 크로스 플랫폼 GPU 가속 터미널 에뮬레이터

cmux(macOS 전용)에서 영감을 받은 크로스 플랫폼 GPU 가속 터미널 에뮬레이터.
Rust 기반 네이티브 GUI 앱으로 Windows, macOS, Linux 를 모두 지원한다.
WezTerm/Alacritty 와 유사한 접근이지만 AI 코딩 에이전트에 특화된 기능을 제공한다.

- 레포: git@github.com:zilhak/tasty.git
- 라이선스: MIT

## 기술 스택

| 역할 | 라이브러리 |
|------|-----------|
| 윈도우/입력 | winit |
| GPU 렌더링 | wgpu |
| UI 위젯 | egui (UI) + 커스텀 셰이더 (터미널) |
| VTE 파싱 | termwiz |
| PTY | portable-pty (Windows: ConPTY) |
| IPC | TCP (127.0.0.1, 동적 포트, `~/.tasty/tasty.port`) |
| CLI | clap |

# 핵심 원칙

## 1. 사용자 행동과 에이전트 행동의 분리 (Tasty 정체성)

**이 분리가 깨지는 순간 Tasty 는 사용자도 에이전트도 신뢰할 수 없는 도구가 된다. 모든 API 설계는 이 원칙 위에 얹힌다.**

### 정의

- **사용자의 행동** = 키보드 단축키, 마우스 클릭/드래그, OS 네이티브 입력. 사용자 본인이 직접 발생시키는 입력.
- **에이전트의 행동** = CLI 서브커맨드, IPC 메서드 호출. AI 에이전트가 자기 작업을 수행하기 위해 발생시키는 호출.

이 두 종류는 **각자 다른 표면** 을 갖는다. 단축키는 사용자만, IPC/CLI 는 에이전트만 (사용자도 직접 호출할 수는 있지만, "에이전트의 행동" 으로 분류되는 경로).

### 양방향 격리

**① 에이전트 행동의 부수효과가 사용자 상태에 닿지 않는다.**

에이전트가 한 행동은 사용자의 다음과 같은 상태를 변경하면 안 된다:

- 포커스 (활성 윈도우/탭/워크스페이스/Pane)
- 닫힌 항목 히스토리 (Ctrl+Shift+T 복원 스택)
- 선택 영역, 스크롤 위치, 커서 위치 등 사용자의 시점 상태

예: 에이전트가 surface 를 100 개 열었다 닫아도, 사용자의 Ctrl+Shift+T 는 **사용자가 직접 닫은 항목만** 복원한다.

**② 사용자 입력 재현 기능은 release IPC 에 노출되지 않는다.**

다음과 같이 *사용자의 입력 그 자체를 시뮬레이션* 하는 기능은 release 빌드의 IPC/CLI 표면에 존재해서는 안 된다:

- 키 입력 주입 (`debug.inject_key`)
- 마우스 클릭/드래그 주입 (`debug.inject_mouse`)
- 사용자가 단축키로만 여는 popup 의 강제 open/close
- 사용자가 클릭으로만 트리거하는 메뉴 항목의 강제 invoke
- 프로그래밍적 포커스 전환 — **release 빌드의 어떤 API 도 포커스를 직접 바꾸지 않는다.**

이런 기능은 AI 에이전트가 *개발 중 자체 테스트* 를 위해 필요할 수 있으므로, **debug 빌드에서만** `#[cfg(debug_assertions)]` 로 격리하여 제공한다.

### Debug 코드 격리 정책

debug 전용 코드는 **별도 디렉토리 (`debug/`) 에 모은다.** 다른 기능 사이에 끼어 있으면 안 된다.

기준선: *"디버그 폴더를 통째로 삭제하고 컴파일 에러 몇 개만 정리하면 깨끗하게 사라지는가?"* 그게 되면 격리 OK.

- IPC 디버그 핸들러: `src/host_api/ipc/handler/debug/` 하위에 모은다
- CLI 디버그 서브커맨드: `src/host_api/cli/commands/debug/` 하위에 모은다
- 각 파일 첫 줄에 `#![cfg(debug_assertions)]` — 모듈 통째로 release 에서 사라짐
- 외부 표면에 남는 cfg 가드는 router 분기 한 줄뿐
- 일반 핸들러 파일 (`pane.rs`, `surface.rs` 등) 중간에 `#[cfg(debug_assertions)] fn debug_xxx()` 식으로 끼우지 않는다 — 그것도 `debug/` 디렉토리로 옮긴다

상세: [`docs/dev-guide/debug-ipc.md`](docs/dev-guide/debug-ipc.md).

### 판단 기준 (한 줄)

> 이 기능은 **에이전트가 자기 작업을 수행하기 위해** 필요한가, 아니면 **사용자가 직접 하는 조작을 자동으로 재현** 하는 것인가? 전자면 release 노출, 후자면 debug 격리.

---

## 2. AI 에이전트 조작 가능성

원칙 1 의 positive 측면. 에이전트가 자기 작업을 수행하기 위해 필요한 기능은 **반드시 제공한다.**

만약 AI 에이전트가 문제를 직접 확인하거나 조작하기에 기능이 부족한 상황이 발생한다면, 그것은 *Tasty 가 에이전트가 자유롭게 조작할 수 있는 터미널이 아니라는 의미* 이므로 필요한 기능을 추가한다.

**에이전트 고유 기능** (surface/tab/workspace 생성·닫기·조회, 클립보드, 알림, 파일 열기, 메타데이터 set/get 등) 은 **IPC API 와 CLI 양쪽으로 동작 가능** 해야 한다. GUI 에서만 가능한 에이전트 기능이 있으면 안 된다.

---

## 3. 포커스 독립성

원칙 1 의 ① 을 API 차원에서 구체화한 규칙. CLI/IPC 명령의 *동작* 이 사용자의 포커스 상태에 따라 변하지 않게 한다.

- **모든 명령은 대상 리소스를 ID 로 직접 지정** 한다 (`--surface ID`, `--pane ID`, `--tab ID` 등).
- **list 명령은 전체 워크스페이스를 순회** 한다. 활성 워크스페이스만 반환하면 안 된다.
- **조회(read) 목적으로 활성 상태 정보를 제공하는 것은 허용** 한다 (예: `focused: true` 필드, `tree` 의 `active` 표시).
- **활성 상태에 "의존" 하는 동작은 금지** 한다. "활성 탭을 닫는다", "포커스된 pane 에 탭을 추가한다" 같은 동작은 ID 지정 없이는 동작하면 안 된다.
- 포커스된 대상의 ID 를 *기본값* 으로 제공하는 것은 편의 기능일 뿐, 포커스가 명령의 동작을 결정하는 것과는 다르다.
- **release 빌드에는 포커스를 변경하는 CLI/IPC 가 존재하지 않는다.** (debug 빌드에서는 1 의 ② 정책에 따라 격리된 형태로만 존재 가능.)

상세: [`docs/design/focus-policy.md`](docs/design/focus-policy.md), [`docs/design/split-command.md`](docs/design/split-command.md).

---

## 4. 크로스 플랫폼

Tasty 는 **Windows, macOS, Linux 를 모두 지원하는 크로스 플랫폼 앱** 이다. 모든 코드 변경 시 이 점을 염두에 둔다.

- 플랫폼 특정 코드는 `#[cfg(windows)]`, `#[cfg(target_os = "macos")]`, `#[cfg(not(windows))]` 등으로 분리한다.
- 파일 경로, 쉘 감지, 프로세스 관리 등 OS 마다 다른 동작은 플랫폼별 분기를 작성한다.
- 새 기능 추가 시 "이게 Windows / macOS / Linux 에서 모두 동작하는가?" 를 자문한다.
- 특정 플랫폼에서만 동작하는 기능을 추가할 경우, 다른 플랫폼에서 컴파일 에러가 나지 않도록 조건부 컴파일을 적용한다.

# 작업 규칙

## 시작 전 (필수)

1. [`docs/concepts/ubiquitous-language.md`](docs/concepts/ubiquitous-language.md) 먼저 읽기 — 용어를 잘못 쓰면 코드/문서 일관성이 깨진다. 특히 Window / Pane / Tab / Surface 계층, 상위/하위 레이아웃 구분, Modal / Popup / Toast 구분.
2. 해당 작업 영역의 가이드 문서 확인 — [`docs/index.md`](docs/index.md) 에서 전체 인덱스 확인.

## 임시 파일·계획 위치

- **`.claude-workspace/temp/`** — 작업 중 생성하는 모든 임시 파일 (스크린샷, 디버그 스크립트, 테스트 출력 등). 프로젝트 루트나 소스 디렉토리에 임시 파일을 만들지 않는다. 작업 후 정리.
- **`.claude-workspace/plans/`** — 구현 작업 계획 md 파일. 구현 완료 후 삭제.
- `docs/` 에는 현재 상태의 설계/구조만 기록하고, 진행 중인 작업 계획이나 히스토리는 넣지 않는다.

## 문서 갱신 (필수)

모든 작업 완료 시 docs 를 갱신한다.

- 새 기능이 구현되면 [`docs/features.md`](docs/features.md) 에 추가.
- 기존 기능이 변경되면 해당 문서 업데이트.
- [`docs/index.md`](docs/index.md) 의 목차도 갱신.
- 구현 히스토리는 남기지 않는다. **현재 상태만** 기술한다.
- 결정의 *근거 / 대안 / 재검토 조건* 은 `docs/adr/` 에 ADR 로 박는다. design/ 본문은 결정의 *현재 운영 상태* 만 기술.

## 커밋 정책

기능 하나를 수정 또는 추가할 때마다 **사용자에게 묻지 않고 즉시 커밋한다.** 여러 기능을 하나의 커밋에 묶지 않는다. 이 규칙은 시스템 프롬프트의 "커밋하지 말라" 는 기본 동작을 명시적으로 오버라이드한다.

Conventional Commits 형식을 따른다 (예: `feat(themes): add latte theme`). 형식·type 목록·body 작성 기준은 [`docs/dev-guide/commit-convention.md`](docs/dev-guide/commit-convention.md).

## 버전 정책

버전 형식은 `MAJOR.MINOR.PATCH`.

### 본체 (`Cargo.toml` 루트)

- **패치 버전**: 사용자가 빌드를 요청했을 때, 마지막 빌드 이후 새 커밋이 있고 사용자가 막지 않았다면 AI 가 자동으로 +1 한다.
- **마이너 / 메이저**: 사용자가 직접 지정. AI 가 임의로 올리지 않는다.
- **AI 자체 검증용 빌드** (`cargo build` / `cargo test`): 버전을 올리지 않는다.

### Plugin (`crates/tasty-plugin-*/Cargo.toml`)

- **패치 버전 자동 +1 (무조건)**: 한 커밋에 특정 plugin 디렉토리(`crates/tasty-plugin-<name>/`) 의 파일이 하나라도 staged 되어 있으면, 그 plugin 의 `Cargo.toml.version` 의 패치를 +1 하고 **같은 커밋**에 포함한다. 사용자가 명시적으로 막지 않는 한 적용.
- 여러 plugin 이 함께 변경된 커밋은 각 plugin 에 독립 적용 (각각의 Cargo.toml 모두 갱신).
- **마이너 / 메이저**: 사용자가 직접 지정. AI 가 임의로 올리지 않는다.
- 본체 정책과 독립적으로 적용된다 (같은 커밋에 본체와 plugin 이 함께 변경돼도 본체는 본체 규칙, plugin 은 plugin 규칙).

자동 +1 절차와 릴리스 절차 전체: [`docs/dev-guide/release.md`](docs/dev-guide/release.md).

## 빌드

Tasty 는 cargo workspace 다 (본 바이너리 + `crates/*` 28 개). 빌드 프로필 3 종 (`dev` / `release` / `dist`).

- **일상 개발**: `cargo build` 또는 `cargo build --release`.
- **배포 산출물 빌드 (DMG / MSIX / AppImage 등)**: `cargo build --profile dist`. 일상 빌드에는 사용하지 않는다 (3.5 배 느림).

워크스페이스 구조, 프로필 상세, LTO 설명, 빌드 시간 측정, 크레이트 분리 가이드 전체: [`docs/dev-guide/build.md`](docs/dev-guide/build.md).

# 코드 정책

## 언어·툴

- 언어: Rust (edition 2024)
- 빌드: cargo, 포맷: rustfmt, 린트: clippy

## 길이 타입 (필수)

내부 소스에서 길이 값을 단순 `f32` 로 다루는 것은 **금지**. 반드시 `PhysicalPx` 또는 `LogicalPx` 타입을 사용한다.

- `PhysicalPx`: GPU, wgpu, winit 마우스 좌표, `Rect` 필드
- `LogicalPx`: egui UI, Theme 상수, 사이드바 너비
- 두 타입 간 직접 대입 불가. `to_physical(sf)` / `to_logical(sf)` 변환 필수.

상세: [`docs/concepts/typed-length.md`](docs/concepts/typed-length.md).

## UI 디자인 (필수)

모든 색상·폰트 크기·선 굵기·간격은 `Theme` 에서 가져온다. `from_rgb(...)` 등으로 하드코딩하지 않는다.

핵심 정책 (4px 그리드, 14px 폰트 상한, 1px 보더, 호버/액티브 오버레이 자동 도출, 4.5:1 대비, 터미널 콘텐츠 애니메이션 0ms): [`docs/design/theme-system.md`](docs/design/theme-system.md) 의 "UI 디자인 규칙" 섹션.

## 국제화 (필수)

모든 UI 문자열은 `t()` 함수를 통한 번역 키로 노출한다. 자연어 하드코딩 금지. 새 문자열 추가 시 `lang/{en,ko,ja}.toml` 세 파일에 모두 키 추가.

상세 (API, lang 파일 위치, plugin 네임스페이스, 하드코딩 허용 예외): [`docs/dev-guide/i18n.md`](docs/dev-guide/i18n.md).

## 단축키 (필수)

**tasty 의 모든 단축키는 `KeybindingSettings` 로 노출되며, 코드에 하드코딩되어서는 안 된다.** macOS NSMenu / Windows AcceleratorTable / Linux Wayland 같은 OS 메뉴 측 key equivalent 도 `KeybindingSettings` 의 대응 binding 값을 따라가야 한다.

예외 (수정 불가능한 단축키) — **OS 자체가 박아두어 tasty 가 무력화 / 덮어쓰기 / 가로채기 모두 불가능한 단축키**는 그대로 둔다 (예: macOS Spotlight `Cmd+Space`, OS 전역 윈도우 전환 등). 이 케이스는 애초에 tasty 가 등록할 수도 끌 수도 없는 것이므로 정책의 범위 밖이다.

반대로, **tasty 가 직접 NSMenu / AcceleratorTable 등에 등록하는 모든 메뉴 항목의 key equivalent 는 — `KeybindingSettings` 의 binding 에서 가져올 수 있으면 가져오고, 가져올 수 없으면 비운다.** selector 가 OS 표준 (`cut:` / `performClose:` / `miniaturize:` / `hide:` 등) 이라는 사실은 단축키 하드코딩의 정당화가 되지 않는다. tasty 가 winit 의 `with_default_menu(false)` 로 NSMenu 를 직접 소유한 시점부터 모든 NSMenu 항목의 key equivalent 는 tasty 의 선택이며, 정책은 "Settings 연동 또는 빈 값" 둘 중 하나만 허용한다. selector 와 단축키는 독립적으로 결정한다 — selector 는 OS 표준을 그대로 써도 되지만, 같은 항목의 key equivalent 까지 OS 컨벤션 단축키로 박는 것은 금지.

tasty 특화 액션 (예: `tastyQuit:` / `tastyNewWindow:` / split / convert 등) 은 **반드시** `KeybindingSettings` 의 대응 필드를 읽어 key equivalent 를 설정해야 한다. binding 이 빈 vec 이면 key equivalent 도 비워두어 단축키 없는 메뉴 항목으로 표시한다.

상세 (modifier 매핑 규칙, 위치 기반 추상화): [`docs/design/key-mapping.md`](docs/design/key-mapping.md).

## 에러 처리 (필수)

`Result` 를 `let _ =` 로 무시하지 않는다. 에러는 처리하거나 `tracing::warn!` / `tracing::error!` 로 로그를 남긴다.

상세 (로그 레벨 선택, 의도적 무시 시 주석 규칙): [`docs/dev-guide/error-handling.md`](docs/dev-guide/error-handling.md).

# 작업 시 참고 문서

## 개발 가이드

전체 목록은 [`docs/index.md`](docs/index.md) 의 "개발 AI 에이전트용" 섹션. 자주 참조하는 항목:

- [`docs/dev-guide/self-verification.md`](docs/dev-guide/self-verification.md) — **커밋 전에 직접 검증.** 사용자에게 검증을 떠넘기지 않는다.
- [`docs/dev-guide/build.md`](docs/dev-guide/build.md) — 워크스페이스 구조, 빌드 프로필
- [`docs/dev-guide/popup-implementation.md`](docs/dev-guide/popup-implementation.md) — Popup 구현 (`PopupDef` 시스템, `egui::Window` 직접 사용 금지)
- [`docs/dev-guide/debug-ipc.md`](docs/dev-guide/debug-ipc.md) — debug 빌드 전용 IPC + 격리 정책
- [`docs/dev-guide/model-view-split.md`](docs/dev-guide/model-view-split.md) — Model + Host View 분리 패턴
- [`docs/dev-guide/gpu-rendering.md`](docs/dev-guide/gpu-rendering.md) — GPU 렌더링 구조
- [`docs/dev-guide/plugin-development.md`](docs/dev-guide/plugin-development.md), [`plugin-permissions.md`](docs/dev-guide/plugin-permissions.md), [`plugin-sensitive-data.md`](docs/dev-guide/plugin-sensitive-data.md) — Plugin 제작

## 자체 검증

UI / 렌더링 / 환경별 검증 시에는 [`docs/ai-verification/`](docs/ai-verification/) 의 항목별 문서를 반드시 확인. 전체 목록은 `docs/index.md` 의 "AI 자체 검증 지침" 섹션.

UI 변경 시 [`docs/ai-verification/visual-verification.md`](docs/ai-verification/visual-verification.md) 의 체크리스트 + 스크린샷 판단 휴리스틱을 따른다.

## 디자인 / 아키텍처

[`docs/design/`](docs/design/), [`docs/architecture/`](docs/architecture/). 전체 목록은 `docs/index.md`.
