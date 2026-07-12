# 외부 프로그램 구동 함정 (external-interaction)

tasty 가 PTY 로 **외부 프로그램**(child Claude Code / codex CLI / 기타 TUI)을 구동하고 입력을
주입·제어할 때, **외부 프로그램 측 동작**(bracketed-paste 감지, 입력 모드, read 타이밍 등)
때문에 생기는 **구조적 함정**을 검증된 사실로 모은다.

[`docs/design/systems/design-parity-notes.md`](../../design/systems/design-parity-notes.md) 의
dev-guide 판이다 — 대상이 "디자인 렌더와 egui 의 구조 차이" 가 아니라 "tasty 가 구동하는
외부 프로그램의 동작" 이라는 점만 다르다.

## 기록 원칙

- **추측 금지.** 실측(재현 실험)·소스 코드로 확인된 것만 적는다.
- 항목 형식: **증상 / 원인 / 임계·근거 / 처방(현재 상태) / 일반 교훈 / 날짜(절대값)**.
- docs 는 현재 상태를 기술한다 — "수정 전/후" 는 함정을 설명하는 데 필요한 만큼만.

## 노트

| 노트 | 요지 |
|------|------|
| [tell-autosubmit-paste-threshold](tell-autosubmit-paste-threshold.md) | child `tell` 단일라인이 63자(code point) 이상이면 자동제출 안 됨 — 본문+`\r` 한 burst 가 수신측 paste 휴리스틱에 흡수. 본문과 제출 `\r` 을 별도 PTY write 로 분리해 해결 |
| [bash-resize-first-byte-loss](bash-resize-first-byte-loss.md) | bash(MSYS)는 resize 후 다음 입력의 첫 1바이트를 지연 SIGWINCH 처리에 소모 (~25–33%, 시간 무관, cmd.exe 무결). 상류 버그 — 에이전트는 `\n` 프리픽스 또는 echo 검증으로 방어 |
