//! 셸 파이프의 오른쪽에 입력을 다 읽기 전에 끝나는 명령이 있는지 확인한다.
//! 앞 명령이 SIGPIPE로 종료되면 pipefail 아래에서 파이프라인이 실패할 수 있다.
//! set -e로 스크립트가 중단되거나 if/|| 조건의 결과가 달라질 수 있다.
//!
//! head, grep 계열의 -q/-m, q를 포함한 sed, exit를 포함한 awk를 검사한다.
//! sed/awk와 옵션 검사는 문자열에 의존하므로 실제 실행을 완전히 해석하지는 않는다.
//! 출력을 변수로 모두 받은 뒤 히어스트링으로 전달하거나 파일을 직접 읽도록 고친다.
//!
//! 터미널에서 직접 입력한 명령은 검사하지 않는다. 조기 종료 검사는 pipefail 유무와
//! 무관하게 실행하며, 조건문 파이프의 pipefail 표기는 아래 별도 검사에서 확인한다.
//! 자동 실행 경로는 docs/dev-guide/ci-gates.md에 있다.

// 테스트의 값 무시를 제품 코드의 lint 목록에서 제외한다.
#![allow(clippy::let_underscore_must_use)]
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

const SKIP_DIRS: &[&str] = &["target", ".git", "_site", "node_modules"];

/// 셸 스크립트 수집 실패를 찾는 하한이다. 전체 수집의 완전성을 보장하지는 않는다.
/// 2026-09-07 측정 23개에서 여러 도구 스크립트를 함께 정리할 여유 8개를 뒀다.
const SHELL_SCRIPT_SCAN_FLOOR: usize = 15;

/// 2026-09-07 워크플로 11개에서 3개의 감소를 허용한 수집 하한이다.
const WORKFLOW_SCAN_FLOOR: usize = 8;

/// Justfile과 워크플로를 합친 수집 하한. 2026-09-06 측정 12개에서 여유 3개를 뒀다.
/// 워크플로를 디스크에서 다시 센 값과 비교하는 검사도 있으나, 두 결과가 함께 비면 통과하므로
/// 그 비교만으로 이 하한을 대신할 수 없다.
const SHELL_CARRIER_SCAN_FLOOR: usize = 9;

fn scan_is_credible(found: usize) -> bool {
    found >= SHELL_SCRIPT_SCAN_FLOOR
}

/// 따옴표 밖에서 시작하는 셸 주석을 제거한다.
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

/// 줄 끝의 역슬래시나 파이프는 다음 줄과 이어진 명령으로 읽는다.
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
        "head" => true,
        "grep" | "egrep" | "fgrep" | "rg" => rest.iter().any(|w| {
            w.starts_with('-') && !w.starts_with("--") && (w.contains('q') || w.contains('m'))
        }),
        // 문자열 포함 여부를 쓰는 근사 판정이다.
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
        // 첫 명령은 입력을 받는 쪽이 아니므로 제외한다.
        if segs[1..].iter().any(|s| is_early_exit_consumer(s)) {
            found.push((line, cmd));
        }
    }
    found
}

/// 파일 전체 크기와 무관하게 shebang을 검사하도록 앞부분만 읽는다.
const SHEBANG_WINDOW: usize = 256;

/// 읽은 앞부분의 첫 줄만 UTF-8로 해석한다. 파일 뒤쪽의 인코딩은 확인하지 않는다.
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
            // 이름이 다른 빌드 디렉터리도 캐시 표식으로 제외한다.
            if SKIP_DIRS.contains(&name.as_ref())
                || (name.starts_with('.') && name != ".githooks")
                || tasty_doc_guards::is_build_cache_dir(&path)
            {
                continue;
            }
            collect_shell_scripts(&path, out);
        } else {
            // 확장자가 없는 훅도 포함하려고 shebang으로 고른다.
            if has_shell_shebang(&path) {
                out.push(path);
            }
        }
    }
}

// Justfile 레시피와 워크플로 run은 파일 첫 줄의 shebang 검사로 찾을 수 없어 따로 수집한다.

