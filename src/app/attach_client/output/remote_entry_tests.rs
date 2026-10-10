//! 원격 항목의 OS 숨김 표시가 탐색기의 숨김 판정까지 닿는지 본다.

use tasty_remote::client_session::RemoteDirEntry;

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
