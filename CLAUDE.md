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

Tasty 의 정체성과 거기서 나오는 **불가침 원칙** 전문은 [`docs/identity.md`](docs/identity.md) — **작업 전 필독.** 아래는 코드 작업 시 즉시 적용하는 집행 요지다 (배경·근거는 identity.md).

1. **사용자 행동 ↔ 에이전트 행동 분리** — 에이전트 행동(IPC/CLI)의 부수효과가 사용자 상태(포커스 / 닫은 항목 히스토리 / 선택·스크롤·커서)에 닿지 않는다. 사용자 입력 재현(키/마우스 주입, popup 강제 open/close, 메뉴 강제 invoke, 포커스 전환)은 release 에 없고 `#[cfg(debug_assertions)]` debug 격리로만 존재한다. debug 핸들러는 **모듈 선언에 cfg 가 붙은 별도 파일**로 모은다(디렉토리 이름이 아니라 그 성질이 기준이다 — `src/adapters/ipc/handler/` 의 `debug.rs`·`popup.rs` 등). 판단 기준: *에이전트가 자기 작업에 필요한가(→ release) vs 사용자 조작을 재현하는가(→ debug)*. 상세 [`docs/dev-guide/debug-ipc.md`](docs/dev-guide/debug-ipc.md).
2. **AI 에이전트 조작 가능성** — 에이전트 기능(surface/tab/workspace 생성·닫기·조회, 클립보드, 알림, 파일 열기, 메타데이터 등)은 **IPC + CLI 양면** 으로 동작해야 한다. GUI 전용 에이전트 기능 금지. 부족하면 추가한다.
3. **포커스 독립성** — 모든 명령은 대상을 ID 로 직접 지정, list 는 전 워크스페이스 순회, 활성 상태 의존 동작 금지, release 엔 포커스 변경 API 없음. 상세 [`docs/design/policies/focus.md`](docs/design/policies/focus.md).
4. **크로스 플랫폼** — Windows/macOS/Linux 모두 1급. 플랫폼 분기는 `#[cfg(...)]`, 한 OS 전용 기능도 다른 OS 컴파일이 깨지지 않게.

# 작업 규칙

## 시작 전 (필수)

1. [`docs/identity.md`](docs/identity.md) 먼저 읽기 — Tasty 정체성과 불가침 원칙. 모든 설계의 축.
2. [`docs/concepts/ubiquitous-language.md`](docs/concepts/ubiquitous-language.md) — 용어를 잘못 쓰면 코드/문서 일관성이 깨진다. 특히 Window / Pane / Tab / Surface 계층, 상위/하위 레이아웃 구분, Modal / Popup / Toast 구분.
3. 해당 작업 영역의 가이드 문서 확인 — [`docs/index.md`](docs/index.md) 에서 전체 인덱스 확인.

## 임시 파일·계획 위치

- 작업 중 생성하는 임시 파일(스크린샷, 디버그 스크립트, 테스트 출력 등)과 구현 작업 계획 md 는 **프로젝트 루트나 소스 디렉토리에 만들지 않는다.** 구체적인 위치는 `.gitignore` 대상인 로컬 작업 폴더이며, 그 경로는 커밋되지 않는 **로컬 전용 지침**(Claude Code 가 세션 시작 시 이 파일과 함께 로드하는 프로젝트 로컬 CLAUDE.md)이 정한다 — git 에 존재하지 않는 경로는 이 파일에 적지 않는다(아래 "소스 주석의 TODO 파일 및 디자인 changelog 인용 금지" 와 같은 원칙).
- 임시 파일은 작업 후 정리하고, 계획 md 는 구현 완료 후 삭제한다.
- `docs/` 에는 현재 상태의 설계/구조만 기록하고, 진행 중인 작업 계획이나 히스토리는 넣지 않는다.

## 문서 갱신 (필수)

모든 작업 완료 시 docs 를 갱신한다.

