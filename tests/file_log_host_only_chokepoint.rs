//! 공유 로그 파일 개방을 host 경로 하나로 묶어두는 가드.
//!
//! 배경: `tasty` 바이너리는 GUI(host)와 CLI 클라이언트를 겸한다. 파일 tracing 레이어를
//! 만들면서 로그 파일까지 함께 열면, 역할 판정(`cli_routing::parse_or_route`)이 그
//! **뒤에** 오기 때문에 CLI 서브커맨드 한 번이 실행 중인 host 의 로그를 truncate 한다
//! (실제로 그랬다 — [ADR-0092](../docs/adr/0092-file-log-host-process-only.md)).
//!
//! 회귀는 조용하다: 컴파일도 되고 테스트도 통과하지만, 진단이 필요한 순간에 로그가
//! 비어 있는 것으로만 드러난다. 그래서 "파일을 여는 지점은 host 확정 이후 한 곳뿐" 을
//! 소스 수준에서 고정한다.
//!
//! 선례: `tests/plugin_popup_close_chokepoint.rs`.

use std::path::{Path, PathBuf};

/// 파일을 실제로 여는 함수의 정의 위치. 이 파일에서도 **정의 함수 본문 안** 만 허용한다
/// — 파일 통째로 봐주면 `init()`(= 모든 프로세스가 탄다) 에서 부르는 것을 못 잡는다.
const IMPL_FILE: &str = "src/platform/crash_report.rs";
/// 얇은 boot 래퍼. 여기도 위임 래퍼 함수 본문 안만 허용한다 — 파일 통째로 스킵하면
/// 모든 프로세스가 타는 `init_crash_report()` 안에 호출 한 줄을 넣는 것만으로
/// ADR-0092 이전 버그가 부활하는데 가드가 초록으로 남는다.
const WRAPPER_FILE: &str = "src/boot/os.rs";
/// 유일한 호출처 — host 확정(`Routed::Gui`) 이후. 이 파일은
/// [`the_call_site_sits_inside_the_host_arm`] 가 **분기 블록 내부인지**로 따로 검사한다.
const CALL_SITE_FILE: &str = "src/boot.rs";

const OPEN_FN: &str = "enable_host_file_log";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 주석 줄인가 — 문서에서 이름을 언급하는 것은 호출이 아니므로 스캔에서 뺀다.
fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with('*')
}

