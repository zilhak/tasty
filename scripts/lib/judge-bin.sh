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
# 다시 지으니 값이 바뀌었다). 그래서 소스와 안 맞으면 **없는 것으로 취급**한다. 없을 때
# 무엇을 할지는 게이트마다 다르므로(넓게 보기 · 판정 불가) 여기서 정하지 않는다.
#
# **신선도는 mtime 이 아니라 내용으로 묻는다.** mtime 으로 재면 git 이 파일을 다시 쓰기만
# 해도 낡은 것으로 나온다 — 내용이 같아도 그렇다. 브랜치를 main 으로 다시 잡았다가
# 체리픽으로 되돌리는 이 저장소의 표준 흐름이 정확히 그 왕복이라(실측: 내용 지문이
# 제자리로 돌아왔는데 mtime 만 새것), 경고가 상시 켜지고 폴백이 기본값이 된다. 그래서
# 판정기 자신에게 묻는다 — `--check-fresh` 가 구운 지문과 디스크를 대조한다.
#
# 종료코드 3(= 판정기 소스가 이 트리에 없다)은 **낡음이 아니다.** 배포 tarball 이나
# 합성 픽스처 저장소가 그 경우이고, 거기서 경고를 내면 정상 상황이 소음이 된다.
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
# 전부와 그 바이너리 자신의 소스 파일 하나다. 바이너리 디렉토리를 통째로 넣으면 **다른
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
    # 판정기에게 직접 묻는다. `--check-fresh` 를 모르는 옛 바이너리는 인자 오류로
    # 0 이 아닌 값을 내는데, 그건 **실제로 낡은 것**이라 그대로 낡음으로 다뤄도 옳다.
    "$_jb_bin" --check-fresh "$_jb_root" 2>/dev/null
    _jb_rc=$?
    if [ "$_jb_rc" -eq 1 ]; then
        echo "[judge] $_jb_name 이 지금 소스로 지어진 것이 아니다 — 옛 판정으로 돌지 않도록 없는 것으로 다룬다." >&2
        echo "        다시 지어라: cargo build -p tasty-doc-guards --bin $_jb_name" >&2
        echo "        (평범한 \`cargo build\` 는 이 바이너리를 짓지 않는다 — 실측으로 밟았다.)" >&2
        printf ''
        return 0
    fi
    if [ "$_jb_rc" -ne 0 ] && [ "$_jb_rc" -ne 3 ]; then
        echo "[judge] $_jb_name 의 신선도를 물을 수 없다(종료코드 $_jb_rc) — 없는 것으로 다룬다." >&2
        printf ''
        return 0
    fi
    printf '%s' "$_jb_bin"
}
