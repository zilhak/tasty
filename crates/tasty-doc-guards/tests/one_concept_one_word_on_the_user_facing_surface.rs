//! 사용자가 읽는 표면에서 한 개념을 두 낱말로 부르지 않는다.
//!
//! # 무엇이 모수인가
//!
//! **사용자가 읽는 것뿐이다** — `lang/*.toml`(호스트 3 + 번들 plugin 27)과 `site/content`
//! 의 한국어 가이드 원본(18). `docs/` 와 `CHANGELOG.md` 는 뺀다. 개발자 산문이라서가
//! 아니라 **같은 잣대를 대면 정의문 자신이 위반이 되기 때문**이다 —
//! `docs/concepts/ubiquitous-language.md` 는 Window 를 "winit OS 창 자원" 으로 풀어 쓰는
//! 자리다. 그쪽까지 한 낱말로 모으려면 별개 결정이 필요하고, 그 결정은 아직 없다.
//!
//! # 왜 텍스트 가드인가
//!
//! 가드를 놓는 보통의 조건 — 재는 쪽과 재이는 쪽이 **같은 어휘를 쓴다** — 은 여기서
//! 성립하지 않는다. CLI 명령은 사용자가 그 낱말을 **친다**. 여기 낱말은 사용자가
//! **읽기만 한다.** 그래서 "사용자가 쓰는 그 형태로 잰다" 는 논증을 못 쓴다.
//!
//! 놓는 근거는 다른 쪽이다. 이 축에는 컴파일러가 없고(값이 toml 과 md 안의 자연어라
//! 타입이 안 붙는다), **가장 싼 초록화가 정본으로 고치는 것**이다 — 새 문자열에서 `창` 을 썼을 때
//! 이 가드를 통과시키는 길은 셋인데, ① 낱말을 `윈도우` 로 바꾸는 것(의도한 수정),
//! ② 아래 명부에 자리를 등록하며 **그것이 Window 가 아니라는 근거**를 적는 것(거짓이면
//! 근거 칸에 그대로 드러난다), ③ 그 문장을 지우는 것(문서가 나빠지므로 더 비싸다).
//! ①이 제일 싸므로 가드를 놓아도 보호 대상이 망가지는 쪽으로 흐르지 않는다.
//!
//! # 지키는 것
//!
//! 세 짝이다. `패인`→`페인`(Pane), `ウインドウ`→`ウィンドウ`(Window, 일본어),
//! `창`→`윈도우`(Window, 한국어). 앞의 둘은 표기가 하나로 정해져 예외가 없고, 마지막은
//! `창` 이 흔한 보통명사라 예외가 남는다 — 그 예외는 **부류가 아니라 자리**로 적는다.
//! 판정 기준은 `docs/concepts/ubiquitous-language.md` 의 Window / Popup 구분이다:
//! OS 창을 소유하는 것(= `View` 구현)만 `윈도우` 다.
//!
//! 일본어를 함께 넣은 이유: `ウインドウ` 는 `창` 과 달리 **다른 뜻으로 쓰이는 보통명사가
//! 아니다.** window 의 음차이고 갈린 것은 작은 `ィ` 하나라, 판정에 문맥이 필요 없다 —
//! `패인`/`페인` 과 같은 표기 축이고 그래서 예외가 없다. 갈린 자리가 `[menu.macos]` 절
//! 하나였다는 것은 **어떻게 흘렀는가**의 사실이지 바늘을 못 든다는 뜻이 아니고, 절 단위로
//! 흐른 것이야말로 파일 전체를 보는 바늘이 제일 잘 잡는 모양이다. 다만 그 절만 갈려 있던
//! 것은 macOS 일본어 메뉴 관례를 따라 적었기 때문으로 **보인다**(추정이지 측정이 아니다) —
//! 나중에 OS 관례를 따르기로 정하면 고칠 자리는 위 `RETIRED` 의 그 한 줄이고, 사유가
//! 그 자리에 붙어 있다.
//!
//! # 채널
//!
//! `doc-guards.yml` — main push · PR 마다 경로 필터 없이 돈다. 이 축을 재는 채널은 그 하나다.

