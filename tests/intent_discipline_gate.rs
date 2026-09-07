//! Intent 규율 게이트(`scripts/check-intent-discipline.sh`)의 **종료 코드**를 합성
//! 트리로 고정한다.
//!
//! 왜 이 게이트인가. 형제 셸 게이트 중 이것만 **잔여 0 hard-fail** 이다 — 나머지
//! 둘(`check-allow-reason.sh` · `check-shared-walk-ratchet.sh`)은 여유 0 양방향
//! 래칫이라 `rc=0 ⟺ 값 == 상한` 이고, 좌변이 반쯤 죽어 값이 줄면 하한 쪽에서 자기가
//! 빨개진다. 픽스처가 없어도 술어가 자기를 반쯤 잰다. 이 게이트에는 그 하한이 없다:
//! 좌변이 반쯤 줄어도 위반이 0 이면 **그냥 초록이다.** 그래서 셸 게이트 여섯 중
//! 계약을 아무도 안 재는 자리가 여기였다.
//!
//! **판정 방식**: 진짜 레포를 보지 않는다. 임시 루트에 게이트와 그것이 source 하는
//! 공용 판정기 찾기를 깔고, 면제 명부가 요구하는 경로를 빈 파일로 실재시킨 뒤 합성
//! `src/` 하나를 넣는다. 그래서 레포 내용이 바뀌어도 여기 값이 안 흔들리고, **면제
//! 경로를 지워야만 만들 수 있는 경우**도 잴 수 있다. 자매 픽스처
//! (`tests/frozen_sum_ratchet_gate.rs`)의 합성 트리 방식을 그대로 빌린다.
//!
//! 면제 명부는 **외우지 않고 스크립트에서 읽는다.** 외우면 명부가 바뀌는 날 이 파일이
//! 조용히 다른 것을 재게 된다 — `src/source_guards/sloc_gate_skip_proxy.rs` 가 자매
//! 게이트의 임계와 skip 목록에 같은 규율을 쓴다.
//!
//! 세 종류를 **서로 다른 종료코드**로 요구한다: 위반(1) / 통과(0) / 판정 불가(2).
//! 셋 중 둘이 같은 값으로 붕괴하면 여기서 죽는다.

#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::Command;

/// 판정 대상이 되는 도메인 직접 호출 한 줄. 게이트의 popup 술어가 잡는 형태다.
const CALL: &str = "    app.popups.open(PopupId::Settings);";

fn gate_src() -> String {
    format!(
        "{}/scripts/check-intent-discipline.sh",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// 게이트가 면제하는 경로 전부(`EXEMPT_ALL` + `EXEMPT_PANE`)를 스크립트에서 읽는다.
///
/// 합성 트리에 이 경로들이 실재하지 않으면 게이트는 판정 불가로 나간다 — 그것이
/// 이 파일의 세 번째 극성이므로, 나머지 둘을 재려면 **먼저 전부 실재시켜야** 한다.
fn exempt_paths() -> Vec<String> {
    let text = fs::read_to_string(gate_src()).expect("게이트를 읽을 수 없다");
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("EXEMPT_ALL=(") || t.starts_with("EXEMPT_PANE=(") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if t == ")" {
            inside = false;
            continue;
        }
        if let Some(p) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            out.push(p.to_string());
        }
    }
    assert!(
        !out.is_empty(),
        "게이트에서 면제 명부를 못 읽었다 — 배열 형식이 바뀌었다"
    );
    out
}

/// 게이트가 판정 불가를 낼 이유가 하나도 없는 합성 루트.
fn synth_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("임시 디렉토리");
    let root = dir.path();
    let here = env!("CARGO_MANIFEST_DIR");
    fs::create_dir_all(root.join("scripts/lib")).expect("scripts/lib");
    fs::copy(gate_src(), root.join("scripts/check-intent-discipline.sh")).expect("게이트 복사");
    // 게이트가 판정기 찾기를 공용 `resolve_judge` 에 위임하므로 그 파일도 합성 루트에
    // 있어야 한다. 없으면 `set -e` 아래에서 source 가 죽어 **판정 불가(2)가 아니라**
    // 다른 값이 나온다 — 게이트가 무엇을 재는지와 무관한 값이라 원인이 안 보인다.
    fs::copy(
        format!("{here}/scripts/lib/judge-bin.sh"),
        root.join("scripts/lib/judge-bin.sh"),
    )
    .expect("판정기 찾기 공용 복사");
    for p in exempt_paths() {
        let f = root.join(&p);
        fs::create_dir_all(f.parent().expect("면제 경로의 부모")).expect("면제 경로 디렉토리");
        fs::write(&f, "// 합성 픽스처: 이 경로가 실재한다는 사실만 만든다.\n")
            .expect("면제 경로 파일");
    }
    dir
}