/// run의 한 줄 값과 들여쓴 블록을 추출한다. YAML 전체 문법을 해석하지는 않는다.
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
        // 대시가 아닌 run 키의 들여쓰기를 기준으로 삼아 다음 env 키를 본문에 넣지 않는다.
        let key_indent = line.find("run:").unwrap_or(line.len() - trimmed.len());
        if rest.starts_with('|') || rest.starts_with('>') {
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

fn shell_carriers(root: &Path) -> Vec<(String, Vec<(usize, String)>)> {
    let mut out: Vec<(String, Vec<(usize, String)>)> = Vec::new();

    // Justfile 전체를 셸로 읽으므로 레시피 밖의 문장도 검사될 수 있다.
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
        wf_files.len() >= WORKFLOW_SCAN_FLOOR,
        "워크플로를 {}개만 찾았다(하한 {WORKFLOW_SCAN_FLOOR}). .github/workflows의 yml/yaml 파일 수와 확장자 필터를 확인한다. 실제 파일이 줄었다면 측정 근거와 함께 하한을 갱신한다.",
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
        "셸 스크립트를 {}개만 찾았다(하한 {SHELL_SCRIPT_SCAN_FLOOR}). 경로와 shebang 판정을 확인한다. 확장자 기준 목록과는 source 전용 파일·확장자 없는 훅 등의 차이가 있으므로 파일별로 대조한다.",
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
        "파이프 오른쪽 명령이 입력을 다 읽기 전에 끝나면 앞 명령이 SIGPIPE로 종료될 수 있다. pipefail이 켜져 있으면 원하는 결과를 찾고도 파이프라인은 실패한다. 출력을 변수로 모두 받은 뒤 히어스트링으로 넘기거나 파일을 직접 읽도록 고친다:\n  {}",
        hits.join("\n  ")
    );
}

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
    assert!(violations("grep -m1 PAT FILE | sed 's/a/b/'").is_empty());
    assert!(violations("head -1 FILE | tr -d ' '").is_empty());
    assert!(violations("grep -q PAT FILE").is_empty());
    assert!(violations("head -40 <<<\"$body\"").is_empty());
}

#[test]
fn a_logical_or_is_not_a_pipe() {
    assert!(violations("cmd || head -1 FILE").is_empty());
    assert!(violations("grep -q PAT FILE || echo missing").is_empty());
    assert_eq!(violations("producer | grep -q PAT || exit 1").len(), 1);
}

#[test]
fn a_pipe_split_across_lines_is_still_one_command() {
    assert_eq!(violations("producer \\\n  | head -5").len(), 1);
    assert_eq!(violations("producer |\n  grep -q PAT").len(), 1);
    assert_eq!(
        violations("if [[ -n \"$x\" ]] &&\n  producer |\n  grep -q PAT; then").len(),
        1
    );
}

#[test]
fn a_comment_is_not_code() {
    assert!(violations("# `tar -tzf ... | grep -q ...` 는 레이스가 난다").is_empty());
    assert!(violations("producer | tail -1  # 예전엔 | head -1 이었다").is_empty());
    assert_eq!(violations("producer | head -1  # 고쳐야 한다").len(), 1);
    assert_eq!(violations("echo \"${x#pre}\" | grep -q PAT").len(), 1);
}

#[test]
fn a_pipe_inside_quotes_is_not_a_pipe() {
    assert!(violations("echo \"a | head -1\"").is_empty());
    assert!(violations("grep -E 'a|b' FILE | tail -1").is_empty());
    assert!(violations("awk -F'|' '{print $1}' FILE | sort").is_empty());
}

#[test]
fn the_scan_refuses_to_report_zero_from_an_empty_input() {
    assert!(violations("").is_empty());
    assert!(logical_lines("").is_empty());
    assert!(!scan_is_credible(0));
    assert!(!scan_is_credible(SHELL_SCRIPT_SCAN_FLOOR - 1));
    assert!(scan_is_credible(SHELL_SCRIPT_SCAN_FLOOR));
}

#[test]
fn no_shell_carrier_pipes_into_an_early_exit_consumer() {
    let root = repo_root();
    let carriers = shell_carriers(&root);
    assert!(
        carriers.len() >= SHELL_CARRIER_SCAN_FLOOR,
        "셸을 담은 파일을 {}개만 찾았다(하한 {SHELL_CARRIER_SCAN_FLOOR}). Justfile과 모든 yml/yaml 워크플로가 포함돼야 한다. Justfile 읽기 실패와 워크플로 수집을 확인한다. 디스크 수와의 비교도 함께 비면 통과하므로 이 하한을 대신하지 않는다.",
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
        "Justfile 또는 워크플로의 파이프 오른쪽에 조기 종료 명령이 있다. 파일을 직접 읽거나 출력을 변수로 모두 받은 뒤 히어스트링으로 넘긴다:\n  {}",
        hits.join("\n  ")
    );
}

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
    assert!(blocks[0].1.starts_with("set -e\n"), "{:?}", blocks[0].1);
    assert_eq!(blocks[1].1, "echo ok");
    let hit = &violations(&blocks[0].1)[0];
    assert_eq!(blocks[0].0 + hit.0 - 1, 7, "줄 번호가 어긋난다");
}