use std::path::{Path, PathBuf};
use tasty_doc_guards::floored_walk::{Descend, Floor, walk_with_floor};

/// 은퇴한 표기와 그 정본.
struct Retired {
    /// 사용자 표면에 있으면 안 되는 표기.
    needle: &'static str,
    /// 대신 쓰는 정본.
    canonical: &'static str,
    /// 정본이 표면에 **실제로 살아 있는지** 보는 하한. 이게 없으면 모수가 통째로 비어도
    /// "은퇴 표기 0" 이 초록으로 나온다 — 지켜서 0 인 것과 아무것도 안 읽어서 0 인 것이
    /// 같은 모양이라, 금지 옆에 대조군을 함께 둔다.
    canonical_floor: usize,
    /// 그 하한을 그 값으로 잡은 근거와 마지막 실측.
    why: &'static str,
}

const RETIRED: &[Retired] = &[
    Retired {
        needle: "패인",
        canonical: "페인",
        canonical_floor: 40,
        why: "Pane 의 한국어 표기. 두 표기가 섞여 있었고 `페인` 으로 확정했다. 2026-09-07 \
              실측 86 — 가이드 장이 늘고 줄어도 절반 아래로는 안 간다.",
    },
    Retired {
        needle: "ウインドウ",
        canonical: "ウィンドウ",
        canonical_floor: 12,
        why: "Window 의 일본어 표기. `[menu.macos]` 세 줄만 갈려 있었다. 2026-09-07 실측 24 \
              — 일본어는 `lang/*/ja.toml` 계열뿐이라 모수가 좁고, 절반을 여유로 둔다.",
    },
];

/// `창` 의 정본이 살아 있는지 보는 대조군. `RETIRED` 와 달리 `창` 은 자리 예외가 있어
/// 구조가 다르므로 하한만 따로 든다.
const WINDOW_CANONICAL: (&str, usize) = ("윈도우", 60);

/// 한국어 `창` 은 위 둘과 다르다 — 흔한 보통명사라 Window 가 아닌 자리가 남는다.
///
/// 그 자리를 **부류로 적지 않는다.** "popup 이면 봐준다" 는 식으로 적으면 새 Window
/// 문구가 popup 을 자칭하며 빠져나간다. 자리마다 파일 · 그 자리를 집는 문구 · 몇 번
/// 나오는가 · **Window 가 아니라는 근거**를 적는다.
struct NotAWindow {
    path: &'static str,
    /// 그 자리를 집는 문구. 파일 안에서 정확히 `count` 번 나와야 한다.
    phrase: &'static str,
    count: usize,
    /// Window 가 아니라는 근거. 부류가 아니라 **확인한 것**을 적는다.
    evidence: &'static str,
}

