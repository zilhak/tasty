//! `resolve_judge` 를 부르는 **모든** 스크립트가 판정기 부재에서 종료 코드를 정해
//! 두었는가를 묻는다.
//!
//! 왜 필요한가: 격하 갈래는 **어디서도 안 돈다.** `script-gates.yml` 은 게이트를
//! 부르기 전에 판정기를 짓고(`cargo build -p tasty-doc-guards --bin mask-source`),
//! 훅도 자기 트리의 산출물을 쓴다. 그래서 그 갈래를 바꿔도 초록이 안 변한다 —
//! 실측: 한 게이트의 `exit 2` 를 `exit 0` 으로 바꿨더니 눈먼 실행이 조용히 통과했고
//! 어떤 타깃도 빨개지지 않았다. 판정기가 없을 때 **판정 불가(2)** 로 나가지 않으면,
//! 원문에서 나온 수가 위반으로 보고되고 그 실패문의 처방(면제 주석·상한 올리기)이
//! 실재하지 않는 결함을 **영구히** 봐주는 자국을 남긴다.
//!
//! **목록을 박지 않는다.** `scripts/` 에서 소비자를 세므로, 소비자를 하나 더 만드는
//! 사람이 이 파일을 안 고쳐도 그 스크립트가 이 물음을 받는다. 상수로 복사해 두면
//! 새로 들어오는 것이 안 세어진다.
//!
//! **예외는 주석이 아니라 여기 목록에 산다.** 스크립트 주석에 적으면 늘리는 비용이
//! 한 줄이고 리뷰에 안 뜬다 — 사유가 그럴듯하면 진짜 예외와 글자로 구분이 안 된다.
//! 목록에 두면 늘릴 때 이 파일이 바뀌고, 그 수가 세어진다. **지금 예외는 하나다.**
//!
//! `#![cfg(unix)]` 인 이유는 형제 셋과 같다 — `bash` 가 없는 플랫폼에서는 게이트가
//! 아니라 셸의 부재가 결과를 정한다.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

/// `scripts/` 의 `.sh` 순회 하한.
///
/// 이 가드의 물음은 "소비자 전부가 종료 코드를 정해 뒀는가" 라서, 순회가 죽어 소비자를
/// 하나도 못 모으면 **위반 0 으로 초록**이 된다 — 지키려던 것이 깨진 그 순간에도.
const SCRIPT_FLOOR: Floor = Floor {
    min: 16,
    measured: 24,
    measured_on: "2026-09-07",
    why_this_gap: "게이트·러너 스크립트는 회차마다 하나씩 늘고 가끔 하나가 접힌다 — 한 번에 \
                   크게 움직이는 모수가 아니다. 여유 8 은 그 폭을 견디되, 순회가 `scripts/lib` \
                   만 보거나 아예 안 내려간 상태는 잡는다",
};

/// 소비자로 세지 않는 파일: `resolve_judge` **정의**가 사는 곳.
const HELPER: &str = "lib/judge-bin.sh";

/// 판정기 부재에서 2 로 안 나가는 것이 **옳은** 소비자. 이름과 **사유**를 함께 둔다.
/// 사유 없는 항목은 아래 테스트가 막는다.
const EXCEPTIONS: &[(&str, &str)] = &[(
    "check-plugin-version-bump.sh",
    "일부러 넓게 본다. 여섯 게이트 중 pre-commit 이 부르는 유일한 것이라 갓 클론한 \
     트리에서 판정기가 없는 것이 정상 상황이고, 거기서 2 로 죽으면 커밋이 막힌다. \
     넓게 본 결과는 조용한 통과가 아니라 오탐(테스트 전용 변경이 bump 를 요구)이며, \
     그 처방인 patch +1 은 아무것도 헐겁게 만들지 않는다. 그 선택 자체는 \
     tests/plugin_version_bump_channel.rs 가 값으로 박는다.",
)];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `(스크립트 파일명, 판정기 환경변수)` 를 `scripts/` 에서 센다.
///
/// 호출 줄만 본다 — 주석 안의 사용례(`#   resolve_judge …`)와 산문을 찍는 `echo` 는
/// 줄 앞에 다른 것이 오므로 걸리지 않는다.
fn consumers() -> Vec<(String, String)> {
    let dir = root().join("scripts");
    let scripts = walk_with_floor(
        &dir,
        &dir,
        &SCRIPT_FLOOR,
        Descend::Everything,
        &|w: &Walked| w.rel.ends_with(".sh"),
    )
    .unwrap_or_else(|why| panic!("{why}"));

    let mut found: Vec<(String, String)> = Vec::new();
    for w in scripts {
        // 구분자는 공용 순회가 이미 `/` 로 폈다. 여기서 다시 펴면 안 된다 — 그 자리를
        // `floored_walk_consumers_do_not_renormalize` 가 짚는다. 안 폈다면 Windows 에서
        // `lib\\judge-bin.sh` 가 남아 아래 비교가 조용히 빗나가고, **정의가 사는 파일이
        // 소비자로 세어진다** — 예외도 실패도 아닌 오답이다.
        if w.rel == HELPER {
            continue;
        }
        let body = fs::read_to_string(&w.path).unwrap_or_default();
        for line in body.lines() {
            let t = line.trim_start();
            let Some(rest) = t.strip_prefix("resolve_judge ") else {
                continue;
            };
            let mut it = rest.split_whitespace();
            let _judge = it.next();
            let Some(env_var) = it.next() else { continue };
            let pair = (w.rel.clone(), env_var.to_string());
            if !found.contains(&pair) {
                found.push(pair);
            }
        }
    }
    found.sort();
    found
}

