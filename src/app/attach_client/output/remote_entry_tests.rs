//! 원격 항목의 OS 숨김 표시와 링크 상태가 탐색기의 판정까지 닿는지 본다.

use tasty_remote::client_session::{RemoteDirEntry, RemoteLink};

use super::remote_entry::from_remote;
use crate::explorer_ui::view::hidden::is_hidden;

fn remote(name: &str, os_hidden: bool) -> RemoteDirEntry {
    RemoteDirEntry {
        name: name.into(),
        is_dir: false,
        size: 0,
        modified: None,
        ext: String::new(),
        os_hidden,
        link: RemoteLink::NotALink,
    }
}

#[test]
fn a_remote_os_hidden_mark_reaches_the_hidden_rule() {
    let entries: Vec<_> = [
        remote("desktop.ini", true),
        remote("notes.txt", false),
        remote(".env", false),
    ]
    .into_iter()
    .map(|entry| from_remote(Some("C:/Users/me"), entry))
    .collect();
    let hidden: Vec<bool> = entries.iter().map(is_hidden).collect();
    assert_eq!(hidden, [true, false, true]);
    assert_eq!(
        entries[0].path,
        std::path::Path::new("C:/Users/me").join("desktop.ini")
    );
}

/// 새 server 가 실은 링크 상태로 원격 목록이 로컬과 같이 끊긴 링크를 알아본다. 열기는 이 판정으로 원인을
/// 알리고(`explorer.state.broken_link`), 폴더 링크는 폴더로 트리에도 들어간다.
#[test]
fn a_remote_link_state_reaches_the_explorer_listing() {
    use crate::core::fs_list::EntryLink;
    use crate::explorer_ui::view::ExplorerView;
    let linked = |name: &str, is_dir, link| RemoteDirEntry {
        is_dir,
        link,
        ..remote(name, false)
    };
    let entries: Vec<_> = [
        linked("docs", true, RemoteLink::Valid),
        linked("notes.md", false, RemoteLink::Valid),
        linked("gone", false, RemoteLink::Broken),
        remote("plain.txt", false),
    ]
    .into_iter()
    .map(|entry| from_remote(Some("/srv"), entry))
    .collect();
    assert_eq!(
        entries.iter().map(|e| e.link).collect::<Vec<_>>(),
        [
            EntryLink::Valid,
            EntryLink::Valid,
            EntryLink::Broken,
            EntryLink::NotALink
        ]
    );

    let panel = tasty_model::ExplorerPanel::new(1, std::path::PathBuf::from("/srv"));
    let mut view = ExplorerView::new();
    view.sync(&panel, Some(7));
    let request_id = view.drain_outbox().pop().expect("list request").request_id;
    assert!(view.apply_remote_list_dir_result(request_id, &panel, Ok(entries)));
    let srv = std::path::Path::new("/srv");
    assert!(view.is_broken_link(&srv.join("gone")));
    assert!(!view.is_broken_link(&srv.join("notes.md")));
    assert!(!view.is_broken_link(&srv.join("plain.txt")));
    let tree: Vec<String> = view
        .tree_children_of(srv, Some(7))
        .iter()
        .map(|e| e.name.clone())
        .collect();
    assert_eq!(tree, ["docs"]);
}

/// 옛 server 의 항목(링크 상태 없음)은 끊긴 링크로 보지 않는다. 지금처럼 일반 파일로 열린다.
#[test]
fn an_old_server_entry_is_never_a_broken_link() {
    let entry = from_remote(Some("/srv"), remote("gone", false));
    assert_eq!(entry.link, crate::core::fs_list::EntryLink::NotALink);
}
