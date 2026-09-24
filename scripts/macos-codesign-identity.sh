#!/bin/bash
# macOS 로컬 개발용 코드 서명 identity를 조회하거나 자체 서명 인증서를 만든다.
# 사용: ./scripts/macos-codesign-identity.sh [--create]
# TASTY_CODESIGN_IDENTITY로 이름을 지정한다. --create는 로그인 키체인과 인증서 신뢰 설정을 변경한다.
# 같은 인증서를 사용하는 빌드를 위한 도구이며 TCC 승인 유지나 배포용 신뢰를 보장하지 않는다.

set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "Error: This script must be run on macOS." >&2
    exit 1
fi

IDENTITY_NAME="${TASTY_CODESIGN_IDENTITY:-Tasty Dev}"

list_identities() {
    security find-identity -v -p codesigning 2>/dev/null || true
}

# grep -q의 조기 종료로 security가 SIGPIPE를 받지 않도록 출력을 먼저 받는다.
has_identity() {
    local identities
    identities=$(list_identities)
    grep -q "$1" <<<"$identities"
}

if [[ "${1:-}" != "--create" ]]; then
    echo "==> 사용 가능한 코드 서명 identity:"
    list_identities
    echo ""
    if has_identity "$IDENTITY_NAME"; then
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

if has_identity "$IDENTITY_NAME"; then
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

# macOS 호환용 PKCS#12를 먼저 시도한다. -legacy 미지원 구현에서는 기본 옵션으로 재시도한다.
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

# 인증서를 로그인 키체인의 코드 서명 신뢰 대상으로 등록한다. 관리자 암호를 요청할 수 있다.
echo ""
echo "==> 인증서 신뢰 설정 중 — macOS 가 암호를 물어본다."
security add-trusted-cert -r trustRoot -p codeSign \
    -k "$HOME/Library/Keychains/login.keychain-db" "$WORK_DIR/cert.pem"

echo ""
if has_identity "$IDENTITY_NAME"; then
    echo "완료. '$IDENTITY_NAME' 로 서명하려면:"
    echo "  ./scripts/install-macos.sh    # 키체인에서 '$IDENTITY_NAME' 를 자동으로 집는다"
    echo ""
    echo "권한 승인 유지 여부는 macOS 정책과 앱 서명 조건에 따라 달라질 수 있다."
else
    echo "Error: 발급은 끝났지만 '$IDENTITY_NAME' 이(가) 서명 identity 로 잡히지 않는다." >&2
    echo "키체인 접근.app 에서 인증서의 신뢰 설정 > 코드 서명을 '항상 신뢰' 로 바꿔라." >&2
    exit 1
fi
