//! 셸 스크립트에서 **조기에 끝나는 소비자**를 파이프의 오른쪽에 두는 것을 막는다.
//!
//! `grep -q` 는 첫 매치에서, `head -N` 은 N 줄에서 파이프를 닫는다. 그때 아직 쓰고 있던
//! producer 는 SIGPIPE 로 죽어 141 을 남기고, `set -o pipefail` 이 그것을 파이프라인의
//! 종료 상태로 올린다 — **소비자가 원하는 것을 찾았는데도 실패로 판정된다.**
//!
//! 증상은 그 자리의 문맥에 따라 갈린다. `set -e` 아래의 대입문이나 단독 파이프라인이면
//! 스크립트가 그 자리에서 죽고, `if` 나 `||` 자리면 `set -e` 가 면제되는 대신 **조건이
//! 조용히 뒤집힌다.** 뒤쪽이 더 나쁘다 — 서명 인증서를 손에 쥐고 "없다" 고 하거나, 설치된
//! 타깃을 "안 깔렸다" 고 한다.
//!
//! **실현 여부는 잔여 출력량의 함수이고, 큰 출력에서는 비결정이 아니라 결정적이다.**
//! 이 레포에서 실측: 8.6MB producer 의 첫 줄에 매치를 두면 파이프 20/20 이 141 을 내고
//! 히어스트링은 20/20 이 0 을 낸다. 작은 출력에서는 값이 흔들려 언젠가 들키지만, 큰
//! 출력에서는 **매번 같은 실패**라 flake 로도 안 보인다.
//!
//! # 판정 대상 — "파이프" 가 아니라 "조기 종료 소비자"
//!
//! 금지 목록을 파이프 전체로 넓히면 잡음이 커서 지켜지지 않는다. 축은 **소비자가 입력을
//! 다 읽기 전에 끝날 수 있는가** 하나다.
//!
//! ```text
//! 대상   head · grep -q · grep -m · sed -n '…q' · awk '…exit'
//! 축 밖  tail · wc · sort · cat · grep -c · grep -o · exit 없는 awk
//! ```
//!
//! `tail` 은 마지막 N 줄을 알려면 정의상 전량을 읽어야 하므로 producer 를 못 죽인다.
//!
//! # 고치는 법 (둘 다 비용 0)
//!
//! - producer 를 변수로 완전히 받은 뒤 히어스트링: `grep -q PAT <<<"$out"`
//! - 애초에 파이프를 만들지 않는다: `grep -m1 PAT FILE` — 파일을 직접 읽으므로 producer 가
//!   없다. 파이프의 **왼쪽**에 오는 `grep -m1` 은 그래서 대상이 아니다.
//!
//! # 이 가드가 판정하지 않는 것
//!
//! - **셸 스크립트 밖의 파이프.** 사람이 터미널에서 `bash mut.sh | head -8` 을 치면 그
//!   파이프는 어느 파일에도 없다. 실제로 그 형태로 사고가 났고(변이 스크립트가 되돌리기
//!   전에 SIGPIPE 로 죽어 소스에 뮤턴트가 남았다) 스크립트를 아무리 훑어도 0 건이었다.
//!   **자동 채널 없음** — 이 축은 절차로만 지킨다.
//! - **`pipefail` 이 켜져 있는가.** 실현 조건이지만 판정에 넣지 않는다. 켜는 것은 언제든
//!   일어나는 한 줄짜리 변경이고, 그때 위반이 조용히 되살아난다. 게다가 producer 가
//!   중간 상태를 만들고 끝에서 되돌리는 형태면 **`pipefail` 과 무관하게** producer 의
//!   죽음 자체가 피해다.
//! - **`sed`/`awk` 의 조기 종료 판정은 근사다.** `q` · `exit` 라는 낱말로 본다.
//!
//! 채널: 이 가드는 통합 테스트다 — 채널 정본은 `docs/dev-guide/ci-gates.md`.

// 이유: 이 타깃은 전부 테스트다. 테스트의 `let _ =` 는 정책이 사유를 요구하지
// 않으므로 `clippy::let_underscore_must_use` 명부(프로덕션 전용)에 섞이면 안 된다
// — docs/dev-guide/error-handling.md.
#![allow(clippy::let_underscore_must_use)]
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

const SKIP_DIRS: &[&str] = &["target", ".git", "_site", "node_modules"];