const NOT_A_WINDOW: &[NotAWindow] = &[
    NotAWindow {
        path: "crates/tasty-plugin-clipboard-viewer/lang/ko.toml",
        phrase: "스냅샷 창은",
        count: 1,
        evidence: "매니페스트 `crates/tasty-plugin-clipboard-viewer/tasty-plugin.toml` 이 \
                   `[[contributes.popup]]` 로 선언한다",
    },
    NotAWindow {
        path: "crates/tasty-plugin-clipboard-viewer/lang/ko.toml",
        phrase: "기존 창을 앞으로",
        count: 1,
        evidence: "같은 popup 을 한 문장 안에서 두 번째로 가리킨다 — 근거는 위와 같은 매니페스트다",
    },
    NotAWindow {
        path: "crates/tasty-plugin-git-viewer/lang/ko.toml",
        phrase: "기존 창을 앞으로",
        count: 1,
        evidence: "매니페스트 `crates/tasty-plugin-git-viewer/tasty-plugin.toml` 이 \
                   `[[contributes.popup]]` 로 선언한다",
    },
    NotAWindow {
        path: "site/content/agents/tasks.md",
        phrase: "Task DAGs --> 창",
        count: 1,
        evidence: "DAG **목록**이다 — `lang/ko.toml` 의 `toggle_dag_list_label` 이 \
                   \"DAG 목록 열기/닫기\" 로 여닫는 패널이고, 같은 문단이 이것과 \"DAG 탭\" 을 \
                   나란히 놓는다",
    },
    NotAWindow {
        path: "site/content/customize/scripts.md",
        phrase: "로그 창 하나",
        count: 1,
        evidence: "Window 도 popup 도 아니다 — 워크스페이스 안 오른쪽에 붙이는 것이라 \
                   서피스/페인 쪽이 맞지만, 어느 낱말로 부를지가 아직 안 정해졌다",
    },
    NotAWindow {
        path: "site/content/customize/scripts.md",
        phrase: "확인 창이 뜹니다",
        count: 1,
        evidence: "스크립트 변경 확인 — host `PopupDef` id `script_changed_confirm`",
    },
    NotAWindow {
        path: "site/content/plugins/index.md",
        phrase: "확인 창이 뜹니다",
        count: 1,
        evidence: "플러그인 서명 확인 — 플러그인 윈도우(`PluginsView`) **안에서** 뜨는 확인이지 \
                   자기 OS 창을 갖는 `View` 가 아니다",
    },
    NotAWindow {
        path: "site/content/using/files.md",
        phrase: "주소창",
        count: 3,
        evidence: "낱말이 다르다 — 탐색기의 address bar 다. Window 를 가리키지 않는다",
    },
    NotAWindow {
        path: "site/content/using/files.md",
        phrase: "파일 선택 창",
        count: 1,
        evidence: "파일 선택 대화상자 — `View` 구현은 다섯(Main·Settings·Plugins·Preset·Quit)뿐이고 \
                   여기 없다",
    },
    NotAWindow {
        path: "site/content/using/files.md",
        phrase: "Choose file handler --> 창이",
        count: 1,
        evidence: "핸들러 선택 대화상자 — 메인 윈도우 안에서 뜬다",
    },
    NotAWindow {
        path: "site/content/using/files.md",
        phrase: "Open Markdown File --> 창을",
        count: 1,
        evidence: "마크다운 열기 대화상자 — 메인 윈도우 안에서 뜬다",
    },
    NotAWindow {
        path: "site/content/using/files.md",
        phrase: "Open HTML --> 창에",
        count: 1,
        evidence: "HTML 열기 대화상자 — 메인 윈도우 안에서 뜬다",
    },
    NotAWindow {
        path: "site/content/using/files.md",
        phrase: "보여주는 창이 뜹니다",
        count: 1,
        evidence: "Git 뷰어 — 매니페스트가 `[[contributes.popup]]` 로 선언하고 설명도 \
                   \"git status / log / diff popup\" 이다",
    },
    NotAWindow {
        path: "site/content/using/files.md",
        phrase: "이 창과 별개로",
        count: 1,
        evidence: "바로 위 문장이 가리킨 그 Git 뷰어 popup 을 되받는다",
    },
    NotAWindow {
        path: "site/content/using/panes-tabs-splits.md",
        phrase: "알림 창 타이틀바",
        count: 1,
        evidence: "알림 목록 — host `PopupDef` id `notifications`. 같은 줄의 \"Tasty 윈도우\" 와 \
                   \"윈도우 크기\" 는 진짜 Window 라 이미 바뀌었다",
    },
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/tasty-doc-guards 위로 두 단계가 레포 루트다")
        .to_path_buf()
}

/// 호스트 `lang/` 세 파일. 여기가 비면 사용자 문자열을 한 줄도 안 본 것이다.
const HOST_LANG_FLOOR: Floor = Floor {
    min: 3,
    measured: 3,
    measured_on: "2026-09-07",
    why_this_gap: "이 모수는 호스트가 출하하는 언어 수다(en · ko · ja). 언어를 늘리는 것은 \
                   드물고 줄이는 것은 더 드물어서 여유를 두지 않았다 — 여기서 하나라도 \
                   빠지면 그것은 정상적인 감소가 아니라 순회 루트가 어긋난 것이다.",
};

