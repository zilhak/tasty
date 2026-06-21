# Plugin 패키징 — 서명 + staging 동기화

번들 plugin 을 release/dist 빌드에 포함시키는 절차: **Ed25519 매니페스트 서명** + **빌드 staging 7 위치 동기화**. 런타임 라이프사이클(자동 install/upgrade)은 [plugin-ecosystem](plugin-ecosystem.md), 제작은 [plugin-development](plugin-development.md).

## 번들 plugin 목록 (SoT)

`crates/tasty-host-plugin/src/builtin.rs::BUILTINS` 가 단일 출처. 8 종:

| crate | plugin ID |
|-------|-----------|
| `tasty-plugin-claude` | `com.tasty.claude` |
| `tasty-plugin-clipboard-history` | `com.tasty.clipboard-history` |
| `tasty-plugin-codex` | `com.tasty.codex` |
| `tasty-plugin-explorer` | `com.tasty.explorer` |
| `tasty-plugin-git-viewer` | `com.tasty.git-viewer` |
| `tasty-plugin-html` | `com.tasty.html` |
| `tasty-plugin-image` | `com.tasty.image` |
| `tasty-plugin-markdown` | `com.tasty.markdown` |

plugin 당 산출물: `<bin>`(Windows `.exe`) · `tasty-plugin.toml`(매니페스트) · `tasty-plugin.toml.sig`(서명 sidecar, non-debug 필수) · `lang/{en,ja,ko}.toml`.

## 서명

| 항목 | 값 |
|------|-----|
| 알고리즘 | Ed25519 (`ed25519-dalek`) |
| 서명 대상 | `<plugin-dir>/tasty-plugin.toml` 의 SHA-256 digest |
| 서명 파일 | `tasty-plugin.toml.sig` (raw 64 byte) |
| Trust store | `crates/tasty-host-plugin/keys/` 의 `release-pubkey.bin` + `dev-pubkey.bin` (multi-pubkey 배열, `bundle_sig.rs::TRUSTED_PUBKEYS`) |
| 검증 시점 | `install_builtins_if_needed()` — release/dist 는 실제 차단, debug 는 warn 만(`#[cfg(debug_assertions)]`) |

보호 범위는 **매니페스트 한 파일만** — 권한/contributes/kind 가 매니페스트 안이라 변조 시 confused-deputy 가 최대 위험. binary 는 OS codesign(macOS notarization/Windows Authenticode)에 위임, lang/ 등 부속은 검증 밖.

### dev key (개발자 1회)

```bash
./scripts/gen-dev-key.sh
# → ~/.tasty-keys/dev.pem  (600, gitignored)
# → crates/tasty-host-plugin/keys/dev-pubkey.bin  (public 32B, 로컬 전용 — 추적 안 함)
```

`dev-pubkey.bin` 은 개발자별 로컬 키라 **추적하지 않는다** (`keys/.gitignore`). 파일이 없어도 build.rs 가 OUT_DIR 슬롯을 all-zero placeholder 로 채워 컴파일은 안 깨지지만, dev key 서명 plugin 을 자동 trust 하려면 `gen-dev-key.sh` 로 키를 생성한 뒤 빌드해야 한다.

### 재서명 (로컬)

`tasty-plugin.toml` 을 한 줄이라도 고치면 기존 `.sig` 무효화. 빌드 직전 재서명:

```bash
./scripts/sign-bundle.sh --key secrets/dev-private.pem --all-builtins
```

빌드 스크립트(`build-macos-dmg.sh`/`build-linux.sh`/`build-windows.ps1`)는 패키징 직전 자동으로 `sign-bundle.sh` 를 호출 — 매니페스트만 고치고 빌드 없이 확인할 때만 수동 호출.

### Release CI 서명

`.github/workflows/release.yml` 이 tag push(`v*`)/manual dispatch 시:

1. GitHub Secret `TASTY_RELEASE_SIGN_KEY`(Ed25519 PEM 의 base64)를 `$RUNNER_TEMP` 에 mode 600 으로 디코딩.
2. `scripts/sign-bundle.sh --key "$TASTY_SIGN_KEY" --all-builtins` 로 8 plugin 서명.
3. 빌드 실행 → 결과 무관하게 `Wipe release signing key` step 으로 키 삭제.