/// 스캔 하한 — 수집이 조용히 줄어들면 위반 0 이 "다 깨끗하다" 로 읽힌다.
///
/// 고정 개수의 두 용도 중 **연기 검사** 쪽이다: 모수를 이 수로 고정하는 것이 아니라,
/// 경로가 어긋나 0 을 내는 것을 막는 하한이다.
const MIN_SHELL_SCRIPTS: usize = 15;

/// 워크플로 수집 하한 — 같은 이유로, 경로가 어긋나 0 을 내는 것을 막는다.
///
/// **이 값은 워크플로 수의 하한이다.** [`shell_carriers`] 가 내는 집합의 하한이 아니다 —
/// 그쪽은 Justfile 을 더한 것이라 모수가 다르고, 자기 이름을 갖는다([`MIN_SHELL_CARRIERS`]).
const MIN_WORKFLOWS: usize = 8;

/// 셸을 담는 자리의 하한 — [`shell_carriers`] 가 내는 집합의 크기.
///
/// **모수가 [`MIN_WORKFLOWS`] 와 다르다.** 여기 담기는 것은 Justfile 하나 + 워크플로
/// 전부다. 한때 이 자리가 `MIN_WORKFLOWS` 를 빌려 썼는데, 그러면 재는 수와 이름이
/// 가리키는 수가 달라 값을 고칠 때 어느 모수를 고쳤는지 못 읽는다.
///
/// 값의 근거: 2026-09-06 실측 **12**(Justfile 1 + `.github/workflows` 의 yml 11).
/// 여유를 셋 둔다 — 워크플로는 하나씩 늘고 줄지 한꺼번에 움직이지 않으므로, 넓은 여유는
/// 수집이 절반 죽어도 통과시킨다.
///
/// ★ **이 하한이 이 파일에서 가장 약한 검사라는 것을 함께 적는다.** 같은 물음의 더 강한
/// 형태가 [`the_carrier_set_covers_the_justfile_and_every_workflow`] 에 있다 — 거기서는
/// Justfile 을 **이름으로** 확인하고 워크플로 수를 디스크와 **같은지**로 확인한다. 이름과
/// 등호가 부등호보다 강하다. 그러니 이 값의 몫은 하나뿐이다: 아래 루프가 얇은 인구 위에서
/// 돌기 전에 그 자리에서 멈추는 것.
const MIN_SHELL_CARRIERS: usize = 9;

/// 수집 결과가 **믿을 만한가** — 판정을 순수 함수로 뽑아 합성 입력으로 찌를 수 있게 한다.
///
/// 하한 검사를 본 테스트 안에 인라인으로 두면 그 검사 자체는 아무 변이로도 고정되지
/// 않는다. 특히 0 을 넣었을 때 거짓이 되는지가 이 가드의 핵심인데, 그것을 확인하는 유일한
/// 길이 실제로 수집을 깨뜨려 보는 것이 되어 버린다.
fn scan_is_credible(found: usize) -> bool {
    found >= MIN_SHELL_SCRIPTS
}

/// 따옴표 밖의 `#` 부터 줄 끝까지 잘라낸다.
///
/// 주석을 코드로 읽으면 **기전을 설명해 둔 주석 자체가 위반으로 잡힌다** — 이 레포에는
/// 그 형태가 실재했다(SIGPIPE 레이스를 설명하는 주석이 `| grep -q` 를 인용한다).
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let (mut single, mut double) = (false, false);
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if !single => i += 1,
            b'\'' if !double => single = !single,
            b'"' if !single => double = !double,
            // 낱말 중간의 `#` 은 주석이 아니다(`${x#y}` · `a#b`).
            b'#' if !single && !double && (i == 0 || bytes[i - 1].is_ascii_whitespace()) => {
                return &line[..i];
            }
            _ => {}
        }
        i += 1;
    }
    line
}

