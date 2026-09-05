//! 순회 결과가 비어도 조용히 통과하는 검사를 **만들 수 없게** 하는 공용 순회.
//!
//! 스캔 가드의 실패 형태는 둘인데 하나만 보인다. 위반을 찾아 빨개지는 것은 보이고,
//! 순회가 죽어 아무것도 안 보고 초록인 것은 안 보인다. 뒤쪽은 그 가드가 지키려던
//! 것이 깨진 바로 그 순간에도 초록이다.
//!
//! **막는 방법은 각자 하한을 박는 것이고, 실제로 다들 박았다 — 형태가 제각각으로.**
//! 수 하한 상수, 빈 결과 거부, 명부 대조, 판정을 함수로 뽑은 것, 지역 변수 비교…
//! 2026-09-06 에 그 형태들을 정적으로 열거해 "방어 없는 순회" 를 세려다 세 번 반례를
//! 맞았다. 방어는 임의의 코드라 열거가 끝나지 않는다. 열거로 셀 수 있는 것은 **이름을
//! 내가 소유한 것** 뿐이고, 그래서 세는 대신 자리를 하나 만든다.
//!
//! 이 모듈을 통과한 순회는 하한을 **빠뜨릴 수 없다.** 하한은 인자이고, 실패 메시지는
//! 소비자가 아니라 여기가 만든다 — 하한을 내려서 통과시키지 말라는 금지도 여기 있다.
//! 소비자마다 다시 쓰면 언젠가 한 곳이 빠뜨리고, 빠뜨린 쪽은 조용하다.
//!
//! 하한을 값 하나로 받지 않는 이유는 [`Floor`] 에 적었다.

use std::path::{Path, PathBuf};

/// 순회가 모아야 할 최소량 — 그리고 **그 값이 무엇의 함수인지**.
///
/// 값 하나만 받으면 그 값은 낡는다. 낡은 하한은 두 방향으로 틀리는데 둘 다 조용하다:
/// 너무 낮으면 순회가 절반 죽어도 통과하고, 너무 높으면 정상적인 감소에 빨개져서
/// 사람이 하한을 내리는 습관을 들인다. 그래서 셋을 함께 받는다 — 마지막으로 **센**
/// 값, 그것을 **잰 날**, 그리고 하한을 그보다 낮게 잡은 **이유**.
///
/// 셋째가 핵심이다. 간격의 크기는 모수가 얼마나 빨리 움직이는가의 판단이고, 그 판단은
/// 자리마다 다르다 — 통합 타깃 수처럼 천천히 느는 모수는 여유가 좁아야 하고(여유가
/// 넓으면 술어가 절반 죽어도 통과한다), 크레이트 분해로 한 번에 크게 움직이는 모수는
/// 넓어야 한다. 그 판단을 안 적으면 다음 사람이 간격만 보고 아무 방향으로나 고친다.
pub struct Floor {
    /// 이 아래로 모이면 순회가 죽은 것으로 본다. 0 은 받지 않는다.
    pub min: usize,
    /// 마지막으로 실제로 센 값.
    pub measured: usize,
    /// 그 값을 잰 날 (`YYYY-MM-DD`).
    pub measured_on: &'static str,
    /// `min` 을 `measured` 보다 낮게 잡은 이유 — 이 모수가 무엇의 함수이고 얼마나 빨리
    /// 움직이는가.
    pub why_this_gap: &'static str,
}

/// 디렉토리를 내려갈지 정하는 방식.
///
/// 이름이 아니라 **성질**로 가른다. 빌드 산출물 디렉토리의 이름은 `CARGO_TARGET_DIR`
/// 하나로 무엇이든 될 수 있고, 이름 목록은 그것을 못 따라간다. cargo 가 만든
/// 디렉토리는 `CACHEDIR.TAG` 를 갖는다.
pub enum Descend {
    /// 전부 내려간다. 순회 루트 아래에 가지칠 것이 없다고 **판정한** 자리에 쓴다 —
    /// 그 판정 자체를 지키는 것은 이 모듈이 아니라 그 자리의 가드 몫이다.
    Everything,
    /// 빌드 캐시 디렉토리를 건너뛴다.
    SkipBuildCaches,
}

