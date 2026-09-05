//! 문서를 읽는 가드들의 집 — **의존이 0 인 것이 이 크레이트의 존재 이유다.**
//!
//! 여기 사는 통합 타깃은 `docs/` · `site/` · `*.md` 를 읽고 소스·워크플로 텍스트와
//! 대조한다. 크레이트 코드는 한 줄도 안 쓴다. 본체 패키지에 얹혀 있을 때 그 가드들의
//! 유일한 자동 채널은 `check-headless` 였는데, 그 잡은
//! `paths-ignore: docs/** · site/** · **/*.md` 뒤에 있다 — **문서만 바뀐 push 에서
//! 정확히 꺼진다.** 즉 그 가드들을 위반할 수 있는 유일한 변경에서만 안 돌았다.
//!
//! 경로 필터를 그냥 떼면 문서 한 줄 고칠 때마다 본체 컴파일(수백 크레이트)이 붙는다.
//! 그래서 필터를 떼는 대신 **잡을 싸게 만들었다** — 의존 0 이면 콜드 빌드가 1 초 미만이라
//! 필터가 필요 없다. 배경·대안·재검토 트리거는 ADR-0138.
//!
//! 여기에 의존을 하나라도 더하면 그 결정의 전제가 사라진다. `Cargo.toml` 의
//! `[dependencies]` 는 비어 있어야 하고, `the_crate_has_no_dependencies` 가 그것을 본다.

// 이유: 테스트 본문의 `let _ =` 는 정책이 사유를 요구하지 않는 자리라
// `clippy::let_underscore_must_use` 명부에 섞이면 안 된다 — 그 명부는 프로덕션에서
// 값을 버리는 자리의 목록이고, 테스트가 늘 때마다 숫자만 흔들리면 새 프로덕션
// 자리가 그 안에 묻힌다(docs/dev-guide/error-handling.md). `cfg_attr(test, ..)` 라
// 라이브러리 타깃의 판정은 그대로다 — 프로덕션 자리는 여전히 명부에 오른다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

/// `#[cfg(...)]` 술어를 읽는다.
pub mod cfg_predicate;
/// 지문 계산 규칙 — `build.rs` 와 **원문 한 벌**을 공유한다(그쪽은 `include!`).
mod fingerprint_rule;
pub mod freshness;
pub mod manifest_text;

/// 아키텍처 문서의 계층 절과 크레이트의 워크스페이스 내부 의존을 읽는다.
pub mod crate_layers;

/// 소스를 텍스트로 읽는 가드들이 공유하는 마스킹·순회.
pub mod source_text;

/// **이 파일은 출하되는가** — 선언 기반 판정 하나.
pub mod shipping_scope;

/// 락 poison 을 보고 없이 복구하는 자리를 집는다.
pub mod poison_recovery;

/// 공유 temp 아래 고정 이름 임시 경로를 집는다(ADR-0129 형태 B).
pub mod temp_path;

/// env·cwd 를 직렬화 없이 만지는 테스트를 집는다(ADR-0129 형태 A).
pub mod env_isolation;

/// 워크플로의 `on:` 트리거를 구조로 읽는다 — 주석과 트리거 키를 가른다.
pub mod workflow_triggers;

/// 하한을 빠뜨릴 수 없는 공용 순회 — 순회가 죽었을 때 조용히 통과하는 것을 막는다.
pub mod floored_walk;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 레포 루트. 이 크레이트가 워크스페이스 루트가 아니라 `crates/<이름>` 아래 살기 때문에
/// `CARGO_MANIFEST_DIR` 이 곧 레포 루트가 아니다 — 두 칸 올라간다.
///
/// **틀린 루트로 조용히 진행하지 않는다.** 스캔 가드에서 경로가 틀어지면 예외가 아니라
/// **조용한 0** 이 나오고, 0 인 모수는 언제나 초록이다(ADR-0133). 그래서 올라간 자리가
/// 레포 루트가 맞는지 표지 파일로 확인하고, 아니면 panic 한다. 여기 사는 타깃이
/// 전부 이 함수를 쓰므로 확인 지점은 하나면 된다.
///
/// 표지는 **이 크레이트 밖에 있고 지워질 리 없는 것**으로 고른다 — `Cargo.toml` 만 보면
/// 이 크레이트 자신의 디렉토리도 통과한다.
pub fn repo_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("CARGO_MANIFEST_DIR 에서 두 칸 올라갈 수 없다"))
        .to_path_buf();

    for marker in [
        "Cargo.toml",
        "CHANGELOG.md",
        "docs/adr/index.md",
        ".github/workflows",
    ] {
        assert!(
            root.join(marker).exists(),
            "레포 루트로 잡은 {} 에 표지 {marker} 가 없다 — 경로가 틀어졌다. \
             그냥 진행하면 이 크레이트의 가드 전부가 빈 모수로 조용히 초록이 된다.",
            root.display()
        );
    }
    root
}

