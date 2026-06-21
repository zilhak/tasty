# tasty-host-plugin Trust Keys

Ed25519 공개키 디렉토리. `bundle_sig.rs` 의 `TRUSTED_PUBKEYS` 가 `include_bytes!`
로 컴파일타임 흡수한다 — 단 **소스 트리가 아니라 `OUT_DIR`** 를 참조한다.
`build.rs` 가 빌드 직전 각 키를 `OUT_DIR` 로 staging 하기 때문이다 (배경은 build.rs
모듈 주석).

## 파일

| 파일 | 추적 | 용도 | 교체 주체 |
|------|------|------|-----------|
| `release-pubkey.bin` | **추적함** (신뢰 루트) | 정식 release plugin manifest 의 ed25519 서명 검증용 (raw 32 byte) | release pipeline (`secrets/release-private.pem` 로 서명) |
| `dev-pubkey.bin` | **추적 안 함** (`.gitignore`) | 로컬 개발자 dev key 의 공개 부분 (raw 32 byte) | 개발자가 `scripts/gen-dev-key.sh` 실행 시 자동 생성 |

두 파일 모두 **32 byte raw ed25519 public key** (헤더/ASN.1 없음). 길이가 다르면
`*include_bytes!(...)` 가 `[u8; 32]` 와 매칭 실패하여 컴파일 에러로 표면화된다.

## dev-pubkey.bin 은 추적하지 않는다

`dev-pubkey.bin` 은 **개발자마다 다른 로컬 키**라 repo 에서 추적하지 않는다
(`keys/.gitignore`). 추적하면 빌드할 때마다 각자의 키로 덮어써져 커밋 노이즈·충돌이
생긴다. 파일이 없어도 `build.rs` 가 `OUT_DIR` 슬롯을 **all-zero placeholder** 로
채우므로 새 클론·CI 빌드가 깨지지 않는다 (placeholder 키는 어떤 서명도 통과시키지
못함 — debug 빌드의 dev bundle 은 unsigned 라 정상, release 빌드는 `cargo:warning`
으로 표면화). 실제 dev 서명을 trust 하려면 `scripts/gen-dev-key.sh` 로 키를
생성한다.

## 공개키는 비밀이 아니다

여기 들어가는 파일은 *공개키* 다. 누구나 보아도 안전하다. 대응하는 *비밀키* 는
`~/.tasty-keys/dev.pem` (gitignored) 또는 release pipeline 의 secret store 에 있다.

> raw 32 byte 키는 git 이 텍스트로 오인해 CRLF 변환하면 손상되므로, `.gitattributes`
> 에서 `keys/*.bin binary` 로 고정한다 (추적되는 `release-pubkey.bin` 보호).

## 교체 시 절차

1. `scripts/gen-dev-key.sh` — dev keypair 생성. `dev-pubkey.bin` 자동 생성(로컬).
2. release-pubkey 교체는 release 작업 영역. 본 디렉토리의 `release-pubkey.bin`
   파일만 32 byte raw 값으로 덮어쓰면 됨 (코드 변경 0).

## 정책 참조

- 정책 4=B (`include_bytes!`): `.claude-workspace/conductor/dmg-signature-gate-design.md` § 4
- 정책 5=B+C (multi-key trust + 사용자 trust DB): 같은 문서 § 5
