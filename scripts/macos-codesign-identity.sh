#!/bin/bash
# macOS 코드 서명 identity 확인 / 발급.
#
# 왜 필요한가
# -----------
# 기본 빌드는 ad-hoc 서명(`codesign --sign -`)을 쓴다. ad-hoc 서명은 인증서가 없어
# designated requirement 가 cdhash(바이너리 내용 해시)뿐이다. 즉 재빌드로 바이너리가
# 1 바이트만 바뀌어도 macOS 는 전혀 다른 앱으로 보고, 그 앱에 붙어 있던 TCC(개인정보
# 보호) 승인·키체인 접근 권한이 전부 초기화된다. 개발 중 자주 빌드하면
# "Tasty 가 다른 앱의 데이터에 접근하려고 합니다" 같은 프롬프트가 매번 다시 뜬다.
#
# 인증서로 서명하면 DR 이 `identifier "com.zilhak.tasty" and certificate leaf = H"..."`
# 형태가 되어, 재빌드해도 같은 앱으로 인식되고 승인이 유지된다. self-signed 로 충분하며
# Apple 개발자 계정은 필요 없다.
#
# 사용법
#   ./scripts/macos-codesign-identity.sh            # 현재 identity 확인
#   ./scripts/macos-codesign-identity.sh --create   # self-signed 인증서 발급
#
# 발급 후 빌드 — install-macos.sh 가 키체인에서 자동으로 집는다:
#   ./scripts/install-macos.sh
#
# 다른 identity 를 쓰려면 $TASTY_CODESIGN_IDENTITY 로 지정한다.
#
# 배포용 산출물(DMG)에는 쓰지 마라 — 다른 사람 머신에서는 이 인증서를 신뢰하지 않아
# ad-hoc 과 다를 게 없다. 어디까지나 로컬 개발 편의용이다.

set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "Error: This script must be run on macOS." >&2
    exit 1
fi

IDENTITY_NAME="${TASTY_CODESIGN_IDENTITY:-Tasty Dev}"

list_identities() {
    security find-identity -v -p codesigning 2>/dev/null || true
}

if [[ "${1:-}" != "--create" ]]; then
    echo "==> 사용 가능한 코드 서명 identity:"
    list_identities
    echo ""
    if list_identities | grep -q "$IDENTITY_NAME"; then
        echo "'$IDENTITY_NAME' 사용 가능. 빌드 시:"
        echo "  ./scripts/install-macos.sh    # 키체인에서 자동으로 집는다"
    else
        echo "'$IDENTITY_NAME' 이(가) 없다. 발급하려면:"
        echo "  $0 --create"
        echo ""
        echo "Xcode 를 쓴다면 Apple Development 인증서도 그대로 쓸 수 있다"
        echo "(위 목록에 있으면 그 이름을 TASTY_CODESIGN_IDENTITY 로 주면 된다)."
    fi
    exit 0
fi

if list_identities | grep -q "$IDENTITY_NAME"; then
    echo "==> '$IDENTITY_NAME' 이(가) 이미 있다. 발급을 건너뛴다."
    echo "  ./scripts/install-macos.sh    # 키체인에서 '$IDENTITY_NAME' 를 자동으로 집는다"
    exit 0
fi

command -v openssl &>/dev/null || {
    echo "Error: openssl not found." >&2
    exit 1
}

WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

# codeSigning EKU 가 있어야 codesign 이 identity 로 인정한다.
cat > "$WORK_DIR/cert.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = v3
prompt = no
[dn]
CN = $IDENTITY_NAME
[v3]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF

echo "==> '$IDENTITY_NAME' 인증서 생성 중..."
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
    -config "$WORK_DIR/cert.cnf" \
    -keyout "$WORK_DIR/key.pem" -out "$WORK_DIR/cert.pem" 2>/dev/null

# OpenSSL 3 의 기본 PKCS#12 암호화(PBES2/AES)는 macOS Security framework 가 읽지
# 못한다 — `-legacy` 로 구형 알고리즘을 쓴다. LibreSSL(/usr/bin/openssl)에는 그
# 플래그가 없고 기본값이 이미 호환되므로, 실패하면 플래그 없이 재시도한다.
P12_PASS="tasty-import"
export_p12() {
    openssl pkcs12 -export "$@" \
        -inkey "$WORK_DIR/key.pem" -in "$WORK_DIR/cert.pem" \
        -out "$WORK_DIR/identity.p12" -passout "pass:$P12_PASS" \
        -name "$IDENTITY_NAME"
}
export_p12 -legacy 2>/dev/null || export_p12

echo "==> 로그인 키체인에 등록 중..."
security import "$WORK_DIR/identity.p12" \
    -k "$HOME/Library/Keychains/login.keychain-db" \
    -P "$P12_PASS" -T /usr/bin/codesign -A

# self-signed 인증서는 신뢰 설정을 해줘야 codesign 이 identity 로 인정한다.
# 이 단계에서 macOS 가 관리자 암호를 묻는다 (키체인 신뢰 설정 변경).
echo ""
echo "==> 인증서 신뢰 설정 중 — macOS 가 암호를 물어본다."
security add-trusted-cert -r trustRoot -p codeSign \
    -k "$HOME/Library/Keychains/login.keychain-db" "$WORK_DIR/cert.pem"

echo ""
if list_identities | grep -q "$IDENTITY_NAME"; then
    echo "완료. '$IDENTITY_NAME' 로 서명하려면:"
    echo "  ./scripts/install-macos.sh    # 키체인에서 '$IDENTITY_NAME' 를 자동으로 집는다"
    echo ""
    echo "첫 실행에서 권한 프롬프트가 한 번 더 뜬 뒤, 이후 재빌드부터는 유지된다."
else
    echo "Error: 발급은 끝났지만 '$IDENTITY_NAME' 이(가) 서명 identity 로 잡히지 않는다." >&2
    echo "키체인 접근.app 에서 인증서의 신뢰 설정 > 코드 서명을 '항상 신뢰' 로 바꿔라." >&2
    exit 1
fi
