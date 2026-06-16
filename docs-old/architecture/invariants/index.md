# Invariants

본 디렉토리의 문서는 *깨지면 안 되는 시스템 약속* 을 기술한다.
코드 변경 시 가장 먼저 점검할 리스트.

| Invariant | 적용 시점 | 강제 기제 |
|-----------|----------|----------|
| [surface-cwd](surface-cwd.md) | surface 변환 / 생성 | Surface::source_cwd() 의 default 없음 — compile-time |
| (후보) focus-independence | CLI/IPC 모든 명령 | review 단계 — 아직 자동 검증 없음 |
| (후보) user-vs-agent-action | release 빌드 IPC 노출 | review + cfg(debug_assertions) |
