# Linux 시스템 트레이 — 미지원 결정

Tasty는 **Linux에서 시스템 트레이 아이콘을 제공하지 않는다.** macOS는 메뉴바, Windows는 알림 영역(트레이)을 OS 표준 패턴으로 제공하지만, Linux는 그렇지 않다.

## 사유

### DE 분열로 일관된 동작 보장 불가

| 데스크톱 환경 | 트레이 지원 |
|--------------|-------------|
| GNOME (Ubuntu·Fedora 기본) | **기본 미지원**. 3.26부터 legacy SystemTray 제거. AppIndicator 확장 추가 설치 필요 |
| KDE Plasma | StatusNotifierItem 네이티브 |
| XFCE | panel-systray / SNI 플러그인 |
| Cinnamon, MATE | 지원 |

GNOME이 데스크톱 Linux 점유율 1위인데 기본 미지원이라 "절반의 사용자에게는 보이지 않는 기능"이 된다.

### Tasty의 백그라운드 동선이 트레이에 의존하지 않음

Tasty는 **마지막 윈도우 닫기 시 `set_minimized(true)`로 태스크바에 유지**한다 (Windows와 동일 동작). 즉 백그라운드 실행/빠른 접근 동선이 이미 태스크바로 해결되어 있어 트레이가 추가로 해결할 사용자 문제가 거의 없다.

### 추가 의존성 비용

`tray-icon` 크레이트로 구현하려면 런타임에 `libayatana-appindicator3-1` 또는 `libdbusmenu-glib4` 같은 패키지가 필요하다. DE별 동작 차이 때문에 테스트 매트릭스도 KDE/GNOME(+확장)/XFCE/Cinnamon으로 늘어난다. 가치 대비 유지보수 비용이 높다.

## 코드 측면

`Cargo.toml`의 `tray-icon` 의존성은 **`cfg(windows)` 한정**으로 유지한다. Linux용 `cfg(target_os = "linux")` 분기를 추가하지 않는다.

## 향후 재검토 조건

다음 중 하나가 충족되면 재검토할 가치가 있다.
- GNOME이 시스템 트레이를 기본 지원으로 복원
- Tasty의 백그라운드 동작이 태스크바 유지로는 부족해지는 새로운 사용 사례 등장 (예: 알림 누적 표시 등)
