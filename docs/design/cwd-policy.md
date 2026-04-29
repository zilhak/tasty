# CWD 정책

## 개요

터미널의 현재 작업 디렉토리(CWD)를 감지하여 새 탭/split 생성, 레이아웃 저장, 닫힌 항목 복원, 터미널 링크 해석 등에서 활용하는 메커니즘.

CWD 감지는 **모든 플랫폼에서 OSC 7에만 의존**한다.

## OSC 7

쉘이 프롬프트마다 `\e]7;file://hostname/path\e\\`을 보내면 즉시 `cached_cwd`에 반영. 비용 0, 이벤트 기반이므로 폴링 불필요.

### 쉘별 지원 현황

| 쉘 | OSC 7 | 비고 |
|----|-------|------|
| zsh | 기본 지원 | macOS 기본 쉘 |
| fish | 기본 지원 | 자동 |
| bash | 수동 설정 필요 | `PROMPT_COMMAND`에 추가 |
| PowerShell 7+ | 수동 설정 필요 | `prompt` 함수 수정 |

### bash에서 OSC 7 활성화

```bash
# ~/.bashrc에 추가
PROMPT_COMMAND='printf "\033]7;file://%s%s\033\\" "$HOSTNAME" "$PWD"'
```

## 쉘이 OSC 7을 보내지 않으면?

`cached_cwd`가 비어 있는 상태로 유지되며, 새 터미널 분할 시 부모 CWD 상속이 동작하지 않는다. 사용 중인 쉘이 OSC 7을 송신하도록 프롬프트를 설정하면 해결된다.

## 관련 코드

| 파일 | 역할 |
|------|------|
| `crates/tasty-terminal/src/vte_handler.rs` | OSC 7 수신 시 `cached_cwd` 즉시 갱신 |
| `crates/tasty-terminal/src/lib.rs` | `Terminal::get_cwd()`, `set_cached_cwd()` |