/// 물리 줄을 **논리 명령**으로 합친다 — 줄 끝의 `\` 와 **줄 끝의 `|`** 를 모두 잇는다.
///
/// 줄 단위로만 보면 두 형태를 원리적으로 못 본다: 역슬래시로 이어 붙인 파이프와, 파이프
/// 문자로 끝나는 줄(셸에서 역슬래시 없이도 합법이다). 실제로 이 레포의 위반 하나가 뒤쪽
/// 형태였고 줄 단위 스캔은 그것을 끝까지 못 봤다.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut start = 0usize;
    for (idx, raw) in text.lines().enumerate() {
        let code = strip_comment(raw).trim_end();
        if code.trim().is_empty() && buf.is_empty() {
            continue;
        }
        if buf.is_empty() {
            start = idx + 1;
        } else {
            buf.push(' ');
        }
        let continues = code.ends_with('\\') || code.ends_with('|');
        buf.push_str(code.trim_end_matches('\\'));
        if !continues {
            out.push((start, std::mem::take(&mut buf)));
        }
    }
    if !buf.is_empty() {
        out.push((start, buf));
    }
    out
}

/// 논리 명령을 **따옴표 밖의 단일 `|`** 로 자른다. `||` 는 파이프가 아니다.
fn pipe_segments(cmd: &str) -> Vec<&str> {
    let bytes = cmd.as_bytes();
    let (mut single, mut double) = (false, false);
    let mut segs = Vec::new();
    let mut from = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if !single => i += 1,
            b'\'' if !double => single = !single,
            b'"' if !single => double = !double,
            b'|' if !single && !double => {
                if bytes.get(i + 1) == Some(&b'|') {
                    i += 1; // `||` — 파이프가 아니다
                } else {
                    segs.push(&cmd[from..i]);
                    from = i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    segs.push(&cmd[from..]);
    segs
}

/// 이 세그먼트가 **입력을 다 읽기 전에 끝날 수 있는 소비자**인가.
fn is_early_exit_consumer(segment: &str) -> bool {
    let words: Vec<&str> = segment.split_whitespace().collect();
    // 앞의 환경변수 대입(`LC_ALL=C grep …`)을 건너뛰고 명령 이름을 찾는다.
    let Some(pos) = words
        .iter()
        .position(|w| !w.contains('=') || w.starts_with('-'))
    else {
        return false;
    };
    let name = words[pos].rsplit('/').next().unwrap_or(words[pos]);
    let rest = &words[pos + 1..];
    match name {
        // `head` 는 정의상 앞에서 끊는다 — 플래그와 무관하다.
        "head" => true,
        // `-q`(조용히 첫 매치에서 종료) · `-m N`(N 매치에서 종료). `-c`/`-o` 는 전량 읽는다.
        "grep" | "egrep" | "fgrep" | "rg" => rest.iter().any(|w| {
            w.starts_with('-') && !w.starts_with("--") && (w.contains('q') || w.contains('m'))
        }),
        // 근사: 스크립트 안에 `q`(sed) · `exit`(awk) 가 낱말로 보이면 조기 종료로 본다.
        "sed" => rest.iter().any(|w| w.contains('q')),
        "awk" | "gawk" | "mawk" => rest.iter().any(|w| w.contains("exit")),
        _ => false,
    }
}

/// 한 파일의 위반 자리 — (줄 번호, 논리 명령).
fn violations(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (line, cmd) in logical_lines(text) {
        let segs = pipe_segments(&cmd);
        if segs.len() < 2 {
            continue;
        }
        // 첫 세그먼트는 producer 다 — 파이프의 **왼쪽**에 온 `grep -m1` 은 대상이 아니다.
        if segs[1..].iter().any(|s| is_early_exit_consumer(s)) {
            found.push((line, cmd));
        }
    }
    found
}

/// shebang 을 읽기 위해 여는 **앞부분 바이트 수**. 한 줄을 판정하는 데 필요한 만큼만
/// 읽는다.
///
/// 종전에는 `read_to_string` 으로 파일을 통째로 읽었다. 이 수집기는 확장자를 안 보고
/// **모든 파일**을 열어 보므로, 그 비용이 트리의 총 바이트에 비례했다 — 실측(2026-09-05)
/// 다른 이름의 빌드 디렉토리가 모수에 들어왔을 때 이 가드가 0.05s → 89.17s (1783 배) 가
/// 됐고, 27 타깃 중 가장 크게 움직였다. 가지치기(ADR-0146)가 그 경로를 막았지만 크기에
/// 비례하는 성질 자체는 남아 있었다 — 산출물이 아닌 큰 파일이 늘면 다시 비싸진다.
///
/// 셸 첫 줄은 실질적으로 이보다 훨씬 짧다. 이 창을 넘는 첫 줄은 shebang 이 아니다.
const SHEBANG_WINDOW: usize = 256;

/// 첫 줄이 셸 shebang 인가.
///
/// UTF-8 로 못 읽는 파일은 셸 스크립트가 아니다. 앞부분만 보므로 **뒤쪽이 UTF-8 이
/// 아닌 파일도 첫 줄로 판정된다** — 종전의 통째 읽기는 그런 파일을 통째로 건너뛰었다.
/// 방향이 옳은 쪽으로만 달라진다: shebang 을 가진 파일이 뒤에 이진 바이트를 담았다고
/// 모수에서 빠질 이유가 없다.
fn has_shell_shebang(path: &Path) -> bool {
    use std::io::Read;

    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; SHEBANG_WINDOW];
    let Ok(n) = file.read(&mut head) else {
        return false;
    };
    let head = &head[..n];
    if !head.starts_with(b"#!") {
        return false;
    }
    let line_end = head.iter().position(|b| *b == b'\n').unwrap_or(head.len());
    let Ok(first) = std::str::from_utf8(&head[..line_end]) else {
        return false;
    };
    first.contains("bash") || first.contains("sh")
}

fn collect_shell_scripts(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        panic!("디렉토리를 읽지 못했다: {}", dir.display());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            // 이름으로 걸리거나, **디렉토리 자신이 빌드 캐시라고 밝히거나**. 이름만 볼 때는
            // `CARGO_TARGET_DIR` 로 만든 다른 이름의 빌드 디렉토리가 통째로 모수에 들어왔고,
            // 이 가드는 아래 `read_to_string` 을 **모든 파일에** 걸기 때문에 그 대가가 가장
            // 컸다 — 실측(2026-09-05) 0.05s → 89.17s (1783 배).
            if SKIP_DIRS.contains(&name.as_ref())
                || (name.starts_with('.') && name != ".githooks")
                || tasty_doc_guards::is_build_cache_dir(&path)
            {
                continue;
            }
            collect_shell_scripts(&path, out);
        } else {
            // 확장자가 아니라 **shebang** 으로 고른다 — `.githooks/pre-commit` 처럼 확장자가
            // 없는 셸 스크립트가 있고, `*.sh` 로 훑으면 그것이 통째로 모수 밖에 남는다.
            if has_shell_shebang(&path) {
                out.push(path);
            }
        }
    }
}

