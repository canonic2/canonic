//! Optional operation stages; observers are request-owned and carry no authority.
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy)]
pub(crate) enum Stage {
    DiscoverRegistry,
    Catalog,
    History,
    Artifact,
    PreparePolicy,
    ComparePolicy,
    PrepareReview,
}

impl Stage {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::DiscoverRegistry => "Discovering registry identity and signing key",
            Self::Catalog => "Reading and verifying registry catalog",
            Self::History => "Reading and verifying pack release history",
            Self::Artifact => "Reading and verifying pack artifact",
            Self::PreparePolicy => "Preparing captured policy inputs",
            Self::ComparePolicy => "Comparing policy permissions and coverage",
            Self::PrepareReview => "Preparing inactive policy review",
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Progress(Option<Arc<Observer>>);

struct Observer {
    sequence: Arc<Mutex<u32>>,
    callback: Box<dyn Fn(u32, Stage) + Send + Sync>,
}

impl Progress {
    pub(crate) fn new(observer: impl Fn(u32, Stage) + Send + Sync + 'static) -> Self {
        Self::default().rebind(observer)
    }

    /// A resumed legacy RPC has fresh control, but retains its monotonic count.
    pub(crate) fn rebind(&self, observer: impl Fn(u32, Stage) + Send + Sync + 'static) -> Self {
        Self(Some(Arc::new(Observer {
            sequence: self
                .0
                .as_ref()
                .map_or_else(|| Arc::new(Mutex::new(0)), |old| old.sequence.clone()),
            callback: Box::new(observer),
        })))
    }

    pub(super) fn stage(&self, stage: Stage) {
        if let Some(observer) = &self.0 {
            let Ok(mut number) = observer.sequence.lock() else {
                return;
            };
            if *number == 64 {
                return;
            }
            *number += 1;
            (observer.callback)(*number, stage);
        }
    }
}

#[cfg(test)]
mod tests;
