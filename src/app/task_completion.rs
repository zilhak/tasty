//! Application strategy registry and explicit target adapters for the independent task runtime.
use tasty_task_runtime::completion::{CompletionKind, CompletionResolver, CompletionStrategy};

pub(crate) struct AppCompletionResolver;
impl CompletionResolver for AppCompletionResolver {
    fn named(&self, id: &str) -> Result<CompletionStrategy, String> {
        crate::completion_strategy::global()
            .resolve_strategy(&crate::completion_strategy::CompletionStrategyId::new(id))
            .map(convert)
            .map_err(|error| error.to_string())
    }
    fn default_for_method(&self, method: &str) -> Option<CompletionStrategy> {
        crate::completion_strategy::global()
            .resolve_default_for_method(method)
            .map(convert)
    }
}
fn convert(strategy: crate::completion_strategy::types::CompletionStrategy) -> CompletionStrategy {
    CompletionStrategy {
        id: strategy.id.as_str().to_owned(),
        kind: match strategy.kind {
            crate::completion_strategy::CompletionStrategyKind::Poll(spec) => {
                CompletionKind::Poll(spec)
            }
            crate::completion_strategy::CompletionStrategyKind::Push {
                notify_via,
                timeout_ms,
            } => CompletionKind::Push {
                notify_via: notify_via.as_str().to_owned(),
                timeout_ms,
            },
        },
    }
}

/// A caller fixes visible/local workspace IDs before entering task execution. The runtime does
/// not own a Core tree or derive targets from a focused View.
pub(crate) fn dag_workspaces(
    ids: impl IntoIterator<Item = u32>,
    requested: Option<u32>,
) -> Vec<u32> {
    if let Some(id) = requested {
        return vec![id];
    }
    let mut ids: Vec<_> = ids.into_iter().collect();
    ids.sort_unstable();
    ids
}
