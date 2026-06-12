# Linux 시스템 트레이 — 미지원 정책

> 결정 근거 / 대안 / 재검토 조건은 [`adr/0001-linux-system-tray-unsupported.md`](../adr/0001-linux-system-tray-unsupported.md) 참조. 본 문서는 현재 정책의 운영 측 상세만 기술한다.

Tasty는 **Linux에서 시스템 트레이 아이콘을 제공하지 않는다.** macOS는 메뉴바, Windows는 알림 영역(트레이)을 제공한다.

## 백그라운드 동선

Tasty는 **마지막 윈도우 닫기 시 `set_minimized(true)`로 태스크바에 유지**한다 (Windows와 동일 동작). Linux에서 백그라운드 실행/빠른 접근 동선은 트레이가 아니라 태스크바로 해결한다.

## 코드 측면

`Cargo.toml`의 `tray-icon` 의존성은 **`cfg(windows)` 한정**으로 유지한다 (`[target.'cfg(windows)'.dependencies]` 블록). Linux용 `cfg(target_os = "linux")` 분기를 추가하지 않는다.
