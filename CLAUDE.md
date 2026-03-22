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

## 개발 환경 주의사항

### Python 실행

이 Windows 환경에서 `python3` 명령은 정상 동작하지 않는다 (exit code 49, `-c` 인자 무시). Python이 필요할 때는 반드시 `python`을 사용할 것.

```bash
# 잘못됨 - 동작하지 않음
python3 -c "print('hello')"

# 올바름
python -c "print('hello')"
```

### TCP 통신 도구

`ncat`, `nc`, `netcat`이 설치되어 있지 않다. IPC 테스트 등에서 TCP 통신이 필요하면 Python socket을 사용할 것.

```python
import socket, json
s = socket.socket()
s.settimeout(5)
s.connect(('127.0.0.1', PORT))
s.sendall((json.dumps(request) + '\n').encode())
data = s.recv(4096)
s.close()
```

### Windows 프로세스 정리

`process.kill()`은 Windows에서 자식 프로세스를 종료하지 않는다. 프로세스 트리 전체를 종료하려면 `taskkill /F /T /PID`를 사용해야 한다. 테스트 harness의 Drop에서 이미 적용되어 있다.
