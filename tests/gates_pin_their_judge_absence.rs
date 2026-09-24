//! scripts의 resolve_judge 호출을 수집해 판정기 부재 시 측정 실패로 종료하는지 확인한다.
//! 예외는 EXCEPTIONS에 사유와 함께 등록한다. Bash를 사용하는 Unix 전용 시험이다.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

/// 스크립트 수집 누락 때문에 소비자가 0개인 채 통과하지 않도록 하한을 둔다.
const SCRIPT_FLOOR: Floor = Floor {
    min: 16,
    measured: 27,
    measured_on: "2026-09-20",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::Tree(
        "e3a747ea4 — scripts 아래 .sh를 재귀로 세면 27 개다. 측정 선언과 새 스크립트를 추가한 커밋이며 현재 이력에서 다시 확인할 수 있다.",
    ),
    why_this_gap: "측정 27 개와 하한 16 사이 여유는 작은 추가·삭제를 허용하고 scripts/lib만 수집하거나 순회를 생략하는 오류를 찾는다. 전체 파일의 수집을 보장하지는 않는다.",
};

/// resolve_judge 정의 파일은 소비자에서 제외한다.
const HELPER: &str = "lib/judge-bin.sh";

const EXCEPTIONS: &[(&str, &str)] = &[(
    "check-plugin-version-bump.sh",
    "pre-commit은 갓 복제한 저장소처럼 판정기가 없는 환경에서도 실행된다. 이때 원문을 넓게 검사해 test 전용 변경에도 버전 증가를 요구할 수 있지만 제품 변경을 누락하지 않는 쪽을 택한다. 해당 동작은 tests/plugin_version_bump_channel.rs에서 검사한다.",
)];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 공백을 뺀 줄이 resolve_judge 호출로 시작하는 경우만 수집한다. 주석·echo 내부 예시는 제외된다.
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

/// 없는 도구를 빌드하려는 소비자가 바깥 Cargo 잠금을 기다리지 않도록 성공하는 Cargo 스텁을 주입한다.
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
    assert!(
        !all.is_empty(),
        "scripts에서 resolve_judge 호출을 찾지 못했다. 경로와 호출 형식을 확인한다."
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
        "판정기가 없는데 측정 실패(2)로 종료하지 않았다. 원문으로 대신 판정해야 하는 소비자는 EXCEPTIONS에 구체적인 사유를 등록한다:\n{}",
        bad.join("\n---\n")
    );
}

#[test]
fn the_exception_list_names_only_real_consumers() {
    let all = consumers();
    for (name, _) in EXCEPTIONS {
        assert!(
            all.iter().any(|(script, _)| script == name),
            "예외 {name}이 더 이상 resolve_judge를 호출하지 않는다. 오래된 항목을 제거한다."
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

/// 종료 코드만으로는 복구 방법을 알 수 없어 해당 판정기를 빌드하는 명령도 요구한다.
#[test]
fn every_judge_consumer_says_what_to_build() {
    let stub = stub_cargo();
    let all = consumers();
    assert!(
        !all.is_empty(),
        "scripts에서 resolve_judge 호출을 찾지 못했다. 경로와 호출 형식을 확인한다."
    );
    let mut bad = Vec::new();
    for (script, env_var) in &all {
        if EXCEPTIONS.iter().any(|(name, _)| name == script) {
            continue;
        }
        let judge = judge_name(script);
        let want = format!("cargo build -p tasty-doc-guards --bin {judge}");
        let (_, text) = run_blind(script, env_var, stub.path());
        if !text.contains(&want) {
            bad.push(format!("{script} (판정기 {judge})\n{text}"));
        }
    }
    assert!(
        bad.is_empty(),
        "판정기가 없을 때 해당 도구의 빌드 명령을 안내해야 한다. cargo build -p tasty-doc-guards --bin <이름>을 실패 진단에 포함한다:\n{}",
        bad.join("\n---\n")
    );
}

fn judge_name(script: &str) -> String {
    let text = fs::read_to_string(root().join("scripts").join(script))
        .unwrap_or_else(|e| panic!("{script} 를 못 읽는다: {e}"));
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("resolve_judge ") {
            if let Some(name) = rest.split_whitespace().next() {
                return name.to_string();
            }
        }
    }
    panic!("{script} 에서 resolve_judge 의 판정기 이름을 못 읽었다 — 호출 형태가 바뀌었다");
}