/// `CACHEDIR.TAG` 의 서명 줄. 규격이 정한 값이라 도구가 달라도 같다 — cargo·ccache·
/// bazel 등이 모두 이 문자열로 캐시 디렉토리를 표시한다.
const CACHEDIR_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// 이 디렉토리가 **빌드 캐시인가** — 이름이 아니라 표식으로 판정한다.
///
/// 스캔 가드는 빌드 산출물을 건너뛰어야 한다. 종전 수단은 디렉토리 **이름**(`"target"`)
/// 이었는데 이름은 성질이 아니다: `CARGO_TARGET_DIR` 로 다른 이름을 주면 그 디렉토리가
/// 통째로 모수에 들어온다. 실측(2026-09-05)으로 그 상태를 만들었을 때 이 크레이트의
/// `no_todo_file_citation` 은 1.30s → 86.30s (66 배), 루트의
/// `no_early_exit_consumer_in_shell_pipes` 는 0.05s → 89.17s (1783 배) 가 됐다.
///
/// **왜 `CACHEDIR.TAG` 인가 — 후보 셋을 관측으로 갈랐다.**
///
/// - `.gitignore` 준수: 탈락. `.gitignore` 자신이 이름 기반이다(`/target`·`/site/target`
///   앵커 규칙). 다른 이름의 빌드 디렉토리는 `git check-ignore` 가 무시하지 않는다 —
///   고치려는 결함을 그대로 물려받는다.
/// - `cargo metadata` 의 `target_directory`: 탈락. **지금** 설정된 한 곳만 답한다.
///   이전 빌드가 남긴 디렉토리는 못 본다.
/// - `CACHEDIR.TAG`: 채택. cargo 가 **빌드할 때 실제로 만든다**(실측: `cargo metadata`
///   만으로는 안 만든다). 디렉토리 자신이 갖는 성질이라 개명해도 따라온다.
///
/// 존재만 보지 않고 **서명 줄까지 확인한다** — 규격이 파일을 그 첫 줄로 정의하므로,
/// 같은 이름의 무관한 파일이 소스 디렉토리를 통째로 가지치기하게 두지 않는다.
///
/// 이것은 이름 가지치기를 **대체하지 않고 보탠다.** `.git`·`node_modules`·vendored
/// `assets` 는 표식을 달지 않으므로 이름이 여전히 유일한 근거다 — 그쪽은 성질이 다른
/// 문제다(산출물이 아니라 "추적되지만 우리가 안 쓴 콘텐츠").
pub fn is_build_cache_dir(dir: &Path) -> bool {
    let Ok(head) = std::fs::read(dir.join("CACHEDIR.TAG")) else {
        return false;
    };
    head.starts_with(CACHEDIR_SIGNATURE)
}

/// 파일 이름의 확장자가 **텍스트로 열 수 없는 바이너리·산출물**인지 — 소스를 텍스트로
/// 읽는 가드가 "이 파일 내용을 스캔할 수 있나" 를 물을 때 쓰는 공용 denylist.
///
/// `cited_coordinates_exist` 와 `no_todo_file_citation` 이 각자 `SKIP_EXTS` 사본을 두고
/// 있었다(전자는 후자를 베끼며 그 사실을 주석에 적었다). 같은 물음이라 정본을 하나 둔다.
/// 가드가 더 뺄 형식(예: 좌표 인용을 안 담는 `.svg`·`.lock`)은 이 위에 얹는다 — 판정은
/// 하나, 모수는 각자다(ADR-0180: 정본은 판정, 스캔 범위는 소비자별).
pub fn is_binary_artifact_ext(ext: &str) -> bool {
    BINARY_ARTIFACT_EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

/// [`is_binary_artifact_ext`] 의 denylist. 텍스트로 열 수 없는 형식만 — 여기 없는
/// 확장자는 스캔 대상이다(denylist 전수).
pub const BINARY_ARTIFACT_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "icns", "pdf", "ttf", "otf", "woff", "woff2", "zip",
    "gz", "xz", "tar", "wasm", "bin", "so", "dylib", "dll", "exe", "sig",
];

