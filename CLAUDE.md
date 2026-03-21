# Tasty - 크로스 플랫폼 GPU 가속 터미널 에뮬레이터

## 프로젝트 개요

cmux(macOS 전용)에서 영감을 받은 크로스 플랫폼 GPU 가속 터미널 에뮬레이터.
Rust 기반 네이티브 GUI 앱으로 Windows, macOS, Linux를 모두 지원한다.
WezTerm/Alacritty와 유사한 접근이지만 AI 코딩 에이전트에 특화된 기능을 제공한다.

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
| IPC | Unix socket (Linux/macOS), Named pipe (Windows) |
| CLI | clap |

## 작업 규칙

### 문서 갱신 (필수)

**모든 작업 완료 시 docs를 반드시 갱신할 것.**

- 새 기능이 구현되면 `docs/features.md`에 해당 기능을 추가하고 설명을 붙인다.
- 기존 기능이 변경되면 해당 문서의 설명을 업데이트한다.
- `docs/index.md`의 목차도 함께 갱신한다.
- 구현 히스토리는 남기지 않는다. 현재 상태만 기술한다.

### 커밋 규칙

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