- 새 기능이 구현되면 [`docs/features/`](docs/features/index.md) 의 해당 카테고리 문서에 추가 (인덱스는 `docs/features/index.md`).
- 기존 기능이 변경되면 해당 문서 업데이트.
- **사용자에게 보이는 동작**(메뉴·단축키·설정 키·CLI 명령·설치 절차)이 바뀌면 공개 사이트의 사용자 가이드 [`site/content/`](site/content/index.md) 도 같은 커밋에서 갱신한다. 가이드는 `docs/` 명세와 독자가 다르다(설치해서 쓰는 사람) — 소스 경로·ADR·IPC 메서드명을 넣지 않는다. 영어 번역(`site/content/en/`)을 손봤으면 `--stamp` 한다 ([`docs/dev-guide/site.md`](docs/dev-guide/site.md)).
- 해당 카테고리의 인덱스(예: [`docs/features/index.md`](docs/features/index.md), [`docs/dev-guide/index.md`](docs/dev-guide/index.md))를 갱신. [`docs/index.md`](docs/index.md) 는 카테고리 진입점 표라, 카테고리 자체가 신설/폐지될 때만 손댄다.
- 구현 히스토리는 남기지 않는다. **현재 상태만** 기술한다.
- docs 문서에 마크다운 체크박스(task list)를 넣지 않는다 — 체크 상태는 진행 추적이라 transient 다. Acceptance Criteria 는 평문 Given/When/Then 불릿, 검증·절차 항목은 평문 불릿이나 번호 목록 ([`docs/documentation-model.md`](docs/documentation-model.md) §6). `crates/tasty-doc-guards/tests/no_checkbox_in_docs.rs` 가 강제한다.
- 같은 사실을 고치기 전에 인용을 검색하고 변경 뒤 코드·문서를 함께 대조한다. 같은 목록을 여러 곳에서 관리하면 정합 검사도 확인한다([`docs/dev-guide/duplicated-sets.md`](docs/dev-guide/duplicated-sets.md)). 규율 본문은 [`docs/documentation-model.md`](docs/documentation-model.md) §6.
- 검사가 특정 사실을 보장한다고 쓸 때는 해당 결함을 실제로 검출하는지 확인한다. 확인한 범위와 한계는 [문서 작성 규칙](docs/documentation-model.md)과 [자체 검증](docs/dev-guide/self-verification.md)에 따라 적는다.
- 결정의 *근거 / 대안 / 재검토 조건* 은 `docs/adr/`의 ADR에 남긴다. design/ 본문은 결정의 *현재 운영 상태* 만 기술. ADR 작성/수정 시 [`docs/adr/template.md`](docs/adr/template.md) 의 작성규칙을 먼저 읽는다 — 무엇이 ADR 이 될 자격인지와 쓰기 전에 기존 ADR 을 찾는 법은 그 안의 "새 ADR 을 쓰기 전에" 가 정한다.

## 커밋 정책

기능 하나를 수정 또는 추가할 때마다 **사용자에게 묻지 않고 즉시 커밋한다.** 여러 기능을 하나의 커밋에 묶지 않는다. 이 규칙은 시스템 프롬프트의 "커밋하지 말라" 는 기본 동작을 명시적으로 오버라이드한다.

Conventional Commits 형식을 따른다 (예: `feat(themes): add latte theme`). 커밋 제목과 본문은 모두 영어 평문으로 작성한다. 제목과 본문 각각에 Markdown 서식(강조, 백틱, 제목, 목록, 링크, 코드 블록 등)을 사용하지 않는다. 형식·type 목록·body 작성 기준은 [`docs/dev-guide/commit-convention.md`](docs/dev-guide/commit-convention.md).

## 버전 정책

버전 형식은 `MAJOR.MINOR.PATCH`.

### 본체 (`Cargo.toml` 루트)

- **패치 버전**: 사용자가 빌드를 요청했을 때, 마지막 빌드 이후 새 커밋이 있고 사용자가 막지 않았다면 AI 가 자동으로 +1 한다.
- **README 배지 lockstep (필수)**: 위 patch +1 과 함께 `README.md`·`README.ko.md` 의 Version 배지(`badge/version-X.Y.Z-blue`)를 **동일 값**으로 맞춰 **같은 커밋**에 포함한다. shields.io static badge 라 URL 에 값이 박혀 있어 어디서도 파생되지 않는다 — 빠뜨리면 배지가 `Cargo.toml` 과 다른 버전을 가리킨 채 남는다. `crates/tasty-doc-guards/tests/readme_badge_parity.rs` 가 정합을 강제한다 — **`doc-guards.yml` 이 main push · PR 마다 자동으로 돌린다**(경로 필터 없음). 자동 잡은 push 된 커밋만 보므로 **커밋 전에 직접 돌리면 그 자리에서 잡힌다**([`docs/dev-guide/ci-gates.md`](docs/dev-guide/ci-gates.md)).
- **마이너 / 메이저**: 사용자가 직접 지정. AI 가 임의로 올리지 않는다.
- **AI 자체 검증용 빌드** (`cargo build` / `cargo test`): 버전을 올리지 않는다.

