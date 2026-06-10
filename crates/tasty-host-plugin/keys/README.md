# tasty-host-plugin Trust Keys

Ed25519 공개키 임베드 디렉토리. `bundle_sig.rs` 의 `TRUSTED_PUBKEYS` 가
`include_bytes!` 로 컴파일타임 흡수한다.

## 파일

| 파일 | 용도 | 교체 주체 |
|------|------|-----------|
| `release-pubkey.bin` | 정식 release plugin manifest 의 ed25519 서명 검증용 (raw 32 byte) | release pipeline (`secrets/release-private.pem` 로 서명) |
| `dev-pubkey.bin` | 로컬 개발자 dev key 의 공개 부분 (raw 32 byte) | 개발자가 `scripts/gen-dev-key.sh` 실행 시 자동 갱신 |

두 파일 모두 **32 byte raw ed25519 public key** (헤더/ASN.1 없음). 길이가 다르면
`*include_bytes!(...)` 가 `[u8; 32]` 와 매칭 실패하여 컴파일 에러로 표면화된다.

## 공개키는 비밀이 아니다

여기 들어가는 파일은 *공개키* 다. 누구나 보아도 안전하며, **git commit 안전**.
대응하는 *비밀키* 는 `secrets/dev-private.pem` (gitignored) 또는 release pipeline
의 secret store 에 있다.

## 교체 시 절차

1. `scripts/gen-dev-key.sh` — dev keypair 생성. `dev-pubkey.bin` 자동 갱신.
2. release-pubkey 교체는 release 작업 영역. 본 디렉토리의 `release-pubkey.bin`
   파일만 32 byte raw 값으로 덮어쓰면 됨 (코드 변경 0).

## 현재 placeholder 상태

본 커밋의 두 파일은 **32 byte zeroed placeholder**. `VerifyingKey::from_bytes` 가
zeroed pubkey 로 어떤 서명도 통과시키지 않으므로 검증은 항상 fail. 실제 release
직전에 키 생성/주입 작업이 별도 PR 로 들어온다.

## 정책 참조

- 정책 4=B (`include_bytes!`): `.claude-workspace/conductor/dmg-signature-gate-design.md` § 4
- 정책 5=B+C (multi-key trust + 사용자 trust DB): 같은 문서 § 5
