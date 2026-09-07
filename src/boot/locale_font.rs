//! 언어팩 `[font]` 선언을 부팅 시 1회 구체 폰트 파일 경로로 resolve 한다.
//!
//! - `file = "fonts/x.ttf"` — 팩 디렉토리 기준 상대경로. 팩 밖으로 벗어나는 경로
//!   (절대경로·상위 참조)는 거부한다.
//! - `family = "Noto Sans Arabic"` — 시스템 폰트 DB 에서 그 패밀리를 **파일로** 가진
//!   face 의 경로.
//! - `candidates = [...]` — 각 항목을 "경로처럼 보이면 file, 아니면 family" 로 순서대로
//!   시도해 첫 성공. 경로처럼 보인다 = `/`·`\` 를 담거나 `.ttf`/`.otf`/`.ttc` 로 끝난다.
//! - `builtin = true` — 아무것도 붙이지 않는다.
//!
//! 어느 쪽이든 붙이기 전에 `ab_glyph`(epaint 가 쓰는 파서)로 검증한다 — egui 는 깨진
//! 폰트에서 복구 경로 없이 panic 하므로 부팅에서 먼저 거른다. resolve 한 절대경로는
//! `TASTY_LOCALE_FONT` env 로 나가(`src/boot/locale.rs`) plugin 프로세스가 같은 파일을
//! 다시 찾지 않게 한다. 실패는 조용하지 않다 — 호출부가 경고(headless/CLI 로그, GUI 토스트)
//! 하고 문자열 자체는 그대로 로드된다.

use std::path::{Component, Path, PathBuf};

use crate::i18n::{FontDecl, LoadOutcome};

/// `[font]` resolve 결과.
pub(crate) enum FontResolution {
    /// 붙일 폰트 없음 — `builtin = true` 이거나 애초에 언어팩이 아니다(내장/오버라이드).
    None,
    /// 검증까지 통과한 폰트 파일 경로.
    Resolved(PathBuf),
    /// `[font]` 를 선언했으나 resolve/검증에 실패 — 경고 대상. 문자열은 그대로 로드된다.
    Failed { detail: String },
}

/// i18n 로드 결과에서 폰트를 resolve 한다. 언어팩(`LoadOutcome::Pack`)이 아니면 `None`.
pub(crate) fn resolve(outcome: &LoadOutcome) -> FontResolution {
    let LoadOutcome::Pack { path, font } = outcome else {
        return FontResolution::None;
    };
    let Some(pack_dir) = path.parent() else {
        return FontResolution::Failed {
            detail: format!("pack manifest has no parent directory: {}", path.display()),
        };
    };
    match font {
        FontDecl::Builtin => FontResolution::None,
        FontDecl::File(rel) => resolve_file(pack_dir, rel),
        FontDecl::Family(name) => resolve_family(name),
        FontDecl::Candidates(items) => resolve_candidates(pack_dir, items),
    }
}

fn resolve_file(pack_dir: &Path, rel: &str) -> FontResolution {
    let relp = Path::new(rel);
    if relp.is_absolute() || relp.components().any(|c| matches!(c, Component::ParentDir)) {
        return FontResolution::Failed {
            detail: format!("[font] file must stay inside the pack directory: {rel}"),
        };
    }
    let full = pack_dir.join(relp);
    match validate(&full) {
        Ok(()) => FontResolution::Resolved(full),
        Err(e) => FontResolution::Failed {
            detail: format!("{}: {e}", full.display()),
        },
    }
}

fn resolve_family(name: &str) -> FontResolution {
    match family_path(name) {
        Some(p) => match validate(&p) {
            Ok(()) => FontResolution::Resolved(p),
            Err(e) => FontResolution::Failed {
                detail: format!("family '{name}' at {}: {e}", p.display()),
            },
        },
        None => FontResolution::Failed {
            detail: format!("font family not installed or not file-backed: {name}"),
        },
    }
}

fn resolve_candidates(pack_dir: &Path, items: &[String]) -> FontResolution {
    let mut errors = Vec::new();
    for item in items {
        let r = if looks_like_path(item) {
            resolve_file(pack_dir, item)
        } else {
            resolve_family(item)
        };
        match r {
            FontResolution::Resolved(p) => return FontResolution::Resolved(p),
            FontResolution::Failed { detail } => errors.push(detail),
            // 후보가 builtin 을 낼 일은 없다(파일/패밀리만).
            FontResolution::None => {}
        }
    }
    FontResolution::Failed {
        detail: format!("no [font] candidate resolved: {}", errors.join(" | ")),
    }
}

/// 후보 항목이 파일 경로처럼 보이는가 — `/`·`\` 를 담거나 폰트 확장자로 끝난다.
fn looks_like_path(s: &str) -> bool {
    if s.contains('/') || s.contains('\\') {
        return true;
    }
    let lower = s.to_ascii_lowercase();
    lower.ends_with(".ttf") || lower.ends_with(".otf") || lower.ends_with(".ttc")
}

