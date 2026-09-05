# 게이트가 부르는 **판정기 바이너리**를 찾는다 — 그리고 낡았는지 본다.
#
# 왜 공용인가: 이 레포의 셸 게이트 여럿이 같은 크레이트의 CLI 판정기를 부른다. 찾는
# 규칙(환경변수 → debug → release)과 **신선도 판정**을 게이트마다 따로 쓰면 같은 물음에
# 답이 여럿이 되고, 갈린 쪽은 조용하다. 판정기를 하나로 모으는 규율을 판정기를 찾는
# 코드에도 적용한다.
#
# **낡은 판정기는 없는 판정기보다 나쁘다.** 판정기가 빌드 산출물이라, 고친 사람과
# 판정을 돌리는 사람이 다르면 **고침이 소스에 있는데 판정은 옛 규칙으로 돈다** — 그리고
# 그 오진은 조용하다(실측 2026-09-05: 고침을 얹은 트리에서 게이트가 옛 결과를 냈고,
# 다시 지으니 값이 바뀌었다). 그래서 소스보다 낡으면 **없는 것으로 취급**한다. 없을 때
# 무엇을 할지는 게이트마다 다르므로(넓게 보기 · 판정 불가) 여기서 정하지 않는다.
#
# 여기서 `cargo build` 를 하지 않는다 — 이 계열 스크립트는 `cargo test` 안에서도 불리고,
# 그때 중첩 cargo 는 빌드 디렉토리 잠금에서 서로를 기다린다.
#
# 사용:
#   . scripts/lib/judge-bin.sh
#   BIN="$(resolve_judge strip-cfg-test TASTY_STRIP_CFG_TEST_BIN "$ROOT")"
#   [ -n "$BIN" ] || <게이트가 정한 처리>
#
# 표준출력은 경로(없으면 빈 문자열)뿐이다. 사유는 표준오류로 나간다 — 호출자가
# `$(...)` 로 받기 때문에 여기서 섞으면 경로가 오염된다.

# 판정기 소스. **그 판정기가 실제로 링크하는 것만** 본다 — 크레이트의 라이브러리
# 전부와 그 바이너리 자신의 `src/bin/<이름>.rs` 다. `src/bin/` 을 통째로 넣으면 **다른
# 판정기를 하나 추가한 것이 이 판정기를 낡게** 만든다(실측으로 밟았다). 반대로 라이브러리를
# 빼면 진짜 낡음을 놓친다 — 오늘의 실사례가 라이브러리 쪽(`cfg_predicate`) 변경이었다.
JUDGE_SRC_DIR="crates/tasty-doc-guards/src"

resolve_judge() {
    _jb_name="$1"; _jb_env="$2"; _jb_root="$3"
    # 셸에 따라 `${!var}` 가 없어서 eval 로 읽는다(POSIX 범위).
    eval "_jb_bin=\${$_jb_env:-}"
    # 넘겨받은 경로가 실행 가능하지 않으면 **없는 것**이고, 여기서 끝난다 — 기본 위치로
    # 물러나지 않는다. 호출자가 경로를 지목한 것은 **그것으로 재라**는 뜻이라, 다른 것을
    # 대신 쓰면 요청한 판정과 다른 판정이 조용히 돈다(게이트의 회귀가 그 형태를 고정한다).
    if [ -n "$_jb_bin" ] && [ ! -x "$_jb_bin" ]; then
        echo "[judge] $_jb_env 가 가리키는 것이 실행 가능하지 않다: $_jb_bin" >&2
        printf ''
        return 0
    fi
    if [ -z "$_jb_bin" ]; then
        for _jb_cand in "$_jb_root/target/debug/$_jb_name" "$_jb_root/target/release/$_jb_name"; do
            if [ -x "$_jb_cand" ]; then _jb_bin="$_jb_cand"; break; fi
        done
    fi
    if [ -z "$_jb_bin" ]; then
        echo "[judge] $_jb_name 이 없다 — 지어라: cargo build -p tasty-doc-guards --bin $_jb_name" >&2
        printf ''
        return 0
    fi
    # 판정기 소스가 이 트리에 없으면(픽스처 저장소·배포 tarball) 신선도를 물을 수 없다.
    if [ -d "$_jb_root/$JUDGE_SRC_DIR" ] && [ -n "$(
        find "$_jb_root/$JUDGE_SRC_DIR" -name '*.rs' -newer "$_jb_bin" \
             \( -path "*/bin/*" -a ! -path "*/bin/$_jb_name.rs" -o -true \) \
             ! \( -path "*/bin/*" -a ! -path "*/bin/$_jb_name.rs" \) \
             -print -quit 2>/dev/null)" ]; then
        echo "[judge] $_jb_name 이 자기 소스보다 낡았다 — 옛 판정으로 돌지 않도록 없는 것으로 다룬다." >&2
        echo "        다시 지어라: cargo build -p tasty-doc-guards --bin $_jb_name" >&2
        printf ''
        return 0
    fi
    printf '%s' "$_jb_bin"
}
