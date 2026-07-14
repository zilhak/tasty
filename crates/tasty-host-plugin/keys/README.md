# tasty-host-plugin Trust Keys

Ed25519 공개키 디렉토리. `bundle_sig.rs` 의 `TRUSTED_PUBKEYS` 가 `include_bytes!`
로 컴파일타임 흡수한다 — 단 **소스 트리가 아니라 `OUT_DIR`** 를 참조한다.
`build.rs` 가 빌드 직전 각 키를 `OUT_DIR` 로 staging 하기 때문이다 (배경은 build.rs
모듈 주석).

## 파일

| 파일 | 추적 | 용도 | 생성 주체 |
|------|------|------|-----------|
| `dev-pubkey.bin` | **추적 안 함** (`.gitignore`) | 로컬 dev key 의 공개 부분 (raw 32 byte) | `scripts/gen-dev-key.sh` 실행 시 자동 생성 |
| `release-pubkey.bin` | **추적 안 함** (`.gitignore`) | 항상 placeholder — 영구 release 신뢰 루트를 두지 않기로 했음(아래 참고) | 없음. 실질적 검증은 dev 슬롯이 담당 |

두 파일 모두 **32 byte raw ed25519 public key** (헤더/ASN.1 없음). 길이가 다르면
`*include_bytes!(...)` 가 `[u8; 32]` 와 매칭 실패하여 컴파일 에러로 표면화된다.

## 두 파일 다 추적하지 않는다

`dev-pubkey.bin` 은 **머신마다 다른 로컬 키**라 repo 에서 추적하지 않는다
(`keys/.gitignore`). 추적하면 빌드할 때마다 각자의 키로 덮어써져 커밋 노이즈·충돌이
생긴다. 파일이 없어도 `build.rs` 가 `OUT_DIR` 슬롯을 **all-zero placeholder** 로
채우므로 새 클론·CI 빌드가 깨지지 않는다. 실제 서명을 trust 하려면
`scripts/gen-dev-key.sh`(release/dist 빌드는 `scripts/ensure-sign-key.sh` 가
자동 호출)로 키를 생성한다.

`release-pubkey.bin`도 같은 이유로 추적하지 않는다 — 원래는 운영자가 1회 발급해
영구 커밋하는 "신뢰 루트"로 설계됐지만, builtin plugin이 항상 앱 바이너리와 같은
번들로 배포되고(`install_builtins_if_needed`, 앱 버전과 독립적인 plugin 업데이트
경로가 없음) 릴리스마다 자기 안에서 완결되는 신뢰 단위라는 게 확인되어, 영구
키 대신 **매 빌드 로컬에서 자동생성되는 dev 키**로 통일했다([ADR-0051](../../../docs/adr/0051-ephemeral-release-signing-key.md) 참고).
이 슬롯은 이제 항상 placeholder 로 남는다 — misconfiguration 이 아니라 정상
상태다. 두 슬롯이 *전부* placeholder 일 때만 `build.rs` 가 release 빌드에서
`cargo:warning` 을 낸다(dev 키 생성 자체가 실패한 경우).

## 공개키는 비밀이 아니다

여기 들어가는 파일은 *공개키* 다. 누구나 보아도 안전하다. 대응하는 *비밀키* 는
빌드를 실행한 머신의 `~/.tasty-keys/dev.pem` (gitignored, 영구 보관되지 않음)에
있다.

## 정책 참조

- 서명·trust 정책(`include_bytes!` 임베드 · 사용자 trust DB · 매 빌드 자동생성 정책 배경): [`docs/dev-guide/plugin-packaging.md`](../../../docs/dev-guide/plugin-packaging.md) 의 "서명" 섹션
