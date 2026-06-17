# 자동 업데이트 확인 (Auto-update)

- **Status**: Implemented
- **주체**: 로컬 사용자 (백그라운드 폴러 + `tasty update` CLI)
- **ADR**: 없음
- **코드**: `crates/tasty-update/`; 폴러 `src/state/update_check.rs`; CLI `crates/tasty-cli/src/commands/update.rs`
- **화면**: [설정 창](../settings/screens/settings.md) Updates 탭 · update_check popup

## 목적

GitHub Releases 를 폴링해 새 버전을 감지하고, **알림 + Settings Updates 탭**으로 노출한다. 설치는 CLI `tasty update` 한 줄로 다운로드 + SHA256 검증 + atomic swap 까지. GUI in-app 설치는 보류(릴리스 페이지 열기).

## 내부 동작

### 트리거

- 앱 시작 시 백그라운드 폴러가 1회 즉시 + 이후 **1시간 간격** 자동 폴링.
- 새 버전 감지 시 **1회 한도** in-app 알림(`notified_version` 으로 중복 차단).
- 도구 메뉴 `Check for updates…` / Settings Updates 탭 `Check now` / CLI `tasty update`.

### 감지

`tasty_update::check_latest(owner, repo, current, allow_prerelease)` → semver 비교(`v` prefix 자동 제거)로 `latest > current` 일 때만 `ReleaseInfo` 반환. 폴러(`UpdateStatus`)가 None→Some 전이 시 `pending_notify` 설정 → 매 프레임 drain 이 `PushNotification { source: "update" }` 발행.

### `tasty update` CLI 흐름

standalone(호스트 미실행 OK — `run.rs` 가 IPC connect 이전에 가로챔): check → 사용자 확인(`--yes` skip) → **asset 선택**(OS×arch) → 다운로드(진행률) → `SHA256SUMS-{platform}.txt` 검증(**hard fail**) → atomic swap → 재시작 안내.

자산 선택(`select_asset`): macOS `*.dmg`(수동), Windows `.msi`→`.zip`, Linux `.deb`/`.rpm`/`.AppImage`/`.tar.gz`(`/etc/os-release` 의 ID/ID_LIKE 로 가족 detect, x64/arm64). atomic swap: Unix `rename(2)`(+`.old` 백업, cross-device 시 copy fallback), Windows `tasty.new.exe` 스테이징 + `tasty-swap.bat`, macOS DMG 안내.

## 인터페이스

- **사용자**: 도구 메뉴/Settings 에서 확인, `Open release page`. 설치는 `tasty update [--check-only] [--yes] [--prerelease]`.

## 비-목표

- 릴리스 *발행* 절차(빌드·태그·CI) — [dev-guide/release](../../dev-guide/release.md).

## 관련

- [dev-guide/release](../../dev-guide/release.md) — SHA256SUMS 4종 산출 (없으면 `tasty update` hard fail)
- [notifications](../notifications/index.md) · [settings](../settings/index.md)
