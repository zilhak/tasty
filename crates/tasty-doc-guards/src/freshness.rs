//! 판정기가 **자기가 지금 소스와 맞는지** 스스로 답한다.
//!
//! ## 왜 mtime 이 아닌가
//!
//! 게이트가 물어야 할 것은 "이 바이너리가 지금 소스로 지어진 것인가" 다. mtime 으로 재면
//! **git 이 파일을 다시 쓰기만 해도 낡은 것으로 나온다** — 내용이 같아도 그렇다.
//! 실측: 브랜치를 main 으로 다시 잡았다가 체리픽으로 되돌리면 소스는 옛 내용을 거쳐
//! 원래 내용으로 돌아오는데, 그 왕복이 mtime 을 새것으로 만든다. 그 흐름은 이 저장소의
//! 표준 절차라 **경고가 상시 켜지고**, 폴백(판정을 넓히는 쪽)이 상시 동작한다. 안전한
//! 방향이라도 상시 켜지면 그것이 기본값이 되어 게이트가 하던 일을 안 하게 된다.
//!
//! 그래서 **내용**으로 판정한다. 라이브러리 지문은 빌드 스크립트가 구워 넣고, 각
//! 바이너리는 자기 소스 한 벌을 직접 들고 있다.
//!
//! ## 왜 바이너리 소스를 지문에서 뺐나
//!
//! 판정기를 **하나 추가**한 것이 다른 판정기를 낡게 만들면 안 된다(실측으로 밟았다).
//! 그래서 지문은 라이브러리만 덮고, 자기 소스는 각자 확인한다.

use std::path::Path;

/// 빌드할 때 구운 라이브러리 소스 지문.
pub const LIB_FINGERPRINT: &str = env!("TASTY_DOC_GUARDS_LIB_FINGERPRINT");

/// 대조 결과.
#[derive(Debug, PartialEq, Eq)]
pub enum Freshness {
    /// 지어진 내용과 디스크의 내용이 같다.
    Fresh,
    /// 다르다 — 다시 지어야 한다. 무엇이 다른지 사람이 읽을 수 있게 담는다.
    Stale(String),
    /// **물을 수 없다.** 판정기 소스가 이 트리에 없다(배포 tarball · 합성 픽스처
    /// 저장소). 낡음과 구분해야 한다 — 없는 것을 낡았다고 하면 정상 상황이 경고가 된다.
    Undecidable,
}

/// 라이브러리 지문과 **자기 소스 한 벌**을 함께 대조한다.
///
/// `own_rel` 은 레포 루트 기준 경로, `own_text` 는 그 파일을 컴파일할 때 담은 내용
/// (`include_str!`). 둘을 인자로 받는 이유는 호출자가 바이너리마다 다르기 때문이고,
/// 여기서 경로를 추측하면 바이너리가 늘 때 조용히 빗나간다.
pub fn check(root: &Path, own_rel: &str, own_text: &str) -> Freshness {
    let lib_dir = root.join("crates/tasty-doc-guards/src");
    if !lib_dir.is_dir() {
        return Freshness::Undecidable;
    }
    let Some(on_disk) = fingerprint(&lib_dir) else {
        return Freshness::Undecidable;
    };
    if on_disk != LIB_FINGERPRINT {
        return Freshness::Stale(format!(
            "라이브러리 소스가 지어진 것과 다르다 (구운 값 {LIB_FINGERPRINT}, 디스크 {on_disk})"
        ));
    }
    let own_path = root.join(own_rel);
    let Ok(disk_text) = std::fs::read_to_string(&own_path) else {
        // 자기 소스가 없는 것은 "물을 수 없다" 가 아니다 — 라이브러리는 있는데 이
        // 파일만 없으면 트리가 어긋난 것이다.
        return Freshness::Stale(format!("자기 소스를 읽을 수 없다: {own_rel}"));
    };
    if disk_text.replace("\r\n", "\n") != own_text.replace("\r\n", "\n") {
        return Freshness::Stale(format!("자기 소스가 지어진 것과 다르다: {own_rel}"));
    }
    Freshness::Fresh
}

/// 빌드 스크립트와 **같은 규칙**으로 디스크에서 지문을 구한다. 두 계산이 갈리면
/// 판정기가 영원히 낡은 것으로 나오므로, 규칙(정렬·`bin/` 제외·LF 정규화)을 바꿀 때는
/// 양쪽을 함께 고친다.
fn fingerprint(lib_dir: &Path) -> Option<String> {
    let mut files = Vec::new();
    collect(lib_dir, lib_dir, &mut files)?;
    files.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (rel, bytes) in &files {
        mix(&mut h, rel.as_bytes());
        mix(&mut h, bytes);
    }
    Some(format!("{h:016x}"))
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Option<()> {
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "bin") {
                continue;
            }
            collect(root, &path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            let rel = path
                .strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, normalize(&std::fs::read(&path).ok()?)));
        }
    }
    Some(())
}

fn normalize(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn mix(h: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *h ^= u64::from(*b);
        *h = h.wrapping_mul(0x100_0000_01b3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이 크레이트 자신에 대고 물으면 **맞아야** 한다. 이것이 빌드 스크립트와 여기의
    /// 계산 규칙이 갈리지 않았다는 유일한 증거다 — 갈리면 판정기가 영원히 낡은 것으로
    /// 나오고, 그 상태는 조용히 폴백만 켠다.
    #[test]
    fn the_baked_fingerprint_matches_this_tree() {
        let root = crate::repo_root();
        let lib_dir = root.join("crates/tasty-doc-guards/src");
        assert!(
            lib_dir.is_dir(),
            "판정기 소스가 없다: {}",
            lib_dir.display()
        );
        assert_eq!(
            fingerprint(&lib_dir).expect("지문"),
            LIB_FINGERPRINT,
            "빌드 스크립트와 런타임의 지문 계산이 갈렸다 — 판정기가 영원히 낡은 것으로 나온다"
        );
    }

    /// 소스가 없는 트리는 **낡음이 아니라 물을 수 없음**이다. 둘을 섞으면 배포
    /// tarball 과 합성 픽스처 저장소에서 정상 상황이 경고가 된다.
    #[test]
    fn a_tree_without_the_judge_sources_is_undecidable() {
        let empty = std::env::temp_dir().join(format!("tasty-fresh-{}", std::process::id()));
        std::fs::create_dir_all(&empty).expect("임시 디렉토리");
        assert_eq!(check(&empty, "x.rs", "y"), Freshness::Undecidable);
        // 뒷정리다 — 여기서 실패해도 판정은 이미 끝났다.
        let _ = std::fs::remove_dir_all(&empty);
    }

    /// 자기 소스가 다르면 낡음이다 — 라이브러리가 맞아도 그렇다. 이 갈래가 없으면
    /// 바이너리 하나만 고친 경우를 못 본다.
    #[test]
    fn a_changed_own_source_is_stale_even_when_the_library_matches() {
        let root = crate::repo_root();
        let got = check(
            &root,
            "crates/tasty-doc-guards/src/lib.rs",
            "이 내용은 디스크와 다르다",
        );
        assert!(
            matches!(got, Freshness::Stale(_)),
            "자기 소스 차이를 못 봤다: {got:?}"
        );
    }
}