// ─── 셸을 담지만 셸 스크립트 파일이 아닌 자리 ────────────────────────────────
//
// 위 수집기는 **파일 첫 줄의 shebang** 으로 고른다. 그 규칙이 통째로 못 보는 자리가 둘
// 있고, 둘 다 이 레포에 실재했다:
//
//   Justfile              첫 줄이 주석이고 `#!/bin/bash` 는 레시피 **안쪽** 11 곳에 있다.
//                         그 레시피들은 `set -euo pipefail` 을 켠다 — 실현 조건이 성립한다.
//   .github/workflows/    `run:` 블록이 셸인데, `.github` 이 dot 디렉토리라 순회에서 빠진다.
//
// 모수를 확장자로도, 첫 줄로도 고르면 안 된다는 뜻이다. **셸을 담는 자리**를 이름으로
// 지목하고, 새 자리가 생겼을 때 조용히 빠지지 않도록 개수를 함께 고정한다.

/// 워크플로 YAML 의 `run:` 블록 본문 — (파일 안 시작 줄, 본문).
///
/// 블록 스칼라(`run: |`)와 한 줄 형태(`run: cmd`)를 모두 본다. YAML 파서를 붙이지 않는
/// 근사다 — 판정이 보수적인 쪽(더 많이 보는 쪽)으로만 틀리도록 들여쓰기로 끊는다.
fn workflow_run_blocks(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        let body_after_key = trimmed
            .strip_prefix("- ")
            .unwrap_or(trimmed)
            .strip_prefix("run:")
            .map(str::trim);
        let Some(rest) = body_after_key else {
            i += 1;
            continue;
        };
        // 키의 들여쓰기는 대시가 아니라 **`run` 낱말의 위치**다. `- run: |` 에서 대시를
        // 기준으로 재면 같은 step 의 `env:`(더 깊이 들여쓴다)까지 본문으로 삼킨다.
        let key_indent = line.find("run:").unwrap_or(line.len() - trimmed.len());
        if rest.starts_with('|') || rest.starts_with('>') {
            // 블록 스칼라 — 키보다 깊이 들여쓴 줄이 본문이다.
            let mut body = Vec::new();
            let mut base: Option<usize> = None;
            let mut j = i + 1;
            while j < lines.len() {
                let l = lines[j];
                if l.trim().is_empty() {
                    body.push(String::new());
                    j += 1;
                    continue;
                }
                let ind = l.len() - l.trim_start().len();
                if ind <= key_indent {
                    break;
                }
                let base = *base.get_or_insert(ind);
                body.push(l.chars().skip(base).collect());
                j += 1;
            }
            out.push((i + 2, body.join("\n")));
            i = j;
        } else {
            if !rest.is_empty() {
                out.push((i + 1, rest.to_string()));
            }
            i += 1;
        }
    }
    out
}

