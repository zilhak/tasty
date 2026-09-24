//! Cheap 평가는 확장자·glob·디렉터리 여부를 확인한다.
//! Deep 평가는 magic·MIME·Lua·JSON Schema를 추가한다. 한 identify 호출의 head/MIME은
//! DeepCtx로 재사용한다. head는 8KB까지 읽으며 구조 검증은 별도로 파일을 읽는다.

use std::collections::HashMap;
use std::path::PathBuf;

use super::types::{DetectorRuleKind, FileTarget};

/// head read 상한 — `magic` 의 offset+bytes 가 이 안에 들어와야 한다.
pub const DEEP_HEAD_CAP: usize = 8 * 1024;

/// 한 `identify` 호출 동안 같은 파일을 여러 번 IO 하지 않도록 캐시.
#[derive(Default)]
pub struct DeepCtx {
    /// path → (metadata is_file, head bytes). head 가 `None` 이면 read 실패 또는 비 regular file.
    cache: HashMap<PathBuf, DeepCacheEntry>,
}

#[derive(Debug, Clone)]
pub(super) struct DeepCacheEntry {
    /// regular file 이면 true. directory / FIFO / socket / device 등은 false.
    pub(super) is_regular: bool,
    /// 최대 `DEEP_HEAD_CAP` 바이트. regular file 이 아니거나 read 실패 시 `None`.
    pub(super) head: Option<Vec<u8>>,
    /// `infer` 가 추정한 MIME. head 가 있을 때만 시도, 매칭 안되면 `None`.
    pub(super) mime: Option<String>,
}

impl DeepCtx {
    pub fn new() -> Self {
        Self::default()
    }

    pub(super) fn entry(&mut self, target: &FileTarget) -> &DeepCacheEntry {
        let path = target.as_path().to_path_buf();
        if !self.cache.contains_key(&path) {
            // URL 은 IO 없이 "regular file 아님" 으로 캐시한다 — `evaluate_deep` 이 이미
            // 막지만 `lua_eval` 등 이 캐시를 직접 여는 평가기가 따로 있다.
            let e = if target.is_url_shaped() {
                DeepCacheEntry {
                    is_regular: false,
                    head: None,
                    mime: None,
                }
            } else {
                read_entry(&path)
            };
            self.cache.insert(path.clone(), e);
        }
        self.cache.get(&path).expect("just inserted")
    }
}

fn read_entry(path: &std::path::Path) -> DeepCacheEntry {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            return DeepCacheEntry {
                is_regular: false,
                head: None,
                mime: None,
            };
        }
    };
    if !meta.is_file() {
        return DeepCacheEntry {
            is_regular: false,
            head: None,
            mime: None,
        };
    }
    let head = read_head(path, DEEP_HEAD_CAP);
    let mime = head
        .as_ref()
        .and_then(|h| infer::get(h).map(|k| k.mime_type().to_string()));
    DeepCacheEntry {
        is_regular: true,
        head,
        mime,
    }
}

fn read_head(path: &std::path::Path, cap: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; cap];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(buf)
}

/// 단일 rule 이 target 에 매칭되는지 평가. cheap path 만.
///
/// 디렉토리 분기는 호출자(registry::identify)가 pre-filter 로 처리하므로 여기서는
/// rule kind 별로 매칭 여부만 판단.
pub fn evaluate_cheap(rule: &DetectorRuleKind, target: &FileTarget) -> bool {
    // registry 를 우회해 평가기를 직접 부르는 자리도 URL 을 파일로 매칭하지 않는다.
    if target.is_url_shaped() {
        return false;
    }
    let path = target.as_path();
    match rule {
        DetectorRuleKind::Extension { values } => path
            .extension()
            .and_then(|s| s.to_str())
            .map(|ext| {
                let lower = ext.to_ascii_lowercase();
                values.iter().any(|v| v.eq_ignore_ascii_case(&lower))
            })
            .unwrap_or(false),

        DetectorRuleKind::PathGlob { pattern } => {
            // 이 단독 평가 함수는 파일명만 비교하고 매번 컴파일한다. registry의 identify는
            // PathGlobCache를 재사용한다. 패턴은 등록할 때 /로 정규화한다.
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => return false,
            };
            globset::Glob::new(pattern)
                .map(|g| g.compile_matcher().is_match(name))
                .unwrap_or(false)
        }

        DetectorRuleKind::IsDirectory => target.is_directory(),

        DetectorRuleKind::Mime { .. }
        | DetectorRuleKind::Magic { .. }
        | DetectorRuleKind::Lua { .. }
        | DetectorRuleKind::StructureCheck { .. }
        | DetectorRuleKind::Unknown { .. } => false,
    }
}

