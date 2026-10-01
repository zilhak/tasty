//! Structural request origin mapping is independent of the caller transport.
use crate::core::origin::{AgentSource, IntentOrigin};
use crate::ipc::caller::CallerContext;
use std::sync::Arc;
use tasty_plugin_manifest::Permission;

fn callers() -> [CallerContext; 3] {
    let permissions: Arc<std::collections::HashSet<Permission>> =
        Arc::new([Permission::SurfaceWrite].into_iter().collect());
    [
        CallerContext::Local,
        CallerContext::Agent {
            agent_id: "child:1".into(),
            permissions: permissions.clone(),
        },
        CallerContext::Plugin {
            plugin_id: "origin-probe".into(),
            permissions,
        },
    ]
}

#[test]
fn caller_contexts_map_to_agent_origins() {
    let [local, agent, plugin] = callers();
    for caller in [local, agent] {
        assert!(
            matches!(
                super::intent_origin_of(&caller),
                IntentOrigin::Agent {
                    source: AgentSource::Ipc
                }
            ),
            "{caller:?}"
        );
    }
    assert!(matches!(
        super::intent_origin_of(&plugin),
        IntentOrigin::Agent { source: AgentSource::Plugin(id) } if id == "origin-probe"
    ));
}
