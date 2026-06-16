# Policies

코드가 따라야 할 규칙의 설계 명세.

| 문서 | 설명 |
|------|------|
| [focus.md](focus.md) | 포커스 정책 — 윈도우/모달 간 입력 라우팅 규칙 |
| [cwd.md](cwd.md) | CWD 정책 — OSC 7 기반 CWD 감지 (모든 플랫폼 공통) |
| [key-mapping.md](key-mapping.md) | 키 매핑 설계 — OS별 물리적 키 위치 매핑, 프리셋, 캡처/매칭 규칙 |
| [keybinding-presets.md](keybinding-presets.md) | 키바인딩 프리셋 — 프리셋 정의/저장 구조 |
| [busy-indicator.md](busy-indicator.md) | 실행 중 표시 — 탭/워크스페이스 busy 판정 정책, 시각 표시, 플랫폼별 foreground 감지 |
| [linux-system-tray.md](linux-system-tray.md) | Linux 시스템 트레이 미지원 정책 — 운영 측 상세 (백그라운드 동선, `tray-icon` cfg(windows) 한정). 결정 근거는 ADR-0001 |
| [lua-hooks.md](lua-hooks.md) | Lua hook 설계 — host 전용·observe-only·event matrix·사용자-직접 변경 의미 |