/// Cheap 규칙 또는 magic·MIME·Lua·구조 검증을 실행한다. 한 identify 호출에서 ctx를 재사용한다.
pub fn evaluate_deep(rule: &DetectorRuleKind, target: &FileTarget, ctx: &mut DeepCtx) -> bool {
    // URL 에 파일 IO(metadata / open / Lua / structure-check)를 시도하지 않는다.
    if target.is_url_shaped() {
        return false;
    }
    match rule {
        DetectorRuleKind::Magic { offset, bytes } => {
            // safety: regular file 아닌 경우 read entry 가 is_regular=false 로 표시.
            // FIFO / socket / device 에 head read 시도 안 함.
            let entry = ctx.entry(target);
            if !entry.is_regular {
                return false;
            }
            let head = match entry.head.as_ref() {
                Some(h) => h,
                None => return false,
            };
            let start = *offset;
            let end = start.saturating_add(bytes.len());
            head.get(start..end)
                .map(|slice| slice == bytes.as_slice())
                .unwrap_or(false)
        }

        DetectorRuleKind::Mime { types } => {
            let entry = ctx.entry(target);
            if !entry.is_regular {
                return false;
            }
            let mime = match entry.mime.as_ref() {
                Some(m) => m,
                None => return false,
            };
            types.iter().any(|t| t.eq_ignore_ascii_case(mime))
        }

        DetectorRuleKind::Lua { script } => super::lua_eval::evaluate_lua(script, target, ctx),

        DetectorRuleKind::StructureCheck { spec_path } => {
            super::structure_eval::evaluate_structure(spec_path, target)
        }

        DetectorRuleKind::Extension { .. }
        | DetectorRuleKind::PathGlob { .. }
        | DetectorRuleKind::IsDirectory => evaluate_cheap(rule, target),

        DetectorRuleKind::Unknown { .. } => false,
    }
}

