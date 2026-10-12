//! Shared operation identity without request authority or serial-number collisions.

use std::sync::Arc;

/// An opaque, single-use identity issued by one component operation.
#[derive(Clone, Debug)]
pub struct Ticket(Arc<()>);
impl Ticket {
    pub(crate) fn new() -> Self {
        Self(Arc::new(()))
    }
}
impl PartialEq for Ticket {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Ticket {}