/// `cargo` 를 가로채는 디렉토리를 만든다.
///
/// 소비자 중 하나(`masked-tree.sh`)는 판정기가 없으면 **짓는다.** 그 줄을 그대로 돌리면
/// `cargo test` 안에서 중첩 cargo 가 빌드 디렉토리 잠금을 두고 서로를 기다린다. 스텁을
/// 물리면 그 갈래가 "지어도 여전히 없다" 로 흘러 자기 종료 코드에 닿는다. **이름으로
/// 가르지 않고 모든 소비자에 똑같이 물린다** — 이름 목록은 또 하나의 안 세어지는 표다.
fn stub_cargo() -> tempfile::TempDir {
    let d = tempfile::tempdir().expect("임시 디렉토리");
    let p = d.path().join("cargo");
    fs::write(&p, "#!/bin/sh\nexit 0\n").expect("스텁 cargo");
    let mut perm = fs::metadata(&p).expect("스텁 권한").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
    fs::set_permissions(&p, perm).expect("스텁 실행권한");
    d
}

fn run_blind(script: &str, env_var: &str, stub: &Path) -> (i32, String) {
    let path = format!(
        "{}:{}",
        stub.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new("bash")
        .arg(root().join("scripts").join(script))
        .current_dir(root())
        .env(env_var, "/nonexistent/judge")
        .env("PATH", path)
        .output()
        .unwrap_or_else(|e| panic!("{script} 실행 실패: {e}"));
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), text)
}

#[test]
fn every_judge_consumer_pins_its_absence_branch() {
    let stub = stub_cargo();
    let all = consumers();
    // 셈이 깨지면 이 테스트는 아무것도 안 물은 채 초록이 된다.
    assert!(
        !all.is_empty(),
        "scripts/ 에서 resolve_judge 소비자를 하나도 못 셌다 — 세는 줄이 깨졌다"
    );
    let mut bad = Vec::new();
    for (script, env_var) in &all {
        if EXCEPTIONS.iter().any(|(name, _)| name == script) {
            continue;
        }
        let (code, text) = run_blind(script, env_var, stub.path());
        if code != 2 {
            bad.push(format!("{script} → rc={code}\n{text}"));
        }
    }
    assert!(
        bad.is_empty(),
        "판정기가 없는데 판정 불가(2)로 안 나간다. 2 가 아닌 rc 는 그 값을 **판정**으로 \
         쓰겠다는 뜻이고, 그 실패문의 처방이 실재하지 않는 결함을 영구히 봐준다. \
         일부러 그런 소비자라면 이 파일의 EXCEPTIONS 에 사유와 함께 적어라 — \
         스크립트 주석에 적지 마라:\n{}",
        bad.join("\n---\n")
    );
}

#[test]
fn the_exception_list_names_only_real_consumers() {
    let all = consumers();
    for (name, _) in EXCEPTIONS {
        assert!(
            all.iter().any(|(script, _)| script == name),
            "EXCEPTIONS 의 '{name}' 은 더 이상 resolve_judge 를 안 부른다 — 지워라. \
             안 지우면 예외의 수가 실제보다 커 보인다"
        );
    }
}

#[test]
fn every_exception_carries_a_reason() {
    for (name, reason) in EXCEPTIONS {
        assert!(
            reason.chars().count() >= 40,
            "'{name}' 의 사유가 너무 짧다 — 왜 2 가 아닌 것이 옳은지를 적어야 한다"
        );
    }
}