/// 이 레포에서 셸을 담는 **비-스크립트 파일**. 새 자리가 생기면 여기 더한다.
fn shell_carriers(root: &Path) -> Vec<(String, Vec<(usize, String)>)> {
    let mut out: Vec<(String, Vec<(usize, String)>)> = Vec::new();

    // Justfile 은 통째로 셸로 읽는다 — 과대근사이지만 **더 많이 보는 쪽**이라 안전하다.
    // 레시피 헤더(`name:`)나 `:=` 대입에는 파이프가 없어서 실측 오탐이 0 이다.
    if let Ok(text) = std::fs::read_to_string(root.join("Justfile")) {
        out.push(("Justfile".to_string(), violations(&text)));
    }

    let wf_dir = root.join(".github").join("workflows");
    let mut wf_files: Vec<PathBuf> = std::fs::read_dir(&wf_dir)
        .unwrap_or_else(|e| panic!("워크플로 디렉토리를 못 읽었다: {} ({e})", wf_dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yml" || x == "yaml"))
        .collect();
    wf_files.sort();
    assert!(
        wf_files.len() >= MIN_WORKFLOWS,
        "워크플로를 {}개밖에 못 찾았다(하한 {MIN_WORKFLOWS}) — 수집이 깨졌다.\n\
         ★ 판별 — 이 자리는 갈래가 하나뿐이다. 바로 위에서 `read_dir` 실패는 이미 panic 으로 걸러 \
         냈으니, 여기까지 와서 수가 모자란 것은 **디렉토리는 열렸는데 그 안에 정말 적은 것**이다. \
         밖에서 세는 값이 같아야 한다:\n\
             ls .github/workflows | grep -cE '[.]ya?ml$'\n\
         2026-09-06 실측 11. 두 수가 다르면 확장자 필터가 어긋난 것이다.\n\
         ★ 이 하한을 내려서 통과시키지 마라 — 워크플로가 진짜로 지워졌다면 그것부터 사고다.",
        wf_files.len()
    );
    for path in wf_files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = format!(
            ".github/workflows/{}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        let mut hits = Vec::new();
        for (start, body) in workflow_run_blocks(&text) {
            for (line, cmd) in violations(&body) {
                hits.push((start + line - 1, cmd));
            }
        }
        out.push((rel, hits));
    }
    out
}

#[test]
fn no_shell_script_pipes_into_an_early_exit_consumer() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_shell_scripts(&root, &mut files);
    assert!(
        scan_is_credible(files.len()),
        "셸 스크립트를 {}개밖에 못 찾았다(하한 {MIN_SHELL_SCRIPTS}) — 수집이 깨졌다. \
         위반 0 이 '깨끗하다' 가 아니라 '아무것도 안 봤다' 일 수 있다.\n\
         ★ 판별 — 이 모수는 **내용(shebang)으로** 고른 집합이고, 확장자로 고른 집합이 그 짝이다:\n\
             git ls-files '*.sh' '*.bash' | wc -l\n\
         2026-09-06 실측 20. 두 수는 원래 안 맞는다 — source 전용 라이브러리는 shebang 을 안 갖고 \
         (실측 1 건), shebang 은 있으나 셸이 아닌 것도 있다(실측 1 건). 그래서 **차이 자체가 아니라 \
         차이가 커지는가**를 봐라. 확장자 쪽이 그대로인데 여기만 무너졌으면 shebang 판정이 깨진 \
         것이고, 둘이 함께 줄었으면 스크립트가 정말 줄어든 것이다.\n\
         ★ 이 하한을 내려서 통과시키지 마라 — 이 가드가 막는 것은 조용한 파이프 위반이고, 모수가 \
         줄면 줄어든 만큼이 정확히 안 보이게 된다.",
        files.len()
    );

    let mut hits = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap_or(file);
        let rel = rel.to_string_lossy().replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (line, cmd) in violations(&text) {
            hits.push(format!("{rel}:{line}  {}", cmd.trim()));
        }
    }

    assert!(
        hits.is_empty(),
        "아래는 조기에 끝나는 소비자를 파이프의 오른쪽에 둔 자리다. 소비자가 파이프를 먼저 \
         닫으면 producer 가 SIGPIPE 로 죽고, `pipefail` 이 켜져 있으면 찾았는데도 실패가 \
         되며 `if`/`||` 자리에서는 조건이 뒤집힌다. producer 를 변수로 받아 히어스트링으로 \
         넘기거나(`grep -q PAT <<<\"$out\"`) 파이프를 안 만들면(`grep -m1 PAT FILE`) 된다 \
         — 둘 다 비용 0 이다:\n  {}",
        hits.join("\n  ")
    );
}

