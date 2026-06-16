# Plugin Signing

tasty 의 release / dist 빌드는 모든 plugin 의 매니페스트가 *trusted private key*
로 서명되어 있어야 한다. debug 빌드는 검증 우회 (warn 로깅만).

서명 알고리즘 / trust store 정책 결정 배경은 `.claude-workspace/conductor/dmg-signature-gate-design.md`
참조. 본 문서는 *개발자가 따라 할 수 있는 절차* 만 다룬다.

## 개요

| 항목 | 값 |
|------|-----|
| 알고리즘 | Ed25519 (`ed25519-dalek`) |
| 서명 대상 | `<plugin-dir>/tasty-plugin.toml` 의 SHA-256 digest |
| 서명 파일 | `<plugin-dir>/tasty-plugin.toml.sig` (raw 64 byte, base64 아님) |
| Trust store | `crates/tasty-host-plugin/keys/` 의 `release-pubkey.bin` + `dev-pubkey.bin` (multi-pubkey 어레이) |
| 검증 시점 | `install_builtins_if_needed()` — release/dist 빌드 시 실제 차단, debug 빌드는 warn 만 |

## 한 번 설정 — 개발자 자신의 dev key 생성

```bash
./scripts/gen-dev-key.sh
# → secrets/dev-private.pem           (private, 권한 600, gitignored)
# → crates/tasty-host-plugin/keys/dev-pubkey.bin  (public 32 B, commit 가능)
```

생성된 dev pubkey 는 본인 시스템에서 컴파일된 release 빌드의 trust store 에
포함되어, *dev key 로 서명된 plugin 을 자동으로 trust* 한다. 다른 개발자의
시스템에서 commit 된 `dev-pubkey.bin` 은 placeholder 이므로 본인 키로 덮어쓴 뒤
`cargo build --release` 해야 한다.

## Plugin 수정 후 재서명 (로컬)

`crates/tasty-plugin-*/tasty-plugin.toml` 을 한 줄이라도 고치면 기존 `.sig` 는
무효화된다. 다음 빌드 직전에 재서명:

```bash
./scripts/sign-bundle.sh --key secrets/dev-private.pem --all-builtins
# → 8 개 builtin plugin 의 tasty-plugin.toml.sig 가 모두 갱신됨
```

빌드 스크립트 (`scripts/build-macos-dmg.sh`, `scripts/build-linux.sh`,
`scripts/build-windows.ps1`) 는 패키징 직전 자동으로 sign-bundle.sh 를 호출한다.
명시적으로 따로 부를 일은 *manifest 만 고치고 빌드 없이 실행 확인* 같은 경우뿐.

## Release 빌드 (CI 자동)

`.github/workflows/release.yml` 이 tag push (`v*`) 또는 manual dispatch 시 빌드.
각 플랫폼 job 의 **빌드 직전** 에:

1. GitHub Secret `TASTY_RELEASE_SIGN_KEY` (Ed25519 private key PEM 의 base64
   인코딩) 을 `$RUNNER_TEMP/tasty-keys/release.pem` 로 디코딩 저장 (mode 600).
2. `TASTY_SIGN_KEY` env 를 위 경로로 설정.
3. `scripts/sign-bundle.sh --key "$TASTY_SIGN_KEY" --all-builtins` 호출 →
   8 plugin 모두 release key 로 서명.
4. 빌드 스크립트 실행 (서명된 `.sig` 가 staging 에 포함됨).
5. 빌드 결과 무관하게 `Wipe release signing key` step 으로 키 파일 + 디렉토리
   삭제.

PR CI (`build-check.yml`) 는 secret 미주입 — debug 빌드라 서명 검증 우회.

## GitHub Secret 등록 (운영자 1 회 작업)

```bash
# 로컬에서 release key 생성 (1 회):
openssl genpkey -algorithm Ed25519 -out release-private.pem
chmod 600 release-private.pem

# pubkey 추출 → repo 에 commit:
openssl pkey -in release-private.pem -pubout -outform DER | tail -c 32 \
    > crates/tasty-host-plugin/keys/release-pubkey.bin

# secret 등록용 base64:
base64 -i release-private.pem | pbcopy   # macOS
# 또는: base64 -w 0 release-private.pem  # Linux
```

GitHub repository 의 *Settings → Secrets and variables → Actions → New repository
secret*:

