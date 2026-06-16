# ADR (Architecture Decision Record) 인덱스

아키텍처/정책 결정의 *근거·대안·재검토 조건* 을 기록한다. design/ 문서가 "지금 어떻게 동작하나" 를 기술한다면, ADR 은 "왜 그렇게 결정했나" 를 기술한다.

- 신규 작성: [`template.md`](template.md) 양식을 따른다. 파일명 `XXXX-<slug>.md`, 번호는 0001 부터 4 자리.
- **Accepted 후에는 Status 만 갱신한다.** 본문 변경이 필요하면 새 ADR 로 Supersede 한다.
- 커밋 형식: `dev-guide/commit-convention.md` *(재작성 예정)* 의 "ADR 커밋" 항목.

## 목록

| # | Title | Status | Date | Tags |
|---|-------|--------|------|------|
| 0001 | [시스템 트레이 — 전 OS best-effort 지원](0001-system-tray-best-effort.md) | Accepted | 2026-06-17 | system-tray, platform, background, cross-platform, windows, macos, linux |
| 0002 | [VTE 파싱을 입력 스레드 밖 파서 스레드로 분리](0002-vte-parsing-off-input-thread.md) | Accepted | 2026-06-15 | performance, terminal, threading, input-latency, vte |
| 0003 | [네이티브 데코 대신 CSD(Client-Side Decorations) 채택](0003-client-side-decorations.md) | Accepted | 2026-06-15 | window, csd, titlebar, cross-platform, winit, macos, windows, linux |
| 0004 | [IPC transport = 127.0.0.1 loopback TCP (동적 포트)](0004-ipc-transport-tcp.md) | Accepted | 2026-06-16 | ipc, transport, tcp, loopback, security, trust-boundary, cross-platform |
| 0005 | [memory secret 영역은 "안전 보관소" 가 아니다](0005-memory-secret-not-a-vault.md) | Accepted | 2026-06-16 | memory, secret, security, encryption, plugin, trust-boundary |
| 0006 | [문서 분류체계 — 동작 우선(behavior-first), 화면 종속](0006-docs-taxonomy-behavior-first.md) | Accepted | 2026-06-16 | docs, taxonomy, headless, screen-spec, design-system, behavior-first |
| 0007 | [attach 는 원격을 대상으로 한다 (로컬 self-attach 는 debug 격리)](0007-attach-targets-remote.md) | Accepted | 2026-06-17 | attach, remote, debug-isolation, cli, user-agent-separation, security |