// ─── 판정기를 겨냥한 변이 (합성 입력) ─────────────────────────────────────────
//
// 판정기가 순수 함수라 합성 문자열로 찌른다. **음성 케이스를 함께 넣는다** — 없으면
// "전부 위반으로 부르는 판정기" 로도 통과한다.

#[test]
fn the_forbidden_consumers_are_caught_on_the_right_of_a_pipe() {
    for cmd in [
        "producer | head -5",
        "producer | grep -q PAT",
        "producer | grep -qx PAT",
        "producer | grep -Eq PAT",
        "producer | grep -m1 PAT",
        "producer | sed -n '1p;q'",
        "producer | awk '/x/{print;exit}'",
        "a | b | head -1",
        "producer | LC_ALL=C grep -q PAT",
    ] {
        assert_eq!(violations(cmd).len(), 1, "못 잡았다: {cmd}");
    }
}

#[test]
fn consumers_that_must_read_everything_are_not_flagged() {
    for cmd in [
        "producer | tail -6",
        "producer | wc -l",
        "producer | sort -u",
        "producer | cat",
        "producer | grep -c PAT",
        "producer | grep -oE '[0-9]+'",
        "producer | awk '{print $1}'",
        "producer | awk 'NR==1{print $1}'",
        "producer | sed 's/a/b/'",
    ] {
        assert!(violations(cmd).is_empty(), "오탐: {cmd}");
    }
}

#[test]
fn a_producer_side_early_exit_is_not_a_violation() {
    // 파이프의 **왼쪽**은 조기 종료해도 아무도 안 죽인다 — 오히려 권장 처방이다.
    assert!(violations("grep -m1 PAT FILE | sed 's/a/b/'").is_empty());
    assert!(violations("head -1 FILE | tr -d ' '").is_empty());
    // 파이프가 아예 없으면 대상이 아니다.
    assert!(violations("grep -q PAT FILE").is_empty());
    assert!(violations("head -40 <<<\"$body\"").is_empty());
}

#[test]
fn a_logical_or_is_not_a_pipe() {
    // `||` 를 파이프로 읽으면 오른쪽의 `head`/`grep -q` 가 전부 오탐이 된다.
    assert!(violations("cmd || head -1 FILE").is_empty());
    assert!(violations("grep -q PAT FILE || echo missing").is_empty());
    // 진짜 파이프와 `||` 가 한 줄에 함께 있으면 파이프 쪽만 잡는다.
    assert_eq!(violations("producer | grep -q PAT || exit 1").len(), 1);
}

#[test]
fn a_pipe_split_across_lines_is_still_one_command() {
    // 역슬래시 연속행.
    assert_eq!(violations("producer \\\n  | head -5").len(), 1);
    // 파이프 문자로 끝나는 줄 — 역슬래시 없이도 셸에서 합법이고, 줄 단위 스캔이 원리적으로
    // 못 보는 형태다. 이 레포의 실제 위반 하나가 이 모양이었다.
    assert_eq!(violations("producer |\n  grep -q PAT").len(), 1);
    // 여러 줄에 걸친 조건문 전체.
    assert_eq!(
        violations("if [[ -n \"$x\" ]] &&\n  producer |\n  grep -q PAT; then").len(),
        1
    );
}

