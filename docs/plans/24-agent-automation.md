# 24. 에이전트 자동화

## 개요

AI 에이전트 간 자동화 통합 기능. Claude Code 등 AI 코딩 에이전트가 다른 에이전트를 생성하고 제어하는 "에이전트가 에이전트를 제어하는 자동화"를 제공한다. tasty의 핵심 차별점이다.

## CLI 인터페이스

### Claude Code 전용 런처

```
tasty claude launch [--workspace <name>] [--directory <path>]
```

- 새 Surface를 생성하고 지정 디렉토리에서 Claude Code를 실행한다
- `--workspace`가 지정되면 해당 워크스페이스에 탭으로 추가, 없으면 새 워크스페이스 생성
- `--directory`가 지정되면 해당 디렉토리로 cd 후 실행

### 통합 실행 명령

```
tasty claude-run [--workspace <name>] [--directory <path>] [--prompt <text>]
```

하나의 명령으로 다음을 순차 실행:

1. 새 Surface 생성
2. 지정 디렉토리로 `cd`
3. Claude Code 실행
4. 프롬프트 입력 대기 감지
5. `--prompt`가 지정되면 자동으로 텍스트 전송

### 멀티 에이전트 배치 실행

```
tasty batch --config agents.toml
```

설정 파일로 여러 에이전트를 동시에 실행한다:

```toml
[[agent]]
name = "frontend"
directory = "./frontend"
prompt = "Fix all TypeScript errors"

[[agent]]
name = "backend"
directory = "./backend"
prompt = "Add input validation to all endpoints"

[[agent]]
name = "docs"
directory = "./docs"
prompt = "Update API documentation"
```

## 에이전트 상태 추적

PTY 출력 패턴과 훅을 조합하여 에이전트의 현재 상태를 감지한다.

### 상태 정의

| 상태 | 아이콘 | 감지 방법 |
|------|--------|-----------|
| 작업 중 | 🟢 | PTY 출력이 활발하게 변경 중 |
| 입력 대기 | 🔵 | 프롬프트 패턴 감지 (예: `❯`, `$`, `>`) |
| 권한 요청 | 🟡 | 권한 확인 프롬프트 패턴 감지 (예: `[Y/n]`, `Allow?`) |
| 에러 | 🔴 | 에러 패턴 감지 또는 프로세스 비정상 종료 |

### 사이드바 상태 표시

사이드바의 각 워크스페이스/Surface 옆에 상태 아이콘을 표시한다. 에이전트가 권한을 요청하고 있으면 사용자가 즉시 알 수 있다.

## 소켓 API

| 메서드 | 파라미터 | 설명 |
|--------|----------|------|
| `agent.launch` | `directory`, `prompt?`, `workspace?` | 에이전트 실행 |
| `agent.status` | `surface_id` | 에이전트 상태 조회 |
| `agent.batch` | `agents[]` | 멀티 에이전트 배치 실행 |
| `agent.list` | — | 실행 중인 에이전트 목록 |

### 요청 예시

```json
{
  "method": "agent.launch",
  "params": {
    "directory": "/home/user/project",
    "prompt": "Fix the login bug",
    "workspace": "bugs"
  }
}
```

### 응답 예시

```json
{
  "surface_id": 7,
  "workspace_id": 2,
  "status": "working"
}
```

## 내부 구현

### 프롬프트 감지

- PTY 출력을 정규식 패턴 집합과 매칭
- Claude Code 특화 패턴: `❯`, `claude>`, 프롬프트 관련 ANSI 시퀀스
- 사용자 정의 패턴 지원 (설정 파일)

### 상태 머신

```
Launching → Working ↔ Waiting → (권한 요청) → Working
                          ↓
                        Done / Error
```

- 출력 활동 기반 타이머로 Working/Waiting 전환
- 프로세스 종료 시 Done 또는 (exit code != 0) Error

### 배치 실행 관리

- 설정 파일의 각 에이전트를 병렬로 실행
- 각 에이전트마다 별도 Surface 생성
- 전체 진행 상황을 사이드바에서 한눈에 확인 가능

## 활용 시나리오

### 단일 에이전트 자동화

```bash
# 프로젝트 디렉토리에서 Claude Code 실행하고 프롬프트 자동 전송
tasty claude-run --directory ./my-project --prompt "Refactor the auth module"
```

### 멀티 에이전트 동시 작업

```bash
# 3개 에이전트를 동시에 실행
tasty batch --config agents.toml
# 사이드바에서 각 에이전트의 실시간 상태 확인
# 권한 요청(🟡) 발생 시 즉시 해당 Surface로 이동하여 승인
```

### 파이프라인 자동화

훅 시스템과 조합하여 순차 실행:

1. 에이전트 A가 코드 변경 완료 (process-exit 훅)
2. 자동으로 에이전트 B 실행 (테스트)
3. 테스트 성공 시 에이전트 C 실행 (배포)

## 구현 가능 여부

| OS | 가능 여부 | 비고 |
|----|-----------|------|
| Windows | 가능 | |
| macOS | 가능 | |
| Linux | 가능 | |