/// 시스템 폰트 DB 는 구성이 무거우므로 family 선언이 실제로 있을 때만 만든다.
fn family_path(name: &str) -> Option<PathBuf> {
    // 크기/패밀리는 lookup 에 무관 — 기본값으로 DB 만 올린다.
    let cfg = tasty_font::FontConfig::new(14.0, "");
    cfg.family_source_path(name)
}

/// 파일을 읽어 `ab_glyph` 로 폰트인지 검증한다(바이트는 버린다 — 실제 로드는 append
/// 지점이 다시 읽어 검증한다). egui 가 보기 전에 깨진 파일을 거르는 관문.
fn validate(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read: {e}"))?;
    ab_glyph::FontRef::try_from_slice(&bytes).map_err(|_| "not a valid font file".to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(dir: &Path, font: FontDecl) -> LoadOutcome {
        LoadOutcome::Pack {
            path: dir.join("pack.toml"),
            font,
        }
    }

    fn tmp_pack(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tasty-locale-font-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// ⓪ 대조군 — 유효한 폰트 파일은 반드시 resolve 에 성공해야 한다. 이것이 실패하면
    /// 아래 실패 케이스들의 `Failed` 가 "제대로 걸러서" 인지 "하네스가 다 죽어서" 인지
    /// 구분되지 않는다(R774).
    #[test]
    fn valid_bundled_font_file_resolves() {
        let dir = tmp_pack("valid");
        let fonts_dir = dir.join("fonts");
        std::fs::create_dir_all(&fonts_dir).unwrap();
        std::fs::write(fonts_dir.join("x.ttf"), crate::font::D2CODING_REGULAR_TTF).unwrap();
        let out = pack(&dir, FontDecl::File("fonts/x.ttf".to_string()));
        match resolve(&out) {
            FontResolution::Resolved(p) => assert_eq!(p, dir.join("fonts/x.ttf")),
            other => panic!("expected Resolved, got {:?}", ResolutionDbg(&other)),
        }
        let _ = std::fs::remove_dir_all(&dir); // 정리 실패는 결과에 무관(임시 디렉토리).
    }

    #[test]
    fn builtin_declaration_resolves_to_none() {
        let dir = tmp_pack("builtin");
        assert!(matches!(
            resolve(&pack(&dir, FontDecl::Builtin)),
            FontResolution::None
        ));
        let _ = std::fs::remove_dir_all(&dir); // 정리 실패는 결과에 무관.
    }

    #[test]
    fn non_pack_outcome_resolves_to_none() {
        assert!(matches!(
            resolve(&LoadOutcome::Builtin),
            FontResolution::None
        ));
    }

    #[test]
    fn missing_file_fails_not_panics() {
        let dir = tmp_pack("missing");
        assert!(matches!(
            resolve(&pack(&dir, FontDecl::File("fonts/nope.ttf".to_string()))),
            FontResolution::Failed { .. }
        ));
        let _ = std::fs::remove_dir_all(&dir); // 정리 실패는 결과에 무관.
    }

    #[test]
    fn garbage_bytes_fail_not_panics() {
        let dir = tmp_pack("garbage");
        std::fs::write(dir.join("bad.ttf"), b"definitely not a font").unwrap();
        assert!(matches!(
            resolve(&pack(&dir, FontDecl::File("bad.ttf".to_string()))),
            FontResolution::Failed { .. }
        ));
        let _ = std::fs::remove_dir_all(&dir); // 정리 실패는 결과에 무관.
    }

    #[test]
    fn a_path_that_escapes_the_pack_is_refused() {
        let dir = tmp_pack("escape");
        assert!(matches!(
            resolve(&pack(
                &dir,
                FontDecl::File("../../etc/evil.ttf".to_string())
            )),
            FontResolution::Failed { .. }
        ));
        assert!(matches!(
            resolve(&pack(&dir, FontDecl::File("/etc/evil.ttf".to_string()))),
            FontResolution::Failed { .. }
        ));
        let _ = std::fs::remove_dir_all(&dir); // 정리 실패는 결과에 무관.
    }

    #[test]
    fn looks_like_path_splits_files_from_families() {
        assert!(looks_like_path("fonts/x.ttf"));
        assert!(looks_like_path("x.OTF"));
        assert!(looks_like_path(r"win\path.ttc"));
        assert!(!looks_like_path("Noto Sans Arabic"));
    }

    // `resolve` 결과를 panic 메시지에 찍기 위한 최소 디버그 래퍼.
    struct ResolutionDbg<'a>(&'a FontResolution);
    impl std::fmt::Debug for ResolutionDbg<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self.0 {
                FontResolution::None => write!(f, "None"),
                FontResolution::Resolved(p) => write!(f, "Resolved({})", p.display()),
                FontResolution::Failed { detail } => write!(f, "Failed({detail})"),
            }
        }
    }
}