#[test]
fn a_comment_is_not_code() {
    // 기전을 설명하는 주석이 needle 을 품는다 — 그것을 세면 "안 고쳤다" 로 읽힌다.
    assert!(violations("# `tar -tzf ... | grep -q ...` 는 레이스가 난다").is_empty());
    assert!(violations("producer | tail -1  # 예전엔 | head -1 이었다").is_empty());
    // 그러나 코드 뒤 주석이 앞의 진짜 위반을 가리지는 않는다.
    assert_eq!(violations("producer | head -1  # 고쳐야 한다").len(), 1);
    // 낱말 중간의 `#` 은 주석이 아니다.
    assert_eq!(violations("echo \"${x#pre}\" | grep -q PAT").len(), 1);
}

#[test]
fn a_pipe_inside_quotes_is_not_a_pipe() {
    assert!(violations("echo \"a | head -1\"").is_empty());
    assert!(violations("grep -E 'a|b' FILE | tail -1").is_empty());
    // 따옴표 안의 `|` 를 파이프로 읽으면 이 줄이 오탐이 된다.
    assert!(violations("awk -F'|' '{print $1}' FILE | sort").is_empty());
}

#[test]
fn the_scan_refuses_to_report_zero_from_an_empty_input() {
    // R31 의 형태 — 입력이 없을 때 판정기가 무엇을 하는지 먼저 본다.
    assert!(violations("").is_empty());
    assert!(logical_lines("").is_empty());
    // 그리고 그 "위반 0" 이 초록으로 새지 않도록 막는 것이 하한이다 — **0 이 안 통과하는
    // 것**이 요점이고, 그것을 여기서 직접 잰다. 하한을 0 으로 낮추면 이 단언이 죽는다.
    assert!(!scan_is_credible(0));
    assert!(!scan_is_credible(MIN_SHELL_SCRIPTS - 1));
    assert!(scan_is_credible(MIN_SHELL_SCRIPTS));
}

#[test]
fn no_shell_carrier_pipes_into_an_early_exit_consumer() {
    let root = repo_root();
    let carriers = shell_carriers(&root);
    assert!(
        carriers.len() >= MIN_SHELL_CARRIERS,
        "셸을 담는 자리를 {}개밖에 못 찾았다(하한 {MIN_SHELL_CARRIERS}) — 수집이 깨졌다.\n\
         ★ 판별 — 이 모수는 Justfile 하나 + 워크플로 전부이고, 그 차가 곧 판별식이다. [`shell_carriers`] 는 워크플로 앞에 Justfile 하나를 \
         담고, 그 자리는 `if let Ok(text)` 라 **읽기에 실패하면 조용히 빠진다.** 그러니:\n\
             carriers.len() - (`.github/workflows` 의 yml/yaml 수) 는 정확히 1 이어야 한다.\n\
         0 이면 Justfile 이 조용히 빠진 것이고(파일이 있는지부터 봐라), 1 인데 전체가 모자라면 \
         워크플로가 준 것이라 [`shell_carriers`] 안쪽 하한이 먼저 말했어야 한다.\n\
         ★ 이 수를 내려서 통과시키지 마라 — 이 단언의 몫은 아래 루프가 얇은 인구 \
         위에서 돌기 전에 멈추는 것 하나뿐이고, 내리면 그 하나가 없어진다.\n\
         그리고 여기서 멈췄다면 `the_carrier_set_covers_the_justfile_and_every_workflow` 가 \
         **먼저** 빨개졌어야 한다 — 그쪽이 Justfile 을 이름으로, 워크플로를 디스크와 등호로 \
         본다. 그 시험이 초록인데 여기가 빨갛다면 둘의 모수가 갈라진 것이니 그것부터 봐라.",
        carriers.len()
    );

    let hits: Vec<String> = carriers
        .iter()
        .flat_map(|(name, found)| {
            found
                .iter()
                .map(move |(line, cmd)| format!("{name}:{line}  {}", cmd.trim()))
        })
        .collect();

    assert!(
        hits.is_empty(),
        "셸 스크립트가 아니지만 셸을 담는 자리에서 조기 종료 소비자가 파이프의 오른쪽에 \
         있다. 처방은 스크립트와 같다 — `grep -m1 PAT FILE` 로 파이프를 없애거나 producer 를 \
         변수로 받아 히어스트링으로 넘긴다:\n  {}",
        hits.join("\n  ")
    );
}

// ─── carrier 추출기를 겨냥한 변이 (합성 입력) ───────────────────────────────

