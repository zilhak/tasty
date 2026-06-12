# ADR-0001: Linux 시스템 트레이 미지원

- **Status**: Accepted
- **Date**: 2026-06-12
- **Tags**: linux, system-tray, platform, dependencies

> 결정 자체는 2026-05-01 commit `c18c32f3` ("docs: record decision to not implement Linux system tray") 에서 기록되었다. 본 ADR 은 그 결정을 표준 양식으로 응축한 것이다.

## Context

Tasty 는 크로스 플랫폼 터미널이다. macOS 는 메뉴바, Windows 는 알림 영역 (트레이) 을 OS 표준 패턴으로 제공하며, Windows 트레이는 이미 구현되어 있다 (commit `e5fe9924` — `tray-icon` 크레이트, Show Window / New Window / Quit 메뉴). 남은 질문은 Linux 트레이 지원 여부였다.

Linux 는 데스크톱 환경 (DE) 별로 트레이 지원이 분열되어 있다:

| 데스크톱 환경 | 트레이 지원 |
|--------------|-------------|
| GNOME (Ubuntu·Fedora 기본) | **기본 미지원**. 3.26부터 legacy SystemTray 제거. AppIndicator 확장 추가 설치 필요 |
| KDE Plasma | StatusNotifierItem 네이티브 |
| XFCE | panel-systray / SNI 플러그인 |
| Cinnamon, MATE | 지원 |

GNOME 이 데스크톱 Linux 점유율 1위인데 기본 미지원이라, 트레이를 구현해도 "절반의 사용자에게는 보이지 않는 기능" 이 된다.

한편 Tasty 의 백그라운드 동선은 트레이에 의존하지 않는다. 마지막 윈도우 닫기 시 `set_minimized(true)` 로 태스크바에 유지하므로 (Windows 와 동일 동작), 백그라운드 실행/빠른 접근이 이미 태스크바로 해결되어 있다.

## Decision

**Linux 에서 시스템 트레이 아이콘을 제공하지 않는다.** 백그라운드 동선은 `set_minimized(true)` + 태스크바 유지로 해결한다. `Cargo.toml` 의 `tray-icon` 의존성은 `cfg(windows)` 한정으로 유지하고, Linux 분기를 추가하지 않는다.

## Consequences

- **얻은 것**: Linux 런타임 의존성 0 (`libayatana-appindicator3-1` / `libdbusmenu-glib4` 불필요). DE 별 테스트 매트릭스 (KDE / GNOME+확장 / XFCE / Cinnamon) 부담 제거. `tray-icon` 은 `cfg(windows)` 한정 유지.
- **잃은 것**: KDE Plasma 등 트레이를 네이티브 지원하는 DE 의 사용자도 트레이 아이콘을 받지 못한다.
- **운영 비용 / 유지 부담**: 없음. 운영 측 정책 (의존성 한정 규칙, 백그라운드 동선) 은 [`design/linux-system-tray.md`](../design/linux-system-tray.md) 가 기술한다.

## Alternatives Considered

- **`tray-icon` 크레이트로 Linux 트레이 구현 (`cfg(target_os = "linux")` 추가)**: 런타임에 `libayatana-appindicator3-1` 또는 `libdbusmenu-glib4` 패키지가 필요하고, DE 별 동작 차이로 테스트 매트릭스가 늘어난다. 가치 대비 유지보수 비용이 높아 기각.
- **AppIndicator/SNI 경유 (GNOME 은 확장 설치 전제)**: GNOME 기본 환경에서 보이지 않아 점유율 1위 DE 의 사용자 다수가 기능을 받지 못한다. 기각.
- **특정 DE (예: KDE) 한정 지원**: 검토 기록 미상 — 당시 결정 기록 (commit `c18c32f3`, design 문서) 에서 개별 대안으로 논의된 흔적이 발굴되지 않았다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

- GNOME 이 시스템 트레이를 기본 지원으로 복원
- Tasty 의 백그라운드 동작이 태스크바 유지로는 부족해지는 새로운 사용 사례 등장 (예: 알림 누적 표시 등)

## References

- [`design/linux-system-tray.md`](../design/linux-system-tray.md) — 현재 정책의 운영 측 상세
- commit `c18c32f3` (2026-05-01) — 결정 기록 원본. 요지: DE 분열로 일관된 동작 보장이 어렵고, 백그라운드 동선은 이미 태스크바 유지로 해결되어 있어 의존성·테스트 비용 대비 ROI 가 낮다
- commit `e5fe9924` (2026-04-16) — Windows 시스템 트레이 구현 (현재 `src/platform/system_tray.rs`, 커밋 당시 경로는 `src/system_tray.rs`)
