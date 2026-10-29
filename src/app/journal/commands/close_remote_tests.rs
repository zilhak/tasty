//! A remote holder closes only the workspace it holds; other holds and other targets are refused.
use super::close::{Cause, Request};
use super::close_history_tests::{boot, request, workspace_with_terminals};
use super::*;

fn close_workspace(workspace: u32) -> JsonRpcRequest {
    request(
        "intent.close",
        serde_json::json!({
            "target": tasty_core::CloseTarget::Workspace(workspace),
            "capture": false,
            "user_close": false,
        }),
        1,
    )
}

fn refusal(result: Result<Request, JsonRpcResponse>) -> String {
    match result {
        Ok(_) => panic!("the close was admitted"),
        Err(response) => response.error.expect("refusal carries an error").message,
    }
}

/// The holder may close its workspace despite its own hard hold. Another client, another target and an
/// ordinary close of the held workspace are refused.
#[test]
fn a_remote_holder_closes_only_its_held_workspace() {
    let (mut session, mut journal) = boot();
    let (workspace, terminals) = workspace_with_terminals(&mut journal, &mut session, 1);
    let other = session
        .core_state
        .local_workspaces()
        .iter()
        .map(|workspace| workspace.id)
        .find(|id| *id != workspace)
        .expect("a second workspace");
    session
        .live
        .occupancy
        .acquire_workspace(workspace, &terminals, &terminals, 7)
        .unwrap();
    let held = Cause::RemoteHolder {
        client: 7,
        workspace,
    };

    let admitted = Request::resolve(&close_workspace(workspace), &session, held)
        .unwrap_or_else(|response| panic!("{response:?}"));
    assert_eq!(
        admitted.target,
        tasty_core::CloseTarget::Workspace(workspace)
    );
    assert!(!admitted.replacement, "another server workspace remains");
    assert_eq!(admitted.expected.len(), terminals.len());

    let stranger = Cause::RemoteHolder {
        client: 8,
        workspace,
    };
    assert!(
        refusal(Request::resolve(
            &close_workspace(workspace),
            &session,
            stranger
        ))
        .contains("holder changed")
    );
    assert!(
        refusal(Request::resolve(&close_workspace(other), &session, held))
            .contains("outside its held workspace")
    );
    assert!(
        refusal(Request::resolve(
            &close_workspace(workspace),
            &session,
            Cause::Ordinary
        ))
        .contains("hard-occupied")
    );
}

/// A terminal that another client holds inside the held workspace blocks the holder's close.
#[test]
fn another_clients_hard_hold_blocks_the_remote_close() {
    let (mut session, mut journal) = boot();
    let (workspace, terminals) = workspace_with_terminals(&mut journal, &mut session, 1);
    session
        .live
        .occupancy
        .acquire_workspace(workspace, &terminals[..1], &terminals, 7)
        .unwrap();
    session.live.occupancy.acquire(terminals[1], 8).unwrap();
    let held = Cause::RemoteHolder {
        client: 7,
        workspace,
    };
    assert!(
        refusal(Request::resolve(
            &close_workspace(workspace),
            &session,
            held
        ))
        .contains(&format!("surface {}", terminals[1]))
    );
}

/// Closing the server's last workspace from a remote holder keeps the server window with a replacement
/// workspace. A local user's close of the last workspace keeps its window-closing contract.
#[test]
fn a_remote_close_of_the_servers_last_workspace_gets_a_replacement() {
    let (mut session, _journal) = boot();
    let workspace = session.core_state.local_workspaces()[0].id;
    let terminals = session.core_state.local_workspaces()[0].all_surface_ids();
    session
        .live
        .occupancy
        .acquire_workspace(workspace, &terminals, &terminals, 7)
        .unwrap();
    let held = Cause::RemoteHolder {
        client: 7,
        workspace,
    };
    let admitted = Request::resolve(&close_workspace(workspace), &session, held)
        .unwrap_or_else(|response| panic!("{response:?}"));
    assert!(admitted.replacement);

    session.live.occupancy.force_detach_workspace(workspace);
    let local = Request::resolve(&close_workspace(workspace), &session, Cause::Ordinary)
        .unwrap_or_else(|response| panic!("{response:?}"));
    assert!(!local.replacement);
}
