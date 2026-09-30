//! A kind instance can be prepared before publishing its external registration.

use crate::model::Surface;

pub type PublicationAction = Box<dyn FnOnce() -> anyhow::Result<()> + Send>;

pub struct PreparedKind {
    pub surface: Box<dyn Surface>,
    pub publication: Option<PublicationAction>,
}

impl PreparedKind {
    pub fn local(surface: Box<dyn Surface>) -> Self {
        Self {
            surface,
            publication: None,
        }
    }

    pub fn deferred(surface: Box<dyn Surface>, publication: PublicationAction) -> Self {
        Self {
            surface,
            publication: Some(publication),
        }
    }

    /// Used by immediate materialization callers. Journal execution separates these two parts.
    pub fn publish(self) -> anyhow::Result<Box<dyn Surface>> {
        if let Some(publication) = self.publication {
            publication()?;
        }
        Ok(self.surface)
    }
}
