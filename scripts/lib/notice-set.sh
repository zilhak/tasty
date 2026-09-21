# shellcheck shell=bash
#
# 이 파일은 `source` 되는 라이브러리라 shebang 이 없다 — 실행 파일이 아니다(위 지시자가
# 정적 검사기에게 대상 셸을 알려 준다. 같은 형태의 본보기는 `scripts/lib/judge-bin.sh`).
#
# 고지 세트를 배포 트리에 스테이징한다. 세트가 무엇인지는 THIRD_PARTY_LICENSES.md 의
# "고지 세트" 절이 정본이고, 생성하지 않고 저장소의 파일을 그대로 나르는 근거는
# docs/adr/0317-the-notice-set-is-staged-not-generated.md 에 있다.
#
# 부르는 쪽은 레포 루트를 cwd 로 둔 채 이 파일을 source 한다 — 아래 경로가 전부 루트
# 기준 상대 경로다.

# Stage the notice set into a distribution tree. `LICENSES/` keeps its
# subdirectory so that the relative links inside THIRD_PARTY_LICENSES.md still
# resolve.
stage_notice() {
    local dest="$1"
    mkdir -p "$dest/LICENSES"
    cp LICENSE "$dest/LICENSE"
    cp THIRD_PARTY_LICENSES.md "$dest/THIRD_PARTY_LICENSES.md"
    cp LICENSES/D2Coding-OFL.txt "$dest/LICENSES/D2Coding-OFL.txt"
}