/// 번들 plugin 의 `lang/` 파일. plugin 수 × 언어 수다.
const PLUGIN_LANG_FLOOR: Floor = Floor {
    min: 15,
    measured: 27,
    measured_on: "2026-09-07",
    why_this_gap: "이 모수는 번들 plugin 수 곱하기 언어 수다(2026-09-07 기준 9 × 3). plugin 은 \
                   늘기도 하고 라이브러리로 접히기도 해서 한 번에 세 파일씩 움직인다 — 그래서 \
                   plugin 넷이 사라져도 견디게 잡았다. 15 아래는 plugin 이 준 게 아니라 \
                   `crates` 순회나 걸러내기가 어긋난 것이다.",
};

/// 한국어 가이드 원본.
const GUIDE_FLOOR: Floor = Floor {
    min: 12,
    measured: 18,
    measured_on: "2026-09-07",
    why_this_gap: "이 모수는 `site/content` 의 한국어 원본 `.md` 수다(번역 `en/` 제외). 가이드 \
                   장은 합쳐지고 갈리므로 몇 개는 움직이지만, 12 아래는 장이 줄어든 게 아니라 \
                   순회 루트나 `en/` 가지치기가 어긋난 것이다.",
};

/// 사용자가 읽는 파일 전부 — repo-relative 경로와 본문의 짝.
fn user_facing_files() -> Vec<(String, String)> {
    let root = repo_root();
    let mut out = Vec::new();

    let mut take = |dir: PathBuf, floor: &Floor, descend: Descend, keep: &dyn Fn(&str) -> bool| {
        let walked = walk_with_floor(&dir, &root, floor, descend, &|w| keep(&w.rel))
            .unwrap_or_else(|why| panic!("{why}"));
        for w in walked {
            // 읽기 실패를 건너뛰면 순회 하한을 통과한 채 본문만 비는 길이 남는다 — 하한이
            // 세는 것은 **찾은** 파일이지 **읽은** 파일이 아니다.
            let text = std::fs::read_to_string(&w.path)
                .unwrap_or_else(|e| panic!("사용자 표면 파일을 읽지 못했다: {} — {e}", w.rel));
            out.push((w.rel, text));
        }
    };

    take(
        root.join("lang"),
        &HOST_LANG_FLOOR,
        Descend::Everything,
        &|rel| rel.ends_with(".toml"),
    );
    take(
        root.join("crates"),
        &PLUGIN_LANG_FLOOR,
        Descend::SkipBuildCaches,
        &|rel| {
            // plugin 이 출하하는 번역만. 크레이트 소스는 사용자가 읽는 것이 아니다.
            rel.contains("/lang/") && rel.ends_with(".toml")
        },
    );
    take(
        root.join("site/content"),
        &GUIDE_FLOOR,
        Descend::Everything,
        &|rel| rel.ends_with(".md") && !rel.starts_with("site/content/en/"),
    );

    out
}