fn write_src(root: &Path, rel: &str, body: &str) {
    let f = root.join(rel);
    fs::create_dir_all(f.parent().expect("소스의 부모")).expect("소스 디렉토리");
    fs::write(&f, body).expect("합성 소스");
}

/// 합성 루트에 **스텁 판정기**를 깐다 — 진짜 `mask-source` 를 안 쓴다.
///
/// 이유는 자매 픽스처와 같다(`tests/file_sloc_gate_fails_loudly.rs` ·
/// `tests/frozen_sum_ratchet_gate.rs`): 진짜 판정기는 cargo 산출물이라 여기서 지으면
/// 바깥 `cargo test` 와 빌드 디렉토리 잠금을 두고 서로를 기다린다.
///
/// 스텁이 **베끼기만 해도 충실한** 이유: 마스킹이 지우는 것은 주석과 문자열 안의 내용인데
/// 이 픽스처의 합성 소스에는 그것이 없다. 그래서 마스킹 사본과 원문이 같고, 스텁은 그
/// 사실을 그대로 구현한다. 픽스처에 주석 속 호출을 넣기 시작하면 이 전제가 깨지므로
/// 그때는 스텁도 함께 바뀌어야 한다.
fn install_stub_masker(root: &Path) -> std::path::PathBuf {
    let bin = root.join("stub-mask-source");
    fs::write(
        &bin,
        "#!/bin/sh\n\
         # --check-fresh <root> 는 신선하다고 답한다(스텁에는 낡을 소스가 없다).\n\
         if [ \"$1\" = \"--check-fresh\" ]; then exit 0; fi\n\
         if [ \"$1\" = \"--keep-comments\" ]; then shift; fi\n\
         out=$1; shift; src=$1; shift\n\
         mkdir -p \"$out\" || exit 1\n\
         for d in \"$@\"; do\n\
         [ -d \"$src/$d\" ] || continue\n\
         cp -r \"$src/$d\" \"$out/$d\" || exit 1\n\
         done\n\
         exit 0\n",
    )
    .expect("스텁 판정기");
    let mut perm = fs::metadata(&bin).expect("스텁 권한 읽기").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
    fs::set_permissions(&bin, perm).expect("스텁 실행권한");
    bin
}

/// 합성 루트에서 게이트를 돌리고 (종료코드, 출력) 을 얻는다. **판정기가 있는** 경로다.
fn run(root: &Path) -> (i32, String) {
    let stub = install_stub_masker(root);
    let out = Command::new("bash")
        .arg(root.join("scripts/check-intent-discipline.sh"))
        .current_dir(root)
        .env("TASTY_MASK_SOURCE_BIN", &stub)
        .output()
        .expect("게이트 실행");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap_or(-1), text)
}

#[test]
fn a_direct_domain_call_is_a_violation() {
    let d = synth_root();
    write_src(
        d.path(),
        "src/zz_direct_call.rs",
        &format!("fn f(app: &mut App) {{\n{CALL}\n}}\n"),
    );
    let (code, text) = run(d.path());
    assert_eq!(code, 1, "직접 호출인데 위반이 아니다:\n{text}");
    assert!(
        text.contains("src/zz_direct_call.rs"),
        "위반의 좌표를 안 찍는다 — 어디를 고쳐야 하는지가 안 나온다:\n{text}"
    );
}

