# AI 자체 검증 지침

tasty 를 개발하는 AI 에이전트가 UI/렌더링/입력을 **스스로 재현·검증** 하는 방법.

| 문서 | 내용 |
|------|------|
| [visual-verification](visual-verification.md) | 시각 변경 체크리스트 + 스크린샷 판단 휴리스틱 |
| [screenshot-methods](screenshot-methods.md) | `ui.screenshot`(debug) vs OS 캡처, 격리 실행 |
| [ipc-usage](ipc-usage.md) | IPC 로 조작·검증 + `\r`/`read_line` 함정 |
| [ime-testing](ime-testing.md) | `surface.ime_*` 로 한글/CJK 입력 시뮬레이션 |

> 검증은 커밋 전 직접 수행한다([dev-guide/self-verification](../dev-guide/self-verification.md)). 개발용 격리는 [dev-guide/independent-verification](../dev-guide/independent-verification.md).
