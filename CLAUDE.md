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

## 작업 규칙

### 문서 갱신 (필수)

**모든 작업 완료 시 docs를 반드시 갱신할 것.**

- 새 기능이 구현되면 `docs/features.md`에 해당 기능을 추가하고 설명을 붙인다.
- 기존 기능이 변경되면 해당 문서의 설명을 업데이트한다.
- `docs/index.md`의 목차도 함께 갱신한다.
- 구현 히스토리는 남기지 않는다. 현재 상태만 기술한다.

### 임시 파일 규칙

작업 중 생성하는 모든 임시 파일(스크린샷, 디버그 스크립트, 테스트 출력 등)은 **`.claude/temp/`** 폴더에 만들 것. 프로젝트 루트나 소스 디렉토리에 임시 파일을 생성하면 안 된다. 작업이 끝나면 정리할 것.

### 커밋 규칙

기능 하나를 수정 또는 추가할 때마다 커밋한다. 여러 기능을 하나의 커밋에 묶지 않는다.

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

### 코드 컨벤션

- 언어: Rust
- 빌드: cargo
- 포맷: rustfmt
- 린트: clippy

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