/// 대조군. 이것이 없으면 위 시험은 **항상 실패하는 게이트**로도 통과한다.
///
/// 빈 트리로 0 을 만들지 않는다 — 그러면 좌변이 죽은 상태와 구분이 안 된다. 같은
/// 호출에 사유 주석만 붙여, 술어가 그 자리를 보고 있으면서 사유를 인정하는 것까지 건다.
#[test]
fn the_same_call_with_a_reason_passes() {
    let d = synth_root();
    write_src(
        d.path(),
        "src/zz_direct_call.rs",
        &format!(
            "fn f(app: &mut App) {{\n    // intent-exempt: 합성 픽스처의 대조군\n{CALL}\n}}\n"
        ),
    );
    let (code, text) = run(d.path());
    assert_eq!(code, 0, "사유가 달렸는데 막혔다:\n{text}");
}

/// 면제 명부가 없는 경로를 가리키면 **판정 불가**다. 통과(0)도 위반(1)도 아니다.
///
/// 이 갈래가 게이트에 있는 이유는 실측이다 — 트리를 재조직하면서 면제 경로가 죽었는데
/// 없는 경로는 조용히 무시되고 그 파일들의 정당한 호출이 위반으로 쌓였다.
#[test]
fn an_exempt_path_that_does_not_exist_is_undecidable() {
    let d = synth_root();
    write_src(d.path(), "src/zz_quiet.rs", "fn f() {}\n");
    let victim = exempt_paths().remove(0);
    fs::remove_file(d.path().join(&victim)).expect("면제 경로 삭제");
    let (code, text) = run(d.path());
    assert_eq!(code, 2, "면제 경로가 죽었는데 판정 불가가 아니다:\n{text}");
    assert!(
        text.contains(&victim),
        "죽은 경로를 지목하지 않는다 — 무엇이 안 따라왔는지가 안 나온다:\n{text}"
    );
}

/// ★ 판정기가 없으면 **판정 불가(2)** 다 — 위반(1)도 통과(0)도 아니다.
///
/// 이 갈래가 없던 동안 게이트는 원문을 그대로 훑고 **위반 1** 을 냈다. 원문 계수는 더
/// 많이 잡는 방향이라 래칫에서는 안전하지만, 이 게이트는 잔여 0 hard-fail 이라 그
/// 방향이 그대로 해가 된다 — 더 잡은 것이 위반으로 나가고 실패문이
/// "`// intent-exempt: <사유>` 를 추가하세요" 로 나간다. **실재하지 않는 위반에 대한
/// 처방이고, 따르면 그 자리가 영구히 면제된다.**
///
/// 실측 2026-09-07 (진짜 레포): 판정기를 안 보이게 하면 위반 1 건이 나왔고 그것이
/// `src/adapters/ui/popup/frame.rs` 의 **doc 주석 한 줄**이었다 — 마스킹이 지우려고
/// 존재하는 바로 그것이다.
///
/// 여기서 **위반이 있는 픽스처**를 쓰는 이유: 위반이 0 인 트리로 재면 2 와 0 이 갈리는
/// 것만 보이고, 이 결함의 실제 모양(1 이 나오던 자리)을 안 짚는다.
#[test]
fn a_missing_judge_is_undecidable_not_a_violation() {
    let d = synth_root();
    write_src(
        d.path(),
        "src/zz_direct_call.rs",
        &format!("fn f(app: &mut App) {{\n{CALL}\n}}\n"),
    );
    let out = Command::new("bash")
        .arg(d.path().join("scripts/check-intent-discipline.sh"))
        .current_dir(d.path())
        // 실행 가능하지 않은 경로를 지목한다. `resolve_judge` 는 이때 **기본 위치로
        // 물러나지 않는다** — 지목은 "그것으로 재라" 는 뜻이라 다른 것을 대신 쓰면
        // 요청한 판정과 다른 판정이 조용히 돈다(그 함수 doc).
        .env("TASTY_MASK_SOURCE_BIN", "/nonexistent/mask-source")
        .output()
        .expect("게이트 실행");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.status.code().unwrap_or(-1),
        2,
        "판정기가 없는데 판정 불가가 아니다 — 1 이면 실재하지 않을 수 있는 위반에 \
         면제 주석을 달라는 처방이 나간다:\n{text}"
    );
    assert!(
        text.contains("mask-source"),
        "무엇을 지어야 하는지가 안 나온다 — 처방 없는 판정 불가는 다음 사람을 세운다:\n{text}"
    );
}