/// 줄 끝 `//` 주석을 잘라낸다 — 중괄호 세기가 주석 속 괄호에 흔들리지 않게.
fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// `header` 를 포함하는 첫 코드 줄부터 중괄호가 균형을 이루는 줄까지의 포함 범위.
///
/// 텍스트 위치 비교(`call > gui_arm`) 대신 이걸 쓴다 — 위치 비교는 `run()` 아래에
/// 정의된 **어떤** 함수(CLI 경로인 `run_subcommand` 포함)에 호출을 옮겨도 통과한다.
fn span_of(lines: &[&str], header: &str) -> Option<(usize, usize)> {
    let start = lines
        .iter()
        .position(|l| !is_comment_line(l) && l.contains(header))?;
    let (mut depth, mut opened) = (0i32, false);
    for (i, line) in lines.iter().enumerate().skip(start) {
        for ch in strip_line_comment(line).chars() {
            match ch {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth <= 0 {
            return Some((start, i));
        }
    }
    None
}

/// `fn <name>(` 의 정의 본문 범위.
fn fn_span(lines: &[&str], name: &str) -> Option<(usize, usize)> {
    span_of(lines, &format!("fn {name}("))
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn log_file_is_opened_from_the_host_path_only() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("src"), &mut files);
    assert!(
        !files.is_empty(),
        "src/ 아래 .rs 파일을 하나도 못 찾았다 — 가드가 헛돈다"
    );

    let mut offenders: Vec<String> = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        // 호출처 파일은 `the_call_site_sits_inside_the_host_arm` 이 더 강한 조건
        // (분기 블록 내부)으로 따로 본다.
        if rel == CALL_SITE_FILE {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        // 정의 파일 / 래퍼 파일은 **정의 함수 본문 안** 만 봐준다. 나머지 파일은 어떤
        // 등장도 허용하지 않는다.
        let allowed = if rel == IMPL_FILE || rel == WRAPPER_FILE {
            Some(fn_span(&lines, OPEN_FN).unwrap_or_else(|| {
                panic!("`{rel}` 에서 `fn {OPEN_FN}` 정의 범위를 못 찾았다 — 가드를 갱신한다")
            }))
        } else {
            None
        };
        for (i, line) in lines.iter().enumerate() {
            if !line.contains(OPEN_FN) || is_comment_line(line) {
                continue;
            }
            if allowed.is_some_and(|(start, end)| i >= start && i <= end) {
                continue;
            }
            offenders.push(format!("{rel}:{}: {}", i + 1, line.trim()));
        }
    }

    assert!(
        offenders.is_empty(),
        "`{OPEN_FN}` 은 host 확정 이후의 `{CALL_SITE_FILE}` 에서만 부른다 — \
         CLI 클라이언트도 같은 바이너리라, 다른 경로에서 부르면 CLI 실행마다 실행 중인 \
         host 의 로그가 truncate 된다(ADR-0092):\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_call_site_sits_inside_the_host_arm() {
    let text = std::fs::read_to_string(repo_root().join(CALL_SITE_FILE))
        .expect("호출처 파일을 읽을 수 있어야 한다");
    let lines: Vec<&str> = text.lines().collect();

    let (arm_start, arm_end) = span_of(&lines, "Routed::Gui(cli) =>")
        .expect("`Routed::Gui` 분기 블록을 못 찾았다 — 라우팅 형태가 바뀌었으면 가드도 갱신한다");

    let calls: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| !is_comment_line(l) && l.contains(&format!("{OPEN_FN}()")))
        .map(|(i, _)| i)
        .collect();

    assert_eq!(
        calls.len(),
        1,
        "`{OPEN_FN}()` 호출은 host 경로 한 곳뿐이어야 한다 — 찾은 줄: {:?}",
        calls.iter().map(|i| i + 1).collect::<Vec<_>>()
    );

    let call = calls[0];
    assert!(
        call > arm_start && call <= arm_end,
        "`{OPEN_FN}()` 이 `Routed::Gui` 분기 블록({}~{} 줄) 밖의 {} 줄에 있다 — \
         `run()` 아래 아무 함수(CLI 경로인 `run_subcommand` 포함)로 옮겨도 단순 위치 \
         비교는 통과하므로, 블록 내부인지로 본다. 밖에서 부르면 CLI 프로세스도 파일을 \
         열게 되어 ADR-0092 의 결정이 무너진다",
        arm_start + 1,
        arm_end + 1,
        call + 1
    );
}

/// 파일을 여는 코드가 `init_tracing`(= 역할을 모르는 시점) 으로 되돌아가지 않았는지.
/// 로그 파일명은 `enable_host_file_log` 안에서만 소비돼야 한다.
#[test]
fn tracing_init_does_not_open_the_log_file() {
    let text = std::fs::read_to_string(repo_root().join(IMPL_FILE))
        .expect("구현 파일을 읽을 수 있어야 한다");
    let open_fn_at = text
        .find(&format!("pub fn {OPEN_FN}"))
        .unwrap_or_else(|| panic!("`{OPEN_FN}` 정의를 못 찾았다"));

    for (offset, _) in text.match_indices("log_file_name()") {
        // 정의 자체(`fn log_file_name() -> …`)는 소비가 아니다.
        let line_start = text[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
        if text[line_start..offset].contains("fn ") {
            continue;
        }
        assert!(
            offset > open_fn_at,
            "`{IMPL_FILE}` 에서 로그 파일명을 `{OPEN_FN}` 밖(= 역할 판정 이전에 도는 \
             초기화 경로)에서 쓰고 있다 — CLI 프로세스가 다시 파일을 열게 된다(ADR-0092)"
        );
    }
}