- Name: `TASTY_RELEASE_SIGN_KEY`
- Value: 위 base64 출력 (개행 없이)

등록 후 `release-private.pem` 은 안전한 위치 (1Password 등) 에 백업 + 로컬
파일은 즉시 삭제. **이 키가 유출되면 임의 plugin 이 빌트인을 가장 가능.**

## 외부 plugin / 사용자 시점 (future)

현재 단계 (0.7.x) 에서는 trust store 가 release + dev key 의 2 entry 만. 외부
plugin 은 첫 launch 시 **first-launch 모달** 표시 예정:

- Plugin name + SHA-256 fingerprint + 요구 권한 목록
- 응답: **Trust always** (영구 trust) / **Trust once** (이번만) / **Reject**
- 영구 trust 정보는 `~/.tasty/known-plugins.toml` 에 저장.

이미 trust 한 plugin 의 매니페스트에서 **권한이 추가** 되면 (예: `network.outbound`
가 새로 요구됨) SSH host key change 패턴으로 재모달 표시.

**구현 시점**: 0.7+ marketplace 단계. 현 release 흐름은 builtin 8 개만.

## Trust 해지 (수동)

```bash
# 특정 plugin trust 해지 (외부 plugin marketplace 단계 이후):
rm ~/.tasty/known-plugins.toml   # 또는 해당 [plugin-id] 섹션만 제거
```

builtin plugin 의 trust 는 코드 (`TRUSTED_PUBKEYS` 어레이) 에 박혀 있어
재컴파일 없이 해지 불가 — release key 가 유출되었을 때만 다음 절차 (키 회전).

## 키 분실 / 사고 시 — 키 회전 (multi-pubkey 전략)

`crates/tasty-host-plugin/keys/release-pubkey.bin` 을 새 키로 교체할 때,
**옛 키도 일시 trust** 유지하여 사용자 강제 업데이트 압박 없이 점진 이행:

```rust
// crates/tasty-host-plugin/src/bundle_sig.rs
const TRUSTED_PUBKEYS: &[[u8; 32]] = &[
    *include_bytes!("../keys/release-pubkey-v2.bin"),  // 신규 (이후 release)
    *include_bytes!("../keys/release-pubkey.bin"),     // 기존 (호환성, 일정 기간 유지)
    *include_bytes!("../keys/dev-pubkey.bin"),
];
```

1. 새 release key 페어 생성 + `TASTY_RELEASE_SIGN_KEY` secret 교체.
2. 옛 pubkey 파일은 *최소 2 minor 동안 trust store 에 잔존*.
3. 보안 권고 발표 + grace period 종료 후 옛 pubkey entry 삭제.

만약 영구 차단 정책이 필요하면 옛 entry 를 한 번에 제거 — 이전 release 의
plugin 즉시 차단 → 사용자 강제 update.

## 트러블슈팅

| 증상 | 원인 / 조치 |
|------|------------|
| `Skipped { signature-invalid }` 로 모든 builtin 미설치 | release/dist 빌드인데 `.sig` 부재. `./scripts/sign-bundle.sh` 호출 후 재빌드 |
| 로컬 release 빌드에서 dev key 로 서명했는데 검증 실패 | `dev-pubkey.bin` 이 사용한 private key 와 매칭 안 됨. `gen-dev-key.sh` 가 같은 페어 생성하므로 두 파일을 함께 갱신 |
| CI 에서 sign step 이 `signing key not found` 로 fail | `TASTY_RELEASE_SIGN_KEY` secret 미등록 또는 base64 디코딩 결과가 PEM 형식이 아님 |
| 사용자 머신에서 새 builtin 1 개만 install 실패 | 해당 plugin 의 매니페스트가 빌드 후 수정됨 → 재서명 누락. 빌드 스크립트가 자동 sign 을 보장하는지 확인 |

## 관련 문서

- [`plugin-development.md`](plugin-development.md) — Plugin 작성 가이드
- [`plugin-permissions.md`](plugin-permissions.md) — 권한 모델 (trust 모달이 보여줄 항목)
- [`release.md`](release.md) — 전체 release 절차 (본 문서의 sign 단계가 포함됨)
- [`debug-ipc.md`](debug-ipc.md) — debug 빌드에서 서명 검증을 우회하는 cfg 분기 설명