/// globset과 호환 범위를 대조하는 시험용 매처. 제품에서는 사용하지 않는다.
/// 여러 *는 지원하지만 ?/[...]/** 문법은 해석하지 않는다.
#[cfg(test)]
fn simple_glob_match(pattern: &str, name: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == name;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut cursor = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !name[cursor..].starts_with(part) {
                return false;
            }
            cursor += part.len();
        } else if i == parts.len() - 1 {
            if !name.ends_with(part) {
                return false;
            }
        } else {
            match name[cursor..].find(part) {
                Some(pos) => cursor += pos + part.len(),
                None => return false,
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn target(p: &str) -> FileTarget {
        FileTarget::new(PathBuf::from(p))
    }

    #[test]
    fn extension_lowercase_match() {
        let rule = DetectorRuleKind::Extension {
            values: vec!["md".into(), "markdown".into()],
        };
        assert!(evaluate_cheap(&rule, &target("a/b.md")));
        assert!(evaluate_cheap(&rule, &target("a/b.MD")));
        assert!(evaluate_cheap(&rule, &target("c.markdown")));
        assert!(!evaluate_cheap(&rule, &target("c.txt")));
        assert!(!evaluate_cheap(&rule, &target("noext")));
    }

    #[test]
    fn path_glob_exact_and_wildcard() {
        let exact = DetectorRuleKind::PathGlob {
            pattern: "Dockerfile".into(),
        };
        assert!(evaluate_cheap(&exact, &target("repo/Dockerfile")));
        assert!(!evaluate_cheap(&exact, &target("repo/Dockerfile.txt")));

        let wild = DetectorRuleKind::PathGlob {
            pattern: "*.config.json".into(),
        };
        assert!(evaluate_cheap(&wild, &target("foo/bar.config.json")));
        assert!(!evaluate_cheap(&wild, &target("foo/bar.json")));
    }

    #[test]
    fn path_glob_supports_standard_glob_syntax() {
        let question = DetectorRuleKind::PathGlob {
            pattern: "file?.txt".into(),
        };
        assert!(evaluate_cheap(&question, &target("file1.txt")));
        assert!(!evaluate_cheap(&question, &target("file12.txt")));

        let bracket = DetectorRuleKind::PathGlob {
            pattern: "[abc]*.rs".into(),
        };
        assert!(evaluate_cheap(&bracket, &target("a_test.rs")));
        assert!(!evaluate_cheap(&bracket, &target("d_test.rs")));

        let double_star = DetectorRuleKind::PathGlob {
            pattern: "**/*.rs".into(),
        };
        // 파일명만 비교하므로 디렉터리를 건너뛰는 효과 없이 ** 문법의 파싱·매칭을 확인한다.
        assert!(evaluate_cheap(&double_star, &target("nested/main.rs")));
    }

    #[test]
    fn glob_migration_compat_agrees_on_previously_supported_patterns() {
        let cases: &[(&str, &str, bool)] = &[
            ("Dockerfile", "Dockerfile", true),
            ("Dockerfile", "dockerfile", false),
            ("*.config.json", "bar.config.json", true),
            ("*.config.json", "bar.json", false),
            ("test.*", "test.rs", true),
            ("a*b*c", "aXbYc", true),
            ("a*b*c", "abc", true),
            ("a*b*c", "ac", false),
            ("*mid*", "xxmidyy", true),
            ("*mid*", "nomatch", false),
        ];
        for (pattern, name, expect) in cases {
            let old = simple_glob_match(pattern, name);
            let new = globset::Glob::new(pattern)
                .map(|g| g.compile_matcher().is_match(name))
                .unwrap_or(false);
            assert_eq!(
                old, *expect,
                "old matcher: pattern={pattern:?} name={name:?}"
            );
            assert_eq!(
                new, *expect,
                "new(globset) matcher regressed vs old: pattern={pattern:?} name={name:?}"
            );
            assert_eq!(
                old, new,
                "old/new matcher disagree: pattern={pattern:?} name={name:?}"
            );
        }
    }

    #[test]
    fn glob_migration_new_matcher_accepts_syntax_old_matcher_rejected() {
        // 옛 simple_glob_match 는 `*` 가 없으면 무조건 exact match 로 취급해
        // `?`/`[...]` 를 리터럴 문자로 봤다 — globset 은 실제 와일드카드로 해석한다.
        let cases: &[(&str, &str)] = &[("file?.txt", "file1.txt"), ("[abc]*.rs", "a_test.rs")];
        for (pattern, name) in cases {
            assert!(
                !simple_glob_match(pattern, name),
                "old matcher unexpectedly matched (test premise broken): {pattern:?} vs {name:?}"
            );
            let new = globset::Glob::new(pattern)
                .map(|g| g.compile_matcher().is_match(name))
                .unwrap_or(false);
            assert!(
                new,
                "new matcher should accept richer glob syntax: {pattern:?} vs {name:?}"
            );
        }
    }

    #[test]
    fn deep_kinds_return_false_in_cheap() {
        let mime = DetectorRuleKind::Mime {
            types: vec!["application/pdf".into()],
        };
        assert!(!evaluate_cheap(&mime, &target("x.pdf")));

        let magic = DetectorRuleKind::Magic {
            offset: 0,
            bytes: vec![0x25, 0x50, 0x44, 0x46],
        };
        assert!(!evaluate_cheap(&magic, &target("x.pdf")));
    }

    fn write_tmp(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.path().join(name);
        std::fs::write(&p, bytes).expect("write tmp");
        p
    }

    #[test]
    fn deep_magic_matches_pdf_header() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_tmp(&dir, "doc.pdf", b"%PDF-1.4\n... rest ...");
        let rule = DetectorRuleKind::Magic {
            offset: 0,
            bytes: b"%PDF".to_vec(),
        };
        let mut ctx = DeepCtx::new();
        assert!(evaluate_deep(&rule, &FileTarget::new(p), &mut ctx));
    }

    #[test]
    fn deep_magic_does_not_match_wrong_offset() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_tmp(&dir, "doc.bin", b"AB%PDF-1.4");
        let rule = DetectorRuleKind::Magic {
            offset: 0,
            bytes: b"%PDF".to_vec(),
        };
        let mut ctx = DeepCtx::new();
        assert!(!evaluate_deep(&rule, &FileTarget::new(p.clone()), &mut ctx));
        let rule_off = DetectorRuleKind::Magic {
            offset: 2,
            bytes: b"%PDF".to_vec(),
        };
        assert!(evaluate_deep(&rule_off, &FileTarget::new(p), &mut ctx));
    }

    #[test]
    fn deep_magic_skips_directory_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = FileTarget::new(dir.path().to_path_buf());
        let rule = DetectorRuleKind::Magic {
            offset: 0,
            bytes: b"%PDF".to_vec(),
        };
        let mut ctx = DeepCtx::new();
        assert!(!evaluate_deep(&rule, &target, &mut ctx));
    }

    #[test]
    fn deep_magic_offset_beyond_head_cap_is_false() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_tmp(&dir, "small.bin", b"hello");
        let rule = DetectorRuleKind::Magic {
            offset: 100,
            bytes: b"world".to_vec(),
        };
        let mut ctx = DeepCtx::new();
        assert!(!evaluate_deep(&rule, &FileTarget::new(p), &mut ctx));
    }

    #[test]
    fn deep_mime_detects_png() {
        let dir = tempfile::tempdir().unwrap();
        // PNG 시그니처: 89 50 4E 47 0D 0A 1A 0A
        let png_sig = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let mut bytes = png_sig.to_vec();
        // padding 으로 infer 가 PNG 로 인식하기 충분히
        bytes.extend_from_slice(&[0u8; 32]);
        let p = write_tmp(&dir, "img.png", &bytes);
        let rule = DetectorRuleKind::Mime {
            types: vec!["image/png".into()],
        };
        let mut ctx = DeepCtx::new();
        assert!(evaluate_deep(&rule, &FileTarget::new(p), &mut ctx));
    }

    #[test]
    fn deep_mime_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let png_sig = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let mut bytes = png_sig.to_vec();
        bytes.extend_from_slice(&[0u8; 32]);
        let p = write_tmp(&dir, "img.png", &bytes);
        let rule = DetectorRuleKind::Mime {
            types: vec!["IMAGE/PNG".into()],
        };
        let mut ctx = DeepCtx::new();
        assert!(evaluate_deep(&rule, &FileTarget::new(p), &mut ctx));
    }

    #[test]
    fn deep_ctx_caches_head_across_rules() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_tmp(&dir, "doc.pdf", b"%PDF-1.4 contents");
        let target = FileTarget::new(p);
        let mut ctx = DeepCtx::new();
        let r1 = DetectorRuleKind::Magic {
            offset: 0,
            bytes: b"%PDF".to_vec(),
        };
        assert!(evaluate_deep(&r1, &target, &mut ctx));
        assert_eq!(ctx.cache.len(), 1);
        let r2 = DetectorRuleKind::Magic {
            offset: 0,
            bytes: b"%PDF-1.4".to_vec(),
        };
        assert!(evaluate_deep(&r2, &target, &mut ctx));
        assert_eq!(ctx.cache.len(), 1);
    }

    #[test]
    fn deep_falls_back_to_cheap_for_extension() {
        let rule = DetectorRuleKind::Extension {
            values: vec!["md".into()],
        };
        let mut ctx = DeepCtx::new();
        assert!(evaluate_deep(&rule, &target("a.md"), &mut ctx));
    }

    /// registry 를 우회해 평가기를 직접 불러도 URL 은 매칭하지 않고 파일 IO 도 안 한다.
    /// Magic 은 원래 IO 를 해야 판정되는 rule 이라 캐시 항목이 regular 가 아닌지로 확인한다.
    #[test]
    fn url_target_is_never_matched_or_read() {
        let url = target("https://example.com/a.md");
        let ext = DetectorRuleKind::Extension {
            values: vec!["md".into()],
        };
        let glob = DetectorRuleKind::PathGlob {
            pattern: "*.md".into(),
        };
        assert!(!evaluate_cheap(&ext, &url));
        assert!(!evaluate_cheap(&glob, &url));
        let mut ctx = DeepCtx::new();
        assert!(!evaluate_deep(&ext, &url, &mut ctx));
        assert!(!evaluate_deep(
            &DetectorRuleKind::Magic {
                offset: 0,
                bytes: b"https".to_vec(),
            },
            &url,
            &mut ctx,
        ));
        assert!(!ctx.entry(&url).is_regular);
        assert!(ctx.entry(&url).head.is_none());
    }
}