/// 순회가 찾은 파일 하나 — 절대 경로와 **정규화된** repo-relative 경로를 짝으로 낸다.
///
/// 짝으로 내는 이유가 있다. 소비자는 파일을 다시 열어야 해서 절대 경로가 필요하고,
/// 명부·면제 목록과 비교하려면 상대 경로가 필요하다. 하나만 내면 소비자가 나머지를
/// 자기 손으로 만들고, **그 "다시 만드는 자리" 가 없애려는 것 자체다.**
///
/// [`Walked::rel`] 의 구분자는 어느 플랫폼에서든 `/` 다. `strip_prefix` 의 결과는 그
/// 플랫폼 구분자를 그대로 물고 나오므로, 소비자가 그것을 펴서 소스에 박힌 `/` 리터럴과
/// 비교하면 Windows 에서 전부 빗나간다 — 그리고 **그 어긋남은 예외가 아니라 조용한 0** 이다.
/// 명부 조회가 모조리 실패하고 가드는 "명부에 없다" 고 보고한다.
///
/// 이 성질은 Linux 에서 잴 수 없다(여기서는 고치기 전에도 `/` 가 나온다). 그래서 Linux
/// 에서 도는 단정을 두지 않는다 — 대신 잴 수 있는 것을 잡는다: 소비자가 정규화를 자기
/// 손으로 하는 자리가 남아 있는지는 텍스트로 셀 수 있고,
/// `floored_walk_consumers_do_not_renormalize` 가 그것을 본다.
#[derive(Debug)]
pub struct Walked {
    /// 파일을 다시 열 때 쓴다.
    pub path: PathBuf,
    /// 비교에 쓴다. 구분자는 언제나 `/`.
    pub rel: String,
}

impl Floor {
    /// 선언 자체가 말이 되는지. 순회를 돌기 **전에** 본다 — 앞뒤가 안 맞는 하한으로
    /// 순회를 돌면 그 결과가 무엇을 뜻하는지 아무도 모른다.
    fn validate(&self) -> Result<(), String> {
        if self.min == 0 {
            return Err(
                "하한이 0 이다 — 아무것도 못 모아도 통과한다. 그것은 하한이 아니라 \
                 하한이 있다는 외양이다. 실제로 세서 그 절반쯤을 넣어라."
                    .to_string(),
            );
        }
        if self.min > self.measured {
            return Err(format!(
                "하한 {} 이 마지막 실측 {} 보다 크다 — 이 선언대로면 이 순회는 잰 그날에도 \
                 실패했어야 한다. 둘 중 하나가 낡았다: 다시 세서 `measured`·`measured_on` \
                 을 함께 갱신하거나, 하한이 과했던 것이면 이유와 함께 내려라.",
                self.min, self.measured
            ));
        }
        if self.measured_on.len() != 10 || self.measured_on.matches('-').count() != 2 {
            return Err(format!(
                "`measured_on` 이 `YYYY-MM-DD` 가 아니다: {:?}. 잰 날이 없으면 실측값은 \
                 언제 것인지 모르는 수가 되고, 그런 수는 갱신 대상으로 안 보인다.",
                self.measured_on
            ));
        }
        if self.why_this_gap.split_whitespace().count() < 10 {
            return Err(format!(
                "`why_this_gap` 이 너무 짧다({} 낱말) — 간격의 크기는 이 모수가 얼마나 빨리 \
                 움직이는가의 판단이고, 그 판단을 안 적으면 다음 사람이 간격만 보고 아무 \
                 방향으로나 고친다. 이 모수가 무엇의 함수인지, 무엇이 그것을 움직이는지 \
                 적어라. ★ 이 문턱을 내려서 통과시키지 마라.",
                self.why_this_gap.split_whitespace().count()
            ));
        }
        Ok(())
    }
}