// ── 좌변이 갈리면 죽는다 ─────────────────────────────────────────────────
//
// 이 게이트는 같은 트리를 **네 번** 지목한다: 마스킹 사본 · 파일 수 · 위반 훑기 ·
// 사유 읽기. 넷이 따로 적혀 있던 동안 훑는 좌변과 사유 좌변이 서로를 몰랐고, 한쪽만
// 넓히면 새 영토의 `[...]` 태그 검사가 **rc=0 초록인 채 꺼졌다**(실측 2026-09-07:
// 오타 태그 셋을 `crates/` 에 심고 훑는 좌변만 넓혔더니 셋 다 안 읽혔다).
//
// **왜 문자열 확인이 아닌가.** "게이트에 `SCAN_DIRS` 가 몇 번 나온다" 를 세는 형태를
// 골랐다면 그것은 **철자를 보는 것**이지 네 소비처가 같은 값을 낸다는 것을 보는 게
// 아니다. 변수를 선언해 놓고 소비처 하나가 `src` 를 그대로 쓰고 있어도 그 검사는
// 통과한다 — 정확히 지금 고친 결함의 모양이다.
//
// 그래서 **좌변 값을 늘려 놓고 소비처가 따라오는지를 종료 코드로 묻는다.** 게이트
// 사본의 `SCAN_DIRS=(src)` 를 `SCAN_DIRS=(src extra)` 로 바꾸고, `extra/` 에 프로브를
// 심는다. 치환이 안 되면(그 줄이 없으면) 그 자리에서 죽는다 — 좌변이 다시 여럿으로
// 흩어졌다는 뜻이라 그것도 잡아야 할 회귀다.
//
// **프로브가 둘인 이유: 하나로는 두 변이 중 하나만 죽는다.**
//   - 훑는 좌변만 `src` 로 되돌린 변이 → 사유 grep 은 `extra/` 를 보므로 태그 프로브는
//     여전히 rc=1 을 낸다. 안 죽는다. 사유 **없는** 프로브가 죽인다.
//   - 사유 좌변만 `src` 로 되돌린 변이 → 위반 프로브는 그대로 rc=1 을 낸다. 안 죽는다.
//     **태그** 프로브가 죽인다.
// 둘이 짝으로 두 방향을 각각 덮는다. rc 만으로는 갈래가 안 갈리므로 실패 문구까지 건다.

/// 게이트 사본의 좌변을 `(src extra)` 로 늘린다. 늘어난 쪽이 안 보이면 시험이 무의미하다.
fn widen_scan_dirs(root: &Path) {
    let p = root.join("scripts/check-intent-discipline.sh");
    let text = fs::read_to_string(&p).expect("게이트 사본을 읽을 수 없다");
    let widened = text.replace("SCAN_DIRS=(src)", "SCAN_DIRS=(src extra)");
    assert_ne!(
        widened, text,
        "게이트에 `SCAN_DIRS=(src)` 한 줄이 없다 — 좌변이 다시 여럿으로 흩어졌거나 \
         이름이 바뀌었다. 이 시험은 그 한 값을 늘려 소비처를 재므로 여기서 멈춘다."
    );
    fs::write(&p, widened).expect("게이트 사본 쓰기");
}

/// 좌변을 늘리면 **훑는 쪽**(마스킹 · find)이 따라온다.
///
/// 이 자리를 죽이는 변이: `find` 나 마스킹 인자를 `src` 로 되돌리는 것. 그러면 `extra/`
/// 의 사유 없는 호출이 안 보여 rc=0 이 된다.
#[test]
fn widening_the_left_side_moves_the_scan() {
    let d = synth_root();
    write_src(d.path(), "src/zz_quiet.rs", "fn f() {}\n");
    write_src(
        d.path(),
        "extra/zz_probe.rs",
        &format!("fn f(app: &mut App) {{\n{CALL}\n}}\n"),
    );
    widen_scan_dirs(d.path());
    let (code, text) = run(d.path());
    assert_eq!(
        code, 1,
        "좌변을 늘렸는데 훑는 쪽이 안 따라왔다 — 새 영토의 직접 호출이 안 보인다:\n{text}"
    );
    assert!(
        text.contains("extra/zz_probe.rs"),
        "새 영토의 좌표를 안 찍는다:\n{text}"
    );
}

