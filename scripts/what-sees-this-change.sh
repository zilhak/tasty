#!/usr/bin/env bash
#
# **무엇이 내 변경을 보는가** — 돌릴 것을 고르기 위한 발견 도구.
#
# 입력은 "이 작업이 무엇에 관한가" 가 아니라 **`git diff --name-only <base>`** 다.
# 그 구분이 이 도구가 존재하는 이유다(실측 2026-09-07): clap 도움말을 번역하는 작업에서
# 새로 만든 파일에 대해서만 "무엇이 이것을 보는가" 를 물었고, 같은 작업이 **스캔 뿌리인
# `crates/tasty-cli/src/lib.rs`** 에도 주석 두 줄을 넣어 가드를 빨갛게 만들었다. 그 파일은
# 가드 소스에 경로가 **리터럴로 박혀** 있어서, 변경집합을 입력으로 줬으면 1 초면 나왔다.
# 술어가 약했던 것이 아니라 **모수가 좁았다.**
#
# ★ 이것은 게이트가 아니라 **값 채널**이다. rc 는 언제나 0 이다 — 여기서 나온 목록은
#   "빨갛다" 가 아니라 "돌려 보라" 다. 판정은 그 타깃을 실제로 돌려서 한다.
#
# ★★ **이 술어가 못 잡는 것 — 안 적으면 이 도구가 거짓 안심을 판다.**
#
#   (가) **순회로 읽는 가드.** 레포를 확장자·shebang·디렉터리 순회로 훑는 가드는 네 파일
#        이름을 소스에 **안 적는다**. `tests/no_early_exit_consumer_in_shell_pipes.rs` 가
#        그 실물이다 — `scripts/` 라는 문자열을 한 번도 안 쓰고 셸을 성질로 모은다. 여기서는
#        **안 나온다.** 그 갈래는 이렇게 따로 묻는다:
#            grep -ln '"\.<확장자>"\|shebang\|read_dir\|WalkDir' tests/*.rs crates/*/tests/*.rs
#
#   (나) **문서를 인용해서 무는 가드.** 새로 적은 경로·`just` 레시피가 실재하는지 보는
#        가드(`cited_coordinates_exist` 등)는 **문서만 고쳤을 때** 빨개지는데, 그 가드 소스에
#        네가 고친 문서 이름이 없을 수 있다.
#
#   (다) **인용의 줄 번호.** 문서가 `파일:줄` 로 좌표를 적을 때, 실재를 보는 가드는 **파일만**
#        본다 — 줄 번호가 낡아도 아무도 안 짚는다(실측: 한 ADR 의 좌표가 224 줄 어긋난 채
#        오래 남아 있었다). 이 도구도 그 축은 못 본다.
#
#   (라) **링크로 딸려 오는 것.** 하네스(`mod common;`)는 여기 안 나온다. 그 물음은 빌드가
#        이미 답해 뒀다 — `target/debug/deps/<타깃>-<해시>.d`.
#
#   (마) 반대 방향의 것: 여기 **나왔다고 해서 그 파일을 고쳐야 하는 것이 아니다.** 이름으로
#        찾으면 그 이름이 이미 참인 자리까지 같이 나온다. 발견 목록은 **돌릴 것**을 고르는
#        데 쓰고, 고칠 것은 자리마다 다시 판정한다.
#
# ☆ 주석/코드 갈림은 **근사**로만 낸다. 줄 앞이 `//` · `#` 로 시작하는지만 본다 — 여러 줄
#   문자열 안의 언급은 못 가른다. 정확한 갈림이 필요하면 `mask-source` 를 쓴다. 여기서
#   근사를 쓰는 이유는 이 도구가 판정기 신선도에 **의존하지 않기** 위해서다(판정기가 낡으면
#   게이트는 exit 2 가 되는데, 발견 도구까지 같이 멈출 이유가 없다).
#
# 사용법:  scripts/what-sees-this-change.sh [<base>]      (기본 base: main)
#
set -u

BASE=${1:-main}

if ! git rev-parse --verify --quiet "$BASE" >/dev/null; then
    echo "판정 불가: base '$BASE' 를 못 찾는다." >&2
    exit 2
fi

# 작업 트리까지 포함한다 — 커밋 **전에** 고를 수 있어야 쓸모가 있다.
mapfile -t FILES < <(git diff --name-only "$BASE")

# 판정자 말뭉치. 여기 없는 곳에서 무는 것은 이 도구가 못 본다 — 위 (가)~(라).
mapfile -t CORPUS < <(
    git ls-files 'tests/*.rs' 'tests/**/*.rs' \
        'crates/*/tests/*.rs' 'crates/*/tests/**/*.rs' \
        'scripts/*.sh' '.githooks/*' 2>/dev/null
)

echo "발견 입력   git diff --name-only ${BASE}   (작업 트리 포함)"
echo "모수        변경 파일 ${#FILES[@]} · 판정자 말뭉치 ${#CORPUS[@]}"
echo

if [ "${#FILES[@]}" -eq 0 ]; then
    echo "변경 파일이 0 이다 — 발견할 것이 없다. (base 를 잘못 줬는지 먼저 의심해라)"
    exit 0
fi

mentioned=0
declare -A TARGETS=()

for f in "${FILES[@]}"; do
    # 파이프를 rc 재는 자리에 두지 않는다 — 값을 먼저 받는다.
    hits=$(grep -l -F -- "$f" "${CORPUS[@]}" 2>/dev/null)
    if [ -z "$hits" ]; then
        continue
    fi
    mentioned=$((mentioned + 1))
    echo "  $f"
    while IFS= read -r g; do
        [ -n "$g" ] || continue
        TARGETS["$g"]=1
        lines=$(grep -n -F -- "$f" "$g" 2>/dev/null)
        total=$(printf '%s\n' "$lines" | grep -c .)
        # 근사: 줄 앞이 주석 표지인가. 여러 줄 문자열은 못 가른다(헤더 ☆).
        cmt=$(printf '%s\n' "$lines" | grep -cE '^[0-9]+:[[:space:]]*(//|#)')
        printf '      %-58s 줄 %-3s (주석 근사 %s / 그 밖 %s)\n' \
            "$g" "$total" "$cmt" "$((total - cmt))"
    done <<<"$hits"
done

echo
echo "변경 파일 ${#FILES[@]} · 리터럴로 언급된 것 ${mentioned} · 대응 타깃 ${#TARGETS[@]}"
echo "★ 이것은 상한이 아니라 **하한**이다 — 순회로 읽는 가드는 여기 안 나온다(헤더 (가))."
exit 0
