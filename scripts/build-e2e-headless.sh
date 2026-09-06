#!/usr/bin/env bash
# e2e 하네스가 띄울 **헤드리스 데몬**을 짓고, 쓸 수 있으면 그 경로를 낸다.
#
# ── 왜 별도 빌드인가 ────────────────────────────────────────────────────
# 루트 `tasty` 에는 lib 타깃이 없다 — 바이너리 전용 패키지다. 그리고 `gui` 는 25 개 남짓의
# 의존성을 켜는 **패키지 단위** feature 다. cargo 의 feature 는 bin 타깃 단위가
# 아니므로 `[[bin]]` 을 하나 더 넣어 그 타깃만 `gui` 를 끄는 것은 **불가능**하다.
# 그래서 조합을 바꾸려면 별도 target 디렉토리로 한 번 더 짓는 수밖에 없다.
#
# ── 왜 이 경로인가 ──────────────────────────────────────────────────────
# host 는 exe 옆의 `builtin-plugins` 를 먼저 보고, 없으면 **exe 의 두 단계 위**를
# 워크스페이스 루트로 역산한다. `target-e2e-headless/debug/tasty` 는 두 단계 위가
# 레포 루트라 맞는다. `target/hl/debug/tasty` 처럼 `target/` **안**에 두면 역산이
# `crates/` 없는 경로를 가리켜 plugin namespace 가 통째로 빠지고, 그 증상은
# `Method not found: <ns>.<method>` 로 "헤드리스에 아직 배선 안 됨" 과 **문구가 같다.**
#
# ── 왜 --workspace 인가 ─────────────────────────────────────────────────
# 없으면 그 target 에 `tasty-plugin-*` 바이너리가 **하나도** 안 생긴다. 위와 증상이
# 같아서 둘을 증상으로 못 가른다 — 두 조건을 다 지키는 것이 유일한 대응이다.
#
# ── fail-safe ───────────────────────────────────────────────────────────
# 빌드가 실패하거나 결과가 낡았으면 **아무것도 내지 않는다.** 호출자가 그때
# `TASTY_E2E_BIN` 을 안 넘기면 하네스는 오늘 동작(`CARGO_BIN_EXE_tasty`)으로
# 떨어진다. 배선이 틀려도 초록이 거짓이 되지 않는 방향이다.
#
# 쓰는 법:
#   BIN=$(scripts/build-e2e-headless.sh) && export TASTY_E2E_BIN=$BIN
#
# 절차와 함정 전체: docs/dev-guide/e2e-tests.md §0-1
set -uo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
HL_DIR="$ROOT/target-e2e-headless"     # .gitignore 에 등재돼 있다
BIN="$HL_DIR/debug/tasty"

CARGO_TARGET_DIR="$HL_DIR" cargo build --workspace --no-default-features >&2
rc=$?
if [ $rc -ne 0 ]; then
  echo "헤드리스 빌드 실패(rc=$rc) — TASTY_E2E_BIN 을 넘기지 마라." >&2
  exit "$rc"
fi

if [ ! -x "$BIN" ]; then
  echo "빌드는 성공했는데 $BIN 이 없다 — 경로 규약이 바뀌었는지 확인해라." >&2
  exit 1
fi

# 낡음 확인. 하네스도 같은 판정을 하지만(`spawn_diag::source_newer_than`) 거기서
# 걸리면 **테스트가 패닉으로 죽는다** — 여기서 먼저 걸러 그냥 안 넘기는 편이 낫다.
# 순서를 뒤집어 읽지 마라: 아래 줄이 위층을 **먼저 막으려고** 있는 것이지, 위층이 이
# 줄을 받쳐 주는 것이 아니다.
#
# ★ 그래서 두 층은 같은 기제를 쓴다 — 같은 mtime, 같은 피연산자. **같은 판정이면 같이
# 죽는다**: mtime 을 못 읽는 파일은 두 층 모두 못 본다. "저쪽이 잡아 준다" 로 셈하지 마라.
#
# ★★ 그래서 **동률(`==`)도 낡음으로 센다** — 위층과 같은 극성이다. 소스가 바이너리와 같은
# 눈금에 떨어지면 순서를 알 수 없고, 그 판정 불가를 "안 낡았다" 로 흡수하면 이 판정이
# 막으려던 것(옛 코드에 대고 재기, 오진은 양방향)이 그대로 통과한다. 반대 방향의 비용은
# 다시 빌드 한 번이다.
#
# `find -newer` 는 **엄격 초과라 동률을 통과시킨다.** 그래서 쓰지 않는다 — 대신 같은
# `find` 가 낸 `%T@` 끼리 견준다(바이너리 쪽도 `find` 로 뽑아 **형식을 한 벌로** 맞춘다.
# `stat` 과 섞으면 소수 자릿수가 달라 비교가 형식 차이에 걸린다).
# ☆ awk 의 배정도는 유효숫자 ~16 자리라 1 µs 미만의 차이는 같은 값으로 뭉갤 수 있다.
# 그 방향은 "같다 → 낡음" 이라 **실패 쪽**이고, 위에서 고른 극성과 같다.
bin_mtime=$(find "$BIN" -maxdepth 0 -printf '%T@' 2>/dev/null)
if [ -z "$bin_mtime" ]; then
  echo "$BIN 의 mtime 을 못 읽었다 — 낡음 판정을 할 수 없다." >&2
  exit 1
fi
# ★ 파이프로 잇지 않는다 — 오른쪽 awk 가 첫 일치에서 `exit` 하는 **조기 종료 소비자**라,
# 파이프면 왼쪽 `find` 가 SIGPIPE 로 죽고 `pipefail` 아래에서 **찾았는데 실패**가 된다.
# producer 를 변수로 받아 히어스트링으로 넘긴다(비용 0).
all_src_mtimes=$(find "$ROOT/src" "$ROOT/crates" -name '*.rs' -printf '%T@ %p\n' 2>/dev/null)
stale=$(awk -v b="$bin_mtime" '$1 >= b { print $2; exit }' <<<"$all_src_mtimes")
if [ -n "$stale" ]; then
  echo "빌드 직후인데 $stale 이 더 새것이다 — 빌드 중에 소스가 바뀌었다. 다시 돌려라." >&2
  exit 1
fi

echo "$BIN"
