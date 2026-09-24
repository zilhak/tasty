# source용 도구 탐색 함수. resolve_judge는 stdout 대신 JUDGE_BIN으로 결과를 전달한다.
# 환경변수의 명시 경로를 우선하며, 없으면 저장소 target의 debug/release를 찾는다.
# --check-fresh로 소스 일치를 확인한다. 도구를 찾지 못해도 여기서 Cargo를 실행하지 않는다.
# shellcheck shell=bash

# 이유: source한 스크립트가 필요할 때 사용할 경로다.
# shellcheck disable=SC2034
JUDGE_SRC_DIR="crates/tasty-doc-guards/src"

resolve_judge() {
    # 실패한 호출에서 이전 결과를 반환하지 않도록 먼저 비운다.
    JUDGE_BIN=""
    _jb_name="$1"; _jb_env="$2"; _jb_root="$3"
    eval "_jb_bin=\${$_jb_env:-}"
    # 명시 경로가 잘못됐으면 기본 경로로 대체하지 않는다.
    if [ -n "$_jb_bin" ] && [ ! -x "$_jb_bin" ]; then
        echo "[judge] $_jb_env 가 가리키는 것이 실행 가능하지 않다: $_jb_bin" >&2
        return 0
    fi
    if [ -z "$_jb_bin" ]; then
        for _jb_cand in "$_jb_root/target/debug/$_jb_name" "$_jb_root/target/release/$_jb_name"; do
            if [ -x "$_jb_cand" ]; then _jb_bin="$_jb_cand"; break; fi
        done
    fi
    if [ -z "$_jb_bin" ]; then
        echo "[judge] $_jb_name 이 없다. 빌드 명령: cargo build -p tasty-doc-guards --bin $_jb_name" >&2
        return 0
    fi
    # --check-fresh의 rc 0과 3을 허용한다. 지원하지 않는 도구나 다른 오류는 거부한다.
    if "$_jb_bin" --check-fresh "$_jb_root" >/dev/null 2>&1; then
        _jb_rc=0
    else
        _jb_rc=$?
    fi
    if [ "$_jb_rc" -eq 1 ]; then
        echo "[judge] $_jb_name 이 지금 소스로 지어진 것이 아니다 — 이전 소스의 결과를 쓰지 않도록 사용을 거부한다." >&2
        echo "        다시 빌드해라: cargo build -p tasty-doc-guards --bin $_jb_name" >&2
        echo "        (루트 패키지의 \`cargo build\`만으로는 이 도구를 빌드하지 않는다.)" >&2
        return 0
    fi
    if [ "$_jb_rc" -ne 0 ] && [ "$_jb_rc" -ne 3 ]; then
        echo "[judge] $_jb_name 의 소스 일치 여부를 확인할 수 없다(종료코드 $_jb_rc) — 없는 것으로 다룬다." >&2
        return 0
    fi
    # 이유: source한 스크립트가 이 변수로 결과를 받는다.
    # shellcheck disable=SC2034
    JUDGE_BIN="$_jb_bin"
}
