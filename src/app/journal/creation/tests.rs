use super::*;
use crate::runtime::journal_product::EffectLease;
use crate::runtime::resource_retirement::EngineRelease;

fn lease() -> EffectLease {
    EffectLease {
        effect_id: "retained-effect".into(),
        operation: OperationId("retained-operation".into()),
        stream: "structure:slot-1".into(),
        runtime_epoch: 3,
        resource_generation: 4,
        attempt: 1,
    }
}

#[test]
fn uncertain_installation_release_waits_for_its_original_remote_receipt() {
    let binding = crate::plugin_bridge::host_cmd::MeshBinding::default();
    let (receipt, completion) =
        crate::plugin_bridge::host_cmd::RemoteRetirementReceipt::pending(1, binding.binding());
    let stage = Stage::Installing {
        installed: Installed::with_test_retirement(lease(), receipt),
        answered: true,
        started: std::time::Instant::now(),
    };
    let owner = UncertainOwner::from_stage(stage).expect("installation owner");
    assert!(matches!(&owner, UncertainOwner::Installed(installed) if installed.lease == lease()));
    let mut release = EngineRelease::default();
    owner.retain_for_release(&mut release);
    let mut session = EngineSession::new(80, 24, Arc::new(|| {})).unwrap();
    assert!(
        !release.poll(&mut session, None),
        "enqueue or owner transfer is not a destroy ACK"
    );
    completion.finish(Ok(()));
    assert!(release.poll(&mut session, None));
}

#[cfg(unix)]
#[test]
fn uncertain_discard_release_retains_the_exact_pty_generation() {
    let (terminal, pty) = tasty_terminal::spawn_terminal(
        tasty_terminal::TerminalConfig {
            cols: 80,
            rows: 24,
            shell: Some("/bin/sh"),
            args: &["-c", "exit 0"],
            surface_id: 1,
            working_dir: None,
            initial_input: None,
            extra_env: &[],
        },
        Arc::new(|| {}),
    )
    .unwrap();
    let generation = pty.generation();
    let retirement = pty.retire();
    drop(terminal);
    let owner = UncertainOwner::from_stage(Stage::Rejected {
        lease: lease(),
        retirement: Some(retirement),
        discard_committed: true,
        reason: "await original reap".into(),
        answered: true,
        started: std::time::Instant::now(),
    })
    .expect("discard owner");
    assert!(
        matches!(&owner, UncertainOwner::Discard { lease: actual, retirement: Some(receipt), discard_committed: true, reason } if *actual == lease() && receipt.generation() == generation && reason == "await original reap")
    );
    let mut release = EngineRelease::default();
    owner.retain_for_release(&mut release);
    assert_eq!(release.retained_pty_generations(), vec![generation]);
    let mut session = EngineSession::new(80, 24, Arc::new(|| {})).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !release.poll(&mut session, None) {
        assert!(
            std::time::Instant::now() < deadline,
            "exact child did not reap"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
