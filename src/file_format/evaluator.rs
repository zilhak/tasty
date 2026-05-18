//! detector rule 평가자.
//!
//! - **Cheap path**: 확장자/glob/is-directory. file IO 없음. hover/typing 등 hot path 용.
//! - **Deep path**: magic bytes / MIME / Lua / structure-check.
//!   magic/MIME 은 8KB head read 1회 + `DeepCtx` 에 캐시. structure-check 는
//!   `structure_eval.rs` 가 5MB cap 으로 전체 파일을 따로 읽는다 (head 8KB 로는
//!   구조 검증에 부족).
//!
//! `evaluate_cheap` 은 단독 호출 가능. `evaluate_deep` 은 `DeepCtx` 가 필요한데,
//! 한 `identify` 호출 안에서 detector 여러 개가 같은 파일의 magic/MIME 을 평가해도
//! head 는 1회만 read.

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
            let e = read_entry(&path);
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
            }
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
            // 1단계: 파일명만 비교 (간단한 wildcard). 본격 glob 매칭은 globset crate 도입
            // 시점에 교체. `Dockerfile`, `*.config.json` 같은 패턴 최소 지원.
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => return false,
            };
            simple_glob_match(pattern, name)
        }

        DetectorRuleKind::IsDirectory => target.is_directory(),

        // 다음 항목들은 Phase A cheap path 에서 false (deep 단계에서 평가).
        DetectorRuleKind::Mime { .. }
        | DetectorRuleKind::Magic { .. }
        | DetectorRuleKind::Lua { .. }
        | DetectorRuleKind::StructureCheck { .. }
        | DetectorRuleKind::Unknown { .. } => false,
    }
}

/// Deep 평가. cheap kind 는 `evaluate_cheap` 으로 위임, magic/MIME 만 새로 처리.
///
/// `ctx` 는 같은 `identify` 호출 안에서 재사용. head/metadata 가 캐시된다.
pub fn evaluate_deep(
    rule: &DetectorRuleKind,
    target: &FileTarget,
    ctx: &mut DeepCtx,
) -> bool {
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

        // cheap kind 는 IO 없이 그대로 평가.
        DetectorRuleKind::Extension { .. }
        | DetectorRuleKind::PathGlob { .. }
        | DetectorRuleKind::IsDirectory => evaluate_cheap(rule, target),

        DetectorRuleKind::Unknown { .. } => false,
    }
}

/// 매우 단순한 glob 매처 — `*` 와 정확 일치만 지원. Phase B 에서 `globset` 도입.
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

    // ── deep path tests ─────────────────────────────────────────────

    fn write_tmp(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.path().join(name);
        std::fs::write(&p, bytes).expect("write tmp");
        p
    }

    #[test]
    fn deep_magic_matches_pdf_header() {
        let dir = tempfile::tempdir().unwrap();
        // %PDF-1.4 헤더
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
        // offset=2 면 매칭.
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
        // 작은 파일에 head cap 보다 큰 offset 요구 → 매칭 실패 (panic 없음)
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
        // 첫 호출 → 캐시 채움
        let r1 = DetectorRuleKind::Magic {
            offset: 0,
            bytes: b"%PDF".to_vec(),
        };
        assert!(evaluate_deep(&r1, &target, &mut ctx));
        assert_eq!(ctx.cache.len(), 1);
        // 다른 rule 도 같은 파일 → 캐시 재사용 (entry 수 동일)
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
}
