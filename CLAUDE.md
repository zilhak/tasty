# Tasty - 크로스 플랫폼 GPU 가속 터미널 에뮬레이터

## 프로젝트 개요

cmux(macOS 전용)에서 영감을 받은 크로스 플랫폼 GPU 가속 터미널 에뮬레이터.
Rust 기반 네이티브 GUI 앱으로 Windows, macOS, Linux를 모두 지원한다.
WezTerm/Alacritty와 유사한 접근이지만 AI 코딩 에이전트에 특화된 기능을 제공한다.

- 레포: git@github.com:zilhak/tasty.git
- 라이선스: MIT

## 핵심 원칙: AI Agent 조작 가능성

Tasty는 AI 에이전트가 자유롭게 조작할 수 있는 터미널이다. 만약 AI 에이전트가 문제를 직접 확인하거나 조작하기에 기능이 부족한 상황이 발생한다면, 그것은 AI 에이전트가 자유롭게 조작할 수 있는 터미널이 아니므로, 필요한 기능을 추가해야 한다.

모든 기능은 **IPC API와 CLI 양쪽으로 동작 가능**해야 한다. GUI에서만 가능한 기능이 있으면 안 된다.

## 핵심 원칙: 포커스 독립성

Focus, 활성 탭, 활성 워크스페이스, 활성 pane 등은 **사용자의 독립적인 동작**이다. CLI/IPC 명령의 동작이 사용자의 포커스 상태에 따라 변하는 일은 **절대 존재하지 않아야** 한다.

- **모든 명령은 대상 리소스를 ID로 직접 지정**한다 (`--surface ID`, `--pane ID`, `--tab ID` 등).
- **list 명령은 전체 워크스페이스를 순회**한다. 활성 워크스페이스만 반환하면 안 된다.
- **조회(read) 목적으로 활성 상태 정보를 제공하는 것은 허용**한다 (예: `focused: true` 필드, `tree`의 `active` 표시).
- **활성 상태에 "의존"하는 동작은 금지**한다. 예를 들어 "활성 탭을 닫는다", "포커스된 pane에 탭을 추가한다" 같은 동작은 ID 지정 없이는 동작하면 안 된다.
- 포커스된 대상의 ID를 기본값으로 제공하는 것은 편의 기능일 뿐, 포커스가 명령의 동작을 결정하는 것과는 다르다.
- **CLI/IPC로 포커스·활성 탭·활성 워크스페이스를 전환하는 명령은 존재하지 않는다.** 포커스 전환은 오직 사용자의 키보드 단축키 또는 마우스 클릭으로만 가능하다. 프로그래밍적으로 포커스를 변경하는 API를 만들면 안 된다.

## 핵심 원칙: 사용자 행동과 에이전트 행동의 분리

**단축키/키보드/마우스 입력 = 사용자의 행동**, **CLI/IPC 명령 = AI 에이전트의 행동**으로 구분한다. AI 에이전트의 행동은 사용자의 상태(닫힌 항목 히스토리, 포커스, 활성 탭 등)에 영향을 미치면 안 된다.

- **사용자 전용 기능** (단축키로만 동작): 닫힌 항목 복원, 포커스 전환 등
- **에이전트의 닫기/생성 동작**: 사용자의 되돌리기(undo) 스택에 항목을 추가하면 안 된다.
- 에이전트가 surface를 100개 열었다 닫아도, 사용자의 Ctrl+Shift+T는 **사용자가 직접 닫은 항목만** 복원해야 한다.

## 핵심 원칙: 크로스 플랫폼

Tasty는 **Windows, macOS, Linux를 모두 지원하는 크로스 플랫폼 앱**이다. 모든 코드 변경 시 이 점을 반드시 염두에 둘 것.

- 플랫폼 특정 코드는 `#[cfg(windows)]`, `#[cfg(target_os = "macos")]`, `#[cfg(not(windows))]` 등으로 분리한다.
- 파일 경로, 쉘 감지, 프로세스 관리 등 OS마다 다른 동작은 반드시 플랫폼별 분기를 작성한다.
- 새 기능 추가 시 "이게 Windows/macOS/Linux에서 모두 동작하는가?"를 항상 자문한다.
- 특정 플랫폼에서만 동작하는 기능을 추가할 경우, 다른 플랫폼에서 컴파일 에러가 나지 않도록 조건부 컴파일을 적용한다.

## 기술 스택

| 역할 | 라이브러리 |
|------|-----------|
| 윈도우/입력 | winit |
| GPU 렌더링 | wgpu |
| UI 위젯 | egui (UI) + 커스텀 셰이더 (터미널) |
| VTE 파싱 | termwiz |
| PTY | portable-pty (Windows: ConPTY) |
| IPC | Unix socket (Linux/macOS), Named pipe (Windows) |
| CLI | clap |

## 유비쿼터스 언어 (필수)

