//! `workspace.overview` 응답의 wire 형식 시험. 기대값의 키는 글자로 적어 이름 변경을 잡는다.

use super::{OverviewSurface, OverviewWorkspace, WorkspaceOverview};
use serde_json::json;

fn example() -> serde_json::Value {
    json!({
        "workspaces": [
            {
                "id": 1, "name": "main", "subtitle": "~/src", "busy_count": 1, "mirror": false,
                "ws_holder": 7,
                "surfaces": [
                    {"id": 10, "kind": "terminal", "title": "zsh", "busy": true, "holder": 7, "granted_seq": 3},
                    {"id": 11, "kind": "markdown", "title": "README.md", "busy": false},
                ],
            },
            {
                "id": 2, "name": "remote", "subtitle": "", "busy_count": 0, "mirror": true,
                "surfaces": [],
            },
        ],
        "capabilities": ["ipc.stream.resize-ack"],
    })
}

#[test]
fn an_overview_reads_and_writes_the_example_unchanged() {
    let overview: WorkspaceOverview =
        serde_json::from_value(example()).expect("example overview parses");
    assert_eq!(serde_json::to_value(&overview).unwrap(), example());
}

#[test]
fn absent_holders_read_as_none_and_are_not_written() {
    let overview: WorkspaceOverview = serde_json::from_value(example()).unwrap();
    let remote = &overview.workspaces[1];
    assert_eq!(remote.ws_holder, None);
    let markdown = &overview.workspaces[0].surfaces[1];
    assert_eq!((markdown.holder, markdown.granted_seq), (None, None));
    let written = serde_json::to_value(remote).unwrap();
    assert!(written.get("ws_holder").is_none(), "{written}");
}

#[test]
fn an_overview_built_in_code_has_the_wire_keys() {
    let overview = WorkspaceOverview {
        workspaces: vec![OverviewWorkspace {
            id: 1,
            name: "main".to_string(),
            subtitle: "~/src".to_string(),
            busy_count: 1,
            mirror: false,
            ws_holder: Some(7),
            surfaces: vec![OverviewSurface {
                id: 10,
                kind: "terminal".to_string(),
                title: "zsh".to_string(),
                busy: true,
                holder: Some(7),
                granted_seq: Some(3),
            }],
        }],
        capabilities: vec!["ipc.stream.resize-ack".to_string()],
    };
    let mut expected = example();
    expected["workspaces"].as_array_mut().unwrap().truncate(1);
    expected["workspaces"][0]["surfaces"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    assert_eq!(serde_json::to_value(&overview).unwrap(), expected);
}
