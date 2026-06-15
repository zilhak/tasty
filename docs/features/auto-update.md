# 자동 업데이트 확인

- **Status**: Implemented

### 개요
GitHub Releases API를 폴링하여 새 버전이 있는지 확인하고, 발견 시 알림과 Settings → Updates 탭으로 노출한다. **Phase J.H — 알림 + `tasty update` 1-click**: 백그라운드 감지 + in-app 알림 + CLI `tasty update` 로 다운로드/SHA256 검증/원자 swap 까지 자동화. GUI 에서의 in-app 설치는 보류 (브라우저로 릴리스 페이지 열기).

### 트리거
- Tools 메뉴의 `Check for updates…` 빌트인 항목 (port_scanner 아래)
- 앱 시작 시 백그라운드 폴러가 1회 즉시 + 이후 1시간 간격으로 자동 폴링
- 새 버전 발견 시 1회 한도로 in-app 알림 발사 (`notified_version` 으로 중복 차단)
- Settings → Updates 탭의 `Check now` 버튼
- CLI: `tasty update` (standalone — 호스트 실행 불필요)

### 동작
- `tasty_update::check_latest(owner, repo, current_version, allow_prerelease)` 호출 → `Result<Option<ReleaseInfo>, UpdateError>`
- 응답이 `Some(ReleaseInfo)` 이면 `latest_version > current_version` 인 경우만 반환 (semver 비교; `v` prefix 자동 제거)
- popup 에서 현재 버전, 최신 버전, 릴리스 노트(스크롤), `Open release page` 버튼, `Check now` 버튼 표시
- Settings → Updates 탭: 현재/최신/마지막 확인 시각, `Check now`, `Open release page…`, CLI 안내
- 에러 발생 시 popup/탭에 `Error: <msg>` 빨간 라벨 표시
- 알림: `update.notify.title` / `update.notify.body` (3 lang)
- `tasty update` CLI 흐름: check → 사용자 확인 (`--yes` 로 skip) → asset 선택 (OS×arch) → 다운로드 + 진행률 표시 → `SHA256SUMS-{platform}.txt` 다운로드 + 검증 (hard fail) → atomic swap → 사용자에게 재시작 안내

### 자산 매트릭스 (`select_asset`)

| target_os | target_arch | 선택 우선순위 |
|-----------|-------------|---------------|
| macos     | any         | `Tasty-{v}-macos.dmg` (.app 교체는 J.H+ 보류 — 사용자 수동 DMG) |
| windows   | x86_64      | `.msi` → `.zip` (fallback) |
| linux     | x86_64      | `.deb` (Debian-like) / `.rpm` (RPM-like) / `.AppImage` / `.tar.gz` |
| linux     | aarch64     | 같은 우선순위, arm64 변종 |

Linux 가족 detect 는 `/etc/os-release` 의 `ID=` / `ID_LIKE=` 기반. SHA256SUMS 파일은 `macos`/`windows`/`linux-x64`/`linux-arm64` 4종.

### 구현
- crate: `tasty-update`
  - `check_latest(owner, repo, current_version, allow_prerelease) -> Result<Option<ReleaseInfo>, UpdateError>`
  - `is_newer(current, remote_tag) -> Result<bool, UpdateError>`
  - `select_asset(info) -> Option<AssetSpec>` — `(target_os, target_arch)` 4 케이스
  - `download_to(asset, dest, progress)` — 스트리밍 다운로드 + Content-Length 기반 진행률
  - `fetch_sha256_sums(info) -> HashMap<name, hex>` + `verify_sha256(path, expected)`
  - `atomic_swap(new, target) -> SwapOutcome::{Completed, RestartRequired}`
    - Unix: `rename(2)` + chmod 755 (실패 시 cross-device copy fallback). `.old` 백업 보존
    - Windows: `tasty.new.exe` 스테이징 + `tasty-swap.bat` (재실행 시 swap)
    - macOS: DMG 안내만 (Completed 아님)
  - 의존성: `ureq`, `serde`, `semver`, `sha2`, `thiserror`, `tracing`, `tempfile` (dev)
- 백그라운드 폴러: `src/state/update_check.rs`
  - `UpdateStatus { latest, last_error, last_checked, in_flight, notified_version, pending_notify }`
  - `spawn_poller(owner, repo, current, interval)` / `trigger_check(...)`
  - 새 버전 감지 (None→Some) 시 `pending_notify = Some(info)`
- 알림 drain: `src/app/dispatch/update_notifications.rs` — 매 frame `pending_notify` 를 take 해 `DomainIntent::PushNotification { source: "update" }` 발행. `notified_version` 으로 중복 차단
- AppState: `update_status: Arc<Mutex<UpdateStatus>>` (1시간 간격 폴러 자동 spawn)
- popup: `src/adapters/ui/popup/update.rs` (`update_check` ID)
- 트리거: `src/adapters/ui/tools_menu.rs` 의 `BUILTIN_TOOLS` 항목
- Settings 탭: `src/view/settings/ui/tabs/updates.rs` (`SettingsTab::Updates`)
- CLI: `crates/tasty-cli/src/commands/update.rs` — `tasty update [--check-only] [--yes] [--prerelease]`. standalone (호스트 미실행 OK), `run.rs` 가 IPC connect 이전에 가로채 실행