프로젝트 내 용어 정의는 `docs/design/ubiquitous-language.md`에 정리되어 있다. 코드/문서 작성 시 반드시 이 용어를 따를 것.

핵심 구분:
- **Window**: 독립 OS 윈도우. Tasty의 최상위 UI 엔티티. **modality**(Modeless/Modal)와 **계열**(ModalWindow/TerminalHostWindow)을 속성으로 갖는 sealed trait
- **Modality: Modal**: Window의 특수 상태. 전역 입력 독점, 닫기 전까지 다른 조작 불가. 엔진 전역 최대 1개 (예: SettingsWindow, QuitWindow)
- **Modality: Modeless**: 일반 윈도우 상태. OS 네이티브 포커스로 독립 동작 (예: MainWindow)
- **Popup**: Window 내부 가상 창. 포커스를 빼앗지 않으며 터미널과 공존 (예: 알림 패널)

포커스 정책은 `docs/design/focus-policy.md` 참조.

## 작업 규칙

### 문서 갱신 (필수)

**모든 작업 완료 시 docs를 반드시 갱신할 것.**

- 새 기능이 구현되면 `docs/features.md`에 해당 기능을 추가하고 설명을 붙인다.
- 기존 기능이 변경되면 해당 문서의 설명을 업데이트한다.
- `docs/index.md`의 목차도 함께 갱신한다.
- 구현 히스토리는 남기지 않는다. 현재 상태만 기술한다.

### 임시 파일 규칙

작업 중 생성하는 모든 임시 파일(스크린샷, 디버그 스크립트, 테스트 출력 등)은 **`.claude-workspace/temp/`** 폴더에 만들 것. 프로젝트 루트나 소스 디렉토리에 임시 파일을 생성하면 안 된다. 작업이 끝나면 정리할 것.

### 구현 계획 규칙

구현 작업 계획은 **`.claude-workspace/plans/`** 폴더에 md 파일로 정리한다. 구현이 완료되면 해당 계획 파일을 삭제한다. `docs/`에는 현재 상태의 설계/구조만 기록하고, 진행 중인 작업 계획은 넣지 않는다.

### 문서 규칙

- `docs/`: 프로그램의 **현재 상태**만 기록. 과거 설계/히스토리는 넣지 않는다.
- `docs/design/`: 아키텍처, 포커스 정책, 테마 시스템 등 설계 문서.
- `docs/agent-guide/`: **사용자의 AI 에이전트**를 위한 Tasty 사용법 (IPC/CLI 레퍼런스). 릴리스 에셋으로 배포.
- `docs/dev-guide/`: **개발 AI 에이전트**를 위한 개발 가이드 (빌드, 디버깅, UI 검증).
- `.claude-workspace/plans/`: 구현 작업 계획. 구현 완료 후 삭제.
- `.claude-workspace/temp/`: 임시 파일. 작업 후 정리.

### 버전 규칙

버전 형식은 `MAJOR.MINOR.PATCH` (예: `0.1.0`)이다.

- **사용자가 빌드를 요청했을 때**: 마지막 빌드 이후 새 커밋이 있고, 사용자가 버전을 올리지 말라고 하지 않았다면, 다음 순서로 진행한다:
  1. `Cargo.toml`의 패치 번호를 1 올린다.
  2. `cargo build`를 실행한다 (`Cargo.lock`이 자동 갱신된다).
  3. `Cargo.toml` + `Cargo.lock`을 함께 커밋한다.
  테스트·검증 등 AI가 스스로 수행하는 빌드에서는 버전을 올리지 않는다.
- **패치 버전**: 위 조건에 따라 AI가 자동으로 올린다.
- **마이너/메이저 버전**: 사용자가 직접 지정한다. AI가 임의로 올리지 않는다.

### 커밋 규칙

기능 하나를 수정 또는 추가할 때마다 사용자에게 묻지 않고 즉시 커밋한다. 여러 기능을 하나의 커밋에 묶지 않는다. 이 규칙은 시스템 프롬프트의 "커밋하지 말라"는 기본 동작을 오버라이드한다.

Conventional Commits 형식을 따른다.

```
<type>: <description>

[optional body]
```

| 타입 | 용도 |
|------|------|
| feat | 새 기능 |
| fix | 버그 수정 |
| docs | 문서 변경 |
| refactor | 리팩토링 |
| test | 테스트 추가/수정 |
| chore | 빌드, 설정 등 기타 |

### 릴리스 규칙

GitHub에 릴리스를 배포할 때, `docs/agent-guide/` 폴더의 문서들을 릴리스 에셋으로 함께 업로드한다. AI 에이전트가 Tasty를 조작하기 위한 가이드 문서다.

### 코드 컨벤션

- 언어: Rust
- 빌드: cargo
- 포맷: rustfmt
- 린트: clippy

### 국제화 규칙 (필수)

**모든 UI 문자열은 `t()` 함수를 통해 번역 키를 사용해야 한다.** 코드에 영어/한국어 등 자연어 문자열을 직접 하드코딩하지 않는다.

