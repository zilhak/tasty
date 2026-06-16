# Plugin staging — 7 위치 동기화

빌트인 plugin 8 종은 다음 *7 위치* 에 중복 명시되어 있다. 새 plugin 추가 / 제거
시 모두 동시에 업데이트해야 한다.

## 빌트인 plugin 목록 (canonical)

`crates/tasty-host-plugin/src/builtin.rs::BUILTINS` 가 **소스-오브-트루스**.
8 개 ID + crate 매핑:

| Plugin crate                    | Plugin ID                   |
|---------------------------------|-----------------------------|
| `tasty-plugin-claude`           | `com.tasty.claude`          |
| `tasty-plugin-clipboard-history`| `com.tasty.clipboard-history`|
| `tasty-plugin-codex`            | `com.tasty.codex`           |
| `tasty-plugin-explorer`         | `com.tasty.explorer`        |
| `tasty-plugin-git-viewer`       | `com.tasty.git-viewer`      |
| `tasty-plugin-html`             | `com.tasty.html`            |
| `tasty-plugin-image`            | `com.tasty.image`           |
| `tasty-plugin-markdown`         | `com.tasty.markdown`        |

산출물 per plugin:
- `<bin>` (또는 `<bin>.exe` on Windows)
- `tasty-plugin.toml` (매니페스트)
- `tasty-plugin.toml.sig` (Ed25519 서명 sidecar — non-debug 빌드 필수)
- `lang/{en,ja,ko}.toml`

## 7 동기화 위치

| # | 위치                                              | 방식                  | 명시/동적 |
|---|---------------------------------------------------|-----------------------|-----------|
| 1 | `Cargo.toml::[package.metadata.deb] assets`       | per-plugin × 4 entries| **명시**  |
| 2 | `Cargo.toml::[package.metadata.generate-rpm]`     | per-plugin × 4 entries| **명시**  |
| 3 | `wix/main.wxs` `<Component>` + `<ComponentRef>`   | per-plugin × 6 (bin + manifest + sig + en + ja + ko) | **명시** |
| 4 | `scripts/build-macos-dmg.sh` staging 루프          | `for d in crates/tasty-plugin-*` glob | 동적 |
| 5 | `scripts/build-linux.sh::stage_plugins()`         | 동일 glob              | 동적     |
| 6 | `scripts/build-windows.ps1::Stage-Plugins`        | `Get-ChildItem` glob   | 동적     |
| 7 | `crates/tasty-host-plugin/src/builtin.rs::BUILTINS` | Rust `const &[BuiltinSpec]` (windows + non-windows cfg) | **명시** |

위치 1/2/3/7 은 새 plugin 추가 시 *반드시 수정*. 4/5/6 은 `crates/tasty-plugin-*`
가 자동 발견되므로 별도 수정 불필요.

## 잠재 drift 표면

### lang 파일 — wix 만 enumerated

- deb/rpm: `lang/*` 글로브로 모든 파일 자동 포함.
- 빌드 스크립트: `cp -R lang` (Linux/macOS) / `Copy-Item -Recurse lang` (Windows) 로 모든 파일 자동 포함.
- **wix: `LangEn` / `LangJa` / `LangKo` Component 를 *enumerate*** — 새 로케일
  파일(`de.toml` 등) 추가 시 wix 만 silent skip → Windows .msi 사용자에게만 누락.

새 로케일 추가 시 `wix/main.wxs` 의 해당 plugin Directory 에 `Component` +
`ComponentRef` 쌍을 *직접* 추가해야 한다.

### `.sig` 의 빌드 시점 의존

`.sig` 파일은 `scripts/sign-bundle.sh` 가 빌드 직전 생성한다. 빌드 산출물
디렉토리에 *staging 시점* 에 존재해야 하며, 모든 6 staging 위치는 비존재 시
non-debug 빌드를 fail 시킨다.

`.sig` 자체는 git 에 commit 되지 않고 *빌드 산출물*. CI 빌드 환경에서
`sign-bundle.sh` 가 항상 실행되도록 보장해야 한다 — 자세한 정책은
[`docs/dev-guide/plugin-signing.md`](plugin-signing.md).

## 비-staging 항목

다음은 *번들 산출물* 이 아니다 — 7 위치에 추가 불필요:

- **`known-plugins.toml`** — `~/.tasty/known-plugins.toml` 의 *사용자 trust DB*.
  런타임이 생성/관리. 빌드 산출물 아님.
- **`.pub` sidecar** — 존재하지 않음. 공개키는 호스트 바이너리에 *embed* + 사용자
  trust DB 에 등록되는 방식.