### Plugin (`crates/tasty-plugin-*/Cargo.toml`)

매니페스트가 있는 번들 플러그인의 제품 내용이 달라지면 패치 버전을 올린다.
해당 디렉터리뿐 아니라 normal/build 의존성의 변경도 포함한다.
저장소 안 path 의존성은 workspace 밖에 있어도 검사한다.

- Rust는 rustfmt 정규화 후 비교하고 테스트 전용 내용은 제외한다.
- 버전 선언 자체는 내용 변경 증거에서 제외한다. 파일 수나 커밋 type으로 면제하지 않는다.
- 주석만 바뀐 경우에도 보수적으로 버전 증가를 요구할 수 있다. 사유를 확인하고 처리한다.
- Cargo.toml, 매니페스트, Cargo.lock을 함께 갱신해 같은 커밋에 넣는다.
- 여러 플러그인이 영향을 받으면 각각 적용한다. minor/major는 사용자가 정한다.
- 서명 파일은 빌드 산출물이며 커밋하지 않는다.
- 나누어 push할 때는 이미 원격에 발행된 버전과 현재 내용을 다시 비교한다.
  pre-commit 통과가 최종 배포 버전 검사를 대신하지 않는다.

내용 비교, 보조 도구 준비, 비교할 원격 커밋을 읽지 못했을 때의 처리는
[릴리스 절차](docs/dev-guide/release.md#플러그인-버전-비교)를 따른다.
설치된 플러그인을 실행 중인 앱에 반영하는 절차는
[플러그인 개발](docs/dev-guide/plugin-development.md)의 §9.1을 따른다.

## 빌드

Tasty 는 cargo workspace 다 (본 바이너리 + `crates/*` 60 개 — 그중 `tasty-plugin-sdk-wasm` 은 workspace `exclude`). 빌드 프로필 3 종 (`dev` / `release` / `dist`).

> 위 크레이트 수는 [`docs/architecture/index.md`](docs/architecture/index.md) 가 정본이고 이 문장은 그 복제본이다 — `crates/tasty-doc-guards/tests/architecture_crate_list_complete.rs` 가 `crates/` 실측과 대조하므로 고칠 때 함께 움직인다. 두 README 의 Workspace 배지·본문도 같은 개수를 사용하고 `readme_badge_parity` 가 본다. 세는 대상은 **`crates/` 바로 아래에서 `Cargo.toml` 을 가진 디렉토리 수**다 — `exclude` 된 것도 세고, 레포 루트의 본 바이너리 크레이트는 안 센다.

- **일상 개발**: `cargo build` 또는 `cargo build --release`.
- **배포 산출물 빌드 (DMG / MSI / AppImage 등)**: `cargo build --profile dist`. 일상 빌드에는 사용하지 않는다 (3.5 배 느림).

워크스페이스 구조, 프로필 상세, LTO 설명, 빌드 시간 측정, 크레이트 분리 가이드 전체: [`docs/dev-guide/build.md`](docs/dev-guide/build.md).

## Conductor/에이전트 병렬 작업 시 빌드·검증 명령

변경에 맞는 검사를 직접 실행한다. 자동 실행 범위와 빌드 조합은
[CI 가이드](docs/dev-guide/ci-gates.md)에서 확인한다.
워크플로 파일에 명령이 있다는 사실만으로 해당 커밋에서 실행됐다고 보고하지 않는다.

| 목적 | 명령 |
|---|---|
| 개발 빌드 | `cargo build` |
| 플러그인을 포함한 개발 빌드 | `cargo build --workspace` |
| release 컴파일·빌드 확인 | `cargo build --release` |
| lint | `cargo clippy --workspace --all-targets --locked` |
| 포맷 | `cargo fmt --check` |
| 문서 검사 | `cargo test -p tasty-doc-guards --locked` |
| 전체 테스트 | `cargo test --workspace --locked` |
| 파일 크기·예외 총합 | `bash scripts/check-file-size.sh`와 `bash scripts/check-frozen-sum-ratchet.sh` |

셸 검사는 `check-intent-discipline.sh`, `check-allow-reason.sh`,
`check-shared-walk-ratchet.sh`, `check-shell-assets.sh`를 사용한다.
필요한 보조 도구가 없거나 오래됐으면 먼저 준비한다. 실제 위반인지 확인하기 전에
상한이나 예외를 바꿔 검사를 통과시키지 않는다.

루트의 `cargo build`만으로 플러그인 실행 파일은 갱신되지 않는다.
`PROFILE=debug just build-plugins`로 빌드와 스테이징을 하고,
검증할 실행 파일과 번들 사본이 새 코드인지 확인한다.
공개키와 번들 준비는 빌드 전에 수행한다.
`cp -p` 등으로 오래된 시각을 복원했다면 내용이 바뀌었어도 갱신이 생략될 수 있다.

workspace에서 제외된 `tasty-plugin-sdk-wasm`은 `--manifest-path`로 따로 검사한다.
사이트는 Cargo 대상이 아니므로 [사이트 가이드](docs/dev-guide/site.md)의 절차를 따른다.
Cargo에는 별도 의존성 설치 단계가 없고 build/test 때 필요한 것을 가져온다.
일반적인 소스 변경 검증에 무조건 전체 캐시를 지우지는 않는다.

빌드·테스트 통과와 실제 실행 확인은 구분한다.
[자체 검증](docs/dev-guide/self-verification.md)에 따라 격리된 debug 인스턴스로 시나리오를 재현한다.

# 코드 정책

## 언어·툴

- 언어: Rust (edition 2024)
- 빌드: cargo, 포맷: rustfmt, 린트: clippy

## 길이 타입 (필수)

내부 소스에서 길이 값을 단순 `f32` 로 다루는 것은 **금지**. 반드시 `PhysicalPx` 또는 `LogicalPx` 타입을 사용한다.

- `PhysicalPx`: GPU, wgpu, winit 마우스 좌표, `Rect` 필드
- `LogicalPx`: egui UI, Theme 상수, 사이드바 너비
- 두 타입 간 직접 대입 불가. `to_physical(sf)` / `to_logical(sf)` 변환 필수. 사각형은 `PhysicalRect` / `LogicalRect` 짝으로 네 변을 한 번에 변환한다.
- **강제 수단이 둘이다**: 두 좌표계를 *섞는* 것은 컴파일러가 막고, 변환을 *빠뜨리는* 것(`.value()` 로 벗겨서 `× ppp` / `÷ scale_factor`)은 `src/dpi_conversion_guard.rs` 가 막는다. 타입만으로는 후자가 안 잡힌다.

상세: [`docs/concepts/typed-length.md`](docs/concepts/typed-length.md).

## UI 디자인 (필수)

모든 색상·폰트 크기·선 굵기·간격은 `Theme` 에서 가져온다. `from_rgb(...)` 등으로 하드코딩하지 않는다.

핵심 정책 (4px 그리드, 14px 폰트 상한, 1px 보더, 호버/액티브 오버레이 자동 도출, 4.5:1 대비, 터미널 콘텐츠 애니메이션 0ms): [`docs/design/systems/theme.md`](docs/design/systems/theme.md) 의 "UI 디자인 규칙" 섹션.

## 갤러리 완전성 · gallery-first (필수)

**갤러리(`crates/tasty-gallery`)는 본체의 모든 UI 컴포넌트를 노출한다 — cut 금지.** 디자인 산출물이 일부 컴포넌트를 카탈로그에서 생략해도 갤러리에서 빼지 않는다(생략은 디자인 측 결함 → 디자인 request 로 보강). **새 modal/popup/공용 위젯은 gallery-first** — 디자인 수령 → 갤러리 specimen → 본체 반영 순서로 만든다.

상세·근거: [ADR-0635](docs/adr/0635-shared-design-and-theme.md) · [`docs/dev-guide/gallery-first.md`](docs/dev-guide/gallery-first.md) · [`docs/design/policies/gallery-completeness.md`](docs/design/policies/gallery-completeness.md).

**UI 를 디자인에 정합시킬 때는 두 축을 함께 충족한다 — 둘 다 필수다:**

1. **구조 축** — [`docs/design/systems/design-parity-notes.md`](docs/design/systems/design-parity-notes.md) 의 **구조 전사(structural transcription)** 원칙과 [`docs/design/systems/design-gallery-mapping.md`](docs/design/systems/design-gallery-mapping.md)(jsx↔함수 매핑)을 읽는다 — egui flow 로 눈대중 흉내 내지 말고 디자인의 레이아웃 구조(grid·컬럼·패딩·정렬)를 **컴포넌트 단위·소스코드 단위로** 1:1 전사한다.
2. **토큰 축** — 위 "UI 디자인 (필수)" 의 [`theme.md` "UI 디자인 규칙"](docs/design/systems/theme.md) 을 **반드시 함께** 적용한다: 색·폰트크기·선굵기·간격은 전부 디자인 토큰(=`Theme`)에서 가져오고 raw px·`from_rgb` 하드코딩 금지, 4px 그리드·14px 폰트 상한·1px 보더.

구조만 맞추고 토큰을 빠뜨리거나 그 반대면 정합이 절반만 된다. 두 축은 독립적으로 어긋날 수 있으므로 매 작업에서 둘 다 점검한다.

## 국제화 (필수)

모든 UI 문자열은 `t()` 함수를 통한 번역 키로 노출한다. 자연어 하드코딩 금지. 새 문자열 추가 시 `lang/{en,ko,ja}.toml` 세 파일에 모두 키 추가.

상세 (API, lang 파일 위치, plugin 네임스페이스, 하드코딩 허용 예외): [`docs/dev-guide/i18n.md`](docs/dev-guide/i18n.md).

## 단축키 (필수)

**tasty 의 모든 단축키는 `KeybindingSettings` 로 노출되며, 코드에 하드코딩되어서는 안 된다.** macOS NSMenu / Windows AcceleratorTable / Linux Wayland 같은 OS 메뉴 측 key equivalent 도 `KeybindingSettings` 의 대응 binding 값을 따라가야 한다.

예외 (수정 불가능한 단축키) — **OS 자체가 박아두어 tasty 가 무력화 / 덮어쓰기 / 가로채기 모두 불가능한 단축키**는 그대로 둔다 (예: macOS Spotlight `Cmd+Space`, OS 전역 윈도우 전환 등). 이 케이스는 애초에 tasty 가 등록할 수도 끌 수도 없는 것이므로 정책의 범위 밖이다.

반대로, **tasty 가 직접 NSMenu / AcceleratorTable 등에 등록하는 모든 메뉴 항목의 key equivalent 는 — `KeybindingSettings` 의 binding 에서 가져올 수 있으면 가져오고, 가져올 수 없으면 비운다.** selector 가 OS 표준 (`cut:` / `performClose:` / `miniaturize:` / `hide:` 등) 이라는 사실은 단축키 하드코딩의 정당화가 되지 않는다. tasty 가 winit 의 `with_default_menu(false)` 로 NSMenu 를 직접 소유한 시점부터 모든 NSMenu 항목의 key equivalent 는 tasty 의 선택이며, 정책은 "Settings 연동 또는 빈 값" 둘 중 하나만 허용한다. selector 와 단축키는 독립적으로 결정한다 — selector 는 OS 표준을 그대로 써도 되지만, 같은 항목의 key equivalent 까지 OS 컨벤션 단축키로 박는 것은 금지.

tasty 특화 액션 (예: `tastyQuit:` / `tastyNewWindow:` / split / convert 등) 은 **반드시** `KeybindingSettings` 의 대응 필드를 읽어 key equivalent 를 설정해야 한다. binding 이 빈 vec 이면 key equivalent 도 비워두어 단축키 없는 메뉴 항목으로 표시한다.

상세 (modifier 매핑 규칙, 위치 기반 추상화): [`docs/design/policies/key-mapping.md`](docs/design/policies/key-mapping.md).

## 에러 처리 (필수)

`Result` 를 `let _ =` 로 무시하지 않는다. 에러는 처리하거나 `tracing::warn!` / `tracing::error!` 로 로그를 남긴다.

상세 (로그 레벨 선택, 의도적 무시 시 주석 규칙): [`docs/dev-guide/error-handling.md`](docs/dev-guide/error-handling.md).

## 소스 주석의 TODO 파일 및 디자인 changelog 인용 금지 (필수)

추적 파일에 로컬 작업 폴더 경로·티켓 번호·일회성 디자인 changelog를 근거로 남기지 않는다.
새 clone에서 읽을 수 없는 자료이기 때문이다. 이유를 직접 쓰거나 현재 가이드·ADR을 연결한다.
할 일 표시인 `TODO:`와 `TODO(범위):`는 사용할 수 있다.

빌드 산출물, 사용자 홈의 런타임 경로, 설정 키 같은 식별자는 필요한 문서에서 설명한다.
금지 형태를 입력으로 쓰는 검사는 해당 패턴만 예외로 둔다.
세부 범위와 인용 방법은 [문서 작성 규칙](docs/documentation-model.md#경로와-코드-인용)을 따른다.
`no_todo_file_citation`은 규칙을 검사하지만 모든 산문의 의미를 판단하지는 않는다.

# 작업 시 참고 문서

## 개발 가이드

전체 목록은 [`docs/dev-guide/index.md`](docs/dev-guide/index.md). 자주 참조하는 항목:

- [`docs/dev-guide/self-verification.md`](docs/dev-guide/self-verification.md) — **커밋 전에 직접 검증.** 사용자에게 검증을 떠넘기지 않는다.
- [`docs/dev-guide/build.md`](docs/dev-guide/build.md) — 워크스페이스 구조, 빌드 프로필
- [`docs/dev-guide/popup-implementation.md`](docs/dev-guide/popup-implementation.md) — Popup 구현 (`PopupDef` 시스템, `egui::Window` 직접 사용 금지)
- [`docs/dev-guide/debug-ipc.md`](docs/dev-guide/debug-ipc.md) — debug 빌드 전용 IPC + 격리 정책
- [`docs/dev-guide/model-view-split.md`](docs/dev-guide/model-view-split.md) — Model + Host View 분리 패턴
- [`docs/dev-guide/gpu-rendering.md`](docs/dev-guide/gpu-rendering.md) — GPU 렌더링 구조
- [`docs/dev-guide/agent-runner.md`](docs/dev-guide/agent-runner.md) — **Task DAG executor.** 여러 AI 에이전트가 같은 tasty 인스턴스를 공유할 때 쓰는 협업 primitive 6종(`agent.*`: task DAG · barrier · semaphore · lease · reducer · rate-limit). 기획/인터페이스는 [`docs/features/agent-collaboration/index.md`](docs/features/agent-collaboration/index.md)
- [`docs/dev-guide/plugin-development.md`](docs/dev-guide/plugin-development.md)(민감 데이터는 그 문서 §6 의 [민감 데이터 절](docs/dev-guide/plugin-development.md#민감-데이터--regular--secret--keyring-선택)), [`plugin-permissions.md`](docs/dev-guide/plugin-permissions.md) — Plugin 제작. **번들 plugin 코드를 고친 뒤 실행 중 tasty 인스턴스에 재빌드·재시작 없이 반영할 때도 이 문서 §9.1** (빌드 → 재서명 → `disable` → `upgrade-builtins` → `enable` 순서) — "새 plugin 만들기"가 아니라도 반드시 먼저 확인한다.

## 자체 검증

UI / 렌더링 / 환경별 검증 시에는 [`docs/ai-verification/`](docs/ai-verification/) 의 항목별 문서를 반드시 확인. 전체 목록은 [`docs/ai-verification/index.md`](docs/ai-verification/index.md).

UI 변경 시 [`docs/ai-verification/screenshot-methods.md`](docs/ai-verification/screenshot-methods.md#시각-판정-체크리스트) 의 "시각 판정 체크리스트" 절(체크리스트 + 스크린샷 판단 휴리스틱)을 따른다.

## 디자인 / 아키텍처

[`docs/design/`](docs/design/), [`docs/architecture/`](docs/architecture/). 전체 목록은 `docs/index.md`.