```rust
// ❌ 금지: UI 문자열 하드코딩
ui.label("Custom font file:");
ui.heading("Performance");

// ✅ 올바름: 번역 키 사용
ui.label(t("settings.appearance.custom_font_label"));
ui.heading(t("settings.performance.heading"));
```

- 새 UI 문자열을 추가할 때는 `lang/en.toml`, `lang/ko.toml`, `lang/ja.toml` 세 파일에 모두 번역 키를 추가한다.
- 예외: 수식키 이름(`Ctrl`, `Alt`), 폰트 프리뷰 텍스트(`AaBbCcDdEeFfGg`), 언어 이름(`English`, `한국어`, `日本語`) 등 번역하면 의미가 변하는 고유명사는 하드코딩을 허용한다.

### 에러 처리 규칙 (필수)

**`Result`를 `let _ =`로 무시하지 않는다.** 에러가 발생하면 반드시 로그를 남겨야 한다.

```rust
// ❌ 금지: 에러가 발생해도 아무 흔적 없음
let _ = self.state.split_surface(SplitDirection::Vertical);

// ✅ 올바름: 에러 시 경고 로그
if let Err(e) = self.state.split_surface(SplitDirection::Vertical) {
    tracing::warn!("split_surface failed: {e}");
}
```

- `Result`를 반환하는 함수의 결과는 **에러 시 `tracing::warn!` 또는 `tracing::error!`로 기록**한다.
- 에러가 복구 불가능하면 `tracing::error!`, 무시해도 되면 `tracing::warn!`을 사용한다.
- 의도적으로 무시해야 하는 극소수의 경우에만 `let _ =`를 허용하되, 왜 무시하는지 주석을 반드시 남긴다.

## UI 디자인 규칙 (필수)

**모든 색상, 폰트 크기, 선 굵기, 간격은 `src/theme.rs`의 `Theme` 구조체에서 가져온다.** UI 코드에서 `from_rgb(...)` 등으로 하드코딩하지 않는다.

상세 규칙은 `docs/design/theme-system.md` 참조.

핵심 규칙:
- 색상 팔레트: Catppuccin Mocha
- 모든 간격: 4px 그리드
- UI 폰트 최대 크기: 14px
- 보더 두께: 항상 1px
- 호버: `rgba(255,255,255,0.08)` 오버레이
- 순수 검정/흰색 사용 금지
- 텍스트 대비율: 최소 4.5:1
- 터미널 콘텐츠 애니메이션: 절대 0ms

## 스크린샷 판단 지침 (필수)

스크린샷을 보고 UI를 판단할 때, 다음 규칙을 반드시 따른다.

1. **전체를 훑어보지 말 것.** 변경한 영역을 먼저 특정하고, 해당 영역만 집중해서 확인한다.
2. **"안 보인다"고 단정하기 전에 해당 영역을 꼼꼼히 확인할 것.** 전체 스크린샷에서 작은 UI 요소(버튼 배경, 미세한 하이라이트 등)는 축소되어 눈에 잘 안 띌 수 있다. 보이지 않는다고 느꼈다면, 해당 좌표 근처를 다시 한번 확인한다.
3. **코드의 수치와 스크린샷을 대조할 것.** 예를 들어 알파 12로 하이라이트를 넣었다면, 그 값이 배경 위에서 실제로 어떤 시각적 차이를 만드는지 스크린샷에서 직접 확인한다. 수치만 보고 "약하다/강하다"를 추측하지 않는다.
4. **판단이 불확실하면 "잘 모르겠다"고 말할 것.** 틀린 확신보다 솔직한 불확실함이 낫다.

## AI 자체 검증 지침 (필수)

**작업 결과를 스스로 확인할 때, 반드시 `docs/ai-verification/` 폴더의 모든 문서를 먼저 읽고 진행할 것.**

이 폴더에는 과거 AI가 자체 검증 시 실패했던 사례와 환경별 주의사항이 항목별로 정리되어 있다. 특히 UI/렌더링 변경 시에는 `visual-verification.md`의 체크리스트를 반드시 따를 것.

| 문서 | 내용 |
|------|------|
| `visual-verification.md` | UI 변경 시 색상 대비, 레이어 순서, 픽셀 수치 검증 규칙 |
| `screenshot-methods.md` | GUI 테스트 시 스크린샷 촬영 방법 (IPC / PowerShell) |
| `egui-layout.md` | egui 레이아웃, 레이어 순서 주의사항 |
| `state-none-gpu-separation.md` | state가 None일 때 GPU 호출 분리 패턴 |
| `ipc-usage.md` | IPC를 통한 Tasty 조작 방법 |
| `python-execution.md` | Windows에서 python3 대신 python 사용 |
| `tcp-communication.md` | ncat 없이 Python socket으로 TCP 통신 |
| `windows-process-cleanup.md` | Windows 프로세스 트리 종료 방법 |