fn count_of(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn no_retired_spelling_survives_on_the_user_facing_surface() {
    let files = user_facing_files();
    for r in RETIRED {
        let mut offenders = Vec::new();
        for (rel, text) in &files {
            let n = count_of(text, r.needle);
            if n > 0 {
                offenders.push(format!("  {rel} — {n} 회"));
            }
        }
        assert!(
            offenders.is_empty(),
            "사용자 표면에 은퇴한 표기 `{}` 가 남아 있다. 정본은 `{}` 다.\n{}\n\
             ★ 명부에 자리를 더해 통과시키지 마라 — 이 짝에는 예외가 없다. 낱말을 고쳐라.",
            r.needle,
            r.canonical,
            offenders.join("\n")
        );
    }
}

#[test]
fn each_canonical_word_is_still_present() {
    let files = user_facing_files();
    let total = |needle: &str| -> usize { files.iter().map(|(_, t)| count_of(t, needle)).sum() };

    for r in RETIRED {
        let n = total(r.canonical);
        assert!(
            n >= r.canonical_floor,
            "정본 `{}` 이 사용자 표면에 {} 번밖에 없다(하한 {}). 금지만 보면 모수가 통째로 \
             비어도 초록이라, 정본이 살아 있는지를 함께 본다.\n근거: {}",
            r.canonical,
            n,
            r.canonical_floor,
            r.why
        );
    }

    let (word, floor) = WINDOW_CANONICAL;
    let n = total(word);
    assert!(
        n >= floor,
        "정본 `{word}` 이 사용자 표면에 {n} 번밖에 없다(하한 {floor}). 2026-09-07 실측 134 — \
         가이드와 `lang` 양쪽에 퍼져 있어 한쪽이 통째로 안 읽혀도 절반은 남는다."
    );
}

#[test]
fn every_registration_occurs_exactly_as_many_times_as_declared() {
    let files = user_facing_files();
    for e in NOT_A_WINDOW {
        let text = files
            .iter()
            .find(|(rel, _)| rel == e.path)
            .map(|(_, t)| t)
            .unwrap_or_else(|| {
                panic!(
                    "명부가 가리키는 `{}` 가 모수에 없다. 파일이 옮겨졌거나 지워졌으면 \
                     명부에서도 지워라 — 안 지우면 그 줄은 아무것도 안 지키면서 남는다.",
                    e.path
                )
            });
        let n = count_of(text, e.phrase);
        assert_eq!(
            n, e.count,
            "`{}` 의 `{}` 가 {} 번 나온다(명부는 {} 번). 자리가 움직였으면 명부를 갱신해라 — \
             수가 늘었으면 새 자리가 근거 없이 들어온 것이고, 줄었으면 명부가 안 지키는 줄을 \
             들고 있는 것이다.",
            e.path, e.phrase, n, e.count
        );
    }
}

#[test]
fn every_remaining_window_word_is_registered_as_not_a_window() {
    let files = user_facing_files();
    for (rel, text) in &files {
        let found = count_of(text, "창");
        let registered: usize = NOT_A_WINDOW
            .iter()
            .filter(|e| e.path == rel)
            .map(|e| count_of(text, e.phrase) * count_of(e.phrase, "창"))
            .sum();
        assert_eq!(
            found, registered,
            "`{rel}` 에 `창` 이 {found} 번 있는데 명부가 덮는 것은 {registered} 번이다.\n\
             ★ Window 를 가리키는 자리면 `윈도우` 로 고쳐라. Window 가 아니면 \
             `NOT_A_WINDOW` 에 자리와 **근거**를 적어라 — 근거는 부류가 아니라 확인한 것이다\
             (매니페스트의 `[[contributes.popup]]`, host `PopupDef` id, `View` 구현 목록 따위)."
        );
    }
}

#[test]
fn registrations_in_the_same_file_do_not_share_evidence() {
    // 파일이 다르면 같은 근거가 정당하다 — 두 plugin 이 같은 이유로 popup 인 것은 베끼기가
    // 아니다. 같은 파일 안에서 근거가 겹치면 그때는 자리를 안 보고 앞 줄을 복사한 것이다.
    for (i, a) in NOT_A_WINDOW.iter().enumerate() {
        for b in NOT_A_WINDOW.iter().skip(i + 1) {
            assert!(
                a.path != b.path || a.evidence != b.evidence,
                "`{}` 안의 두 자리(`{}` · `{}`)가 근거 문장을 그대로 공유한다. 자리가 다르면 \
                 무엇을 확인했는지도 다르다 — 앞 줄을 복사하지 말고 그 자리를 보고 적어라.",
                a.path,
                a.phrase,
                b.phrase
            );
        }
    }
}

#[test]
fn the_population_holds_both_kinds_of_user_facing_file() {
    // 모수가 한쪽으로 쏠리면 위 단정들은 남은 쪽만 보고도 전부 초록이다. 두 종류가 모두
    // 들어왔는지를 여기서 따로 본다 — `walk_with_floor` 의 하한은 각 순회 안에서만 센다.
    let files = user_facing_files();
    let langs = files.iter().filter(|(r, _)| r.ends_with(".toml")).count();
    let guides = files.iter().filter(|(r, _)| r.ends_with(".md")).count();
    assert!(
        langs >= 18 && guides >= 12,
        "모수가 한쪽으로 쏠렸다 — lang {langs} · 가이드 {guides}. 2026-09-07 실측 30 · 18."
    );
}
