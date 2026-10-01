//! The application resolves its installed strategies; execution receives only their fixed values.
#[derive(Clone, Debug)]
pub enum CompletionKind {
    Poll(tasty_agent::PollSpec),
    Push { notify_via: String, timeout_ms: u64 },
}
#[derive(Clone, Debug)]
pub struct CompletionStrategy {
    pub id: String,
    pub kind: CompletionKind,
}
pub trait CompletionResolver: Send + Sync {
    fn named(&self, id: &str) -> Result<CompletionStrategy, String>;
    fn default_for_method(&self, method: &str) -> Option<CompletionStrategy>;
}

#[cfg(test)]
pub(crate) mod fixture {
    use super::*;

    /// A fixed application answer. Tests exercise runner execution, not host registry installation.
    #[derive(Default)]
    pub(crate) struct Resolver {
        pub(crate) strategy: Option<CompletionStrategy>,
        pub(crate) default_method: Option<String>,
    }

    impl CompletionResolver for Resolver {
        fn named(&self, id: &str) -> Result<CompletionStrategy, String> {
            self.strategy
                .as_ref()
                .filter(|strategy| strategy.id == id)
                .cloned()
                .ok_or_else(|| format!("unknown completion strategy: {id}"))
        }

        fn default_for_method(&self, method: &str) -> Option<CompletionStrategy> {
            self.default_method
                .as_deref()
                .filter(|name| *name == method)
                .and(self.strategy.clone())
        }
    }
}
