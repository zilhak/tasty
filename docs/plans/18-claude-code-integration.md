# 18. Claude Code 통합

## cmux 구현 방식

- Claude Code idle/permission 훅 연동
- Claude 활동 추적 (SurfaceActivitySnapshot)
- 사이드바 Claude 상태 표시
- `cmux claude-teams` 런처
- Read mark 시스템

## 크로스 플랫폼 구현 방안

### 핵심 통합 포인트

tasty의 존재 이유. Claude Code와의 긴밀한 통합이 핵심 가치.

### 1. 훅 연동

Claude Code는 훅(hooks)을 통해 외부 프로그램에 이벤트를 전달한다.

```json
// ~/.claude/settings.json
{
  "hooks": {
    "notification": [
      { "command": "tasty notify --workspace $TASTY_WORKSPACE_ID" }
    ]
  }
}
```

tasty CLI가 설치되어 있으면 OS에 무관하게 동작한다.

### 2. 활동 상태 추적

PTY 출력을 모니터링하여 Claude Code의 상태를 추적:

| 상태 | 감지 방법 | 사이드바 표시 |
|------|----------|-------------|
| 작업 중 | PTY 출력이 계속 발생 | 🟢 녹색 인디케이터 |
| 입력 대기 | 프롬프트 패턴 감지 / termwiz OSC 파싱 | 🔵 파란색 인디케이터 |
| 권한 요청 | tool use approval 패턴 | 🟡 노란색 인디케이터 + 시스템 알림 |
| 에러 | 에러 패턴 감지 | 🔴 빨간색 인디케이터 |
| 완료 | 프롬프트 복귀 | ⚪ 회색 인디케이터 |

### 3. 사이드바 상태 표시

GUI 렌더링이므로 풍부한 상태 표시가 가능:

- 색상 인디케이터 (활동 상태)
- 진행률 바 (가능한 경우)
- 마지막 활동 타임스탬프
- 토큰 사용량 (감지 가능한 경우)
- 현재 작업 요약

### 4. 전용 런처

```bash
tasty claude launch [--workspace <name>] [--directory <path>]
```

새 워크스페이스를 만들고 Claude Code를 바로 실행.

### 5. 멀티 에이전트 워크플로우

```bash
tasty batch --config agents.toml
```

```toml
[[agent]]
name = "frontend"
directory = "./packages/web"
command = "claude --task 'Fix the login page'"

[[agent]]
name = "backend"
directory = "./packages/api"
command = "claude --task 'Add rate limiting'"
```

여러 에이전트를 한 번에 실행하고 사이드바에서 상태를 모니터링.

### 6. 원클릭 권한 승인

GUI 앱이므로 권한 요청 시:
- 시스템 알림으로 표시
- 사이드바에서 클릭 한 번으로 승인 (PTY에 'y' + Enter 전송)
- 멀티 에이전트 일괄 승인 옵션

## 최적화 전략

- **상태 추적 디바운스**: PTY 출력 기반 상태 감지를 적절한 간격으로 제한한다. 매 줄이 아닌 일정 간격(예: 200ms)으로 상태를 판단한다.
- **훅 실행 비동기**: CLI 훅 호출을 별도 프로세스/스레드에서 실행하여 PTY I/O 블로킹을 방지한다. 훅 결과는 콜백으로 처리한다.
- **배치 런처 병렬화**: 여러 에이전트를 동시에 시작할 때 PTY 생성을 병렬로 처리한다. 순차 생성 대비 시작 시간을 단축한다.
- **패턴 매칭 최적화**: 상태 감지 정규식을 사전 컴파일(`regex::Regex::new`을 한 번만 호출)하여 매 PTY 출력마다 컴파일 비용을 피한다.

## 구현 가능 여부

| OS | 가능 여부 | 비고 |
|----|-----------|------|
| Windows | ✅ 가능 | Claude Code가 Windows 지원 |
| macOS | ✅ 가능 | |
| Linux | ✅ 가능 | |

Claude Code 자체가 크로스 플랫폼이므로 tasty 통합도 OS에 무관하게 동작.