/// `root` 아래를 재귀 순회해 `keep` 이 참인 파일을 모은다. 모인 수가 하한에 못 미치면
/// **결과 대신 이유를 돌려준다.**
///
/// 소비자는 이렇게 쓴다:
///
/// ```ignore
/// let files = walk_with_floor(&root.join("src"), &FLOOR, Descend::Everything, &|p| {
///     p.extension().is_some_and(|e| e == "rs")
/// })
/// .unwrap_or_else(|why| panic!("{why}"));
/// ```
///
/// `Result` 를 버리면 하한이 없는 것과 같아지므로 `#[must_use]` 를 붙였다.
#[must_use = "하한 미달은 이 Result 로만 나온다 — 버리면 순회가 죽어도 조용히 통과한다"]
pub fn walk_with_floor(
    root: &Path,
    rel_base: &Path,
    floor: &Floor,
    descend: Descend,
    keep: &dyn Fn(&Walked) -> bool,
) -> Result<Vec<Walked>, String> {
    floor.validate().map_err(|why| {
        format!(
            "순회 하한 선언이 앞뒤가 안 맞는다 ({}): {why}",
            root.display()
        )
    })?;

    let mut out = Vec::new();
    collect(root, rel_base, &descend, keep, &mut out);
    // `rel` 로 정렬한다 — 문자열이라 순서가 플랫폼에 안 흔들린다.
    out.sort_by(|a, b| a.rel.cmp(&b.rel));

    if out.len() < floor.min {
        return Err(format!(
            "{} 순회가 {} 개만 모았다(하한 {}) — 이 상태에서 뒤따르는 \"위반 0\" 은 위반이 \
             없다는 뜻이 아니라 아무것도 안 봤다는 뜻이다.\n\
             마지막 실측은 {} 의 {} 개였고, 하한을 그보다 낮게 잡은 이유는 이렇다: {}\n\
             ★ 하한을 내려서 통과시키지 마라 — 순회가 죽은 것을 그대로 승인하는 것이다.\n\
             순서가 있다. (1) 순회 루트와 가지치기와 `read_dir` 실패를 먼저 확인한다. \
             (2) 대상이 정말 줄어든 것이면 실제 수를 다시 세서 `measured`·`measured_on` 을 \
             함께 갱신한다. (3) 그러고 나서 `why_this_gap` 이 여전히 맞는지 다시 읽는다 — \
             모수가 움직이는 속도가 바뀌었으면 간격도 바뀐다.",
            root.display(),
            out.len(),
            floor.min,
            floor.measured_on,
            floor.measured,
            floor.why_this_gap,
        ));
    }
    Ok(out)
}

fn collect(
    dir: &Path,
    rel_base: &Path,
    descend: &Descend,
    keep: &dyn Fn(&Walked) -> bool,
    out: &mut Vec<Walked>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // **심볼릭 링크는 따라가지 않는다.** 따라가면 순회가 레포 밖 실제 경로로 새고,
        // 그 순간 모수가 작업 트리의 종류를 읽는다 — worktree 에는 레포 밖을 가리키는
        // 링크가 있고 보통의 clone 에는 없어서, 같은 커밋이 기계마다 다른 수를 낸다.
        // 실측 2026-09-06: 이 레포에서 따라가면 `.rs` 가 15 개 더 세어졌고, 그 15 는
        // 커밋되지 않는 로컬 작업 폴더의 것이라 어느 가드의 대상도 아니다.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            if matches!(descend, Descend::SkipBuildCaches) && crate::is_build_cache_dir(&path) {
                continue;
            }
            collect(&path, rel_base, descend, keep, out);
        } else {
            let found = Walked {
                rel: normalized_rel(&path, rel_base),
                path,
            };
            if keep(&found) {
                out.push(found);
            }
        }
    }
}

