//! Local control service composition; endpoint ownership, transport and QA workflows stay separate.
mod endpoint;
mod reads;
mod requests;
mod server;
mod transport;

pub(crate) use endpoint::Endpoint;
pub(super) use server::serve;
pub(crate) use server::serve_with_owner;
pub(crate) use transport::check_owner;
pub(crate) use transport::{read, request, write};

#[cfg(test)]
mod tests;