/// 면제 항목이 가리키는 경로 중 **실재하지 않는 것**을 돌려준다 — 참조 무결성.
///
/// **이 판정의 초록이 뜻하는 것은 "면제가 아직 필요하다" 가 아니다.** 가리키는 것이
/// 실재한다는 것뿐이다. 가리키는 파일이 있어도 그 면제가 아무것도 안 덮고 있을 수 있고,
/// 그것은 결함이 아니다(ADR-0150). 두 축을 섞으면 "안 덮으면 지워라" 라는 틀린 처방이
/// 참조 무결성의 옷을 입고 되살아난다.
///
/// **왜 필요한가**: 경로가 썩으면 그 면제는 조용히 아무 일도 안 하게 된다. 면제가 덮던
/// 자리는 이제 검사받지만, 목록에는 "여기는 원래 위반해도 된다" 는 신호가 남는다. 실측
/// (2026-09-05) 경로·키를 가리키는 면제 8 겹 중 이 검사를 가진 것은 하나뿐이었다.
///
/// **끝의 `/` 로 갈린다.** 접두 면제는 디렉토리를 가리키고 나머지는 파일을 가리킨다 —
/// 한 목록이 둘을 섞어 담는 것은 그 목록의 매칭이 접두이기 때문이라 설계상 그렇다.
/// 그래서 `exists()` 로 뭉개지 않고 쓰인 형태대로 판정한다: 파일 자리에 디렉토리가
/// 생겨도(그 반대도) 면제는 의도한 것을 더 이상 안 가리킨다.
pub fn missing_referents<'a>(
    root: &Path,
    cited: impl IntoIterator<Item = &'a str>,
) -> Vec<&'a str> {
    cited
        .into_iter()
        .filter(|rel| {
            let full = root.join(rel.trim_end_matches('/'));
            if rel.ends_with('/') {
                !full.is_dir()
            } else {
                !full.is_file()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 표식이 있는 임시 디렉토리를 만든다. `tempfile` 을 쓰지 않는 이유는 이 크레이트의
    /// **의존이 0 이어야 하기 때문**이다(ADR-0138) — dev-dependency 도 이 크레이트의
    /// 잡을 비싸게 만든다.
    fn temp_dir_named(suffix: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tasty-cachedir-{}-{suffix}", std::process::id()));
        // 이전 완주가 남긴 것을 치운다. **없는 것이 정상이라 실패가 아니다** — 뒤의
        // create_dir_all 이 진짜 판정이고, 여기서 죽으면 그 판정을 못 본다.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("임시 디렉토리 생성");
        dir
    }

    /// 양극성 — **합성 입력으로 잡는다.** 실재하는 면제 목록을 상대로만 시험하면 그
    /// 목록을 고치는 순간 이 회귀가 거짓 초록이 된다.
    #[test]
    fn missing_referents_reports_only_what_is_absent() {
        let root = repo_root();
        let found = missing_referents(
            &root,
            [
                "Cargo.toml",
                "src/__no_such_file__.rs",
                "crates/",
                "crates/__no_such_dir__/",
            ],
        );
        assert_eq!(
            found,
            vec!["src/__no_such_file__.rs", "crates/__no_such_dir__/"]
        );
    }

    /// 끝의 `/` 가 판정을 가른다 — 파일을 디렉토리로, 디렉토리를 파일로 적으면 잡힌다.
    /// 이 절이 없으면 `exists()` 로 뭉갠 구현도 위 테스트를 통과한다.
    #[test]
    fn the_trailing_slash_decides_which_kind_is_required() {
        let root = repo_root();
        assert_eq!(
            missing_referents(&root, ["Cargo.toml/"]),
            vec!["Cargo.toml/"]
        );
        assert_eq!(missing_referents(&root, ["crates"]), vec!["crates"]);
        assert!(missing_referents(&root, ["Cargo.toml", "crates/"]).is_empty());
    }

    /// 정본 denylist teeth — 바이너리는 막고 텍스트·무확장자는 통과시킨다. 이 판정을
    /// `cited_coordinates_exist`·`no_todo_file_citation` 이 위임받는다(ADR-0180).
    #[test]
    fn binary_exts_are_denied_and_text_is_scanned() {
        assert!(is_binary_artifact_ext("png"));
        assert!(is_binary_artifact_ext("PNG"), "대소문자 무관이어야 한다");
        assert!(is_binary_artifact_ext("woff2"));
        assert!(!is_binary_artifact_ext("rs"), "소스는 스캔 대상이다");
        assert!(!is_binary_artifact_ext("md"));
        assert!(
            !is_binary_artifact_ext(""),
            "확장자 없음은 텍스트로 본다(denylist 전수)"
        );
        // 소비자가 위에 얹는 형식은 정본에 없다 — 그건 각 가드의 모수다.
        assert!(!is_binary_artifact_ext("lock"));
        assert!(!is_binary_artifact_ext("svg"));
    }

    #[test]
    fn a_directory_with_the_signature_is_a_build_cache() {
        let dir = temp_dir_named("hit");
        std::fs::write(
            dir.join("CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n# cargo\n",
        )
        .expect("표식 쓰기");
        assert!(is_build_cache_dir(&dir));
        // 정리 실패는 무시한다 — 임시 디렉토리라 남아도 판정에 영향이 없고, 여기서
        // 죽으면 진짜 실패가 정리 오류에 가린다.
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 반대 극성 — **이름만 같은 파일로는 가지치기되지 않는다.** 이 절이 없으면
    /// "존재하면 참" 으로 퇴화해도 위 테스트가 초록이라, 소스 디렉토리에 우연히
    /// 같은 이름의 파일이 생겼을 때 그 디렉토리가 통째로 모수에서 사라진다.
    #[test]
    fn a_file_with_the_same_name_but_wrong_content_is_not_enough() {
        let dir = temp_dir_named("miss");
        std::fs::write(dir.join("CACHEDIR.TAG"), "메모\n").expect("가짜 표식 쓰기");
        assert!(!is_build_cache_dir(&dir));

        std::fs::remove_file(dir.join("CACHEDIR.TAG")).expect("표식 제거");
        assert!(!is_build_cache_dir(&dir), "표식이 없으면 캐시가 아니다");
        // 정리 실패는 무시한다 — 임시 디렉토리라 남아도 판정에 영향이 없고, 여기서
        // 죽으면 진짜 실패가 정리 오류에 가린다.
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 레포에서의 실제 관측 — 소스 루트는 캐시가 아니고, 빌드 디렉토리는 캐시다.
    /// `target/` 은 이 테스트를 돌리는 `cargo test` 자신이 만들지만, 다른
    /// `CARGO_TARGET_DIR` 로 돌 수도 있으므로 **있을 때만** 단정한다.
    #[test]
    fn the_repo_root_is_not_a_cache_but_a_build_dir_is() {
        let root = repo_root();
        assert!(!is_build_cache_dir(&root));
        assert!(!is_build_cache_dir(&root.join("crates")));

        let target = root.join("target");
        if target.join("CACHEDIR.TAG").exists() {
            assert!(is_build_cache_dir(&target));
        }
    }

    #[test]
    fn the_root_is_the_repo_not_this_crate() {
        let root = repo_root();
        assert!(root.join("crates/tasty-doc-guards/Cargo.toml").exists());
        assert!(!root.ends_with("tasty-doc-guards"));
    }

    /// 표지 검사가 실제로 무엇을 거르는지 — 이 크레이트 디렉토리는 `Cargo.toml` 이
    /// 있어도 표지 전체는 못 채운다. 표지를 `Cargo.toml` 하나로 줄이면 이 판정이 죽는다.
    #[test]
    fn this_crate_dir_would_not_pass_as_the_root() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(here.join("Cargo.toml").exists(), "대조: Cargo.toml 은 있다");
        assert!(!here.join("CHANGELOG.md").exists());
        assert!(!here.join("docs/adr/index.md").exists());
    }
}

// ── `crates/tasty-ipc/src/method_meta.rs` 의 `METHOD_TABLE` 텍스트 판독 ──────────
//
// 가드 둘이 같은 표를 **같은 뜻으로** 읽는다(권한 표의 메서드 목록 · 토큰 없이 부를 수
// 있는 메서드). 그래서 한 벌로 둔다 — 사본이 둘이면 갈리고, 갈린 쪽은 조용하다. 물음이
// 달랐다면 합치는 것이 손실이었을 것이다.
//
// 이 판독이 진짜 표와 어긋날 수 있다는 것이 이 방식의 유일한 위험이다. 그 위험은 본체
// 패키지의 `tests/method_table_readings_agree.rs` 가 런타임 열거와 대조해 붙박는다 —
// 그 대조는 `tasty_ipc` 를 링크해야 하므로 의존 0 인 여기 둘 수 없다.

/// 줄 주석을 지운다. 문자열 리터럴 안의 `//` 는 건드리지 않는다.
pub fn strip_line_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let mut in_str = false;
        let mut cut = line.len();
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'\\' if in_str => i += 1,
                b'"' => in_str = !in_str,
                b'/' if !in_str && i + 1 < b.len() && b[i + 1] == b'/' => {
                    cut = i;
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        out.push_str(&line[..cut]);
        out.push(' ');
    }
    out
}

/// 이 파서가 **해석할 줄 아는** 생성자와 그 뜻.
///
/// `true` = plugin 이 부를 수 있다(권한 목록을 `&[..]` 에서 읽는다), `false` = 못 부른다.
/// 이 목록은 아래 [`constructors_in_source`] 가 소스에서 뽑은 집합과 **대조된다** —
/// 손으로 적은 목록은 진짜 집합과 따로 늙기 때문이다.
pub const KNOWN_CTORS: &[(&str, bool)] = &[
    ("plugin", true),
    // 권한 축에서 `plugin` 과 같다. 다른 것은 외부 호출자에게 dispatch arm 이 없다는
    // 것뿐이고, 그건 이 문서(권한 표)의 관심사가 아니다.
    ("plugin_only", true),
    ("local_only", false),
];

/// `METHOD_TABLE` 을 메서드 → 필요 variant 로 읽는다. `None` 은 plugin 이 못 부르는 것.
///
/// 항목이 여러 줄에 걸치고 후행 쉼표가 붙는 형태(`(\n "x",\n plugin(&[..]),\n)`)가 실제로
/// 있으므로 줄 단위로 읽지 않는다 — 그렇게 읽으면 그 항목들이 **조용히 빠진다.**
///
/// 모르는 생성자를 만나면 **실패한다.** 건너뛰면 검사가 조용히 꺼지고, 검사가 있다는
/// 사실 자체가 거짓이 된다.
pub fn method_table(src: &str) -> BTreeMap<String, Option<Vec<String>>> {
    let start = src
        .find("pub const METHOD_TABLE")
        .expect("METHOD_TABLE 을 못 찾았다");
    let end = src[start..]
        .find("\npub const DEBUG_METHODS")
        .expect("METHOD_TABLE 의 끝을 못 찾았다");
    let flat = strip_line_comments(&src[start..start + end]);
    let b = flat.as_bytes();
    let mut out = BTreeMap::new();
    let mut unknown: Vec<String> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'"' {
            i += 1;
            continue;
        }
        let Some(close) = flat[i + 1..].find('"') else {
            break;
        };
        let name = &flat[i + 1..i + 1 + close];
        let after = &flat[i + 1 + close + 1..];
        let trimmed = after.trim_start();
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_')
            && trimmed.starts_with(',')
        {
            let tail = trimmed[1..].trim_start();
            let ctor: String = tail
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
                .collect();
            if tail[ctor.len()..].starts_with('(') {
                let Some((_, plugin_callable)) =
                    KNOWN_CTORS.iter().find(|(n, _)| *n == ctor.as_str())
                else {
                    unknown.push(format!("{name} → {ctor}(…)"));
                    i += 1 + close + 1;
                    continue;
                };
                if *plugin_callable {
                    let rest = &tail[ctor.len() + 1..];
                    let open = rest.find('[').expect("plugin 계열은 &[..] 형태다");
                    let close2 = rest[open..].find(']').expect("&[..] 가 안 닫혔다");
                    let inner = &rest[open + 1..open + close2];
                    let vs: Vec<String> = inner
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    out.insert(name.to_string(), Some(vs));
                } else {
                    out.insert(name.to_string(), None);
                }
            }
        }
        i += 1 + close + 1;
    }
    // 빈 판독은 어느 경우에도 정당하지 않다 — 위 두 `expect` 를 통과했다는 것은 표가
    // 있다는 뜻이니, 여기서 0 건이면 표가 빈 것이 아니라 **판독기가 못 읽은 것**이다.
    // 이 단정이 없으면 소비자 중 「A 의 원소가 전부 B 에 있나」 형태(한 방향)는 A 가
    // 비는 순간 조용히 초록이 된다. 실측: 이 단정 없이 이름 문자셋을 깨면 소비자 5 개
    // 판정 중 2 개가 살아남았다.
    assert!(
        !out.is_empty(),
        "METHOD_TABLE 을 텍스트로 읽었는데 한 건도 안 나왔다 — 표가 빈 것이 아니라 \
         파서가 형태를 못 맞춘 것이다. 이 상태로 반환하면 이 표를 쓰는 가드들이 \
         빈 집합을 대조하며 통과한다."
    );
    assert!(
        unknown.is_empty(),
        "METHOD_TABLE 에 이 파서가 모르는 생성자가 있다 — 건너뛰면 그 항목들이 표에서 \
         사라져 이 가드가 거짓 결함을 보고한다. `KNOWN_CTORS` 에 해석을 더해라:\n  {}",
        unknown.join("\n  ")
    );
    out
}
