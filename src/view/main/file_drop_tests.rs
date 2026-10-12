//! OS 파일 드롭 분기 시험. hover 가 정한 explorer 칸, 좌표 없음, 터미널 영역 밖, 좌표 아래 칸 순서와
//! explorer 칸에 놓을 때의 작업을 본다.

use super::*;
use crate::app::explorer_files::job::OpKind;

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[test]
fn the_folder_chosen_while_hovering_wins_over_the_cursor() {
    for cursor in [None, Some(((5.0, 5.0), false)), Some(((5.0, 5.0), true))] {
        assert_eq!(
            route(Some((7, p("/dest"), Verdict::Go(OpKind::Copy))), cursor),
            Route::Hovered(7, p("/dest"), Verdict::Go(OpKind::Copy))
        );
    }
    let refused = Verdict::No("explorer.drag.no_write");
    assert_eq!(
        route(Some((7, p("/dest"), refused)), None),
        Route::Hovered(7, p("/dest"), refused)
    );
}

#[test]
fn without_a_hovered_folder_the_cursor_decides() {
    assert_eq!(route(None, None), Route::NoCursor);
    assert_eq!(route(None, Some(((5.0, 6.0), false))), Route::Outside);
    assert_eq!(
        route(None, Some(((5.0, 6.0), true))),
        Route::AtCursor(5.0, 6.0)
    );
}

/// OS 에서 온 파일은 판정이 이동이어도 복사한다. 원본을 지우지 않는다.
#[test]
fn files_from_the_os_are_always_copied() {
    let paths = vec![p("/os/a.txt"), p("/os/b")];
    for decided in [Verdict::Go(OpKind::Copy), Verdict::Go(OpKind::Move)] {
        let Ok(Operation::Paste {
            paths: sent,
            destination,
            cut,
        }) = explorer_drop(decided, p("/dest"), paths.clone())
        else {
            panic!("{decided:?} must paste");
        };
        assert_eq!((sent, destination, cut), (paths.clone(), p("/dest"), false));
    }
}

#[test]
fn a_refused_folder_explains_why() {
    let reason = "explorer.drag.no_write";
    let Err(text) = explorer_drop(Verdict::No(reason), p("/dest"), vec![p("/os/a.txt")]) else {
        panic!("a refused drop must not paste");
    };
    assert_eq!(
        text,
        format!(
            "{}{}",
            crate::i18n::t("explorer.drag.refused"),
            crate::i18n::t_fmt("explorer.drag.refused_reason", crate::i18n::t(reason))
        )
    );
    assert!(text.contains(crate::i18n::t(reason)));
}
