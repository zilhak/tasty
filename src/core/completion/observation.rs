//! Passive reconciliation uses host observations; never probes a terminal with input.
use crate::core::CoreState;
impl CoreState {
    pub(crate) fn reconcile_completions(&self) {
        let result = (|| -> anyhow::Result<()> {
            self.publish_completion_ownership()?;
            let snapshot = self.completion.snapshot()?;
            let live = self.live_surface_ids();
            for binding in snapshot
                .bindings
                .values()
                .filter(|b| b.phase == "verified" && live.contains(&b.surface))
            {
                #[cfg(windows)]
                let quote = |s: &str| format!("\"{s}\"");
                #[cfg(not(windows))]
                let quote = |s: &str| format!("'{}'", s.replace('\'', "'\"'\"'"));
                let auth = binding
                    .auth_env
                    .as_deref()
                    .map(|s| format!(" --remote-auth-token-env {}", quote(s)))
                    .unwrap_or_default();
                let command = format!(
                    "codex --remote {}{auth} resume --dangerously-bypass-hook-trust {}",
                    quote(&binding.endpoint),
                    quote(&binding.thread_id)
                );
                let mut memory = self
                    .memory
                    .lock()
                    .map_err(|_| anyhow::anyhow!("metadata poisoned"))?;
                if crate::surface_meta::SurfaceMetaStore::get(
                    &mut *memory,
                    binding.surface,
                    "restore.command",
                )
                .as_deref()
                    != Some(&command)
                {
                    crate::surface_meta::SurfaceMetaStore::set(
                        &mut *memory,
                        binding.surface,
                        "restore.command",
                        &command,
                    )?;
                }
            }
            let targets: std::collections::HashSet<_> = snapshot
                .subscriptions
                .values()
                .filter(|s| s.active)
                .map(|s| s.child)
                .collect();
            for target in targets {
                if !live.contains(&target) {
                    // The journal spans windows; another CoreState may own this surface.
                    continue;
                }
                let observed = self.child_liveness_with_live(target, &live);
                if observed.state.as_str() == "stale" {
                    self.completion.observe(target,"stalled","passive_liveness","Completion hook missing; observed stalled state is not successful completion")?;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            tracing::warn!("completion reconciliation failed: {error}");
        }
    }
}
