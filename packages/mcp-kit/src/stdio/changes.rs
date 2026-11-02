//! A bounded, connection-owned producer queue that wakes the dispatcher.
use crate::Change;
use std::{sync::mpsc, thread};

/// Changes buffered for one connection before publication must apply backpressure.
pub const CHANGE_BUFFER: usize = 16;

/// A nonblocking producer for the connection supplied to `Handler::connected`.
#[derive(Debug, Clone)]
pub struct ChangeSender {
    sender: mpsc::SyncSender<Change>,
    dispatcher: thread::Thread,
    prompts: bool,
    resources: bool,
}

/// Publication refused before enqueueing. Each variant retains the unsent change.
#[derive(Debug, thiserror::Error)]
pub enum SendChangeError {
    /// The producer must retain/coalesce the change or report backpressure.
    #[error("subscription change queue is full")]
    Full(Change),
    /// This connection has ended; do not retry against this sender.
    #[error("subscription connection is closed")]
    Closed(Change),
    /// The change targets a disabled catalog or has an invalid resource URI.
    #[error("subscription change targets a disabled catalog or has an invalid resource URI")]
    Invalid(Change),
}

impl ChangeSender {
    /// Enqueues a change, without waiting for a worker or claiming wire delivery.
    ///
    /// # Errors
    /// Returns the unsent change for a disabled catalog, invalid URI, full queue
    /// or closed connection.
    pub fn send(&self, change: Change) -> Result<(), SendChangeError> {
        let invalid = match &change {
            Change::Tools(_) => false,
            Change::Prompts(_) => !self.prompts,
            Change::Resources(_) => !self.resources,
            Change::ResourceUpdated(uri) => {
                !self.resources || crate::resource::validate_uri(uri).is_err()
            }
        };
        if invalid {
            return Err(SendChangeError::Invalid(change));
        }
        self.sender.try_send(change).map_err(|error| match error {
            mpsc::TrySendError::Full(change) => SendChangeError::Full(change),
            mpsc::TrySendError::Disconnected(change) => SendChangeError::Closed(change),
        })?;
        self.dispatcher.unpark();
        Ok(())
    }
}

pub(super) fn channel(options: &crate::SessionOptions) -> (ChangeSender, mpsc::Receiver<Change>) {
    let (sender, receiver) = mpsc::sync_channel(CHANGE_BUFFER);
    (
        ChangeSender {
            sender,
            dispatcher: thread::current(),
            prompts: options.prompts.is_some(),
            resources: options.resources.is_some() || options.skills.is_some(),
        },
        receiver,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn subscriptions_queue_is_bounded_and_returns_unsent_changes() {
        let (sender, receiver) = channel(&crate::SessionOptions::default());
        let catalog = Arc::new(crate::Catalog::new(vec![]).unwrap());
        for _ in 0..CHANGE_BUFFER {
            sender.send(Change::Tools(catalog.clone())).unwrap();
        }
        let Err(SendChangeError::Full(Change::Tools(unsent))) =
            sender.send(Change::Tools(catalog.clone()))
        else {
            panic!("full queue")
        };
        assert!(Arc::ptr_eq(&catalog, &unsent));
        receiver.try_recv().unwrap();
        sender.send(Change::Tools(unsent)).unwrap();
        drop(receiver);
        assert!(matches!(
            sender.send(Change::Tools(catalog)),
            Err(SendChangeError::Closed(_))
        ));
        assert!(matches!(
            sender.send(Change::ResourceUpdated("relative".into())),
            Err(SendChangeError::Invalid(_))
        ));
    }

    #[test]
    fn subscriptions_sender_rejects_disabled_catalogs_before_queueing() {
        let (sender, receiver) = channel(&crate::SessionOptions::default());
        let prompts = Arc::new(crate::PromptCatalog::new(vec![]).unwrap());
        let resources = Arc::new(crate::ResourceCatalog::new(vec![], vec![]).unwrap());
        let Err(SendChangeError::Invalid(Change::Prompts(unsent))) =
            sender.send(Change::Prompts(prompts.clone()))
        else {
            panic!("disabled prompts")
        };
        assert!(Arc::ptr_eq(&prompts, &unsent));
        assert!(matches!(
            sender.send(Change::Resources(resources.clone())),
            Err(SendChangeError::Invalid(_))
        ));
        assert!(matches!(
            sender.send(Change::ResourceUpdated("acme://report".into())),
            Err(SendChangeError::Invalid(_))
        ));
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        let options = crate::SessionOptions {
            prompts: Some(prompts.clone()),
            resources: Some(resources.clone()),
            ..crate::SessionOptions::default()
        };
        let (sender, receiver) = channel(&options);
        sender.send(Change::Prompts(prompts)).unwrap();
        sender.send(Change::Resources(resources)).unwrap();
        sender
            .send(Change::ResourceUpdated("acme://report".into()))
            .unwrap();
        assert_eq!(receiver.try_iter().count(), 3);
    }
}