#[test]
fn a_workflow_key_that_merely_ends_in_run_is_not_a_run_block() {
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

#[test]
fn a_build_dir_is_recognised_by_its_tag_not_its_name() {
    let dir = std::env::temp_dir().join(format!("tasty-shellprune-{}", std::process::id()));
    // 임시 파일 정리 실패가 실제 검사 결과를 가리지 않게 한다.
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

    // 임시 파일 정리 실패가 실제 검사 결과를 가리지 않게 한다.
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_carrier_set_covers_the_justfile_and_every_workflow() {
    let root = repo_root();
    let carriers = shell_carriers(&root);
    let names: Vec<&str> = carriers.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"Justfile"), "Justfile 이 빠졌다: {names:?}");
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

// pipefail을 쓰지 않으면 앞 명령이 실패해도 파이프라인은 마지막 명령의 종료코드를 낸다.
// 아래 검사는 shebang으로 수집한 스크립트에서 if/elif/while/until로 시작하는 파이프를 찾는다.
// Justfile과 워크플로 run 블록은 이 검사에 포함하지 않는다.
// 대입 후 $? 확인, &&/|| 뒤의 종료코드 판정은 놓친다. PIPESTATUS 직접 확인도 대상이 아니다.
// 조건 안의 명령치환으로 값만 구하는 파이프는 종료코드를 쓰지 않아도 검출될 수 있다.

/// 주석을 제외한 논리 명령에 pipefail이라는 문자열이 있는지만 확인한다.
/// 실제로 옵션을 켰는지, 뒤에서 껐는지, 어느 분기에서 적용되는지는 알지 못한다.
fn declares_pipefail(text: &str) -> bool {
    logical_lines(text)
        .iter()
        .any(|(_, cmd)| cmd.contains("pipefail"))
}

/// if/elif/while/until로 시작하고 파이프가 포함된 논리 줄을 찾는다.
fn deciding_pipelines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (line, cmd) in logical_lines(text) {
        let head = cmd.trim_start();
        let decides = ["if ", "elif ", "while ", "until "]
            .iter()
            .any(|kw| head.starts_with(kw));
        if decides && pipe_segments(&cmd).len() > 1 {
            out.push((line, cmd));
        }
    }
    out
}

#[test]
fn a_pipeline_whose_status_decides_lives_in_a_script_that_declares_pipefail() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_shell_scripts(&root, &mut files);
    assert!(
        scan_is_credible(files.len()),
        "셸 스크립트를 {}개만 찾았다(하한 {SHELL_SCRIPT_SCAN_FLOOR}). 경로와 shebang 수집을 확인한다.",
        files.len()
    );

    let mut hits = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if declares_pipefail(&text) {
            continue;
        }
        let rel = file.strip_prefix(&root).unwrap_or(file);
        let rel = rel.to_string_lossy().replace('\\', "/");
        for (line, cmd) in deciding_pipelines(&text) {
            hits.push(format!("{rel}:{line}  {}", cmd.trim()));
        }
    }

    assert!(
        hits.is_empty(),
        "조건문의 파이프라인이 있는 스크립트에서 pipefail을 찾지 못했다. 앞 명령의 실패가 마지막 명령의 성공으로 가려질 수 있다. set -o pipefail을 적용하거나 명령 출력을 파일로 받고 종료코드를 직접 확인한다. 조건 안에서 값만 구하는 명령치환인지도 검토한다:\n  {}",
        hits.join("\n  ")
    );
}

#[test]
fn a_comment_explaining_pipefail_is_not_a_declaration() {
    assert!(!declares_pipefail(
        "#!/bin/bash\n# pipefail 이 없으면 샌다\nset -e\n"
    ));
    assert!(declares_pipefail("#!/bin/bash\nset -e -o pipefail\n"));
    assert!(declares_pipefail("#!/bin/bash\nset -euo pipefail\n"));
}

#[test]
fn a_pipeline_outside_a_condition_is_not_a_decision() {
    assert!(deciding_pipelines("files=$(git ls-files | wc -l)\n").is_empty());
    assert_eq!(
        deciding_pipelines("if ! cargo check 2>&1 | tail -5; then\n").len(),
        1
    );
    assert_eq!(deciding_pipelines("while read f | grep x; do\n").len(), 1);
}

#[test]
fn a_condition_without_a_pipe_is_not_flagged() {
    assert!(deciding_pipelines("if ! cargo check; then\n").is_empty());
    assert!(deciding_pipelines("if cargo check || true; then\n").is_empty());
}