PR CI 는 secret 미주입(debug 빌드라 검증 우회). repo 의 `.sig` 는 *로컬 release/dev 검증용* — CI 정식 release 는 secret 키로 재서명하므로 repo 와 다른 키로 덮어쓰는 게 정상.

### release key 등록 (운영자 1회)

```bash
openssl genpkey -algorithm Ed25519 -out release-private.pem && chmod 600 release-private.pem
openssl pkey -in release-private.pem -pubout -outform DER | tail -c 32 > crates/tasty-host-plugin/keys/release-pubkey.bin
base64 -w 0 release-private.pem   # → GitHub Secret TASTY_RELEASE_SIGN_KEY (개행 없이)
```

`release-private.pem` 은 안전한 곳에 백업 후 로컬 즉시 삭제. **유출되면 임의 plugin 이 빌트인을 가장 가능.**

### 키 회전 (multi-pubkey)

`TRUSTED_PUBKEYS` 배열에 새 키를 prepend 하고 옛 키를 *최소 2 minor 유지* 후 제거 — 강제 업데이트 압박 없이 점진 이행. 영구 차단이 필요하면 옛 entry 즉시 제거(이전 release plugin 즉시 차단).

## staging 7 위치 동기화

새 plugin 추가/제거 시 동시 갱신할 위치:

| # | 위치 | 명시/동적 |
|---|------|-----------|
| 1 | `Cargo.toml [package.metadata.deb] assets` | **명시** (per-plugin ×4) |
| 2 | `Cargo.toml [package.metadata.generate-rpm]` | **명시** (×4) |
| 3 | `wix/main.wxs` `<Component>` + `<ComponentRef>` | **명시** (×6: bin/manifest/sig/en/ja/ko) |
| 4 | `scripts/build-macos-dmg.sh` staging 루프 | 동적 (`crates/tasty-plugin-*` glob) |
| 5 | `scripts/build-linux.sh::stage_plugins()` | 동적 |
| 6 | `scripts/build-windows.ps1::Stage-Plugins` | 동적 |
| 7 | `builtin.rs::BUILTINS` | **명시** (windows + non-windows cfg) |

1/2/3/7 은 새 plugin 시 **반드시 수정**, 4/5/6 은 glob 자동 발견.

### drift 함정

- **lang 파일 — wix 만 enumerate**: deb/rpm/빌드스크립트는 `lang/*` 자동 포함, wix 는 `LangEn`/`LangJa`/`LangKo` Component 를 *나열* → 새 로케일(`de.toml` 등) 추가 시 wix 만 silent skip → .msi 사용자만 누락. wix 의 해당 plugin Directory 에 `Component`+`ComponentRef` 직접 추가 필요.
- **`.sig` 빌드 시점 의존**: git 에 commit 안 되는 빌드 산출물. 6 staging 위치 모두 비존재 시 non-debug 빌드 fail — CI 가 `sign-bundle.sh` 를 항상 실행하도록 보장.

비-staging(번들 산출물 아님): `~/.tasty/known-plugins.toml`(사용자 trust DB, 런타임 생성) · `.pub` sidecar(없음 — 공개키는 호스트 바이너리 embed).

## 트러블슈팅

| 증상 | 조치 |
|------|------|
| `Skipped { signature-invalid }` 로 builtin 미설치 | release/dist 인데 `.sig` 부재 → `sign-bundle.sh` 후 재빌드 |
| 로컬 release 에서 dev key 서명 검증 실패 | `dev-pubkey.bin` 이 사용 private key 와 불일치 → `gen-dev-key.sh` 로 두 파일 함께 갱신 |
| CI sign step `signing key not found` | `TASTY_RELEASE_SIGN_KEY` 미등록 또는 base64 디코딩이 PEM 아님 |
| Windows `candle could not be found` | WiX 3.x 미설치/`WIX` env 누락 → `winget install WiXToolset.WiXToolset` |

## 관련

- [plugin-ecosystem](plugin-ecosystem.md) — bundle signature 검증 + 자동 upgrade 런타임
- [plugin-development](plugin-development.md) · [plugin-permissions](plugin-permissions.md) · [release](release.md) · [debug-ipc](debug-ipc.md)