/// `rel_base` 기준 상대 경로를 만들고 구분자를 `/` 로 편다.
///
/// **정규화는 여기 한 곳에서만 한다.** 소비자마다 하면 언젠가 한 곳이 빠뜨리고, 빠뜨린
/// 쪽은 예외가 아니라 조용한 0 을 낸다 — 소스에 박힌 `/` 리터럴과의 비교가 모조리
/// 빗나가고, 가드는 "명부에 없다" 고 보고한다.
///
/// [`walk_with_floor`] 가 파일마다 이것을 부르지만, 디렉토리를 모으거나 경로 하나를
/// 다루는 자리는 순회를 안 거친다. 그런 자리도 자기 손으로 펴지 말고 이것을 불러라 —
/// 그래야 "정규화하는 자리" 가 저장소에 하나로 남고, 그 하나만 맞으면 전부 맞는다.
///
/// **재는 채널**: 이 함수가 옳은지는 `crossplatform-check` 의 `check-windows` 만 잰다
/// (Linux 에서는 고치기 전에도 `/` 가 나온다). 소비자가 이것을 안 부르고 자기 손으로
/// 펴는 자리가 남아 있는지는 `floored_walk_consumers_do_not_renormalize` 가 잰다.
pub fn normalized_rel(path: &Path, rel_base: &Path) -> String {
    path.strip_prefix(rel_base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이 판정들이 무엇에 반응하는지 보이려면 실제 트리가 필요하다. 소스에 심은 문자열로
    /// 대신하면 순회가 죽어도 통과한다 — 그것이 바로 이 모듈이 막으려는 형태다.
    struct Tree(PathBuf);

    impl Tree {
        fn new(files: &[&str], cache_dirs: &[&str]) -> Self {
            let base = std::env::temp_dir().join(format!(
                "tasty-floored-walk-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            for rel in files {
                let path = base.join(rel);
                std::fs::create_dir_all(path.parent().expect("파일에 부모가 없다")).unwrap();
                std::fs::write(&path, b"x").unwrap();
            }
            for rel in cache_dirs {
                let dir = base.join(rel);
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(
                    dir.join("CACHEDIR.TAG"),
                    b"Signature: 8a477f597d28d172789f06886806bc55",
                )
                .unwrap();
            }
            Self(base)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            // 정리 실패는 무시한다 — 판정은 이미 끝났고, 여기서 `unwrap` 을 쓰면 임시
            // 디렉토리 삭제 실패가 테스트의 빨강으로 둔갑한다. 남아도 임시 경로다.
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn floor(min: usize, measured: usize) -> Floor {
        Floor {
            min,
            measured,
            measured_on: "2026-09-06",
            why_this_gap: "시험용 고정값이다 — 이 모수는 테스트 안에서만 살고 아무것도 \
                           따라가지 않으므로 간격에 뜻이 없다",
        }
    }

    fn all(_: &Walked) -> bool {
        true
    }

    #[test]
    fn a_walk_that_meets_its_floor_returns_what_it_found() {
        let t = Tree::new(&["a.rs", "sub/b.rs", "sub/deep/c.rs"], &[]);
        let got = walk_with_floor(&t.0, &t.0, &floor(3, 3), Descend::Everything, &all)
            .expect("셋을 모았으면 통과해야 한다");
        assert_eq!(got.len(), 3, "재귀가 죽으면 얕은 자리만 모은다");
    }

    #[test]
    fn a_dead_walk_returns_why_instead_of_an_empty_list() {
        let t = Tree::new(&["a.rs"], &[]);
        let why = walk_with_floor(&t.0, &t.0, &floor(3, 10), Descend::Everything, &all)
            .expect_err("하한 미달인데 통과했다");
        assert!(
            why.contains("1 개만 모았다") && why.contains("하한 3"),
            "실패문이 실제로 몇 개를 모았는지 말하지 않는다: {why}"
        );
        assert!(
            why.contains("하한을 내려서 통과시키지 마라"),
            "금지가 빠졌다 — 이 문장이 없으면 다음 사람의 최소 수선은 하한을 내리는 것이다: {why}"
        );
    }

    #[test]
    fn the_keep_predicate_actually_filters() {
        let t = Tree::new(&["a.rs", "b.md", "c.md"], &[]);
        let got = walk_with_floor(&t.0, &t.0, &floor(1, 1), Descend::Everything, &|w| {
            w.rel.ends_with(".md")
        })
        .expect("둘을 모았으면 통과해야 한다");
        assert_eq!(
            got.len(),
            2,
            "`keep` 을 안 보고 전부 모으거나 아무것도 안 모은다"
        );
    }

    /// 가지치기는 **양방향으로** 재야 무정보가 아니다. 한쪽만 보면 "건너뛴다" 와
    /// "애초에 그 파일이 없다" 가 구별되지 않는다.
    #[test]
    fn build_caches_are_skipped_only_when_asked() {
        let t = Tree::new(&["keep.rs", "cache/inside.rs"], &["cache"]);
        let skipped = walk_with_floor(&t.0, &t.0, &floor(1, 1), Descend::SkipBuildCaches, &|w| {
            w.rel.ends_with(".rs")
        })
        .expect("바깥 파일 하나는 남는다");
        assert_eq!(skipped.len(), 1, "빌드 캐시 안을 들여다봤다");

        let everything = walk_with_floor(&t.0, &t.0, &floor(1, 1), Descend::Everything, &|w| {
            w.rel.ends_with(".rs")
        })
        .expect("둘 다 모인다");
        assert_eq!(
            everything.len(),
            2,
            "`Everything` 인데도 건너뛴다 — 그러면 위 검사의 초록은 가지치기의 증거가 아니다"
        );
    }

    /// 결과 순서는 소비자가 기대는 성질이다 — 실패문에 목록을 싣는 가드는 순서가 흔들리면
    /// 같은 결함을 매번 다른 모습으로 보고한다. 파일을 스무 개 만드는 것은 그 때문이다:
    /// `read_dir` 이 돌려주는 순서는 파일시스템이 정하고, 셋만 만들면 그것이 우연히
    /// 정렬 순서와 같아 정렬을 지워도 이 대조가 안 죽는다.
    #[test]
    fn the_walk_returns_a_sorted_list() {
        let names: Vec<String> = (0..20).rev().map(|i| format!("f{i:02}.rs")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let t = Tree::new(&refs, &[]);
        let got = walk_with_floor(&t.0, &t.0, &floor(20, 20), Descend::Everything, &all)
            .expect("스무 개를 모았으면 통과해야 한다");
        assert!(
            got.windows(2).all(|p| p[0].rel <= p[1].rel),
            "순회 결과가 정렬돼 있지 않다 — 순서를 파일시스템에 맡기면 같은 결함이 완주마다 \
             다른 순서로 보고된다: {:?}",
            got.iter().map(|w| &w.rel).collect::<Vec<_>>()
        );
    }

    /// 링크를 안 따라간다는 것은 **양방향으로** 재야 한다. 링크 너머 파일이 안 세어지는
    /// 것만 보면 "링크를 안 따라갔다" 와 "거기 파일이 없다" 가 구별되지 않는다.
    #[test]
    fn symlinks_are_not_followed_out_of_the_tree() {
        let outside = Tree::new(&["beyond.rs"], &[]);
        let inside = Tree::new(&["here.rs"], &[]);
        std::os::unix::fs::symlink(&outside.0, inside.0.join("link")).unwrap();

        let got = walk_with_floor(
            &inside.0,
            &inside.0,
            &floor(1, 1),
            Descend::Everything,
            &all,
        )
        .expect("트리 안의 파일 하나는 모인다");
        let rels: Vec<&str> = got.iter().map(|w| w.rel.as_str()).collect();
        assert_eq!(
            rels,
            vec!["here.rs"],
            "링크 너머로 순회가 샜다 — 그러면 모수가 작업 트리의 종류를 읽는다"
        );

        // 반대편: 링크가 아니라 진짜 디렉토리였으면 그 파일이 세어진다. 이것이 없으면
        // 위 초록은 "링크를 건너뛰었다" 가 아니라 "거기 아무것도 없었다" 일 수 있다.
        let plain = Tree::new(&["here.rs", "sub/beyond.rs"], &[]);
        let got = walk_with_floor(&plain.0, &plain.0, &floor(1, 1), Descend::Everything, &all)
            .expect("둘 다 모인다");
        assert_eq!(got.len(), 2, "평범한 하위 디렉토리까지 건너뛴다");
    }

    #[test]
    fn a_floor_of_zero_is_refused_before_the_walk_runs() {
        let t = Tree::new(&[], &[]);
        let why = walk_with_floor(&t.0, &t.0, &floor(0, 10), Descend::Everything, &all)
            .expect_err("하한 0 을 받아들였다");
        assert!(
            why.contains("하한이 0 이다"),
            "0 을 다른 이유로 거부했다: {why}"
        );
    }

    #[test]
    fn a_floor_above_its_own_measurement_is_refused() {
        let t = Tree::new(&["a.rs"], &[]);
        let why = walk_with_floor(&t.0, &t.0, &floor(11, 10), Descend::Everything, &all)
            .expect_err("실측보다 높은 하한을 받아들였다");
        assert!(why.contains("보다 크다"), "다른 이유로 거부했다: {why}");
    }

    #[test]
    fn an_undated_measurement_is_refused() {
        let t = Tree::new(&["a.rs"], &[]);
        let bad = Floor {
            measured_on: "얼마 전",
            ..floor(1, 10)
        };
        let why = walk_with_floor(&t.0, &t.0, &bad, Descend::Everything, &all)
            .expect_err("날짜 없는 실측을 받아들였다");
        assert!(why.contains("YYYY-MM-DD"), "다른 이유로 거부했다: {why}");
    }

    #[test]
    fn a_gap_without_a_reason_is_refused() {
        let t = Tree::new(&["a.rs"], &[]);
        let bad = Floor {
            why_this_gap: "적당히",
            ..floor(1, 10)
        };
        let why = walk_with_floor(&t.0, &t.0, &bad, Descend::Everything, &all)
            .expect_err("사유 없는 간격을 받아들였다");
        assert!(why.contains("너무 짧다"), "다른 이유로 거부했다: {why}");
    }
}