#[test]
fn a_workflow_run_block_is_extracted_as_shell() {
    let yaml = "\
jobs:
  a:
    steps:
      - name: x
        run: |
          set -e
          v=$(producer | head -1)
      - name: y
        run: echo ok
";
    let blocks = workflow_run_blocks(yaml);
    assert_eq!(blocks.len(), 2, "블록 두 개를 못 뽑았다: {blocks:?}");
    // 블록 스칼라 본문은 들여쓰기가 벗겨진 채로 나온다.
    assert!(blocks[0].1.starts_with("set -e\n"), "{:?}", blocks[0].1);
    assert_eq!(blocks[1].1, "echo ok");
    // 그리고 그 본문에서 위반이 잡힌다 — 줄 번호는 파일 기준이다.
    let hit = &violations(&blocks[0].1)[0];
    assert_eq!(blocks[0].0 + hit.0 - 1, 7, "줄 번호가 어긋난다");
}

#[test]
fn a_workflow_key_that_merely_ends_in_run_is_not_a_run_block() {
    // `dry-run:` · `should_run:` 를 `run:` 으로 읽으면 엉뚱한 값이 셸로 들어온다.
    let yaml = "\
jobs:
  a:
    steps:
      - with:
          dry-run: producer | head -1
        run: echo ok
";
    let blocks = workflow_run_blocks(yaml);
    assert_eq!(blocks.len(), 1, "{blocks:?}");
    assert_eq!(blocks[0].1, "echo ok");
}

#[test]
fn a_workflow_block_ends_at_the_next_key() {
    // 본문이 다음 키를 삼키면, 그 키의 값이 셸로 판정된다.
    let yaml = "\
jobs:
  a:
    steps:
      - run: |
          echo one
        env:
          X: producer | head -1
";
    let blocks = workflow_run_blocks(yaml);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].1.trim(), "echo one");
    assert!(violations(&blocks[0].1).is_empty());
}

/// **빌드 디렉토리는 이름이 아니라 표식으로 걸러진다.** 이 가드는 shebang 을 보려고
/// 모든 파일을 읽으므로, 산출물이 모수에 들어오면 대가가 가장 크다 — 실측(2026-09-05)
/// 다른 이름의 빌드 디렉토리를 두었을 때 0.05s 가 89.17s 가 됐다.
///
/// 양극성으로 잡는다. 이 절이 없으면 판정이 이름으로 되돌아가도 나머지 테스트가 전부
/// 초록이라, 모수가 다시 새는 것을 아무도 못 본다.
#[test]
fn a_build_dir_is_recognised_by_its_tag_not_its_name() {
    let dir = std::env::temp_dir().join(format!("tasty-shellprune-{}", std::process::id()));
    // 정리 실패는 무시한다 — 임시 디렉토리라 남아도 판정에 영향이 없고, 여기서
    // 죽으면 진짜 실패가 정리 오류에 가린다.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("임시 디렉토리");

    assert!(
        !tasty_doc_guards::is_build_cache_dir(&dir),
        "표식이 없으면 빌드 캐시가 아니다"
    );

    std::fs::write(
        dir.join("CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )
    .expect("표식 쓰기");
    assert!(
        tasty_doc_guards::is_build_cache_dir(&dir),
        "표식이 있으면 이름과 무관하게 빌드 캐시다"
    );

    // 정리 실패는 무시한다 — 임시 디렉토리라 남아도 판정에 영향이 없고, 여기서
    // 죽으면 진짜 실패가 정리 오류에 가린다.
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_carrier_set_covers_the_justfile_and_every_workflow() {
    let root = repo_root();
    let carriers = shell_carriers(&root);
    let names: Vec<&str> = carriers.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"Justfile"), "Justfile 이 빠졌다: {names:?}");
    // 워크플로는 디렉토리에 있는 만큼 전부 들어와야 한다 — 새 워크플로가 조용히
    // 모수 밖에 남는 것이 이 가드가 늦게 선 이유다.
    let on_disk = std::fs::read_dir(root.join(".github").join("workflows"))
        .expect("워크플로 디렉토리")
        .flatten()
        .filter(|e| {
            let p = e.path();
            p.extension().is_some_and(|x| x == "yml" || x == "yaml")
        })
        .count();
    assert_eq!(
        names.iter().filter(|n| n.starts_with(".github/")).count(),
        on_disk,
        "워크플로 수집이 디스크와 어긋난다: {names:?}"
    );
}