/// 좌변을 늘리면 **사유를 읽는 쪽**(두 번째 검사의 grep)도 따라온다.
///
/// 이 자리를 죽이는 변이: 사유 grep 을 `src` 로 되돌리는 것. 그러면 `extra/` 의 오타
/// 태그가 안 읽혀 **rc=0 초록**이 된다 — 검사가 있다는 사실 자체가 거짓이 되는 모양이다.
///
/// 태그가 `[겨과사용]` 인 것은 오타다. 일부러다: 실측에서 검사를 조용히 끈 것이 이
/// 형태였고, 알려진 둘(`[결과사용]` · `[부재 …]`) 이 아니면 실패라는 계약을 여기서 건다.
#[test]
fn widening_the_left_side_moves_the_reason_check() {
    let d = synth_root();
    write_src(d.path(), "src/zz_quiet.rs", "fn f() {}\n");
    write_src(
        d.path(),
        "extra/zz_probe.rs",
        &format!(
            "fn f(app: &mut App) {{\n    // intent-exempt: [겨과사용] 오타 태그\n{CALL}\n}}\n"
        ),
    );
    widen_scan_dirs(d.path());
    let (code, text) = run(d.path());
    assert_eq!(
        code, 1,
        "좌변을 늘렸는데 사유 검사가 안 따라왔다 — 새 영토의 오타 태그가 안 읽혀 \
         초록이 나온다. 검사가 있다는 사실 자체가 거짓이 되는 자리다:\n{text}"
    );
    assert!(
        text.contains("모르는 사유 태그"),
        "위반으로는 잡혔는데 태그 검사가 안 돌았다 — 이 시험이 재려던 것이 아니다:\n{text}"
    );
    assert!(
        text.contains("extra/zz_probe.rs"),
        "새 영토의 좌표를 안 찍는다:\n{text}"
    );
}

/// 좌변을 늘리면 **초록이 찍는 수**도 따라온다.
///
/// 위 둘로는 이 자리가 안 죽는다(실측: 파일 수 소비처만 `src` 로 되돌린 변이에서
/// 여섯 시험 전부 통과했다). 그 소비처는 종료 코드에 안 들어가고 **찍히는 수**에만
/// 들어가기 때문이다. 그런데 그 수는 장식이 아니다 — 게이트 본문이 적듯 수를 안 찍으면
/// 초록이 "위반이 없다" 와 "아무것도 안 봤다" 가 같은 모양이 된다. 수가 좌변을 안
/// 따라가면 그 방어가 반쪽이 된다: 넓힌 영토를 안 보고도 초록이 같은 수를 찍는다.
///
/// **명부 크기를 외우지 않는다.** 합성 트리에는 면제 경로 파일이 함께 깔리고 그 수는
/// 게이트가 정한다. 그래서 절대값이 아니라 **같은 트리의 늘리기 전후 차**를 본다 —
/// 늘린 것이 파일 하나이므로 차는 정확히 1 이다.
#[test]
fn widening_the_left_side_moves_the_reported_count() {
    fn scanned(text: &str) -> usize {
        let tail = text.split(".rs ").nth(1).unwrap_or_else(|| {
            panic!("초록 문구에서 훑은 수를 못 읽었다 — 형식이 바뀌었다:\n{text}")
        });
        tail.split('개')
            .next()
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or_else(|| panic!("훑은 수가 숫자가 아니다:\n{text}"))
    }

    let d = synth_root();
    write_src(d.path(), "src/zz_quiet.rs", "fn f() {}\n");
    write_src(d.path(), "extra/zz_quiet.rs", "fn g() {}\n");

    let (code, before) = run(d.path());
    assert_eq!(code, 0, "좌변을 늘리기 전인데 초록이 아니다:\n{before}");

    widen_scan_dirs(d.path());
    let (code, after) = run(d.path());
    assert_eq!(code, 0, "좌변을 늘렸더니 초록이 아니다:\n{after}");

    assert_eq!(
        scanned(&after),
        scanned(&before) + 1,
        "좌변을 늘렸는데 훑은 수가 안 늘었다 — 넓힌 영토를 안 보고도 초록이 같은 수를 \
         찍는다. 그 수는 '아무것도 안 봤다' 와 '위반이 없다' 를 가르려고 있는 것이다.\
         \n늘리기 전:\n{before}\n늘린 뒤:\n{after}"
    );
}
